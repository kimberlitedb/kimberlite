//! Unit tests for fault injector semantics.
//!
//! These tests verify that each fault injector works correctly in isolation,
//! without requiring Kimberlite infrastructure. They prove the simulator's
//! fault injection mechanisms are faithful to their specifications.

use kimberlite_sim::{
    FsyncResult, NetworkConfig, ReadResult, SimNetwork, SimRng, SimStorage, StorageConfig,
    WriteResult,
};
use std::collections::HashSet;

// ============================================================================
// Network Partition Tests
// ============================================================================

#[test]
fn test_partition_blocks_cross_group_messages() {
    let mut network = SimNetwork::new(NetworkConfig::default());
    let mut rng = SimRng::new(12345);

    // Register 5 nodes
    for i in 0..5 {
        network.register_node(i);
    }

    // Create partition: [[0, 1], [2, 3, 4]]
    let mut group_a = HashSet::new();
    group_a.insert(0);
    group_a.insert(1);

    let mut group_b = HashSet::new();
    group_b.insert(2);
    group_b.insert(3);
    group_b.insert(4);

    network.create_partition(group_a, group_b, true);

    // Test same-group communication (should work)
    use kimberlite_sim::SendResult;

    // Group A to Group A
    match network.send(0, 1, vec![1, 2, 3], 0, &mut rng) {
        SendResult::Queued { .. } => {}
        other => panic!("Expected Queued, got {:?}", other),
    }

    // Group B to Group B
    match network.send(2, 3, vec![1, 2, 3], 0, &mut rng) {
        SendResult::Queued { .. } => {}
        other => panic!("Expected Queued, got {:?}", other),
    }

    // Test cross-group communication (should be blocked)
    match network.send(0, 2, vec![1, 2, 3], 0, &mut rng) {
        SendResult::Rejected { .. } => {}
        other => panic!("Expected Rejected, got {:?}", other),
    }

    match network.send(2, 0, vec![1, 2, 3], 0, &mut rng) {
        SendResult::Rejected { .. } => {}
        other => panic!("Expected Rejected, got {:?}", other),
    }
}

#[test]
fn test_partition_heal_restores_connectivity() {
    let mut network = SimNetwork::new(NetworkConfig::default());
    let mut rng = SimRng::new(54321);

    for i in 0..3 {
        network.register_node(i);
    }

    // Create partition: [[0], [1, 2]]
    let mut group_a = HashSet::new();
    group_a.insert(0);

    let mut group_b = HashSet::new();
    group_b.insert(1);
    group_b.insert(2);

    let partition_id = network.create_partition(group_a, group_b, true);

    // Verify partition blocks communication
    use kimberlite_sim::SendResult;
    match network.send(0, 1, vec![1, 2, 3], 0, &mut rng) {
        SendResult::Rejected { .. } => {}
        other => panic!("Expected Rejected while partitioned, got {:?}", other),
    }

    // Heal partition
    assert!(network.heal_partition(partition_id));

    // Verify communication is restored
    match network.send(0, 1, vec![1, 2, 3], 0, &mut rng) {
        SendResult::Queued { .. } => {}
        other => panic!("Expected Queued after healing, got {:?}", other),
    }
}

#[test]
fn test_partition_asymmetric() {
    let mut network = SimNetwork::new(NetworkConfig::default());
    let mut rng = SimRng::new(99999);

    for i in 0..3 {
        network.register_node(i);
    }

    // Create asymmetric partition: [0] -> [1, 2] blocked, but [1, 2] -> [0] allowed
    let mut group_a = HashSet::new();
    group_a.insert(0);

    let mut group_b = HashSet::new();
    group_b.insert(1);
    group_b.insert(2);

    network.create_partition(group_a, group_b, false); // asymmetric

    use kimberlite_sim::SendResult;

    // A -> B should be blocked
    match network.send(0, 1, vec![1, 2, 3], 0, &mut rng) {
        SendResult::Rejected { .. } => {}
        other => panic!("Expected Rejected for A->B, got {:?}", other),
    }

    // B -> A should be allowed
    match network.send(1, 0, vec![1, 2, 3], 0, &mut rng) {
        SendResult::Queued { .. } => {}
        other => panic!("Expected Queued for B->A, got {:?}", other),
    }
}

// ============================================================================
// Message Drop Tests
// ============================================================================

#[test]
fn test_drop_always_drops_with_probability_one() {
    let mut network = SimNetwork::new(NetworkConfig {
        drop_probability: 1.0, // 100% drop rate
        ..Default::default()
    });
    let mut rng = SimRng::new(42);

    network.register_node(0);
    network.register_node(1);

    // All messages should be dropped
    for _ in 0..100 {
        use kimberlite_sim::SendResult;
        match network.send(0, 1, vec![1, 2, 3], 0, &mut rng) {
            SendResult::Dropped => {}
            other => panic!("Expected Dropped with 100% probability, got {:?}", other),
        }
    }
}

#[test]
fn test_drop_never_drops_with_probability_zero() {
    let mut network = SimNetwork::new(NetworkConfig {
        drop_probability: 0.0, // 0% drop rate
        ..Default::default()
    });
    let mut rng = SimRng::new(42);

    network.register_node(0);
    network.register_node(1);

    // No messages should be dropped
    for _ in 0..100 {
        use kimberlite_sim::SendResult;
        match network.send(0, 1, vec![1, 2, 3], 0, &mut rng) {
            SendResult::Queued { .. } => {}
            other => panic!("Expected Queued with 0% drop probability, got {:?}", other),
        }
    }
}

// ============================================================================
// Storage Corruption Tests
// ============================================================================

#[test]
fn test_corruption_always_detected_with_checksums() {
    // This test verifies that corrupted data is detected by checksums
    let mut storage = SimStorage::new(StorageConfig {
        read_corruption_probability: 1.0, // Always corrupt on read
        ..Default::default()
    });
    let mut rng = SimRng::new(12345);

    // Write data
    match storage.write(0, vec![1, 2, 3, 4, 5], &mut rng) {
        WriteResult::Success { .. } => {}
        _ => panic!("Write should succeed"),
    }

    // Fsync to make durable
    match storage.fsync(&mut rng) {
        FsyncResult::Success { .. } => {}
        _ => panic!("Fsync should succeed"),
    }

    // With 100% read corruption probability, reads should detect corruption
    // Note: Corruption happens during read operations
    let mut corruption_detected = false;
    for _ in 0..10 {
        match storage.read(0, &mut rng) {
            ReadResult::Corrupted { .. } => {
                corruption_detected = true;
                break;
            }
            ReadResult::Success { .. } => {
                // Corruption might not trigger every time due to RNG
            }
            _ => {}
        }
    }

    // With 100% corruption rate, should always detect corruption
    assert!(
        corruption_detected,
        "Corruption should be detected with 100% probability"
    );
}

// ============================================================================
// Crash Recovery Tests
// ============================================================================

#[test]
fn test_crash_loses_pending_writes() {
    let mut storage = SimStorage::new(StorageConfig::default());
    let mut rng = SimRng::new(42);

    // Write some data
    match storage.write(0, vec![1, 2, 3], &mut rng) {
        WriteResult::Success { .. } => {}
        _ => panic!("Write should succeed"),
    }

    // Fsync to make it durable
    match storage.fsync(&mut rng) {
        FsyncResult::Success { .. } => {}
        _ => panic!("Fsync should succeed"),
    }

    // Write more data WITHOUT fsync
    match storage.write(1, vec![4, 5, 6], &mut rng) {
        WriteResult::Success { .. } => {}
        _ => panic!("Write should succeed"),
    }

    // Crash before fsync
    storage.crash(None, &mut rng);

    // First write (fsync'd) should survive
    match storage.read(0, &mut rng) {
        ReadResult::Success { data, .. } => {
            assert_eq!(data, vec![1, 2, 3]);
        }
        _ => panic!("Fsync'd data should survive crash"),
    }

    // Second write (not fsync'd) should be lost
    match storage.read(1, &mut rng) {
        ReadResult::NotFound { .. } => {}
        other => panic!("Unfsynced data should be lost after crash, got {:?}", other),
    }
}

#[test]
fn test_fsync_durability_semantics() {
    let mut storage = SimStorage::new(StorageConfig::default());
    let mut rng = SimRng::new(99999);

    // Write + fsync = durable
    match storage.write(0, vec![10, 20, 30], &mut rng) {
        WriteResult::Success { .. } => {}
        _ => panic!("Write should succeed"),
    }
    match storage.fsync(&mut rng) {
        FsyncResult::Success { .. } => {}
        _ => panic!("Fsync should succeed"),
    }

    // Write without fsync = volatile
    match storage.write(1, vec![40, 50, 60], &mut rng) {
        WriteResult::Success { .. } => {}
        _ => panic!("Write should succeed"),
    }

    // Crash
    storage.crash(None, &mut rng);

    // Durable data survives
    match storage.read(0, &mut rng) {
        ReadResult::Success { data, .. } => {
            assert_eq!(data, vec![10, 20, 30]);
        }
        _ => panic!("Durable data should survive"),
    }

    // Volatile data is lost
    match storage.read(1, &mut rng) {
        ReadResult::NotFound { .. } => {}
        other => panic!("Volatile data should be lost, got {:?}", other),
    }
}

#[test]
fn test_fsync_failure_loses_data() {
    let mut storage = SimStorage::new(StorageConfig {
        fsync_failure_probability: 1.0, // Always fail
        ..Default::default()
    });
    let mut rng = SimRng::new(42);

    // Write data
    match storage.write(0, vec![1, 2, 3], &mut rng) {
        WriteResult::Success { .. } => {}
        _ => panic!("Write should succeed"),
    }

    // Fsync fails
    match storage.fsync(&mut rng) {
        FsyncResult::Failed { .. } => {}
        other => panic!("Expected fsync failure, got {:?}", other),
    }

    // Data should be lost (pending writes cleared on fsync failure)
    assert!(!storage.is_dirty());

    match storage.read(0, &mut rng) {
        ReadResult::NotFound { .. } => {}
        other => panic!("Data should be lost after failed fsync, got {:?}", other),
    }
}

// ============================================================================
// Network Delay Tests
// ============================================================================

#[test]
fn test_message_delay_within_configured_range() {
    let min_delay = 1_000_000; // 1ms
    let max_delay = 10_000_000; // 10ms

    let mut network = SimNetwork::new(NetworkConfig {
        min_delay_ns: min_delay,
        max_delay_ns: max_delay,
        ..Default::default()
    });
    let mut rng = SimRng::new(12345);

    network.register_node(0);
    network.register_node(1);

    // Send many messages and check delivery times
    for _ in 0..100 {
        use kimberlite_sim::SendResult;
        match network.send(0, 1, vec![1, 2, 3], 0, &mut rng) {
            SendResult::Queued { deliver_at_ns, .. } => {
                assert!(
                    deliver_at_ns >= min_delay && deliver_at_ns < max_delay,
                    "Delivery time {} outside range [{}, {})",
                    deliver_at_ns,
                    min_delay,
                    max_delay
                );
            }
            other => panic!("Expected Queued, got {:?}", other),
        }
    }
}

// ============================================================================
// RNG Determinism Tests
// ============================================================================

#[test]
fn test_same_seed_same_behavior() {
    // Create two identical networks with same seed
    let config = NetworkConfig {
        drop_probability: 0.5,
        ..Default::default()
    };

    let mut network1 = SimNetwork::new(config.clone());
    let mut network2 = SimNetwork::new(config);

    let mut rng1 = SimRng::new(42);
    let mut rng2 = SimRng::new(42);

    for i in 0..2 {
        network1.register_node(i);
        network2.register_node(i);
    }

    // Send same messages with same RNG seed
    let mut results1 = Vec::new();
    let mut results2 = Vec::new();

    for _ in 0..20 {
        let r1 = network1.send(0, 1, vec![1, 2, 3], 0, &mut rng1);
        let r2 = network2.send(0, 1, vec![1, 2, 3], 0, &mut rng2);

        results1.push(matches!(r1, kimberlite_sim::SendResult::Dropped));
        results2.push(matches!(r2, kimberlite_sim::SendResult::Dropped));
    }

    // Results should be identical
    assert_eq!(results1, results2, "Same seed should produce same behavior");
}

#[test]
fn test_different_seed_different_behavior() {
    let config = NetworkConfig {
        drop_probability: 0.5,
        ..Default::default()
    };

    let mut network1 = SimNetwork::new(config.clone());
    let mut network2 = SimNetwork::new(config);

    let mut rng1 = SimRng::new(42);
    let mut rng2 = SimRng::new(99999); // Different seed

    for i in 0..2 {
        network1.register_node(i);
        network2.register_node(i);
    }

    let mut results1 = Vec::new();
    let mut results2 = Vec::new();

    for _ in 0..20 {
        let r1 = network1.send(0, 1, vec![1, 2, 3], 0, &mut rng1);
        let r2 = network2.send(0, 1, vec![1, 2, 3], 0, &mut rng2);

        results1.push(matches!(r1, kimberlite_sim::SendResult::Dropped));
        results2.push(matches!(r2, kimberlite_sim::SendResult::Dropped));
    }

    // Results should be different (with high probability)
    assert_ne!(
        results1, results2,
        "Different seeds should produce different behavior"
    );
}

// ============================================================================
// Storage Statistics Tests
// ============================================================================

#[test]
fn test_storage_tracks_statistics() {
    let mut storage = SimStorage::new(StorageConfig::default());
    let mut rng = SimRng::new(42);

    // Initial stats
    let stats = storage.stats();
    assert_eq!(stats.writes, 0);
    assert_eq!(stats.reads, 0);
    assert_eq!(stats.fsyncs, 0);

    // Write
    storage.write(0, vec![1, 2, 3], &mut rng);
    assert_eq!(storage.stats().writes, 1);

    // Fsync
    storage.fsync(&mut rng);
    assert_eq!(storage.stats().fsyncs, 1);
    assert_eq!(storage.stats().fsyncs_successful, 1);

    // Read
    storage.read(0, &mut rng);
    assert_eq!(storage.stats().reads, 1);
}

#[test]
fn test_network_tracks_statistics() {
    let mut network = SimNetwork::new(NetworkConfig {
        drop_probability: 0.5,
        ..Default::default()
    });
    let mut rng = SimRng::new(42);

    network.register_node(0);
    network.register_node(1);

    // Initial stats
    let stats = network.stats();
    assert_eq!(stats.messages_sent, 0);
    assert_eq!(stats.messages_dropped, 0);

    // Send messages
    for _ in 0..100 {
        network.send(0, 1, vec![1, 2, 3], 0, &mut rng);
    }

    let stats = network.stats();
    assert_eq!(stats.messages_sent, 100);
    assert!(
        stats.messages_dropped > 0,
        "Should have dropped some messages"
    );
    assert!(
        stats.messages_dropped < 100,
        "Should not have dropped all messages"
    );
}
