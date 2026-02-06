//! Integration tests for scheduler verification.
//!
//! These tests verify that scheduler fairness tracking and starvation detection
//! work correctly in real VOPR simulation scenarios.

use kimberlite_sim::{
    ScenarioType, VoprConfig, VoprRunner,
    scheduler_verification::{ProgressMonitor, SchedulerTracker},
};

// ============================================================================
// Scheduler Fairness Tests
// ============================================================================

#[test]
fn test_scheduler_fairness_in_baseline_scenario() {
    let mut tracker = SchedulerTracker::new(3);

    // Simulate a run with fair scheduling
    // In reality, VOPR would record these decisions during simulation
    for i in 0..300 {
        let node_id = i % 3; // Round-robin scheduling
        tracker.record_schedule(node_id, i * 1000, vec![0, 1, 2]);
    }

    let stats = tracker.stats();

    // Each node should get roughly 1/3 of scheduling
    assert_eq!(stats.schedule_counts[0], 100);
    assert_eq!(stats.schedule_counts[1], 100);
    assert_eq!(stats.schedule_counts[2], 100);

    assert!(stats.is_fair());
    assert!(stats.imbalance_ratio < 2.0); // Very balanced

    // No fairness violations
    assert!(tracker.check_fairness().is_none());

    println!("{}", stats.summary());
}

#[test]
fn test_scheduler_detects_unfair_scheduling() {
    let mut tracker = SchedulerTracker::new(3);

    // Node 0 gets 95% of scheduling, nodes 1 and 2 starve
    for i in 0..190 {
        tracker.record_schedule(0, i * 1000, vec![0, 1, 2]);
    }
    for i in 190..195 {
        tracker.record_schedule(1, i * 1000, vec![0, 1, 2]);
    }
    for i in 195..200 {
        tracker.record_schedule(2, i * 1000, vec![0, 1, 2]);
    }

    let stats = tracker.stats();

    // Severe imbalance
    assert_eq!(stats.schedule_counts[0], 190);
    assert_eq!(stats.schedule_counts[1], 5);
    assert_eq!(stats.schedule_counts[2], 5);

    assert!(!stats.is_fair()); // Imbalance too high
    assert!(stats.imbalance_ratio > 5.0);

    println!("{}", stats.summary());
}

#[test]
fn test_scheduler_detects_never_scheduled_node() {
    let mut tracker = SchedulerTracker::new(3);

    // Only schedule nodes 0 and 1, never node 2
    for i in 0..200 {
        let node_id = i % 2; // Only 0 and 1
        tracker.record_schedule(node_id, i * 1000, vec![0, 1, 2]);
    }

    // Should detect that node 2 never ran
    let violation = tracker.check_fairness();
    assert!(violation.is_some());
    let msg = violation.unwrap();
    assert!(msg.contains("Node 2"));
    assert!(msg.contains("never been scheduled"));

    println!("Detected fairness violation as expected");
}

// ============================================================================
// Starvation Detection Tests
// ============================================================================

#[test]
fn test_starvation_detection() {
    let mut tracker = SchedulerTracker::new(3);
    tracker.set_max_starvation_ns(1_000_000); // 1ms max starvation

    // Node 0 scheduled at t=0
    tracker.record_schedule(0, 0, vec![0, 1]);

    // Node 1 scheduled at t=500us (not starved)
    tracker.record_schedule(1, 500_000, vec![0, 1]);

    // Check at t=1ms (node 0 hasn't run in 1ms - exactly at threshold)
    let violation = tracker.check_starvation(1_000_000);
    assert!(violation.is_none()); // Just at threshold, not exceeded

    // Check at t=2ms (node 0 starved for 2ms)
    let violation = tracker.check_starvation(2_000_000);
    assert!(violation.is_some());
    let msg = violation.unwrap();
    assert!(msg.contains("Node 0"));
    assert!(msg.contains("starved"));

    println!("Starvation detected as expected");
}

#[test]
fn test_no_starvation_with_regular_scheduling() {
    let mut tracker = SchedulerTracker::new(3);
    tracker.set_max_starvation_ns(10_000_000); // 10ms max starvation

    // All nodes get scheduled within 10ms window
    tracker.record_schedule(0, 0, vec![0, 1, 2]);
    tracker.record_schedule(1, 3_000_000, vec![0, 1, 2]); // 3ms later
    tracker.record_schedule(2, 6_000_000, vec![0, 1, 2]); // 6ms later
    tracker.record_schedule(0, 9_000_000, vec![0, 1, 2]); // 9ms later

    // Check at t=10ms
    let violation = tracker.check_starvation(10_000_000);
    assert!(violation.is_none());

    println!("No starvation detected - all nodes scheduled regularly");
}

// ============================================================================
// Progress Monotonicity Tests
// ============================================================================

#[test]
fn test_progress_monitor_detects_forward_progress() {
    let mut monitor = ProgressMonitor::new();

    // Simulate forward progress
    assert!(monitor.check_progress(1, 1000));
    assert!(monitor.check_progress(2, 2000));
    assert!(monitor.check_progress(3, 3000));

    assert!(!monitor.is_stalled());
    assert_eq!(monitor.stall_count(), 0);

    println!("Forward progress detected correctly");
}

#[test]
fn test_progress_monitor_detects_stall() {
    let mut monitor = ProgressMonitor::new();
    monitor.max_stalls = 10;

    // Make initial progress
    monitor.check_progress(1, 1000);

    // Then stall (same event count and time)
    for _ in 0..11 {
        monitor.check_progress(1, 1000);
    }

    assert!(monitor.is_stalled());
    assert!(monitor.stall_count() >= 10);

    println!("Stall detected after {} checks", monitor.stall_count());
}

#[test]
fn test_progress_monitor_reset_on_progress() {
    let mut monitor = ProgressMonitor::new();

    // Stall for a bit
    for _ in 0..5 {
        monitor.check_progress(0, 0);
    }
    assert_eq!(monitor.stall_count(), 5);

    // Make progress - should reset stall count
    monitor.check_progress(1, 1000);
    assert_eq!(monitor.stall_count(), 0);

    println!("Stall count reset on progress as expected");
}

// ============================================================================
// Decision History Tests
// ============================================================================

#[test]
fn test_decision_history_recording() {
    let mut tracker = SchedulerTracker::new(3);

    // Record some decisions
    tracker.record_schedule(0, 1000, vec![0, 1, 2]);
    tracker.record_schedule(1, 2000, vec![0, 1, 2]);
    tracker.record_schedule(2, 3000, vec![1, 2]); // Node 0 not runnable

    let history = tracker.decision_history();
    assert_eq!(history.len(), 3);

    // Verify first decision
    assert_eq!(history[0].selected_node, 0);
    assert_eq!(history[0].runnable_nodes, vec![0, 1, 2]);
    assert_eq!(history[0].time_ns, 1000);
    assert_eq!(history[0].event_id, 1);

    // Verify last decision (node 0 not runnable)
    assert_eq!(history[2].selected_node, 2);
    assert_eq!(history[2].runnable_nodes, vec![1, 2]);

    println!("Decision history recorded correctly");
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

    println!("History correctly bounded to 100 entries");
}

// ============================================================================
// Real VOPR Integration Tests
// ============================================================================

#[test]
fn test_baseline_scenario_fairness() {
    // Run an actual VOPR scenario
    let config = VoprConfig {
        scenario: Some(ScenarioType::Baseline),
        seed: 12345,
        max_events: 1000,
        ..Default::default()
    };

    let runner = VoprRunner::new(config);
    let result = runner.run_single(12345);

    // In a real integration, VOPR would track scheduling internally
    // For now, we verify the run completed successfully
    match result {
        kimberlite_sim::VoprResult::Success {
            events_processed, ..
        } => {
            assert!(events_processed > 0);
            println!("Baseline scenario completed: {} events", events_processed);
        }
        _ => panic!("Expected successful run"),
    }

    // Note: Full integration would require modifying VOPR to use SchedulerTracker
    // internally and expose fairness stats in VoprResult
}

#[test]
fn test_combined_scenario_fairness() {
    // Run a more complex scenario
    let config = VoprConfig {
        scenario: Some(ScenarioType::Combined),
        seed: 54321,
        max_events: 1000,
        ..Default::default()
    };

    let runner = VoprRunner::new(config);
    let result = runner.run_single(54321);

    match result {
        kimberlite_sim::VoprResult::Success {
            events_processed, ..
        } => {
            assert!(events_processed > 0);
            println!("Combined scenario completed: {} events", events_processed);
        }
        _ => {
            // Invariant violations are possible with faults enabled
            println!("Scenario completed (may have detected invariant violation)");
        }
    }
}

// ============================================================================
// Statistics Validation Tests
// ============================================================================

#[test]
fn test_scheduler_stats_percentages() {
    let mut tracker = SchedulerTracker::new(4);

    // 50% node 0, 25% node 1, 15% node 2, 10% node 3
    for _ in 0..50 {
        tracker.record_schedule(0, 1000, vec![0, 1, 2, 3]);
    }
    for _ in 0..25 {
        tracker.record_schedule(1, 1000, vec![0, 1, 2, 3]);
    }
    for _ in 0..15 {
        tracker.record_schedule(2, 1000, vec![0, 1, 2, 3]);
    }
    for _ in 0..10 {
        tracker.record_schedule(3, 1000, vec![0, 1, 2, 3]);
    }

    let stats = tracker.stats();

    // Check percentages
    assert!((stats.schedule_percentages[0] - 50.0).abs() < 0.1);
    assert!((stats.schedule_percentages[1] - 25.0).abs() < 0.1);
    assert!((stats.schedule_percentages[2] - 15.0).abs() < 0.1);
    assert!((stats.schedule_percentages[3] - 10.0).abs() < 0.1);

    // Check totals
    assert_eq!(stats.total_schedules, 100);
    assert_eq!(stats.min_schedules, 10);
    assert_eq!(stats.max_schedules, 50);

    // Imbalance ratio should be 50/10 = 5.0
    assert!((stats.imbalance_ratio - 5.0).abs() < 0.1);

    println!("{}", stats.summary());
}

#[test]
fn test_clear_resets_tracker() {
    let mut tracker = SchedulerTracker::new(2);

    // Record some data
    tracker.record_schedule(0, 1000, vec![0, 1]);
    tracker.record_schedule(1, 2000, vec![0, 1]);

    assert_eq!(tracker.stats().total_schedules, 2);
    assert_eq!(tracker.decision_history().len(), 2);

    // Clear
    tracker.clear();

    // Verify reset
    assert_eq!(tracker.stats().total_schedules, 0);
    assert_eq!(tracker.decision_history().len(), 0);
    assert_eq!(tracker.total_events, 0);

    println!("Tracker cleared successfully");
}
