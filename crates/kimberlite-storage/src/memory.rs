//! Pure in-memory implementation of [`StorageBackend`].
//!
//! Mirrors enough of the on-disk [`crate::Storage`] semantics for tests
//! and ephemeral worker processes to run the full `Kimberlite` stack
//! without touching disk. Explicitly NOT a fault-injection backend —
//! see `kimberlite-sim::SimStorage` for that.
//!
//! # What is preserved
//!
//! - **Hash-chain determinism**: `append_batch` computes per-record
//!   hashes exactly like [`crate::Storage`]. A byte-for-byte comparable
//!   sequence of appends produces the same terminal `ChainHash`. The
//!   parity test in `crate::tests` locks this in.
//! - **Offset monotonicity**: offsets advance by `events.len()` per
//!   batch, matching the on-disk impl.
//! - **Notional segment rotation**: when the total in-memory size for
//!   a stream would exceed `max_segment_size`, a new logical segment
//!   is opened. The data isn't actually partitioned — the segment
//!   boundaries are just bookkeeping that lets `segment_count` and
//!   `completed_segments` return the same counts as the on-disk impl
//!   would on the equivalent workload.
//! - **Record format**: events are stored as encoded [`Record`] bytes
//!   so that read paths produce the same visible payload sequence as
//!   the on-disk impl. A pure `Vec<Bytes>` would have been cheaper
//!   but would drift from the on-disk format, making parity tests
//!   fragile.
//!
//! # What is dropped
//!
//! - No fsync. `fsync` argument is silently ignored.
//! - No durable state across process restarts — reopening
//!   `MemoryStorage::new()` always starts empty. `latest_chain_hash`
//!   on a stream you never appended to returns `Ok(None)`.
//! - No mmap, no segment files, no manifest.json.
//! - No compression. All records are stored with `CompressionKind::None`.
//!   (The on-disk impl only compresses when it actually reduces size,
//!   so opting out entirely is a semantics-preserving simplification.)
//! - No checkpoint records. `read_from` never synthesises checkpoints,
//!   so the `read_records_verified` code path used by `Storage`
//!   naturally collapses to a linear walk. That's fine — `MemoryStorage`
//!   is optimised for latency, not for minimising verification cost.
//!
//! # Assertions
//!
//! Production assertions documented per `docs/internals/testing/assertions-inventory.md`:
//! - `append_batch` asserts `!events.is_empty()` (matches `Storage`).
//! - `append_batch` asserts offset monotonicity post-write.
//!
//! Each has a paired `#[should_panic]` test in the crate tests module.

use std::collections::HashMap;

use bytes::Bytes;
use kimberlite_crypto::ChainHash;
use kimberlite_types::{CompressionKind, Offset, RecordKind, StreamId};

use crate::backend::StorageBackend;
use crate::error::StorageError;
use crate::record::Record;

/// Default maximum segment size in bytes (256 MB).
///
/// Mirrors the `Storage` constant. Kept private so the two can drift
/// independently if the on-disk backend ever changes its default
/// without breaking the memory backend's tests.
const DEFAULT_MAX_SEGMENT_SIZE: u64 = 256 * 1024 * 1024;

/// Bookkeeping for a single notional segment.
///
/// Records aren't actually partitioned — the `bytes` field is the
/// shared per-stream record buffer. Each segment just remembers its
/// byte range within that buffer so `segment_count` /
/// `completed_segments` return on-disk-equivalent numbers.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // first_offset/next_offset mirror the on-disk manifest layout
struct SegmentMeta {
    /// Segment number (0-based).
    segment_num: u32,
    /// First logical offset in this segment.
    first_offset: u64,
    /// One past the last logical offset in this segment.
    next_offset: u64,
    /// Size of this segment in bytes.
    size_bytes: u64,
}

/// Per-stream state: all records serialized into a flat buffer plus a
/// segment manifest that tracks notional rotation boundaries.
///
/// The `Default` impl is intentionally absent — a brand-new stream
/// needs a one-element manifest (segment 0), which `Default::default()`
/// would produce an empty `Vec` for. Use [`StreamState::new`] instead.
#[derive(Debug)]
struct StreamState {
    /// All record bytes for this stream, in append order.
    ///
    /// Shared across notional segments — the segment boundary is
    /// purely logical. `SegmentMeta::size_bytes` sums to `bytes.len()`.
    bytes: Vec<u8>,
    /// Per-record starting byte offsets within `bytes`. Index == offset.
    record_starts: Vec<u64>,
    /// Notional segment manifest (ordered by `segment_num`).
    segments: Vec<SegmentMeta>,
    /// Active (writable) segment number.
    active_segment: u32,
}

impl StreamState {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            record_starts: Vec::new(),
            segments: vec![SegmentMeta {
                segment_num: 0,
                first_offset: 0,
                next_offset: 0,
                size_bytes: 0,
            }],
            active_segment: 0,
        }
    }

    fn active_mut(&mut self) -> &mut SegmentMeta {
        let active = self.active_segment;
        self.segments
            .iter_mut()
            .find(|s| s.segment_num == active)
            .expect("active segment must exist in manifest")
    }

    /// Rotates to a new notional segment. Mirrors `SegmentManifest::rotate`
    /// in the on-disk impl.
    fn rotate(&mut self, first_offset: u64) {
        let new_num = self.active_segment + 1;
        self.segments.push(SegmentMeta {
            segment_num: new_num,
            first_offset,
            next_offset: first_offset,
            size_bytes: 0,
        });
        self.active_segment = new_num;
    }
}

/// Pure in-memory storage backend.
///
/// See module docs for the detailed semantic contract. Safe to share
/// across threads as `Send + Sync`; internal state is behind `&mut self`
/// so callers wrap it in their own synchronisation (as
/// `KimberliteInner` does via its outer `RwLock`).
#[derive(Debug)]
pub struct MemoryStorage {
    streams: HashMap<StreamId, StreamState>,
    max_segment_size: u64,
}

impl MemoryStorage {
    /// Creates a new empty in-memory storage with default segment size.
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
            max_segment_size: DEFAULT_MAX_SEGMENT_SIZE,
        }
    }

    /// Creates a new empty in-memory storage with a custom notional
    /// segment size. Intended for tests that want to exercise the
    /// rotation bookkeeping without writing 256 MB of data.
    pub fn with_max_segment_size(max_segment_size: u64) -> Self {
        Self {
            streams: HashMap::new(),
            max_segment_size,
        }
    }

    /// Returns the configured notional segment size in bytes.
    pub fn max_segment_size(&self) -> u64 {
        self.max_segment_size
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for MemoryStorage {
    fn append_batch(
        &mut self,
        stream_id: StreamId,
        events: Vec<Bytes>,
        expected_offset: Offset,
        prev_hash: Option<ChainHash>,
        _fsync: bool,
    ) -> Result<(Offset, ChainHash), StorageError> {
        // Precondition: batch must not be empty. Matches `Storage`.
        assert!(!events.is_empty(), "cannot append empty batch");

        // `or_insert_with` — StreamState deliberately does not impl
        // Default (a default Vec-of-segments is empty, which would
        // break the `active_mut()` invariant).
        let stream = self
            .streams
            .entry(stream_id)
            .or_insert_with(StreamState::new);

        let mut current_offset = expected_offset;
        let mut current_hash = prev_hash;

        for event in events {
            // Pure `Record` with no compression — hash is computed over
            // the original payload, matching `Storage::append_batch`.
            let hash_record = Record::new(current_offset, current_hash, event.clone());
            let record_hash = hash_record.compute_hash();

            let stored_record = Record::with_compression(
                current_offset,
                current_hash,
                RecordKind::Data,
                CompressionKind::None,
                event,
            );
            let bytes = stored_record.to_bytes();

            stream.record_starts.push(stream.bytes.len() as u64);
            stream.bytes.extend_from_slice(&bytes);

            {
                let active = stream.active_mut();
                active.next_offset = current_offset.as_u64() + 1;
                active.size_bytes += bytes.len() as u64;
            }

            current_hash = Some(record_hash);
            current_offset += Offset::from(1u64);
        }

        // Rotate if the active segment exceeded the notional limit.
        // Matches the on-disk impl's post-batch rotation check.
        let should_rotate = stream.active_mut().size_bytes >= self.max_segment_size;
        if should_rotate {
            stream.rotate(current_offset.as_u64());
        }

        // Postcondition: offset must only advance forward.
        assert!(
            current_offset.as_u64() >= expected_offset.as_u64(),
            "offset must only advance forward after append_batch"
        );

        Ok((
            current_offset,
            current_hash.expect("batch was non-empty, hash must be set"),
        ))
    }

    fn read_from(
        &mut self,
        stream_id: StreamId,
        from_offset: Offset,
        max_bytes: u64,
    ) -> Result<Vec<Bytes>, StorageError> {
        let Some(stream) = self.streams.get(&stream_id) else {
            return Ok(Vec::new());
        };

        let mut results = Vec::new();
        let mut bytes_read: u64 = 0;
        let mut expected_prev_hash: Option<ChainHash> = None;

        // Linear walk through the record buffer, verifying the full
        // hash chain (genesis-to-from_offset). Equivalent to the
        // on-disk `read_records_from_genesis` code path modulo
        // checkpoint skipping, which `MemoryStorage` doesn't emit.
        let buf = Bytes::copy_from_slice(&stream.bytes);
        let mut pos = 0usize;
        while pos < buf.len() && bytes_read < max_bytes {
            let (record, consumed) = Record::from_bytes(&buf.slice(pos..))?;

            if record.prev_hash() != expected_prev_hash {
                return Err(StorageError::ChainVerificationFailed {
                    offset: record.offset(),
                    expected: expected_prev_hash,
                    actual: record.prev_hash(),
                });
            }

            expected_prev_hash = Some(record.compute_hash());
            pos += consumed;

            // `MemoryStorage` never emits checkpoint records, but the
            // skip is cheap and keeps us honest if the record
            // serialiser ever changes.
            if record.offset() < from_offset || record.is_checkpoint() {
                continue;
            }

            bytes_read += record.payload().len() as u64;
            results.push(record.payload().clone());
        }

        Ok(results)
    }

    fn latest_chain_hash(
        &mut self,
        stream_id: StreamId,
    ) -> Result<Option<ChainHash>, StorageError> {
        let Some(stream) = self.streams.get(&stream_id) else {
            return Ok(None);
        };

        if stream.bytes.is_empty() {
            return Ok(None);
        }

        let buf = Bytes::copy_from_slice(&stream.bytes);
        let mut pos = 0usize;
        let mut last_hash: Option<ChainHash> = None;
        while pos < buf.len() {
            let (record, consumed) = Record::from_bytes(&buf.slice(pos..))?;
            last_hash = Some(record.compute_hash());
            pos += consumed;
        }
        Ok(last_hash)
    }

    fn segment_count(&self, stream_id: StreamId) -> usize {
        self.streams.get(&stream_id).map_or(0, |s| s.segments.len())
    }

    fn completed_segments(&self, stream_id: StreamId) -> Vec<u32> {
        self.streams.get(&stream_id).map_or_else(Vec::new, |s| {
            s.segments
                .iter()
                .filter(|seg| seg.segment_num != s.active_segment)
                .map(|seg| seg.segment_num)
                .collect()
        })
    }

    fn flush_indexes(&mut self) -> Result<(), StorageError> {
        // No-op. In-memory state is always "flushed" in the sense that
        // a subsequent `read_from` will observe it. No index file to
        // compact.
        Ok(())
    }

    #[cfg(feature = "fuzz-reset")]
    fn reset(&mut self) -> Result<(), StorageError> {
        self.streams.clear();
        Ok(())
    }
}
