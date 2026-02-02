//! Phase 2 integration tests for sometimes_assert! and determinism.

use super::invariant_runtime::{
    get_all_invariant_run_counts, init_invariant_runtime, should_check_invariant,
};

#[test]
fn test_deterministic_invariant_sampling() {
    // Test that the SAME runtime with the SAME seed produces deterministic results
    // Note: Global step counter advances, so we test that hash(seed ^ key ^ step) is deterministic
    // by comparing results from two separate runtimes with the same seed

    use super::invariant_runtime::InvariantRuntime;

    let runtime1 = InvariantRuntime::new(54321);
    let runtime2 = InvariantRuntime::new(54321);

    // Run both for 100 iterations
    for _ in 0..100 {
        let check1 = runtime1.should_check("test.determinism", 10);
        let check2 = runtime2.should_check("test.determinism", 10);

        // At the same step, same seed → same decision
        assert_eq!(
            check1, check2,
            "Determinism violated: same seed + step produced different results"
        );
    }
}

#[test]
fn test_different_seeds_different_patterns() {
    init_invariant_runtime(11111);

    let mut checks_seed1 = Vec::new();
    for _ in 0..100 {
        if should_check_invariant("test.seeds", 10) {
            checks_seed1.push(true);
        } else {
            checks_seed1.push(false);
        }
    }

    init_invariant_runtime(22222);

    let mut checks_seed2 = Vec::new();
    for _ in 0..100 {
        if should_check_invariant("test.seeds", 10) {
            checks_seed2.push(true);
        } else {
            checks_seed2.push(false);
        }
    }

    // Different seeds should produce different patterns (statistically very unlikely to be the same)
    assert_ne!(
        checks_seed1, checks_seed2,
        "Different seeds produced identical patterns (statistically impossible)"
    );
}

#[test]
fn test_sampling_rate_statistical() {
    init_invariant_runtime(99999);

    let rate = 20u64; // 1 in 20 = 5%
    let samples = 10000;
    let mut hits = 0;

    for _ in 0..samples {
        if should_check_invariant("test.statistical", rate) {
            hits += 1;
        }
    }

    // Expected: samples / rate = 500
    // Allow 20% tolerance
    let expected = samples / rate;
    let tolerance = expected / 5;

    assert!(
        hits >= expected - tolerance && hits <= expected + tolerance,
        "Expected ~{expected} hits (±{tolerance}), got {hits}"
    );
}

#[test]
fn test_run_count_tracking_integration() {
    init_invariant_runtime(77777);

    // Run some invariants
    for _ in 0..10 {
        should_check_invariant("inv1", 1); // Always runs
    }

    for _ in 0..20 {
        should_check_invariant("inv2", 1); // Always runs
    }

    // Get counts
    let counts = get_all_invariant_run_counts();

    assert_eq!(counts.get("inv1"), Some(&10));
    assert_eq!(counts.get("inv2"), Some(&20));
}

#[test]
fn test_independent_invariant_keys() {
    init_invariant_runtime(33333);

    let mut inv_a_hits = 0;
    let mut inv_b_hits = 0;

    for _ in 0..1000 {
        if should_check_invariant("independent.a", 5) {
            inv_a_hits += 1;
        }
        if should_check_invariant("independent.b", 5) {
            inv_b_hits += 1;
        }
    }

    // Both should have hits
    assert!(inv_a_hits > 0, "invariant A never ran");
    assert!(inv_b_hits > 0, "invariant B never ran");

    // They should be different (independent sampling)
    assert_ne!(
        inv_a_hits, inv_b_hits,
        "Independent invariants had identical sample counts"
    );
}

#[test]
fn test_rate_zero_never_runs() {
    init_invariant_runtime(88888);

    for _ in 0..1000 {
        assert!(
            !should_check_invariant("disabled", 0),
            "Rate 0 should never trigger"
        );
    }

    let counts = get_all_invariant_run_counts();
    assert_eq!(counts.get("disabled"), None); // Should not be in registry
}

#[test]
fn test_rate_one_always_runs() {
    init_invariant_runtime(66666);

    for _ in 0..100 {
        assert!(
            should_check_invariant("always", 1),
            "Rate 1 should always trigger"
        );
    }

    let counts = get_all_invariant_run_counts();
    assert_eq!(counts.get("always"), Some(&100));
}
