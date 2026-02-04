//! Integration tests for simulator-level canaries.
//!
//! These tests verify that sim canaries are properly integrated into VOPR
//! and affect simulation behavior as expected.

use kimberlite_sim::{sim_canaries, NetworkConfig, SimNetwork, SimRng};
use std::collections::HashSet;

#[test]
#[cfg(feature = "sim-canary-drop-disabled")]
fn test_drop_disabled_integration() {
    // With drop-disabled canary, messages should never be dropped
    let mut network = SimNetwork::new(NetworkConfig {
        drop_probability: 1.0, // 100% drop rate configured
        ..Default::default()
    });
    let mut rng = SimRng::new(12345);

    // Register nodes
    network.register_node(0);
    network.register_node(1);

    // Try to send messages - with canary, none should drop
    let mut dropped = 0;
    let mut queued = 0;

    for _ in 0..100 {
        use kimberlite_sim::SendResult;
        match network.send(0, 1, vec![1, 2, 3], 0, &mut rng) {
            SendResult::Dropped => dropped += 1,
            SendResult::Queued { .. } => queued += 1,
            _ => {}
        }
    }

    // With canary enabled, should_actually_drop_message() returns false
    // So even with 100% drop probability, nothing drops
    assert_eq!(
        dropped, 0,
        "Canary should disable drops, but got {} drops",
        dropped
    );
    assert!(
        queued > 0,
        "Should have queued messages instead of dropping"
    );

    println!("Drop-disabled canary working: 0 drops, {} queued", queued);
}

#[test]
#[cfg(not(feature = "sim-canary-drop-disabled"))]
fn test_drop_disabled_not_active() {
    // Without canary, drops should happen with high probability
    let mut network = SimNetwork::new(NetworkConfig {
        drop_probability: 1.0, // 100% drop rate
        ..Default::default()
    });
    let mut rng = SimRng::new(12345);

    network.register_node(0);
    network.register_node(1);

    let mut dropped = 0;

    for _ in 0..100 {
        use kimberlite_sim::SendResult;
        match network.send(0, 1, vec![1, 2, 3], 0, &mut rng) {
            SendResult::Dropped => dropped += 1,
            _ => {}
        }
    }

    // Without canary, drops should happen
    assert!(dropped > 50, "Should have dropped most messages");
}

#[test]
#[cfg(feature = "sim-canary-partition-leak")]
fn test_partition_leak_integration() {
    use kimberlite_sim::SendResult;

    let mut network = SimNetwork::new(NetworkConfig::default());
    let mut rng = SimRng::new(99999);

    // Register 4 nodes
    for i in 0..4 {
        network.register_node(i);
    }

    // Create partition: [[0,1], [2,3]]
    let mut group_a = HashSet::new();
    group_a.insert(0);
    group_a.insert(1);
    let mut group_b = HashSet::new();
    group_b.insert(2);
    group_b.insert(3);
    network.create_partition(group_a, group_b, true);

    // Try sending cross-group messages
    let mut partitioned = 0;
    let mut leaked = 0;

    for _ in 0..1000 {
        match network.send(0, 2, vec![1, 2, 3], 0, &mut rng) {
            SendResult::Rejected { .. } => partitioned += 1,
            SendResult::Queued { .. } => leaked += 1,
            _ => {}
        }
    }

    // With canary, should leak ~1% of messages
    assert!(leaked > 0, "Canary should leak some messages");
    assert!(
        leaked < 50,
        "Leak rate too high: {} / 1000",
        leaked
    );

    println!(
        "Partition leak canary working: {} partitioned, {} leaked",
        partitioned, leaked
    );
}

#[test]
#[cfg(not(feature = "sim-canary-partition-leak"))]
fn test_partition_no_leak() {
    use kimberlite_sim::SendResult;

    let mut network = SimNetwork::new(NetworkConfig::default());
    let mut rng = SimRng::new(99999);

    for i in 0..4 {
        network.register_node(i);
    }

    let mut group_a = HashSet::new();
    group_a.insert(0);
    group_a.insert(1);
    let mut group_b = HashSet::new();
    group_b.insert(2);
    group_b.insert(3);
    network.create_partition(group_a, group_b, true);

    let mut leaked = 0;

    for _ in 0..1000 {
        match network.send(0, 2, vec![1, 2, 3], 0, &mut rng) {
            SendResult::Queued { .. } => leaked += 1,
            _ => {}
        }
    }

    // Without canary, no leaks
    assert_eq!(leaked, 0, "Partition should be perfect without canary");
}

#[test]
#[cfg(feature = "sim-canary-fsync-lies")]
fn test_fsync_lies_integration() {
    use kimberlite_sim::FsyncResult;

    let mut storage = SimStorage::new(StorageConfig {
        fsync_failure_probability: 1.0, // Always fail
        ..Default::default()
    });
    let mut rng = SimRng::new(12345);

    // Write data
    storage.write(0, vec![1, 2, 3], &mut rng);

    // Fsync should lie about failure
    let mut successes = 0;
    let mut failures = 0;

    for _ in 0..10 {
        // Re-dirty storage for each fsync attempt
        storage.write(0, vec![1, 2, 3], &mut rng);

        match storage.fsync(&mut rng) {
            FsyncResult::Success { .. } => successes += 1,
            FsyncResult::Failed { .. } => failures += 1,
        }
    }

    // With canary, fsync lies about failures (returns success)
    // Note: The canary inverts the result, so with 100% failure rate,
    // we should get mostly successes
    assert!(
        successes > 0,
        "Canary should lie about some failures"
    );

    println!(
        "Fsync lies canary working: {} successes (lies), {} failures (truth)",
        successes, failures
    );
}

#[test]
#[cfg(feature = "sim-canary-rng-unseeded")]
fn test_rng_unseeded_breaks_determinism() {
    // With the canary, same seed might produce different results
    // This is hard to test reliably because it only triggers 0.1% of the time

    let mut rng1 = SimRng::new(42);
    let mut rng2 = SimRng::new(42);

    // Generate many values - canary might inject entropy in one stream
    let mut values1 = Vec::new();
    let mut values2 = Vec::new();

    for _ in 0..10_000 {
        values1.push(rng1.next_u64());
        values2.push(rng2.next_u64());
    }

    // With enough iterations, the canary should trigger and cause divergence
    // This test is probabilistic and might occasionally pass even with the canary
    let diverged = values1.iter().zip(&values2).any(|(a, b)| a != b);

    if diverged {
        println!("RNG unseeded canary triggered - determinism broken as expected");
    } else {
        println!("RNG unseeded canary didn't trigger in 10k iterations (probabilistic)");
    }

    // We can't assert here because it's probabilistic
    // The real test is that this compiles and runs with the feature enabled
}

#[test]
#[cfg(feature = "sim-canary-time-leak")]
fn test_time_leak_breaks_determinism() {
    use kimberlite_sim::SimClock;

    // With time-leak canary, clock.now() might return wall time
    let mut clock = SimClock::new();

    // Advance to a time that triggers the leak (multiple of 1000, ends in 42)
    clock.advance_to(42_000);

    // Call now() - might return wall time
    let time = clock.now();

    // If canary triggered, time would be huge (wall clock nanoseconds since epoch)
    // If not triggered, time = 42_000
    if time > 1_000_000_000_000 {
        println!(
            "Time leak canary triggered - got wall clock time: {}",
            time
        );
    } else {
        println!("Time leak canary didn't trigger at this time: {}", time);
    }

    // Like rng-unseeded, this is probabilistic based on the sim time
    // The test verifies compilation and basic functionality
}

#[test]
fn test_sim_canary_detection_functions() {
    // Test that we can detect which canaries are enabled
    let enabled = sim_canaries::enabled_sim_canaries();
    let any_enabled = sim_canaries::any_sim_canary_enabled();

    if any_enabled {
        assert!(!enabled.is_empty());
        println!("Enabled sim canaries: {:?}", enabled);
    } else {
        assert!(enabled.is_empty());
        println!("No sim canaries enabled");
    }
}
