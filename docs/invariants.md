# VOPR Invariants Reference

This document catalogs all invariants checked by Kimberlite's VOPR testing framework.

Each invariant includes:
- **What it checks**: The correctness property
- **Why it matters**: Impact if violated
- **When it runs**: Execution context
- **References**: Protocol specifications or papers

---

## Table of Contents

1. [Storage Invariants](#storage-invariants) (3)
2. [VSR Consensus Invariants](#vsr-consensus-invariants) (4)
3. [Kernel Invariants](#kernel-invariants) (2)
4. [Projection/MVCC Invariants](#projectionmvcc-invariants) (4)
5. [Client-Visible Invariants](#client-visible-invariants) (3)
6. [SQL Invariants](#sql-invariants) (3)

**Total**: 19 invariants

---

## Storage Invariants

### 1. HashChainChecker

**File**: `/crates/kimberlite-sim/src/invariant.rs`

**What it checks**:
- Every record's `prev_hash` matches the actual hash of the previous record
- Offset sequence is contiguous (no gaps)
- Genesis record (offset 0) has prev_hash = 0

**Why it matters**:
- Detects corruption in the hash chain (append-only log integrity)
- Ensures log tamper-evidence
- Validates CRC32 + length framing

**When it runs**:
- After every `SimStorage::write()`
- Triggered by `record_append()`

**Violation example**:
```
Hash chain broken at offset 42:
  Expected prev_hash: 0xABCD1234
  Actual prev_hash:   0x0000DEAD
```

**References**:
- TigerBeetle VSR: Hash-chained log design
- Kimberlite storage spec: `crates/kimberlite-storage/src/lib.rs`

---

### 2. StorageDeterminismChecker

**File**: `/crates/kimberlite-sim/src/invariant.rs`

**What it checks**:
- Same log → same storage hash (CRC32 of all blocks)
- Same log → same kernel state hash (BLAKE3 of sorted tables)
- Event counts match across replicas
- Final time matches across replicas

**Why it matters**:
- Validates deterministic state machine property
- Detects nondeterminism bugs (race conditions, uninitialized memory)
- Enables reproducible debugging

**When it runs**:
- With `--check-determinism` flag
- After running the same seed twice

**Violation example**:
```
Determinism violation at replica 0:
  Run 1 storage hash: 0xABCD1234
  Run 2 storage hash: 0x5678EFAB  ← DIFFERENT
  Divergence likely in storage layer (fsync timing?)
```

**References**:
- FoundationDB: Deterministic simulation
- Phase 3 of VOPR Enhancement Plan

---

### 3. ReplicaConsistencyChecker

**File**: `/crates/kimberlite-sim/src/invariant.rs`

**What it checks**:
- For any offset `o`, all replicas that have offset `o` agree on its content
- Replicas can be at different lengths (normal during replication)
- But where they overlap, they must match byte-for-byte

**Why it matters**:
- Detects divergence (replicas write different data at same offset)
- Validates replication correctness
- Complements VSR agreement invariant

**When it runs**:
- After every commit
- Triggered by `check_all_replicas()`

**Violation example**:
```
Replicas diverged at offset 100:
  Replica 0: hash 0xABCD1234
  Replica 1: hash 0x5678EFAB  ← DIFFERENT
```

**References**:
- Viewstamped Replication: State machine replication
- Raft: Log matching property

---

## VSR Consensus Invariants

### 4. AgreementChecker

**File**: `/crates/kimberlite-sim/src/vsr_invariants.rs`

**What it checks**:
- No two replicas commit different operations at the same `(view, op)` position
- If replica A commits `op=5` with hash H1, and replica B commits `op=5`, it must also have hash H1

**Why it matters**:
- **Core safety property of consensus**
- Violation = data loss or divergence
- Ensures all replicas agree on the log

**When it runs**:
- After every `record_commit()`
- Checks against all previously committed ops at same `(view, op)`

**Violation example**:
```
Agreement violated at (view=2, op=5):
  Replica 0 committed hash: 0xABCD1234
  Replica 1 committed hash: 0x5678EFAB  ← DIFFERENT
```

**References**:
- Viewstamped Replication Revisited (Liskov & Cowling, 2012), Section 4.1
- TigerBeetle: VSR implementation

---

### 5. PrefixPropertyChecker

**File**: `/crates/kimberlite-sim/src/vsr_invariants.rs`

**What it checks**:
- If replica A has operation at position `o`, and replica B also has position `o`, they agree on **all** operations in `[0..o]` (not just op `o`)
- The log prefix is consistent across replicas

**Why it matters**:
- Prevents "holes" in committed log
- Ensures total ordering of operations
- Stronger than just agreement on single ops

**When it runs**:
- After every `record_commit()`
- Validates entire prefix `[0..op]`

**Violation example**:
```
Prefix property violated at op=10:
  Replica 0 ops [0..10]: [H0, H1, H2, ..., H9, H10]
  Replica 1 ops [0..10]: [H0, H1, H99, ..., H9, H10]  ← MISMATCH at op=2
```

**References**:
- Viewstamped Replication: Log prefix property
- Raft: Log matching property (similar concept)

---

### 6. ViewChangeSafetyChecker

**File**: `/crates/kimberlite-sim/src/vsr_invariants.rs`

**What it checks**:
- When a view change completes, the new primary has **all** committed operations from the previous view
- No committed ops are lost during view change

**Why it matters**:
- **Critical for durability**
- Violation = committed data lost after leader change
- Ensures clients' committed writes survive failover

**When it runs**:
- After every `record_view_change()`
- Checks new primary's log contains all previous commits

**Violation example**:
```
View change safety violated (view 1 → 2):
  Previous commits in view 1: ops [0..100]
  New primary's log in view 2: ops [0..95]  ← MISSING ops 96-100
```

**References**:
- Viewstamped Replication Revisited, Section 4.3 (View Change Protocol)
- TigerBeetle: View change correctness

---

### 7. RecoverySafetyChecker

**File**: `/crates/kimberlite-sim/src/vsr_invariants.rs`

**What it checks**:
- Recovery records never discard committed offsets
- After recovery, all previously committed ops are still present
- Recovery can only extend the log, never truncate committed prefix

**Why it matters**:
- **Durability guarantee**
- Violation = data loss after crash
- Ensures crash recovery is safe

**When it runs**:
- After every `record_recovery()`
- Checks recovered log contains all pre-crash commits

**Violation example**:
```
Recovery safety violated for replica 0:
  Before crash: committed up to op=100
  After recovery: committed up to op=95  ← LOST ops 96-100
```

**References**:
- Viewstamped Replication: Recovery protocol
- Raft: Log recovery

---

## Kernel Invariants

### 8. ClientSessionChecker

**File**: `/crates/kimberlite-sim/src/invariant.rs`

**What it checks**:
- Client idempotency positions are monotonic (no regression)
- No gaps in client position sequence
- Retries with same `IdempotencyId` are idempotent

**Why it matters**:
- Validates exactly-once semantics
- Detects double-application bugs
- Ensures client retries are safe

**When it runs**:
- After every client operation
- Triggered by `record_operation()`

**Violation example**:
```
Client session violated for client_id=42:
  Last position: 5
  New position: 3  ← REGRESSION
```

**References**:
- Kimberlite kernel: Idempotency tracking
- Idempotency pattern in distributed systems

---

### 9. CommitHistoryChecker

**File**: `/crates/kimberlite-sim/src/invariant.rs`

**What it checks**:
- Commit offsets are monotonic
- No duplicate commit offsets
- No gaps in commit sequence

**Why it matters**:
- Validates commit log integrity
- Detects commit ordering bugs
- Ensures linearizable commit order

**When it runs**:
- After every `record_commit()`
- Checks commit sequence

**Violation example**:
```
Commit history violated for replica 0:
  Last commit: offset 42
  New commit: offset 42  ← DUPLICATE
```

**References**:
- Commit protocol in VSR
- Linearizability (Herlihy & Wing, 1990)

---

## Projection/MVCC Invariants

### 10. AppliedPositionMonotonicChecker

**File**: `/crates/kimberlite-sim/src/projection_invariants.rs`

**What it checks**:
- `applied_position` never regresses
- `applied_position ≤ commit_index` (can't apply uncommitted ops)
- Each projection's `applied_position` is monotonic

**Why it matters**:
- Validates MVCC visibility
- Ensures `AS OF POSITION` queries are consistent
- Detects replay bugs

**When it runs**:
- After every `record_applied_position()`
- Checks monotonicity and upper bound

**Violation example**:
```
Applied position violated for projection "user_view":
  applied_position=100, commit_index=95  ← applied > commit
```

**References**:
- Kimberlite kernel: MVCC design
- PostgreSQL: MVCC snapshot isolation

---

### 11. MvccVisibilityChecker

**File**: `/crates/kimberlite-sim/src/projection_invariants.rs`

**What it checks**:
- Queries with `AS OF POSITION p` only see data committed at or before position `p`
- No "time travel" violations (seeing future data)
- Visibility is consistent across repeated queries

**Why it matters**:
- **Core correctness for MVCC**
- Violation = queries see uncommitted or future data
- Ensures snapshot isolation

**When it runs**:
- After every query with MVCC position
- Triggered by `record_query()`

**Violation example**:
```
MVCC visibility violated:
  Query "AS OF POSITION 100" saw row with applied_position=105  ← FUTURE DATA
```

**References**:
- Multiversion Concurrency Control (Bernstein & Goodman, 1983)
- Kimberlite: MVCC snapshot reads

---

### 12. AppliedIndexIntegrityChecker

**File**: `/crates/kimberlite-sim/src/projection_invariants.rs`

**What it checks**:
- `AppliedIndex` references a real log entry (not dangling)
- Hash in `AppliedIndex` matches actual log entry hash
- `AppliedIndex` is consistent across replicas

**Why it matters**:
- Validates projection → log link
- Detects corruption in applied index
- Ensures projections can replay from log

**When it runs**:
- After every `record_applied_index()`
- Checks log entry exists and hash matches

**Violation example**:
```
Applied index integrity violated:
  AppliedIndex points to offset=42 with hash 0xABCD1234
  Log entry at offset=42 has hash 0x5678EFAB  ← MISMATCH
```

**References**:
- Kimberlite kernel: AppliedIndex design
- Event sourcing: Projection materialization

---

### 13. ProjectionCatchupChecker

**File**: `/crates/kimberlite-sim/src/projection_invariants.rs`

**What it checks**:
- Projections eventually catch up to `commit_index` within bounded steps
- No projection lags indefinitely
- Lag is within acceptable threshold (default: 10k steps)

**Why it matters**:
- **Liveness property**
- Ensures queries eventually see recent data
- Detects stuck projections

**When it runs**:
- After projection updates
- Uses deferred assertions (`assert_within_steps!`)

**Violation example**:
```
Projection catchup violated for "user_view":
  commit_index=100,000
  applied_position=50,000
  Steps since last update=10,001  ← EXCEEDS THRESHOLD
```

**References**:
- Event sourcing: Projection lag
- CQRS: Read model staleness

---

## Client-Visible Invariants

### 14. LinearizabilityChecker

**File**: `/crates/kimberlite-sim/src/invariant.rs`

**What it checks**:
- Operations appear to execute atomically and in real-time order
- If op A completes before op B starts, B observes A's effects
- Reads return the most recent write

**Why it matters**:
- **Strongest consistency guarantee**
- Client-visible correctness
- Detects stale reads, lost writes, causality violations

**When it runs**:
- After every client operation
- Triggered by `record_operation()`

**Violation example**:
```
Linearizability violated:
  Client A: WRITE(x=1) at t=1000, completes at t=1005
  Client B: READ(x) at t=1010, observes x=0  ← STALE READ
```

**References**:
- Linearizability (Herlihy & Wing, 1990)
- Jepsen: Linearizability checking (knossos library)

---

### 15. ReadYourWritesChecker

**File**: `/crates/kimberlite-sim/src/query_invariants.rs`

**What it checks**:
- After a client writes data, subsequent reads by the same client see that write
- No "lost writes" from a client's perspective

**Why it matters**:
- Session consistency guarantee
- Client UX (users expect to see their own writes)
- Weaker than linearizability but easier to provide

**When it runs**:
- After every client read
- Checks against client's write history

**Violation example**:
```
Read-your-writes violated for client_id=42:
  Client wrote x=1 at position=100
  Client read x=0 at position=105  ← DID NOT SEE OWN WRITE
```

**References**:
- Session guarantees (Terry et al., 1994)
- Consistency models in distributed systems

---

### 16. TenantIsolationChecker

**File**: `/crates/kimberlite-sim/src/query_invariants.rs`

**What it checks**:
- Queries for tenant A never return data belonging to tenant B
- Row-level security (RLS) enforced correctly
- No cross-tenant data leaks

**Why it matters**:
- **Critical for multi-tenancy**
- Violation = data breach
- Regulatory compliance (HIPAA, GDPR, SOC 2)

**When it runs**:
- After every query
- Checks result set against expected tenant

**Violation example**:
```
Tenant isolation violated:
  Query by tenant_id=42 returned row with tenant_id=99  ← LEAK
```

**References**:
- Kimberlite: Multi-tenant architecture
- PostgreSQL: Row-level security

---

## SQL Invariants

### 17. QueryDeterminismChecker

**File**: `/crates/kimberlite-sim/src/query_invariants.rs`

**What it checks**:
- Same query + same database state → same result
- No nondeterminism in query execution
- Result ordering is consistent

**Why it matters**:
- Validates deterministic query engine
- Enables reproducible query debugging
- Detects nondeterministic functions (e.g., `RANDOM()`)

**When it runs**:
- After every query
- Re-executes query, compares results

**Violation example**:
```
Query determinism violated:
  Query: "SELECT * FROM users WHERE active=true"
  Run 1: 10 rows
  Run 2: 12 rows  ← DIFFERENT (nondeterministic?)
```

**References**:
- SQLite: Deterministic execution
- PostgreSQL: Query plan stability

---

### 18. TlpOracle (Ternary Logic Partitioning)

**File**: `/crates/kimberlite-sim/src/sql_oracles.rs`

**What it checks**:
- Metamorphic testing: partition WHERE clause into TRUE/FALSE/NULL partitions
- `COUNT(original query) == COUNT(true partition) + COUNT(false partition) + COUNT(null partition)`
- Result counts must match

**Why it matters**:
- Catches SQL logic bugs without manual test cases
- Validates three-valued logic (TRUE/FALSE/NULL) correctness
- Inspired by SQLancer

**When it runs**:
- After executing SQL queries
- Triggered by `verify_partitioning()`

**Violation example**:
```
TLP violation for query "SELECT * FROM users WHERE age > 18":
  Original count: 100
  TRUE partition (age > 18): 60
  FALSE partition (age <= 18): 35
  NULL partition (age IS NULL): 3
  Sum: 98  ← MISMATCH (should be 100)
```

**References**:
- SQLancer: Automated testing of database systems
- Ternary Logic Partitioning (Rigger & Zhendong, 2020)

---

### 19. NoRecOracle (Non-optimizing Reference Engine Comparison)

**File**: `/crates/kimberlite-sim/src/sql_oracles.rs`

**What it checks**:
- Optimized query plan produces same results as unoptimized (naive) plan
- No optimization bugs (e.g., wrong index pushdown, join reordering)
- `SELECT * FROM users WHERE active=true` (optimized) == same query without indexes

**Why it matters**:
- Catches query optimizer bugs
- Validates query plan correctness
- No manual test cases needed

**When it runs**:
- After executing optimized queries
- Compares against unoptimized execution

**Violation example**:
```
NoREC violation:
  Optimized query result: 100 rows
  Unoptimized query result: 105 rows  ← MISMATCH
```

**References**:
- SQLancer: NoREC oracle
- Metamorphic testing for databases

---

## Invariant Execution Tracking

All invariants are tracked via `invariant_tracker::record_invariant_execution("name")`.

Coverage reports show:
```json
{
  "invariant_executions": {
    "linearizability": 150000,
    "hash_chain": 75000,
    "vsr_agreement": 10000,
    "projection_applied_position_monotonic": 5000,
    ...
  },
  "missed_invariants": []
}
```

If an invariant never executes, CI fails:
```
❌ Required invariant 'vsr_view_change_safety' never executed
```

---

## Adding New Invariants

See `docs/adding-invariants.md` for step-by-step guide.

Quick checklist:
1. [ ] Create checker struct in appropriate file
2. [ ] Implement checker logic with `InvariantResult` return type
3. [ ] Add `invariant_tracker::record_invariant_execution("name")` call
4. [ ] Write unit tests (pass + violation cases)
5. [ ] Add to coverage thresholds (`required_invariants` list)
6. [ ] Document in this file

---

## Summary

**19 invariants** covering:
- **Storage**: Hash chains, determinism, replication
- **Consensus**: Agreement, prefix property, view change, recovery
- **Kernel**: Client sessions, commits
- **Projection/MVCC**: Applied position, visibility, integrity, catchup
- **Client-visible**: Linearizability, read-your-writes, tenant isolation
- **SQL**: Query determinism, TLP, NoREC

Every invariant is **automatically tracked** and **enforced by CI**.

---

**Last Updated**: 2026-02-02
**Total Invariants**: 19
**Coverage**: 100% executed in nightly runs
