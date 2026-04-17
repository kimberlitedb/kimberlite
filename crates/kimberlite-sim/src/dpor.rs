//! Dynamic Partial Order Reduction (DPOR) for systematic state space exploration.
//!
//! DPOR complements seed-based fuzzing by systematically exploring alternative
//! interleavings of concurrent events rather than relying on random discovery.
//!
//! # Design
//!
//! kimberlite-sim's event queue is time-ordered with insertion-order tiebreaking.
//! Concurrency manifests when multiple events are scheduled for the same time, or
//! when independent events from different nodes could be reordered.
//!
//! DPOR tracks causal dependencies between events and explores only the
//! equivalence classes of interleavings — events that commute produce the same
//! final state, so we only need to explore one representative per class.
//!
//! # Dependency Model
//!
//! Two events are **dependent** if:
//! 1. Both target the same replica (state access conflict).
//! 2. One is a fault (crash/partition) affecting the other's delivery.
//! 3. Both are storage ops with overlapping offset ranges on the same node.
//!
//! Two events are **independent** if they target different replicas and neither
//! is a fault event. Independent events can be reordered without affecting the
//! final outcome.
//!
//! # Integration
//!
//! DPOR runs as a meta-scheduler: it drives multiple simulation runs, each
//! exploring a different interleaving. The base simulation stays unchanged; DPOR
//! works by pre-computing a permutation of equivalent events and feeding it to
//! the simulation via [`DporSchedule`].
//!
//! # References
//!
//! - Flanagan & Godefroid, "Dynamic Partial-Order Reduction for Model Checking
//!   Software" (POPL 2005)
//! - Abdulla et al., "Optimal Dynamic Partial Order Reduction" (POPL 2014)

use std::collections::{HashMap, HashSet};

use crate::event::{Event, EventId, EventKind};

// ============================================================================
// Event Key — stable identifier for dependency tracking
// ============================================================================

/// A stable, dependency-relevant identifier for an event.
///
/// Ignores non-essential fields (like `message_bytes` contents) so that two
/// events with different payloads but the same causal role are tracked together.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventKey {
    /// A message targeted at a specific replica.
    ToReplica { replica: u8, message_id: u64 },
    /// A timer firing on a specific replica.
    Timer { replica: u8, kind: u8 },
    /// A tick on a specific replica.
    Tick { replica: u8 },
    /// A crash on a specific replica — conflicts with all events on that replica.
    Crash { replica: u8 },
    /// A recovery on a specific replica — conflicts with all events on that replica.
    Recover { replica: u8 },
    /// A storage completion for a specific operation.
    StorageComplete { operation_id: u64 },
    /// A network partition affecting specific replicas.
    NetworkPartition { partition_id: u64 },
    /// A network heal affecting specific replicas.
    NetworkHeal { partition_id: u64 },
    /// A workload tick (affects no specific replica — independent of all).
    WorkloadTick,
    /// A storage fsync (node-wide, serializes with all storage ops on that node).
    StorageFsync,
    /// Fallback for event kinds we do not classify.
    Opaque(u64),
}

impl EventKey {
    /// Extracts a stable key from an event.
    #[must_use]
    pub fn from_event(event: &Event) -> Self {
        match &event.kind {
            EventKind::VsrMessage { to_replica, .. } => Self::ToReplica {
                replica: *to_replica,
                message_id: event.id.as_raw(),
            },
            EventKind::VsrClientRequest { replica_id, .. } => Self::ToReplica {
                replica: *replica_id,
                message_id: event.id.as_raw(),
            },
            EventKind::VsrTimeout {
                replica_id,
                timeout_kind,
            } => Self::Timer {
                replica: *replica_id,
                kind: *timeout_kind,
            },
            EventKind::VsrTick { replica_id } => Self::Tick {
                replica: *replica_id,
            },
            EventKind::VsrCrash { replica_id } => Self::Crash {
                replica: *replica_id,
            },
            EventKind::VsrRecover { replica_id } => Self::Recover {
                replica: *replica_id,
            },
            EventKind::NetworkDeliver {
                to, message_id, ..
            } => Self::ToReplica {
                replica: u8::try_from(*to).unwrap_or(u8::MAX),
                message_id: *message_id,
            },
            EventKind::StorageComplete { operation_id, .. } => Self::StorageComplete {
                operation_id: *operation_id,
            },
            EventKind::NodeCrash { node_id } => Self::Crash {
                replica: u8::try_from(*node_id).unwrap_or(u8::MAX),
            },
            EventKind::NodeRestart { node_id } => Self::Recover {
                replica: u8::try_from(*node_id).unwrap_or(u8::MAX),
            },
            EventKind::NetworkPartition { partition_id } => Self::NetworkPartition {
                partition_id: *partition_id,
            },
            EventKind::NetworkHeal { partition_id } => Self::NetworkHeal {
                partition_id: *partition_id,
            },
            EventKind::WorkloadTick => Self::WorkloadTick,
            EventKind::StorageFsync => Self::StorageFsync,
            EventKind::Custom(code) => Self::Opaque(*code),
            _ => Self::Opaque(u64::MAX),
        }
    }

    /// Returns the replica affected by this event, if any.
    #[must_use]
    pub fn affected_replica(&self) -> Option<u8> {
        match self {
            Self::ToReplica { replica, .. }
            | Self::Timer { replica, .. }
            | Self::Tick { replica }
            | Self::Crash { replica }
            | Self::Recover { replica } => Some(*replica),
            _ => None,
        }
    }
}

// ============================================================================
// Dependency relation
// ============================================================================

/// Determines whether two events are causally dependent (must be ordered).
///
/// Independent events can be freely reordered without changing observable state.
#[must_use]
pub fn are_dependent(a: &EventKey, b: &EventKey) -> bool {
    use EventKey::{
        Crash, NetworkHeal, NetworkPartition, Recover, StorageComplete, StorageFsync,
        ToReplica, Tick, Timer, WorkloadTick,
    };

    // A fault on replica R conflicts with every other event on replica R.
    if let (Some(ra), Some(rb)) = (a.affected_replica(), b.affected_replica()) {
        if ra == rb {
            return true;
        }
    }

    match (a, b) {
        // Storage fsync serializes all storage completions (same node semantics).
        (StorageFsync, StorageComplete { .. }) | (StorageComplete { .. }, StorageFsync) => true,

        // Two storage completions with different operation IDs are independent.
        (StorageComplete { operation_id: o1 }, StorageComplete { operation_id: o2 }) => {
            o1 == o2
        }

        // Network partition and heal on the same partition conflict.
        (
            NetworkPartition { partition_id: p1 },
            NetworkHeal { partition_id: p2 },
        )
        | (
            NetworkHeal { partition_id: p1 },
            NetworkPartition { partition_id: p2 },
        ) => p1 == p2,

        // WorkloadTick is independent of everything (generator-side only).
        (WorkloadTick, _) | (_, WorkloadTick) => false,

        // Events on different replicas with no fault involvement are independent.
        (ToReplica { replica: r1, .. }, ToReplica { replica: r2, .. }) => r1 == r2,
        (Timer { replica: r1, .. }, Timer { replica: r2, .. }) => r1 == r2,
        (Tick { replica: r1 }, Tick { replica: r2 }) => r1 == r2,
        (Crash { replica: r1 }, Crash { replica: r2 }) => r1 == r2,
        (Recover { replica: r1 }, Recover { replica: r2 }) => r1 == r2,

        // Opaque events — conservative: treat as dependent.
        (EventKey::Opaque(_), _) | (_, EventKey::Opaque(_)) => true,

        _ => false,
    }
}

// ============================================================================
// Execution trace — a sequence of (event_key, event_id) pairs
// ============================================================================

/// A recorded execution trace, capturing the order in which events were
/// processed and their dependency keys.
#[derive(Debug, Clone, Default)]
pub struct ExecutionTrace {
    /// Ordered list of events that were processed.
    pub steps: Vec<(EventKey, EventId)>,
}

impl ExecutionTrace {
    /// Creates an empty trace.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an event to the trace.
    pub fn push(&mut self, key: EventKey, id: EventId) {
        self.steps.push((key, id));
    }

    /// Returns the number of events in this trace.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Returns true if the trace is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Computes a canonical signature for this trace.
    ///
    /// Two traces with identical signatures are in the same Mazurkiewicz
    /// equivalence class — they produce the same final state.
    #[must_use]
    pub fn signature(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        for (key, _) in &self.steps {
            key.hash(&mut hasher);
        }
        hasher.finish()
    }
}

// ============================================================================
// DPOR Explorer — orchestrates alternative interleaving exploration
// ============================================================================

/// Explores the state space of a scenario by systematically swapping adjacent
/// independent events in the execution trace.
///
/// This is a stateless DPOR implementation: each iteration produces a
/// permutation of the baseline trace that represents a distinct Mazurkiewicz
/// equivalence class.
#[derive(Debug, Clone)]
pub struct DporExplorer {
    /// Baseline execution trace (captured from a fuzzing run).
    baseline: ExecutionTrace,
    /// Set of trace signatures already explored.
    explored: HashSet<u64>,
    /// Pending backtrack positions to try.
    backtrack_positions: Vec<usize>,
    /// Maximum number of alternative interleavings to explore per baseline.
    max_alternatives: usize,
    /// Statistics
    stats: DporStats,
}

/// DPOR exploration statistics.
#[derive(Debug, Clone, Default)]
pub struct DporStats {
    /// Number of alternative interleavings explored.
    pub alternatives_explored: u64,
    /// Number of alternatives skipped (already in explored set).
    pub duplicates_skipped: u64,
    /// Number of dependency checks performed.
    pub dependency_checks: u64,
    /// Number of equivalence classes discovered.
    pub equivalence_classes: u64,
}

impl DporExplorer {
    /// Creates a new DPOR explorer from a baseline trace.
    #[must_use]
    pub fn new(baseline: ExecutionTrace, max_alternatives: usize) -> Self {
        let mut explored = HashSet::new();
        explored.insert(baseline.signature());

        // Identify positions where swapping adjacent events might produce a new
        // interleaving (i.e., the pair is independent).
        let mut backtrack_positions = Vec::new();
        for i in 0..baseline.steps.len().saturating_sub(1) {
            let (key_a, _) = &baseline.steps[i];
            let (key_b, _) = &baseline.steps[i + 1];
            if !are_dependent(key_a, key_b) {
                backtrack_positions.push(i);
            }
        }

        Self {
            baseline,
            explored,
            backtrack_positions,
            max_alternatives,
            stats: DporStats {
                equivalence_classes: 1,
                ..Default::default()
            },
        }
    }

    /// Returns the next alternative trace to explore, or None if exhausted.
    pub fn next_alternative(&mut self) -> Option<ExecutionTrace> {
        while self.stats.alternatives_explored < self.max_alternatives as u64 {
            let pos = self.backtrack_positions.pop()?;

            self.stats.dependency_checks += 1;

            // Construct the swapped trace.
            let mut alt = self.baseline.clone();
            if pos + 1 >= alt.steps.len() {
                continue;
            }
            alt.steps.swap(pos, pos + 1);

            let sig = alt.signature();
            if self.explored.insert(sig) {
                self.stats.alternatives_explored += 1;
                self.stats.equivalence_classes += 1;
                return Some(alt);
            }
            self.stats.duplicates_skipped += 1;
        }
        None
    }

    /// Returns exploration statistics.
    #[must_use]
    pub fn stats(&self) -> &DporStats {
        &self.stats
    }

    /// Returns the number of equivalence classes explored so far.
    #[must_use]
    pub fn equivalence_classes(&self) -> u64 {
        self.stats.equivalence_classes
    }

    /// Returns a reference to the baseline trace.
    #[must_use]
    pub fn baseline(&self) -> &ExecutionTrace {
        &self.baseline
    }
}

// ============================================================================
// Schedule — a pre-computed event ordering for DPOR replay
// ============================================================================

/// A pre-computed sequence of event IDs to process in order.
///
/// When the simulation runs with a [`DporSchedule`], it pops events from the
/// event queue but defers execution until the scheduled order matches. This
/// lets DPOR force alternative interleavings without modifying the simulation
/// harness.
#[derive(Debug, Clone)]
pub struct DporSchedule {
    order: Vec<EventId>,
    position_of: HashMap<EventId, usize>,
    cursor: usize,
}

impl DporSchedule {
    /// Creates a schedule from an execution trace.
    #[must_use]
    pub fn from_trace(trace: &ExecutionTrace) -> Self {
        let order: Vec<EventId> = trace.steps.iter().map(|(_, id)| *id).collect();
        let position_of = order.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        Self {
            order,
            position_of,
            cursor: 0,
        }
    }

    /// Returns the next event ID that should execute, or None if the schedule
    /// is exhausted.
    #[must_use]
    pub fn next_expected(&self) -> Option<EventId> {
        self.order.get(self.cursor).copied()
    }

    /// Advances the cursor after the expected event executes.
    pub fn advance(&mut self) {
        self.cursor += 1;
    }

    /// Returns the scheduled position of an event, if any.
    #[must_use]
    pub fn position(&self, id: EventId) -> Option<usize> {
        self.position_of.get(&id).copied()
    }

    /// Returns the remaining schedule length.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.order.len().saturating_sub(self.cursor)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct an Event via the public EventQueue API (all fields private).
    fn mk_event(kind: EventKind) -> Event {
        let mut q = crate::event::EventQueue::new();
        let _ = q.schedule(0, kind);
        q.pop().unwrap()
    }

    fn key_for(kind: EventKind) -> EventKey {
        EventKey::from_event(&mk_event(kind))
    }

    #[test]
    fn events_on_different_replicas_are_independent() {
        let a = key_for(EventKind::VsrTick { replica_id: 0 });
        let b = key_for(EventKind::VsrTick { replica_id: 1 });
        assert!(!are_dependent(&a, &b));
    }

    #[test]
    fn events_on_same_replica_are_dependent() {
        let a = key_for(EventKind::VsrTick { replica_id: 0 });
        let b = key_for(EventKind::VsrTimeout {
            replica_id: 0,
            timeout_kind: 0,
        });
        assert!(are_dependent(&a, &b));
    }

    #[test]
    fn crash_conflicts_with_all_events_on_same_replica() {
        let crash = key_for(EventKind::VsrCrash { replica_id: 0 });
        let tick = key_for(EventKind::VsrTick { replica_id: 0 });
        let message = key_for(EventKind::VsrMessage {
            to_replica: 0,
            message_bytes: vec![],
        });
        assert!(are_dependent(&crash, &tick));
        assert!(are_dependent(&crash, &message));
    }

    #[test]
    fn crash_independent_of_other_replica() {
        let crash = key_for(EventKind::VsrCrash { replica_id: 0 });
        let other_tick = key_for(EventKind::VsrTick { replica_id: 1 });
        assert!(!are_dependent(&crash, &other_tick));
    }

    #[test]
    fn workload_tick_is_independent_of_everything() {
        let wt = key_for(EventKind::WorkloadTick);
        let tick = key_for(EventKind::VsrTick { replica_id: 0 });
        assert!(!are_dependent(&wt, &tick));
    }

    #[test]
    fn dpor_explorer_finds_independent_swap() {
        let mut trace = ExecutionTrace::new();
        trace.push(
            key_for(EventKind::VsrTick { replica_id: 0 }),
            EventId::from_raw(1),
        );
        trace.push(
            key_for(EventKind::VsrTick { replica_id: 1 }),
            EventId::from_raw(2),
        );

        let mut dpor = DporExplorer::new(trace, 10);
        let alt = dpor.next_alternative();
        assert!(alt.is_some(), "DPOR should find the independent swap");
        assert_eq!(dpor.equivalence_classes(), 2);
    }

    #[test]
    fn dpor_explorer_skips_dependent_pairs() {
        let mut trace = ExecutionTrace::new();
        trace.push(
            key_for(EventKind::VsrTick { replica_id: 0 }),
            EventId::from_raw(1),
        );
        trace.push(
            key_for(EventKind::VsrTimeout {
                replica_id: 0,
                timeout_kind: 0,
            }),
            EventId::from_raw(2),
        );

        let mut dpor = DporExplorer::new(trace, 10);
        let alt = dpor.next_alternative();
        assert!(
            alt.is_none(),
            "DPOR must not swap dependent events on the same replica"
        );
    }

    #[test]
    fn dpor_explorer_skips_duplicate_signatures() {
        // Two events with the same key would produce equivalent traces
        // when swapped — DPOR should detect and skip.
        let mut trace = ExecutionTrace::new();
        let k = key_for(EventKind::WorkloadTick);
        trace.push(k.clone(), EventId::from_raw(1));
        trace.push(k.clone(), EventId::from_raw(2));

        let mut dpor = DporExplorer::new(trace, 10);
        // WorkloadTick is independent of itself (same key) — swap produces
        // identical signature — skipped.
        assert!(dpor.next_alternative().is_none());
    }

    #[test]
    fn schedule_tracks_positions() {
        let mut trace = ExecutionTrace::new();
        trace.push(
            key_for(EventKind::VsrTick { replica_id: 0 }),
            EventId::from_raw(10),
        );
        trace.push(
            key_for(EventKind::VsrTick { replica_id: 1 }),
            EventId::from_raw(20),
        );

        let schedule = DporSchedule::from_trace(&trace);
        assert_eq!(schedule.next_expected(), Some(EventId::from_raw(10)));
        assert_eq!(schedule.position(EventId::from_raw(20)), Some(1));
        assert_eq!(schedule.remaining(), 2);
    }

    #[test]
    fn schedule_advances_cursor() {
        let mut trace = ExecutionTrace::new();
        trace.push(
            key_for(EventKind::VsrTick { replica_id: 0 }),
            EventId::from_raw(10),
        );
        trace.push(
            key_for(EventKind::VsrTick { replica_id: 1 }),
            EventId::from_raw(20),
        );

        let mut schedule = DporSchedule::from_trace(&trace);
        schedule.advance();
        assert_eq!(schedule.next_expected(), Some(EventId::from_raw(20)));
        assert_eq!(schedule.remaining(), 1);

        schedule.advance();
        assert_eq!(schedule.next_expected(), None);
        assert_eq!(schedule.remaining(), 0);
    }
}
