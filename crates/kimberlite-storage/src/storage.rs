//! Append-only event log storage with checkpoint support and segment rotation.
//!
//! The [`Storage`] struct manages segment files on disk, providing append and read
//! operations for event streams. Each stream gets its own directory with
//! numbered segment files that rotate when reaching the configured size limit.
//!
//! # File Layout
//!
//! ```text
//! {data_dir}/
//! └── {stream_id}/
//!     ├── segment_000000.log      <- first segment (immutable after rotation)
//!     ├── segment_000000.log.idx  <- offset index for segment 0
//!     ├── segment_000001.log      <- second segment (active)
//!     ├── segment_000001.log.idx  <- offset index for segment 1
//!     └── manifest.json           <- segment manifest (offset ranges)
//! ```
//!
//! # Segment Rotation
//!
//! When a segment exceeds `max_segment_size` bytes, a new segment is created.
//! Completed segments are immutable and can be safely memory-mapped. The hash
//! chain is continuous across segment boundaries.
//!
//! # Hash Chain Integrity
//!
//! Every record contains a cryptographic link (`prev_hash`) to the previous record,
//! forming a tamper-evident chain. Reads verify this chain from genesis (or a
//! checkpoint) to detect any corruption or tampering.
//!
//! # Checkpoints
//!
//! Checkpoints are periodic verification anchors stored as special records in the
//! log. They enable efficient verified reads by reducing verification from O(n)
//! to O(k) where k is the distance to the nearest checkpoint.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use bytes::Bytes;
use kimberlite_crypto::ChainHash;
use kimberlite_types::{CheckpointPolicy, Offset, RecordKind, StreamId};

use crate::checkpoint::{
    CheckpointIndex, deserialize_checkpoint_payload, serialize_checkpoint_payload,
};
use crate::{OffsetIndex, Record, StorageError};

/// Number of dirty records before an index is flushed to disk.
const INDEX_FLUSH_THRESHOLD: usize = 100;

/// Default maximum segment size in bytes (256 MB).
const DEFAULT_MAX_SEGMENT_SIZE: u64 = 256 * 1024 * 1024;

/// Manifest filename for segment metadata.
const MANIFEST_FILENAME: &str = "manifest.json";

/// Formats a segment filename from its number.
fn segment_filename(segment_num: u32) -> String {
    format!("segment_{segment_num:06}.log")
}

/// Formats an index filename from a segment number.
fn segment_index_filename(segment_num: u32) -> String {
    format!("segment_{segment_num:06}.log.idx")
}

/// Metadata for a single segment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SegmentMeta {
    /// Segment number (0-based).
    segment_num: u32,
    /// First logical offset in this segment.
    first_offset: u64,
    /// One past the last logical offset in this segment (exclusive).
    /// For the active segment this is the next offset to be written.
    next_offset: u64,
    /// Size of the segment file in bytes.
    size_bytes: u64,
}

/// Per-stream segment manifest tracking all segments and their offset ranges.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SegmentManifest {
    /// Ordered list of segments (ascending by `segment_num`).
    segments: Vec<SegmentMeta>,
    /// The currently active (writable) segment number.
    active_segment: u32,
}

impl SegmentManifest {
    /// Creates a new manifest with a single empty segment.
    fn new() -> Self {
        Self {
            segments: vec![SegmentMeta {
                segment_num: 0,
                first_offset: 0,
                next_offset: 0,
                size_bytes: 0,
            }],
            active_segment: 0,
        }
    }

    /// Persists the manifest to disk.
    fn save(&self, stream_dir: &std::path::Path) -> Result<(), StorageError> {
        let path = stream_dir.join(MANIFEST_FILENAME);
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Loads a manifest from disk.
    fn load(stream_dir: &std::path::Path) -> Result<Self, StorageError> {
        let path = stream_dir.join(MANIFEST_FILENAME);
        let json = fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&json).map_err(std::io::Error::other)?;
        Ok(manifest)
    }

    /// Returns the active segment metadata mutably.
    fn active_mut(&mut self) -> &mut SegmentMeta {
        self.segments
            .iter_mut()
            .find(|s| s.segment_num == self.active_segment)
            .expect("active segment must exist in manifest")
    }

    /// Finds which segment contains a given logical offset.
    fn find_segment_for_offset(&self, offset: u64) -> Option<&SegmentMeta> {
        // Binary search: segments are ordered by first_offset
        match self
            .segments
            .binary_search_by_key(&offset, |s| s.first_offset)
        {
            Ok(idx) => Some(&self.segments[idx]),
            Err(idx) => {
                if idx == 0 {
                    None
                } else {
                    let seg = &self.segments[idx - 1];
                    if offset < seg.next_offset {
                        Some(seg)
                    } else {
                        // Offset is in the active segment (beyond last completed segment)
                        self.segments.last()
                    }
                }
            }
        }
    }

    /// Adds a new segment and returns its number.
    fn rotate(&mut self, first_offset: u64) -> u32 {
        let new_num = self.active_segment + 1;
        self.segments.push(SegmentMeta {
            segment_num: new_num,
            first_offset,
            next_offset: first_offset,
            size_bytes: 0,
        });
        self.active_segment = new_num;
        new_num
    }
}

/// Append-only event log storage with checkpoint support and segment rotation.
///
/// Manages segment files on disk, providing append and read operations for
/// event streams. Each stream gets its own directory with numbered segment files.
/// Segments rotate when they exceed `max_segment_size` bytes.
///
/// # Invariants
///
/// - Records are append-only; existing data is never modified
/// - Each record links to the previous via `prev_hash` (hash chain)
/// - The offset index stays in sync with the log (updated atomically with appends)
/// - Checkpoints are created according to the configured policy
/// - Hash chain integrity is maintained across segment boundaries
#[derive(Debug, Clone)]
pub struct Storage {
    /// Root directory for all stream data.
    data_dir: PathBuf,

    /// In-memory cache of offset indexes, keyed by (stream, `segment_num`).
    /// Loaded lazily on first access, kept in sync during appends.
    index_cache: HashMap<(StreamId, u32), OffsetIndex>,

    /// In-memory cache of checkpoint indexes, keyed by stream.
    /// Rebuilt on first access by scanning for checkpoint records.
    checkpoint_cache: HashMap<StreamId, CheckpointIndex>,

    /// Policy for automatic checkpoint creation.
    checkpoint_policy: CheckpointPolicy,

    /// Tracks how many records have been appended to each stream/segment's index
    /// since the last index flush. Used to batch index writes for performance.
    index_dirty_count: HashMap<(StreamId, u32), usize>,

    /// Per-stream segment manifests.
    manifests: HashMap<StreamId, SegmentManifest>,

    /// Maximum segment size in bytes before rotation.
    max_segment_size: u64,

    /// Tracks the number of entries already flushed to the main index file
    /// for each (stream, segment). Entries beyond this count are in the WAL.
    index_flushed_count: HashMap<(StreamId, u32), usize>,

    /// Cached data for completed (immutable) segments.
    ///
    /// Only rotated segments are cached. Active segments are read fresh from disk
    /// since they may still be written to. This avoids repeated `fs::read()` calls
    /// and heap allocations for immutable data.
    segment_data_cache: HashMap<(StreamId, u32), Bytes>,
}

impl Storage {
    /// Creates a new storage instance with the given data directory.
    ///
    /// The directory will be created if it doesn't exist when the first
    /// write occurs. Uses the default checkpoint policy.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self::with_checkpoint_policy(data_dir, CheckpointPolicy::default())
    }

    /// Creates a new storage instance with a custom checkpoint policy.
    pub fn with_checkpoint_policy(
        data_dir: impl Into<PathBuf>,
        checkpoint_policy: CheckpointPolicy,
    ) -> Self {
        Self {
            data_dir: data_dir.into(),
            index_cache: HashMap::new(),
            checkpoint_cache: HashMap::new(),
            checkpoint_policy,
            index_dirty_count: HashMap::new(),
            manifests: HashMap::new(),
            max_segment_size: DEFAULT_MAX_SEGMENT_SIZE,
            index_flushed_count: HashMap::new(),
            segment_data_cache: HashMap::new(),
        }
    }

    /// Creates a new storage instance with a custom maximum segment size.
    pub fn with_max_segment_size(
        data_dir: impl Into<PathBuf>,
        checkpoint_policy: CheckpointPolicy,
        max_segment_size: u64,
    ) -> Self {
        Self {
            data_dir: data_dir.into(),
            index_cache: HashMap::new(),
            checkpoint_cache: HashMap::new(),
            checkpoint_policy,
            index_dirty_count: HashMap::new(),
            manifests: HashMap::new(),
            max_segment_size,
            index_flushed_count: HashMap::new(),
            segment_data_cache: HashMap::new(),
        }
    }

    /// Returns the current checkpoint policy.
    pub fn checkpoint_policy(&self) -> &CheckpointPolicy {
        &self.checkpoint_policy
    }

    /// Returns the data directory path.
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Returns the maximum segment size in bytes.
    pub fn max_segment_size(&self) -> u64 {
        self.max_segment_size
    }

    /// Returns the stream directory path.
    fn stream_dir(&self, stream_id: StreamId) -> PathBuf {
        self.data_dir.join(stream_id.to_string())
    }

    /// Returns the path to a specific segment file.
    fn segment_path_for(&self, stream_id: StreamId, segment_num: u32) -> PathBuf {
        self.stream_dir(stream_id)
            .join(segment_filename(segment_num))
    }

    /// Returns the path to the index file for a specific segment.
    fn index_path_for(&self, stream_id: StreamId, segment_num: u32) -> PathBuf {
        self.stream_dir(stream_id)
            .join(segment_index_filename(segment_num))
    }

    /// Returns the path to the index file for the active segment.
    pub fn index_path(&self, stream_id: StreamId) -> PathBuf {
        let segment_num = self
            .manifests
            .get(&stream_id)
            .map_or(0, |m| m.active_segment);
        self.index_path_for(stream_id, segment_num)
    }

    /// Gets or loads the manifest for a stream.
    ///
    /// If no manifest exists on disk, creates a fresh empty manifest.
    fn get_or_load_manifest(
        &mut self,
        stream_id: StreamId,
    ) -> Result<&mut SegmentManifest, StorageError> {
        if !self.manifests.contains_key(&stream_id) {
            let stream_dir = self.stream_dir(stream_id);
            let manifest = if stream_dir.join(MANIFEST_FILENAME).exists() {
                SegmentManifest::load(&stream_dir)?
            } else {
                SegmentManifest::new()
            };
            self.manifests.insert(stream_id, manifest);
        }
        Ok(self.manifests.get_mut(&stream_id).expect("just inserted"))
    }

    /// Rebuilds the offset index for a specific segment by scanning the log file.
    ///
    /// This is the recovery path when the index file is missing or corrupted.
    /// Scans every record in the segment to reconstruct byte positions.
    ///
    /// # Performance
    ///
    /// O(n) where n is the number of records in the segment.
    /// With segment rotation, this is bounded to a single segment's worth of data.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::CorruptedRecord`] if any record in the log is invalid.
    pub fn rebuild_index(&self, stream_id: StreamId) -> Result<OffsetIndex, StorageError> {
        let segment_num = self
            .manifests
            .get(&stream_id)
            .map_or(0, |m| m.active_segment);
        self.rebuild_index_for_segment(stream_id, segment_num)
    }

    /// Rebuilds the offset index for a specific segment.
    fn rebuild_index_for_segment(
        &self,
        stream_id: StreamId,
        segment_num: u32,
    ) -> Result<OffsetIndex, StorageError> {
        let segment_path = self.segment_path_for(stream_id, segment_num);

        if !segment_path.exists() {
            return Ok(OffsetIndex::new());
        }

        let data: Bytes = fs::read(&segment_path)?.into();
        let mut index = OffsetIndex::new();
        let mut pos = 0;

        while pos < data.len() {
            index.append(pos as u64);
            let (_, consumed) = Record::from_bytes(&data.slice(pos..))?;
            pos += consumed;
        }

        // Postcondition: index has one entry per record
        debug_assert_eq!(
            index.len(),
            {
                let mut count = 0;
                let mut p = 0;
                while p < data.len() {
                    let (_, c) = Record::from_bytes(&data.slice(p..)).unwrap();
                    p += c;
                    count += 1;
                }
                count
            },
            "index entry count mismatch"
        );

        let index_path = self.index_path_for(stream_id, segment_num);
        index.save(&index_path)?;

        Ok(index)
    }

    /// Loads the offset index for a specific segment from disk, or rebuilds it.
    ///
    /// Attempts to load the main index + replay WAL (fast path). Falls back
    /// to a full log scan if the index is corrupted.
    fn load_or_rebuild_index_for_segment(
        &self,
        stream_id: StreamId,
        segment_num: u32,
    ) -> Result<OffsetIndex, StorageError> {
        let index_path = self.index_path_for(stream_id, segment_num);

        // Try load with WAL replay (fast path)
        if let Ok(index) = OffsetIndex::load_with_wal(&index_path) {
            return Ok(index);
        }

        // Fall back to plain load (no WAL)
        if let Ok(index) = OffsetIndex::load(&index_path) {
            return Ok(index);
        }

        tracing::warn!(
            stream_id = %stream_id,
            segment_num = segment_num,
            "index missing or corrupted, rebuilding from log"
        );
        self.rebuild_index_for_segment(stream_id, segment_num)
    }

    /// Loads the offset index from disk, or rebuilds it if missing/corrupted.
    ///
    /// This is the primary way to obtain an index for the active segment.
    pub fn load_or_rebuild_index(&self, stream_id: StreamId) -> Result<OffsetIndex, StorageError> {
        let segment_num = self
            .manifests
            .get(&stream_id)
            .map_or(0, |m| m.active_segment);
        self.load_or_rebuild_index_for_segment(stream_id, segment_num)
    }

    /// Ensures the index for a given (stream, segment) is in the cache.
    fn ensure_index_cached(
        &mut self,
        stream_id: StreamId,
        segment_num: u32,
    ) -> Result<(), StorageError> {
        let key = (stream_id, segment_num);
        if !self.index_cache.contains_key(&key) {
            let loaded = self.load_or_rebuild_index_for_segment(stream_id, segment_num)?;
            let flushed = loaded.len(); // Everything loaded is considered "flushed"
            self.index_cache.insert(key, loaded);
            self.index_flushed_count.insert(key, flushed);
        }
        Ok(())
    }

    /// Reads segment data, using cached `Bytes` for completed segments and fresh `fs::read` for active.
    ///
    /// Completed (immutable) segments are memory-mapped for zero-copy access.
    /// The active segment uses standard I/O since it may still be written to.
    fn read_segment_data(
        &mut self,
        stream_id: StreamId,
        segment_num: u32,
    ) -> Result<Bytes, StorageError> {
        let is_active = self
            .manifests
            .get(&stream_id)
            .is_some_and(|m| m.active_segment == segment_num);

        if is_active {
            // Active segment: read fresh from disk (file may still be written to)
            let path = self.segment_path_for(stream_id, segment_num);
            Ok(fs::read(&path)?.into())
        } else {
            // Completed segment: return cached data or read and cache
            let key = (stream_id, segment_num);
            if let Some(cached) = self.segment_data_cache.get(&key) {
                return Ok(cached.clone());
            }

            let path = self.segment_path_for(stream_id, segment_num);
            let data: Bytes = fs::read(&path)?.into();
            self.segment_data_cache.insert(key, data.clone());
            Ok(data)
        }
    }

    /// Returns a list of all segment numbers for a stream, in order.
    fn segment_numbers(&self, stream_id: StreamId) -> Vec<u32> {
        self.manifests.get(&stream_id).map_or_else(
            || {
                // No manifest yet, check if segment 0 exists
                if self.segment_path_for(stream_id, 0).exists() {
                    vec![0]
                } else {
                    vec![]
                }
            },
            |m| m.segments.iter().map(|s| s.segment_num).collect(),
        )
    }

    /// Appends a batch of events to a stream, building the hash chain.
    ///
    /// Each event is written as a [`Record`] with a cryptographic link to the
    /// previous record, forming a tamper-evident chain. The offset index is
    /// updated atomically with the append to maintain O(1) lookup capability.
    ///
    /// If the active segment exceeds `max_segment_size`, a new segment is
    /// created (rotation). The hash chain remains continuous across segments.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - The stream to append to
    /// * `events` - The event payloads to append (must not be empty)
    /// * `expected_offset` - The offset to start writing at
    /// * `prev_hash` - Hash of the previous record (`None` for genesis)
    /// * `fsync` - Whether to fsync after writing (recommended for durability)
    ///
    /// # Returns
    ///
    /// A tuple of:
    /// - The next offset (for subsequent appends)
    /// - The hash of the last record written (for chain continuity)
    ///
    /// # Panics
    ///
    /// Panics if `events` is empty. Empty batches are a caller bug.
    pub fn append_batch(
        &mut self,
        stream_id: StreamId,
        events: Vec<Bytes>,
        expected_offset: Offset,
        prev_hash: Option<ChainHash>,
        fsync: bool,
    ) -> Result<(Offset, ChainHash), StorageError> {
        // Precondition: batch must not be empty
        assert!(!events.is_empty(), "cannot append empty batch");

        let event_count = events.len();

        // Ensure stream directory exists
        let stream_dir = self.stream_dir(stream_id);
        fs::create_dir_all(&stream_dir)?;

        // Load or create manifest
        let manifest = self.get_or_load_manifest(stream_id)?;
        let active_seg = manifest.active_segment;

        // Open segment file for appending
        let segment_path = self.segment_path_for(stream_id, active_seg);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&segment_path)?;

        // Get current file size as starting byte position for new records
        let mut byte_position: u64 = file.metadata()?.len();

        // Pre-compute paths
        let index_path = self.index_path_for(stream_id, active_seg);
        let cache_key = (stream_id, active_seg);

        // Load index into cache if not present
        self.ensure_index_cached(stream_id, active_seg)?;

        let index = self
            .index_cache
            .get_mut(&cache_key)
            .expect("index exists: just ensured");

        let mut current_offset = expected_offset;
        let mut current_hash = prev_hash;

        for event in events {
            // Record byte position BEFORE writing (where this record starts)
            index.append(byte_position);

            let record = Record::new(current_offset, current_hash, event);
            let record_bytes = record.to_bytes();

            // Update position AFTER computing record size
            byte_position += record_bytes.len() as u64;

            file.write_all(&record_bytes)?;

            current_hash = Some(record.compute_hash());
            current_offset += Offset::from(1u64);
        }

        // Ensure durability if requested
        if fsync {
            file.sync_all()?;
        }

        // Update manifest with new segment size and offset
        let manifest = self.manifests.get_mut(&stream_id).expect("manifest loaded");
        let active_meta = manifest.active_mut();
        active_meta.size_bytes = byte_position;
        active_meta.next_offset = current_offset.as_u64();

        // Track dirty count and use WAL for incremental index writes.
        // WAL appends are O(1) per entry vs O(n) for full index rewrite.
        // Compaction to full index happens when WAL reaches 1000 entries.
        let cache_key_for_flush = (stream_id, active_seg);
        let dirty = self
            .index_dirty_count
            .entry(cache_key_for_flush)
            .or_insert(0);
        *dirty += event_count;
        if *dirty >= INDEX_FLUSH_THRESHOLD || fsync {
            let index = self
                .index_cache
                .get(&cache_key_for_flush)
                .expect("index exists: just used above");
            let flushed = *self
                .index_flushed_count
                .get(&cache_key_for_flush)
                .unwrap_or(&0);
            // Use WAL for incremental writes; compact at 1000 WAL entries
            index.save_incremental(&index_path, flushed, 1000)?;
            // After save_incremental, all entries up to current len are on disk (main + WAL)
            self.index_flushed_count
                .insert(cache_key_for_flush, index.len());
            *dirty = 0;
        }

        // Check if segment rotation is needed
        if byte_position >= self.max_segment_size {
            self.rotate_segment(stream_id, current_offset)?;
        }

        // Persist manifest after writes
        let stream_dir = self.stream_dir(stream_id);
        let manifest = self.manifests.get(&stream_id).expect("manifest loaded");
        manifest.save(&stream_dir)?;

        // Postcondition: we wrote exactly event_count records
        debug_assert_eq!(
            current_offset.as_u64() - expected_offset.as_u64(),
            event_count as u64,
            "offset mismatch after batch write"
        );

        Ok((current_offset, current_hash.expect("batch was non-empty")))
    }

    /// Rotates the active segment for a stream.
    ///
    /// Flushes the current segment's index and creates a new empty segment.
    fn rotate_segment(
        &mut self,
        stream_id: StreamId,
        next_offset: Offset,
    ) -> Result<(), StorageError> {
        let old_seg = self
            .manifests
            .get(&stream_id)
            .expect("manifest loaded")
            .active_segment;

        // Flush the old segment's index
        let old_key = (stream_id, old_seg);
        if let Some(index) = self.index_cache.get(&old_key) {
            let index_path = self.index_path_for(stream_id, old_seg);
            index.save(&index_path)?;
        }
        self.index_dirty_count.insert(old_key, 0);

        // Rotate to new segment
        let manifest = self.manifests.get_mut(&stream_id).expect("manifest loaded");
        let new_seg = manifest.rotate(next_offset.as_u64());

        tracing::info!(
            stream_id = %stream_id,
            old_segment = old_seg,
            new_segment = new_seg,
            "rotated segment"
        );

        Ok(())
    }

    /// Reads events from a stream with checkpoint-optimized verification.
    ///
    /// Uses the nearest checkpoint as a verification anchor, reducing verification
    /// cost from O(n) to O(k) where k is the distance to the nearest checkpoint.
    /// Falls back to genesis verification if no checkpoints exist.
    ///
    /// Reads span across segment boundaries transparently.
    pub fn read_from(
        &mut self,
        stream_id: StreamId,
        from_offset: Offset,
        max_bytes: u64,
    ) -> Result<Vec<Bytes>, StorageError> {
        let records = self.read_records_verified(stream_id, from_offset, max_bytes)?;
        Ok(records.into_iter().map(|r| r.payload().clone()).collect())
    }

    /// Reads events from a stream with full genesis verification.
    ///
    /// Verifies the hash chain from genesis (offset 0) across all segments.
    /// For most use cases, prefer [`Self::read_from`] which uses checkpoint-optimized
    /// verification for better performance.
    pub fn read_from_genesis(
        &mut self,
        stream_id: StreamId,
        from_offset: Offset,
        max_bytes: u64,
    ) -> Result<Vec<Bytes>, StorageError> {
        let records = self.read_records_from_genesis(stream_id, from_offset, max_bytes)?;
        Ok(records.into_iter().map(|r| r.payload().clone()).collect())
    }

    /// Reads records from a stream with full genesis hash chain verification.
    ///
    /// Verifies the hash chain from genesis up to and including the requested
    /// records, spanning all segments. This ensures tamper detection.
    pub fn read_records_from_genesis(
        &mut self,
        stream_id: StreamId,
        from_offset: Offset,
        max_bytes: u64,
    ) -> Result<Vec<Record>, StorageError> {
        let segment_nums = self.segment_numbers(stream_id);

        let mut results = Vec::new();
        let mut bytes_read: u64 = 0;
        let mut expected_prev_hash: Option<ChainHash> = None;
        let mut records_verified: u64 = 0;

        for seg_num in segment_nums {
            let seg_path = self.segment_path_for(stream_id, seg_num);
            if !seg_path.exists() {
                continue;
            }

            let data = self.read_segment_data(stream_id, seg_num)?;
            let mut pos = 0;

            while pos < data.len() && bytes_read < max_bytes {
                let (record, consumed) = Record::from_bytes(&data.slice(pos..))?;

                // Verify hash chain integrity
                if record.prev_hash() != expected_prev_hash {
                    return Err(StorageError::ChainVerificationFailed {
                        offset: record.offset(),
                        expected: expected_prev_hash,
                        actual: record.prev_hash(),
                    });
                }

                expected_prev_hash = Some(record.compute_hash());
                records_verified += 1;
                pos += consumed;

                // Only collect records at or after the requested offset
                if record.offset() < from_offset {
                    continue;
                }

                bytes_read += record.payload().len() as u64;
                results.push(record);
            }

            if bytes_read >= max_bytes {
                break;
            }
        }

        // Postcondition: we verified all records we read
        debug_assert!(
            records_verified == 0 || expected_prev_hash.is_some(),
            "verified records but no final hash"
        );

        Ok(results)
    }

    // ========================================================================
    // Checkpoint Support
    // ========================================================================

    /// Rebuilds the checkpoint index by scanning all segments for checkpoint records.
    pub fn rebuild_checkpoint_index(
        &mut self,
        stream_id: StreamId,
    ) -> Result<CheckpointIndex, StorageError> {
        let segment_nums = self.segment_numbers(stream_id);
        let mut checkpoint_index = CheckpointIndex::new();

        for seg_num in segment_nums {
            let seg_path = self.segment_path_for(stream_id, seg_num);
            if !seg_path.exists() {
                continue;
            }

            let data = self.read_segment_data(stream_id, seg_num)?;
            let mut pos = 0;

            while pos < data.len() {
                let (record, consumed) = Record::from_bytes(&data.slice(pos..))?;

                if record.is_checkpoint() {
                    checkpoint_index.add(record.offset());
                }

                pos += consumed;
            }
        }

        tracing::debug!(
            stream_id = %stream_id,
            checkpoint_count = checkpoint_index.len(),
            "rebuilt checkpoint index"
        );

        Ok(checkpoint_index)
    }

    /// Gets the checkpoint index for a stream, rebuilding if necessary.
    fn get_or_rebuild_checkpoint_index(
        &mut self,
        stream_id: StreamId,
    ) -> Result<&CheckpointIndex, StorageError> {
        if !self.checkpoint_cache.contains_key(&stream_id) {
            let index = self.rebuild_checkpoint_index(stream_id)?;
            self.checkpoint_cache.insert(stream_id, index);
        }
        Ok(self
            .checkpoint_cache
            .get(&stream_id)
            .expect("just inserted"))
    }

    /// Creates a checkpoint at the current position in the active segment.
    pub fn create_checkpoint(
        &mut self,
        stream_id: StreamId,
        current_offset: Offset,
        prev_hash: Option<ChainHash>,
        record_count: u64,
        fsync: bool,
    ) -> Result<(Offset, ChainHash), StorageError> {
        let chain_hash = prev_hash.unwrap_or_else(|| ChainHash::from_bytes(&[0u8; 32]));

        let payload = serialize_checkpoint_payload(&chain_hash, record_count);

        let record = Record::with_kind(current_offset, prev_hash, RecordKind::Checkpoint, payload);
        let record_bytes = record.to_bytes();
        let record_hash = record.compute_hash();

        // Ensure stream directory exists and manifest loaded
        let stream_dir = self.stream_dir(stream_id);
        fs::create_dir_all(&stream_dir)?;
        let manifest = self.get_or_load_manifest(stream_id)?;
        let active_seg = manifest.active_segment;

        // Open active segment file for appending
        let segment_path = self.segment_path_for(stream_id, active_seg);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&segment_path)?;

        let byte_position = file.metadata()?.len();

        file.write_all(&record_bytes)?;

        if fsync {
            file.sync_all()?;
        }

        // Update offset index
        let cache_key = (stream_id, active_seg);
        let index_path = self.index_path_for(stream_id, active_seg);
        self.ensure_index_cached(stream_id, active_seg)?;
        let index = self.index_cache.get_mut(&cache_key).expect("just loaded");
        index.append(byte_position);
        // Checkpoints are safety boundaries — always flush full index + compact WAL
        index.save(&index_path)?;
        self.index_dirty_count.insert(cache_key, 0);
        self.index_flushed_count.insert(cache_key, index.len());
        // Remove WAL after compaction
        let wal_path = {
            let mut p = index_path.as_os_str().to_owned();
            p.push(".wal");
            std::path::PathBuf::from(p)
        };
        let _ = fs::remove_file(wal_path);

        // Update manifest
        let new_size = byte_position + record_bytes.len() as u64;
        let manifest = self.manifests.get_mut(&stream_id).expect("manifest loaded");
        let active_meta = manifest.active_mut();
        active_meta.size_bytes = new_size;
        active_meta.next_offset = current_offset.as_u64() + 1;
        manifest.save(&stream_dir)?;

        // Update checkpoint index
        if let Some(cp_index) = self.checkpoint_cache.get_mut(&stream_id) {
            cp_index.add(current_offset);
        }

        tracing::info!(
            stream_id = %stream_id,
            offset = %current_offset,
            record_count = record_count,
            "created checkpoint"
        );

        let next_offset = current_offset + Offset::from(1u64);
        Ok((next_offset, record_hash))
    }

    /// Reads records with checkpoint-optimized verification.
    ///
    /// Instead of verifying from genesis, this method verifies from the nearest
    /// checkpoint before `from_offset`. Reads span across segment boundaries.
    pub fn read_records_verified(
        &mut self,
        stream_id: StreamId,
        from_offset: Offset,
        max_bytes: u64,
    ) -> Result<Vec<Record>, StorageError> {
        // Load manifest to know about segments
        let _ = self.get_or_load_manifest(stream_id);
        let segment_nums = self.segment_numbers(stream_id);

        if segment_nums.is_empty() {
            return Ok(Vec::new());
        }

        // Check if the first segment even exists
        let first_seg_path = self.segment_path_for(stream_id, segment_nums[0]);
        if !first_seg_path.exists() {
            return Ok(Vec::new());
        }

        // Find nearest checkpoint
        let checkpoint_index = self.get_or_rebuild_checkpoint_index(stream_id)?;
        let verification_start = checkpoint_index.find_nearest(from_offset);

        // Determine which segment to start verification from
        let (start_seg_num, start_pos, mut expected_prev_hash) = match verification_start {
            Some(cp_offset) => {
                // Find which segment contains this checkpoint
                let manifest = self.manifests.get(&stream_id);
                let seg_num = manifest
                    .and_then(|m| {
                        m.find_segment_for_offset(cp_offset.as_u64())
                            .map(|s| s.segment_num)
                    })
                    .unwrap_or(0);

                // Load the index for that segment to get byte position
                self.ensure_index_cached(stream_id, seg_num)?;
                let offset_index = self
                    .index_cache
                    .get(&(stream_id, seg_num))
                    .expect("just ensured");

                // The checkpoint offset within this segment
                let first_offset_in_seg = self
                    .manifests
                    .get(&stream_id)
                    .and_then(|m| {
                        m.find_segment_for_offset(cp_offset.as_u64())
                            .map(|s| s.first_offset)
                    })
                    .unwrap_or(0);
                let local_offset = Offset::new(cp_offset.as_u64() - first_offset_in_seg);

                let byte_pos = offset_index
                    .lookup(local_offset)
                    .ok_or(StorageError::UnexpectedEof)?;

                // Read checkpoint record to get its chain_hash
                let data = self.read_segment_data(stream_id, seg_num)?;
                let (cp_record, _) = Record::from_bytes(&data.slice(byte_pos as usize..))?;
                debug_assert!(cp_record.is_checkpoint());

                let (chain_hash, _) =
                    deserialize_checkpoint_payload(cp_record.payload(), cp_offset)?;

                (seg_num, byte_pos as usize, Some(chain_hash))
            }
            None => (segment_nums[0], 0, None),
        };

        let mut results = Vec::new();
        let mut bytes_read: u64 = 0;
        let mut started = false;

        for &seg_num in &segment_nums {
            if seg_num < start_seg_num {
                continue;
            }

            let seg_path = self.segment_path_for(stream_id, seg_num);
            if !seg_path.exists() {
                continue;
            }

            let data = self.read_segment_data(stream_id, seg_num)?;
            let mut pos = if seg_num == start_seg_num && !started {
                started = true;
                start_pos
            } else {
                0
            };

            while pos < data.len() && bytes_read < max_bytes {
                let (record, consumed) = Record::from_bytes(&data.slice(pos..))?;

                if record.prev_hash() != expected_prev_hash {
                    return Err(StorageError::ChainVerificationFailed {
                        offset: record.offset(),
                        expected: expected_prev_hash,
                        actual: record.prev_hash(),
                    });
                }

                expected_prev_hash = Some(record.compute_hash());
                pos += consumed;

                if record.offset() >= from_offset && !record.is_checkpoint() {
                    bytes_read += record.payload().len() as u64;
                    results.push(record);
                }
            }

            if bytes_read >= max_bytes {
                break;
            }
        }

        Ok(results)
    }

    /// Returns the last checkpoint for a stream, if any.
    pub fn last_checkpoint(&mut self, stream_id: StreamId) -> Result<Option<Offset>, StorageError> {
        let index = self.get_or_rebuild_checkpoint_index(stream_id)?;
        Ok(index.last())
    }

    /// Returns information about all segments for a stream.
    pub fn segment_count(&self, stream_id: StreamId) -> usize {
        self.manifests
            .get(&stream_id)
            .map_or(0, |m| m.segments.len())
    }

    /// Returns the list of completed (immutable) segment numbers for a stream.
    pub fn completed_segments(&self, stream_id: StreamId) -> Vec<u32> {
        self.manifests.get(&stream_id).map_or_else(Vec::new, |m| {
            m.segments
                .iter()
                .filter(|s| s.segment_num != m.active_segment)
                .map(|s| s.segment_num)
                .collect()
        })
    }

    /// Flushes all dirty indexes to disk.
    ///
    /// Call this on shutdown or before operations that require index durability.
    /// This is also called automatically from `Drop` to prevent stale indexes.
    pub fn flush_indexes(&mut self) -> Result<(), StorageError> {
        let dirty_keys: Vec<(StreamId, u32)> = self
            .index_dirty_count
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(&key, _)| key)
            .collect();

        let mut first_error: Option<StorageError> = None;

        for (stream_id, segment_num) in dirty_keys {
            if let Some(index) = self.index_cache.get(&(stream_id, segment_num)) {
                let index_path = self.index_path_for(stream_id, segment_num);
                // Full save compacts the WAL into the main index
                if let Err(e) = index.save(&index_path) {
                    tracing::error!(
                        stream_id = %stream_id,
                        segment_num = segment_num,
                        error = %e,
                        "failed to flush index on shutdown"
                    );
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                } else {
                    self.index_dirty_count.insert((stream_id, segment_num), 0);
                    self.index_flushed_count
                        .insert((stream_id, segment_num), index.len());
                    // Remove WAL file after successful compaction
                    let wal_path = {
                        let mut p = index_path.as_os_str().to_owned();
                        p.push(".wal");
                        std::path::PathBuf::from(p)
                    };
                    let _ = fs::remove_file(wal_path);
                }
            }
        }

        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        if let Err(e) = self.flush_indexes() {
            tracing::error!(error = %e, "failed to flush indexes during Storage drop");
        }
    }
}
