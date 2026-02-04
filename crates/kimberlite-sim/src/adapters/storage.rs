//! Storage adapter trait for simulation vs production I/O.
//!
//! This module provides a trait-based abstraction for storage I/O:
//! - **Deterministic simulation**: Use `SimStorage` with failures, latency, crashes
//! - **Production use**: Could use `FileStorage` or other persistent backend
//!
//! # Performance
//!
//! The `Storage` trait is on the cold path (I/O), so trait objects are acceptable.
//! Methods do NOT need `#[inline]` as they involve disk I/O simulation.

// Re-export types from parent module
pub use crate::storage::{
    FsyncResult, ReadResult, SimStorage, StorageCheckpoint, StorageConfig, StorageStats,
    WriteFailure, WriteResult,
};
pub use crate::crash_recovery::CrashScenario;
pub use crate::rng::SimRng;

/// Trait for storage I/O (simulation or production).
///
/// Implementations handle block-level read/write operations with optional
/// failures, latency, and crash semantics.
pub trait Storage {
    /// Writes data to the specified address.
    ///
    /// # Arguments
    ///
    /// * `address` - Block address to write to
    /// * `data` - Data to write
    /// * `rng` - Random number generator (for latency, failures)
    ///
    /// # Returns
    ///
    /// - `WriteResult::Success` if write completed
    /// - `WriteResult::Failed` if write failed
    /// - `WriteResult::Partial` if partial/torn write occurred
    fn write(&mut self, address: u64, data: Vec<u8>, rng: &mut SimRng) -> WriteResult;

    /// Reads data from the specified address.
    ///
    /// # Arguments
    ///
    /// * `address` - Block address to read from
    /// * `rng` - Random number generator (for latency, corruption)
    ///
    /// # Returns
    ///
    /// - `ReadResult::Success` with data if read completed
    /// - `ReadResult::Corrupted` if data was corrupted
    /// - `ReadResult::NotFound` if address has no data
    fn read(&mut self, address: u64, rng: &mut SimRng) -> ReadResult;

    /// Syncs all pending writes to durable storage.
    ///
    /// # Arguments
    ///
    /// * `rng` - Random number generator (for latency, failures)
    ///
    /// # Returns
    ///
    /// - `FsyncResult::Success` if fsync completed
    /// - `FsyncResult::Failed` if fsync failed
    fn fsync(&mut self, rng: &mut SimRng) -> FsyncResult;

    /// Simulates a crash - loses all pending (unfsynced) writes.
    ///
    /// # Arguments
    ///
    /// * `scenario` - Optional crash scenario for realistic crash behavior
    /// * `rng` - Random number generator (for torn writes, etc.)
    fn crash(&mut self, scenario: Option<CrashScenario>, rng: &mut SimRng);

    /// Creates a checkpoint of current durable state.
    ///
    /// Used for recovery testing and verification.
    fn checkpoint(&self) -> StorageCheckpoint;

    /// Restores storage from a checkpoint.
    ///
    /// Used for recovery testing.
    fn restore(&mut self, checkpoint: &StorageCheckpoint);

    /// Returns storage statistics (for monitoring and debugging).
    fn stats(&self) -> StorageStats;
}

// ============================================================================
// Simulation Implementation
// ============================================================================

impl Storage for SimStorage {
    fn write(&mut self, address: u64, data: Vec<u8>, rng: &mut SimRng) -> WriteResult {
        SimStorage::write(self, address, data, rng)
    }

    fn read(&mut self, address: u64, rng: &mut SimRng) -> ReadResult {
        SimStorage::read(self, address, rng)
    }

    fn fsync(&mut self, rng: &mut SimRng) -> FsyncResult {
        SimStorage::fsync(self, rng)
    }

    fn crash(&mut self, scenario: Option<CrashScenario>, rng: &mut SimRng) {
        SimStorage::crash(self, scenario, rng);
    }

    fn checkpoint(&self) -> StorageCheckpoint {
        SimStorage::checkpoint(self)
    }

    fn restore(&mut self, checkpoint: &StorageCheckpoint) {
        SimStorage::restore_checkpoint(self, checkpoint);
    }

    fn stats(&self) -> StorageStats {
        SimStorage::stats(self).clone()
    }
}

// ============================================================================
// Production Implementation (Sketch)
// ============================================================================

/// File-based storage for production use (sketch).
///
/// **Note**: This is a sketch for architectural demonstration.
/// Full implementation would use proper file I/O with fsync.
#[cfg(not(test))]
pub struct FileStorage {
    // Would contain file handles, cache, etc.
    _placeholder: (),
}

#[cfg(not(test))]
impl FileStorage {
    /// Creates a new file-based storage.
    pub fn new(_path: &std::path::Path) -> Self {
        Self { _placeholder: () }
    }
}

#[cfg(not(test))]
impl Storage for FileStorage {
    fn write(&mut self, _address: u64, _data: Vec<u8>, _rng: &mut SimRng) -> WriteResult {
        // Would write to file
        WriteResult::Success {
            latency_ns: 1_000_000,
            bytes_written: _data.len(),
        }
    }

    fn read(&mut self, _address: u64, _rng: &mut SimRng) -> ReadResult {
        // Would read from file
        ReadResult::NotFound { latency_ns: 500_000 }
    }

    fn fsync(&mut self, _rng: &mut SimRng) -> FsyncResult {
        // Would call fsync()
        FsyncResult::Success {
            latency_ns: 5_000_000,
        }
    }

    fn crash(&mut self, _scenario: Option<CrashScenario>, _rng: &mut SimRng) {
        // Crashes not applicable in production
    }

    fn checkpoint(&self) -> StorageCheckpoint {
        StorageCheckpoint::default()
    }

    fn restore(&mut self, _checkpoint: &StorageCheckpoint) {
        // Would restore from file
    }

    fn stats(&self) -> StorageStats {
        StorageStats::default()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_storage_trait_impl() {
        let mut storage: Box<dyn Storage> = Box::new(SimStorage::reliable());
        let mut rng = SimRng::new(12345);

        // Write data
        let result = storage.write(0, vec![1, 2, 3, 4], &mut rng);
        assert!(matches!(result, WriteResult::Success { .. }));

        // Read it back
        let result = storage.read(0, &mut rng);
        if let ReadResult::Success { data, .. } = result {
            assert_eq!(data, vec![1, 2, 3, 4]);
        } else {
            panic!("Read should succeed");
        }
    }

    #[test]
    fn sim_storage_fsync_via_trait() {
        let mut storage: Box<dyn Storage> = Box::new(SimStorage::reliable());
        let mut rng = SimRng::new(12345);

        // Write data
        storage.write(0, vec![1, 2, 3, 4], &mut rng);

        // Fsync should succeed
        let result = storage.fsync(&mut rng);
        assert!(matches!(result, FsyncResult::Success { .. }));
    }

    #[test]
    fn sim_storage_crash_loses_pending_writes() {
        let mut storage: Box<dyn Storage> = Box::new(SimStorage::reliable());
        let mut rng = SimRng::new(12345);

        // Write and fsync (durable)
        storage.write(0, vec![1, 2, 3, 4], &mut rng);
        storage.fsync(&mut rng);

        // Write without fsync (pending)
        storage.write(1, vec![5, 6, 7, 8], &mut rng);

        // Crash
        storage.crash(None, &mut rng);

        // Address 0 should still have data (was fsynced)
        let result = storage.read(0, &mut rng);
        assert!(matches!(result, ReadResult::Success { .. }));

        // Address 1 should be lost (was pending)
        let result = storage.read(1, &mut rng);
        assert!(matches!(result, ReadResult::NotFound { .. }));
    }

    #[test]
    fn sim_storage_checkpoint_and_restore() {
        let mut storage = SimStorage::reliable();
        let mut rng = SimRng::new(12345);

        // Write some data
        storage.write(0, vec![1, 2, 3], &mut rng);
        storage.write(1, vec![4, 5, 6], &mut rng);
        storage.fsync(&mut rng);

        // Create checkpoint
        let checkpoint = storage.checkpoint();

        // Write more data
        storage.write(2, vec![7, 8, 9], &mut rng);
        storage.fsync(&mut rng);

        // Verify new data exists
        if let ReadResult::Success { data, .. } = storage.read(2, &mut rng) {
            assert_eq!(data, vec![7, 8, 9]);
        }

        // Restore from checkpoint
        storage.restore(&checkpoint);

        // New data should be gone
        let result = storage.read(2, &mut rng);
        assert!(matches!(result, ReadResult::NotFound { .. }));

        // Old data should still exist
        if let ReadResult::Success { data, .. } = storage.read(0, &mut rng) {
            assert_eq!(data, vec![1, 2, 3]);
        }
    }

    #[test]
    fn sim_storage_write_failure() {
        let config = StorageConfig {
            write_failure_probability: 1.0, // Always fail
            ..StorageConfig::default()
        };
        let mut storage: Box<dyn Storage> = Box::new(SimStorage::new(config));
        let mut rng = SimRng::new(12345);

        // Write should fail
        let result = storage.write(0, vec![1, 2, 3], &mut rng);
        assert!(matches!(result, WriteResult::Failed { .. }));

        // Data should not be readable
        let result = storage.read(0, &mut rng);
        assert!(matches!(result, ReadResult::NotFound { .. }));
    }

    #[test]
    fn sim_storage_read_corruption() {
        let config = StorageConfig {
            read_corruption_probability: 1.0, // Always corrupt
            ..StorageConfig::default()
        };
        let mut storage: Box<dyn Storage> = Box::new(SimStorage::new(config));
        let mut rng = SimRng::new(12345);

        // Write data
        storage.write(0, vec![1, 2, 3, 4], &mut rng);
        storage.fsync(&mut rng);

        // Read should be corrupted
        let result = storage.read(0, &mut rng);
        assert!(matches!(result, ReadResult::Corrupted { .. }));
    }

    #[test]
    fn sim_storage_stats() {
        let mut storage: Box<dyn Storage> = Box::new(SimStorage::reliable());
        let mut rng = SimRng::new(12345);

        // Perform some operations
        storage.write(0, vec![1, 2, 3], &mut rng);
        storage.read(0, &mut rng);
        storage.fsync(&mut rng);

        // Check stats
        let stats = storage.stats();
        assert_eq!(stats.writes, 1);
        assert_eq!(stats.reads, 1);
        assert_eq!(stats.fsyncs, 1);
    }
}
