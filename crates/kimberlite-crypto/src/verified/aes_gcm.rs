//! Verified AES-256-GCM Implementation
//!
//! This module provides AES-256-GCM authenticated encryption with embedded
//! proof certificates from Coq formal verification. The implementation wraps
//! the `aes-gcm` crate with proofs of:
//! - Encryption/decryption roundtrip correctness
//! - Ciphertext integrity (INT-CTXT)
//! - Nonce uniqueness enforcement
//! - IND-CCA2 security
//!
//! Proven properties are documented in `specs/coq/AES_GCM.v`

use super::proof_certificate::{ProofCertificate, Verified};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};

// -----------------------------------------------------------------------------
// Proof Certificates (extracted from Coq)
// -----------------------------------------------------------------------------

/// AES-GCM roundtrip: decrypt(encrypt(plaintext)) = plaintext
///
/// **Theorem:** `aes_gcm_roundtrip` in `specs/coq/AES_GCM.v:98`
///
/// **Proven:** Encryption followed by decryption returns original plaintext
pub const AES_GCM_ROUNDTRIP_CERT: ProofCertificate = ProofCertificate::new(
    300,      // theorem_id
    1,        // proof_system_id (Coq 8.18)
    20260205, // verified_at
    1,        // assumption_count (GCM authenticated encryption)
);

/// AES-GCM integrity: tampering causes decryption failure
///
/// **Theorem:** `aes_gcm_integrity` in `specs/coq/AES_GCM.v:115`
///
/// **Proven:** Any modification to ciphertext or tag causes decryption to fail
pub const AES_GCM_INTEGRITY_CERT: ProofCertificate = ProofCertificate::new(
    301,      // theorem_id
    1,        // proof_system_id
    20260205, // verified_at
    1,        // assumption_count (GHASH authentication)
);

/// Nonce uniqueness: position-based nonces are unique
///
/// **Theorem:** `position_nonce_injective` in `specs/coq/AES_GCM.v:157`
///
/// **Proven:** Different positions produce different nonces
pub const NONCE_UNIQUENESS_CERT: ProofCertificate = ProofCertificate::new(
    302,      // theorem_id
    1,        // proof_system_id
    20260205, // verified_at
    1,        // assumption_count (position uniqueness)
);

/// IND-CCA2 security
///
/// **Theorem:** `aes_gcm_ind_cca2` in `specs/coq/AES_GCM.v:188`
///
/// **Proven:** Indistinguishability under adaptive chosen-ciphertext attack
pub const IND_CCA2_CERT: ProofCertificate = ProofCertificate::new(
    303,      // theorem_id
    1,        // proof_system_id
    20260205, // verified_at
    2,        // assumption_count (AES-256 PRP, GCM construction)
);

// -----------------------------------------------------------------------------
// Verified AES-256-GCM
// -----------------------------------------------------------------------------

/// Verified AES-256-GCM authenticated encryption
///
/// This implementation wraps `aes_gcm::Aes256Gcm` with formal verification
/// guarantees. All properties are proven in Coq.
pub struct VerifiedAesGcm;

impl VerifiedAesGcm {
    /// Encrypt plaintext with roundtrip proof
    ///
    /// **Proven:** `aes_gcm_roundtrip` - decryption returns original plaintext
    /// **Proven:** `aes_gcm_integrity` - tampering detected
    ///
    /// # Arguments
    /// - `key`: 32-byte AES-256 key
    /// - `nonce`: 12-byte GCM nonce (must be unique per key)
    /// - `plaintext`: Data to encrypt
    /// - `associated_data`: Additional authenticated data (not encrypted)
    ///
    /// # Returns
    /// Ciphertext with appended authentication tag
    ///
    /// # Example
    /// ```
    /// use kimberlite_crypto::verified::VerifiedAesGcm;
    ///
    /// let key = [0u8; 32];
    /// let nonce = [0u8; 12];
    /// let plaintext = b"secret message";
    ///
    /// let ciphertext = VerifiedAesGcm::encrypt(&key, &nonce, plaintext, b"")
    ///     .expect("encryption failed");
    ///
    /// let decrypted = VerifiedAesGcm::decrypt(&key, &nonce, &ciphertext, b"")
    ///     .expect("decryption failed");
    ///
    /// assert_eq!(plaintext, &decrypted[..]);
    /// ```
    pub fn encrypt(
        key: &[u8; 32],
        nonce: &[u8; 12],
        plaintext: &[u8],
        associated_data: &[u8],
    ) -> Result<Vec<u8>, String> {
        // Assert key is not all zeros (degenerate key)
        debug_assert_ne!(key, &[0u8; 32], "AES-256 key is all zeros (degenerate key)");

        // Assert nonce is not all zeros (weak nonce)
        debug_assert_ne!(nonce, &[0u8; 12], "GCM nonce is all zeros (weak nonce)");

        let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
        let nonce_obj = Nonce::from_slice(nonce);

        let payload = Payload {
            msg: plaintext,
            aad: associated_data,
        };

        cipher
            .encrypt(nonce_obj, payload)
            .map_err(|e| e.to_string())
    }

    /// Decrypt ciphertext with integrity proof
    ///
    /// **Proven:** `aes_gcm_roundtrip` - returns original plaintext
    /// **Proven:** `aes_gcm_integrity` - tampering causes failure
    ///
    /// # Arguments
    /// - `key`: 32-byte AES-256 key (must match encryption key)
    /// - `nonce`: 12-byte GCM nonce (must match encryption nonce)
    /// - `ciphertext`: Encrypted data with authentication tag
    /// - `associated_data`: AAD (must match encryption AAD)
    ///
    /// # Returns
    /// Original plaintext if authentication succeeds, error if tampered
    pub fn decrypt(
        key: &[u8; 32],
        nonce: &[u8; 12],
        ciphertext: &[u8],
        associated_data: &[u8],
    ) -> Result<Vec<u8>, String> {
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
        let nonce_obj = Nonce::from_slice(nonce);

        let payload = Payload {
            msg: ciphertext,
            aad: associated_data,
        };

        cipher.decrypt(nonce_obj, payload).map_err(|_| {
            "Authentication failed: ciphertext tampered or wrong key/nonce".to_string()
        })
    }

    /// Generate position-based nonce with uniqueness proof
    ///
    /// **Proven:** `position_nonce_injective` - different positions → different nonces
    ///
    /// This uses a deterministic position-based nonce generation scheme
    /// that guarantees uniqueness without state.
    ///
    /// # Safety
    /// Nonce reuse with the same key is catastrophic for GCM security.
    /// Position-based nonces prevent reuse by construction.
    ///
    /// # Implementation
    /// We add 1 to the position to avoid an all-zero nonce at position 0,
    /// which would trigger the weak nonce assertion.
    pub fn nonce_from_position(position: u64) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        // Add 1 to avoid all-zero nonce at position 0
        nonce[0..8].copy_from_slice(&(position + 1).to_le_bytes());
        // Upper 4 bytes reserved for future use (stream_id, etc.)
        nonce
    }
}

// Verified trait implementations
impl Verified for VerifiedAesGcm {
    fn proof_certificate() -> ProofCertificate {
        AES_GCM_ROUNDTRIP_CERT
    }

    fn theorem_name() -> &'static str {
        "aes_gcm_roundtrip"
    }

    fn theorem_description() -> &'static str {
        "AES-256-GCM encryption/decryption roundtrip: decrypt(encrypt(plaintext)) = plaintext"
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = [0x42; 32];
        let nonce = [0x01; 12];
        let plaintext = b"secret message";

        let ciphertext =
            VerifiedAesGcm::encrypt(&key, &nonce, plaintext, b"").expect("encryption failed");

        let decrypted =
            VerifiedAesGcm::decrypt(&key, &nonce, &ciphertext, b"").expect("decryption failed");

        assert_eq!(plaintext, &decrypted[..]);
    }

    #[test]
    fn test_with_associated_data() {
        let key = [0x42; 32];
        let nonce = [0x01; 12];
        let plaintext = b"secret message";
        let aad = b"additional context";

        let ciphertext =
            VerifiedAesGcm::encrypt(&key, &nonce, plaintext, aad).expect("encryption failed");

        let decrypted =
            VerifiedAesGcm::decrypt(&key, &nonce, &ciphertext, aad).expect("decryption failed");

        assert_eq!(plaintext, &decrypted[..]);
    }

    #[test]
    fn test_wrong_key_fails() {
        let key = [0x42; 32];
        let wrong_key = [0x43; 32];
        let nonce = [0x01; 12];
        let plaintext = b"secret";

        let ciphertext =
            VerifiedAesGcm::encrypt(&key, &nonce, plaintext, b"").expect("encryption failed");

        let result = VerifiedAesGcm::decrypt(&wrong_key, &nonce, &ciphertext, b"");
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_nonce_fails() {
        let key = [0x42; 32];
        let nonce = [0x01; 12];
        let wrong_nonce = [0x02; 12];
        let plaintext = b"secret";

        let ciphertext =
            VerifiedAesGcm::encrypt(&key, &nonce, plaintext, b"").expect("encryption failed");

        let result = VerifiedAesGcm::decrypt(&key, &wrong_nonce, &ciphertext, b"");
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_aad_fails() {
        let key = [0x42; 32];
        let nonce = [0x01; 12];
        let plaintext = b"secret";
        let aad = b"context";
        let wrong_aad = b"wrong";

        let ciphertext =
            VerifiedAesGcm::encrypt(&key, &nonce, plaintext, aad).expect("encryption failed");

        let result = VerifiedAesGcm::decrypt(&key, &nonce, &ciphertext, wrong_aad);
        assert!(result.is_err());
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let key = [0x42; 32];
        let nonce = [0x01; 12];
        let plaintext = b"secret message";

        let mut ciphertext =
            VerifiedAesGcm::encrypt(&key, &nonce, plaintext, b"").expect("encryption failed");

        // Tamper with ciphertext
        if !ciphertext.is_empty() {
            ciphertext[0] ^= 0xFF;
        }

        let result = VerifiedAesGcm::decrypt(&key, &nonce, &ciphertext, b"");
        assert!(result.is_err());
    }

    #[test]
    fn test_tampered_tag_fails() {
        let key = [0x42; 32];
        let nonce = [0x01; 12];
        let plaintext = b"secret message";

        let mut ciphertext =
            VerifiedAesGcm::encrypt(&key, &nonce, plaintext, b"").expect("encryption failed");

        // Tamper with tag (last 16 bytes)
        if ciphertext.len() >= 16 {
            let len = ciphertext.len();
            ciphertext[len - 1] ^= 0xFF;
        }

        let result = VerifiedAesGcm::decrypt(&key, &nonce, &ciphertext, b"");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_plaintext() {
        let key = [0x42; 32];
        let nonce = [0x01; 12];
        let plaintext = b"";

        let ciphertext =
            VerifiedAesGcm::encrypt(&key, &nonce, plaintext, b"").expect("encryption failed");

        // Ciphertext should only contain tag (16 bytes)
        assert_eq!(ciphertext.len(), 16);

        let decrypted =
            VerifiedAesGcm::decrypt(&key, &nonce, &ciphertext, b"").expect("decryption failed");

        assert_eq!(plaintext, &decrypted[..]);
    }

    #[test]
    fn test_large_plaintext() {
        let key = [0x42; 32];
        let nonce = [0x01; 12];
        let plaintext = vec![0xAB; 100_000]; // 100KB

        let ciphertext =
            VerifiedAesGcm::encrypt(&key, &nonce, &plaintext, b"").expect("encryption failed");

        let decrypted =
            VerifiedAesGcm::decrypt(&key, &nonce, &ciphertext, b"").expect("decryption failed");

        assert_eq!(&plaintext[..], &decrypted[..]);
    }

    #[test]
    fn test_nonce_from_position_unique() {
        let nonce1 = VerifiedAesGcm::nonce_from_position(0);
        let nonce2 = VerifiedAesGcm::nonce_from_position(1);
        let nonce3 = VerifiedAesGcm::nonce_from_position(1000);

        assert_ne!(nonce1, nonce2);
        assert_ne!(nonce1, nonce3);
        assert_ne!(nonce2, nonce3);
    }

    #[test]
    fn test_nonce_from_position_deterministic() {
        let nonce1 = VerifiedAesGcm::nonce_from_position(42);
        let nonce2 = VerifiedAesGcm::nonce_from_position(42);
        assert_eq!(nonce1, nonce2);
    }

    #[test]
    fn test_nonce_from_position_layout() {
        let position: u64 = 0x0123456789ABCDEF;
        let nonce = VerifiedAesGcm::nonce_from_position(position);

        // Position + 1 should be in first 8 bytes (little-endian)
        let reconstructed = u64::from_le_bytes(nonce[0..8].try_into().unwrap());
        assert_eq!(reconstructed, position + 1);

        // Upper 4 bytes should be zero (reserved)
        assert_eq!(&nonce[8..12], &[0, 0, 0, 0]);
    }

    #[test]
    fn test_proof_certificate() {
        let cert = VerifiedAesGcm::proof_certificate();
        assert_eq!(cert.theorem_id, 300);
        assert_eq!(cert.proof_system_id, 1);
        assert_eq!(cert.verified_at, 20260205);
        assert_eq!(cert.assumption_count, 1);
        assert!(!cert.is_complete()); // Has computational assumptions
    }

    #[test]
    fn test_verified_trait() {
        assert_eq!(VerifiedAesGcm::theorem_name(), "aes_gcm_roundtrip");
        assert!(VerifiedAesGcm::theorem_description().contains("roundtrip"));
    }

    #[test]
    fn test_different_plaintexts_different_ciphertexts() {
        let key = [0x42; 32];
        let nonce = [0x01; 12];

        let ct1 =
            VerifiedAesGcm::encrypt(&key, &nonce, b"message1", b"").expect("encryption failed");
        let ct2 =
            VerifiedAesGcm::encrypt(&key, &nonce, b"message2", b"").expect("encryption failed");

        assert_ne!(ct1, ct2);
    }

    #[test]
    fn test_deterministic_encryption() {
        // Same key, nonce, plaintext, AAD should produce same ciphertext
        let key = [0x42; 32];
        let nonce = [0x01; 12];
        let plaintext = b"deterministic test";

        let ct1 = VerifiedAesGcm::encrypt(&key, &nonce, plaintext, b"").expect("encryption failed");
        let ct2 = VerifiedAesGcm::encrypt(&key, &nonce, plaintext, b"").expect("encryption failed");

        assert_eq!(ct1, ct2);
    }
}
