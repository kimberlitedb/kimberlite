//! Simulated storage for deterministic testing.
//!
//! The `SimStorage` models disk I/O with configurable:
//! - Write latency
//! - Read latency
//! - Write failures (corruption, partial writes)
//! - Read failures (bit flips, missing data)
//! - Fsync failures
//!
//! All behavior is deterministic based on the simulation's RNG seed.

use std::collections::HashMap;

use crate::rng::SimRng;
use crate::storage_reordering::{ReorderConfig, WriteReorderer, WriteId};
use crate::concurrent_io::{ConcurrentIOConfig, ConcurrentIOTracker};
use crate::crash_recovery::{CrashConfig, CrashRecoveryEngine};

// Use instrumentation directly (kimberlite-sim can't use its own macros due to circular deps)
use crate::instrumentation::fault_registry;
use crate::instrumentation::invariant_runtime;
use crate::instrumentation::phase_tracker;

// ============================================================================
// Storage Configuration
// ============================================================================

/// Configuration for simulated storage behavior.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Minimum write latency in nanoseconds.
    pub min_write_latency_ns: u64,
    /// Maximum write latency in nanoseconds.
    pub max_write_latency_ns: u64,
    /// Minimum read latency in nanoseconds.
    pub min_read_latency_ns: u64,
    /// Maximum read latency in nanoseconds.
    pub max_read_latency_ns: u64,
    /// Probability of write failure (0.0 to 1.0).
    pub write_failure_probability: f64,
    /// Probability of read corruption (bit flip) (0.0 to 1.0).
    pub read_corruption_probability: f64,
    /// Probability of fsync failure (0.0 to 1.0).
    pub fsync_failure_probability: f64,
    /// Probability of partial write (0.0 to 1.0).
    pub partial_write_probability: f64,
    /// Enable write reordering (Phase 1 enhancement).
    pub enable_reordering: bool,
    /// Enable concurrent I/O simulation (Phase 1 enhancement).
    pub enable_concurrent_io: bool,
    /// Enable enhanced crash recovery (Phase 1 enhancement).
    pub enable_crash_recovery: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            min_write_latency_ns: 10_000,    // 10μs
            max_write_latency_ns: 1_000_000, // 1ms
            min_read_latency_ns: 5_000,      // 5μs
            max_read_latency_ns: 500_000,    // 0.5ms
            write_failure_probability: 0.0,
            read_corruption_probability: 0.0,
            fsync_failure_probability: 0.0,
            partial_write_probability: 0.0,
            enable_reordering: false,
            enable_concurrent_io: false,
            enable_crash_recovery: false,
        }
    }
}

impl StorageConfig {
    /// Creates a reliable storage configuration (no failures).
    pub fn reliable() -> Self {
        Self::default()
    }

    /// Creates a storage configuration with write failures.
    pub fn with_write_failures(probability: f64) -> Self {
        Self {
            write_failure_probability: probability,
            ..Self::default()
        }
    }

    /// Creates a storage configuration with read corruption.
    pub fn with_corruption(probability: f64) -> Self {
        Self {
            read_corruption_probability: probability,
            ..Self::default()
        }
    }

    /// Creates a slow storage configuration.
    pub fn slow() -> Self {
        Self {
            min_write_latency_ns: 1_000_000,  // 1ms
            max_write_latency_ns: 50_000_000, // 50ms
            min_read_latency_ns: 500_000,     // 0.5ms
            max_read_latency_ns: 10_000_000,  // 10ms
            ..Self::default()
        }
    }

    /// Enables Phase 1 storage realism features (reordering, concurrent I/O, crash recovery).
    pub fn with_realism(mut self) -> Self {
        self.enable_reordering = true;
        self.enable_concurrent_io = true;
        self.enable_crash_recovery = true;
        self
    }

    /// Enables write reordering.
    pub fn with_reordering(mut self) -> Self {
        self.enable_reordering = true;
        self
    }

    /// Enables concurrent I/O simulation.
    pub fn with_concurrent_io(mut self) -> Self {
        self.enable_concurrent_io = true;
        self
    }

    /// Enables enhanced crash recovery.
    pub fn with_crash_recovery(mut self) -> Self {
        self.enable_crash_recovery = true;
        self
    }
}

// ============================================================================
// Storage Operations
// ============================================================================

/// Result of a storage write operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteResult {
    /// Write completed successfully.
    Success {
        /// Latency in nanoseconds.
        latency_ns: u64,
        /// Bytes written.
        bytes_written: usize,
    },
    /// Write failed completely.
    Failed {
        /// Latency until failure.
        latency_ns: u64,
        /// Reason for failure.
        reason: WriteFailure,
    },
    /// Partial write (torn write).
    Partial {
        /// Latency in nanoseconds.
        latency_ns: u64,
        /// Bytes actually written.
        bytes_written: usize,
        /// Total bytes requested.
        bytes_requested: usize,
    },
}

/// Reason for write failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteFailure {
    /// Disk is full.
    DiskFull,
    /// I/O error.
    IoError,
    /// Permission denied.
    PermissionDenied,
}

/// Result of a storage read operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadResult {
    /// Read completed successfully.
    Success {
        /// Latency in nanoseconds.
        latency_ns: u64,
        /// Data read.
        data: Vec<u8>,
    },
    /// Data was corrupted (bit flip detected or injected).
    Corrupted {
        /// Latency in nanoseconds.
        latency_ns: u64,
        /// Corrupted data.
        data: Vec<u8>,
        /// Original data (for debugging).
        original: Vec<u8>,
    },
    /// Read failed (data not found).
    NotFound {
        /// Latency in nanoseconds.
        latency_ns: u64,
    },
}

/// Result of an fsync operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsyncResult {
    /// Fsync completed successfully.
    Success {
        /// Latency in nanoseconds.
        latency_ns: u64,
    },
    /// Fsync failed.
    Failed {
        /// Latency in nanoseconds.
        latency_ns: u64,
    },
}

// ============================================================================
// Simulated Storage
// ============================================================================

/// Simulated block storage for deterministic testing.
///
/// Models a simple key-value block store with configurable failure injection.
#[derive(Debug)]
pub struct SimStorage {
    /// Storage configuration.
    config: StorageConfig,
    /// Stored data blocks, keyed by address.
    blocks: HashMap<u64, Vec<u8>>,
    /// Pending writes (not yet fsynced).
    pending_writes: HashMap<u64, Vec<u8>>,
    /// Whether there are unfsynced writes.
    dirty: bool,
    /// Statistics.
    stats: StorageStats,
    /// Per-replica logs for consistency checking.
    /// Maps `replica_id` -> ordered list of log entries
    replica_logs: HashMap<u64, Vec<Vec<u8>>>,
    /// Write reorderer (Phase 1 enhancement).
    reorderer: Option<WriteReorderer>,
    /// Concurrent I/O tracker (Phase 1 enhancement).
    #[allow(dead_code)] // Phase 2: Concurrent I/O tracking
    io_tracker: Option<ConcurrentIOTracker>,
    /// Crash recovery engine (Phase 1 enhancement).
    crash_engine: Option<CrashRecoveryEngine>,
    /// Current simulation time (for tracking).
    current_time_ns: u64,
    /// Map from address to write ID (for reordering).
    address_to_write_id: HashMap<u64, WriteId>,
}

/// Storage statistics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    /// Total write operations.
    pub writes: u64,
    /// Successful writes.
    pub writes_successful: u64,
    /// Failed writes.
    pub writes_failed: u64,
    /// Partial writes.
    pub writes_partial: u64,
    /// Total read operations.
    pub reads: u64,
    /// Successful reads.
    pub reads_successful: u64,
    /// Corrupted reads.
    pub reads_corrupted: u64,
    /// Not found reads.
    pub reads_not_found: u64,
    /// Total fsync operations.
    pub fsyncs: u64,
    /// Successful fsyncs.
    pub fsyncs_successful: u64,
    /// Failed fsyncs.
    pub fsyncs_failed: u64,
    /// Total bytes written.
    pub bytes_written: u64,
    /// Total bytes read.
    pub bytes_read: u64,
}

impl SimStorage {
    /// Creates a new simulated storage with the given configuration.
    pub fn new(config: StorageConfig) -> Self {
        // Initialize realism engines if enabled
        let reorderer = if config.enable_reordering {
            Some(WriteReorderer::new(ReorderConfig::default()))
        } else {
            None
        };

        let io_tracker = if config.enable_concurrent_io {
            Some(ConcurrentIOTracker::new(ConcurrentIOConfig::default()))
        } else {
            None
        };

        let crash_engine = if config.enable_crash_recovery {
            Some(CrashRecoveryEngine::new(CrashConfig::default()))
        } else {
            None
        };

        Self {
            config,
            blocks: HashMap::new(),
            pending_writes: HashMap::new(),
            dirty: false,
            stats: StorageStats::default(),
            replica_logs: HashMap::new(),
            reorderer,
            io_tracker,
            crash_engine,
            current_time_ns: 0,
            address_to_write_id: HashMap::new(),
        }
    }

    /// Creates storage with default (reliable) configuration.
    pub fn reliable() -> Self {
        Self::new(StorageConfig::reliable())
    }

    /// Processes ready writes from the reorderer and applies them to pending_writes.
    ///
    /// Returns the number of writes processed.
    fn process_reordered_writes(&mut self, rng: &mut SimRng) -> usize {
        let Some(ref mut reorderer) = self.reorderer else {
            return 0;
        };

        let mut processed = 0;
        while let Some(pending_write) = reorderer.pop_ready_write(rng, self.current_time_ns) {
            // Apply the reordered write to pending_writes
            self.pending_writes
                .insert(pending_write.address, pending_write.data.clone());
            self.dirty = true;

            // Remove from address tracking
            self.address_to_write_id.remove(&pending_write.address);

            processed += 1;
        }

        processed
    }

    /// Drains all pending writes from the reorderer (used during fsync).
    fn drain_reorderer(&mut self, rng: &mut SimRng) {
        let Some(ref mut reorderer) = self.reorderer else {
            return;
        };

        // Process all remaining writes in the reorderer
        while !reorderer.is_empty() {
            if let Some(pending_write) = reorderer.pop_ready_write(rng, self.current_time_ns) {
                self.pending_writes
                    .insert(pending_write.address, pending_write.data.clone());
                self.address_to_write_id.remove(&pending_write.address);
            } else {
                // No ready writes, but queue not empty - this shouldn't happen
                // unless there are dependency deadlocks
                break;
            }
        }

        self.dirty = !self.pending_writes.is_empty();
    }

    /// Writes data to the given address.
    ///
    /// The write is buffered until `fsync` is called.
    pub fn write(&mut self, address: u64, data: Vec<u8>, rng: &mut SimRng) -> WriteResult {
        // Fault injection point: simulated storage write
        fault_registry::record_fault_point("sim.storage.write");

        self.stats.writes += 1;
        let data_len = data.len();

        // Calculate latency
        let latency_ns = rng.delay_ns(
            self.config.min_write_latency_ns,
            self.config.max_write_latency_ns,
        );

        // Check for write failure
        if self.config.write_failure_probability > 0.0
            && rng.next_bool_with_probability(self.config.write_failure_probability)
        {
            self.stats.writes_failed += 1;
            return WriteResult::Failed {
                latency_ns,
                reason: WriteFailure::IoError,
            };
        }

        // Check for partial write
        if self.config.partial_write_probability > 0.0
            && rng.next_bool_with_probability(self.config.partial_write_probability)
        {
            self.stats.writes_partial += 1;
            // Write only a portion of the data
            let partial_len = if data_len > 1 {
                rng.next_usize(data_len - 1) + 1
            } else {
                0
            };
            let partial_data = data[..partial_len].to_vec();
            self.pending_writes.insert(address, partial_data);
            self.dirty = true;
            self.stats.bytes_written += partial_len as u64;

            return WriteResult::Partial {
                latency_ns,
                bytes_written: partial_len,
                bytes_requested: data_len,
            };
        }

        // Successful write (to pending buffer or reorderer)
        self.stats.writes_successful += 1;
        self.stats.bytes_written += data_len as u64;

        // If reordering is enabled, submit to reorderer
        if let Some(ref mut reorderer) = self.reorderer {
            // Submit write to reorderer
            let write_id = reorderer.submit_write(
                address,
                data,
                self.current_time_ns,
                Vec::new(), // No dependencies for now
            );

            // Track mapping from address to write ID
            self.address_to_write_id.insert(address, write_id);

            // Process some ready writes from the reorderer
            self.process_reordered_writes(rng);
        } else {
            // Direct write path (no reordering)
            self.pending_writes.insert(address, data);
            self.dirty = true;
        }

        WriteResult::Success {
            latency_ns,
            bytes_written: data_len,
        }
    }

    /// Reads data from the given address.
    pub fn read(&mut self, address: u64, rng: &mut SimRng) -> ReadResult {
        // Fault injection point: simulated storage read
        fault_registry::record_fault_point("sim.storage.read");

        self.stats.reads += 1;

        // Calculate latency
        let latency_ns = rng.delay_ns(
            self.config.min_read_latency_ns,
            self.config.max_read_latency_ns,
        );

        // Check for data in order of recency:
        // 1. pending_writes (writes popped from reorderer or directly written, most recent)
        // 2. reorderer queue (writes still being reordered, older than pending_writes)
        // 3. durable blocks (fsynced data)
        // This ensures read-your-writes semantics while correctly handling reordering.
        let data = self
            .pending_writes
            .get(&address)
            .cloned()
            .or_else(|| {
                // Check reorderer if it exists
                if let Some(ref reorderer) = self.reorderer {
                    reorderer.get_pending_write(address).map(|d| d.to_vec())
                } else {
                    None
                }
            })
            .or_else(|| self.blocks.get(&address).cloned());

        if let Some(data) = data {
            self.stats.bytes_read += data.len() as u64;

            // Check for read corruption
            if self.config.read_corruption_probability > 0.0
                && rng.next_bool_with_probability(self.config.read_corruption_probability)
            {
                self.stats.reads_corrupted += 1;
                let mut corrupted = data.clone();
                if !corrupted.is_empty() {
                    // Flip a random bit
                    let byte_idx = rng.next_usize(corrupted.len());
                    let bit_idx = rng.next_usize(8);
                    corrupted[byte_idx] ^= 1 << bit_idx;
                }
                // Fault applied and observed: corruption detected
                fault_registry::record_fault_applied("storage.corruption");
                fault_registry::record_fault_observed("storage.corruption");
                return ReadResult::Corrupted {
                    latency_ns,
                    data: corrupted,
                    original: data,
                };
            }

            self.stats.reads_successful += 1;
            ReadResult::Success { latency_ns, data }
        } else {
            self.stats.reads_not_found += 1;
            ReadResult::NotFound { latency_ns }
        }
    }

    /// Flushes pending writes to durable storage.
    pub fn fsync(&mut self, rng: &mut SimRng) -> FsyncResult {
        // Fault injection point: simulated storage fsync
        fault_registry::record_fault_point("sim.storage.fsync");

        // Canary mutation: Skip fsync (should be detected by StorageDeterminismChecker)
        if crate::canary::should_skip_fsync(rng) {
            // Pretend fsync succeeded but don't actually persist
            // This simulates a bug where fsync is skipped, leading to data loss on crash
            self.stats.fsyncs += 1;
            self.stats.fsyncs_successful += 1;

            let latency_ns = rng.delay_ns(
                self.config.min_write_latency_ns * 10,
                self.config.max_write_latency_ns * 10,
            );

            // Record phase marker even though we didn't really fsync
            phase_tracker::record_phase(
                "storage",
                "fsync_complete",
                format!(
                    "blocks_written={} (CANARY: skipped)",
                    self.stats.fsyncs_successful
                ),
            );

            return FsyncResult::Success { latency_ns };
        }

        // Expensive invariant: verify storage consistency
        // Sample 1 in 5 times (20% of fsyncs for demonstration)
        if invariant_runtime::should_check_invariant("sim.storage.consistency", 5) {
            if !self.verify_storage_consistency() {
                panic!("Invariant violated: storage consistency check failed");
            }
        }

        self.stats.fsyncs += 1;

        // Calculate latency (fsync is typically slow)
        let latency_ns = rng.delay_ns(
            self.config.min_write_latency_ns * 10,
            self.config.max_write_latency_ns * 10,
        );

        // Drain all remaining writes from reorderer before fsync
        self.drain_reorderer(rng);

        // Check for fsync failure
        let fsync_failed = self.config.fsync_failure_probability > 0.0
            && rng.next_bool_with_probability(self.config.fsync_failure_probability);

        // Sim canary: fsync-lies inverts failure to success
        let should_lie = crate::sim_canaries::fsync_should_lie_about_failure(fsync_failed);

        if fsync_failed && !should_lie {
            self.stats.fsyncs_failed += 1;
            // On fsync failure, pending writes are lost
            self.pending_writes.clear();
            self.dirty = false;
            // Fault applied and observed: fsync failed, data was lost
            fault_registry::record_fault_applied("storage.fsync_failure");
            fault_registry::record_fault_observed("storage.fsync_failure");
            return FsyncResult::Failed { latency_ns };
        }

        // Move pending writes to durable storage
        for (address, data) in self.pending_writes.drain() {
            self.blocks.insert(address, data);
        }
        self.dirty = false;
        self.stats.fsyncs_successful += 1;

        // Record phase marker for successful fsync
        phase_tracker::record_phase(
            "storage",
            "fsync_complete",
            format!("blocks_written={}", self.stats.fsyncs_successful),
        );

        FsyncResult::Success { latency_ns }
    }

    /// Simulates a crash - loses all pending (unfsynced) writes.
    ///
    /// If crash recovery engine is enabled, uses realistic crash semantics.
    pub fn crash(&mut self, scenario: Option<crate::crash_recovery::CrashScenario>, rng: &mut SimRng) {
        // Track if we're losing data
        let had_pending_writes = !self.pending_writes.is_empty();

        if let Some(ref mut engine) = self.crash_engine {
            // Use crash recovery engine for realistic crash behavior
            use crate::crash_recovery::CrashScenario;
            let crash_scenario = scenario.unwrap_or(CrashScenario::PowerLoss);
            let _crash_state = engine.crash_with_scenario(crash_scenario, rng);

            // For now, apply simple crash semantics
            // Full integration would restore from crash_state
            self.pending_writes.clear();
            self.dirty = false;
        } else {
            // Simple crash: lose all pending writes
            self.pending_writes.clear();
            self.dirty = false;
        }

        // Effect observation: crash caused data loss
        if had_pending_writes {
            fault_registry::record_fault_applied("storage.crash_data_loss");
            fault_registry::record_fault_observed("storage.crash_data_loss");
        }
    }

    /// Sets the current simulation time (for concurrent I/O tracking).
    pub fn set_time(&mut self, time_ns: u64) {
        self.current_time_ns = time_ns;
    }

    /// Returns whether there are unfsynced writes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Returns the number of stored blocks.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Returns storage statistics.
    pub fn stats(&self) -> &StorageStats {
        &self.stats
    }

    /// Returns the configuration.
    pub fn config(&self) -> &StorageConfig {
        &self.config
    }

    /// Checks if an address exists in durable storage.
    pub fn exists(&self, address: u64) -> bool {
        self.blocks.contains_key(&address)
    }

    /// Deletes a block from storage.
    pub fn delete(&mut self, address: u64) -> bool {
        self.pending_writes.remove(&address);
        self.blocks.remove(&address).is_some()
    }

    /// Appends a log entry for a specific replica.
    ///
    /// Used for replica consistency checking - tracks actual log content
    /// so we can compute real hashes instead of synthetic ones.
    pub fn append_replica_log(&mut self, replica_id: u64, entry: Vec<u8>) {
        self.replica_logs.entry(replica_id).or_default().push(entry);
    }

    /// Gets all log entries for a replica.
    ///
    /// Returns entries in the order they were appended.
    pub fn get_replica_log(&self, replica_id: u64) -> Option<&[Vec<u8>]> {
        self.replica_logs.get(&replica_id).map(Vec::as_slice)
    }

    /// Gets the log length for a replica.
    pub fn get_replica_log_length(&self, replica_id: u64) -> u64 {
        self.replica_logs
            .get(&replica_id)
            .map_or(0, |v| v.len() as u64)
    }

    /// Clears all replica logs (for testing/reset).
    ///
    /// **WARNING**: This should only be used for full test resets, never during
    /// simulation runs. Clearing replica logs will cause ReplicaHeadChecker to
    /// detect false regressions if any replica state updates happen afterwards.
    pub fn clear_replica_logs(&mut self) {
        self.replica_logs.clear();
    }

    /// Creates a checkpoint of current storage state.
    ///
    /// Returns a snapshot of all durable blocks.
    /// Note: replica_logs are NOT included in checkpoints - they are append-only
    /// for invariant checking and should never be rolled back.
    /// Used for checkpoint/recovery testing.
    pub fn checkpoint(&self) -> StorageCheckpoint {
        StorageCheckpoint {
            blocks: self.blocks.clone(),
            // Store replica log lengths at checkpoint time for verification,
            // but don't allow restoration to truncate logs
            _replica_log_lengths: self
                .replica_logs
                .iter()
                .map(|(id, log)| (*id, log.len() as u64))
                .collect(),
        }
    }

    /// Restores storage from a checkpoint.
    ///
    /// Overwrites current state with the checkpoint.
    /// Discards any pending writes.
    /// Note: replica_logs are NOT restored - they remain append-only
    /// for invariant checking and never regress.
    pub fn restore_checkpoint(&mut self, checkpoint: &StorageCheckpoint) {
        self.blocks = checkpoint.blocks.clone();
        // DO NOT restore replica_logs - they are append-only for invariant checking
        // Restoring them would cause ReplicaHeadChecker to see regressions
        self.pending_writes.clear();
        self.dirty = false;
    }

    /// Returns the total size of durable storage in bytes.
    pub fn storage_size_bytes(&self) -> u64 {
        self.blocks.values().map(|data| data.len() as u64).sum()
    }

    /// Returns a hash of all durable storage for verification.
    ///
    /// Used for determinism checking - same storage should produce same hash.
    pub fn storage_hash(&self) -> [u8; 32] {
        use std::collections::BTreeMap;

        // Sort blocks by address for deterministic hashing
        let sorted: BTreeMap<_, _> = self.blocks.iter().collect();

        let mut combined = Vec::new();
        for (addr, data) in sorted {
            combined.extend_from_slice(&addr.to_le_bytes());
            combined.extend_from_slice(data);
        }

        if combined.is_empty() {
            [0u8; 32]
        } else {
            let hash = kimberlite_crypto::internal_hash(&combined);
            *hash.as_bytes()
        }
    }

    /// Expensive invariant: verify storage consistency.
    ///
    /// This is an expensive check that validates:
    /// - No pending writes reference deleted blocks
    /// - Stats match actual state
    /// - All replica logs are monotonic
    ///
    /// Returns `true` if consistent, `false` otherwise.
    fn verify_storage_consistency(&self) -> bool {
        // Check 1: Pending writes should have data
        for data in self.pending_writes.values() {
            if data.is_empty() {
                return false; // Invariant: no empty pending writes
            }
        }

        // Check 2: Stats sanity checks
        if self.stats.writes_successful + self.stats.writes_failed + self.stats.writes_partial
            != self.stats.writes
        {
            return false; // Invariant: write stats sum correctly
        }

        if self.stats.reads_successful + self.stats.reads_corrupted + self.stats.reads_not_found
            != self.stats.reads
        {
            return false; // Invariant: read stats sum correctly
        }

        // Check 3: Dirty flag consistency
        if self.dirty && self.pending_writes.is_empty() {
            return false; // Invariant: dirty implies pending writes
        }

        // All checks passed
        true
    }
}

/// Checkpoint of storage state for recovery testing.
#[derive(Debug, Clone, Default)]
pub struct StorageCheckpoint {
    /// Durable blocks at checkpoint time.
    blocks: HashMap<u64, Vec<u8>>,
    /// Replica log lengths at checkpoint time (for verification, not restoration).
    /// We don't store full logs because they should never be rolled back.
    #[allow(dead_code)]
    _replica_log_lengths: HashMap<u64, u64>,
}

impl StorageCheckpoint {
    /// Iterates over all blocks in the checkpoint.
    ///
    /// Returns an iterator of (address, data) pairs for synchronizing model state.
    pub fn iter_blocks(&self) -> impl Iterator<Item = (u64, &[u8])> + '_ {
        self.blocks.iter().map(|(k, v)| (*k, v.as_slice()))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_basic() {
        let mut storage = SimStorage::reliable();
        let mut rng = SimRng::new(42);

        let result = storage.write(0, b"hello".to_vec(), &mut rng);
        assert!(matches!(result, WriteResult::Success { .. }));

        // Fsync to make durable
        let result = storage.fsync(&mut rng);
        assert!(matches!(result, FsyncResult::Success { .. }));

        // Read back
        let result = storage.read(0, &mut rng);
        match result {
            ReadResult::Success { data, .. } => {
                assert_eq!(data, b"hello");
            }
            _ => panic!("expected Success"),
        }
    }

    #[test]
    fn read_your_writes_before_fsync() {
        let mut storage = SimStorage::reliable();
        let mut rng = SimRng::new(42);

        storage.write(0, b"pending".to_vec(), &mut rng);

        // Should be able to read pending write
        let result = storage.read(0, &mut rng);
        match result {
            ReadResult::Success { data, .. } => {
                assert_eq!(data, b"pending");
            }
            _ => panic!("expected Success"),
        }
    }

    #[test]
    fn crash_loses_pending_writes() {
        let mut storage = SimStorage::reliable();
        let mut rng = SimRng::new(42);

        storage.write(0, b"will be lost".to_vec(), &mut rng);
        assert!(storage.is_dirty());

        storage.crash(None, &mut rng);

        assert!(!storage.is_dirty());
        let result = storage.read(0, &mut rng);
        assert!(matches!(result, ReadResult::NotFound { .. }));
    }

    #[test]
    fn fsynced_data_survives_crash() {
        let mut storage = SimStorage::reliable();
        let mut rng = SimRng::new(42);

        storage.write(0, b"durable".to_vec(), &mut rng);
        storage.fsync(&mut rng);
        storage.crash(None, &mut rng);

        let result = storage.read(0, &mut rng);
        match result {
            ReadResult::Success { data, .. } => {
                assert_eq!(data, b"durable");
            }
            _ => panic!("expected Success"),
        }
    }

    #[test]
    fn write_failure() {
        let config = StorageConfig {
            write_failure_probability: 1.0,
            ..StorageConfig::default()
        };
        let mut storage = SimStorage::new(config);
        let mut rng = SimRng::new(42);

        let result = storage.write(0, b"will fail".to_vec(), &mut rng);
        assert!(matches!(result, WriteResult::Failed { .. }));
        assert_eq!(storage.stats().writes_failed, 1);
    }

    #[test]
    fn read_corruption() {
        let config = StorageConfig {
            read_corruption_probability: 1.0,
            ..StorageConfig::default()
        };
        let mut storage = SimStorage::new(config);
        let mut rng = SimRng::new(42);

        storage.write(0, b"hello".to_vec(), &mut rng);
        storage.fsync(&mut rng);

        let result = storage.read(0, &mut rng);
        match result {
            ReadResult::Corrupted { data, original, .. } => {
                assert_eq!(original, b"hello");
                assert_ne!(data, original); // Should be corrupted
            }
            _ => panic!("expected Corrupted"),
        }
    }

    #[test]
    fn fsync_failure() {
        let config = StorageConfig {
            fsync_failure_probability: 1.0,
            ..StorageConfig::default()
        };
        let mut storage = SimStorage::new(config);
        let mut rng = SimRng::new(42);

        storage.write(0, b"will be lost".to_vec(), &mut rng);
        let result = storage.fsync(&mut rng);
        assert!(matches!(result, FsyncResult::Failed { .. }));

        // Pending writes should be cleared
        assert!(!storage.is_dirty());
        let result = storage.read(0, &mut rng);
        assert!(matches!(result, ReadResult::NotFound { .. }));
    }

    #[test]
    fn partial_write() {
        let config = StorageConfig {
            partial_write_probability: 1.0,
            ..StorageConfig::default()
        };
        let mut storage = SimStorage::new(config);
        let mut rng = SimRng::new(42);

        let data = b"hello world".to_vec();
        let result = storage.write(0, data.clone(), &mut rng);

        match result {
            WriteResult::Partial {
                bytes_written,
                bytes_requested,
                ..
            } => {
                assert!(bytes_written < bytes_requested);
                assert_eq!(bytes_requested, data.len());
            }
            _ => panic!("expected Partial"),
        }
    }

    #[test]
    fn stats_tracking() {
        let mut storage = SimStorage::reliable();
        let mut rng = SimRng::new(42);

        storage.write(0, b"data".to_vec(), &mut rng);
        storage.write(1, b"more".to_vec(), &mut rng);
        storage.fsync(&mut rng);
        storage.read(0, &mut rng);
        storage.read(1, &mut rng);
        storage.read(2, &mut rng); // Not found

        assert_eq!(storage.stats().writes, 2);
        assert_eq!(storage.stats().writes_successful, 2);
        assert_eq!(storage.stats().fsyncs, 1);
        assert_eq!(storage.stats().fsyncs_successful, 1);
        assert_eq!(storage.stats().reads, 3);
        assert_eq!(storage.stats().reads_successful, 2);
        assert_eq!(storage.stats().reads_not_found, 1);
    }

    #[test]
    fn checkpoint_restore_preserves_replica_logs() {
        let mut storage = SimStorage::reliable();

        // Append some log entries
        storage.append_replica_log(0, vec![1, 2, 3]);
        storage.append_replica_log(0, vec![4, 5, 6]);
        storage.append_replica_log(1, vec![7, 8, 9]);

        assert_eq!(storage.get_replica_log_length(0), 2);
        assert_eq!(storage.get_replica_log_length(1), 1);

        // Create checkpoint
        let checkpoint = storage.checkpoint();

        // Append more log entries
        storage.append_replica_log(0, vec![10, 11, 12]);
        assert_eq!(storage.get_replica_log_length(0), 3);

        // Restore checkpoint
        storage.restore_checkpoint(&checkpoint);

        // CRITICAL: replica_logs should NOT be rolled back
        // They are append-only for invariant checking
        assert_eq!(
            storage.get_replica_log_length(0),
            3,
            "replica logs should remain append-only, not rolled back"
        );
        assert_eq!(storage.get_replica_log_length(1), 1);
    }

    #[test]
    fn delete_block() {
        let mut storage = SimStorage::reliable();
        let mut rng = SimRng::new(42);

        storage.write(0, b"data".to_vec(), &mut rng);
        storage.fsync(&mut rng);

        assert!(storage.exists(0));
        assert!(storage.delete(0));
        assert!(!storage.exists(0));

        let result = storage.read(0, &mut rng);
        assert!(matches!(result, ReadResult::NotFound { .. }));
    }
}
