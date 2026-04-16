# Dynamic Partial Order Reduction (DPOR) in kimberlite-sim

**Status:** Core module landed 2026-04-16; VOPR integration pending
**Module:** `crates/kimberlite-sim/src/dpor.rs`
**Inspired by:** cuddly-duddly's `cuddly-scheduler`, Flanagan & Godefroid POPL 2005

## Context

kimberlite-sim uses **seed-based random fuzzing** — every run is driven by a
u64 seed that deterministically shapes network delays, storage faults, and
Byzantine mutations. This is fast (85k–167k sims/sec) and effective for coverage
breadth, but it's incomplete: a specific 4-event interleaving bug might occur
with probability 1/10⁶ under uniform random, meaning you could burn 10M seeds
and still miss it.

**DPOR (Dynamic Partial Order Reduction)** systematically explores
*equivalence classes* of interleavings. Two interleavings that differ only in
the order of causally independent events produce the same final state, so we
only need to explore one representative from each equivalence class. This
converts an intractable exponential search into a manageable polynomial one for
many realistic scenarios.

## Dependency Model

Events are represented by `EventKey`, a stable dependency-relevant extraction
from kimberlite-sim's existing `EventKind`:

```rust
pub enum EventKey {
    ToReplica { replica: u8, message_id: u64 },
    Timer { replica: u8, kind: u8 },
    Tick { replica: u8 },
    Crash { replica: u8 },
    Recover { replica: u8 },
    StorageComplete { operation_id: u64 },
    NetworkPartition { partition_id: u64 },
    NetworkHeal { partition_id: u64 },
    WorkloadTick,
    StorageFsync,
    Opaque(u64),
}
```

**Two events are dependent if any of:**

1. Both affect the same replica. (State conflict.)
2. One is `StorageFsync`, the other is `StorageComplete`. (Durability serialization.)
3. Both are `StorageComplete` with the same `operation_id`.
4. They form a `NetworkPartition` / `NetworkHeal` pair with matching partition ID.
5. Both are `Opaque(_)` — conservative fallback.

**Independent (commute freely):**

- Events on different replicas with no fault involvement.
- `WorkloadTick` with anything (generator-side only; affects no replica state).
- Storage completions for unrelated operation IDs.

Rationale: Kimberlite's FCIS design means replica state is private. A message
delivered to replica 0 does not affect replica 1's state transition, so the two
can be ordered either way without observable difference. Crashes and partitions
are the exception — they affect multiple replicas' causal histories.

## Execution Trace

```rust
pub struct ExecutionTrace {
    pub steps: Vec<(EventKey, EventId)>,
}
```

A trace captures the order events were processed during a simulation run. Two
traces are in the same Mazurkiewicz equivalence class iff their
`signature()` (hash of the EventKey sequence) is identical.

## DPOR Explorer

```rust
pub struct DporExplorer { ... }
impl DporExplorer {
    pub fn new(baseline: ExecutionTrace, max_alternatives: usize) -> Self;
    pub fn next_alternative(&mut self) -> Option<ExecutionTrace>;
    pub fn stats(&self) -> &DporStats;
}
```

The explorer implements a stateless DPOR variant:

1. Take the baseline trace (captured from a fuzzing run).
2. For every adjacent pair `(trace[i], trace[i+1])` where
   `!are_dependent(keys[i], keys[i+1])`, record a backtrack position.
3. On each `next_alternative()` call, pop a backtrack position and return a
   trace with those two events swapped.
4. Deduplicate by signature — two swaps that produce the same canonical
   ordering only count once.

This finds all O(n²) adjacent-swap equivalence classes cheaply. Full
unfolding-based DPOR (UDPOR, POPL 2014) is a future enhancement; the adjacent-
swap variant already gives significant coverage improvement.

## Schedule-Driven Replay (planned)

```rust
pub struct DporSchedule {
    order: Vec<EventId>,
    position_of: HashMap<EventId, usize>,
    cursor: usize,
}
```

To replay an alternative trace, VOPR pops events from the event queue and
reorders them to match the schedule. A follow-up commit will integrate this
into the core event loop so that `vopr run --dpor` drives systematic exploration.

## Throughput Expectations

- **Existing fuzzing:** 85k–167k sims/sec, seed-based.
- **DPOR alternatives:** 1k–10k per seconds (fewer because each alternative
  replays a full simulation).
- **Hybrid strategy:** Fast fuzz to find interesting seeds, then DPOR-explore
  the local neighborhood. Expect ≈10× more unique coverage per CPU-hour than
  pure fuzzing.

## Integration Plan

- [x] Land dependency model and explorer (`crates/kimberlite-sim/src/dpor.rs`).
- [x] 10 unit tests for dependency relation, explorer invariants, schedule tracking.
- [ ] Trace capture hook: mutate `run_simulation` to record `ExecutionTrace` when
      `--capture-trace` is passed. Use existing `TraceCollector` as the data source.
- [ ] Replay hook: when a `DporSchedule` is set, dequeue events from the event
      queue in schedule order rather than time order.
- [ ] CLI: `just vopr-dpor --scenario view_change_safety` that runs fuzzing to find
      a baseline, then DPOR on that baseline up to `--max-alternatives`.
- [ ] Coverage integration: feed `equivalence_classes` to the existing
      `CoverageTracker` in `coverage_fuzzer.rs`.

## Why Not Port cuddly-duddly's Full DPOR?

cuddly-duddly's DPOR operates at the *kernel C source level* via `DPOR_YIELD(id)`
macros and an out-of-tree kernel module. The scheduling points are inside real
kernel code (e.g., `net/sched/sch_hfsc.c`) and the host controls guest threads
via KVM hypercalls. This gives it the ability to discover kernel concurrency
bugs in real Linux — not applicable to Rust userspace.

For kimberlite's pure Rust + FCIS design, event-level DPOR in the simulator is
the right granularity. We don't need thread-level scheduling because the kernel
is already pure (no threads, no shared state). The "concurrency" that DPOR
explores is the interleaving of messages, storage operations, and faults across
replicas — exactly what kimberlite-sim's event queue already models.

## References

- Flanagan & Godefroid, "Dynamic Partial-Order Reduction for Model Checking
  Software," POPL 2005.
- Abdulla et al., "Optimal Dynamic Partial Order Reduction," POPL 2014 (UDPOR).
- "DPOR-DS: Dynamic Partial Order Reduction in Distributed Systems," EPFL (2017).
- cuddly-duddly `cuddly-scheduler/src/dpor.rs` — kernel-level DPOR for
  KernelCTF concurrency races.
