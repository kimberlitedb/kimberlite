//! CRC32 (IEEE 802.3) checksum implementation.
//!
//! Provides fast, table-driven CRC32 calculation using the IEEE 802.3 polynomial
//! (0xEDB88320). Used for integrity checking in storage, VSR, and wire protocol.
//!
//! ## Rationale
//!
//! This replaces the external `crc32fast` crate following PRESSURECRAFT §636:
//! - Simple enough to implement ourselves (~70 lines)
//! - Higher auditability for compliance-focused architecture
//! - Eliminates external dependency with zero maintenance burden
//! - CRC32 spec unchanged since 1975
//!
//! ## Usage
//!
//! ```
//! use kimberlite_crypto::crc32;
//!
//! // One-shot calculation
//! let checksum = crc32(b"hello world");
//!
//! // Incremental calculation for streaming data
//! let mut hasher = crc32::Crc32::new();
//! hasher.update(b"hello ");
//! hasher.update(b"world");
//! let checksum = hasher.finalize();
//! ```

/// IEEE 802.3 CRC32 polynomial (reversed): 0xEDB88320
const POLYNOMIAL: u32 = 0xEDB88320;

/// Precomputed CRC32 lookup table (256 entries).
/// Generated at compile time using const evaluation.
const CRC32_TABLE: [u32; 256] = generate_table();

/// Generates the CRC32 lookup table at compile time.
const fn generate_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 == 1 {
                crc = (crc >> 1) ^ POLYNOMIAL;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// Computes the CRC32 checksum of the given data in one shot.
///
/// Uses the IEEE 802.3 polynomial (0xEDB88320).
///
/// # Examples
///
/// ```
/// use kimberlite_crypto::crc32;
/// let checksum = crc32(b"hello world");
/// ```
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF; // Initial value
    for &byte in data {
        let index = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    crc ^ 0xFFFF_FFFF // Final XOR
}

/// Incremental CRC32 hasher for streaming or chunked data.
///
/// Allows computing CRC32 over multiple calls to `update()`.
///
/// # Examples
///
/// ```
/// use kimberlite_crypto::crc32::Crc32;
///
/// let mut hasher = Crc32::new();
/// hasher.update(b"hello ");
/// hasher.update(b"world");
/// let checksum = hasher.finalize();
/// ```
#[derive(Debug, Clone)]
pub struct Crc32 {
    state: u32,
}

impl Crc32 {
    /// Creates a new CRC32 hasher.
    #[must_use]
    pub fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    /// Updates the CRC32 state with the given data.
    pub fn update(&mut self, data: &[u8]) {
        for &byte in data {
            let index = ((self.state ^ u32::from(byte)) & 0xFF) as usize;
            self.state = (self.state >> 8) ^ CRC32_TABLE[index];
        }
    }

    /// Finalizes the CRC32 computation and returns the checksum.
    ///
    /// Consumes the hasher to prevent reuse after finalization.
    #[must_use]
    pub fn finalize(self) -> u32 {
        self.state ^ 0xFFFF_FFFF
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Known test vectors from various sources (zlib, PNG spec, etc.)
    #[test]
    fn test_crc32_empty() {
        assert_eq!(crc32(b""), 0x0000_0000);
    }

    #[test]
    fn test_crc32_known_vectors() {
        // "123456789" - standard CRC32 test vector
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);

        // "The quick brown fox jumps over the lazy dog"
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);

        // Single character
        assert_eq!(crc32(b"a"), 0xE8B7_BE43);
    }

    #[test]
    fn test_incremental_matches_oneshot() {
        let data = b"hello world this is a test";

        let mut hasher = Crc32::new();
        hasher.update(data);
        let incremental = hasher.finalize();

        let oneshot = crc32(data);

        assert_eq!(incremental, oneshot);
    }

    #[test]
    fn test_chunking_invariant() {
        let data = b"The quick brown fox jumps over the lazy dog";

        // Split at various points
        for split in 0..data.len() {
            let mut hasher = Crc32::new();
            hasher.update(&data[..split]);
            hasher.update(&data[split..]);
            assert_eq!(hasher.finalize(), crc32(data));
        }
    }

    #[test]
    fn test_multiple_chunks() {
        let mut hasher = Crc32::new();
        hasher.update(b"hello ");
        hasher.update(b"world ");
        hasher.update(b"from ");
        hasher.update(b"rust");

        assert_eq!(hasher.finalize(), crc32(b"hello world from rust"));
    }

    #[cfg(feature = "proptest")]
    #[test]
    fn proptest_incremental_matches_oneshot() {
        use proptest::prelude::*;

        proptest!(|(data: Vec<u8>)| {
            let mut hasher = Crc32::new();
            hasher.update(&data);
            prop_assert_eq!(hasher.finalize(), crc32(&data));
        });
    }

    #[cfg(feature = "proptest")]
    #[test]
    fn proptest_chunking_invariant() {
        use proptest::prelude::*;

        proptest!(|(data: Vec<u8>, split: usize)| {
            if data.is_empty() {
                return Ok(());
            }
            let split = split % data.len();
            let mut hasher = Crc32::new();
            hasher.update(&data[..split]);
            hasher.update(&data[split..]);
            prop_assert_eq!(hasher.finalize(), crc32(&data));
        });
    }
}
