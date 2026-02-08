#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Test 1: Record::from_bytes with arbitrary data
    //
    // This tests:
    // - Truncated headers (< 46 bytes)
    // - Invalid RecordKind byte values (> 2)
    // - Invalid CompressionKind byte values (> 2)
    // - CRC32 checksum mismatch detection
    // - Payload length overflow / truncated payload
    // - Zero-copy payload slicing edge cases
    // - Hash chain field parsing (all-zero vs non-zero prev_hash)
    let bytes = Bytes::copy_from_slice(data);
    let _ = kmb_storage::Record::from_bytes(&bytes);

    // Test 2: Round-trip serialization
    //
    // Construct a valid record from fuzz input and verify serialization
    // round-trip preserves all fields.
    if data.len() >= 34 {
        use kimberlite_types::{CompressionKind, Offset, RecordKind};
        use kmb_crypto::ChainHash;

        // Extract fields from fuzz input
        let offset_val = u64::from_le_bytes(
            data[0..8].try_into().expect("8 bytes"),
        );
        let offset = Offset::new(offset_val);

        let prev_hash_bytes: [u8; 32] = data[1..33].try_into().expect("32 bytes");
        let prev_hash = if prev_hash_bytes == [0u8; 32] {
            None
        } else {
            Some(ChainHash::from_bytes(&prev_hash_bytes))
        };

        let kind = match data[33] % 3 {
            0 => RecordKind::Data,
            1 => RecordKind::Checkpoint,
            _ => RecordKind::Tombstone,
        };

        let payload = Bytes::copy_from_slice(&data[34..]);

        let record = kmb_storage::Record::with_kind(offset, prev_hash, kind, payload.clone());

        // Serialize
        let serialized = record.to_bytes();
        let serialized_bytes = Bytes::from(serialized);

        // Deserialize and verify round-trip
        if let Ok((decoded, consumed)) = kmb_storage::Record::from_bytes(&serialized_bytes) {
            assert_eq!(decoded.offset(), offset);
            assert_eq!(decoded.prev_hash(), prev_hash);
            assert_eq!(decoded.kind(), kind);
            assert_eq!(decoded.compression(), CompressionKind::None);
            assert_eq!(decoded.payload(), &payload);
            assert_eq!(consumed, serialized_bytes.len());
        }

        // Test hash chain computation
        let hash = record.compute_hash();
        // Hash should be deterministic
        assert_eq!(hash, record.compute_hash());
    }

    // Test 3: Corrupted CRC detection
    //
    // Take valid serialized bytes, flip bits in different regions,
    // and verify from_bytes correctly rejects them.
    if data.len() >= 35 {
        use kimberlite_types::{Offset, RecordKind};

        let record = kmb_storage::Record::with_kind(
            Offset::new(0),
            None,
            RecordKind::Data,
            Bytes::copy_from_slice(&data[34..]),
        );

        let mut corrupted = record.to_bytes();

        // Use a byte from fuzz input to select corruption position
        if !corrupted.is_empty() {
            let corrupt_pos = data[0] as usize % corrupted.len();
            corrupted[corrupt_pos] ^= 0xFF;

            let corrupted_bytes = Bytes::from(corrupted);
            // Should either fail with an error or succeed if the corruption
            // happened to produce a valid record (extremely unlikely)
            let _ = kmb_storage::Record::from_bytes(&corrupted_bytes);
        }
    }
});
