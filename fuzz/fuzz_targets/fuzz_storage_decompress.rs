#![no_main]

// Fuzz target for compression codecs (LZ4, Zstd).
//
// Compressed blobs are read from disk and may also be replicated over the
// network via `StateTransferResponse.checkpoint_data`. Both paths are reached
// by attacker-controlled bytes, so decompression must be panic-free and
// bounded.
//
// Oracles:
//   1. Decompress never panics on arbitrary input.
//   2. If decompression succeeds, output length is bounded at 1 GiB
//      (`MAX_DECOMPRESSED_SIZE`).
//   3. Round-trip: compress(input) → decompress must return the original bytes.
//   4. LZ4 size-prefix bomb — the first 4 bytes of an LZ4 input are an
//      attacker-controlled u32 size. libFuzzer's default RSS limit (2 GiB)
//      turns any oversized allocation into an OOM crash; any such crash here
//      is a real bug worth filing.

use kmb_storage::codec::{Codec, Lz4Codec, ZstdCodec};
use libfuzzer_sys::fuzz_target;

const MAX_DECOMPRESSED_SIZE: usize = 1024 * 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    // ── Section 1: adversarial decompression ────────────────────────────────
    //
    // Feed arbitrary bytes directly to each codec's decompress path. No panic
    // is required, and any Ok output must respect the size guard.
    let lz4 = Lz4Codec;
    match lz4.decompress(data) {
        Ok(out) => {
            assert!(
                out.len() <= MAX_DECOMPRESSED_SIZE,
                "LZ4 decompress output {} bytes exceeds MAX_DECOMPRESSED_SIZE",
                out.len()
            );
        }
        Err(_) => {}
    }

    let zstd = ZstdCodec::default();
    match zstd.decompress(data) {
        Ok(out) => {
            assert!(
                out.len() <= MAX_DECOMPRESSED_SIZE,
                "Zstd decompress output {} bytes exceeds MAX_DECOMPRESSED_SIZE",
                out.len()
            );
        }
        Err(_) => {}
    }

    // ── Section 2: compress → decompress round-trip ─────────────────────────
    //
    // For any input that the codec accepts as plaintext, the codec must
    // round-trip it identically. We cap the input length to keep per-iteration
    // cost bounded.
    if data.len() > 64 * 1024 {
        return;
    }

    if let Ok(compressed) = lz4.compress(data) {
        let decompressed = lz4
            .decompress(&compressed)
            .expect("LZ4: a freshly compressed blob must decompress");
        assert_eq!(
            decompressed, data,
            "LZ4 round-trip must preserve input bytes"
        );
    }

    if let Ok(compressed) = zstd.compress(data) {
        let decompressed = zstd
            .decompress(&compressed)
            .expect("Zstd: a freshly compressed blob must decompress");
        assert_eq!(
            decompressed, data,
            "Zstd round-trip must preserve input bytes"
        );
    }
});
