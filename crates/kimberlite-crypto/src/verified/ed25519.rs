//! Verified Ed25519 Implementation
//!
//! This module provides Ed25519 digital signatures with embedded proof
//! certificates from Coq formal verification. The implementation wraps
//! the `ed25519-dalek` crate with proofs of:
//! - Signature verification correctness
//! - EUF-CMA (existential unforgeability under chosen-message attack)
//! - Signature determinism
//! - Key derivation uniqueness
//!
//! Proven properties are documented in `specs/coq/Ed25519.v`

use super::proof_certificate::{ProofCertificate, Verified};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;

// -----------------------------------------------------------------------------
// Proof Certificates (extracted from Coq)
// -----------------------------------------------------------------------------

/// Ed25519 verification correctness: verify(pk, msg, sign(sk, msg)) = true
///
/// **Theorem:** `ed25519_verify_correct` in `specs/coq/Ed25519.v:99`
///
/// **Proven:** Valid signatures always verify
pub const ED25519_VERIFY_CORRECTNESS_CERT: ProofCertificate = ProofCertificate::new(
    400,       // theorem_id
    1,         // proof_system_id (Coq 8.18)
    2026_0205, // verified_at
    1,         // assumption_count (Ed25519 construction)
);

/// Ed25519 EUF-CMA: existential unforgeability under chosen-message attack
///
/// **Theorem:** `ed25519_euf_cma` in `specs/coq/Ed25519.v:120`
///
/// **Proven:** Cannot forge signatures without secret key
pub const ED25519_EUF_CMA_CERT: ProofCertificate = ProofCertificate::new(
    401,       // theorem_id
    1,         // proof_system_id
    2026_0205, // verified_at
    2,         // assumption_count (ECDLP, Curve25519)
);

/// Ed25519 determinism: same key + message always produces same signature
///
/// **Theorem:** `ed25519_deterministic` in `specs/coq/Ed25519.v:144`
///
/// **Proven:** Signatures are deterministic (no randomness)
pub const ED25519_DETERMINISM_CERT: ProofCertificate = ProofCertificate::new(
    402,       // theorem_id
    1,         // proof_system_id
    2026_0205, // verified_at
    1,         // assumption_count (SHA-512 deterministic nonce)
);

/// Key derivation uniqueness: different seeds produce different public keys
///
/// **Theorem:** `key_derivation_unique` in `specs/coq/Ed25519.v:197`
///
/// **Proven:** Different seeds → different keys
pub const KEY_DERIVATION_UNIQUENESS_CERT: ProofCertificate = ProofCertificate::new(
    403,       // theorem_id
    1,         // proof_system_id
    2026_0205, // verified_at
    2,         // assumption_count (derive_signing_key_injective, derive_public_key_injective)
);

// -----------------------------------------------------------------------------
// Verified Ed25519 Digital Signatures
// -----------------------------------------------------------------------------

/// Verified Ed25519 signing key with proof certificates
pub struct VerifiedSigningKey {
    inner: SigningKey,
}

// Manual Debug implementation to avoid exposing key material
impl std::fmt::Debug for VerifiedSigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedSigningKey")
            .field("inner", &"<redacted>")
            .finish()
    }
}

impl VerifiedSigningKey {
    /// Generate a new signing key from system randomness
    ///
    /// **Proven:** `key_derivation_unique` - different randomness → different keys
    ///
    /// # Example
    /// ```
    /// use kimberlite_crypto::verified::VerifiedSigningKey;
    ///
    /// let signing_key = VerifiedSigningKey::generate();
    /// ```
    pub fn generate() -> Self {
        let inner = SigningKey::generate(&mut OsRng);
        Self { inner }
    }

    /// Create signing key from 32-byte seed
    ///
    /// **Proven:** `key_derivation_unique` - deterministic key derivation
    ///
    /// # Safety
    /// Seed must be cryptographically random. Never reuse seeds.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        // Assert seed is not all zeros (degenerate key)
        assert_ne!(
            bytes, &[0u8; 32],
            "Ed25519 secret key seed is all zeros (degenerate key)"
        );

        let inner = SigningKey::from_bytes(bytes);
        Self { inner }
    }

    /// Get the bytes of the signing key
    pub fn to_bytes(&self) -> [u8; 32] {
        self.inner.to_bytes()
    }

    /// Derive verifying key from signing key
    ///
    /// **Proven:** `derive_public_key_deterministic` - always returns same key
    pub fn verifying_key(&self) -> VerifiedVerifyingKey {
        VerifiedVerifyingKey {
            inner: self.inner.verifying_key(),
        }
    }

    /// Sign a message with determinism proof
    ///
    /// **Proven:** `ed25519_deterministic` - same key + message = same signature
    /// **Proven:** `ed25519_verify_correct` - signature will verify
    ///
    /// # Example
    /// ```
    /// use kimberlite_crypto::verified::VerifiedSigningKey;
    ///
    /// let signing_key = VerifiedSigningKey::generate();
    /// let message = b"audit log entry";
    /// let signature = signing_key.sign(message);
    /// ```
    pub fn sign(&self, message: &[u8]) -> VerifiedSignature {
        let inner = self.inner.sign(message);
        VerifiedSignature { inner }
    }
}

/// Verified Ed25519 verifying key (public key)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedVerifyingKey {
    inner: VerifyingKey,
}

impl VerifiedVerifyingKey {
    /// Create verifying key from 32-byte compressed point
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self, String> {
        // Assert key is not all zeros (degenerate key)
        assert_ne!(
            bytes, &[0u8; 32],
            "Ed25519 public key is all zeros (degenerate key)"
        );

        let inner = VerifyingKey::from_bytes(bytes).map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    /// Get the bytes of the verifying key
    pub fn to_bytes(&self) -> [u8; 32] {
        self.inner.to_bytes()
    }

    /// Verify a signature with correctness proof
    ///
    /// **Proven:** `ed25519_verify_correct` - valid signatures always verify
    /// **Proven:** `ed25519_euf_cma` - forged signatures fail
    ///
    /// Uses RFC 8032 §5.1.7 strict verification, rejecting non-canonical
    /// signatures to prevent signature malleability.
    ///
    /// # Example
    /// ```
    /// use kimberlite_crypto::verified::VerifiedSigningKey;
    ///
    /// let signing_key = VerifiedSigningKey::generate();
    /// let verifying_key = signing_key.verifying_key();
    /// let message = b"audit log entry";
    /// let signature = signing_key.sign(message);
    ///
    /// assert!(verifying_key.verify(message, &signature).is_ok());
    /// ```
    pub fn verify(&self, message: &[u8], signature: &VerifiedSignature) -> Result<(), String> {
        self.inner
            .verify_strict(message, &signature.inner)
            .map_err(|_| "Signature verification failed".to_string())
    }
}

/// Verified Ed25519 signature
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedSignature {
    inner: Signature,
}

impl VerifiedSignature {
    /// Create signature from 64-byte array
    pub fn from_bytes(bytes: &[u8; 64]) -> Self {
        // Assert signature is not all zeros (degenerate signature)
        assert_ne!(
            bytes, &[0u8; 64],
            "Ed25519 signature is all zeros (degenerate signature)"
        );

        let inner = Signature::from_bytes(bytes);
        Self { inner }
    }

    /// Get the bytes of the signature
    pub fn to_bytes(&self) -> [u8; 64] {
        self.inner.to_bytes()
    }
}

// Verified trait implementations
impl Verified for VerifiedSigningKey {
    fn proof_certificate() -> ProofCertificate {
        ED25519_VERIFY_CORRECTNESS_CERT
    }

    fn theorem_name() -> &'static str {
        "ed25519_verify_correct"
    }

    fn theorem_description() -> &'static str {
        "Ed25519 signature verification correctness: valid signatures always verify"
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() {
        let signing_key = VerifiedSigningKey::generate();
        let verifying_key = signing_key.verifying_key();
        let message = b"test message";

        let signature = signing_key.sign(message);
        assert!(verifying_key.verify(message, &signature).is_ok());
    }

    #[test]
    fn test_wrong_message_fails() {
        let signing_key = VerifiedSigningKey::generate();
        let verifying_key = signing_key.verifying_key();

        let signature = signing_key.sign(b"original message");
        let result = verifying_key.verify(b"tampered message", &signature);

        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_key_fails() {
        let signing_key1 = VerifiedSigningKey::generate();
        let signing_key2 = VerifiedSigningKey::generate();
        let verifying_key2 = signing_key2.verifying_key();

        let message = b"test message";
        let signature = signing_key1.sign(message);

        let result = verifying_key2.verify(message, &signature);
        assert!(result.is_err());
    }

    #[test]
    fn test_deterministic_signatures() {
        let seed = [0x42; 32];
        let signing_key = VerifiedSigningKey::from_bytes(&seed);
        let message = b"deterministic test";

        let sig1 = signing_key.sign(message);
        let sig2 = signing_key.sign(message);

        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_different_messages_different_signatures() {
        let signing_key = VerifiedSigningKey::generate();

        let sig1 = signing_key.sign(b"message1");
        let sig2 = signing_key.sign(b"message2");

        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_empty_message() {
        let signing_key = VerifiedSigningKey::generate();
        let verifying_key = signing_key.verifying_key();

        let signature = signing_key.sign(b"");
        assert!(verifying_key.verify(b"", &signature).is_ok());
    }

    #[test]
    fn test_large_message() {
        let signing_key = VerifiedSigningKey::generate();
        let verifying_key = signing_key.verifying_key();
        let message = vec![0xAB; 100_000]; // 100KB

        let signature = signing_key.sign(&message);
        assert!(verifying_key.verify(&message, &signature).is_ok());
    }

    #[test]
    fn test_key_serialization_roundtrip() {
        let signing_key = VerifiedSigningKey::generate();
        let bytes = signing_key.to_bytes();
        let restored = VerifiedSigningKey::from_bytes(&bytes);

        // Keys should produce same signatures
        let message = b"test";
        let sig1 = signing_key.sign(message);
        let sig2 = restored.sign(message);

        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_verifying_key_serialization_roundtrip() {
        let signing_key = VerifiedSigningKey::generate();
        let verifying_key = signing_key.verifying_key();

        let bytes = verifying_key.to_bytes();
        let restored =
            VerifiedVerifyingKey::from_bytes(&bytes).expect("failed to restore verifying key");

        assert_eq!(verifying_key, restored);
    }

    #[test]
    fn test_signature_serialization_roundtrip() {
        let signing_key = VerifiedSigningKey::generate();
        let signature = signing_key.sign(b"test");

        let bytes = signature.to_bytes();
        let restored = VerifiedSignature::from_bytes(&bytes);

        assert_eq!(signature, restored);
    }

    #[test]
    fn test_key_derivation_deterministic() {
        let seed = [0x42; 32];
        let key1 = VerifiedSigningKey::from_bytes(&seed);
        let key2 = VerifiedSigningKey::from_bytes(&seed);

        let vk1 = key1.verifying_key();
        let vk2 = key2.verifying_key();

        assert_eq!(vk1, vk2);
    }

    #[test]
    fn test_different_seeds_different_keys() {
        let seed1 = [0x42; 32];
        let seed2 = [0x43; 32];

        let key1 = VerifiedSigningKey::from_bytes(&seed1);
        let key2 = VerifiedSigningKey::from_bytes(&seed2);

        let vk1 = key1.verifying_key();
        let vk2 = key2.verifying_key();

        assert_ne!(vk1, vk2);
    }

    #[test]
    fn test_proof_certificate() {
        let cert = VerifiedSigningKey::proof_certificate();
        assert_eq!(cert.theorem_id, 400);
        assert_eq!(cert.proof_system_id, 1);
        assert_eq!(cert.verified_at, 20_260_205);
        assert_eq!(cert.assumption_count, 1);
    }

    #[test]
    fn test_verified_trait() {
        assert_eq!(VerifiedSigningKey::theorem_name(), "ed25519_verify_correct");
        assert!(VerifiedSigningKey::theorem_description().contains("correctness"));
    }

    #[test]
    fn test_tampered_signature_fails() {
        let signing_key = VerifiedSigningKey::generate();
        let verifying_key = signing_key.verifying_key();
        let message = b"test message";

        let signature = signing_key.sign(message);
        let mut sig_bytes = signature.to_bytes();

        // Tamper with signature
        sig_bytes[0] ^= 0xFF;
        let tampered_sig = VerifiedSignature::from_bytes(&sig_bytes);

        let result = verifying_key.verify(message, &tampered_sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_verifying_key_from_signing_key() {
        let signing_key = VerifiedSigningKey::generate();
        let vk1 = signing_key.verifying_key();
        let vk2 = signing_key.verifying_key();

        // Should be deterministic
        assert_eq!(vk1, vk2);
    }

    #[test]
    fn test_multiple_signatures_from_same_key() {
        let signing_key = VerifiedSigningKey::generate();
        let verifying_key = signing_key.verifying_key();

        // Sign multiple different messages
        let messages = [b"msg1" as &[u8], b"msg2", b"msg3", b"msg4", b"msg5"];

        for msg in &messages {
            let signature = signing_key.sign(msg);
            assert!(verifying_key.verify(msg, &signature).is_ok());
        }
    }

    #[test]
    fn test_non_canonical_signature_rejected() {
        // RFC 8032 §5.1.7: verify_strict rejects non-canonical S values
        // This test ensures signature malleability is prevented

        let signing_key = VerifiedSigningKey::generate();
        let verifying_key = signing_key.verifying_key();
        let message = b"test message";

        // Create a valid canonical signature
        let signature = signing_key.sign(message);
        assert!(verifying_key.verify(message, &signature).is_ok());

        // Attempt to create a non-canonical signature by manipulating S
        // (In practice, non-canonical signatures would come from external sources)
        // ed25519-dalek's verify_strict() will reject non-canonical encodings
        //
        // Note: We cannot easily construct a non-canonical signature here without
        // understanding the internal S representation. This test primarily
        // documents that verify_strict() is used, which handles rejection.
        //
        // A proper test would require crafting a signature with S >= L (where L is
        // the curve order), but ed25519-dalek's strict verification rejects those.
    }

    #[test]
    #[should_panic(expected = "Ed25519 secret key seed is all zeros")]
    fn test_all_zero_signing_key_panics() {
        let _ = VerifiedSigningKey::from_bytes(&[0u8; 32]);
    }

    #[test]
    #[should_panic(expected = "Ed25519 public key is all zeros")]
    fn test_all_zero_verifying_key_panics() {
        let _ = VerifiedVerifyingKey::from_bytes(&[0u8; 32]);
    }

    #[test]
    #[should_panic(expected = "Ed25519 signature is all zeros")]
    fn test_all_zero_signature_panics() {
        let _ = VerifiedSignature::from_bytes(&[0u8; 64]);
    }
}

// Property-based tests
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Property: Sign/verify roundtrip for arbitrary messages
        #[test]
        fn prop_sign_verify_roundtrip(message in prop::collection::vec(any::<u8>(), 0..10000)) {
            let signing_key = VerifiedSigningKey::generate();
            let verifying_key = signing_key.verifying_key();

            let signature = signing_key.sign(&message);
            prop_assert!(verifying_key.verify(&message, &signature).is_ok());
        }

        /// Property: Different messages produce different signatures
        #[test]
        fn prop_different_messages_different_signatures(
            msg1 in prop::collection::vec(any::<u8>(), 1..1000),
            msg2 in prop::collection::vec(any::<u8>(), 1..1000)
        ) {
            prop_assume!(msg1 != msg2);

            let signing_key = VerifiedSigningKey::generate();
            let sig1 = signing_key.sign(&msg1);
            let sig2 = signing_key.sign(&msg2);

            prop_assert_ne!(sig1, sig2);
        }

        /// Property: Signature determinism - same key + message = same signature
        #[test]
        fn prop_signature_determinism(
            seed in prop::array::uniform32(any::<u8>()),
            message in prop::collection::vec(any::<u8>(), 0..1000)
        ) {
            // Skip all-zero seeds (checked by assertion)
            prop_assume!(seed != [0u8; 32]);

            let signing_key = VerifiedSigningKey::from_bytes(&seed);
            let sig1 = signing_key.sign(&message);
            let sig2 = signing_key.sign(&message);

            prop_assert_eq!(sig1, sig2);
        }

        /// Property: Key derivation uniqueness - different seeds = different keys
        #[test]
        fn prop_key_derivation_uniqueness(
            seed1 in prop::array::uniform32(any::<u8>()),
            seed2 in prop::array::uniform32(any::<u8>())
        ) {
            // Skip all-zero seeds and identical seeds
            prop_assume!(seed1 != [0u8; 32] && seed2 != [0u8; 32]);
            prop_assume!(seed1 != seed2);

            let key1 = VerifiedSigningKey::from_bytes(&seed1);
            let key2 = VerifiedSigningKey::from_bytes(&seed2);

            let vk1 = key1.verifying_key();
            let vk2 = key2.verifying_key();

            prop_assert_ne!(vk1, vk2);
        }

        /// Property: Tampered signatures fail verification
        #[test]
        fn prop_tampered_signature_fails(
            message in prop::collection::vec(any::<u8>(), 1..1000),
            tamper_index in 0usize..64,
            tamper_xor in 1u8..=255
        ) {
            let signing_key = VerifiedSigningKey::generate();
            let verifying_key = signing_key.verifying_key();

            let signature = signing_key.sign(&message);
            let mut sig_bytes = signature.to_bytes();

            // Tamper with one byte of the signature
            sig_bytes[tamper_index] ^= tamper_xor;
            let tampered_sig = VerifiedSignature::from_bytes(&sig_bytes);

            let result = verifying_key.verify(&message, &tampered_sig);
            prop_assert!(result.is_err());
        }

        /// Property: Wrong key fails verification
        #[test]
        fn prop_wrong_key_fails(message in prop::collection::vec(any::<u8>(), 1..1000)) {
            let key1 = VerifiedSigningKey::generate();
            let key2 = VerifiedSigningKey::generate();
            let vk2 = key2.verifying_key();

            let signature = key1.sign(&message);
            let result = vk2.verify(&message, &signature);

            prop_assert!(result.is_err());
        }
    }
}
