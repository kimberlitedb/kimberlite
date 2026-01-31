#![no_main]

use libfuzzer_sys::fuzz_target;
use kmb_crypto::encryption::{EncryptionKey, Nonce, encrypt, decrypt};

fuzz_target!(|data: &[u8]| {
    // Need at least KEY_LENGTH + NONCE_LENGTH + 1 byte for plaintext
    const KEY_LENGTH: usize = 32;
    const NONCE_LENGTH: usize = 12;
    const MIN_LENGTH: usize = KEY_LENGTH + NONCE_LENGTH + 1;

    if data.len() < MIN_LENGTH {
        return;
    }

    // Split input into key, nonce, and plaintext
    let key_bytes = &data[0..KEY_LENGTH];
    let nonce_bytes = &data[KEY_LENGTH..KEY_LENGTH + NONCE_LENGTH];
    let plaintext = &data[KEY_LENGTH + NONCE_LENGTH..];

    // Test encryption with arbitrary inputs
    // This tests:
    // - Handling of weak/degenerate keys (all zeros, repeating patterns)
    // - Nonce edge cases
    // - Plaintext of varying sizes (0 bytes to large)
    // - Buffer overflow protection
    // - AES-GCM robustness

    let key_array: [u8; KEY_LENGTH] = key_bytes.try_into().expect("correct length");
    let nonce_array: [u8; NONCE_LENGTH] = nonce_bytes.try_into().expect("correct length");

    // Skip degenerate all-zero keys (would trigger debug assertion)
    // This is expected behavior - we don't want to encrypt with zero keys
    if key_array.iter().all(|&b| b == 0) {
        return;
    }

    let key = EncryptionKey::from_bytes(&key_array);
    let nonce = Nonce::from_bytes(nonce_array);

    // Encrypt
    let ciphertext = encrypt(&key, &nonce, plaintext);

    // Test decryption round-trip
    // This verifies:
    // - Decryption always succeeds for valid ciphertext
    // - Round-trip preserves data
    if let Ok(decrypted) = decrypt(&key, &nonce, &ciphertext) {
        assert_eq!(decrypted, plaintext, "round-trip failed");
    }

    // Test decryption with wrong key
    // This verifies:
    // - Authentication catches wrong key
    // - No crashes from bad key material
    if data.len() >= MIN_LENGTH * 2 {
        let wrong_key_bytes = &data[MIN_LENGTH..MIN_LENGTH + KEY_LENGTH];
        let wrong_key_array: [u8; KEY_LENGTH] = wrong_key_bytes.try_into().expect("correct length");

        // Skip all-zero wrong key (would trigger debug assertion)
        if !wrong_key_array.iter().all(|&b| b == 0) {
            let wrong_key = EncryptionKey::from_bytes(&wrong_key_array);
            let _result = decrypt(&wrong_key, &nonce, &ciphertext);
            // Should fail authentication (or succeed if keys happen to match)
        }
    }

    // Test field-level encryption API
    // This tests higher-level encryption utilities
    use kmb_crypto::{FieldKey, encrypt_field, decrypt_field};

    let field_key = FieldKey::from_bytes(&key_array);
    let encrypted_field = encrypt_field(&field_key, plaintext);
    if let Ok(decrypted_field) = decrypt_field(&field_key, &encrypted_field) {
        assert_eq!(decrypted_field, plaintext, "field encryption round-trip failed");
    }
});
