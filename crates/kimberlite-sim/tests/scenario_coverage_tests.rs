//! Scenario coverage audit tests.
//!
//! These tests verify that VOPR scenarios actually exercise their intended fault
//! types and achieve minimum effectiveness thresholds.

use kimberlite_sim::{
    ScenarioType, VoprConfig, VoprRunner,
    instrumentation::fault_registry::{get_fault_registry, reset_fault_registry},
};

/// Helper to run a scenario and get fault registry
fn run_scenario_and_get_registry(
    scenario: ScenarioType,
    iterations: u64,
    seed: u64,
) -> kimberlite_sim::instrumentation::fault_registry::FaultRegistry {
    // Reset fault registry before test
    reset_fault_registry();

    let config = VoprConfig {
        scenario: Some(scenario),
        seed,
        max_events: iterations,
        ..Default::default()
    };

    let runner = VoprRunner::new(config);
    let _result = runner.run_single(seed);

    // Get the fault registry after the run
    get_fault_registry()
}

// ============================================================================
// Combined Scenario Tests
// ============================================================================

#[test]
fn test_combined_scenario_exercises_multiple_fault_types() {
    let registry = run_scenario_and_get_registry(ScenarioType::Combined, 10000, 12345);

    // Combined should exercise multiple fault types
    let fault_types = vec![
        ("network.partition", "partitions"),
        ("network.drop", "drops"),
        ("storage.corruption", "corruptions"),
        ("storage.crash_data_loss", "crash data loss"),
    ];

    let mut exercised_count = 0;

    for (fault_key, fault_name) in &fault_types {
        let applied = registry.get_applied(fault_key);
        let observed = registry.get_observed(fault_key);

        if observed > 0 {
            exercised_count += 1;
            println!(
                "✓ Combined scenario exercised {fault_name}: {observed} observed ({applied}applied)"
            );
        }
    }

    // Combined should exercise at least 2 different fault types
    assert!(
        exercised_count >= 2,
        "Combined scenario only exercised {exercised_count} fault types, expected at least 2"
    );
}

#[test]
fn test_combined_scenario_fault_effectiveness() {
    let registry = run_scenario_and_get_registry(ScenarioType::Combined, 10000, 54321);

    let report = registry.effectiveness_report();

    println!("Combined Scenario Effectiveness Report:");
    println!("  Partition: {:.1}%", report.partition);
    println!("  Corruption: {:.1}%", report.corruption);
    println!("  Crash: {:.1}%", report.crash);
    println!("  Drop: {:.1}%", report.drop);
    println!("  Delay: {:.1}%", report.delay);

    // At least one fault type should have some effectiveness
    let max_effectiveness = report
        .partition
        .max(report.corruption)
        .max(report.crash)
        .max(report.drop)
        .max(report.delay);

    assert!(
        max_effectiveness > 0.0,
        "Combined scenario had no observable fault effects"
    );
}

// ============================================================================
// Baseline Scenario Tests
// ============================================================================

#[test]
fn test_baseline_scenario_has_minimal_faults() {
    let registry = run_scenario_and_get_registry(ScenarioType::Baseline, 1000, 42);

    // Baseline should have very few or no fault effects
    let total_faults_observed = registry
        .all_fault_points()
        .values()
        .map(|fp| fp.observed)
        .sum::<u64>();

    println!(
        "Baseline scenario observed {total_faults_observed} total fault effects"
    );

    // Baseline might have some delays but should have minimal faults
    assert!(
        total_faults_observed < 100,
        "Baseline scenario observed too many faults ({total_faults_observed})"
    );
}

// ============================================================================
// Gray Failures Scenario Tests
// ============================================================================

#[test]
fn test_gray_failures_exercises_faults() {
    let registry = run_scenario_and_get_registry(ScenarioType::GrayFailures, 5000, 99999);

    // Gray failures should exercise some fault types
    let total_faults_applied = registry
        .all_fault_points()
        .values()
        .map(|fp| fp.applied)
        .sum::<u64>();

    assert!(
        total_faults_applied > 0,
        "GrayFailures scenario didn't apply any faults"
    );

    println!(
        "GrayFailures scenario applied {total_faults_applied} faults"
    );
}

// ============================================================================
// Fault Registry API Tests
// ============================================================================

#[test]
fn test_fault_registry_tracks_counters() {
    let registry = run_scenario_and_get_registry(ScenarioType::Combined, 1000, 12345);

    // Check that we're tracking applied and observed
    for (key, fault_point) in registry.all_fault_points() {
        if fault_point.observed > 0 {
            println!(
                "{}: applied={}, observed={}, effectiveness={:.1}%",
                key,
                fault_point.applied,
                fault_point.observed,
                registry.effectiveness(key)
            );

            // Observed should never exceed applied
            assert!(
                fault_point.observed <= fault_point.applied,
                "{}: Observed ({}) cannot exceed applied ({})",
                key,
                fault_point.observed,
                fault_point.applied
            );
        }
    }
}

#[test]
fn test_effectiveness_report_format() {
    let registry = run_scenario_and_get_registry(ScenarioType::Combined, 5000, 77777);

    let report = registry.effectiveness_report();

    // All percentages should be in range [0.0, 100.0]
    let values = vec![
        ("partition", report.partition),
        ("corruption", report.corruption),
        ("crash", report.crash),
        ("slow_disk", report.slow_disk),
        ("drop", report.drop),
        ("delay", report.delay),
    ];

    for (name, value) in &values {
        assert!(
            (0.0..=100.0).contains(value),
            "Effectiveness for {name} is out of range [0, 100]: {value}"
        );
    }
}

// ============================================================================
// Determinism Tests
// ============================================================================

#[test]
fn test_scenario_determinism_with_same_seed() {
    // Run same scenario twice with same seed
    let registry1 = run_scenario_and_get_registry(ScenarioType::Combined, 1000, 42);
    let registry2 = run_scenario_and_get_registry(ScenarioType::Combined, 1000, 42);

    // Get all fault keys from registry1
    let keys: Vec<String> = registry1.all_fault_points().keys().cloned().collect();

    for key in &keys {
        let applied1 = registry1.get_applied(key);
        let applied2 = registry2.get_applied(key);
        let observed1 = registry1.get_observed(key);
        let observed2 = registry2.get_observed(key);

        assert_eq!(
            applied1, applied2,
            "{key}: Same seed should produce same applied count ({applied1} vs {applied2})"
        );
        assert_eq!(
            observed1, observed2,
            "{key}: Same seed should produce same observed count ({observed1} vs {observed2})"
        );
    }

    println!("Determinism verified for all {} fault types", keys.len());
}

#[test]
fn test_zero_effectiveness_when_no_faults_applied() {
    let registry = run_scenario_and_get_registry(ScenarioType::Baseline, 100, 42);

    // Check effectiveness for a fault that wasn't applied
    let effectiveness = registry.effectiveness("nonexistent.fault");

    assert_eq!(
        effectiveness, 0.0,
        "Effectiveness should be 0.0 when fault not applied"
    );
}

// ============================================================================
// Effect Tracking Sanity Tests
// ============================================================================

#[test]
fn test_network_drop_effectiveness_is_100_percent() {
    let registry = run_scenario_and_get_registry(ScenarioType::Combined, 5000, 54321);

    let applied = registry.get_applied("network.drop");
    if applied == 0 {
        println!("No drops applied, skipping test");
        return;
    }

    let observed = registry.get_observed("network.drop");

    // Every drop should be observed (the drop itself is the observable effect)
    assert_eq!(
        applied, observed,
        "Drop should have 100% effectiveness: applied {applied} but observed {observed}"
    );
}
