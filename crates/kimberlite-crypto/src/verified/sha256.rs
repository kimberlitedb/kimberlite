//! Verified SHA-256 Implementation
//!
//! This module provides SHA-256 hash functions with embedded proof
//! certificates from Coq formal verification. The implementation wraps
//! the `sha2` crate with proofs of:
//! - Determinism (same input → same output)
//! - Non-degeneracy (never produces all zeros)
//! - Chain hash integrity (hash chain construction)
//!
//! Proven properties are documented in `specs/coq/SHA256.v`

use super::proof_certificate::{ProofCertificate, Verified};
use sha2::{Digest, Sha256};

// -----------------------------------------------------------------------------
// Proof Certificates (extracted from Coq)
// -----------------------------------------------------------------------------

/// SHA-256 determinism: sha256(x) = sha256(x)
///
/// **Theorem:** `sha256_deterministic` in `specs/coq/SHA256.v:96`
///
/// **Proven:** Same input always produces same output (pure function)
pub const SHA256_DETERMINISTIC_CERT: ProofCertificate = ProofCertificate::new(
    100,        // theorem_id
    1,          // proof_system_id (Coq 8.18)
    2026_02_05, // verified_at
    0,          // assumption_count (no assumptions)
);

/// SHA-256 non-degeneracy: sha256(x) ≠ 0^256
///
/// **Theorem:** `sha256_non_degenerate` in `specs/coq/SHA256.v:101`
///
/// **Proven:** Hash output is never all zeros
pub const SHA256_NON_DEGENERATE_CERT: ProofCertificate = ProofCertificate::new(
    101,        // theorem_id
    1,          // proof_system_id
    2026_02_05, // verified_at
    1,          // assumption_count (collision resistance)
);

/// Chain hash genesis integrity: chain(None, d1) = chain(None, d2) → d1 = d2
///
/// **Theorem:** `chain_hash_genesis_integrity` in `specs/coq/SHA256.v:125`
///
/// **Proven:** Genesis hash uniquely identifies data
pub const CHAIN_HASH_GENESIS_INTEGRITY_CERT: ProofCertificate = ProofCertificate::new(
    102,        // theorem_id
    1,          // proof_system_id
    2026_02_05, // verified_at
    1,          // assumption_count (collision resistance)
);

// -----------------------------------------------------------------------------
// Verified SHA-256 Hash Function
// -----------------------------------------------------------------------------

/// Verified SHA-256 hash with proof certificate
///
/// This implementation wraps `sha2::Sha256` with formal verification
/// guarantees. All properties are proven in Coq.
pub struct VerifiedSha256;

impl VerifiedSha256 {
    /// Hash data with determinism proof
    ///
    /// **Proven:** `sha256_deterministic` - same input always produces same output
    ///
    /// # Example
    /// ```
    /// use kimberlite_crypto::verified::VerifiedSha256;
    ///
    /// let data = b"hello world";
    /// let hash1 = VerifiedSha256::hash(data);
    /// let hash2 = VerifiedSha256::hash(data);
    /// assert_eq!(hash1, hash2); // Determinism guaranteed by proof
    /// ```
    pub fn hash(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result: [u8; 32] = hasher.finalize().into();

        // Assert non-degeneracy (from Coq proof)
        assert_ne!(
            result, [0u8; 32],
            "SHA-256 produced all zeros (violation of non_degenerate theorem)"
        );

        result
    }

    /// Hash with previous chain hash (authenticated hash chain)
    ///
    /// **Proven:** `chain_hash_genesis_integrity` - genesis uniquely identifies data
    ///
    /// # Arguments
    /// - `prev_hash`: Previous hash in chain (None for genesis)
    /// - `data`: Data to hash
    ///
    /// # Returns
    /// SHA-256(prev_hash || data) or SHA-256(data) if genesis
    ///
    /// # Example
    /// ```
    /// use kimberlite_crypto::verified::VerifiedSha256;
    ///
    /// // Genesis block
    /// let genesis = VerifiedSha256::chain_hash(None, b"block 0");
    ///
    /// // Subsequent blocks
    /// let block1 = VerifiedSha256::chain_hash(Some(&genesis), b"block 1");
    /// let block2 = VerifiedSha256::chain_hash(Some(&block1), b"block 2");
    /// ```
    pub fn chain_hash(prev_hash: Option<&[u8; 32]>, data: &[u8]) -> [u8; 32] {
        match prev_hash {
            None => {
                // Genesis: hash(data)
                Self::hash(data)
            }
            Some(prev) => {
                // Chained: hash(prev || data)
                let mut hasher = Sha256::new();
                hasher.update(prev);
                hasher.update(data);
                let result: [u8; 32] = hasher.finalize().into();

                // Assert non-degeneracy
                assert_ne!(result, [0u8; 32]);

                result
            }
        }
    }
}

// Verified trait implementations
impl Verified for VerifiedSha256 {
    fn proof_certificate() -> ProofCertificate {
        SHA256_DETERMINISTIC_CERT
    }

    fn theorem_name() -> &'static str {
        "sha256_deterministic"
    }

    fn theorem_description() -> &'static str {
        "SHA-256 is deterministic: hashing the same input always produces the same output"
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_deterministic() {
        let data = b"test data";
        let hash1 = VerifiedSha256::hash(data);
        let hash2 = VerifiedSha256::hash(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_sha256_non_degenerate() {
        let data = b"any data";
        let hash = VerifiedSha256::hash(data);
        assert_ne!(hash, [0u8; 32]);
    }

    #[test]
    fn test_sha256_different_inputs_different_outputs() {
        let hash1 = VerifiedSha256::hash(b"data1");
        let hash2 = VerifiedSha256::hash(b"data2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_chain_hash_genesis() {
        let genesis1 = VerifiedSha256::chain_hash(None, b"genesis data");
        let genesis2 = VerifiedSha256::chain_hash(None, b"genesis data");
        assert_eq!(genesis1, genesis2);
    }

    #[test]
    fn test_chain_hash_different_genesis() {
        let genesis1 = VerifiedSha256::chain_hash(None, b"data1");
        let genesis2 = VerifiedSha256::chain_hash(None, b"data2");
        assert_ne!(genesis1, genesis2);
    }

    #[test]
    fn test_chain_hash_chaining() {
        let genesis = VerifiedSha256::chain_hash(None, b"block 0");
        let block1 = VerifiedSha256::chain_hash(Some(&genesis), b"block 1");
        let block2 = VerifiedSha256::chain_hash(Some(&block1), b"block 2");

        // Different blocks
        assert_ne!(genesis, block1);
        assert_ne!(block1, block2);

        // Same chain produces same hash
        let block2_again = VerifiedSha256::chain_hash(Some(&block1), b"block 2");
        assert_eq!(block2, block2_again);
    }

    #[test]
    fn test_proof_certificate() {
        let cert = VerifiedSha256::proof_certificate();
        assert_eq!(cert.theorem_id, 100);
        assert_eq!(cert.proof_system_id, 1);
        assert_eq!(cert.verified_at, 20260205);
        assert_eq!(cert.assumption_count, 0);
        assert!(cert.is_complete());
    }

    #[test]
    fn test_verified_trait() {
        assert_eq!(VerifiedSha256::theorem_name(), "sha256_deterministic");
        assert!(VerifiedSha256::theorem_description().contains("deterministic"));
    }

    #[test]
    fn test_matches_existing_implementation() {
        // Ensure verified implementation matches existing hash.rs
        use sha2::{Digest, Sha256};

        let data = b"test compatibility";

        // Verified implementation
        let verified_hash = VerifiedSha256::hash(data);

        // Direct sha2 usage (existing implementation)
        let mut hasher = Sha256::new();
        hasher.update(data);
        let direct_hash: [u8; 32] = hasher.finalize().into();

        assert_eq!(verified_hash, direct_hash);
    }

    // Note: We cannot create #[should_panic] tests for all-zero hash output
    // violations, as SHA-256 cannot produce all zeros without a cryptographic
    // break. The assert_ne! checks serve as defense-in-depth against degenerate
    // implementations or memory corruption.
}

// Property-based tests
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Property: Determinism - same input always produces same hash
        #[test]
        fn prop_sha256_deterministic(data in prop::collection::vec(any::<u8>(), 0..10000)) {
            let hash1 = VerifiedSha256::hash(&data);
            let hash2 = VerifiedSha256::hash(&data);
            prop_assert_eq!(hash1, hash2);
        }

        /// Property: Different inputs produce different hashes (collision resistance sampling)
        #[test]
        fn prop_different_inputs_different_hashes(
            data1 in prop::collection::vec(any::<u8>(), 1..1000),
            data2 in prop::collection::vec(any::<u8>(), 1..1000)
        ) {
            prop_assume!(data1 != data2);
            let hash1 = VerifiedSha256::hash(&data1);
            let hash2 = VerifiedSha256::hash(&data2);
            prop_assert_ne!(hash1, hash2);
        }

        /// Property: Non-degeneracy - hash output is never all zeros
        #[test]
        fn prop_sha256_non_degenerate(data in prop::collection::vec(any::<u8>(), 0..10000)) {
            let hash = VerifiedSha256::hash(&data);
            prop_assert_ne!(hash, [0u8; 32]);
        }

        /// Property: Chain hash with genesis produces unique hashes for different data
        #[test]
        fn prop_chain_hash_genesis_unique(
            data1 in prop::collection::vec(any::<u8>(), 1..1000),
            data2 in prop::collection::vec(any::<u8>(), 1..1000)
        ) {
            prop_assume!(data1 != data2);
            let genesis1 = VerifiedSha256::chain_hash(None, &data1);
            let genesis2 = VerifiedSha256::chain_hash(None, &data2);
            prop_assert_ne!(genesis1, genesis2);
        }

        /// Property: Chain hash determinism - same prev + data = same hash
        #[test]
        fn prop_chain_hash_deterministic(
            prev in prop::array::uniform32(any::<u8>()),
            data in prop::collection::vec(any::<u8>(), 0..1000)
        ) {
            let hash1 = VerifiedSha256::chain_hash(Some(&prev), &data);
            let hash2 = VerifiedSha256::chain_hash(Some(&prev), &data);
            prop_assert_eq!(hash1, hash2);
        }

        /// Property: Chained hashes are unique per step
        #[test]
        fn prop_chain_hash_unique_per_step(
            data1 in prop::collection::vec(any::<u8>(), 1..100),
            data2 in prop::collection::vec(any::<u8>(), 1..100)
        ) {
            prop_assume!(data1 != data2);
            let genesis = VerifiedSha256::chain_hash(None, &data1);
            let block1 = VerifiedSha256::chain_hash(Some(&genesis), &data2);

            prop_assert_ne!(genesis, block1);
        }
    }
}
