//! Log compaction for merging and cleaning old segments.
//!
//! Compaction merges consecutive completed segments, removes tombstoned records
//! whose data records exist in later segments, and optionally compresses the
//! output. The hash chain is rebuilt for the compacted segment.
//!
//! # Algorithm
//!
//! 1. Select consecutive completed segments below `merge_threshold_bytes`
//! 2. Read all records, filter tombstones whose data exists in later segments
//! 3. Rewrite surviving records to a new segment, rebuilding the hash chain
//! 4. Rebuild offset index for the compacted segment
//! 5. Atomically update manifest (remove old entries, insert compacted)
//! 6. Delete old segment files and indexes

use kimberlite_types::CompressionKind;

/// Configuration for log compaction.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Minimum number of completed segments before compaction triggers.
    pub min_segments: usize,
    /// Maximum total size of segments to merge in one pass (bytes).
    pub merge_threshold_bytes: u64,
    /// Whether to compress records during compaction.
    pub compress_on_compact: bool,
    /// Compression algorithm to use when `compress_on_compact` is true.
    pub compression: CompressionKind,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            min_segments: 4,
            merge_threshold_bytes: 128 * 1024 * 1024, // 128 MB
            compress_on_compact: false,
            compression: CompressionKind::None,
        }
    }
}

/// Result of a compaction operation.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// Number of segments before compaction.
    pub segments_before: usize,
    /// Number of segments after compaction.
    pub segments_after: usize,
    /// Total bytes reclaimed by compaction.
    pub bytes_reclaimed: u64,
    /// Number of tombstone records removed.
    pub tombstones_removed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = CompactionConfig::default();
        assert_eq!(config.min_segments, 4);
        assert_eq!(config.merge_threshold_bytes, 128 * 1024 * 1024);
        assert!(!config.compress_on_compact);
        assert_eq!(config.compression, CompressionKind::None);
    }

    #[test]
    fn compaction_result_fields() {
        let result = CompactionResult {
            segments_before: 8,
            segments_after: 2,
            bytes_reclaimed: 512_000,
            tombstones_removed: 42,
        };
        assert_eq!(result.segments_before, 8);
        assert_eq!(result.segments_after, 2);
        assert_eq!(result.bytes_reclaimed, 512_000);
        assert_eq!(result.tombstones_removed, 42);
    }
}
