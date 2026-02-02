# Phase 7 Complete: Projection/MVCC Invariants

## Summary

Phase 7 has been successfully implemented. Kimberlite now has 4 additional invariants that verify the correctness of state machine projection and Multi-Version Concurrency Control (MVCC). These invariants ensure that derived state is consistent with the log and that point-in-time queries return correct snapshots.

---

## Deliverables ✅

### Task #1: Projection/MVCC Invariant Checkers Module ✅

**Module Created**: `/crates/kimberlite-sim/src/projection_invariants.rs` (730+ lines)

**Purpose**: Verify state machine projection correctness and MVCC snapshot isolation

**Invariants Implemented** (4 total):

1. **AppliedPositionMonotonicChecker**: Verifies applied_position never regresses and stays ≤ commit_index
   - **Safety Property 1**: applied_position is monotonically non-decreasing
   - **Safety Property 2**: applied_position ≤ commit_index (projection can't be ahead of log)
   - **Violation Examples**:
     - applied_position goes from 100 → 50 (regression/data loss)
     - applied_position = 200 but commit_index = 150 (applying uncommitted data)
   - **Why it matters**: Projection state must be consistent with the log. Regression means data loss, being ahead means applying uncommitted operations.
   - **Expected to catch**: Unsafe recovery, batch application bugs, wrong log position

2. **MvccVisibilityChecker**: Verifies AS OF POSITION queries see correct snapshots
   - **Safety Property**: Query `AS OF POSITION p` sees exactly the state after applying position p
   - **Violation Example**: At position 5, key "x" = "value5", but query returns "value10" from position 10
   - **Why it matters**: MVCC visibility is critical for point-in-time consistency, compliance guarantees, audit trails
   - **Expected to catch**: Off-by-one errors in MVCC logic, wrong visibility predicates, timestamp bugs

3. **AppliedIndexIntegrityChecker**: Verifies AppliedIndex references real log entries with correct hashes
   - **Safety Property**: When projection claims "applied up to position P with hash H", the log must have entry at P with hash H
   - **Violation Examples**:
     - Projection claims applied_position=100, hash=ABC, but log entry has hash=XYZ
     - No log entry exists at position 100 (dangling reference)
   - **Why it matters**: AppliedIndex is used for crash recovery. Wrong references cause recovery failures or corrupt state.
   - **Expected to catch**: Hash calculation bugs, superblock corruption, log/projection desynchronization

4. **ProjectionCatchupChecker**: Verifies projection catches up to commit_index within bounded steps
   - **Safety Property**: If projection is lagging, it must catch up within max_catchup_steps
   - **Violation Example**: Lag starts at step 1000 (applied=50, commit=100), still lagging at step 12000
   - **Why it matters**: Unbounded lag means queries see arbitrarily stale data, violating compliance requirements
   - **Expected to catch**: Projection application bugs, performance regressions, backpressure issues
   - **Default**: 10,000 simulation steps for catchup

---

### Task #2: API Design ✅

**Core Types**:
```rust
pub struct AppliedPositionMonotonicChecker {
    last_applied: HashMap<String, u64>,
    last_commit: HashMap<String, u64>,
    checks_performed: u64,
}

pub struct MvccVisibilityChecker {
    version_history: HashMap<(String, String), BTreeMap<u64, Option<ChainHash>>>,
    checks_performed: u64,
}

pub struct AppliedIndexIntegrityChecker {
    log_entries: BTreeMap<u64, ChainHash>,
    checks_performed: u64,
}

pub struct ProjectionCatchupChecker {
    lag_started: HashMap<String, (u64, u64, u64)>,
    max_catchup_steps: u64,
    checks_performed: u64,
}
```

**AppliedPosition Monotonic API**:
```rust
impl AppliedPositionMonotonicChecker {
    pub fn new() -> Self;
    pub fn record_applied_position(
        &mut self,
        projection_id: &str,
        applied_position: Offset,
        commit_index: Offset,
    ) -> InvariantResult;
    pub fn checks_performed(&self) -> u64;
    pub fn reset(&mut self);
}
```

**MVCC Visibility API**:
```rust
impl MvccVisibilityChecker {
    pub fn new() -> Self;
    pub fn record_write(
        &mut self,
        table_id: &str,
        key: &str,
        position: Offset,
        value_hash: Option<ChainHash>,
    );
    pub fn check_read_at_position(
        &mut self,
        table_id: &str,
        key: &str,
        position: Offset,
        observed_value_hash: Option<&ChainHash>,
    ) -> InvariantResult;
    pub fn checks_performed(&self) -> u64;
    pub fn reset(&mut self);
}
```

**AppliedIndex Integrity API**:
```rust
impl AppliedIndexIntegrityChecker {
    pub fn new() -> Self;
    pub fn record_log_entry(&mut self, position: Offset, entry_hash: &ChainHash);
    pub fn check_applied_index(
        &mut self,
        applied_position: Offset,
        claimed_hash: &ChainHash,
    ) -> InvariantResult;
    pub fn checks_performed(&self) -> u64;
    pub fn reset(&mut self);
}
```

**Projection Catchup API**:
```rust
impl ProjectionCatchupChecker {
    pub fn new(max_catchup_steps: u64) -> Self;
    pub fn check_catchup(
        &mut self,
        projection_id: &str,
        current_step: u64,
        applied_position: Offset,
        commit_index: Offset,
    ) -> InvariantResult;
    pub fn checks_performed(&self) -> u64;
    pub fn reset(&mut self);
}
```

---

### Task #3: Integration with Existing Infrastructure ✅

**Uses Existing Types**:
- `kimberlite_types::Offset` for log positions
- `kimberlite_crypto::ChainHash` for value hashing
- `InvariantResult` from `invariant.rs`
- `invariant_tracker` for execution tracking

**Invariant Tracking**:
- All 4 checkers call `invariant_tracker::record_invariant_execution()`
- Tracked names:
  - `projection_applied_position_monotonic`
  - `projection_mvcc_visibility`
  - `projection_applied_index_integrity`
  - `projection_catchup`
- Enables coverage reporting in VOPR

**Data Structures**:
- `HashMap` for O(1) lookups (projection IDs, table/key pairs)
- `BTreeMap` for sorted iteration (version history, log entries)
- Efficient range queries for MVCC visibility checks

---

### Task #4: Comprehensive Testing ✅

**Tests Added** (11 total in `projection_invariants.rs`):

1. **test_applied_position_monotonic_ok**
   - Normal progression: 0 → 5 → 10 within bounds
   - Verifies monotonicity works correctly

2. **test_applied_position_regression**
   - applied_position goes from 10 → 5 → VIOLATION
   - Catches regression bugs

3. **test_applied_position_ahead_of_commit**
   - applied=20, commit=10 → VIOLATION
   - Catches projection running ahead of log

4. **test_mvcc_visibility_ok**
   - Write at positions 5 and 10
   - Queries at both positions see correct values
   - Verifies MVCC visibility rules

5. **test_mvcc_visibility_violation**
   - Query at position 5 sees value from position 10 → VIOLATION
   - Catches off-by-one and visibility bugs

6. **test_applied_index_integrity_ok**
   - Log entry at position 100 with hash ABC
   - AppliedIndex claims position 100, hash ABC → OK
   - Normal case

7. **test_applied_index_integrity_hash_mismatch**
   - Log has hash ABC, AppliedIndex claims hash XYZ → VIOLATION
   - Catches hash calculation bugs

8. **test_applied_index_integrity_missing_entry**
   - AppliedIndex references non-existent log entry → VIOLATION
   - Catches dangling references

9. **test_projection_catchup_ok**
   - Projection lags but catches up within 100 steps → OK
   - Verifies normal catchup behavior

10. **test_projection_catchup_violation**
    - Projection lags for >100 steps → VIOLATION
    - Catches unbounded lag

11. **test_all_projection_checkers_track_execution**
    - Verifies all 4 invariants integrate with invariant_tracker
    - Confirms coverage tracking works

**All tests pass** (214/214 in kimberlite-sim, up from 203)

---

## Architecture

```
┌────────────────────────────────────────────────────────────┐
│  Projection Store (kimberlite-store)                       │
│  - apply(batch) at position P                              │
│  - applied_position() returns last applied P               │
│  - get_at(table, key, position) returns MVCC snapshot      │
└────────────────┬───────────────────────────────────────────┘
                 │ Records writes, queries, applied state
                 ▼
┌────────────────────────────────────────────────────────────┐
│  Projection Invariant Checkers (kimberlite-sim)            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ AppliedPositionMonotonicChecker                      │  │
│  │  - record_applied_position(id, applied, commit)     │  │
│  │  - Detects: regression, ahead of commit             │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ MvccVisibilityChecker                                │  │
│  │  - record_write(table, key, position, hash)         │  │
│  │  - check_read_at_position(table, key, pos, hash)    │  │
│  │  - Detects: wrong snapshot visibility               │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ AppliedIndexIntegrityChecker                         │  │
│  │  - record_log_entry(position, hash)                 │  │
│  │  - check_applied_index(position, hash)              │  │
│  │  - Detects: dangling references, hash mismatches    │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ ProjectionCatchupChecker                             │  │
│  │  - check_catchup(id, step, applied, commit)         │  │
│  │  - Detects: unbounded lag (>10k steps)              │  │
│  └──────────────────────────────────────────────────────┘  │
└────────────────┬───────────────────────────────────────────┘
                 │ Reports violations
                 ▼
┌────────────────────────────────────────────────────────────┐
│  VOPR (Simulation Harness)                                 │
│  - Runs projection scenarios                               │
│  - Injects faults (lag, corruption, crashes)               │
│  - Checks invariants after each event                      │
│  - Fails on violation with detailed context                │
└────────────────────────────────────────────────────────────┘
```

---

## Testing & Verification ✅

### Unit Tests (11 new)
- **kimberlite-sim**: 214/214 passing (up from 203)
  - `projection_invariants::tests`: 11 tests covering all 4 checkers
  - Each invariant has multiple test cases (ok, regression, violation)
  - Invariant tracking verified

### Coverage Tracking
All 4 projection invariants integrated with `invariant_tracker`:
```bash
# After running VOPR with projection simulation
invariant_tracker.get_run_count("projection_applied_position_monotonic")  // > 0
invariant_tracker.get_run_count("projection_mvcc_visibility")             // > 0
invariant_tracker.get_run_count("projection_applied_index_integrity")     // > 0
invariant_tracker.get_run_count("projection_catchup")                     // > 0
```

---

## Usage Examples

### AppliedPosition Monotonic Checker

```rust
use kimberlite_sim::{AppliedPositionMonotonicChecker, InvariantResult};
use kimberlite_types::Offset;

let mut checker = AppliedPositionMonotonicChecker::new();

// Normal progression
let result = checker.record_applied_position(
    "proj1",
    Offset::new(0),
    Offset::new(10),
);
assert!(matches!(result, InvariantResult::Ok));

// Applied advances
let result = checker.record_applied_position(
    "proj1",
    Offset::new(5),
    Offset::new(10),
);
assert!(matches!(result, InvariantResult::Ok));

// Regression - VIOLATION
let result = checker.record_applied_position(
    "proj1",
    Offset::new(3),
    Offset::new(10),
);
assert!(matches!(result, InvariantResult::Violated { .. }));

// Ahead of commit - VIOLATION
let result = checker.record_applied_position(
    "proj1",
    Offset::new(20),
    Offset::new(10),
);
assert!(matches!(result, InvariantResult::Violated { .. }));
```

### MVCC Visibility Checker

```rust
use kimberlite_sim::MvccVisibilityChecker;
use kimberlite_crypto::ChainHash;

let mut checker = MvccVisibilityChecker::new();

// Write "value5" at position 5
let hash5 = /* compute hash of "value5" */;
checker.record_write("users", "user123", Offset::new(5), Some(hash5));

// Write "value10" at position 10
let hash10 = /* compute hash of "value10" */;
checker.record_write("users", "user123", Offset::new(10), Some(hash10));

// Query AS OF POSITION 5 - should see hash5
let result = checker.check_read_at_position(
    "users",
    "user123",
    Offset::new(5),
    Some(&hash5),
);
assert!(matches!(result, InvariantResult::Ok));

// Query AS OF POSITION 5 but see hash10 - VIOLATION
let result = checker.check_read_at_position(
    "users",
    "user123",
    Offset::new(5),
    Some(&hash10),
);
assert!(matches!(result, InvariantResult::Violated { .. }));
```

### AppliedIndex Integrity Checker

```rust
use kimberlite_sim::AppliedIndexIntegrityChecker;

let mut checker = AppliedIndexIntegrityChecker::new();

// Record log entry at position 100
let hash = /* compute hash of log entry */;
checker.record_log_entry(Offset::new(100), &hash);

// Projection claims applied to position 100 with correct hash - OK
let result = checker.check_applied_index(Offset::new(100), &hash);
assert!(matches!(result, InvariantResult::Ok));

// Wrong hash - VIOLATION
let wrong_hash = /* different hash */;
let result = checker.check_applied_index(Offset::new(100), &wrong_hash);
assert!(matches!(result, InvariantResult::Violated { .. }));

// Non-existent entry - VIOLATION
let result = checker.check_applied_index(Offset::new(200), &hash);
assert!(matches!(result, InvariantResult::Violated { .. }));
```

### Projection Catchup Checker

```rust
use kimberlite_sim::ProjectionCatchupChecker;

let mut checker = ProjectionCatchupChecker::new(100); // Max 100 steps

// Projection starts lagging at step 1000
let result = checker.check_catchup(
    "proj1",
    1000,
    Offset::new(10),
    Offset::new(20),
);
assert!(matches!(result, InvariantResult::Ok));

// Still lagging at step 1050 - OK (within 100 steps)
let result = checker.check_catchup(
    "proj1",
    1050,
    Offset::new(15),
    Offset::new(25),
);
assert!(matches!(result, InvariantResult::Ok));

// Still lagging at step 1101 - VIOLATION (>100 steps)
let result = checker.check_catchup(
    "proj1",
    1101,
    Offset::new(16),
    Offset::new(30),
);
assert!(matches!(result, InvariantResult::Violated { .. }));

// Caught up - resets lag tracking
let result = checker.check_catchup(
    "proj1",
    1110,
    Offset::new(30),
    Offset::new(30),
);
assert!(matches!(result, InvariantResult::Ok));
```

---

## Integration with VOPR (Future)

When projection simulation is added to VOPR:

```rust
// In VOPR simulation loop (future integration)
let mut applied_pos = AppliedPositionMonotonicChecker::new();
let mut mvcc = MvccVisibilityChecker::new();
let mut applied_index = AppliedIndexIntegrityChecker::new();
let mut catchup = ProjectionCatchupChecker::new(10_000);

// After each projection event
match event {
    ProjectionEvent::BatchApplied { projection_id, position, commit_index } => {
        // Check applied position is monotonic and ≤ commit
        if let InvariantResult::Violated { .. } =
            applied_pos.record_applied_position(&projection_id, position, commit_index) {
            return Err(SimError::InvariantViolation(/* ... */));
        }

        // Track for catchup checking
        if let InvariantResult::Violated { .. } =
            catchup.check_catchup(&projection_id, current_step, position, commit_index) {
            return Err(SimError::InvariantViolation(/* ... */));
        }
    }

    ProjectionEvent::Write { table, key, position, value_hash } => {
        // Record write for MVCC tracking
        mvcc.record_write(&table, &key, position, value_hash);
    }

    ProjectionEvent::Query { table, key, position, observed_hash } => {
        // Check MVCC visibility
        if let InvariantResult::Violated { .. } =
            mvcc.check_read_at_position(&table, &key, position, observed_hash) {
            return Err(SimError::InvariantViolation(/* ... */));
        }
    }

    ProjectionEvent::AppliedIndexUpdated { position, hash } => {
        // Check applied index integrity
        if let InvariantResult::Violated { .. } =
            applied_index.check_applied_index(position, &hash) {
            return Err(SimError::InvariantViolation(/* ... */));
        }
    }

    ProjectionEvent::LogEntryWritten { position, hash } => {
        // Record for applied index verification
        applied_index.record_log_entry(position, &hash);
    }

    _ => {}
}
```

---

## Key Design Decisions

### 1. String-Based Projection IDs
- **Decision**: Use `&str` for projection_id, not typed ID
- **Why**: More flexible - supports multiple projection types, tenant-specific projections
- **Alternative**: Typed `ProjectionId` enum (less flexible)
- **Benefit**: Works with any projection naming scheme

### 2. BTreeMap for Version History
- **Decision**: `BTreeMap<u64, Option<ChainHash>>` for MVCC versions
- **Why**: Sorted iteration, efficient range queries for "most recent version ≤ position"
- **Alternative**: Linear scan through Vec (O(n) vs O(log n))
- **Benefit**: Fast lookups with `.range(..=position).next_back()`

### 3. Option<ChainHash> for Deletions
- **Decision**: Use `Option<ChainHash>` where `None` = deleted
- **Why**: Distinguishes between "value exists" and "value deleted"
- **Alternative**: Separate deletion tracking (more complex)
- **Benefit**: Simple, matches MVCC visibility semantics

### 4. Configurable Catchup Budget
- **Decision**: `ProjectionCatchupChecker::new(max_catchup_steps)`
- **Why**: Different projections have different latency requirements
- **Alternative**: Hardcoded limit (less flexible)
- **Example**: Critical projections = 1k steps, analytics = 100k steps

### 5. Lag Tracking with Initial State
- **Decision**: Store `(lag_start_step, initial_applied, initial_commit)` when lag detected
- **Why**: Provides rich context in violation messages (lag growth/shrinkage)
- **Alternative**: Only track lag_start_step (less diagnostic info)
- **Benefit**: Can see if projection is catching up slowly vs stalled

---

## Known Limitations

1. **MVCC checker doesn't handle concurrent transactions**
   - Current: Assumes serial write order
   - Missing: Tracking uncommitted writes, transaction isolation levels
   - Reason: Kimberlite uses serial log application (no concurrency)
   - Impact: None for current architecture

2. **Projection catchup uses step count, not wall time**
   - Current: Bounded by simulation steps
   - Alternative: Use simulated time (ns)
   - Reason: Step count is deterministic, time can vary
   - Trade-off: Steps might not correlate with real-world latency

3. **No validation of projection tombstones/GC**
   - Current: Tracks deletions but not garbage collection
   - Missing: Verify old versions are properly cleaned up
   - Impact: Could miss GC bugs that leak storage
   - Plan: Add in future if GC becomes an issue

4. **MVCC visibility doesn't check transaction semantics**
   - Current: Only checks visibility at specific positions
   - Missing: Serializable snapshot isolation (SSI) validation
   - Reason: Kimberlite doesn't implement full MVCC yet
   - Impact: None for current log-replay model

5. **Applied index checker doesn't verify superblock durability**
   - Current: Only checks hash consistency
   - Missing: Verify superblock persists applied_position correctly
   - Reason: Superblock-specific invariants deferred
   - Impact: Could miss superblock write bugs

---

## Next Steps

### Immediate (Remaining Phase 7 Work)
1. None - Phase 7 projection invariants are complete ✅

### Integration Work (Future Phases)
1. **VOPR Integration**:
   - Add projection events to VOPR simulation
   - Hook invariant checkers into event stream
   - Add projection-specific scenarios (lag, MVCC queries)

2. **Apply Remaining Canaries** (from Phase 5):
   - `canary-idempotency-race` → integrate with `ClientSessionChecker`
   - `canary-monotonic-regression` → integrate with `ReplicaHeadChecker`
   - Verify checkers catch canaries

3. **Deferred Assertions for Catchup** (from Phase 4):
   - Use `assert_within_steps!` for projection catchup
   - Example: `assert_within_steps!(steps = 10_000, key = "projection_catchup", || applied == commit)`

4. **Phase Markers for Projection**:
   - `phase!("projection", "batch_applied", { position, count })`
   - `phase!("projection", "catchup_complete", { lag_duration })`
   - `phase!("projection", "snapshot_taken", { position })`

### Phase 8: SQL Metamorphic Testing
1. TLP (Ternary Logic Partitioning) oracles
2. NoREC (Non-optimizing Reference Engine)
3. Query plan coverage
4. Database state mutators

---

## Files Created/Modified

### New Files (1)
1. `/crates/kimberlite-sim/src/projection_invariants.rs` (730+ lines) - Projection & MVCC invariant checkers

### Modified Files (2)
1. `/crates/kimberlite-sim/src/lib.rs` - Added `pub mod projection_invariants` and exports
2. `/PHASE7_COMPLETE.md` - This documentation

---

## Metrics

### New in Phase 7
- **New Files**: 1 (projection_invariants.rs)
- **Modified Files**: 1 (lib.rs)
- **New Tests**: 11 (projection invariant tests)
- **Lines of Code**: ~730 (projection_invariants.rs)
- **Invariants Defined**: 4 (AppliedPosition, MVCC, AppliedIndex, Catchup)
- **Invariants Tracked**: 4 (all integrate with invariant_tracker)

### Cumulative (Phases 1-7)
- **Total Tests**: 214 (all passing)
- **Instrumentation Tests**: 26
- **Proc Macros**: 6 (fault_point!, fault!, phase!, sometimes_assert!, assert_after!, assert_within_steps!)
- **Fault Points**: 5
- **Invariants Tracked**: 16 (8 original + 4 VSR + 4 projection)
- **Phase Markers**: 1 (storage:fsync_complete)
- **Deferred Assertions**: Infrastructure complete
- **Canaries**: 5 defined, 1 applied
- **VSR Invariants**: 4 (Agreement, Prefix Property, View-Change Safety, Recovery Safety)
- **Projection Invariants**: 4 (AppliedPosition, MVCC Visibility, AppliedIndex Integrity, Catchup)

---

## References

- **TigerBeetle**: Deterministic state machine projection
- **FoundationDB**: MVCC and point-in-time queries
- **PostgreSQL**: MVCC visibility rules (xmin/xmax)
- **CockroachDB**: MVCC timestamp ordering
- **"Making Snapshot Isolation Serializable"** (Fekete et al.) - MVCC correctness
- **"An Empirical Evaluation of In-Memory Multi-Version Concurrency Control"** (Wu et al.) - MVCC performance

---

**Phase 7 Status**: ✅ **COMPLETE**
**Date Completed**: 2026-02-02
**Tests Passing**: 214/214 (kimberlite-sim)
**Projection Invariants**: 4 (all tested and tracked)
**Integration**: Ready for VOPR (future phase)
**Next Phase**: Phase 8 - SQL Metamorphic Testing
