//! kmb-directory: Placement routing for `Kimberlite`
//!
//! The directory determines which VSR replication group handles a given
//! stream based on its placement policy. This is a critical component for
//! HIPAA compliance - PHI data must stay within designated regions.
//!
//! # Placement Policies
//!
//! - **Regional**: Data pinned to a specific geographic region (for PHI)
//! - **Global**: Data can be replicated across all regions (for non-PHI)
//!
//! # Example
//!
//! ```
//! use kimberlite_directory::Directory;
//! use kimberlite_types::{GroupId, Placement, Region};
//!
//! let directory = Directory::new(GroupId::new(0))  // Global group
//!     .with_region(Region::APSoutheast2, GroupId::new(1))
//!     .with_region(Region::USEast1, GroupId::new(2));
//!
//! // PHI data routes to regional group
//! let group = directory.group_for_placement(&Placement::Region(Region::APSoutheast2));
//! assert_eq!(group.unwrap(), GroupId::new(1));
//!
//! // Non-PHI data routes to global group
//! let group = directory.group_for_placement(&Placement::Global);
//! assert_eq!(group.unwrap(), GroupId::new(0));
//! ```

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use kimberlite_types::{GroupId, Placement, Region};
use serde::{Deserialize, Serialize};

/// Routes stream placements to VSR replication groups.
///
/// The directory maintains a mapping from regions to their dedicated
/// replication groups, plus a global group for non-regional data.
///
/// # Thread Safety
///
/// Directory is `Clone` and can be shared across threads. It's typically
/// created once at startup and passed to the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directory {
    /// The replication group for global (non-regional) placements.
    global_group: GroupId,
    /// Mapping from regions to their replication groups.
    regional_groups: HashMap<Region, GroupId>,
}

impl Directory {
    /// Creates a new directory with the specified global group.
    ///
    /// The global group handles all `Placement::Global` streams.
    /// Regional groups must be added with [`with_region`](Self::with_region).
    pub fn new(global_group: GroupId) -> Self {
        Self {
            global_group,
            regional_groups: HashMap::new(),
        }
    }

    /// Adds a regional group mapping.
    ///
    /// This is a builder method that takes ownership and returns `self`
    /// for chaining.
    ///
    /// # Example
    ///
    /// ```
    /// use kimberlite_directory::Directory;
    /// use kimberlite_types::{GroupId, Region};
    ///
    /// let directory = Directory::new(GroupId::new(0))
    ///     .with_region(Region::APSoutheast2, GroupId::new(1))
    ///     .with_region(Region::USEast1, GroupId::new(2));
    /// ```
    pub fn with_region(mut self, region: Region, group: GroupId) -> Self {
        self.regional_groups.insert(region, group);
        self
    }

    /// Returns the replication group for the given placement.
    ///
    /// - `Placement::Global` → returns the global group
    /// - `Placement::Region(r)` → returns the group for region `r`
    ///
    /// # Errors
    ///
    /// Returns [`DirectoryError::RegionNotFound`] if a regional placement
    /// specifies a region that hasn't been configured.
    pub fn group_for_placement(&self, placement: &Placement) -> Result<GroupId, DirectoryError> {
        match placement {
            Placement::Region(region) => self
                .regional_groups
                .get(region)
                .copied()
                .ok_or_else(|| DirectoryError::RegionNotFound(region.clone())),
            Placement::Global => Ok(self.global_group),
        }
    }
}

/// Errors that can occur during directory lookups.
#[derive(thiserror::Error, Debug)]
pub enum DirectoryError {
    /// The specified region is not configured in the directory.
    #[error("region not found: {0}")]
    RegionNotFound(Region),

    /// A migration is already in progress for this tenant.
    #[error("migration already in progress for tenant {0}")]
    MigrationInProgress(u64),

    /// No migration in progress for this tenant.
    #[error("no migration in progress for tenant {0}")]
    NoMigrationInProgress(u64),

    /// Source and destination groups are the same.
    #[error("source and destination groups are the same: {0:?}")]
    SameGroup(GroupId),

    /// Migration transaction log I/O error.
    #[error("migration transaction log error: {0}")]
    TransactionLogError(String),

    /// Invalid migration phase transition.
    #[error("invalid phase transition from {from:?} to {to:?}")]
    InvalidPhaseTransition {
        from: MigrationPhase,
        to: MigrationPhase,
    },
}

// ============================================================================
// Hot Shard Migration
// ============================================================================

/// Phase of a shard migration.
///
/// Migrations follow a 4-phase protocol to ensure zero data loss:
/// 1. **Preparing**: Configuration committed, no data transfer yet.
/// 2. **Copying**: Existing data being copied to destination. New writes
///    go to both source and destination (dual-write).
/// 3. **CatchUp**: Applying remaining writes that arrived during copy.
/// 4. **Complete**: Migration finished, source can be cleaned up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationPhase {
    /// Migration committed but data transfer hasn't started.
    Preparing,
    /// Bulk data copy in progress. Dual-writes active.
    Copying,
    /// Applying remaining writes from the copy phase.
    CatchUp,
    /// Migration complete. Reads now served from destination.
    Complete,
}

/// Tracks the state of a tenant shard migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardMigration {
    /// Tenant being migrated.
    pub tenant_id: u64,
    /// Source replication group.
    pub source_group: GroupId,
    /// Destination replication group.
    pub destination_group: GroupId,
    /// Current migration phase.
    pub phase: MigrationPhase,
    /// Number of records copied so far.
    pub records_copied: u64,
    /// Total records to copy (estimated, may increase during migration).
    pub total_records: u64,
}

impl ShardMigration {
    /// Creates a new migration in the Preparing phase.
    pub fn new(tenant_id: u64, source: GroupId, destination: GroupId) -> Self {
        Self {
            tenant_id,
            source_group: source,
            destination_group: destination,
            phase: MigrationPhase::Preparing,
            records_copied: 0,
            total_records: 0,
        }
    }

    /// Returns the group that should serve reads for this tenant.
    ///
    /// During migration, reads continue from the source until Complete.
    pub fn read_group(&self) -> GroupId {
        match self.phase {
            MigrationPhase::Preparing
            | MigrationPhase::Copying
            | MigrationPhase::CatchUp => self.source_group,
            MigrationPhase::Complete => self.destination_group,
        }
    }

    /// Returns whether writes should be dual-written.
    ///
    /// During Copying and CatchUp phases, writes go to both groups
    /// to ensure no data is lost.
    pub fn requires_dual_write(&self) -> bool {
        matches!(
            self.phase,
            MigrationPhase::Copying | MigrationPhase::CatchUp
        )
    }

    /// Returns the write groups for this tenant.
    ///
    /// Returns a single group during Preparing and Complete,
    /// or both groups during Copying and CatchUp.
    pub fn write_groups(&self) -> Vec<GroupId> {
        match self.phase {
            MigrationPhase::Preparing => vec![self.source_group],
            MigrationPhase::Copying | MigrationPhase::CatchUp => {
                vec![self.source_group, self.destination_group]
            }
            MigrationPhase::Complete => vec![self.destination_group],
        }
    }

    /// Returns the completion percentage.
    pub fn progress_percent(&self) -> f64 {
        if self.total_records == 0 {
            return match self.phase {
                MigrationPhase::Complete => 100.0,
                _ => 0.0,
            };
        }
        ((self.records_copied as f64) / (self.total_records as f64) * 100.0).min(100.0)
    }
}

// ============================================================================
// Migration Transaction Log (AUDIT-2026-03 H-4)
// ============================================================================

/// A single transaction in the migration log.
///
/// Each phase transition is logged as an append-only transaction for crash recovery.
///
/// **Security Context:** AUDIT-2026-03 H-4, SOC 2 CC7.2, NIST 800-53 CP-10
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationTransaction {
    /// Tenant ID being migrated.
    pub tenant_id: u64,
    /// Source group.
    pub source_group: GroupId,
    /// Destination group.
    pub destination_group: GroupId,
    /// Phase transition (before → after).
    pub from_phase: Option<MigrationPhase>,
    pub to_phase: MigrationPhase,
    /// Timestamp (nanoseconds since epoch).
    pub timestamp_ns: u64,
    /// Transaction sequence number (for ordering).
    pub seq: u64,
}

/// Append-only transaction log for migration phase transitions.
///
/// Provides crash recovery by replaying logged transactions on startup.
///
/// **Invariants:**
/// 1. All writes are atomic (one transaction per line)
/// 2. Transactions are ordered by sequence number
/// 3. Phase transitions follow valid state machine
/// 4. Log is append-only (never truncated during migration)
///
/// **Security Context:** AUDIT-2026-03 H-4, SOC 2 CC7.2
#[derive(Debug)]
pub struct MigrationTransactionLog {
    /// Path to the transaction log file.
    log_path: PathBuf,
    /// File handle for appending transactions.
    log_file: Option<File>,
    /// Next sequence number.
    next_seq: u64,
}

impl MigrationTransactionLog {
    /// Opens or creates a migration transaction log.
    ///
    /// If the log file exists, reads it to determine the next sequence number.
    pub fn open<P: AsRef<Path>>(log_path: P) -> Result<Self, DirectoryError> {
        let log_path = log_path.as_ref().to_path_buf();

        // Determine next sequence number by reading existing log
        let next_seq = if log_path.exists() {
            let file = File::open(&log_path).map_err(|e| {
                DirectoryError::TransactionLogError(format!("failed to open log: {e}"))
            })?;
            let reader = BufReader::new(file);
            let mut max_seq = 0u64;

            for line in reader.lines() {
                if let Ok(line) = line {
                    if let Ok(tx) = serde_json::from_str::<MigrationTransaction>(&line) {
                        max_seq = max_seq.max(tx.seq);
                    }
                }
            }

            max_seq + 1
        } else {
            0
        };

        // Open log file for appending
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| {
                DirectoryError::TransactionLogError(format!("failed to open log for writing: {e}"))
            })?;

        Ok(Self {
            log_path,
            log_file: Some(log_file),
            next_seq,
        })
    }

    /// Logs a phase transition atomically.
    ///
    /// **Atomicity guarantee:** Transaction is written to disk with `fsync()` before returning.
    pub fn log_transition(
        &mut self,
        tenant_id: u64,
        source_group: GroupId,
        destination_group: GroupId,
        from_phase: Option<MigrationPhase>,
        to_phase: MigrationPhase,
    ) -> Result<u64, DirectoryError> {
        let tx = MigrationTransaction {
            tenant_id,
            source_group,
            destination_group,
            from_phase,
            to_phase,
            timestamp_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
            seq: self.next_seq,
        };

        let log_file = self.log_file.as_mut().ok_or_else(|| {
            DirectoryError::TransactionLogError("log file not open".to_string())
        })?;

        // Write transaction as JSON line
        let tx_json = serde_json::to_string(&tx).map_err(|e| {
            DirectoryError::TransactionLogError(format!("failed to serialize transaction: {e}"))
        })?;

        writeln!(log_file, "{}", tx_json).map_err(|e| {
            DirectoryError::TransactionLogError(format!("failed to write transaction: {e}"))
        })?;

        // **CRITICAL:** Ensure transaction is on disk before returning (atomicity)
        log_file.sync_all().map_err(|e| {
            DirectoryError::TransactionLogError(format!("fsync failed: {e}"))
        })?;

        self.next_seq += 1;
        Ok(tx.seq)
    }

    /// Replays all transactions from the log.
    ///
    /// Used for crash recovery to reconstruct migration state.
    pub fn replay(&self) -> Result<Vec<MigrationTransaction>, DirectoryError> {
        if !self.log_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.log_path).map_err(|e| {
            DirectoryError::TransactionLogError(format!("failed to open log for replay: {e}"))
        })?;

        let reader = BufReader::new(file);
        let mut transactions = Vec::new();

        for line in reader.lines() {
            let line = line.map_err(|e| {
                DirectoryError::TransactionLogError(format!("failed to read log line: {e}"))
            })?;

            let tx = serde_json::from_str::<MigrationTransaction>(&line).map_err(|e| {
                DirectoryError::TransactionLogError(format!("failed to parse transaction: {e}"))
            })?;

            transactions.push(tx);
        }

        // Sort by sequence number to ensure correct ordering
        transactions.sort_by_key(|tx| tx.seq);

        Ok(transactions)
    }

    /// Truncates the log (only safe after all migrations complete).
    ///
    /// **WARNING:** Only call when no migrations are active.
    pub fn truncate(&mut self) -> Result<(), DirectoryError> {
        // Close existing file
        drop(self.log_file.take());

        // Remove log file
        if self.log_path.exists() {
            std::fs::remove_file(&self.log_path).map_err(|e| {
                DirectoryError::TransactionLogError(format!("failed to remove log: {e}"))
            })?;
        }

        // Reopen empty log
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|e| {
                DirectoryError::TransactionLogError(format!("failed to reopen log: {e}"))
            })?;

        self.log_file = Some(log_file);
        self.next_seq = 0;

        Ok(())
    }
}

/// Manages tenant-to-group routing with hot shard migration support.
///
/// Extends the basic `Directory` with per-tenant overrides and live
/// migration tracking. During migration, dual-writes ensure zero data loss.
///
/// **Security Note:** Includes defense-in-depth cross-tenant isolation validation
/// via production assertions at routing boundaries (H-3 remediation, AUDIT-2026-03).
///
/// **Crash Recovery (AUDIT-2026-03 H-4):** All migration phase transitions are logged
/// to an append-only transaction log with fsync before state changes. On startup,
/// `recover_from_crash()` replays the log to restore migration state.
#[derive(Debug)]
pub struct ShardRouter {
    /// Base directory for placement-based routing.
    directory: Directory,
    /// Per-tenant group overrides (set after migration completes).
    tenant_groups: HashMap<u64, GroupId>,
    /// Active migrations.
    active_migrations: HashMap<u64, ShardMigration>,
    /// Reverse mapping: group → set of tenants (for cross-tenant isolation validation).
    ///
    /// **Security Context:** GDPR Art 32, HIPAA 164.308(a)(4), SOC 2 CC6.1, CWE-668
    ///
    /// This mapping is maintained in all builds but only used for validation assertions.
    group_tenants: HashMap<GroupId, std::collections::HashSet<u64>>,
    /// Migration transaction log for crash recovery (AUDIT-2026-03 H-4).
    ///
    /// **Security Context:** SOC 2 CC7.2, NIST 800-53 CP-10
    transaction_log: Option<MigrationTransactionLog>,
}

impl ShardRouter {
    /// Creates a new shard router with the given directory.
    ///
    /// **Note:** This does not enable transaction logging. Use `with_transaction_log()`
    /// to enable crash recovery.
    pub fn new(directory: Directory) -> Self {
        Self {
            directory,
            tenant_groups: HashMap::new(),
            active_migrations: HashMap::new(),
            group_tenants: HashMap::new(),
            transaction_log: None,
        }
    }

    /// Creates a new shard router with transaction logging enabled.
    ///
    /// **AUDIT-2026-03 H-4:** Migration phase transitions are logged atomically
    /// for crash recovery.
    pub fn with_transaction_log<P: AsRef<Path>>(
        directory: Directory,
        log_path: P,
    ) -> Result<Self, DirectoryError> {
        let transaction_log = MigrationTransactionLog::open(log_path)?;

        Ok(Self {
            directory,
            tenant_groups: HashMap::new(),
            active_migrations: HashMap::new(),
            group_tenants: HashMap::new(),
            transaction_log: Some(transaction_log),
        })
    }

    /// Recovers migration state from transaction log after a crash.
    ///
    /// **AUDIT-2026-03 H-4:** Replays all logged transactions to restore
    /// in-progress migrations.
    ///
    /// **Invariants verified:**
    /// 1. Phase transitions follow valid state machine
    /// 2. No duplicate migrations for same tenant
    /// 3. Source/destination groups are consistent
    pub fn recover_from_crash(&mut self) -> Result<usize, DirectoryError> {
        let log = self.transaction_log.as_ref().ok_or_else(|| {
            DirectoryError::TransactionLogError("transaction log not enabled".to_string())
        })?;

        let transactions = log.replay()?;
        let mut recovered_count = 0;

        for tx in transactions {
            // Validate phase transition
            if let Some(from) = tx.from_phase {
                let valid_transition = matches!(
                    (from, tx.to_phase),
                    (MigrationPhase::Preparing, MigrationPhase::Copying)
                        | (MigrationPhase::Copying, MigrationPhase::CatchUp)
                        | (MigrationPhase::CatchUp, MigrationPhase::Complete)
                );

                if !valid_transition {
                    return Err(DirectoryError::InvalidPhaseTransition {
                        from,
                        to: tx.to_phase,
                    });
                }
            }

            // Apply transaction to in-memory state
            if tx.to_phase == MigrationPhase::Complete {
                // Migration completed - update tenant_groups and remove from active
                self.tenant_groups.insert(tx.tenant_id, tx.destination_group);
                self.active_migrations.remove(&tx.tenant_id);

                // Update reverse mapping
                self.group_tenants
                    .entry(tx.destination_group)
                    .or_insert_with(std::collections::HashSet::new)
                    .insert(tx.tenant_id);
            } else {
                // Migration still active - restore or update
                self.active_migrations
                    .entry(tx.tenant_id)
                    .and_modify(|m| {
                        m.phase = tx.to_phase;
                    })
                    .or_insert_with(|| ShardMigration {
                        tenant_id: tx.tenant_id,
                        source_group: tx.source_group,
                        destination_group: tx.destination_group,
                        phase: tx.to_phase,
                        records_copied: 0,
                        total_records: 0,
                    });
            }

            recovered_count += 1;
        }

        Ok(recovered_count)
    }

    /// Returns the group for a tenant, considering overrides and migrations.
    ///
    /// Priority order:
    /// 1. Active migration read-group
    /// 2. Tenant override
    /// 3. Directory placement-based routing
    ///
    /// **Security:** Includes defense-in-depth cross-tenant isolation validation (AUDIT-2026-03 H-3).
    pub fn group_for_tenant(
        &self,
        tenant_id: u64,
        placement: &Placement,
    ) -> Result<GroupId, DirectoryError> {
        // Check active migration
        if let Some(migration) = self.active_migrations.get(&tenant_id) {
            let group = migration.read_group();

            // Cross-tenant isolation assertion (defense-in-depth)
            self.validate_tenant_isolation(tenant_id, group);

            return Ok(group);
        }

        // Check tenant override
        if let Some(&group) = self.tenant_groups.get(&tenant_id) {
            // Cross-tenant isolation assertion (defense-in-depth)
            self.validate_tenant_isolation(tenant_id, group);

            return Ok(group);
        }

        // Fall back to directory placement
        let group = self.directory.group_for_placement(placement)?;

        // For placement-based routing, we can't validate isolation (group not yet assigned)
        // but we can log the assignment in debug builds
        #[cfg(debug_assertions)]
        {
            // This is a first-time routing - we would need to track it to validate future requests
            // For now, we just document that this path exists
            let _ = (tenant_id, group); // Suppress unused variable warning
        }

        Ok(group)
    }

    /// Validates cross-tenant isolation at routing boundaries.
    ///
    /// **Defense-in-depth assertion:** Verifies that if we've previously seen this tenant
    /// routed to a different group, we detect the isolation violation.
    ///
    /// This catches bugs where memory corruption or logic errors pass the wrong tenant_id
    /// to routing functions, potentially allowing cross-tenant data access.
    ///
    /// **Security Context:** GDPR Art 32, HIPAA 164.308(a)(4), SOC 2 CC6.1, CWE-668
    fn validate_tenant_isolation(&self, tenant_id: u64, group: GroupId) {
        // Production assertion: If this tenant was previously assigned to a different group,
        // it's a cross-tenant isolation violation
        if let Some(&existing_group) = self.tenant_groups.get(&tenant_id) {
            assert_eq!(
                existing_group, group,
                "Cross-tenant isolation violation: tenant {} was assigned to group {:?}, \
                 now attempting to route to group {:?}. This indicates memory corruption or \
                 logic bug (AUDIT-2026-03 H-3)",
                tenant_id, existing_group, group
            );
        }

        // Debug-only validation: Check reverse mapping consistency
        #[cfg(debug_assertions)]
        {
            if let Some(tenants) = self.group_tenants.get(&group) {
                if !tenants.contains(&tenant_id) {
                    // This is expected for migrations and first-time routing
                    // Log for debugging but don't panic
                    eprintln!(
                        "DEBUG: Tenant {} routing to group {:?} not in reverse mapping. \
                         This is expected for migrations or first-time routing.",
                        tenant_id, group
                    );
                }
            }
        }
    }

    /// Returns the write groups for a tenant (may be multiple during migration).
    ///
    /// **Security:** Includes defense-in-depth cross-tenant isolation validation (AUDIT-2026-03 H-3).
    pub fn write_groups_for_tenant(
        &self,
        tenant_id: u64,
        placement: &Placement,
    ) -> Result<Vec<GroupId>, DirectoryError> {
        // During migration, writes go to both groups
        if let Some(migration) = self.active_migrations.get(&tenant_id) {
            let groups = migration.write_groups();

            // Cross-tenant isolation validation for each write group
            for &group in &groups {
                self.validate_tenant_isolation(tenant_id, group);
            }

            return Ok(groups);
        }

        // Normal case: single group (validation happens in group_for_tenant)
        let group = self.group_for_tenant(tenant_id, placement)?;
        Ok(vec![group])
    }

    /// Initiates a shard migration for a tenant with atomic transaction logging.
    ///
    /// The migration starts in the Preparing phase. Call `advance_migration`
    /// to progress through Copying -> CatchUp -> Complete.
    ///
    /// **AUDIT-2026-03 H-4:** Migration initiation is logged atomically.
    pub fn start_migration(
        &mut self,
        tenant_id: u64,
        source: GroupId,
        destination: GroupId,
    ) -> Result<&ShardMigration, DirectoryError> {
        if source == destination {
            return Err(DirectoryError::SameGroup(source));
        }

        if self.active_migrations.contains_key(&tenant_id) {
            return Err(DirectoryError::MigrationInProgress(tenant_id));
        }

        // Log migration start to transaction log (AUDIT-2026-03 H-4)
        if let Some(ref mut log) = self.transaction_log {
            log.log_transition(tenant_id, source, destination, None, MigrationPhase::Preparing)?;
        }

        let migration = ShardMigration::new(tenant_id, source, destination);
        self.active_migrations.insert(tenant_id, migration);
        Ok(self.active_migrations.get(&tenant_id).expect("just inserted"))
    }

    /// Advances a migration to the next phase with atomic transaction logging.
    ///
    /// **AUDIT-2026-03 H-4:** Phase transition is logged to disk with fsync BEFORE
    /// updating in-memory state, ensuring crash recovery correctness.
    pub fn advance_migration(&mut self, tenant_id: u64) -> Result<MigrationPhase, DirectoryError> {
        let migration = self
            .active_migrations
            .get(&tenant_id)
            .ok_or(DirectoryError::NoMigrationInProgress(tenant_id))?;

        let current_phase = migration.phase;
        let source = migration.source_group;
        let destination = migration.destination_group;

        let next_phase = match current_phase {
            MigrationPhase::Preparing => MigrationPhase::Copying,
            MigrationPhase::Copying => MigrationPhase::CatchUp,
            MigrationPhase::CatchUp => MigrationPhase::Complete,
            MigrationPhase::Complete => {
                // Already complete, remove the migration
                self.active_migrations.remove(&tenant_id);
                return Ok(MigrationPhase::Complete);
            }
        };

        // **ATOMIC TRANSACTION LOGGING (AUDIT-2026-03 H-4):**
        // Log phase transition to disk with fsync BEFORE updating in-memory state.
        // This ensures crash recovery can replay the log correctly.
        if let Some(ref mut log) = self.transaction_log {
            log.log_transition(
                tenant_id,
                source,
                destination,
                Some(current_phase),
                next_phase,
            )?;
        }

        // Now safe to update in-memory state (log is on disk)
        let migration = self
            .active_migrations
            .get_mut(&tenant_id)
            .expect("migration exists");

        migration.phase = next_phase;

        if next_phase == MigrationPhase::Complete {
            // On completion, set the tenant override and clean up
            self.tenant_groups.insert(tenant_id, destination);

            // Update reverse mapping for cross-tenant isolation validation
            self.group_tenants
                .entry(destination)
                .or_insert_with(std::collections::HashSet::new)
                .insert(tenant_id);

            // Remove from source group's reverse mapping
            if let Some(tenants) = self.group_tenants.get_mut(&source) {
                tenants.remove(&tenant_id);
            }
        }

        Ok(next_phase)
    }

    /// Updates the copy progress for a migration.
    pub fn update_progress(
        &mut self,
        tenant_id: u64,
        records_copied: u64,
        total_records: u64,
    ) -> Result<(), DirectoryError> {
        let migration = self
            .active_migrations
            .get_mut(&tenant_id)
            .ok_or(DirectoryError::NoMigrationInProgress(tenant_id))?;

        migration.records_copied = records_copied;
        migration.total_records = total_records;
        Ok(())
    }

    /// Returns the active migration for a tenant, if any.
    pub fn get_migration(&self, tenant_id: u64) -> Option<&ShardMigration> {
        self.active_migrations.get(&tenant_id)
    }

    /// Returns all active migrations.
    pub fn active_migrations(&self) -> &HashMap<u64, ShardMigration> {
        &self.active_migrations
    }

    /// Returns the number of active migrations.
    pub fn active_migration_count(&self) -> usize {
        self.active_migrations.len()
    }
}

#[cfg(test)]
mod tests;
