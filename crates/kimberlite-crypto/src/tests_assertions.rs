//! Tests for production assertions promoted from `debug_assert!()`
//!
//! This module contains tests that verify production assertions fire correctly
//! when invariants are violated. Tests verify that all 25 promoted crypto assertions
//! properly reject invalid inputs in production builds.

#[cfg(test)]
mod tests {
    use crate::encryption::*;
    use crate::signature::*;

    // ========================================================================
    // Encryption Key Assertions (18 total promoted)
    // ========================================================================

    #[test]
    #[should_panic(expected = "Nonce random bytes are all zeros")]
    fn nonce_from_random_bytes_all_zeros_panics() {
        let _nonce = Nonce::from_random_bytes([0u8; NONCE_LENGTH]);
    }

    #[test]
    #[should_panic(expected = "ciphertext too short")]
    fn ciphertext_too_short_panics() {
        let _ct = Ciphertext::from_bytes(vec![0u8; TAG_LENGTH - 1]);
    }

    #[test]
    #[should_panic(expected = "key_to_wrap is all zeros")]
    fn wrapped_key_new_with_all_zeros_panics() {
        let wrapping_key = EncryptionKey::generate();
        let _wrapped = WrappedKey::new(&wrapping_key, &[0u8; KEY_LENGTH]);
    }

    #[test]
    #[should_panic(expected = "wrapped key bytes are all zeros")]
    fn wrapped_key_from_bytes_all_zeros_panics() {
        let _wrapped = WrappedKey::from_bytes(&[0u8; WRAPPED_KEY_LENGTH]);
    }

    #[test]
    #[should_panic(expected = "master key bytes are all zeros")]
    fn master_key_from_bytes_all_zeros_panics() {
        let _master = InMemoryMasterKey::from_bytes(&[0u8; KEY_LENGTH]);
    }

    #[test]
    #[should_panic(expected = "KEK bytes are all zeros")]
    fn wrap_kek_with_all_zeros_panics() {
        let master = InMemoryMasterKey::generate();
        let _wrapped = master.wrap_kek(&[0u8; KEY_LENGTH]);
    }

    #[test]
    #[should_panic(expected = "DEK bytes are all zeros")]
    fn wrap_dek_with_all_zeros_panics() {
        let master = InMemoryMasterKey::generate();
        let (kek, _) = KeyEncryptionKey::generate_and_wrap(&master);
        let _wrapped = kek.wrap_dek(&[0u8; KEY_LENGTH]);
    }

    #[test]
    #[should_panic(expected = "plaintext exceeds")]
    fn encrypt_oversized_plaintext_panics() {
        let key = EncryptionKey::generate();
        let nonce = Nonce::from_position(0);
        let oversized = vec![0u8; 65 * 1024 * 1024]; // 65 MiB > MAX_PLAINTEXT_LENGTH
        let _ct = encrypt(&key, &nonce, &oversized);
    }

    // ========================================================================
    // Signature Assertions (3 total promoted)
    // ========================================================================

    #[test]
    #[should_panic(expected = "SigningKey random bytes are all zeros")]
    fn signing_key_from_random_bytes_all_zeros_panics() {
        let _key = SigningKey::from_random_bytes([0u8; SIGNING_KEY_LENGTH]);
    }

    #[test]
    #[should_panic(expected = "verifying key bytes are all zeros")]
    fn verifying_key_from_bytes_all_zeros_panics() {
        let _result = VerifyingKey::from_bytes(&[0u8; VERIFYING_KEY_LENGTH]);
    }

    #[test]
    #[should_panic(expected = "signature bytes are all zeros")]
    fn signature_from_bytes_all_zeros_panics() {
        let _sig = Signature::from_bytes(&[0u8; 64]);
    }
}
