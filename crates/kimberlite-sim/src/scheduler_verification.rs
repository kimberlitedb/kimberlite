//! Scheduler verification and fairness tracking.
//!
//! This module provides tools to verify that the VOPR scheduler is fair and
//! doesn't starve any nodes or code paths.
//!
//! ## Design
//!
//! Tracks:
//! - Which nodes were runnable but not scheduled (fairness)
//! - How long nodes waited before being scheduled (starvation detection)
//! - Distribution of scheduling decisions across nodes
//! - Progress monotonicity (events always increase)
//!
//! ## Usage
//!
//! ```ignore
//! let mut tracker = SchedulerTracker::new(3); // 3 nodes
//!
//! // Record scheduling decision
//! tracker.record_schedule(0, time_ns, vec![0, 1, 2]); // Node 0 selected, all 3 runnable
//!
//! // Check for violations
//! if let Some(violation) = tracker.check_fairness() {
//!     panic!("Fairness violation: {}", violation);
//! }
//! ```

use serde::{Deserialize, Serialize};

// ============================================================================
// Scheduler Tracking
// ============================================================================

/// Tracks scheduler decisions for fairness and starvation detection.
#[derive(Debug)]
pub struct SchedulerTracker {
    /// Number of nodes being tracked.
    node_count: usize,

    /// How many times each node was scheduled.
    schedule_counts: Vec<u64>,

    /// Last time each node was scheduled (ns).
    last_scheduled_ns: Vec<Option<u64>>,

    /// Total events processed (for progress monotonicity).
    pub total_events: u64,

    /// Maximum starvation window allowed (ns).
    max_starvation_ns: u64,

    /// History of scheduling decisions (for analysis).
    decision_history: Vec<SchedulerDecision>,

    /// Maximum history size (bounded memory).
    pub max_history_size: usize,
}

/// A single scheduler decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerDecision {
    /// Which node was selected.
    pub selected_node: u64,

    /// All runnable nodes at decision time.
    pub runnable_nodes: Vec<u64>,

    /// Simulation time when decision was made (ns).
    pub time_ns: u64,

    /// Event number.
    pub event_id: u64,
}

impl SchedulerTracker {
    /// Creates a new scheduler tracker.
    ///
    /// # Arguments
    ///
    /// * `node_count` - Number of nodes in the simulation
    pub fn new(node_count: usize) -> Self {
        Self {
            node_count,
            schedule_counts: vec![0; node_count],
            last_scheduled_ns: vec![None; node_count],
            total_events: 0,
            max_starvation_ns: 1_000_000_000, // 1 second default
            decision_history: Vec::new(),
            max_history_size: 10_000,
        }
    }

    /// Records a scheduling decision.
    ///
    /// # Arguments
    ///
    /// * `selected_node` - Node that was scheduled
    /// * `time_ns` - Current simulation time
    /// * `runnable_nodes` - All nodes that were runnable
    pub fn record_schedule(&mut self, selected_node: u64, time_ns: u64, runnable_nodes: Vec<u64>) {
        let node_idx = selected_node as usize;
        if node_idx >= self.node_count {
            return; // Invalid node ID
        }

        // Update schedule count
        self.schedule_counts[node_idx] += 1;

        // Update last scheduled time
        self.last_scheduled_ns[node_idx] = Some(time_ns);

        // Increment total events
        self.total_events += 1;

        // Record decision in history
        if self.decision_history.len() < self.max_history_size {
            self.decision_history.push(SchedulerDecision {
                selected_node,
                runnable_nodes,
                time_ns,
                event_id: self.total_events,
            });
        }
    }

    /// Checks for fairness violations.
    ///
    /// Returns a description of the violation if one is detected.
    pub fn check_fairness(&self) -> Option<String> {
        // Check if any node has never been scheduled
        for (node_id, &count) in self.schedule_counts.iter().enumerate() {
            if count == 0 && self.total_events > 100 {
                // Only report if we've run enough events
                return Some(format!("Node {} has never been scheduled", node_id));
            }
        }

        // Check for severe imbalance (one node gets >90% of scheduling)
        if self.total_events > 1000 {
            let max_count = *self.schedule_counts.iter().max().unwrap_or(&0);
            let total = self.schedule_counts.iter().sum::<u64>();

            if total > 0 {
                let max_percentage = (max_count as f64 / total as f64) * 100.0;
                if max_percentage > 90.0 && self.node_count > 1 {
                    return Some(format!(
                        "Severe scheduling imbalance: one node got {:.1}% of scheduling decisions",
                        max_percentage
                    ));
                }
            }
        }

        None
    }

    /// Checks for starvation violations.
    ///
    /// Returns a description of the violation if one is detected.
    pub fn check_starvation(&self, current_time_ns: u64) -> Option<String> {
        for (node_id, last_scheduled) in self.last_scheduled_ns.iter().enumerate() {
            if let Some(last_ns) = last_scheduled {
                let time_since_last = current_time_ns.saturating_sub(*last_ns);
                if time_since_last > self.max_starvation_ns {
                    return Some(format!(
                        "Node {} starved for {} ns (max allowed: {} ns)",
                        node_id, time_since_last, self.max_starvation_ns
                    ));
                }
            }
        }

        None
    }

    /// Returns scheduling statistics.
    pub fn stats(&self) -> SchedulerStats {
        let total_schedules: u64 = self.schedule_counts.iter().sum();

        let percentages: Vec<f64> = if total_schedules > 0 {
            self.schedule_counts
                .iter()
                .map(|&count| (count as f64 / total_schedules as f64) * 100.0)
                .collect()
        } else {
            vec![0.0; self.node_count]
        };

        let min_count = *self.schedule_counts.iter().min().unwrap_or(&0);
        let max_count = *self.schedule_counts.iter().max().unwrap_or(&0);
        let imbalance_ratio = if min_count > 0 {
            max_count as f64 / min_count as f64
        } else {
            f64::INFINITY
        };

        SchedulerStats {
            node_count: self.node_count,
            total_schedules,
            schedule_counts: self.schedule_counts.clone(),
            schedule_percentages: percentages,
            min_schedules: min_count,
            max_schedules: max_count,
            imbalance_ratio,
            total_events: self.total_events,
        }
    }

    /// Returns the scheduling decision history.
    pub fn decision_history(&self) -> &[SchedulerDecision] {
        &self.decision_history
    }

    /// Clears all tracking data.
    pub fn clear(&mut self) {
        self.schedule_counts.fill(0);
        self.last_scheduled_ns.fill(None);
        self.total_events = 0;
        self.decision_history.clear();
    }

    /// Sets the maximum allowed starvation window.
    pub fn set_max_starvation_ns(&mut self, max_ns: u64) {
        self.max_starvation_ns = max_ns;
    }
}

/// Scheduler fairness and starvation statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerStats {
    /// Number of nodes tracked.
    pub node_count: usize,

    /// Total scheduling decisions made.
    pub total_schedules: u64,

    /// How many times each node was scheduled.
    pub schedule_counts: Vec<u64>,

    /// Percentage of scheduling decisions for each node.
    pub schedule_percentages: Vec<f64>,

    /// Minimum number of times any node was scheduled.
    pub min_schedules: u64,

    /// Maximum number of times any node was scheduled.
    pub max_schedules: u64,

    /// Ratio of max/min schedules (fairness metric).
    pub imbalance_ratio: f64,

    /// Total events processed.
    pub total_events: u64,
}

impl SchedulerStats {
    /// Returns true if scheduling appears fair.
    ///
    /// Fair means:
    /// - All nodes have been scheduled at least once
    /// - Imbalance ratio < 5.0 (no node gets >5x more than another)
    pub fn is_fair(&self) -> bool {
        self.min_schedules > 0 && self.imbalance_ratio < 5.0
    }

    /// Returns a human-readable summary.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "Scheduler Stats ({} nodes, {} total schedules):\n",
            self.node_count, self.total_schedules
        );

        for (node_id, (&count, &pct)) in self
            .schedule_counts
            .iter()
            .zip(&self.schedule_percentages)
            .enumerate()
        {
            s.push_str(&format!(
                "  Node {}: {} schedules ({:.1}%)\n",
                node_id, count, pct
            ));
        }

        s.push_str(&format!(
            "  Imbalance ratio: {:.2} (min: {}, max: {})\n",
            self.imbalance_ratio, self.min_schedules, self.max_schedules
        ));

        s.push_str(&format!("  Fair: {}\n", self.is_fair()));

        s
    }
}

// ============================================================================
// Progress Monotonicity Checker
// ============================================================================

/// Checks that simulation makes forward progress.
#[derive(Debug)]
pub struct ProgressMonitor {
    /// Last event count.
    last_event_count: u64,

    /// Last time (ns).
    last_time_ns: u64,

    /// Number of checks without progress.
    stall_count: u64,

    /// Maximum stalls allowed before reporting violation.
    pub max_stalls: u64,
}

impl ProgressMonitor {
    /// Creates a new progress monitor.
    pub fn new() -> Self {
        Self {
            last_event_count: 0,
            last_time_ns: 0,
            stall_count: 0,
            max_stalls: 1000, // Allow up to 1000 checks without progress
        }
    }

    /// Checks for forward progress.
    ///
    /// Returns true if progress was made, false if stalled.
    pub fn check_progress(&mut self, event_count: u64, time_ns: u64) -> bool {
        let made_progress = event_count > self.last_event_count || time_ns > self.last_time_ns;

        if made_progress {
            self.stall_count = 0;
        } else {
            self.stall_count += 1;
        }

        self.last_event_count = event_count;
        self.last_time_ns = time_ns;

        made_progress
    }

    /// Returns true if simulation appears to be stalled.
    pub fn is_stalled(&self) -> bool {
        self.stall_count >= self.max_stalls
    }

    /// Returns the number of consecutive stalls.
    pub fn stall_count(&self) -> u64 {
        self.stall_count
    }
}

impl Default for ProgressMonitor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_tracker_basic() {
        let mut tracker = SchedulerTracker::new(3);

        // Record some scheduling decisions
        tracker.record_schedule(0, 1000, vec![0, 1, 2]);
        tracker.record_schedule(1, 2000, vec![0, 1, 2]);
        tracker.record_schedule(2, 3000, vec![0, 1, 2]);

        let stats = tracker.stats();
        assert_eq!(stats.total_schedules, 3);
        assert_eq!(stats.schedule_counts, vec![1, 1, 1]);
        assert!(stats.is_fair());
    }

    #[test]
    fn test_scheduler_tracker_imbalance() {
        let mut tracker = SchedulerTracker::new(3);

        // Node 0 gets scheduled 90% of the time
        for _ in 0..90 {
            tracker.record_schedule(0, 1000, vec![0, 1, 2]);
        }
        for _ in 0..5 {
            tracker.record_schedule(1, 1000, vec![0, 1, 2]);
        }
        for _ in 0..5 {
            tracker.record_schedule(2, 1000, vec![0, 1, 2]);
        }

        let stats = tracker.stats();
        assert_eq!(stats.total_schedules, 100);
        assert!(!stats.is_fair()); // Imbalance too high
        assert!(stats.imbalance_ratio > 5.0);
    }

    #[test]
    fn test_fairness_violation_detection() {
        let mut tracker = SchedulerTracker::new(3);

        // Only schedule node 0
        for i in 0..200 {
            tracker.record_schedule(0, i * 1000, vec![0, 1, 2]);
        }

        // Should detect fairness violation
        let violation = tracker.check_fairness();
        assert!(violation.is_some());
        assert!(violation.unwrap().contains("never been scheduled"));
    }

    #[test]
    fn test_starvation_detection() {
        let mut tracker = SchedulerTracker::new(3);
        tracker.set_max_starvation_ns(1_000_000); // 1ms

        // Schedule node 0 at t=0
        tracker.record_schedule(0, 0, vec![0, 1]);

        // Check at t=2ms (starved for 2ms)
        let violation = tracker.check_starvation(2_000_000);
        assert!(violation.is_some());
        assert!(violation.unwrap().contains("starved"));
    }

    #[test]
    fn test_progress_monitor_forward_progress() {
        let mut monitor = ProgressMonitor::new();

        // Make progress
        assert!(monitor.check_progress(1, 1000));
        assert!(monitor.check_progress(2, 2000));
        assert!(monitor.check_progress(3, 3000));

        assert!(!monitor.is_stalled());
        assert_eq!(monitor.stall_count(), 0);
    }

    #[test]
    fn test_progress_monitor_stall_detection() {
        let mut monitor = ProgressMonitor::new();
        monitor.max_stalls = 10;

        // Stall for 11 iterations
        for _ in 0..11 {
            monitor.check_progress(0, 0);
        }

        assert!(monitor.is_stalled());
        assert_eq!(monitor.stall_count(), 11);
    }

    #[test]
    fn test_scheduler_stats_summary() {
        let mut tracker = SchedulerTracker::new(2);

        for _ in 0..60 {
            tracker.record_schedule(0, 1000, vec![0, 1]);
        }
        for _ in 0..40 {
            tracker.record_schedule(1, 1000, vec![0, 1]);
        }

        let stats = tracker.stats();
        let summary = stats.summary();

        assert!(summary.contains("Node 0: 60"));
        assert!(summary.contains("Node 1: 40"));
        assert!(summary.contains("60.0%"));
        assert!(summary.contains("40.0%"));
    }

    #[test]
    fn test_decision_history_bounded() {
        let mut tracker = SchedulerTracker::new(2);
        tracker.max_history_size = 100;

        // Record 200 decisions
        for i in 0..200 {
            tracker.record_schedule((i % 2) as u64, i * 1000, vec![0, 1]);
        }

        // Should only keep 100
        assert_eq!(tracker.decision_history().len(), 100);
    }
}
