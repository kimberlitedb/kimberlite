//! Compression codecs for record payloads.
//!
//! Provides a [`Codec`] trait with implementations for LZ4 and Zstandard.
//! Codecs are registered in a [`CodecRegistry`] for lookup by [`CompressionKind`].

use kimberlite_types::CompressionKind;

use crate::StorageError;

/// A compression/decompression codec.
pub trait Codec: Send + Sync {
    /// Returns the compression kind for this codec.
    fn kind(&self) -> CompressionKind;

    /// Compresses input data.
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>, StorageError>;

    /// Decompresses previously compressed data.
    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>, StorageError>;
}

/// No-op codec (passthrough).
#[derive(Debug, Clone, Copy)]
pub struct NoneCodec;

impl Codec for NoneCodec {
    fn kind(&self) -> CompressionKind {
        CompressionKind::None
    }

    fn compress(&self, input: &[u8]) -> Result<Vec<u8>, StorageError> {
        Ok(input.to_vec())
    }

    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>, StorageError> {
        Ok(input.to_vec())
    }
}

/// LZ4 codec using `lz4_flex` (pure Rust, fast).
#[derive(Debug, Clone, Copy)]
pub struct Lz4Codec;

impl Codec for Lz4Codec {
    fn kind(&self) -> CompressionKind {
        CompressionKind::Lz4
    }

    fn compress(&self, input: &[u8]) -> Result<Vec<u8>, StorageError> {
        Ok(lz4_flex::compress_prepend_size(input))
    }

    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>, StorageError> {
        lz4_flex::decompress_size_prepended(input).map_err(|e| {
            StorageError::DecompressionFailed {
                codec: "lz4",
                reason: e.to_string(),
            }
        })
    }
}

/// Zstandard codec with configurable compression level.
#[derive(Debug, Clone, Copy)]
pub struct ZstdCodec {
    /// Compression level (1-22, default 3).
    pub level: i32,
}

impl ZstdCodec {
    /// Creates a new Zstd codec with the given compression level.
    pub fn new(level: i32) -> Self {
        Self { level }
    }
}

impl Default for ZstdCodec {
    fn default() -> Self {
        Self { level: 3 }
    }
}

impl Codec for ZstdCodec {
    fn kind(&self) -> CompressionKind {
        CompressionKind::Zstd
    }

    fn compress(&self, input: &[u8]) -> Result<Vec<u8>, StorageError> {
        zstd::encode_all(input, self.level).map_err(|e| StorageError::CompressionFailed {
            codec: "zstd",
            reason: e.to_string(),
        })
    }

    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>, StorageError> {
        zstd::decode_all(input).map_err(|e| StorageError::DecompressionFailed {
            codec: "zstd",
            reason: e.to_string(),
        })
    }
}

/// Registry of compression codecs, keyed by [`CompressionKind`].
#[derive(Debug)]
pub struct CodecRegistry {
    lz4: Lz4Codec,
    zstd: ZstdCodec,
    none: NoneCodec,
}

impl CodecRegistry {
    /// Creates a registry with default codec settings.
    pub fn new() -> Self {
        Self {
            lz4: Lz4Codec,
            zstd: ZstdCodec::default(),
            none: NoneCodec,
        }
    }

    /// Creates a registry with a custom Zstd compression level.
    pub fn with_zstd_level(level: i32) -> Self {
        Self {
            lz4: Lz4Codec,
            zstd: ZstdCodec::new(level),
            none: NoneCodec,
        }
    }

    /// Returns the codec for the given compression kind.
    pub fn get(&self, kind: CompressionKind) -> &dyn Codec {
        match kind {
            CompressionKind::None => &self.none,
            CompressionKind::Lz4 => &self.lz4,
            CompressionKind::Zstd => &self.zstd,
        }
    }

    /// Compresses data using the specified codec.
    pub fn compress(&self, kind: CompressionKind, data: &[u8]) -> Result<Vec<u8>, StorageError> {
        self.get(kind).compress(data)
    }

    /// Decompresses data using the specified codec.
    pub fn decompress(
        &self,
        kind: CompressionKind,
        data: &[u8],
    ) -> Result<Vec<u8>, StorageError> {
        self.get(kind).decompress(data)
    }
}

impl Default for CodecRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_codec_roundtrip() {
        let codec = NoneCodec;
        let data = b"hello world";
        let compressed = codec.compress(data).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(data.as_slice(), &decompressed);
    }

    #[test]
    fn lz4_codec_roundtrip() {
        let codec = Lz4Codec;
        let data = b"hello world hello world hello world";
        let compressed = codec.compress(data).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(data.as_slice(), &decompressed);
    }

    #[test]
    fn zstd_codec_roundtrip() {
        let codec = ZstdCodec::default();
        let data = b"hello world hello world hello world";
        let compressed = codec.compress(data).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(data.as_slice(), &decompressed);
    }

    #[test]
    fn lz4_compresses_repetitive_data() {
        let codec = Lz4Codec;
        let data: Vec<u8> = vec![42; 10_000];
        let compressed = codec.compress(&data).unwrap();
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn zstd_compresses_repetitive_data() {
        let codec = ZstdCodec::default();
        let data: Vec<u8> = vec![42; 10_000];
        let compressed = codec.compress(&data).unwrap();
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn codec_registry_lookup() {
        let registry = CodecRegistry::new();
        assert_eq!(registry.get(CompressionKind::None).kind(), CompressionKind::None);
        assert_eq!(registry.get(CompressionKind::Lz4).kind(), CompressionKind::Lz4);
        assert_eq!(registry.get(CompressionKind::Zstd).kind(), CompressionKind::Zstd);
    }

    #[test]
    fn codec_registry_roundtrip() {
        let registry = CodecRegistry::new();
        let data = b"test data for codec registry roundtrip";

        for kind in [CompressionKind::None, CompressionKind::Lz4, CompressionKind::Zstd] {
            let compressed = registry.compress(kind, data).unwrap();
            let decompressed = registry.decompress(kind, &compressed).unwrap();
            assert_eq!(data.as_slice(), &decompressed, "roundtrip failed for {kind}");
        }
    }

    #[test]
    fn empty_data_roundtrip() {
        let registry = CodecRegistry::new();
        let data = b"";

        for kind in [CompressionKind::None, CompressionKind::Lz4, CompressionKind::Zstd] {
            let compressed = registry.compress(kind, data).unwrap();
            let decompressed = registry.decompress(kind, &compressed).unwrap();
            assert_eq!(data.as_slice(), &decompressed, "empty roundtrip failed for {kind}");
        }
    }
}
