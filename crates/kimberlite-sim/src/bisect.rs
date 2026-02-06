//! Binary search algorithm for finding first failing event.
//!
//! This module implements automatic bisection to identify the minimal event
//! prefix that triggers an invariant violation. Uses checkpointing for efficient
//! replay from intermediate states.

use crate::checkpoint::{CheckpointManager, RngCheckpoint, SimulationCheckpoint};
use crate::event_log::ReproBundle;

// ============================================================================
// Bisection Configuration
// ============================================================================

/// Configuration for bisection engine.
#[derive(Debug, Clone)]
pub struct BisectConfig {
    /// Create checkpoint every N events (default: 1000).
    pub checkpoint_interval: u64,
    /// Maximum bisection iterations (default: 50).
    pub max_iterations: usize,
    /// Invariant to verify.
    pub verify_invariant: String,
}

impl Default for BisectConfig {
    fn default() -> Self {
        Self {
            checkpoint_interval: 1000,
            max_iterations: 50,
            verify_invariant: String::new(),
        }
    }
}

// ============================================================================
// Bisection Result
// ============================================================================

/// Result of bisection search.
#[derive(Debug, Clone)]
pub struct BisectResult {
    /// First event that triggers failure.
    pub first_bad_event: u64,
    /// Last event before failure.
    pub last_good_event: u64,
    /// Number of bisection iterations.
    pub iterations: usize,
    /// Number of checkpoints created.
    pub checkpoints_created: usize,
    /// Total replay time (ms).
    pub replay_time_ms: u64,
    /// Minimized bundle with only necessary events.
    pub minimized_bundle: ReproBundle,
}

// ============================================================================
// Bisection Engine
// ============================================================================

/// Binary search engine to find first failing event.
pub struct BisectEngine {
    /// Original failure bundle.
    bundle: ReproBundle,
    /// Bisection configuration.
    config: BisectConfig,
    /// Checkpoint manager.
    checkpoint_manager: CheckpointManager,
}

impl BisectEngine {
    /// Creates a new bisection engine.
    pub fn new(bundle: ReproBundle, config: BisectConfig) -> Self {
        Self {
            bundle,
            config,
            checkpoint_manager: CheckpointManager::new(20), // Keep 20 checkpoints
        }
    }

    /// Runs binary search to find first failing event.
    pub fn bisect(&mut self) -> Result<BisectResult, BisectError> {
        let total_events = self
            .bundle
            .event_log
            .as_ref()
            .ok_or(BisectError::NoEventLog)?
            .len() as u64;

        if total_events == 0 {
            return Err(BisectError::NoEvents);
        }

        // PRECONDITION (after error handling)
        assert!(total_events > 0, "bisect requires non-empty event log");

        println!("Starting bisection...");
        println!("Total events: {}", total_events);
        println!("Checkpoint interval: {}", self.config.checkpoint_interval);

        let start_time = std::time::Instant::now();
        let mut left = 0u64;
        let mut right = total_events;
        let mut iterations = 0;

        while left < right && iterations < self.config.max_iterations {
            // LOOP INVARIANT
            assert!(
                left <= right,
                "binary search invariant violated: left={left}, right={right}"
            );
            assert!(
                right <= total_events,
                "right boundary exceeded: right={right}, total={total_events}"
            );
            let mid = u64::midpoint(left, right);

            println!(
                "\nIteration {}: Testing event range [{}, {}], mid={}",
                iterations, left, right, mid
            );

            let result = self.test_events_up_to(mid)?;

            match result {
                TestResult::Failure => {
                    println!("  → Failure at or before event {}", mid);
                    right = mid;
                }
                TestResult::Success => {
                    println!("  → No failure up to event {}", mid);
                    left = mid + 1;
                }
            }

            iterations += 1;
        }

        let elapsed = start_time.elapsed();

        // POSTCONDITION: Binary search converged
        assert!(
            left == right || iterations >= self.config.max_iterations,
            "bisect terminated incorrectly: left={left}, right={right}, iterations={iterations}"
        );

        println!("\n═══════════════════════════════════════════");
        println!("Bisection Complete");
        println!("═══════════════════════════════════════════");
        println!("First bad event:  {}", left);
        println!("Last good event:  {}", left.saturating_sub(1));
        println!("Iterations:       {}", iterations);
        println!("Checkpoints:      {}", self.checkpoint_manager.len());
        println!("Time:             {:.2}s", elapsed.as_secs_f64());

        // Create minimized bundle
        let minimized_bundle = self.create_minimized_bundle(left)?;

        Ok(BisectResult {
            first_bad_event: left,
            last_good_event: left.saturating_sub(1),
            iterations,
            checkpoints_created: self.checkpoint_manager.len(),
            replay_time_ms: elapsed.as_millis() as u64,
            minimized_bundle,
        })
    }

    /// Tests whether failure occurs up to specified event.
    ///
    /// This is a simplified implementation for proof-of-concept. A full implementation
    /// would replay the actual event log and check real invariants.
    #[allow(clippy::unnecessary_wraps)] // Phase 1 infrastructure: Result for future error handling
    fn test_events_up_to(&mut self, max_event: u64) -> Result<TestResult, BisectError> {
        // Find closest checkpoint
        let checkpoint = self.checkpoint_manager.find_closest(max_event);

        let start_event = checkpoint.map(|cp| cp.event_count).unwrap_or(0);

        if checkpoint.is_some() {
            println!("    Restoring from checkpoint at event {}", start_event);
        } else {
            println!("    Starting from genesis");
        }

        // Create checkpoints as we go
        let mut last_checkpoint = start_event;
        while last_checkpoint + self.config.checkpoint_interval <= max_event {
            last_checkpoint += self.config.checkpoint_interval;

            let checkpoint = SimulationCheckpoint::new(
                last_checkpoint,
                last_checkpoint * 1000, // Simplified time
                RngCheckpoint {
                    seed: self.bundle.seed,
                    step_count: last_checkpoint,
                },
            );
            self.checkpoint_manager.save(checkpoint);
        }

        // Check if failure occurs in the range [start_event, max_event]
        let failure_event = self.bundle.failure.failed_at_event;

        if failure_event >= start_event && failure_event <= max_event {
            Ok(TestResult::Failure)
        } else {
            Ok(TestResult::Success)
        }
    }

    /// Creates minimized bundle with only events up to first bad event.
    #[allow(clippy::unnecessary_wraps)] // Phase 1 infrastructure: Result for future error handling
    fn create_minimized_bundle(&self, max_event: u64) -> Result<ReproBundle, BisectError> {
        let mut minimized = self.bundle.clone();

        if let Some(ref mut events) = minimized.event_log {
            events.truncate(max_event as usize);
        }

        Ok(minimized)
    }
}

// ============================================================================
// Test Result
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestResult {
    /// Test passed (no failure).
    Success,
    /// Test failed (invariant violated).
    Failure,
}

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur during bisection.
#[derive(Debug, thiserror::Error)]
pub enum BisectError {
    #[error("Bundle has no event log")]
    NoEventLog,

    #[error("Event log is empty")]
    NoEvents,

    #[error("Bisection did not converge")]
    NoConvergence,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{Decision, FailureInfo, LoggedEvent};

    fn create_test_bundle(num_events: usize, failure_at: u64) -> ReproBundle {
        let mut events = Vec::new();

        for i in 0..num_events {
            events.push(LoggedEvent {
                event_id: i as u64,
                time_ns: i as u64 * 1000,
                decision: Decision::RngValue { value: i as u64 },
            });
        }

        ReproBundle::new(
            42,
            "test".to_string(),
            Some(events),
            FailureInfo {
                invariant_name: "test_invariant".to_string(),
                message: "Test failure".to_string(),
                failed_at_event: failure_at,
                failed_at_time_ns: failure_at * 1000,
            },
        )
    }

    #[test]
    fn bisect_basic() {
        let bundle = create_test_bundle(100, 50);
        let config = BisectConfig {
            checkpoint_interval: 10,
            max_iterations: 20,
            verify_invariant: "test_invariant".to_string(),
        };

        let mut engine = BisectEngine::new(bundle, config);
        let result = engine.bisect().unwrap();

        // Should find event 50 as first bad event
        assert_eq!(result.first_bad_event, 50);
        assert_eq!(result.last_good_event, 49);
        assert!(result.iterations <= 20);
    }

    #[test]
    fn bisect_first_event_fails() {
        let bundle = create_test_bundle(100, 0);
        let config = BisectConfig::default();

        let mut engine = BisectEngine::new(bundle, config);
        let result = engine.bisect().unwrap();

        assert_eq!(result.first_bad_event, 0);
    }

    #[test]
    fn bisect_last_event_fails() {
        let bundle = create_test_bundle(100, 99);
        let config = BisectConfig::default();

        let mut engine = BisectEngine::new(bundle, config);
        let result = engine.bisect().unwrap();

        assert_eq!(result.first_bad_event, 99);
        assert_eq!(result.last_good_event, 98);
    }

    #[test]
    fn bisect_no_events_error() {
        let bundle = create_test_bundle(0, 0);
        let config = BisectConfig::default();

        let mut engine = BisectEngine::new(bundle, config);
        let result = engine.bisect();

        assert!(matches!(result, Err(BisectError::NoEvents)));
    }

    #[test]
    fn bisect_creates_checkpoints() {
        let bundle = create_test_bundle(1000, 500);
        let config = BisectConfig {
            checkpoint_interval: 100,
            ..Default::default()
        };

        let mut engine = BisectEngine::new(bundle, config);
        let result = engine.bisect().unwrap();

        // Should have created some checkpoints
        assert!(result.checkpoints_created > 0);
    }

    #[test]
    fn bisect_minimized_bundle() {
        let bundle = create_test_bundle(100, 50);
        let config = BisectConfig::default();

        let mut engine = BisectEngine::new(bundle, config);
        let result = engine.bisect().unwrap();

        // Minimized bundle should have events up to first_bad_event
        let events = result.minimized_bundle.event_log.unwrap();
        assert_eq!(events.len(), 50);
    }
}
