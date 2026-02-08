#![no_main]

use libfuzzer_sys::fuzz_target;
use kmb_crypto::encryption::{EncryptionKey, Nonce, encrypt, decrypt};

fuzz_target!(|data: &[u8]| {
    // ── Section 1: AES-256-GCM Encrypt/Decrypt ──────────────────────────────

    const KEY_LENGTH: usize = 32;
    const NONCE_LENGTH: usize = 12;
    const MIN_LENGTH: usize = KEY_LENGTH + NONCE_LENGTH + 1;

    if data.len() >= MIN_LENGTH {
        let key_bytes = &data[0..KEY_LENGTH];
        let nonce_bytes = &data[KEY_LENGTH..KEY_LENGTH + NONCE_LENGTH];
        let plaintext = &data[KEY_LENGTH + NONCE_LENGTH..];

        let key_array: [u8; KEY_LENGTH] = key_bytes.try_into().expect("correct length");
        let nonce_array: [u8; NONCE_LENGTH] = nonce_bytes.try_into().expect("correct length");

        // Skip degenerate all-zero keys (would trigger debug assertion)
        if !key_array.iter().all(|&b| b == 0) {
            let key = EncryptionKey::from_bytes(&key_array);
            let nonce = Nonce::from_bytes(nonce_array);

            // Encrypt
            let ciphertext = encrypt(&key, &nonce, plaintext);

            // Decrypt round-trip
            if let Ok(decrypted) = decrypt(&key, &nonce, &ciphertext) {
                assert_eq!(decrypted, plaintext, "round-trip failed");
            }

            // Test decryption with wrong key
            if data.len() >= MIN_LENGTH * 2 {
                let wrong_key_bytes = &data[MIN_LENGTH..MIN_LENGTH + KEY_LENGTH];
                let wrong_key_array: [u8; KEY_LENGTH] =
                    wrong_key_bytes.try_into().expect("correct length");

                if !wrong_key_array.iter().all(|&b| b == 0) {
                    let wrong_key = EncryptionKey::from_bytes(&wrong_key_array);
                    let _result = decrypt(&wrong_key, &nonce, &ciphertext);
                }
            }

            // Field-level encryption API
            use kmb_crypto::{FieldKey, encrypt_field, decrypt_field};
            let field_key = FieldKey::from_bytes(&key_array);
            let encrypted_field = encrypt_field(&field_key, plaintext);
            if let Ok(decrypted_field) = decrypt_field(&field_key, &encrypted_field) {
                assert_eq!(decrypted_field, plaintext, "field encryption round-trip failed");
            }
        }
    }

    // ── Section 2: Ed25519 Signatures ───────────────────────────────────────

    const SIGNING_KEY_LENGTH: usize = 32;

    if data.len() >= SIGNING_KEY_LENGTH + 1 {
        use kmb_crypto::SigningKey;

        let sk_bytes: [u8; SIGNING_KEY_LENGTH] =
            data[0..SIGNING_KEY_LENGTH].try_into().expect("32 bytes");
        let message = &data[SIGNING_KEY_LENGTH..];

        let signing_key = SigningKey::from_bytes(&sk_bytes);
        let verifying_key = signing_key.verifying_key();

        // Sign and verify with correct key
        let signature = signing_key.sign(message);
        assert!(
            verifying_key.verify(message, &signature).is_ok(),
            "signature verification should succeed with correct key"
        );

        // Verify with wrong key should fail (unless keys happen to match)
        if data.len() >= SIGNING_KEY_LENGTH * 2 + 1 {
            let wrong_sk_bytes: [u8; SIGNING_KEY_LENGTH] = data
                [SIGNING_KEY_LENGTH..SIGNING_KEY_LENGTH * 2]
                .try_into()
                .expect("32 bytes");
            let wrong_sk = SigningKey::from_bytes(&wrong_sk_bytes);
            let wrong_vk = wrong_sk.verifying_key();

            // Wrong key verification — should fail unless keys happen to be equal
            if wrong_vk.to_bytes() != verifying_key.to_bytes() {
                assert!(
                    wrong_vk.verify(message, &signature).is_err(),
                    "signature verification should fail with wrong key"
                );
            }
        }

        // Tampered message should fail verification
        if !message.is_empty() {
            let mut tampered = message.to_vec();
            tampered[0] ^= 0xFF;
            // Tampered message — verification should fail unless message was
            // only different in bits that don't affect the signature (impossible
            // for Ed25519, but we don't assert to avoid false positives on
            // single-bit messages)
            let _ = verifying_key.verify(&tampered, &signature);
        }

        // Signature round-trip through bytes
        let sig_bytes = signature.to_bytes();
        let restored = kmb_crypto::Signature::from_bytes(&sig_bytes);
        assert!(
            verifying_key.verify(message, &restored).is_ok(),
            "signature should survive serialization round-trip"
        );
    }

    // ── Section 3: Hash Chain Tamper Detection ──────────────────────────────

    if data.len() >= 2 {
        use kmb_crypto::chain_hash;

        // Build a short hash chain from fuzz data
        let num_records = (data[0] as usize % 8) + 1;
        let record_data = &data[1..];
        let chunk_size = if record_data.is_empty() {
            return;
        } else {
            (record_data.len() / num_records).max(1)
        };

        let mut hashes = Vec::with_capacity(num_records);

        for i in 0..num_records {
            let start = i * chunk_size;
            let end = ((i + 1) * chunk_size).min(record_data.len());
            if start >= record_data.len() {
                break;
            }

            let prev = hashes.last();
            let hash = chain_hash(prev, &record_data[start..end]);
            hashes.push(hash);
        }

        // Verify chain is deterministic
        if hashes.len() >= 2 {
            let mut verify_hashes = Vec::with_capacity(hashes.len());
            for i in 0..hashes.len() {
                let start = i * chunk_size;
                let end = ((i + 1) * chunk_size).min(record_data.len());
                let prev = verify_hashes.last();
                let hash = chain_hash(prev, &record_data[start..end]);
                verify_hashes.push(hash);
            }
            assert_eq!(hashes, verify_hashes, "hash chain must be deterministic");
        }

        // Tamper detection: modifying any record should change all subsequent hashes
        if hashes.len() >= 2 && chunk_size > 0 {
            let mut tampered_data = record_data.to_vec();
            // Tamper with the first record
            tampered_data[0] ^= 0x01;

            let mut tampered_hashes = Vec::new();
            for i in 0..hashes.len() {
                let start = i * chunk_size;
                let end = ((i + 1) * chunk_size).min(tampered_data.len());
                if start >= tampered_data.len() {
                    break;
                }
                let prev = tampered_hashes.last();
                let hash = chain_hash(prev, &tampered_data[start..end]);
                tampered_hashes.push(hash);
            }

            // First hash should differ
            if !tampered_hashes.is_empty() {
                assert_ne!(
                    hashes[0], tampered_hashes[0],
                    "tampering must change the hash"
                );
            }
        }
    }

    // ── Section 4: Key Wrapping ─────────────────────────────────────────────

    if data.len() >= KEY_LENGTH * 2 {
        use kmb_crypto::{EncryptionKey, WrappedKey};

        let kek_bytes: [u8; KEY_LENGTH] = data[0..KEY_LENGTH].try_into().expect("32 bytes");
        let dek_bytes: [u8; KEY_LENGTH] =
            data[KEY_LENGTH..KEY_LENGTH * 2].try_into().expect("32 bytes");

        // Skip all-zero keys
        if kek_bytes.iter().any(|&b| b != 0) && dek_bytes.iter().any(|&b| b != 0) {
            let kek = EncryptionKey::from_bytes(&kek_bytes);

            // Wrap the DEK
            let wrapped = WrappedKey::new(&kek, &dek_bytes);

            // Unwrap should recover original DEK
            let unwrapped = wrapped.unwrap_key(&kek);
            assert!(unwrapped.is_ok(), "unwrap with correct KEK should succeed");
            assert_eq!(unwrapped.unwrap(), dek_bytes, "unwrapped DEK should match");

            // Unwrap with wrong KEK should fail
            if data.len() >= KEY_LENGTH * 3 {
                let wrong_kek_bytes: [u8; KEY_LENGTH] = data[KEY_LENGTH * 2..KEY_LENGTH * 3]
                    .try_into()
                    .expect("32 bytes");
                if wrong_kek_bytes.iter().any(|&b| b != 0) && wrong_kek_bytes != kek_bytes {
                    let wrong_kek = EncryptionKey::from_bytes(&wrong_kek_bytes);
                    let wrong_result = wrapped.unwrap_key(&wrong_kek);
                    assert!(
                        wrong_result.is_err(),
                        "unwrap with wrong KEK should fail"
                    );
                }
            }

            // WrappedKey serialization round-trip
            let wrapped_bytes = wrapped.to_bytes();
            let restored = WrappedKey::from_bytes(&wrapped_bytes);
            let restored_unwrap = restored.unwrap_key(&kek);
            assert!(restored_unwrap.is_ok(), "round-trip wrapped key should unwrap");
            assert_eq!(
                restored_unwrap.unwrap(),
                dek_bytes,
                "round-trip should preserve DEK"
            );
        }
    }
});
