//! Canary mutations for testing VOPR's ability to detect bugs.
//!
//! This module contains intentional bugs that should be caught by invariants.
//! Each canary is gated by a feature flag and should never be enabled in production.
//!
//! # Philosophy
//!
//! A testing framework is only as good as the bugs it can catch. Canary mutations
//! are intentional bugs that VOPR **must** detect. If a canary is not caught,
//! either:
//! 1. The invariant is missing
//! 2. The invariant is not running
//! 3. The invariant is too weak
//!
//! # Available Canaries
//!
//! - `canary-skip-fsync`: Skip fsync 1 in 1000 times
//! - `canary-wrong-hash`: Use incorrect hash in chain
//! - `canary-commit-quorum`: Commit with f instead of f+1 replicas
//! - `canary-idempotency-race`: Record idempotency after apply
//! - `canary-monotonic-regression`: Allow view/op to regress
//!
//! # Usage
//!
//! ```bash
//! # Run VOPR with a canary - should detect violation
//! cargo test -p kimberlite-sim --features canary-skip-fsync -- --nocapture
//!
//! # In CI, verify canaries are caught
//! ./scripts/test-canaries.sh
//! ```

use crate::rng::SimRng;

/// Canary: Skip fsync 1 in 1000 times.
///
/// **Expected Detection**: StorageDeterminismChecker
/// **Invariant**: storage_determinism
/// **Why it fails**: Skipping fsync causes data loss on crash, leading to
/// replica divergence.
#[cfg(feature = "canary-skip-fsync")]
pub fn should_skip_fsync(rng: &mut SimRng) -> bool {
    rng.next_bool_with_probability(0.001) // 0.1% chance
}

#[cfg(not(feature = "canary-skip-fsync"))]
#[allow(unused_variables)]
pub fn should_skip_fsync(rng: &mut SimRng) -> bool {
    false
}

/// Canary: Corrupt hash chain by using wrong previous hash.
///
/// **Expected Detection**: HashChainChecker
/// **Invariant**: hash_chain_integrity
/// **Why it fails**: Breaks the hash chain linkage invariant.
#[cfg(feature = "canary-wrong-hash")]
pub fn corrupt_hash(hash: &[u8; 32]) -> [u8; 32] {
    let mut corrupted = *hash;
    corrupted[0] ^= 0xFF; // Flip bits in first byte
    corrupted
}

#[cfg(not(feature = "canary-wrong-hash"))]
pub fn corrupt_hash(hash: &[u8; 32]) -> [u8; 32] {
    *hash
}

/// Canary: Commit with only f replicas instead of f+1.
///
/// **Expected Detection**: Future VSR invariants (Agreement, Prefix Property)
/// **Invariant**: commit_quorum_size (not yet implemented)
/// **Why it fails**: Violates VSR safety - can commit operations that aren't
/// on a quorum of replicas.
#[cfg(feature = "canary-commit-quorum")]
pub fn use_insufficient_quorum() -> bool {
    true // Always use f instead of f+1
}

#[cfg(not(feature = "canary-commit-quorum"))]
pub fn use_insufficient_quorum() -> bool {
    false
}

/// Canary: Record idempotency after apply instead of before.
///
/// **Expected Detection**: ClientSessionChecker
/// **Invariant**: client_session_monotonic (idempotency check)
/// **Why it fails**: Creates a race where the same request could be applied
/// twice if it arrives during a narrow window.
#[cfg(feature = "canary-idempotency-race")]
pub fn record_idempotency_after_apply() -> bool {
    true
}

#[cfg(not(feature = "canary-idempotency-race"))]
pub fn record_idempotency_after_apply() -> bool {
    false
}

/// Canary: Allow replica head (view, op) to regress.
///
/// **Expected Detection**: ReplicaHeadChecker
/// **Invariant**: replica_head_progress
/// **Why it fails**: Violates monotonicity - replica state should only move forward.
#[cfg(feature = "canary-monotonic-regression")]
pub fn allow_head_regression(rng: &mut SimRng) -> bool {
    // 1% chance to allow regression
    rng.next_bool_with_probability(0.01)
}

#[cfg(not(feature = "canary-monotonic-regression"))]
#[allow(unused_variables)]
pub fn allow_head_regression(rng: &mut SimRng) -> bool {
    false
}

/// Returns true if any canary feature is enabled.
///
/// Useful for CI to verify that at least one canary runs in each test.
pub fn any_canary_enabled() -> bool {
    cfg!(feature = "canary-skip-fsync")
        || cfg!(feature = "canary-wrong-hash")
        || cfg!(feature = "canary-commit-quorum")
        || cfg!(feature = "canary-idempotency-race")
        || cfg!(feature = "canary-monotonic-regression")
}

/// Returns a list of all enabled canaries.
pub fn enabled_canaries() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut canaries = Vec::new();

    #[cfg(feature = "canary-skip-fsync")]
    canaries.push("canary-skip-fsync");

    #[cfg(feature = "canary-wrong-hash")]
    canaries.push("canary-wrong-hash");

    #[cfg(feature = "canary-commit-quorum")]
    canaries.push("canary-commit-quorum");

    #[cfg(feature = "canary-idempotency-race")]
    canaries.push("canary-idempotency-race");

    #[cfg(feature = "canary-monotonic-regression")]
    canaries.push("canary-monotonic-regression");

    canaries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "canary-skip-fsync")]
    fn test_skip_fsync_canary_enabled() {
        let mut rng = SimRng::new(12345);

        // With enough iterations, should skip at least once
        let mut skipped = false;
        for _ in 0..10_000 {
            if should_skip_fsync(&mut rng) {
                skipped = true;
                break;
            }
        }

        assert!(
            skipped,
            "canary-skip-fsync should trigger within 10k iterations"
        );
    }

    #[test]
    #[cfg(not(feature = "canary-skip-fsync"))]
    fn test_skip_fsync_canary_disabled() {
        let mut rng = SimRng::new(12345);

        // Should never skip when canary is disabled
        for _ in 0..1_000 {
            assert!(!should_skip_fsync(&mut rng));
        }
    }

    #[test]
    #[cfg(feature = "canary-wrong-hash")]
    fn test_wrong_hash_canary_enabled() {
        let hash = [0u8; 32];
        let corrupted = corrupt_hash(&hash);

        assert_ne!(
            hash, corrupted,
            "hash should be corrupted when canary is enabled"
        );
    }

    #[test]
    #[cfg(not(feature = "canary-wrong-hash"))]
    fn test_wrong_hash_canary_disabled() {
        let hash = [0u8; 32];
        let not_corrupted = corrupt_hash(&hash);

        assert_eq!(
            hash, not_corrupted,
            "hash should not be corrupted when canary is disabled"
        );
    }

    #[test]
    fn test_canary_detection() {
        let canaries = enabled_canaries();

        if any_canary_enabled() {
            assert!(
                !canaries.is_empty(),
                "should report which canaries are enabled"
            );
            println!("Enabled canaries: {:?}", canaries);
        } else {
            assert!(
                canaries.is_empty(),
                "should report no canaries when all disabled"
            );
        }
    }

    #[test]
    #[cfg(feature = "canary-skip-fsync")]
    fn test_skip_fsync_causes_determinism_violation() {
        use crate::{SimRng, SimStorage, StorageConfig, WriteResult};

        // Create two storage instances with the same seed
        let seed = 12345u64;
        let mut rng1 = SimRng::new(seed);
        let mut rng2 = SimRng::new(seed);

        let mut storage1 = SimStorage::new(StorageConfig::default());
        let mut storage2 = SimStorage::new(StorageConfig::default());

        // Write the same data to both
        let data = vec![1, 2, 3, 4, 5];
        match storage1.write(0, data.clone(), &mut rng1) {
            WriteResult::Success { .. } => {}
            _ => panic!("write failed"),
        }
        match storage2.write(0, data.clone(), &mut rng2) {
            WriteResult::Success { .. } => {}
            _ => panic!("write failed"),
        }

        // Fsync both - with canary enabled, one might skip fsync
        // Run multiple times to increase chance of divergence
        for i in 1..100 {
            let _result1 = storage1.fsync(&mut rng1);
            let _result2 = storage2.fsync(&mut rng2);

            // Write more data
            let data = vec![i as u8; 10];
            match storage1.write(i, data.clone(), &mut rng1) {
                WriteResult::Success { .. } => {}
                _ => panic!("write failed"),
            }
            match storage2.write(i, data.clone(), &mut rng2) {
                WriteResult::Success { .. } => {}
                _ => panic!("write failed"),
            }
        }

        // Final fsync
        let _result1 = storage1.fsync(&mut rng1);
        let _result2 = storage2.fsync(&mut rng2);

        // With the canary enabled, storage hashes should eventually diverge
        // because skipped fsyncs mean data is lost on crash simulation
        //
        // Note: This test doesn't guarantee detection on every run (probabilistic),
        // but over many iterations the canary should trigger and cause divergence.
        // The actual detection happens via StorageDeterminismChecker in VOPR runs.

        println!(
            "Skip-fsync canary is active - StorageDeterminismChecker should detect divergence in VOPR runs"
        );
    }

    #[test]
    #[cfg(feature = "canary-commit-quorum")]
    fn test_commit_quorum_violation() {
        // This canary requires VSR infrastructure, which is not yet fully integrated
        // For now, just verify the canary returns true when enabled
        assert!(use_insufficient_quorum(), "canary should be enabled");
        println!(
            "Commit quorum canary is active - should be caught by future VSR Agreement invariant"
        );
    }

    #[test]
    #[cfg(feature = "canary-idempotency-race")]
    fn test_idempotency_race_enabled() {
        assert!(record_idempotency_after_apply(), "canary should be enabled");
        println!("Idempotency race canary is active - should be caught by ClientSessionChecker");
    }

    #[test]
    #[cfg(feature = "canary-monotonic-regression")]
    fn test_monotonic_regression_canary() {
        let mut rng = SimRng::new(99999);

        // With enough iterations, should allow regression at least once
        let mut allowed = false;
        for _ in 0..1000 {
            if allow_head_regression(&mut rng) {
                allowed = true;
                break;
            }
        }

        assert!(
            allowed,
            "canary should allow regression within 1000 iterations"
        );
        println!("Monotonic regression canary triggered - should be caught by ReplicaHeadChecker");
    }
}
