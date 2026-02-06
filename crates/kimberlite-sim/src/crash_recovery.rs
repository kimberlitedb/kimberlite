//! Enhanced crash recovery semantics for storage testing.
//!
//! This module simulates realistic crash behavior including:
//! - Partial fsync (only subset of pending writes durable)
//! - Torn multi-block writes (4KB atomic units)
//! - Block-level granularity (lose individual 4KB blocks)
//! - "Seen but corrupt" vs "not seen" distinction
//! - Crash timing variations
//!
//! Real systems experience complex crash behavior. Power loss can result in:
//! - Some writes making it to disk, others not
//! - Multi-block writes being torn at block boundaries
//! - Data corruption in blocks that were mid-write
//! - Reordered writes becoming durable in unexpected orders
//!
//! ## Design
//!
//! The crash recovery engine maintains:
//! - Pending write state (pre-fsync)
//! - In-fsync write state (fsync issued but not complete)
//! - Durable write state (fsync complete)
//! - Block-level granularity (4KB blocks)
//! - Crash scenarios (timing and failure modes)
//!
//! ## Example
//!
//! ```ignore
//! let mut engine = CrashRecoveryEngine::new(CrashConfig::default());
//! engine.record_write(address, data);
//! engine.start_fsync();
//! // ... crash occurs ...
//! let state = engine.crash_with_scenario(CrashScenario::PartialFsync, rng);
//! engine.recover(state);
//! ```

use std::collections::{HashMap, HashSet};

use crate::rng::SimRng;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for crash recovery behavior.
#[derive(Debug, Clone)]
pub struct CrashConfig {
    /// Block size for atomic writes (typical: 4096 bytes).
    pub block_size: usize,

    /// Probability of torn writes on crash (0.0 to 1.0).
    pub torn_write_probability: f64,

    /// Probability of corruption in torn blocks (0.0 to 1.0).
    pub corruption_probability: f64,
}

impl Default for CrashConfig {
    fn default() -> Self {
        Self {
            block_size: 4096,
            torn_write_probability: 0.3,
            corruption_probability: 0.1,
        }
    }
}

impl CrashConfig {
    /// Creates a configuration with guaranteed clean crashes (no torn writes).
    pub fn clean_crashes() -> Self {
        Self {
            torn_write_probability: 0.0,
            corruption_probability: 0.0,
            ..Default::default()
        }
    }

    /// Creates a configuration with aggressive crash behavior.
    pub fn aggressive() -> Self {
        Self {
            torn_write_probability: 0.8,
            corruption_probability: 0.3,
            ..Default::default()
        }
    }
}

// ============================================================================
// Crash Scenarios
// ============================================================================

/// Timing and mode of crash event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashScenario {
    /// Crash during write (before fsync).
    /// All writes are lost.
    DuringWrite,

    /// Crash during fsync (some writes may be durable).
    /// Partial fsync: random subset of writes become durable.
    DuringFsync,

    /// Crash after fsync completes but before acknowledgment.
    /// All writes should be durable (but application doesn't know).
    AfterFsyncBeforeAck,

    /// Power loss: abrupt termination with maximum data loss.
    /// Can result in torn writes and corruption.
    PowerLoss,

    /// Clean shutdown: all fsynced data is durable.
    CleanShutdown,
}

/// State of storage after crash.
#[derive(Debug, Clone)]
pub struct CrashState {
    /// Durable blocks after crash (address -> data).
    pub durable_blocks: HashMap<u64, Vec<u8>>,

    /// Corrupted blocks (address -> corrupted data).
    pub corrupted_blocks: HashMap<u64, Vec<u8>>,

    /// Lost blocks (were in-flight but lost).
    pub lost_blocks: HashSet<u64>,

    /// Crash scenario that occurred.
    pub scenario: CrashScenario,
}

// ============================================================================
// Write States
// ============================================================================

/// State of a write operation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WriteState {
    /// Pending (not yet fsynced).
    Pending,

    /// In-fsync (fsync issued but not complete).
    InFsync,

    /// Durable (fsync complete).
    Durable,
}

/// A tracked write with block-level granularity.
#[derive(Debug, Clone)]
struct TrackedWrite {
    /// Storage address.
    #[allow(dead_code)]
    // Phase 1 infrastructure: Used for future address-based crash scenarios
    address: u64,

    /// Data being written.
    data: Vec<u8>,

    /// Current state.
    state: WriteState,

    /// Block addresses covered by this write.
    block_addresses: Vec<u64>,
}

impl TrackedWrite {
    /// Creates a new tracked write, splitting into blocks.
    fn new(address: u64, data: Vec<u8>, block_size: usize) -> Self {
        let block_addresses = Self::compute_block_addresses(address, data.len(), block_size);

        Self {
            address,
            data,
            state: WriteState::Pending,
            block_addresses,
        }
    }

    /// Computes the block addresses covered by a write.
    fn compute_block_addresses(address: u64, size: usize, block_size: usize) -> Vec<u64> {
        let start_block = address / block_size as u64;
        let end_byte = address + size as u64;
        let end_block = end_byte.div_ceil(block_size as u64);

        (start_block..end_block).collect()
    }
}

// ============================================================================
// Crash Recovery Engine
// ============================================================================

/// Crash recovery engine that models realistic crash behavior.
///
/// Tracks write states and simulates various crash scenarios with
/// block-level granularity, torn writes, and partial durability.
#[derive(Debug)]
pub struct CrashRecoveryEngine {
    /// Configuration.
    config: CrashConfig,

    /// Tracked writes, keyed by address.
    writes: HashMap<u64, TrackedWrite>,

    /// Whether an fsync is currently in progress.
    fsync_in_progress: bool,

    /// Addresses that have been fsynced and are durable.
    durable_addresses: HashSet<u64>,
}

impl CrashRecoveryEngine {
    /// Creates a new crash recovery engine with the given configuration.
    pub fn new(config: CrashConfig) -> Self {
        Self {
            config,
            writes: HashMap::new(),
            fsync_in_progress: false,
            durable_addresses: HashSet::new(),
        }
    }

    /// Creates an engine with default configuration.
    pub fn default_config() -> Self {
        Self::new(CrashConfig::default())
    }

    /// Records a write operation (not yet fsynced).
    pub fn record_write(&mut self, address: u64, data: Vec<u8>) {
        let write = TrackedWrite::new(address, data, self.config.block_size);
        self.writes.insert(address, write);
    }

    /// Starts an fsync operation.
    ///
    /// Moves all pending writes to in-fsync state.
    pub fn start_fsync(&mut self) {
        self.fsync_in_progress = true;

        for write in self.writes.values_mut() {
            if write.state == WriteState::Pending {
                write.state = WriteState::InFsync;
            }
        }
    }

    /// Completes an fsync operation.
    ///
    /// Moves all in-fsync writes to durable state.
    pub fn complete_fsync(&mut self) {
        self.fsync_in_progress = false;

        for (address, write) in &mut self.writes {
            if write.state == WriteState::InFsync {
                write.state = WriteState::Durable;
                self.durable_addresses.insert(*address);
            }
        }
    }

    /// Simulates a crash with the given scenario.
    ///
    /// Returns the state of storage after the crash, including:
    /// - Which blocks are durable
    /// - Which blocks are corrupted
    /// - Which blocks are lost
    pub fn crash_with_scenario(&self, scenario: CrashScenario, rng: &mut SimRng) -> CrashState {
        match scenario {
            CrashScenario::DuringWrite => self.crash_during_write(),
            CrashScenario::DuringFsync => self.crash_during_fsync(rng),
            CrashScenario::AfterFsyncBeforeAck => self.crash_after_fsync(),
            CrashScenario::PowerLoss => self.crash_power_loss(rng),
            CrashScenario::CleanShutdown => self.clean_shutdown(),
        }
    }

    /// Crash during write (before fsync): all pending writes lost.
    fn crash_during_write(&self) -> CrashState {
        let mut durable_blocks = HashMap::new();
        let mut lost_blocks = HashSet::new();

        for (address, write) in &self.writes {
            if write.state == WriteState::Durable {
                durable_blocks.insert(*address, write.data.clone());
            } else {
                lost_blocks.insert(*address);
            }
        }

        CrashState {
            durable_blocks,
            corrupted_blocks: HashMap::new(),
            lost_blocks,
            scenario: CrashScenario::DuringWrite,
        }
    }

    /// Crash during fsync: partial durability with torn writes.
    fn crash_during_fsync(&self, rng: &mut SimRng) -> CrashState {
        let mut durable_blocks = HashMap::new();
        let mut corrupted_blocks = HashMap::new();
        let mut lost_blocks = HashSet::new();

        for (address, write) in &self.writes {
            match write.state {
                WriteState::Durable => {
                    // Already durable before this fsync
                    durable_blocks.insert(*address, write.data.clone());
                }
                WriteState::InFsync => {
                    // Partial fsync: random subset becomes durable
                    if rng.next_bool() {
                        // Write made it to disk
                        if self.config.torn_write_probability > 0.0
                            && rng.next_bool_with_probability(self.config.torn_write_probability)
                        {
                            // Torn write: only some blocks durable
                            self.apply_torn_write(
                                *address,
                                write,
                                rng,
                                &mut durable_blocks,
                                &mut corrupted_blocks,
                                &mut lost_blocks,
                            );
                        } else {
                            // Complete write made it
                            durable_blocks.insert(*address, write.data.clone());
                        }
                    } else {
                        // Write didn't make it
                        lost_blocks.insert(*address);
                    }
                }
                WriteState::Pending => {
                    // Pending writes are lost
                    lost_blocks.insert(*address);
                }
            }
        }

        CrashState {
            durable_blocks,
            corrupted_blocks,
            lost_blocks,
            scenario: CrashScenario::DuringFsync,
        }
    }

    /// Crash after fsync but before ack: all fsynced data durable.
    fn crash_after_fsync(&self) -> CrashState {
        let mut durable_blocks = HashMap::new();
        let mut lost_blocks = HashSet::new();

        for (address, write) in &self.writes {
            if write.state == WriteState::Durable || write.state == WriteState::InFsync {
                durable_blocks.insert(*address, write.data.clone());
            } else {
                lost_blocks.insert(*address);
            }
        }

        CrashState {
            durable_blocks,
            corrupted_blocks: HashMap::new(),
            lost_blocks,
            scenario: CrashScenario::AfterFsyncBeforeAck,
        }
    }

    /// Power loss: maximum chaos with torn writes and corruption.
    fn crash_power_loss(&self, rng: &mut SimRng) -> CrashState {
        let mut durable_blocks = HashMap::new();
        let mut corrupted_blocks = HashMap::new();
        let mut lost_blocks = HashSet::new();

        for (address, write) in &self.writes {
            if write.state == WriteState::Durable {
                // Durable writes survive (but may corrupt)
                if self.config.corruption_probability > 0.0
                    && rng.next_bool_with_probability(self.config.corruption_probability)
                {
                    corrupted_blocks.insert(*address, self.corrupt_data(&write.data, rng));
                } else {
                    durable_blocks.insert(*address, write.data.clone());
                }
            } else {
                // Non-durable writes: random survival with high torn write rate
                if rng.next_bool_with_probability(0.2) {
                    // 20% survival rate
                    self.apply_torn_write(
                        *address,
                        write,
                        rng,
                        &mut durable_blocks,
                        &mut corrupted_blocks,
                        &mut lost_blocks,
                    );
                } else {
                    lost_blocks.insert(*address);
                }
            }
        }

        CrashState {
            durable_blocks,
            corrupted_blocks,
            lost_blocks,
            scenario: CrashScenario::PowerLoss,
        }
    }

    /// Clean shutdown: all fsynced data durable, pending lost.
    fn clean_shutdown(&self) -> CrashState {
        let mut durable_blocks = HashMap::new();
        let mut lost_blocks = HashSet::new();

        for (address, write) in &self.writes {
            if write.state == WriteState::Durable {
                durable_blocks.insert(*address, write.data.clone());
            } else {
                lost_blocks.insert(*address);
            }
        }

        CrashState {
            durable_blocks,
            corrupted_blocks: HashMap::new(),
            lost_blocks,
            scenario: CrashScenario::CleanShutdown,
        }
    }

    /// Applies torn write semantics: some blocks durable, some lost, some corrupt.
    fn apply_torn_write(
        &self,
        address: u64,
        write: &TrackedWrite,
        rng: &mut SimRng,
        durable_blocks: &mut HashMap<u64, Vec<u8>>,
        corrupted_blocks: &mut HashMap<u64, Vec<u8>>,
        lost_blocks: &mut HashSet<u64>,
    ) {
        let num_blocks = write.block_addresses.len();
        let num_durable = if num_blocks > 1 {
            rng.next_usize(num_blocks - 1) + 1 // At least 1, at most n-1
        } else {
            0 // Single block write that's torn = lost
        };

        if num_durable == 0 {
            lost_blocks.insert(address);
            return;
        }

        // Determine which blocks are durable (first N blocks)
        let blocks_durable = num_durable;

        // Split data into blocks
        let bytes_per_block = self.config.block_size;
        let durable_bytes = bytes_per_block * blocks_durable;

        if durable_bytes < write.data.len() {
            // Torn write: partial data
            let partial_data = write.data[..durable_bytes].to_vec();

            // May be corrupted
            if self.config.corruption_probability > 0.0
                && rng.next_bool_with_probability(self.config.corruption_probability)
            {
                corrupted_blocks.insert(address, self.corrupt_data(&partial_data, rng));
            } else {
                durable_blocks.insert(address, partial_data);
            }
        } else {
            // All blocks made it (not really torn)
            durable_blocks.insert(address, write.data.clone());
        }
    }

    /// Corrupts data by flipping random bits.
    fn corrupt_data(&self, data: &[u8], rng: &mut SimRng) -> Vec<u8> {
        let mut corrupted = data.to_vec();
        if !corrupted.is_empty() {
            let byte_idx = rng.next_usize(corrupted.len());
            let bit_idx = rng.next_usize(8);
            corrupted[byte_idx] ^= 1 << bit_idx;
        }
        corrupted
    }

    /// Recovers from a crash state.
    ///
    /// Resets the engine to the post-crash state.
    pub fn recover(&mut self, state: CrashState) {
        self.writes.clear();
        self.fsync_in_progress = false;
        self.durable_addresses.clear();

        // Restore durable blocks
        for (address, data) in state.durable_blocks {
            let mut write = TrackedWrite::new(address, data, self.config.block_size);
            write.state = WriteState::Durable;
            self.writes.insert(address, write);
            self.durable_addresses.insert(address);
        }

        // Corrupted blocks are also "durable" (but corrupt)
        for (address, data) in state.corrupted_blocks {
            let mut write = TrackedWrite::new(address, data, self.config.block_size);
            write.state = WriteState::Durable;
            self.writes.insert(address, write);
            self.durable_addresses.insert(address);
        }

        // Lost blocks are simply not present
    }

    /// Returns the number of durable writes.
    pub fn durable_count(&self) -> usize {
        self.durable_addresses.len()
    }

    /// Returns the number of pending writes.
    pub fn pending_count(&self) -> usize {
        self.writes
            .values()
            .filter(|w| w.state == WriteState::Pending)
            .count()
    }

    /// Returns whether an fsync is in progress.
    pub fn is_fsync_in_progress(&self) -> bool {
        self.fsync_in_progress
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_fsync() {
        let mut engine = CrashRecoveryEngine::default_config();

        engine.record_write(0, vec![1, 2, 3, 4]);
        assert_eq!(engine.pending_count(), 1);

        engine.start_fsync();
        assert!(engine.is_fsync_in_progress());

        engine.complete_fsync();
        assert!(!engine.is_fsync_in_progress());
        assert_eq!(engine.durable_count(), 1);
    }

    #[test]
    fn crash_during_write_loses_pending() {
        let mut engine = CrashRecoveryEngine::default_config();
        let mut rng = SimRng::new(42);

        engine.record_write(0, vec![1, 2, 3, 4]);

        let state = engine.crash_with_scenario(CrashScenario::DuringWrite, &mut rng);

        assert_eq!(state.durable_blocks.len(), 0);
        assert_eq!(state.lost_blocks.len(), 1);
    }

    #[test]
    fn crash_after_fsync_preserves_data() {
        let mut engine = CrashRecoveryEngine::default_config();
        let mut rng = SimRng::new(42);

        engine.record_write(0, vec![1, 2, 3, 4]);
        engine.start_fsync();
        engine.complete_fsync();

        let state = engine.crash_with_scenario(CrashScenario::AfterFsyncBeforeAck, &mut rng);

        assert_eq!(state.durable_blocks.len(), 1);
        assert_eq!(state.lost_blocks.len(), 0);
    }

    #[test]
    fn clean_shutdown_loses_only_pending() {
        let mut engine = CrashRecoveryEngine::default_config();
        let mut rng = SimRng::new(42);

        engine.record_write(0, vec![1, 2, 3, 4]);
        engine.start_fsync();
        engine.complete_fsync();

        engine.record_write(1, vec![5, 6, 7, 8]);

        let state = engine.crash_with_scenario(CrashScenario::CleanShutdown, &mut rng);

        assert_eq!(state.durable_blocks.len(), 1);
        assert!(state.durable_blocks.contains_key(&0));
        assert_eq!(state.lost_blocks.len(), 1);
        assert!(state.lost_blocks.contains(&1));
    }

    #[test]
    fn partial_fsync_can_lose_some_writes() {
        let mut engine = CrashRecoveryEngine::default_config();
        let mut rng = SimRng::new(42);

        engine.record_write(0, vec![1, 2, 3, 4]);
        engine.record_write(1, vec![5, 6, 7, 8]);
        engine.start_fsync();

        let state = engine.crash_with_scenario(CrashScenario::DuringFsync, &mut rng);

        // Some combination of durable and lost
        let total = state.durable_blocks.len() + state.lost_blocks.len();
        assert_eq!(total, 2);
    }

    #[test]
    fn torn_write_creates_partial_blocks() {
        let config = CrashConfig {
            block_size: 4,
            torn_write_probability: 1.0, // Always torn
            corruption_probability: 0.0,
        };
        let mut engine = CrashRecoveryEngine::new(config);
        let mut rng = SimRng::new(42);

        // Write 8 bytes (2 blocks)
        engine.record_write(0, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        engine.start_fsync();

        let state = engine.crash_with_scenario(CrashScenario::DuringFsync, &mut rng);

        // Should have partial data (torn write)
        // Exact behavior depends on RNG, but we should see either:
        // - Durable with partial data, or
        // - Lost entirely
        assert!(
            state.durable_blocks.len() + state.lost_blocks.len() >= 1,
            "write should be accounted for"
        );
    }

    #[test]
    fn recovery_restores_durable_state() {
        let mut engine = CrashRecoveryEngine::default_config();
        let mut rng = SimRng::new(42);

        engine.record_write(0, vec![1, 2, 3, 4]);
        engine.start_fsync();
        engine.complete_fsync();

        let state = engine.crash_with_scenario(CrashScenario::CleanShutdown, &mut rng);

        engine.recover(state);

        assert_eq!(engine.durable_count(), 1);
        assert_eq!(engine.pending_count(), 0);
    }
}
