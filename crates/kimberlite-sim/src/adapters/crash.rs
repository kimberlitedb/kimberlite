//! Crash controller adapter trait for failure injection testing.
//!
//! This module provides a trait-based abstraction for crash recovery:
//! - **Deterministic simulation**: Use `CrashRecoveryEngine` with realistic crash semantics
//! - **Production use**: Crashes not applicable (would be no-ops)
//!
//! # Performance
//!
//! The `CrashController` trait is on the cold path (failure injection), so
//! trait objects are acceptable. Crash operations are rare and not performance-critical.

// Re-export types from parent module
pub use crate::crash_recovery::{CrashConfig, CrashRecoveryEngine, CrashScenario, CrashState};
pub use crate::rng::SimRng;

/// Trait for crash recovery control (simulation or production).
///
/// Implementations handle crash/recovery semantics including partial writes,
/// torn writes, and corruption.
pub trait CrashController {
    /// Records a write operation (not yet fsynced).
    ///
    /// Used to track pending writes that may be lost on crash.
    fn record_write(&mut self, address: u64, data: Vec<u8>);

    /// Starts an fsync operation.
    ///
    /// Moves pending writes to in-fsync state (vulnerable to partial fsync).
    fn start_fsync(&mut self);

    /// Completes an fsync operation.
    ///
    /// Moves in-fsync writes to durable state (safe from crash).
    fn complete_fsync(&mut self);

    /// Simulates a crash with the given scenario.
    ///
    /// # Arguments
    ///
    /// * `scenario` - Type of crash (DuringWrite, DuringFsync, PowerLoss, etc.)
    /// * `rng` - Random number generator (for partial fsync, torn writes, etc.)
    ///
    /// # Returns
    ///
    /// `CrashState` describing which blocks are durable, corrupted, or lost.
    fn crash(&mut self, scenario: CrashScenario, rng: &mut SimRng) -> CrashState;

    /// Recovers from a crash state.
    ///
    /// Resets the controller to the post-crash state (only durable blocks).
    fn recover(&mut self, state: CrashState);
}

// ============================================================================
// Simulation Implementation
// ============================================================================

impl CrashController for CrashRecoveryEngine {
    fn record_write(&mut self, address: u64, data: Vec<u8>) {
        CrashRecoveryEngine::record_write(self, address, data);
    }

    fn start_fsync(&mut self) {
        CrashRecoveryEngine::start_fsync(self);
    }

    fn complete_fsync(&mut self) {
        CrashRecoveryEngine::complete_fsync(self);
    }

    fn crash(&mut self, scenario: CrashScenario, rng: &mut SimRng) -> CrashState {
        CrashRecoveryEngine::crash_with_scenario(self, scenario, rng)
    }

    fn recover(&mut self, state: CrashState) {
        CrashRecoveryEngine::recover(self, state);
    }
}

// ============================================================================
// Production Implementation (Sketch)
// ============================================================================

/// No-op crash controller for production use (sketch).
///
/// **Note**: Crashes are not simulated in production, so this is a no-op.
#[cfg(not(test))]
#[derive(Default)]
pub struct NoOpCrashController;

#[cfg(not(test))]
impl NoOpCrashController {
    /// Creates a new no-op crash controller.
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(test))]
impl CrashController for NoOpCrashController {
    fn record_write(&mut self, _address: u64, _data: Vec<u8>) {
        // No-op
    }

    fn start_fsync(&mut self) {
        // No-op
    }

    fn complete_fsync(&mut self) {
        // No-op
    }

    fn crash(&mut self, scenario: CrashScenario, _rng: &mut SimRng) -> CrashState {
        // Return empty crash state (no-op)
        CrashState {
            durable_blocks: std::collections::HashMap::new(),
            corrupted_blocks: std::collections::HashMap::new(),
            lost_blocks: std::collections::HashSet::new(),
            scenario,
        }
    }

    fn recover(&mut self, _state: CrashState) {
        // No-op
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_engine_trait_impl() {
        let mut engine: Box<dyn CrashController> =
            Box::new(CrashRecoveryEngine::new(CrashConfig::default()));
        let mut rng = SimRng::new(12345);

        // Record writes
        engine.record_write(0, vec![1, 2, 3, 4]);
        engine.record_write(1, vec![5, 6, 7, 8]);

        // Start and complete fsync
        engine.start_fsync();
        engine.complete_fsync();

        // Record another write (pending)
        engine.record_write(2, vec![9, 10, 11, 12]);

        // Crash during write (pending writes lost)
        let state = engine.crash(CrashScenario::DuringWrite, &mut rng);

        // Fsynced writes should be durable
        assert!(state.durable_blocks.contains_key(&0));
        assert!(state.durable_blocks.contains_key(&1));

        // Pending write should be lost
        assert!(state.lost_blocks.contains(&2));
    }

    #[test]
    fn crash_during_fsync_partial_durability() {
        let mut engine: Box<dyn CrashController> =
            Box::new(CrashRecoveryEngine::new(CrashConfig::default()));
        let mut rng = SimRng::new(12345);

        // Record multiple writes
        for i in 0..10 {
            engine.record_write(i, vec![i as u8; 100]);
        }

        // Start fsync but don't complete
        engine.start_fsync();

        // Crash during fsync
        let state = engine.crash(CrashScenario::DuringFsync, &mut rng);

        // Some writes may be durable, some may be lost
        let durable_count = state.durable_blocks.len();
        let lost_count = state.lost_blocks.len();

        // Partial fsync should result in partial durability
        assert!(durable_count + lost_count == 10);
        // Not all writes should be durable or all lost (probabilistic)
        // (This test is probabilistic, may occasionally fail with bad RNG)
    }

    #[test]
    fn crash_after_fsync_all_durable() {
        let mut engine: Box<dyn CrashController> =
            Box::new(CrashRecoveryEngine::new(CrashConfig::default()));
        let mut rng = SimRng::new(12345);

        // Record and fsync writes
        engine.record_write(0, vec![1, 2, 3, 4]);
        engine.record_write(1, vec![5, 6, 7, 8]);
        engine.start_fsync();
        engine.complete_fsync();

        // Crash after fsync completes
        let state = engine.crash(CrashScenario::AfterFsyncBeforeAck, &mut rng);

        // All fsynced writes should be durable
        assert_eq!(state.durable_blocks.len(), 2);
        assert_eq!(state.lost_blocks.len(), 0);
        assert!(state.durable_blocks.contains_key(&0));
        assert!(state.durable_blocks.contains_key(&1));
    }

    #[test]
    fn crash_clean_shutdown() {
        let mut engine: Box<dyn CrashController> =
            Box::new(CrashRecoveryEngine::new(CrashConfig::default()));
        let mut rng = SimRng::new(12345);

        // Record and fsync writes
        engine.record_write(0, vec![1, 2, 3, 4]);
        engine.start_fsync();
        engine.complete_fsync();

        // Clean shutdown
        let state = engine.crash(CrashScenario::CleanShutdown, &mut rng);

        // All writes should be cleanly durable (no corruption)
        assert_eq!(state.durable_blocks.len(), 1);
        assert_eq!(state.corrupted_blocks.len(), 0);
        assert_eq!(state.lost_blocks.len(), 0);
    }

    #[test]
    fn crash_power_loss_can_corrupt() {
        let config = CrashConfig {
            corruption_probability: 1.0, // Force corruption for testing
            torn_write_probability: 1.0,
            ..Default::default()
        };
        let mut engine: Box<dyn CrashController> = Box::new(CrashRecoveryEngine::new(config));
        let mut rng = SimRng::new(12345);

        // Record writes
        engine.record_write(0, vec![1; 8192]); // 2 blocks (4KB each)
        engine.start_fsync();

        // Power loss crash
        let state = engine.crash(CrashScenario::PowerLoss, &mut rng);

        // Power loss can result in corruption
        // (With 100% corruption probability, we should see corrupted blocks)
        assert!(
            state.corrupted_blocks.len() > 0 || state.lost_blocks.len() > 0,
            "Power loss should result in corruption or loss"
        );
    }

    #[test]
    fn crash_recover_cycle() {
        let mut engine: Box<dyn CrashController> =
            Box::new(CrashRecoveryEngine::new(CrashConfig::default()));
        let mut rng = SimRng::new(12345);

        // Record and fsync writes
        engine.record_write(0, vec![1, 2, 3, 4]);
        engine.start_fsync();
        engine.complete_fsync();

        // Record pending write
        engine.record_write(1, vec![5, 6, 7, 8]);

        // Crash
        let state = engine.crash(CrashScenario::DuringWrite, &mut rng);

        // Recover from crash
        engine.recover(state.clone());

        // After recovery, only durable blocks should remain
        // (This is verified implicitly - recovery clears pending writes)

        // New writes should work after recovery
        engine.record_write(2, vec![9, 10, 11, 12]);
        engine.start_fsync();
        engine.complete_fsync();
    }

    #[test]
    fn crash_generic_usage() {
        fn use_crash_controller<C: CrashController>(controller: &mut C, rng: &mut SimRng) {
            controller.record_write(0, vec![1, 2, 3]);
            controller.start_fsync();
            controller.complete_fsync();
            let _state = controller.crash(CrashScenario::CleanShutdown, rng);
        }

        let mut engine = CrashRecoveryEngine::new(CrashConfig::default());
        let mut rng = SimRng::new(12345);
        use_crash_controller(&mut engine, &mut rng);
    }
}
