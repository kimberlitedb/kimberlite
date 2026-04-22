//! Abstract storage backend trait.
//!
//! The [`StorageBackend`] trait extracts the pure I/O surface that
//! [`crate::Storage`] exposes to its consumers — the kernel's effect
//! executor in `kimberlite::KimberliteInner`, primarily. It exists so
//! that the same `Kimberlite` facade can run on-disk (production,
//! default) or entirely in RAM (tests, ephemeral workers, fuzzers),
//! without the outer API being polluted by backend-specific details.
//!
//! ## What this trait is
//!
//! - A pure I/O surface: append, read, chain-hash recovery, segment
//!   accounting, and a best-effort flush.
//! - Dyn-compatible (object-safe): consumers hold `Box<dyn
//!   StorageBackend>` so they can swap backends at runtime without a
//!   generic parameter leaking into the rest of the codebase.
//!
//! ## What this trait is NOT
//!
//! - The kernel's `kimberlite-kernel::traits::Storage` trait — that
//!   one models a much simpler key/value style API used by the
//!   effect-executor prototype and is separate. Don't confuse them.
//! - A fault-injection surface. `kimberlite-sim::SimStorage` has its
//!   own shape with explicit failure modes (torn writes, latency,
//!   corruption) and is intentionally not a `StorageBackend`.
//!
//! ## Implementations
//!
//! - [`crate::Storage`]: the production on-disk implementation.
//!   Segment files, CRC32, hash chain, checkpoint index.
//! - [`crate::MemoryStorage`]: a pure in-memory backend that preserves
//!   hash-chain determinism (same append sequence → same chain hash).
//!   No fsync, no mmap, no disk. Intended for tests, sandboxes, and
//!   short-lived worker processes.
//!
//! ## FCIS compliance
//!
//! The trait is deliberately narrow — no runtime, no clock, no
//! `Effect` types cross this boundary. It exposes only the
//! read/write/chain-state primitives the imperative shell needs. Pure
//! kernel code never touches it.
//!
//! ## Why `&mut self` everywhere
//!
//! Both the on-disk and in-memory backends maintain mutable caches
//! (offset index, manifest, chain state). `&mut` keeps the trait
//! faithful to that reality; callers already hold the backend behind
//! a `RwLock` in `Kimberlite::submit`, so this isn't a meaningful
//! constraint in practice.
//!
//! ## Stability
//!
//! Public for downstream users to implement their own backends (e.g.
//! test doubles), but the method set may grow between minor versions
//! as more of `Storage`'s surface is pulled behind the trait.
//! Changes will be additive — existing impls won't break.

use bytes::Bytes;
use kimberlite_crypto::ChainHash;
use kimberlite_types::{Offset, StreamId};

use crate::error::StorageError;

/// Abstract storage backend used by `KimberliteInner`'s effect executor.
///
/// See module-level docs for scope and semantics. Implementations must
/// uphold:
///
/// 1. **Append-only**: `append_batch` never overwrites existing records.
/// 2. **Hash-chain determinism**: given the same `(stream_id, events,
///    expected_offset, prev_hash)` sequence, two different backends
///    must produce the same final `ChainHash`. Tests enforce this
///    between [`crate::Storage`] and [`crate::MemoryStorage`].
/// 3. **Offset monotonicity**: the returned offset must equal
///    `expected_offset + events.len()`.
/// 4. **Read visibility**: records become visible to `read_from`
///    immediately after `append_batch` returns.
pub trait StorageBackend: Send + Sync + std::fmt::Debug {
    /// Appends a batch of events to a stream and extends the hash chain.
    ///
    /// Returns `(next_offset, last_record_hash)`. The caller is
    /// responsible for supplying the current chain head; a stale
    /// `prev_hash` produces a permanent chain break that surfaces on
    /// later verified reads. Backends must NOT recover the chain head
    /// internally here — `latest_chain_hash` is the restart recovery
    /// entrypoint.
    ///
    /// `fsync` is advisory: on-disk impls honour it; pure in-memory
    /// impls treat it as a no-op.
    ///
    /// # Panics
    ///
    /// Panics if `events` is empty. Empty batches are a caller bug —
    /// both shipping impls agree on this.
    fn append_batch(
        &mut self,
        stream_id: StreamId,
        events: Vec<Bytes>,
        expected_offset: Offset,
        prev_hash: Option<ChainHash>,
        fsync: bool,
    ) -> Result<(Offset, ChainHash), StorageError>;

    /// Reads events from a stream with checkpoint-optimised chain
    /// verification.
    ///
    /// The returned slice contains decoded payloads, not raw
    /// [`crate::Record`]s. `max_bytes` caps the response size so a
    /// wayward query cannot blow the heap.
    fn read_from(
        &mut self,
        stream_id: StreamId,
        from_offset: Offset,
        max_bytes: u64,
    ) -> Result<Vec<Bytes>, StorageError>;

    /// Returns the chain hash of the last appended record for the
    /// stream, or `None` if the stream has never been written to.
    ///
    /// This is the restart-recovery entrypoint: after a process
    /// restart the in-memory `chain_heads` map in `KimberliteInner` is
    /// empty; this call rebuilds a single entry lazily on demand. See
    /// `KimberliteInner::execute_effects` for the call site.
    fn latest_chain_hash(&mut self, stream_id: StreamId)
    -> Result<Option<ChainHash>, StorageError>;

    /// Number of segments (active + completed) for a stream.
    ///
    /// In-memory backends emulate segment rotation so this matches
    /// the on-disk backend's count for equivalent workloads.
    fn segment_count(&self, stream_id: StreamId) -> usize;

    /// Numbers of the completed (immutable) segments for a stream.
    fn completed_segments(&self, stream_id: StreamId) -> Vec<u32>;

    /// Best-effort flush of any backend-internal buffers.
    ///
    /// On-disk impls fsync index files; in-memory impls are a no-op.
    /// Errors are surfaced so callers can decide whether to fail the
    /// enclosing operation.
    fn flush_indexes(&mut self) -> Result<(), StorageError>;

    /// Wipes all backend state, returning to an empty initial
    /// condition.
    ///
    /// Fuzz-only escape hatch — production code never calls this. The
    /// on-disk backend only exposes a matching method under
    /// `#[cfg(feature = "fuzz-reset")]`, so the trait method is also
    /// gated to prevent production use.
    #[cfg(feature = "fuzz-reset")]
    fn reset(&mut self) -> Result<(), StorageError>;
}
