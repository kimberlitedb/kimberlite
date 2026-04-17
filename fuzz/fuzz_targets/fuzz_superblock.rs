#![no_main]

// Fuzz target for the 4-copy superblock recovery voting logic.
//
// The superblock is the durability root of a VSR replica: at startup the
// replica reads all four 512-byte copies and selects the one with the
// highest valid sequence. An adversary with disk access (or a flaky
// device) can present any mix of valid-but-stale and invalid copies.
//
// Oracles:
//   1. `SuperblockData::from_bytes` on any 512-byte buffer never panics.
//   2. `Superblock::open` on any 2048-byte buffer never panics; it either
//      returns `Err` (no valid copy) or `Ok(sb)` with a specific selection.
//   3. On success, the selected copy has the globally highest sequence
//      among all valid copies — selection is a total max, not first-fit.
//   4. Round-trip: any `SuperblockData` that we serialize must decode back
//      to an identical value.

use std::io::Cursor;

use kimberlite_vsr::superblock::{
    SUPERBLOCK_COPIES, SUPERBLOCK_COPY_SIZE, SUPERBLOCK_TOTAL_SIZE, Superblock, SuperblockData,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // ── Section 1: per-copy parsing on arbitrary 512-byte slices ────────────
    if data.len() >= SUPERBLOCK_COPY_SIZE {
        let mut buf = [0u8; SUPERBLOCK_COPY_SIZE];
        buf.copy_from_slice(&data[..SUPERBLOCK_COPY_SIZE]);
        let decoded = SuperblockData::from_bytes(&buf);

        if let Some(sb) = decoded {
            // Round-trip the decoded value and re-decode. Must be identical.
            let reencoded = sb.to_bytes();
            let redecoded = SuperblockData::from_bytes(&reencoded)
                .expect("a freshly encoded superblock must decode");
            assert_eq!(
                sb, redecoded,
                "SuperblockData encode→decode is not a fixed point"
            );

            // And encoding is deterministic.
            assert_eq!(
                reencoded,
                redecoded.to_bytes(),
                "SuperblockData encoding must be deterministic"
            );
        }
    }

    // ── Section 2: 4-copy selection invariant ───────────────────────────────
    //
    // Build a 2048-byte region from fuzz input (pad with zero if short, truncate
    // if long), then run `Superblock::open` and assert that the selected copy
    // has the globally highest sequence among valid copies.
    let mut region = vec![0u8; SUPERBLOCK_TOTAL_SIZE];
    let copy_len = data.len().min(SUPERBLOCK_TOTAL_SIZE);
    region[..copy_len].copy_from_slice(&data[..copy_len]);

    // Compute the maximum sequence across all valid copies, independently of
    // the opener's logic.
    let mut max_valid_seq: Option<u64> = None;
    for slot in 0..SUPERBLOCK_COPIES {
        let start = slot * SUPERBLOCK_COPY_SIZE;
        let end = start + SUPERBLOCK_COPY_SIZE;
        let mut buf = [0u8; SUPERBLOCK_COPY_SIZE];
        buf.copy_from_slice(&region[start..end]);
        if let Some(sb) = SuperblockData::from_bytes(&buf) {
            max_valid_seq = Some(max_valid_seq.map_or(sb.sequence, |m| m.max(sb.sequence)));
        }
    }

    // The opener consumes the whole region and votes.
    let cursor = Cursor::new(region);
    match Superblock::open(cursor) {
        Ok(sb) => {
            let selected_seq = sb.data().sequence;
            let expected = max_valid_seq.expect(
                "Superblock::open must have seen at least one valid copy to return Ok",
            );
            assert_eq!(
                selected_seq, expected,
                "Superblock::open did not select the highest-sequence valid copy"
            );
        }
        Err(_) => {
            assert!(
                max_valid_seq.is_none(),
                "Superblock::open returned Err but at least one copy was valid"
            );
        }
    }
});
