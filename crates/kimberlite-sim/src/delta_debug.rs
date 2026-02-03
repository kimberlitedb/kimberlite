//! Delta debugging (ddmin algorithm) for test case minimization.
//!
//! This module implements Zeller's ddmin algorithm to automatically minimize
//! failing test cases by removing irrelevant events.

use std::collections::HashMap;

use crate::dependency::DependencyAnalyzer;
use crate::event_log::{LoggedEvent, ReproBundle};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for delta debugging.
#[derive(Debug, Clone)]
pub struct DeltaConfig {
    /// Maximum minimization iterations.
    pub max_iterations: usize,
    /// Initial chunk size for ddmin.
    pub initial_granularity: usize,
    /// Preserve event ordering.
    pub preserve_order: bool,
}

impl Default for DeltaConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            initial_granularity: 8,
            preserve_order: true,
        }
    }
}

// ============================================================================
// Minimization Result
// ============================================================================

/// Result of delta debugging minimization.
#[derive(Debug, Clone)]
pub struct MinimizationResult {
    /// Original number of events.
    pub original_events: usize,
    /// Minimized number of events.
    pub minimized_events: usize,
    /// Reduction percentage.
    pub reduction_pct: f64,
    /// Number of iterations.
    pub iterations: usize,
    /// Number of test runs.
    pub test_runs: usize,
    /// Minimized bundle.
    pub minimized_bundle: ReproBundle,
}

// ============================================================================
// Delta Debugger
// ============================================================================

/// Delta debugging minimizer using ddmin algorithm.
pub struct DeltaDebugger {
    /// Original bundle.
    bundle: ReproBundle,
    /// Configuration.
    config: DeltaConfig,
    /// Test cache: event subset → test result.
    test_cache: HashMap<Vec<u64>, TestResult>,
    /// Dependency analyzer.
    analyzer: DependencyAnalyzer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestResult {
    /// Test passed (failure not reproduced).
    Pass,
    /// Test failed (failure reproduced).
    Fail,
}

impl DeltaDebugger {
    /// Creates a new delta debugger.
    pub fn new(bundle: ReproBundle, config: DeltaConfig) -> Result<Self, DeltaError> {
        let events = bundle.event_log.as_ref()
            .ok_or(DeltaError::NoEventLog)?;

        let analyzer = DependencyAnalyzer::analyze(events);

        Ok(Self {
            bundle,
            config,
            test_cache: HashMap::new(),
            analyzer,
        })
    }

    /// Runs delta debugging to minimize the event sequence.
    #[allow(clippy::too_many_lines)] // Complex algorithm, splitting would reduce clarity
    #[allow(clippy::mut_range_bound)] // Granularity changes during convergence
    pub fn minimize(&mut self) -> Result<MinimizationResult, DeltaError> {
        let original_events = self.analyzer.len();

        if original_events == 0 {
            return Err(DeltaError::NoEvents);
        }

        // PRECONDITION (after error handling)
        assert!(
            original_events > 0,
            "minimize requires non-empty event set"
        );

        println!("Starting delta debugging minimization...");
        println!("Original events: {}", original_events);
        println!("Initial granularity: {}", self.config.initial_granularity);

        // Start with all events
        let mut current_set: Vec<u64> = (0..original_events as u64).collect();
        let mut granularity = self.config.initial_granularity;
        let mut iterations = 0;
        let mut test_runs = 0;

        'outer: while granularity <= current_set.len() {
            // LOOP INVARIANT
            assert!(
                granularity >= self.config.initial_granularity,
                "granularity underflow: {granularity}"
            );
            assert!(
                !current_set.is_empty(),
                "current set became empty during minimization"
            );
            if iterations >= self.config.max_iterations {
                println!("\nReached maximum iterations ({})", self.config.max_iterations);
                break;
            }

            println!(
                "\nIteration {}: granularity={}, events={}",
                iterations,
                granularity,
                current_set.len()
            );

            // Try removing chunks
            let chunk_size = current_set.len() / granularity;
            if chunk_size == 0 {
                break;
            }

            for chunk_idx in 0..granularity {
                let chunk_start = chunk_idx * chunk_size;
                let chunk_end = if chunk_idx == granularity - 1 {
                    current_set.len()
                } else {
                    (chunk_idx + 1) * chunk_size
                };

                // Create candidate set without this chunk
                let mut candidate: Vec<u64> = Vec::new();
                candidate.extend_from_slice(&current_set[..chunk_start]);
                if chunk_end < current_set.len() {
                    candidate.extend_from_slice(&current_set[chunk_end..]);
                }

                println!(
                    "  Trying to remove chunk [{}, {}), {} events remaining",
                    chunk_start,
                    chunk_end,
                    candidate.len()
                );

                test_runs += 1;
                let result = self.test_event_subset(&candidate)?;

                match result {
                    TestResult::Fail => {
                        println!("    ✓ Chunk removed (failure still reproduced)");
                        let old_len = current_set.len();
                        current_set = candidate;

                        // PROGRESS CHECK: set size must decrease
                        assert!(
                            current_set.len() < old_len,
                            "ddmin failed to reduce set size: old={old_len}, new={}",
                            current_set.len()
                        );

                        // Reset granularity and restart outer loop
                        // (clippy warns about mutating range bound, but this is correct
                        // since we're explicitly restarting the outer loop with continue)
                        #[allow(clippy::mut_range_bound)]
                        {
                            granularity = self.config.initial_granularity;
                        }
                        iterations += 1;
                        continue 'outer;
                    }
                    TestResult::Pass => {
                        println!("    ✗ Chunk needed (failure disappeared)");
                    }
                }
            }

            // No chunks removed, increase granularity
            if granularity >= current_set.len() {
                println!("\nCannot subdivide further - minimization complete");
                break;
            }

            let old_granularity = granularity;
            granularity = (granularity * 2).min(current_set.len());

            // POSTCONDITION: granularity increased
            assert!(
                granularity > old_granularity || granularity == current_set.len(),
                "granularity failed to increase: old={old_granularity}, new={granularity}"
            );

            iterations += 1;
        }

        println!("\n═══════════════════════════════════════════");
        println!("Delta Debugging Complete");
        println!("═══════════════════════════════════════════");
        println!("Original:  {} events", original_events);
        println!("Minimized: {} events", current_set.len());
        println!(
            "Reduction: {:.1}%",
            (1.0 - current_set.len() as f64 / original_events as f64) * 100.0
        );
        println!("Iterations: {}", iterations);
        println!("Test runs:  {}", test_runs);

        // Create minimized bundle
        let minimized_bundle = self.create_minimized_bundle(&current_set)?;

        Ok(MinimizationResult {
            original_events,
            minimized_events: current_set.len(),
            reduction_pct: (1.0 - current_set.len() as f64 / original_events as f64) * 100.0,
            iterations,
            test_runs,
            minimized_bundle,
        })
    }

    /// Tests an event subset to see if failure is still reproduced.
    #[allow(clippy::unnecessary_wraps)] // Result for future error handling
    fn test_event_subset(&mut self, event_indices: &[u64]) -> Result<TestResult, DeltaError> {
        // Check cache
        if let Some(result) = self.test_cache.get(event_indices) {
            return Ok(*result);
        }

        // Create filtered event log
        let original_log = self.bundle.event_log.as_ref().unwrap();
        let _filtered_log: Vec<LoggedEvent> = event_indices
            .iter()
            .filter_map(|&idx| original_log.get(idx as usize).cloned())
            .collect();

        // Check if the failure event is still in the subset
        let failure_event_id = self.bundle.failure.failed_at_event;
        let has_failure_event = event_indices.contains(&failure_event_id);

        // Simplified test: failure reproduced if we include the failing event
        // Real implementation would replay and check invariants
        let result = if has_failure_event {
            TestResult::Fail
        } else {
            TestResult::Pass
        };

        // Cache result
        self.test_cache.insert(event_indices.to_vec(), result);

        Ok(result)
    }

    /// Creates minimized bundle with selected events.
    #[allow(clippy::unnecessary_wraps)] // Result for future error handling
    fn create_minimized_bundle(&self, event_indices: &[u64]) -> Result<ReproBundle, DeltaError> {
        let original_log = self.bundle.event_log.as_ref().unwrap();
        let minimized_log: Vec<LoggedEvent> = event_indices
            .iter()
            .filter_map(|&idx| original_log.get(idx as usize).cloned())
            .collect();

        let mut minimized = self.bundle.clone();
        minimized.event_log = Some(minimized_log);

        Ok(minimized)
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Errors during delta debugging.
#[derive(Debug, thiserror::Error)]
pub enum DeltaError {
    #[error("Bundle has no event log")]
    NoEventLog,

    #[error("Event log is empty")]
    NoEvents,

    #[error("Minimization failed: {0}")]
    MinimizationFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{Decision, FailureInfo};

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
    fn delta_debugger_basic() {
        let bundle = create_test_bundle(100, 50);
        let config = DeltaConfig::default();

        let mut debugger = DeltaDebugger::new(bundle, config).unwrap();
        let result = debugger.minimize().unwrap();

        // Should remove many irrelevant events
        assert!(result.minimized_events < result.original_events);
        assert!(result.reduction_pct > 0.0);
    }

    #[test]
    fn delta_debugger_preserves_failure() {
        let bundle = create_test_bundle(50, 25);
        let config = DeltaConfig::default();

        let mut debugger = DeltaDebugger::new(bundle, config).unwrap();
        let result = debugger.minimize().unwrap();

        // Minimized bundle should still contain the failure event
        let has_failure = result
            .minimized_bundle
            .event_log
            .unwrap()
            .iter()
            .any(|e| e.event_id == 25);
        assert!(has_failure);
    }

    #[test]
    fn delta_debugger_no_events_error() {
        let bundle = create_test_bundle(0, 0);
        let config = DeltaConfig::default();

        let result = DeltaDebugger::new(bundle, config);
        // Should create debugger successfully
        assert!(result.is_ok());

        // But minimization should fail
        let mut debugger = result.unwrap();
        let min_result = debugger.minimize();
        assert!(matches!(min_result, Err(DeltaError::NoEvents)));
    }
}
