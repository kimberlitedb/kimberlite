# Phase 6 Complete: Missing VSR Invariants

## Summary

Phase 6 has been successfully implemented. Kimberlite now has 4 additional VSR (Viewstamped Replication) consensus invariants that verify the safety properties of the distributed replication protocol. These invariants fill critical gaps in consensus correctness checking.

---

## Deliverables ✅

### Task #1: VSR Invariant Checkers Module ✅

**Module Created**: `/crates/kimberlite-sim/src/vsr_invariants.rs` (690+ lines)

**Purpose**: Verify consensus safety properties for VSR protocol

**Invariants Implemented** (4 total):

1. **AgreementChecker**: No divergent commits at same (view, op) position
   - **Safety Property**: For any (view, op) tuple, at most one operation is committed
   - **Violation Example**: Replica R1 commits operation A at (view=1, op=5), Replica R2 commits operation B at (view=1, op=5)
   - **Why it matters**: Agreement is fundamental to consensus - divergence breaks consistency
   - **Expected to catch**: `canary-commit-quorum`, network partition bugs, view-change bugs allowing multiple primaries

2. **PrefixPropertyChecker**: All replicas agree on log prefix
   - **Safety Property**: If replica R has committed operation o, all replicas agree on operations [0..o]
   - **Violation Example**: R1 has [A, B, C], R2 has [A, X, C] where X ≠ B
   - **Why it matters**: Log prefix must be consistent - divergence means incompatible histories
   - **Expected to catch**: Log truncation bugs, view-change bugs, unsafe repair operations

3. **ViewChangeSafetyChecker**: View changes preserve committed operations
   - **Safety Property**: After view change, new primary has all operations committed in previous view
   - **Violation Example**: View 0 committed [A, B, C], but View 1's new primary only has [A, B]
   - **Why it matters**: View changes must preserve durability - losing committed data violates guarantees
   - **Expected to catch**: Premature view-change completion, unsafe log truncation, quorum calculation bugs

4. **RecoverySafetyChecker**: Recovery never discards committed offsets
   - **Safety Property**: During recovery, replica never truncates below highest committed operation
   - **Violation Example**: Had committed [0, 1, 2, 3] before crash, only has [0, 1] after recovery
   - **Why it matters**: Recovery must preserve durability - discarding acknowledged writes is data loss
   - **Expected to catch**: Unsafe log truncation, missing NACK quorum checks, superblock corruption

---

### Task #2: API Design ✅

**Core Types**:
```rust
pub struct AgreementChecker {
    committed: HashMap<(u64, u64), HashMap<u8, ChainHash>>,
    checks_performed: u64,
}

pub struct PrefixPropertyChecker {
    replica_logs: HashMap<u8, BTreeMap<u64, ChainHash>>,
    checks_performed: u64,
}

pub struct ViewChangeSafetyChecker {
    committed_in_view: HashMap<u64, u64>,
    primary_after_view_change: HashMap<u64, BTreeMap<u64, ChainHash>>,
    checks_performed: u64,
}

pub struct RecoverySafetyChecker {
    pre_crash_commit: HashMap<u8, u64>,
    checks_performed: u64,
}
```

**Agreement Checker API**:
```rust
impl AgreementChecker {
    pub fn new() -> Self;
    pub fn record_commit(
        &mut self,
        replica_id: ReplicaId,
        view: ViewNumber,
        op: OpNumber,
        operation_hash: &ChainHash,
    ) -> InvariantResult;
    pub fn checks_performed(&self) -> u64;
    pub fn reset(&mut self);
}
```

**Prefix Property Checker API**:
```rust
impl PrefixPropertyChecker {
    pub fn new() -> Self;
    pub fn record_committed_op(
        &mut self,
        replica_id: ReplicaId,
        op: OpNumber,
        operation_hash: &ChainHash,
    );
    pub fn check_prefix_agreement(&mut self, up_to_op: OpNumber) -> InvariantResult;
    pub fn checks_performed(&self) -> u64;
    pub fn reset(&mut self);
}
```

**View-Change Safety Checker API**:
```rust
impl ViewChangeSafetyChecker {
    pub fn new() -> Self;
    pub fn record_committed_in_view(&mut self, view: ViewNumber, highest_op: OpNumber);
    pub fn record_view_change_complete(
        &mut self,
        new_view: ViewNumber,
        primary_log: &BTreeMap<u64, ChainHash>,
    ) -> InvariantResult;
    pub fn checks_performed(&self) -> u64;
    pub fn reset(&mut self);
}
```

**Recovery Safety Checker API**:
```rust
impl RecoverySafetyChecker {
    pub fn new() -> Self;
    pub fn record_pre_crash_state(&mut self, replica_id: ReplicaId, highest_commit: OpNumber);
    pub fn check_post_recovery_state(
        &mut self,
        replica_id: ReplicaId,
        post_recovery_commit: OpNumber,
    ) -> InvariantResult;
    pub fn checks_performed(&self) -> u64;
    pub fn reset(&mut self);
}
```

---

### Task #3: Integration with Existing Infrastructure ✅

**Added Dependencies**:
- Modified `/crates/kimberlite-sim/Cargo.toml` to add `kimberlite-vsr` dependency
- Uses existing `InvariantResult` type from `invariant.rs`
- Integrates with `invariant_tracker` for execution tracking

**Type Integration**:
- Uses `kimberlite_vsr::{ViewNumber, OpNumber, ReplicaId}`
- Uses `kimberlite_crypto::ChainHash` for operation hashing
- Leverages `std::collections::{HashMap, BTreeMap}` for efficient lookups

**Invariant Tracking**:
- All 4 checkers call `invariant_tracker::record_invariant_execution()`
- Tracked names: `vsr_agreement`, `vsr_prefix_property`, `vsr_view_change_safety`, `vsr_recovery_safety`
- Enables coverage reporting in VOPR

---

### Task #4: Comprehensive Testing ✅

**Tests Added** (9 total in `vsr_invariants.rs`):

1. **test_agreement_checker_ok**
   - Two replicas commit same operation at same position → OK
   - Verifies normal case works

2. **test_agreement_checker_violation**
   - Two replicas commit different operations at same position → VIOLATION
   - Confirms agreement invariant catches divergence

3. **test_prefix_property_checker_ok**
   - Two replicas with matching prefix → OK
   - One replica can be ahead (not a violation)

4. **test_prefix_property_checker_violation**
   - Replicas disagree on operation at position 1 → VIOLATION
   - Catches divergent histories

5. **test_view_change_safety_ok**
   - New primary has all committed ops from previous view → OK
   - Normal view-change case

6. **test_view_change_safety_violation**
   - New primary missing ops 4 and 5 from previous view → VIOLATION
   - Catches unsafe view changes

7. **test_recovery_safety_ok**
   - Recovery preserves or advances commit point → OK
   - Normal recovery case

8. **test_recovery_safety_violation**
   - Recovery discards committed operations → VIOLATION
   - Catches unsafe recovery

9. **test_all_checkers_track_execution**
   - Verifies all 4 invariants integrate with invariant_tracker
   - Confirms coverage tracking works

**All tests pass** (203/203 in kimberlite-sim, up from 194)

---

## Architecture

```
┌────────────────────────────────────────────────────────────┐
│  VSR Protocol (kimberlite-vsr)                             │
│  - ReplicaState transitions                                │
│  - Prepare/PrepareOk/Commit messages                       │
│  - View changes (DoViewChange/StartView)                   │
│  - Recovery (RecoveryRequest/Response)                     │
└────────────────┬───────────────────────────────────────────┘
                 │ Records commits, view changes, recovery
                 ▼
┌────────────────────────────────────────────────────────────┐
│  VSR Invariant Checkers (kimberlite-sim)                   │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ AgreementChecker                                     │  │
│  │  - record_commit(replica, view, op, hash)           │  │
│  │  - Detects: divergent commits at same position      │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ PrefixPropertyChecker                                │  │
│  │  - record_committed_op(replica, op, hash)           │  │
│  │  - check_prefix_agreement(up_to_op)                 │  │
│  │  - Detects: divergent log prefixes                  │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ ViewChangeSafetyChecker                              │  │
│  │  - record_committed_in_view(view, highest_op)       │  │
│  │  - record_view_change_complete(new_view, log)       │  │
│  │  - Detects: lost ops during view change             │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ RecoverySafetyChecker                                │  │
│  │  - record_pre_crash_state(replica, commit)          │  │
│  │  - check_post_recovery_state(replica, commit)       │  │
│  │  - Detects: lost ops during recovery                │  │
│  └──────────────────────────────────────────────────────┘  │
└────────────────┬───────────────────────────────────────────┘
                 │ Reports violations
                 ▼
┌────────────────────────────────────────────────────────────┐
│  VOPR (Simulation Harness)                                 │
│  - Runs VSR scenarios                                      │
│  - Injects faults (network, storage, crashes)              │
│  - Checks invariants after each event                      │
│  - Fails on violation with detailed context                │
└────────────────────────────────────────────────────────────┘
```

---

## Testing & Verification ✅

### Unit Tests (9 new)
- **kimberlite-sim**: 203/203 passing (up from 194)
  - `vsr_invariants::tests`: 9 tests covering all 4 checkers
  - Each invariant has both "ok" (normal) and "violation" (error) test cases
  - Invariant tracking verified

### Coverage Tracking
All 4 VSR invariants integrated with `invariant_tracker`:
```bash
# After running VOPR with VSR simulation
invariant_tracker.get_run_count("vsr_agreement")          // > 0
invariant_tracker.get_run_count("vsr_prefix_property")    // > 0
invariant_tracker.get_run_count("vsr_view_change_safety") // > 0
invariant_tracker.get_run_count("vsr_recovery_safety")    // > 0
```

---

## Usage Examples

### Agreement Checker

```rust
use kimberlite_sim::{AgreementChecker, InvariantResult};
use kimberlite_vsr::{ReplicaId, ViewNumber, OpNumber};
use kimberlite_crypto::ChainHash;

let mut checker = AgreementChecker::new();

// Replica 0 commits operation at (view=1, op=5)
let hash = /* compute hash of operation */;
let result = checker.record_commit(
    ReplicaId::new(0),
    ViewNumber::from(1),
    OpNumber::from(5),
    &hash,
);
assert!(matches!(result, InvariantResult::Ok));

// Replica 1 commits SAME operation - OK
let result = checker.record_commit(
    ReplicaId::new(1),
    ViewNumber::from(1),
    OpNumber::from(5),
    &hash,
);
assert!(matches!(result, InvariantResult::Ok));

// Replica 2 commits DIFFERENT operation - VIOLATION
let different_hash = /* different operation */;
let result = checker.record_commit(
    ReplicaId::new(2),
    ViewNumber::from(1),
    OpNumber::from(5),
    &different_hash,
);
assert!(matches!(result, InvariantResult::Violated { .. }));
```

### Prefix Property Checker

```rust
use kimberlite_sim::PrefixPropertyChecker;

let mut checker = PrefixPropertyChecker::new();

// Replica 0: [A, B, C]
checker.record_committed_op(ReplicaId::new(0), OpNumber::from(0), &hash_a);
checker.record_committed_op(ReplicaId::new(0), OpNumber::from(1), &hash_b);
checker.record_committed_op(ReplicaId::new(0), OpNumber::from(2), &hash_c);

// Replica 1: [A, B] (only has prefix, not caught up yet)
checker.record_committed_op(ReplicaId::new(1), OpNumber::from(0), &hash_a);
checker.record_committed_op(ReplicaId::new(1), OpNumber::from(1), &hash_b);

// Check prefix agreement up to op 1 - should be OK
let result = checker.check_prefix_agreement(OpNumber::from(1));
assert!(matches!(result, InvariantResult::Ok));

// If Replica 1 had different op at position 1, it would violate
```

### View-Change Safety Checker

```rust
use kimberlite_sim::ViewChangeSafetyChecker;
use std::collections::BTreeMap;

let mut checker = ViewChangeSafetyChecker::new();

// View 0 committed operations 0-5
checker.record_committed_in_view(ViewNumber::from(0), OpNumber::from(5));

// View change to view 1
// New primary must have all ops 0-5
let mut primary_log = BTreeMap::new();
for i in 0..=5 {
    primary_log.insert(i, /* hash of op i */);
}

let result = checker.record_view_change_complete(ViewNumber::from(1), &primary_log);
assert!(matches!(result, InvariantResult::Ok));

// If primary_log only had 0-3, it would violate (lost ops 4, 5)
```

### Recovery Safety Checker

```rust
use kimberlite_sim::RecoverySafetyChecker;

let mut checker = RecoverySafetyChecker::new();

// Before crash: committed up to op 10
checker.record_pre_crash_state(ReplicaId::new(0), OpNumber::from(10));

// After recovery: must have at least op 10
let result = checker.check_post_recovery_state(ReplicaId::new(0), OpNumber::from(10));
assert!(matches!(result, InvariantResult::Ok));

// Can have more (replayed additional ops)
let result = checker.check_post_recovery_state(ReplicaId::new(0), OpNumber::from(15));
assert!(matches!(result, InvariantResult::Ok));

// Cannot have less - VIOLATION
let result = checker.check_post_recovery_state(ReplicaId::new(0), OpNumber::from(7));
assert!(matches!(result, InvariantResult::Violated { .. }));
```

---

## Integration with VOPR (Future)

When VSR simulation is added to VOPR, these invariants will be checked:

```rust
// In VOPR simulation loop (future integration)
let mut agreement = AgreementChecker::new();
let mut prefix = PrefixPropertyChecker::new();
let mut view_change = ViewChangeSafetyChecker::new();
let mut recovery = RecoverySafetyChecker::new();

// After each VSR event
match event {
    VsrEvent::Commit { replica, view, op, hash } => {
        // Check agreement
        if let InvariantResult::Violated { .. } = agreement.record_commit(replica, view, op, &hash) {
            return Err(SimError::InvariantViolation(/* ... */));
        }

        // Track for prefix property
        prefix.record_committed_op(replica, op, &hash);
        view_change.record_committed_in_view(view, op);
    }

    VsrEvent::ViewChangeComplete { new_view, primary_log } => {
        if let InvariantResult::Violated { .. } = view_change.record_view_change_complete(new_view, &primary_log) {
            return Err(SimError::InvariantViolation(/* ... */));
        }
    }

    VsrEvent::Crash { replica, commit } => {
        recovery.record_pre_crash_state(replica, commit);
    }

    VsrEvent::RecoveryComplete { replica, commit } => {
        if let InvariantResult::Violated { .. } = recovery.check_post_recovery_state(replica, commit) {
            return Err(SimError::InvariantViolation(/* ... */));
        }
    }

    _ => {}
}

// Periodically check prefix property
if step % 1000 == 0 {
    if let InvariantResult::Violated { .. } = prefix.check_prefix_agreement(highest_op) {
        return Err(SimError::InvariantViolation(/* ... */));
    }
}
```

---

## Key Design Decisions

### 1. Explicit Recording vs Implicit Checking
- **Decision**: Require explicit `record_commit()` calls, not automatic interception
- **Why**: VSR protocol is complex - explicit calls make integration points clear
- **Alternative**: Hook into VSR internals (too invasive)
- **Benefit**: Clean separation between protocol and invariants

### 2. BTreeMap for Replica Logs
- **Decision**: Use `BTreeMap<u64, ChainHash>` instead of `Vec`
- **Why**: Sparse logs (gaps from uncommitted ops), efficient prefix checks, sorted iteration
- **Alternative**: Dense `Vec` with Option (wastes space, harder to check ranges)
- **Benefit**: O(log n) lookups, efficient range queries

### 3. Stateful Checkers (Not Pure Functions)
- **Decision**: Checkers maintain state across multiple events
- **Why**: Need to track replica histories, view transitions, pre-crash state
- **Alternative**: Pure functions requiring full history passed each time (expensive)
- **Trade-off**: Checkers must be reset between VOPR runs

### 4. HashMap for View-Change Tracker
- **Decision**: `HashMap<u64, BTreeMap<u64, ChainHash>>` for view → log mapping
- **Why**: Views are sparse (only track ones with view changes), log needs sorted iteration
- **Alternative**: Single global log (doesn't track per-view state)
- **Benefit**: Efficient view-specific queries

### 5. Separate Checkers vs Unified
- **Decision**: 4 separate checker structs, not one monolithic VSR checker
- **Why**: Single Responsibility Principle, easier to test, can be used independently
- **Alternative**: One `VsrChecker` with multiple methods (harder to test, tightly coupled)
- **Benefit**: Composable, testable, can enable/disable individually

---

## Known Limitations

1. **Not yet integrated into VOPR**
   - Current: Checkers are defined and tested in isolation
   - Reason: VSR simulation infrastructure not yet integrated with VOPR
   - Plan: Add VSR events to VOPR in Phase 7
   - Workaround: Can be manually tested with mock VSR events

2. **No canary integration yet**
   - Current: `canary-commit-quorum` defined (Phase 5) but not connected
   - Need: Apply canary to VSR commit logic
   - Need: Verify AgreementChecker catches it
   - Plan: Phase 7 integration

3. **Prefix checking is O(n²) in replicas**
   - Current: Compares all pairs of replicas
   - Impact: With 7 replicas, 21 comparisons per check
   - Alternative: Track canonical log and compare each replica to it (O(n))
   - Mitigation: Typically 3-7 replicas (acceptable performance)

4. **No view-change phase markers yet**
   - Current: ViewChangeSafetyChecker API ready, but no phase markers in VSR code
   - Need: Add `phase!("vsr", "view_change_complete", ...)` to VSR protocol
   - Plan: Phase 7 when instrumenting VSR

5. **Recovery checker doesn't verify superblock**
   - Current: Only checks commit log position
   - Missing: Superblock integrity check
   - Reason: Superblock-specific invariants deferred
   - Impact: Could miss superblock corruption cases

---

## Next Steps

### Immediate (Remaining Phase 6 Work)
1. None - Phase 6 is complete ✅

### Phase 7: VSR Integration (Week 9)
1. Add VSR phase markers:
   - `phase!("vsr", "prepare_sent", { view, op })`
   - `phase!("vsr", "commit_broadcast", { view, op })`
   - `phase!("vsr", "view_change_complete", { new_view, primary })`
   - `phase!("vsr", "recovery_complete", { replica, commit })`

2. Integrate VSR invariants into VOPR:
   - Create `VoprVsrRunner` that tracks VSR events
   - Hook invariant checkers into event stream
   - Add coverage tracking for VSR invariants

3. Apply `canary-commit-quorum`:
   - Modify VSR commit logic to use f instead of f+1 when canary enabled
   - Verify AgreementChecker catches it

4. Create VSR-specific VOPR scenarios:
   - View changes under load
   - Crash and recovery
   - Network partitions

### Phase 8: Projection/MVCC Invariants
1. Projection catchup tracking
2. MVCC visibility invariants
3. AppliedIndex integrity

---

## Files Created/Modified

### New Files (1)
1. `/crates/kimberlite-sim/src/vsr_invariants.rs` (690+ lines) - VSR invariant checkers

### Modified Files (3)
1. `/crates/kimberlite-sim/Cargo.toml` - Added `kimberlite-vsr` dependency
2. `/crates/kimberlite-sim/src/lib.rs` - Added `pub mod vsr_invariants` and exports
3. `/PHASE6_COMPLETE.md` - This documentation

---

## Metrics

### New in Phase 6
- **New Files**: 1 (vsr_invariants.rs)
- **Modified Files**: 2 (Cargo.toml, lib.rs)
- **New Tests**: 9 (VSR invariant tests)
- **Lines of Code**: ~690 (vsr_invariants.rs)
- **Invariants Defined**: 4 (Agreement, Prefix Property, View-Change Safety, Recovery Safety)
- **Invariants Tracked**: 4 (all integrate with invariant_tracker)

### Cumulative (Phases 1-6)
- **Total Tests**: 203 (all passing)
- **Instrumentation Tests**: 26
- **Proc Macros**: 6 (fault_point!, fault!, phase!, sometimes_assert!, assert_after!, assert_within_steps!)
- **Fault Points**: 5
- **Invariants Tracked**: 12 (8 original + 4 VSR)
- **Phase Markers**: 1 (storage:fsync_complete)
- **Deferred Assertions**: Infrastructure complete
- **Canaries**: 5 defined, 1 applied
- **VSR Invariants**: 4 (Agreement, Prefix Property, View-Change Safety, Recovery Safety)

---

## References

- **"Viewstamped Replication Revisited"** (Liskov & Cowling, 2012) - VSR protocol specification
- **TigerBeetle VOPR** - VSR simulation testing methodology
- **FoundationDB** - Simulation testing with consensus invariants
- **Raft Paper** - Consensus safety properties (similar to VSR)
- **Jepsen** - Distributed systems testing, linearizability checking

---

**Phase 6 Status**: ✅ **COMPLETE**
**Date Completed**: 2026-02-02
**Tests Passing**: 203/203 (kimberlite-sim)
**VSR Invariants**: 4 (all tested and tracked)
**Integration**: Ready for VOPR (Phase 7)
**Canary Support**: Ready for commit-quorum canary
**Next Phase**: Phase 7 - VSR Integration & Projection/MVCC Invariants
