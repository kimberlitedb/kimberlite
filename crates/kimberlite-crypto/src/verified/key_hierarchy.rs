//! Verified Key Hierarchy Implementation
//!
//! This module provides a 3-level key hierarchy (Master → KEK → DEK) with
//! embedded proof certificates from Coq formal verification. The implementation
//! wraps the encryption module with proofs of:
//! - Tenant isolation (different tenants → different keys)
//! - Key wrapping soundness (wrap/unwrap roundtrip)
//! - Forward secrecy (lower-level compromise doesn't reveal upper levels)
//! - Key derivation injectivity (unique keys)
//!
//! Proven properties are documented in `specs/coq/KeyHierarchy.v`

use super::aes_gcm::VerifiedAesGcm;
use super::proof_certificate::{ProofCertificate, Verified};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

// -----------------------------------------------------------------------------
// Proof Certificates (extracted from Coq)
// -----------------------------------------------------------------------------

/// Tenant isolation: different tenants have different KEKs
///
/// **Theorem:** `tenant_isolation` in `specs/coq/KeyHierarchy.v:100`
///
/// **Proven:** tenant1 ≠ tenant2 → derive_kek(master, tenant1) ≠ derive_kek(master, tenant2)
pub const TENANT_ISOLATION_CERT: ProofCertificate = ProofCertificate::new(
    500,      // theorem_id
    1,        // proof_system_id (Coq 8.18)
    20260205, // verified_at
    1,        // assumption_count (HKDF injectivity)
);

/// Key wrapping soundness: unwrap(wrap(dek)) = dek
///
/// **Theorem:** `key_wrapping_sound` in `specs/coq/KeyHierarchy.v:141`
///
/// **Proven:** Key wrapping and unwrapping preserve the original key
pub const KEY_WRAPPING_SOUNDNESS_CERT: ProofCertificate = ProofCertificate::new(
    501,      // theorem_id
    1,        // proof_system_id
    20260205, // verified_at
    1,        // assumption_count (AES-GCM roundtrip)
);

/// Forward secrecy: DEK compromise doesn't reveal KEK or Master
///
/// **Theorem:** `forward_secrecy` in `specs/coq/KeyHierarchy.v:197`
///
/// **Proven:** Lower-level key compromise doesn't reveal upper-level keys
pub const FORWARD_SECRECY_CERT: ProofCertificate = ProofCertificate::new(
    502,      // theorem_id
    1,        // proof_system_id
    20260205, // verified_at
    2,        // assumption_count (one-way functions)
);

/// Key derivation injectivity
///
/// **Theorem:** `key_derivation_injective` in `specs/coq/KeyHierarchy.v:244`
///
/// **Proven:** Different inputs produce different derived keys
pub const KEY_DERIVATION_INJECTIVE_CERT: ProofCertificate = ProofCertificate::new(
    503,      // theorem_id
    1,        // proof_system_id
    20260205, // verified_at
    2,        // assumption_count (HKDF injectivity, tenant/stream uniqueness)
);

// -----------------------------------------------------------------------------
// Key Hierarchy Types
// -----------------------------------------------------------------------------

/// Master key (top level) - 32 bytes
///
/// This is the root of the key hierarchy. Should be stored in HSM/KMS.
/// Key material is securely zeroed from memory when dropped.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct VerifiedMasterKey {
    key: [u8; 32],
}

/// Key Encryption Key (KEK) - derived per tenant
///
/// **Proven:** Different tenants have different KEKs (tenant isolation)
/// Key material is securely zeroed from memory when dropped.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct VerifiedKEK {
    key: [u8; 32],
}

/// Data Encryption Key (DEK) - derived per stream
///
/// **Proven:** Different streams have different DEKs
/// Key material is securely zeroed from memory when dropped.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct VerifiedDEK {
    key: [u8; 32],
}

// -----------------------------------------------------------------------------
// Master Key
// -----------------------------------------------------------------------------

impl VerifiedMasterKey {
    /// Generate a new master key from system randomness
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);

        // Assert key is not all zeros
        assert_ne!(key, [0u8; 32], "Master key is all zeros (degenerate)");

        Self { key }
    }

    /// Create master key from bytes
    ///
    /// # Safety
    /// Key material must be cryptographically random
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        assert_ne!(bytes, [0u8; 32], "Master key is all zeros");
        Self { key: bytes }
    }

    /// Get key bytes (sensitive operation)
    pub fn to_bytes(&self) -> [u8; 32] {
        self.key
    }

    /// Derive KEK for a tenant with isolation proof
    ///
    /// **Proven:** `tenant_isolation` - different tenants → different KEKs
    ///
    /// # Example
    /// ```
    /// use kimberlite_crypto::verified::VerifiedMasterKey;
    ///
    /// let master = VerifiedMasterKey::generate();
    /// let kek_tenant1 = master.derive_kek(1);
    /// let kek_tenant2 = master.derive_kek(2);
    /// // Proven: kek_tenant1 ≠ kek_tenant2
    /// ```
    pub fn derive_kek(&self, tenant_id: u64) -> VerifiedKEK {
        // HKDF-SHA256(master_key, salt="kek", info=tenant_id)
        let key = Self::hkdf_derive(&self.key, b"kek", &tenant_id.to_le_bytes());
        VerifiedKEK { key }
    }

    /// RFC 5869 HKDF Extract+Expand key derivation.
    fn hkdf_derive(ikm: &[u8; 32], salt: &[u8], info: &[u8]) -> [u8; 32] {
        let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
        let mut okm = [0u8; 32];
        hk.expand(info, &mut okm)
            .expect("32-byte output within HKDF maximum");
        okm
    }
}

// -----------------------------------------------------------------------------
// KEK (Key Encryption Key)
// -----------------------------------------------------------------------------

impl VerifiedKEK {
    /// Derive DEK for a stream with uniqueness proof
    ///
    /// **Proven:** `key_derivation_injective` - different streams → different DEKs
    ///
    /// # Example
    /// ```
    /// use kimberlite_crypto::verified::VerifiedMasterKey;
    ///
    /// let master = VerifiedMasterKey::generate();
    /// let kek = master.derive_kek(1);
    /// let dek_stream1 = kek.derive_dek(100);
    /// let dek_stream2 = kek.derive_dek(200);
    /// // Proven: dek_stream1 ≠ dek_stream2
    /// ```
    pub fn derive_dek(&self, stream_id: u64) -> VerifiedDEK {
        // HKDF-SHA256(kek, salt="dek", info=stream_id)
        let key = VerifiedMasterKey::hkdf_derive(&self.key, b"dek", &stream_id.to_le_bytes());
        VerifiedDEK { key }
    }

    /// Wrap (encrypt) a DEK for storage with soundness proof
    ///
    /// **Proven:** `key_wrapping_sound` - unwrap(wrap(dek)) = dek
    ///
    /// # Example
    /// ```
    /// use kimberlite_crypto::verified::{VerifiedMasterKey, VerifiedWrappedDEK};
    ///
    /// let master = VerifiedMasterKey::generate();
    /// let kek = master.derive_kek(1);
    /// let dek = kek.derive_dek(100);
    ///
    /// let wrapped = kek.wrap_dek(&dek).expect("wrap failed");
    /// let unwrapped = kek.unwrap_dek(&wrapped).expect("unwrap failed");
    /// // Proven: unwrapped = dek
    /// ```
    pub fn wrap_dek(&self, dek: &VerifiedDEK) -> Result<VerifiedWrappedDEK, String> {
        // Derive synthetic nonce from KEK and DEK: SHA-256(KEK || DEK)[0..12]
        let nonce = Self::derive_wrap_nonce(&self.key, &dek.key);

        let ciphertext = VerifiedAesGcm::encrypt(&self.key, &nonce, &dek.key, b"")?;

        // Prepend nonce to ciphertext so unwrap can extract it
        let mut output = Vec::with_capacity(12 + ciphertext.len());
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&ciphertext);

        Ok(VerifiedWrappedDEK {
            ciphertext: output,
        })
    }

    /// Unwrap (decrypt) a DEK from storage
    ///
    /// **Proven:** Returns original DEK if not tampered
    pub fn unwrap_dek(&self, wrapped: &VerifiedWrappedDEK) -> Result<VerifiedDEK, String> {
        if wrapped.ciphertext.len() < 12 {
            return Err("Wrapped DEK too short to contain nonce".to_string());
        }

        // Extract nonce (first 12 bytes) and ciphertext (remainder)
        let nonce: [u8; 12] = wrapped.ciphertext[..12]
            .try_into()
            .map_err(|_| "Failed to extract nonce from wrapped DEK")?;
        let ciphertext = &wrapped.ciphertext[12..];

        let plaintext = VerifiedAesGcm::decrypt(&self.key, &nonce, ciphertext, b"")?;

        if plaintext.len() != 32 {
            return Err("Unwrapped DEK has wrong length".to_string());
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&plaintext);

        Ok(VerifiedDEK { key })
    }

    /// Get key bytes (sensitive operation)
    pub fn to_bytes(&self) -> [u8; 32] {
        self.key
    }

    /// Derive a synthetic nonce for key wrapping from KEK and DEK material.
    ///
    /// Uses `SHA-256(KEK || DEK)[0..12]` to produce a unique, deterministic
    /// nonce per KEK-DEK pair, avoiding fixed-nonce reuse.
    fn derive_wrap_nonce(kek: &[u8; 32], dek: &[u8; 32]) -> [u8; 12] {
        let mut hasher = Sha256::new();
        hasher.update(kek);
        hasher.update(dek);
        let hash = hasher.finalize();
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&hash[..12]);
        nonce
    }
}

// -----------------------------------------------------------------------------
// DEK (Data Encryption Key)
// -----------------------------------------------------------------------------

impl VerifiedDEK {
    /// Encrypt data with this DEK
    pub fn encrypt(&self, position: u64, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let nonce = VerifiedAesGcm::nonce_from_position(position);
        VerifiedAesGcm::encrypt(&self.key, &nonce, plaintext, b"")
    }

    /// Decrypt data with this DEK
    pub fn decrypt(&self, position: u64, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        let nonce = VerifiedAesGcm::nonce_from_position(position);
        VerifiedAesGcm::decrypt(&self.key, &nonce, ciphertext, b"")
    }

    /// Get key bytes (sensitive operation)
    pub fn to_bytes(&self) -> [u8; 32] {
        self.key
    }
}

// -----------------------------------------------------------------------------
// Wrapped DEK
// -----------------------------------------------------------------------------

/// Wrapped (encrypted) DEK for storage
#[derive(Clone)]
pub struct VerifiedWrappedDEK {
    ciphertext: Vec<u8>,
}

impl VerifiedWrappedDEK {
    /// Get wrapped bytes for serialization
    pub fn to_bytes(&self) -> Vec<u8> {
        self.ciphertext.clone()
    }

    /// Create from wrapped bytes
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { ciphertext: bytes }
    }
}

// Verified trait implementation
impl Verified for VerifiedMasterKey {
    fn proof_certificate() -> ProofCertificate {
        TENANT_ISOLATION_CERT
    }

    fn theorem_name() -> &'static str {
        "tenant_isolation"
    }

    fn theorem_description() -> &'static str {
        "Tenant isolation: different tenants have cryptographically different KEKs"
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_master_key_generation() {
        let master1 = VerifiedMasterKey::generate();
        let master2 = VerifiedMasterKey::generate();

        // Different master keys
        assert_ne!(master1.to_bytes(), master2.to_bytes());
    }

    #[test]
    fn test_kek_derivation_deterministic() {
        let master = VerifiedMasterKey::generate();

        let kek1 = master.derive_kek(42);
        let kek2 = master.derive_kek(42);

        assert_eq!(kek1.to_bytes(), kek2.to_bytes());
    }

    #[test]
    fn test_tenant_isolation() {
        let master = VerifiedMasterKey::generate();

        let kek1 = master.derive_kek(1);
        let kek2 = master.derive_kek(2);

        assert_ne!(kek1.to_bytes(), kek2.to_bytes());
    }

    #[test]
    fn test_dek_derivation_deterministic() {
        let master = VerifiedMasterKey::generate();
        let kek = master.derive_kek(1);

        let dek1 = kek.derive_dek(100);
        let dek2 = kek.derive_dek(100);

        assert_eq!(dek1.to_bytes(), dek2.to_bytes());
    }

    #[test]
    fn test_dek_derivation_unique() {
        let master = VerifiedMasterKey::generate();
        let kek = master.derive_kek(1);

        let dek1 = kek.derive_dek(100);
        let dek2 = kek.derive_dek(200);

        assert_ne!(dek1.to_bytes(), dek2.to_bytes());
    }

    #[test]
    fn test_key_wrapping_roundtrip() {
        let master = VerifiedMasterKey::generate();
        let kek = master.derive_kek(1);
        let dek = kek.derive_dek(100);

        let wrapped = kek.wrap_dek(&dek).expect("wrap failed");
        let unwrapped = kek.unwrap_dek(&wrapped).expect("unwrap failed");

        assert_eq!(dek.to_bytes(), unwrapped.to_bytes());
    }

    #[test]
    fn test_wrong_kek_unwrap_fails() {
        let master = VerifiedMasterKey::generate();
        let kek1 = master.derive_kek(1);
        let kek2 = master.derive_kek(2);
        let dek = kek1.derive_dek(100);

        let wrapped = kek1.wrap_dek(&dek).expect("wrap failed");
        let result = kek2.unwrap_dek(&wrapped);

        assert!(result.is_err());
    }

    #[test]
    fn test_tampered_wrapped_dek_fails() {
        let master = VerifiedMasterKey::generate();
        let kek = master.derive_kek(1);
        let dek = kek.derive_dek(100);

        let wrapped = kek.wrap_dek(&dek).expect("wrap failed");

        // Tamper with wrapped bytes
        let mut tampered_bytes = wrapped.to_bytes();
        if !tampered_bytes.is_empty() {
            tampered_bytes[0] ^= 0xFF;
        }
        let tampered = VerifiedWrappedDEK::from_bytes(tampered_bytes);

        let result = kek.unwrap_dek(&tampered);
        assert!(result.is_err());
    }

    #[test]
    fn test_dek_encrypt_decrypt_roundtrip() {
        let master = VerifiedMasterKey::generate();
        let kek = master.derive_kek(1);
        let dek = kek.derive_dek(100);

        let plaintext = b"sensitive data";
        let position = 0;

        let ciphertext = dek.encrypt(position, plaintext).expect("encrypt failed");
        let decrypted = dek.decrypt(position, &ciphertext).expect("decrypt failed");

        assert_eq!(plaintext, &decrypted[..]);
    }

    #[test]
    fn test_different_positions_different_ciphertexts() {
        let master = VerifiedMasterKey::generate();
        let kek = master.derive_kek(1);
        let dek = kek.derive_dek(100);

        let plaintext = b"data";

        let ct1 = dek.encrypt(0, plaintext).expect("encrypt failed");
        let ct2 = dek.encrypt(1, plaintext).expect("encrypt failed");

        assert_ne!(ct1, ct2);
    }

    #[test]
    fn test_full_hierarchy() {
        // Master → KEK (tenant 1) → DEK (stream 100) → encrypt data
        let master = VerifiedMasterKey::generate();
        let kek = master.derive_kek(1);
        let dek = kek.derive_dek(100);

        // Wrap DEK for storage
        let wrapped_dek = kek.wrap_dek(&dek).expect("wrap failed");

        // Later: unwrap DEK and use it
        let restored_dek = kek.unwrap_dek(&wrapped_dek).expect("unwrap failed");

        // Encrypt with restored DEK
        let plaintext = b"test data";
        let ciphertext = restored_dek.encrypt(0, plaintext).expect("encrypt failed");
        let decrypted = restored_dek
            .decrypt(0, &ciphertext)
            .expect("decrypt failed");

        assert_eq!(plaintext, &decrypted[..]);
    }

    #[test]
    fn test_tenant_dek_isolation() {
        let master = VerifiedMasterKey::generate();

        // Two different tenants
        let kek1 = master.derive_kek(1);
        let kek2 = master.derive_kek(2);

        // Same stream ID, different tenants
        let dek1 = kek1.derive_dek(100);
        let dek2 = kek2.derive_dek(100);

        // DEKs should be different (tenant isolation)
        assert_ne!(dek1.to_bytes(), dek2.to_bytes());
    }

    #[test]
    fn test_master_key_serialization() {
        let master = VerifiedMasterKey::generate();
        let bytes = master.to_bytes();
        let restored = VerifiedMasterKey::from_bytes(bytes);

        // Should derive same KEKs
        let kek1 = master.derive_kek(42);
        let kek2 = restored.derive_kek(42);

        assert_eq!(kek1.to_bytes(), kek2.to_bytes());
    }

    #[test]
    fn test_wrapped_dek_serialization() {
        let master = VerifiedMasterKey::generate();
        let kek = master.derive_kek(1);
        let dek = kek.derive_dek(100);

        let wrapped = kek.wrap_dek(&dek).expect("wrap failed");
        let bytes = wrapped.to_bytes();
        let restored = VerifiedWrappedDEK::from_bytes(bytes);

        // Should unwrap to same DEK
        let unwrapped = kek.unwrap_dek(&restored).expect("unwrap failed");
        assert_eq!(dek.to_bytes(), unwrapped.to_bytes());
    }

    #[test]
    fn test_proof_certificate() {
        let cert = VerifiedMasterKey::proof_certificate();
        assert_eq!(cert.theorem_id, 500);
        assert_eq!(cert.proof_system_id, 1);
        assert_eq!(cert.verified_at, 20260205);
        assert_eq!(cert.assumption_count, 1);
    }

    #[test]
    fn test_verified_trait() {
        assert_eq!(VerifiedMasterKey::theorem_name(), "tenant_isolation");
        assert!(VerifiedMasterKey::theorem_description().contains("isolation"));
    }

    #[test]
    #[should_panic(expected = "Master key is all zeros")]
    fn test_master_key_from_bytes_rejects_zero() {
        VerifiedMasterKey::from_bytes([0u8; 32]);
    }

    #[test]
    fn test_forward_secrecy_simulation() {
        // Simulate: If DEK is compromised, KEK and Master should remain secure
        let master = VerifiedMasterKey::generate();
        let kek = master.derive_kek(1);
        let dek = kek.derive_dek(100);

        // Attacker gets DEK bytes
        let _compromised_dek = dek.to_bytes();

        // Attacker cannot derive KEK or Master from DEK
        // (This is a cryptographic assumption, not directly testable)
        // But we can verify that KEK and Master are distinct

        assert_ne!(kek.to_bytes(), dek.to_bytes());
        assert_ne!(master.to_bytes(), dek.to_bytes());
        assert_ne!(master.to_bytes(), kek.to_bytes());
    }
}
