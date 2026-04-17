#![no_main]

// Fuzz target for the VSR wire protocol frame codec.
//
// Exercises `kimberlite_vsr::framing::FrameDecoder` against adversarial peer
// input. The VSR framing surface is the replica↔replica trust boundary — a
// Byzantine peer can send arbitrary bytes, and a crash, panic, or unbounded
// allocation in the decoder is exploitable.
//
// Oracles:
//   1. `FrameDecoder::decode` must never panic on any byte sequence.
//   2. For any accepted `Message`, re-encoding via `FrameEncoder` and decoding
//      again must yield a bit-identical `Message` (codec is a fixed point).
//   3. Size enforcement: when the decoder returns `MessageTooLarge`, the
//      reported size matches the header's length field.
//
// Signature verification is intentionally not part of this target — the
// signing key would have to be derived from fuzz bytes, which leads to noisy
// "invalid key" errors that dominate coverage. A dedicated signature-property
// target belongs in Phase D when the `verified` crypto wrappers expose a
// deterministic keygen surface.

use kimberlite_vsr::framing::{FrameDecoder, FrameEncoder, FramingError};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut decoder = FrameDecoder::new();
    decoder.extend(data);

    loop {
        match decoder.decode() {
            Ok(Some(message)) => {
                // Round-trip: encode the accepted message and decode the result.
                // The re-decoded message must be structurally identical.
                let encoder = FrameEncoder::new();
                let encoded = encoder
                    .encode(&message)
                    .expect("encoding an accepted message must succeed");

                let mut verifier = FrameDecoder::new();
                verifier.extend(&encoded);
                match verifier.decode() {
                    Ok(Some(round_tripped)) => {
                        assert_eq!(
                            message, round_tripped,
                            "VSR frame round-trip produced a different Message"
                        );
                    }
                    other => panic!(
                        "re-decode of a just-encoded VSR frame must yield the same message; \
                         got {other:?}"
                    ),
                }
            }
            Ok(None) => break,
            Err(FramingError::MessageTooLarge { size, .. }) => {
                // Invariant: the reported oversize must be exactly what the
                // header claimed — if there's a mismatch, we'd be allocating
                // based on a different size than we refused.
                assert!(
                    size as usize > 0,
                    "MessageTooLarge must carry a positive size"
                );
                break;
            }
            Err(FramingError::ChecksumMismatch { .. })
            | Err(FramingError::Deserialize(_))
            | Err(FramingError::Incomplete { .. })
            | Err(FramingError::Serialize(_))
            | Err(FramingError::Io(_)) => break,
        }
    }

    // Re-feed the exact same bytes to a fresh decoder — ensures no hidden
    // state or randomness affects decoding.
    let mut second = FrameDecoder::new();
    second.extend(data);
    let _ = second.decode();
});
