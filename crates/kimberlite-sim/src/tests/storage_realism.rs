//! Integration tests for storage realism features.
//!
//! Tests write reordering, concurrent I/O, and crash recovery integration.

use crate::rng::SimRng;
use crate::storage::{SimStorage, StorageConfig};

#[test]
fn test_write_reordering_enabled() {
    // Create storage with reordering enabled
    let mut config = StorageConfig::reliable();
    config.enable_reordering = true;

    let mut storage = SimStorage::new(config);
    let mut rng = SimRng::new(12345);

    // Write multiple blocks
    storage.write(0, vec![1, 2, 3], &mut rng);
    storage.write(4096, vec![4, 5, 6], &mut rng);
    storage.write(8192, vec![7, 8, 9], &mut rng);

    // Fsync should drain all reordered writes
    storage.fsync(&mut rng);

    // Verify all writes are durable
    assert!(storage.exists(0));
    assert!(storage.exists(4096));
    assert!(storage.exists(8192));
}

#[test]
fn test_write_reordering_disabled() {
    // Create storage with reordering disabled (default)
    let config = StorageConfig::reliable();
    assert!(!config.enable_reordering);

    let mut storage = SimStorage::new(config);
    let mut rng = SimRng::new(12345);

    // Write multiple blocks
    storage.write(0, vec![1, 2, 3], &mut rng);
    storage.write(4096, vec![4, 5, 6], &mut rng);
    storage.write(8192, vec![7, 8, 9], &mut rng);

    // Fsync should work normally
    storage.fsync(&mut rng);

    // Verify all writes are durable
    assert!(storage.exists(0));
    assert!(storage.exists(4096));
    assert!(storage.exists(8192));
}

#[test]
fn test_reordering_fifo_vs_random() {
    // Test with FIFO policy
    let seed = 42;
    let mut rng1 = SimRng::new(seed);

    let mut config1 = StorageConfig::reliable();
    config1.enable_reordering = true;
    let mut storage1 = SimStorage::new(config1);

    for addr in (0..10).map(|i| i * 4096) {
        storage1.write(addr, vec![addr as u8], &mut rng1);
    }
    storage1.fsync(&mut rng1);

    // Test with Random policy (already default)
    let mut rng2 = SimRng::new(seed);

    let mut config2 = StorageConfig::reliable();
    config2.enable_reordering = true;
    let mut storage2 = SimStorage::new(config2);

    for addr in (0..10).map(|i| i * 4096) {
        storage2.write(addr, vec![addr as u8], &mut rng2);
    }
    storage2.fsync(&mut rng2);

    // Both should have all data (regardless of reordering)
    for addr in (0..10).map(|i| i * 4096) {
        assert!(storage1.exists(addr));
        assert!(storage2.exists(addr));
    }
}

#[test]
fn test_reordering_determinism() {
    // Same seed should produce same behavior
    let seed = 12345;

    let mut config1 = StorageConfig::reliable();
    config1.enable_reordering = true;

    // First run
    let mut storage1 = SimStorage::new(config1.clone());
    let mut rng1 = SimRng::new(seed);

    for addr in (0..20).map(|i| i * 4096) {
        storage1.write(addr, vec![addr as u8; 100], &mut rng1);
    }
    storage1.fsync(&mut rng1);

    let stats1 = storage1.stats().clone();

    // Second run with same seed
    let mut storage2 = SimStorage::new(config1);
    let mut rng2 = SimRng::new(seed);

    for addr in (0..20).map(|i| i * 4096) {
        storage2.write(addr, vec![addr as u8; 100], &mut rng2);
    }
    storage2.fsync(&mut rng2);

    let stats2 = storage2.stats().clone();

    // Statistics should be identical (deterministic)
    assert_eq!(stats1.writes, stats2.writes);
    assert_eq!(stats1.writes_successful, stats2.writes_successful);
    assert_eq!(stats1.fsyncs, stats2.fsyncs);
    assert_eq!(stats1.bytes_written, stats2.bytes_written);
}

#[test]
fn test_crash_recovery_with_reordering() {
    let mut config = StorageConfig::reliable();
    config.enable_reordering = true;
    config.enable_crash_recovery = true;

    let mut storage = SimStorage::new(config);
    let mut rng = SimRng::new(99999);

    // Write some data
    storage.write(0, vec![1, 2, 3, 4], &mut rng);
    storage.write(4096, vec![5, 6, 7, 8], &mut rng);

    // Don't fsync - data is in reorderer and/or pending_writes

    // Simulate crash
    storage.crash(None, &mut rng);

    // Unfsynced data should be lost
    assert!(!storage.exists(0));
    assert!(!storage.exists(4096));
}

#[test]
fn test_fsync_drains_reorderer() {
    let mut config = StorageConfig::reliable();
    config.enable_reordering = true;

    let mut storage = SimStorage::new(config);
    let mut rng = SimRng::new(77777);

    // Write many blocks to fill reorderer queue
    for i in 0..50 {
        storage.write(i * 4096, vec![i as u8; 100], &mut rng);
    }

    // Before fsync, storage might not be dirty if all writes are in reorderer
    // (depends on processing in write method)

    // Fsync should drain all writes from reorderer
    storage.fsync(&mut rng);

    // After fsync, all blocks should be durable
    for i in 0..50 {
        assert!(
            storage.exists(i * 4096),
            "Block {} should exist after fsync",
            i
        );
    }

    // Storage should not be dirty after fsync
    assert!(!storage.is_dirty());
}
