#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Test 1: Record::from_bytes with arbitrary data must never panic.
    //
    // Covers truncated headers, invalid RecordKind/CompressionKind bytes,
    // CRC32 checksum mismatches, payload length overflow, and hash chain
    // field parsing (all-zero vs non-zero prev_hash).
    let bytes = Bytes::copy_from_slice(data);
    let _ = kmb_storage::Record::from_bytes(&bytes);

    // Test 2: Round-trip serialization.
    //
    // Build a valid record from fuzz input and verify serialize → deserialize
    // preserves every field. Requires at least enough bytes for the header
    // fields we read (offset + prev_hash + kind = 41 bytes).
    if data.len() >= 41 {
        use kimberlite_types::{CompressionKind, Offset, RecordKind};
        use kmb_crypto::ChainHash;

        let offset_val = u64::from_le_bytes(data[0..8].try_into().expect("8 bytes"));
        let offset = Offset::new(offset_val);

        let prev_hash_bytes: [u8; 32] = data[8..40].try_into().expect("32 bytes");
        let prev_hash = if prev_hash_bytes == [0u8; 32] {
            None
        } else {
            Some(ChainHash::from_bytes(&prev_hash_bytes))
        };

        let kind = match data[40] % 3 {
            0 => RecordKind::Data,
            1 => RecordKind::Checkpoint,
            _ => RecordKind::Tombstone,
        };

        let payload = Bytes::copy_from_slice(&data[41..]);

        let record = kmb_storage::Record::with_kind(offset, prev_hash, kind, payload.clone());

        let serialized = Bytes::from(record.to_bytes());

        let (decoded, consumed) = kmb_storage::Record::from_bytes(&serialized)
            .expect("a record we just serialized must decode");
        assert_eq!(decoded.offset(), offset, "offset round-trip");
        assert_eq!(decoded.prev_hash(), prev_hash, "prev_hash round-trip");
        assert_eq!(decoded.kind(), kind, "kind round-trip");
        assert_eq!(
            decoded.compression(),
            CompressionKind::None,
            "compression round-trip"
        );
        assert_eq!(decoded.payload(), &payload, "payload round-trip");
        assert_eq!(consumed, serialized.len(), "all bytes consumed");

        // Hash computation is deterministic.
        assert_eq!(record.compute_hash(), record.compute_hash());

        // Test 3: Corruption-detection oracle.
        //
        // Flip one byte of the serialized record and require that decoding
        // fails. The record format is:
        //   [RECORD_START:4][offset:8][prev_hash:32][kind:1][compression:1]
        //   [length:4][payload:var][crc32:4][RECORD_END:4]
        //
        // Every field participates in either:
        //   - the RECORD_START / RECORD_END sentinels → TornWrite
        //   - the kind / compression enum bytes → Invalid{Record,Compression}Kind
        //   - the CRC-covered region (offset .. payload) → CorruptedRecord
        //   - the length or CRC bytes themselves → CorruptedRecord or UnexpectedEof
        //
        // A single-bit flip yields a CRC32 collision with probability 2^-32 per
        // flipped byte — effectively zero at libFuzzer iteration counts, so we
        // assert strict rejection.
        let mut corrupted = serialized.to_vec();
        if !corrupted.is_empty() {
            // Pick a deterministic but fuzz-driven position and delta.
            let corrupt_pos = (data[0] as usize) % corrupted.len();
            // XOR with a non-zero byte so the flip is guaranteed to change bytes.
            let corrupt_xor = data[1 % data.len()] | 0x01;
            corrupted[corrupt_pos] ^= corrupt_xor;

            let corrupted_bytes = Bytes::from(corrupted);
            let decoded = kmb_storage::Record::from_bytes(&corrupted_bytes);
            assert!(
                decoded.is_err(),
                "corruption at byte {corrupt_pos} (xor {corrupt_xor:#04x}) was not detected"
            );
        }
    }
});
