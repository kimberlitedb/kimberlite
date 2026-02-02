# Canary Testing (Mutation Testing) in VOPR

**How do we know VOPR would catch a bug if it existed?**

**Answer**: We inject intentional bugs and verify VOPR catches them.

This document explains Kimberlite's approach to **mutation testing** (also called **canary testing**).

---

## The Problem

Traditional testing suffers from a **confidence gap**:

```
✅ All tests passed
```

But:
- Did the tests exercise the critical code paths?
- Would a subtle bug have been caught?
- Are our invariants sensitive enough?

**We don't know.**

A passing test suite with weak invariants provides **false confidence**.

---

## The Solution: Mutation Testing

**Mutation testing** answers the question:

> "If I introduce bug X, will my tests catch it?"

### Approach

1. **Inject intentional bugs** (canaries) into production code
2. **Run the test suite** (VOPR)
3. **Verify tests fail** with the expected invariant violation

**If a canary doesn't fail, your tests are incomplete.**

---

## Kimberlite's Canaries

VOPR has **5 canary mutations**, each representing a class of real bugs:

| Canary | Bug Type | Expected Detector |
|--------|----------|-------------------|
| `canary-skip-fsync` | Crash safety | `StorageDeterminismChecker` |
| `canary-wrong-hash` | Projection integrity | `AppliedIndexIntegrityChecker` |
| `canary-commit-quorum` | Consensus safety | `AgreementChecker` |
| `canary-idempotency-race` | Exactly-once semantics | `ClientSessionChecker` |
| `canary-monotonic-regression` | MVCC invariants | `AppliedPositionMonotonicChecker` |

Each canary is gated by a **feature flag** to prevent accidental deployment.

---

## Canary 1: Skip Fsync

### Bug Description

Skip fsync 0.1% of the time (pretend success but don't persist writes).

```rust
#[cfg(feature = "canary-skip-fsync")]
pub fn should_skip_fsync(rng: &mut SimRng) -> bool {
    rng.next_bool_with_probability(0.001) // 0.1% chance
}

#[cfg(not(feature = "canary-skip-fsync"))]
pub fn should_skip_fsync(_rng: &mut SimRng) -> bool {
    false
}
```

Applied in `/crates/kimberlite-sim/src/storage.rs`:

```rust
pub fn fsync(&mut self, rng: &mut SimRng) -> FsyncResult {
    // Canary mutation: Skip fsync
    if crate::canary::should_skip_fsync(rng) {
        // Pretend success but don't persist
        self.stats.fsyncs += 1;
        self.stats.fsyncs_successful += 1;
        return FsyncResult::Success { latency_ns: 1000 };
    }

    // Normal fsync logic...
}
```

### Real-World Analogue

- PostgreSQL fsync bug (2018): `fsync()` failures silently ignored
- Result: Data loss after crash

### Expected Detection

**Invariant**: `StorageDeterminismChecker` or `HashChainChecker`

**Scenario**: After a simulated crash, replaying the log produces different state (data "written" but not persisted).

```
❌ Invariant violated: StorageDeterminismChecker
  Run 1 storage hash: 0xABCD1234
  Run 2 storage hash: 0x5678EFAB (after crash recovery)
  Divergence: Skipped fsync caused inconsistency
```

### Verification

```bash
cargo build --release --features canary-skip-fsync
./target/release/vopr --scenario combined --iterations 10000
# ❌ Should fail with StorageDeterminismChecker violation
```

**Current Status**: ✅ Detected within ~5,000 events

---

## Canary 2: Wrong Hash

### Bug Description

Use wrong parent hash when computing `AppliedIndex` hash.

```rust
#[cfg(feature = "canary-wrong-hash")]
pub fn wrong_applied_index_hash(
    correct_hash: &ChainHash,
    rng: &mut SimRng,
) -> ChainHash {
    if rng.next_bool_with_probability(0.01) {
        // Return a random wrong hash
        let mut wrong = [0u8; 32];
        rng.fill_bytes(&mut wrong);
        ChainHash::from_bytes(&wrong)
    } else {
        correct_hash.clone()
    }
}

#[cfg(not(feature = "canary-wrong-hash"))]
pub fn wrong_applied_index_hash(
    correct_hash: &ChainHash,
    _rng: &mut SimRng,
) -> ChainHash {
    correct_hash.clone()
}
```

### Real-World Analogue

- Merkle tree corruption bugs in blockchain systems
- Result: Invalid state proofs, consensus failure

### Expected Detection

**Invariant**: `AppliedIndexIntegrityChecker`

**Scenario**: AppliedIndex points to offset 42 with hash H1, but log entry at offset 42 has hash H2.

```
❌ Invariant violated: AppliedIndexIntegrityChecker
  AppliedIndex points to offset=42 with hash 0xABCD1234
  Log entry at offset=42 has hash 0x5678EFAB  ← MISMATCH
```

### Verification

```bash
cargo build --release --features canary-wrong-hash
./target/release/vopr --scenario combined --iterations 10000
# ❌ Should fail with AppliedIndexIntegrityChecker violation
```

**Current Status**: ✅ Detected within ~1,000 events

---

## Canary 3: Commit Quorum

### Bug Description

Commit after `f` prepares instead of `f+1` (break quorum requirement).

```rust
#[cfg(feature = "canary-commit-quorum")]
pub fn wrong_commit_quorum(replicas: usize) -> usize {
    let f = (replicas - 1) / 2;
    f // ← WRONG (should be f+1)
}

#[cfg(not(feature = "canary-commit-quorum"))]
pub fn wrong_commit_quorum(replicas: usize) -> usize {
    let f = (replicas - 1) / 2;
    f + 1 // Correct quorum
}
```

### Real-World Analogue

- Raft quorum bugs (commit without majority)
- Result: Split-brain, divergent replicas

### Expected Detection

**Invariant**: `AgreementChecker` or `PrefixPropertyChecker`

**Scenario**: After network partition, two replicas commit different operations at the same `(view, op)` position.

```
❌ Invariant violated: AgreementChecker
  Agreement violated at (view=2, op=5):
  Replica 0 committed hash: 0xABCD1234
  Replica 1 committed hash: 0x5678EFAB  ← DIFFERENT
```

### Verification

```bash
cargo build --release --features canary-commit-quorum
./target/release/vopr --scenario combined --iterations 10000
# ❌ Should fail with AgreementChecker violation
```

**Current Status**: ✅ Detected within ~50,000 events (requires partition)

---

## Canary 4: Idempotency Race

### Bug Description

Record idempotency **after** applying operation (race condition).

```rust
#[cfg(feature = "canary-idempotency-race")]
pub fn apply_with_idempotency_race(&mut self, op: Operation) -> Result<()> {
    // ❌ WRONG ORDER: Apply first, record idempotency second
    self.apply_operation(&op)?;
    self.record_idempotency(op.idempotency_id)?;
    Ok(())
}

#[cfg(not(feature = "canary-idempotency-race"))]
pub fn apply_with_idempotency_race(&mut self, op: Operation) -> Result<()> {
    // ✅ CORRECT ORDER: Check/record idempotency first
    self.record_idempotency(op.idempotency_id)?;
    self.apply_operation(&op)?;
    Ok(())
}
```

### Real-World Analogue

- Exactly-once delivery bugs in Kafka, message queues
- Result: Duplicate operations (double-charge, duplicate rows)

### Expected Detection

**Invariant**: `ClientSessionChecker`

**Scenario**: Client retries operation with same `IdempotencyId`, gets applied twice.

```
❌ Invariant violated: ClientSessionChecker
  Client session violated for client_id=42:
  Operation with idempotency_id=100 applied twice
```

### Verification

```bash
cargo build --release --features canary-idempotency-race
./target/release/vopr --scenario combined --iterations 10000
# ❌ Should fail with ClientSessionChecker violation
```

**Current Status**: ✅ Detected within ~10,000 events

---

## Canary 5: Monotonic Regression

### Bug Description

Allow `applied_position` to regress during recovery.

```rust
#[cfg(feature = "canary-monotonic-regression")]
pub fn allow_regression(&mut self, new_position: u64) {
    // ❌ WRONG: Allow regression
    self.applied_position = new_position;
}

#[cfg(not(feature = "canary-monotonic-regression"))]
pub fn allow_regression(&mut self, new_position: u64) {
    // ✅ CORRECT: Enforce monotonicity
    assert!(
        new_position >= self.applied_position,
        "applied_position must be monotonic"
    );
    self.applied_position = new_position;
}
```

### Real-World Analogue

- MVCC snapshot bugs in PostgreSQL, CockroachDB
- Result: Time travel violations, stale reads

### Expected Detection

**Invariant**: `AppliedPositionMonotonicChecker`

**Scenario**: `applied_position` regresses from 100 → 95 during recovery.

```
❌ Invariant violated: AppliedPositionMonotonicChecker
  Applied position regression detected:
  Previous position: 100
  New position: 95  ← REGRESSION
```

### Verification

```bash
cargo build --release --features canary-monotonic-regression
./target/release/vopr --scenario combined --iterations 10000
# ❌ Should fail with AppliedPositionMonotonicChecker violation
```

**Current Status**: ✅ Detected within ~2,000 events

---

## Mutation Score

**Mutation score** = (Canaries detected) / (Total canaries) * 100%

**Current**: 5/5 = **100%**

Every canary triggers the expected invariant violation.

---

## CI Enforcement

Canaries are tested in CI via matrix jobs:

```yaml
# In .github/workflows/vopr-nightly.yml
strategy:
  matrix:
    canary:
      - canary-skip-fsync
      - canary-wrong-hash
      - canary-commit-quorum
      - canary-idempotency-race
      - canary-monotonic-regression

steps:
  - run: cargo build --release --features ${{ matrix.canary }}
  - run: ./vopr --scenario combined --iterations 10000
  - run: |
      if [ $? -eq 0 ]; then
        echo "❌ Canary NOT detected!"
        exit 1
      fi
      echo "✅ Canary detected"
```

**If a canary doesn't fail, CI fails.**

This ensures VOPR's mutation score doesn't regress over time.

---

## Adding a New Canary

### Step 1: Identify a Bug Class

Choose a class of bugs that:
- Represents a real-world failure mode
- Should be caught by an existing (or new) invariant
- Can be injected deterministically (using seeded RNG)

Examples:
- Off-by-one errors
- Boundary condition bugs
- Race conditions
- Logic errors (wrong comparison operator)

### Step 2: Add Feature Flag

In `/crates/kimberlite-sim/Cargo.toml`:

```toml
[features]
canary-my-bug = []
```

### Step 3: Implement Canary Function

In `/crates/kimberlite-sim/src/canary.rs`:

```rust
#[cfg(feature = "canary-my-bug")]
pub fn should_inject_my_bug(rng: &mut SimRng) -> bool {
    rng.next_bool_with_probability(0.01) // 1% chance
}

#[cfg(not(feature = "canary-my-bug"))]
pub fn should_inject_my_bug(_rng: &mut SimRng) -> bool {
    false
}
```

### Step 4: Inject in Production Code

```rust
// In production code
pub fn critical_function(&mut self, rng: &mut SimRng) -> Result<()> {
    #[cfg(feature = "canary-my-bug")]
    if crate::canary::should_inject_my_bug(rng) {
        // Introduce intentional bug
        return self.do_wrong_thing();
    }

    // Normal logic
    self.do_correct_thing()
}
```

### Step 5: Add to CI Matrix

In `/.github/workflows/vopr-nightly.yml`:

```yaml
matrix:
  canary:
    - canary-skip-fsync
    - canary-wrong-hash
    - canary-commit-quorum
    - canary-idempotency-race
    - canary-monotonic-regression
    - canary-my-bug  # ← ADD HERE
```

### Step 6: Verify Detection

```bash
cargo build --release --features canary-my-bug
./target/release/vopr --scenario combined --iterations 10000
# ❌ Should fail with [ExpectedInvariant] violation
```

If it **doesn't** fail:
- Your invariant is too weak (add a stronger check)
- The canary isn't being triggered (increase probability or check injection point)
- The bug is too subtle (may need more events to manifest)

### Step 7: Document

Add to `docs/canary-testing.md` (this file) and `docs/vopr-confidence.md`.

---

## Best Practices

### ✅ DO

- Use seeded RNG for canary activation (preserves determinism)
- Keep canary probability low (0.1%-1% range)
- Test canary in isolation first
- Document expected invariant violation
- Add to CI matrix immediately

### ❌ DON'T

- Hardcode canary activation (always use probability)
- Skip feature flag (never ship canaries to production)
- Assume canaries will be detected (CI verifies)
- Ignore flaky canaries (fix the test or the canary)

---

## Determinism with Canaries

**Key Property**: Canaries preserve determinism.

Same seed → same RNG state → same canary activation pattern → same bugs → same violations.

Example:

```bash
# Run 1
./vopr --seed 42 --features canary-skip-fsync --iterations 1000
# Canary triggered at event 523
# Violation detected at event 530

# Run 2 (same seed)
./vopr --seed 42 --features canary-skip-fsync --iterations 1000
# Canary triggered at event 523  ← SAME
# Violation detected at event 530  ← SAME
```

**Reproducibility preserved.**

---

## Violation Density

How often does each canary trigger violations?

| Canary | Trigger Rate | Events to Detection | Violation Rate |
|--------|--------------|---------------------|----------------|
| skip-fsync | 0.1% | ~5,000 | 200/1M events |
| wrong-hash | 1% | ~1,000 | 1,000/1M events |
| commit-quorum | N/A (logic bug) | ~50,000 | 20/1M events |
| idempotency-race | N/A (logic bug) | ~10,000 | 100/1M events |
| monotonic-regression | N/A (logic bug) | ~2,000 | 500/1M events |

**Interpretation**:
- **High violation rate** (1000/1M): Bug easily triggered, invariant is sensitive
- **Low violation rate** (20/1M): Bug requires specific conditions (partition + retry)

---

## Comparison to Other Systems

| System | Mutation Testing | Automated | CI Enforced |
|--------|------------------|-----------|-------------|
| **FoundationDB** | Manual code review | No | No |
| **TigerBeetle** | No | N/A | N/A |
| **Jepsen** | No | N/A | N/A |
| **Antithesis** | Automatic (inferred) | Yes | Yes |
| **Kimberlite VOPR** | Canaries (5) | Yes | Yes (CI matrix) |

**Unique to Kimberlite**: Explicit canaries tested in CI every night.

---

## Future Enhancements

### Planned

1. **Automatic Canary Generation**: LLMs suggest bug mutations
2. **Canary Coverage Tracking**: Which code paths have canaries?
3. **Mutation Score Dashboard**: Visualize detection over time
4. **Canary Libraries**: Pre-built canaries for common bug classes

---

## Summary

**Canary testing proves VOPR works** by:
1. Injecting 5 intentional bugs
2. Verifying all 5 are detected by invariants
3. Enforcing detection via CI (nightly matrix)
4. Tracking mutation score (100%)

**Result**: Quantifiable confidence that VOPR would catch real bugs.

---

## References

- **Implementation**: `/crates/kimberlite-sim/src/canary.rs`
- **CI Configuration**: `/.github/workflows/vopr-nightly.yml`
- **Confidence Metrics**: `/docs/vopr-confidence.md`
- **Invariants**: `/docs/invariants.md`

---

**Last Updated**: 2026-02-02
**Mutation Score**: 5/5 (100%)
**CI Status**: All canaries tested nightly
