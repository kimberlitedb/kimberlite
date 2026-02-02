//! Canary mutations for testing VOPR's ability to detect bugs.
//!
//! This module contains intentional bugs that should be caught by invariants.
//! Each canary is gated by a feature flag and should never be enabled in production.
//!
//! # Canaries
//!
//! - `canary-skip-fsync`: Randomly skip fsync operations
//! - `canary-wrong-hash`: Use incorrect hash in chain verification
//!
//! # Usage
//!
//! ```bash
//! # Run VOPR with a canary - should detect violation
//! cargo test --features canary-skip-fsync
//! ```

use crate::rng::SimRng;

/// Canary: Skip fsync 1 in 1000 times.
///
/// This simulates a bug where fsync is occasionally skipped, leading to
/// data loss on crash. Should be caught by StorageDeterminismChecker.
#[cfg(feature = "canary-skip-fsync")]
pub fn should_skip_fsync(rng: &mut SimRng) -> bool {
    rng.next_bool_with_probability(0.001) // 0.1% chance
}

#[cfg(not(feature = "canary-skip-fsync"))]
pub fn should_skip_fsync(_rng: &mut SimRng) -> bool {
    false
}

/// Canary: Corrupt hash chain by using wrong previous hash.
///
/// This simulates a bug where the hash chain is broken. Should be
/// caught by HashChainChecker.
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
        
        assert!(skipped, "canary-skip-fsync should trigger");
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
        
        assert_ne!(hash, corrupted, "hash should be corrupted");
    }
    
    #[test]
    #[cfg(not(feature = "canary-wrong-hash"))]
    fn test_wrong_hash_canary_disabled() {
        let hash = [0u8; 32];
        let not_corrupted = corrupt_hash(&hash);
        
        assert_eq!(hash, not_corrupted, "hash should not be corrupted");
    }
}
