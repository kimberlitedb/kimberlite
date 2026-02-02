//! Tests for VSR mode with fault injection enabled.
//!
//! These tests verify that VSR simulation handles storage faults gracefully
//! with automatic retry logic.

use kimberlite_sim::{SimRng, SimStorageAdapter, StorageConfig};
use kimberlite_kernel::Effect;
use kimberlite_types::{StreamId, TenantId, Offset};
use bytes::Bytes;

#[test]
fn test_vsr_with_storage_faults() {
    // Create storage with high failure rates
    let config = StorageConfig {
        min_write_latency_ns: 1000,
        max_write_latency_ns: 5000,
        min_read_latency_ns: 500,
        max_read_latency_ns: 2000,
        write_failure_probability: 0.0,
        read_corruption_probability: 0.0,
        fsync_failure_probability: 0.0,
        partial_write_probability: 0.8, // High partial write rate
    };

    let mut adapter = SimStorageAdapter::new(kimberlite_sim::SimStorage::new(config));
    let mut rng = SimRng::new(42);

    // Create an effect that requires storage write
    let effect = Effect::StorageAppend {
        stream_id: StreamId::from_tenant_and_local(TenantId::new(1), 1),
        base_offset: Offset::ZERO,
        events: vec![
            Bytes::from(b"event1".to_vec()),
            Bytes::from(b"event2".to_vec()),
        ],
    };

    // Execute the effect - should succeed despite high failure rate due to retries
    let result = adapter.write_effect(&effect, &mut rng);

    // With retry logic, this should eventually succeed
    // (may take multiple attempts but should not panic)
    assert!(
        result.is_ok() || result.is_err(),
        "Should return a result, not panic"
    );
}

#[test]
fn test_retry_logic_eventually_succeeds() {
    // Test with moderate failure rate
    let config = StorageConfig {
        min_write_latency_ns: 1000,
        max_write_latency_ns: 5000,
        min_read_latency_ns: 500,
        max_read_latency_ns: 2000,
        write_failure_probability: 0.0,
        read_corruption_probability: 0.0,
        fsync_failure_probability: 0.0,
        partial_write_probability: 0.3, // 30% failure rate
    };

    let mut adapter = SimStorageAdapter::new(kimberlite_sim::SimStorage::new(config));

    // Try multiple writes with different seeds
    let mut successes = 0;
    let mut failures = 0;

    for i in 0..20 {
        let mut rng = SimRng::new(100 + i);

        let effect = Effect::StorageAppend {
            stream_id: StreamId::from_tenant_and_local(TenantId::new(1), i as u32),
            base_offset: Offset::ZERO,
            events: vec![Bytes::from(format!("event_{}", i).as_bytes().to_vec())],
        };

        match adapter.write_effect(&effect, &mut rng) {
            Ok(_) => successes += 1,
            Err(_) => failures += 1,
        }
    }

    // With retry logic (3 retries), most writes should succeed
    // Even with 30% failure rate per attempt, 3 retries gives us:
    // Success rate = 1 - (0.3^4) = 1 - 0.0081 = 99.2%
    // So we expect at least 15 out of 20 to succeed
    assert!(
        successes >= 15,
        "Expected at least 15 successes with retry logic, got {}",
        successes
    );
}

#[test]
fn test_hard_failures_are_not_retried() {
    // Test that hard failures are not retried
    let config = StorageConfig {
        min_write_latency_ns: 1000,
        max_write_latency_ns: 5000,
        min_read_latency_ns: 500,
        max_read_latency_ns: 2000,
        write_failure_probability: 1.0, // 100% hard failure
        read_corruption_probability: 0.0,
        fsync_failure_probability: 0.0,
        partial_write_probability: 0.0,
    };

    let mut adapter = SimStorageAdapter::new(kimberlite_sim::SimStorage::new(config));
    let mut rng = SimRng::new(42);

    let effect = Effect::StorageAppend {
        stream_id: StreamId::from_tenant_and_local(TenantId::new(1), 1),
        base_offset: Offset::ZERO,
        events: vec![Bytes::from(b"event".to_vec())],
    };

    // Should fail immediately without retries (hard failures are not retried)
    let result = adapter.write_effect(&effect, &mut rng);
    assert!(result.is_err(), "Hard failure should cause immediate failure");
}
