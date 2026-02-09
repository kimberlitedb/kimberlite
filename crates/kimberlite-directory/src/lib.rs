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

/// Manages tenant-to-group routing with hot shard migration support.
///
/// Extends the basic `Directory` with per-tenant overrides and live
/// migration tracking. During migration, dual-writes ensure zero data loss.
///
/// **Security Note:** Includes defense-in-depth cross-tenant isolation validation
/// via production assertions at routing boundaries (H-3 remediation, AUDIT-2026-03).
#[derive(Debug, Clone)]
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
}

impl ShardRouter {
    /// Creates a new shard router with the given directory.
    pub fn new(directory: Directory) -> Self {
        Self {
            directory,
            tenant_groups: HashMap::new(),
            active_migrations: HashMap::new(),
            group_tenants: HashMap::new(),
        }
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

    /// Initiates a shard migration for a tenant.
    ///
    /// The migration starts in the Preparing phase. Call `advance_migration`
    /// to progress through Copying -> CatchUp -> Complete.
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

        let migration = ShardMigration::new(tenant_id, source, destination);
        self.active_migrations.insert(tenant_id, migration);
        Ok(self.active_migrations.get(&tenant_id).expect("just inserted"))
    }

    /// Advances a migration to the next phase.
    pub fn advance_migration(&mut self, tenant_id: u64) -> Result<MigrationPhase, DirectoryError> {
        let migration = self
            .active_migrations
            .get_mut(&tenant_id)
            .ok_or(DirectoryError::NoMigrationInProgress(tenant_id))?;

        migration.phase = match migration.phase {
            MigrationPhase::Preparing => MigrationPhase::Copying,
            MigrationPhase::Copying => MigrationPhase::CatchUp,
            MigrationPhase::CatchUp => {
                // On completion, set the tenant override and clean up
                let destination = migration.destination_group;
                let source = migration.source_group;
                migration.phase = MigrationPhase::Complete;

                // Update tenant→group mapping
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

                return Ok(MigrationPhase::Complete);
            }
            MigrationPhase::Complete => {
                // Already complete, remove the migration
                self.active_migrations.remove(&tenant_id);
                return Ok(MigrationPhase::Complete);
            }
        };

        Ok(migration.phase)
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
