//! In-memory timestamp → projection-offset index.
//!
//! v0.6.0 Tier 2 #6 — backs the default `FOR SYSTEM_TIME AS OF '<iso>'`
//! / `AS OF TIMESTAMP` resolver. On every DML commit the runtime
//! records a `(projection_offset, commit_ns)` pair here; callers that
//! issue a timestamp-qualified query get a binary-search lookup to the
//! projection offset whose commit timestamp is the greatest value ≤
//! the target.
//!
//! # Where is this the right place?
//!
//! The main append-only log records don't carry wall-clock timestamps
//! (the kernel is deterministic and takes no clock), and the audit log
//! lives on a separate chain that doesn't cover every DML — only
//! compliance-relevant mutations (consent, erasure, breach, …). The
//! projection store, which is what `query_at(offset)` actually
//! snapshots against, is the only place that sees every commit. So
//! the index is maintained here, keyed by *projection* offset
//! (`ProjectionStore::applied_position()` at the moment of commit).
//!
//! # Durability
//!
//! The index is in-memory only; on restart it rebuilds lazily from
//! the subsequent writes. Callers that need historical time-travel
//! across a restart should persist `(log_offset, wall_ns)` alongside
//! their business records and call `QueryEngine::query_at_timestamp`
//! directly — that API still accepts a caller-supplied resolver for
//! exactly this use case. A durable on-disk index is tracked in
//! `ROADMAP.md` under v0.7.0.
//!
//! # Monotonicity
//!
//! Real clocks are not strictly monotonic. The runtime clamps each
//! inserted timestamp to at least `last_ns + 1` before appending,
//! preserving the invariant `positions[i].1 < positions[i+1].1` that
//! binary search relies on.

use kimberlite_query::TimestampResolution;
use kimberlite_types::Offset;

/// In-memory `(projection_offset, commit_ns)` index used by the
/// default timestamp→offset resolver.
///
/// # Invariants
///
/// - `entries` is sorted strictly ascending by both offset and ns.
/// - `entries[i].0 < entries[i+1].0` (projection offsets grow with
///   every commit — the projection store is append-only).
/// - `entries[i].1 < entries[i+1].1` (clamped monotonic timestamps).
///
/// Debug builds verify the second invariant on every `insert`.
#[derive(Debug, Default, Clone)]
pub(crate) struct TimestampIndex {
    entries: Vec<(Offset, i64)>,
}

impl TimestampIndex {
    /// Creates an empty index.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Records a newly-committed `(projection_offset, wall_ns)` pair.
    ///
    /// If `wall_ns` is not strictly greater than the previous entry's
    /// timestamp (clock skew, resolution limits), it is clamped to
    /// `prev_ns + 1`. This keeps the strictly-monotonic invariant
    /// that `resolve` relies on without drifting far from the wall
    /// clock in practice.
    ///
    /// If `offset` is not strictly greater than the previous entry's
    /// offset, the call is a no-op — the runtime already recorded
    /// this commit, typically because two effects in the same
    /// `execute_effects` batch landed at the same projection
    /// position (both `StorageAppend` and `UpdateProjection` for one
    /// DML statement).
    pub(crate) fn insert(&mut self, offset: Offset, wall_ns: i64) {
        let (clamped_ns, accept) = match self.entries.last().copied() {
            Some((prev_off, prev_ns)) => {
                if offset <= prev_off {
                    return;
                }
                let clamped = if wall_ns > prev_ns {
                    wall_ns
                } else {
                    // Saturating guard: u64 upper half of i64 is unreachable
                    // in practice (would be ~year 2262), but keep the math
                    // total so index ingestion never panics.
                    prev_ns.saturating_add(1)
                };
                (clamped, true)
            }
            None => (wall_ns, true),
        };
        if !accept {
            return;
        }
        // Postcondition guard — if it ever fails, the runtime fed us an
        // out-of-order commit and the resolver would silently lie.
        debug_assert!(
            self.entries
                .last()
                .is_none_or(|(off, ns)| offset > *off && clamped_ns > *ns),
            "TimestampIndex invariant broken: inserting ({offset:?}, {clamped_ns}) after \
             last {:?}",
            self.entries.last()
        );
        self.entries.push((offset, clamped_ns));
    }

    /// Resolves a target Unix-nanosecond timestamp to a projection
    /// offset. Returns the variant of `TimestampResolution` that
    /// distinguishes the retention-horizon case from an empty log.
    ///
    /// # Semantics
    ///
    /// - Empty index → `LogEmpty`.
    /// - Target < earliest entry → `BeforeRetentionHorizon { horizon_ns }`.
    /// - Otherwise → `Offset(o)` where `o` is the largest offset
    ///   whose commit timestamp is ≤ `target_ns`.
    pub(crate) fn resolve(&self, target_ns: i64) -> TimestampResolution {
        if self.entries.is_empty() {
            return TimestampResolution::LogEmpty;
        }
        // SAFETY: `is_empty()` guard above — `first()` is Some.
        let earliest_ns = self.entries[0].1;
        if target_ns < earliest_ns {
            return TimestampResolution::BeforeRetentionHorizon {
                horizon_ns: earliest_ns,
            };
        }
        // Binary search for greatest ns ≤ target_ns.
        // `partition_point` returns the first index where predicate is false.
        let idx = self.entries.partition_point(|(_, ns)| *ns <= target_ns);
        // idx is >= 1 because target_ns >= earliest_ns (checked above),
        // so there's always at least one match. Guard just in case.
        debug_assert!(idx >= 1);
        let (offset, _) = self.entries[idx.saturating_sub(1)];
        TimestampResolution::Offset(offset)
    }

    /// Returns the earliest `(offset, ns)` pair, if any. Useful for
    /// introspection and tests.
    #[cfg(test)]
    pub(crate) fn earliest(&self) -> Option<(Offset, i64)> {
        self.entries.first().copied()
    }

    /// Number of entries in the index.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_index_resolves_to_log_empty() {
        let idx = TimestampIndex::new();
        assert_eq!(idx.resolve(1_700_000_000_000_000_000), TimestampResolution::LogEmpty);
    }

    #[test]
    fn target_before_earliest_resolves_to_retention_horizon() {
        let mut idx = TimestampIndex::new();
        idx.insert(Offset::new(1), 1_700_000_000_000_000_000);
        idx.insert(Offset::new(2), 1_700_000_001_000_000_000);
        match idx.resolve(1_699_000_000_000_000_000) {
            TimestampResolution::BeforeRetentionHorizon { horizon_ns } => {
                assert_eq!(horizon_ns, 1_700_000_000_000_000_000);
            }
            other => panic!("expected BeforeRetentionHorizon, got {other:?}"),
        }
    }

    #[test]
    fn exact_match_returns_matching_offset() {
        let mut idx = TimestampIndex::new();
        idx.insert(Offset::new(10), 1_000);
        idx.insert(Offset::new(20), 2_000);
        idx.insert(Offset::new(30), 3_000);
        assert_eq!(idx.resolve(2_000), TimestampResolution::Offset(Offset::new(20)));
    }

    #[test]
    fn between_entries_returns_floor() {
        let mut idx = TimestampIndex::new();
        idx.insert(Offset::new(10), 1_000);
        idx.insert(Offset::new(20), 2_000);
        idx.insert(Offset::new(30), 3_000);
        // Between ns=2000 and ns=3000 → offset 20.
        assert_eq!(idx.resolve(2_500), TimestampResolution::Offset(Offset::new(20)));
    }

    #[test]
    fn future_timestamp_returns_latest_offset() {
        let mut idx = TimestampIndex::new();
        idx.insert(Offset::new(10), 1_000);
        idx.insert(Offset::new(20), 2_000);
        assert_eq!(
            idx.resolve(i64::MAX),
            TimestampResolution::Offset(Offset::new(20))
        );
    }

    #[test]
    fn clock_skew_clamps_to_monotonic() {
        let mut idx = TimestampIndex::new();
        idx.insert(Offset::new(1), 5_000);
        // Earlier wall clock — gets clamped to 5_001 so the sort
        // invariant holds.
        idx.insert(Offset::new(2), 4_000);
        assert_eq!(idx.len(), 2);
        // 4_000 is strictly before the earliest retained timestamp
        // (5_000), so the index honestly reports BeforeRetentionHorizon
        // rather than silently returning offset 1 — callers get a clear
        // error instead of the wrong state.
        assert_eq!(
            idx.resolve(4_000),
            TimestampResolution::BeforeRetentionHorizon { horizon_ns: 5_000 }
        );
        // 5_000 (exactly the earliest stored ts) resolves to offset 1.
        assert_eq!(idx.resolve(5_000), TimestampResolution::Offset(Offset::new(1)));
        // 5_001 (clamped offset 2) resolves to offset 2.
        assert_eq!(idx.resolve(5_001), TimestampResolution::Offset(Offset::new(2)));
    }

    #[test]
    fn duplicate_offset_is_noop() {
        let mut idx = TimestampIndex::new();
        idx.insert(Offset::new(5), 1_000);
        idx.insert(Offset::new(5), 2_000); // same offset → skip
        assert_eq!(idx.len(), 1);
        assert_eq!(idx.earliest(), Some((Offset::new(5), 1_000)));
    }

    #[test]
    fn proptest_resolve_returns_consistent_offset_for_monotonic_inserts() {
        use proptest::prelude::*;
        proptest!(|(count in 1usize..50)| {
            let mut idx = TimestampIndex::new();
            for i in 0..count {
                let off = Offset::new((i as u64) + 1);
                // Spaced 1_000 ns apart so no clamping kicks in.
                let ns = 1_000_000 + (i as i64) * 1_000;
                idx.insert(off, ns);
            }
            // For every inserted timestamp, resolving that exact ns
            // returns its own offset.
            for i in 0..count {
                let ns = 1_000_000 + (i as i64) * 1_000;
                let expected_off = Offset::new((i as u64) + 1);
                prop_assert_eq!(idx.resolve(ns), TimestampResolution::Offset(expected_off));
            }
            // Before the earliest → BeforeRetentionHorizon.
            match idx.resolve(0) {
                TimestampResolution::BeforeRetentionHorizon { horizon_ns } => {
                    prop_assert_eq!(horizon_ns, 1_000_000);
                }
                other => prop_assert!(false, "expected horizon, got {:?}", other),
            }
        });
    }
}
