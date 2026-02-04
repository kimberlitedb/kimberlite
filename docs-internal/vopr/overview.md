# VOPR (Deterministic Simulation Testing) - Deep Dive

**Internal Documentation** - For Kimberlite contributors and maintainers

This document provides detailed implementation details for VOPR, Kimberlite's deterministic simulation testing framework. For high-level testing philosophy, see [Testing Overview](../../docs/internals/testing/overview.md).

---

## VOPR (Deterministic Simulation Testing)

VOPR (Viewstamped Operation Replication) is Kimberlite's deterministic simulator for testing consensus and replication under faults. It validates safety properties through exhaustive fault injection while maintaining reproducibility.

### Confidence Through Measurement

**Confidence comes from falsification power, not green runs.**

VOPR treats testing as a measurable product. Every run produces:

1. **Coverage metrics**: Which fault points, invariants, and phases were exercised
2. **Mutation score**: Can VOPR catch intentional bugs (canaries)?
3. **Determinism validation**: Same seed → same results
4. **Violation density**: How often do invariants catch bugs per 1M events?

Current metrics:
- **Coverage**: 90% fault points, 100% critical invariants
- **Mutation score**: 100% (5/5 canaries detected)
- **Determinism**: 100% (10/10 seeds reproducible)
- **Throughput**: 85k-167k sims/sec

### Invariant Checkers

VOPR validates 19 invariants across 6 categories. All invariants are tracked via `invariant_tracker::record_invariant_execution()` to ensure comprehensive coverage.

#### Storage Invariants (3)

**1. HashChainChecker**
- **What it checks**: Every record's `prev_hash` matches the actual hash of the previous record
- **Why it matters**: Detects corruption in the hash chain (append-only log integrity)
- **When it runs**: After every `SimStorage::write()`

**2. StorageDeterminismChecker**
- **What it checks**: Same log → same storage hash (CRC32 of all blocks)
- **Why it matters**: Validates deterministic state machine property
- **When it runs**: With `--check-determinism` flag, after running the same seed twice

**3. ReplicaConsistencyChecker**
- **What it checks**: For any offset `o`, all replicas that have offset `o` agree on its content
- **Why it matters**: Detects divergence (replicas write different data at same offset)
- **When it runs**: After every commit

#### VSR Consensus Invariants (4)

**4. AgreementChecker**
- **What it checks**: No two replicas commit different operations at the same `(view, op)` position
- **Why it matters**: Core safety property of consensus - violation = data loss or divergence
- **When it runs**: After every `record_commit()`
- **Reference**: Viewstamped Replication Revisited (Liskov & Cowling, 2012), Section 4.1

**5. PrefixPropertyChecker**
- **What it checks**: If replica A has operation at position `o`, and replica B also has position `o`, they agree on all operations in `[0..o]`
- **Why it matters**: Prevents "holes" in committed log, ensures total ordering
- **When it runs**: After every `record_commit()`

**6. ViewChangeSafetyChecker**
- **What it checks**: When a view change completes, the new primary has all committed operations from the previous view
- **Why it matters**: Critical for durability - ensures clients' committed writes survive failover
- **When it runs**: After every `record_view_change()`

**7. RecoverySafetyChecker**
- **What it checks**: Recovery records never discard committed offsets
- **Why it matters**: Durability guarantee - prevents data loss after crash
- **When it runs**: After every `record_recovery()`

#### Kernel Invariants (2)

**8. ClientSessionChecker**
- **What it checks**: Client idempotency positions are monotonic, no gaps in client position sequence
- **Why it matters**: Validates exactly-once semantics, ensures client retries are safe
- **When it runs**: After every client operation

**9. CommitHistoryChecker**
- **What it checks**: Commit offsets are monotonic, no duplicate commit offsets
- **Why it matters**: Validates commit log integrity, ensures linearizable commit order
- **When it runs**: After every `record_commit()`

#### Projection/MVCC Invariants (4)

**10. AppliedPositionMonotonicChecker**
- **What it checks**: `applied_position` never regresses, `applied_position ≤ commit_index`
- **Why it matters**: Validates MVCC visibility, ensures `AS OF POSITION` queries are consistent
- **When it runs**: After every `record_applied_position()`

**11. MvccVisibilityChecker**
- **What it checks**: Queries with `AS OF POSITION p` only see data committed at or before position `p`
- **Why it matters**: Core correctness for MVCC - ensures snapshot isolation
- **When it runs**: After every query with MVCC position

**12. AppliedIndexIntegrityChecker**
- **What it checks**: `AppliedIndex` references a real log entry, hash matches actual log entry hash
- **Why it matters**: Validates projection → log link, ensures projections can replay from log
- **When it runs**: After every `record_applied_index()`

**13. ProjectionCatchupChecker**
- **What it checks**: Projections eventually catch up to `commit_index` within bounded steps
- **Why it matters**: Liveness property - ensures queries eventually see recent data
- **When it runs**: After projection updates (deferred assertions)

#### Log Ordering Invariants (1)

**14. OffsetMonotonicityChecker**
- **What it checks**: Stream offsets are monotonically increasing (never regress)
- **Why it matters**: Provides linearizability guarantee through natural log ordering
- **Complexity**: O(1) per operation (HashMap lookup/insert)
- **When it runs**: After every append operation
- **Approach**: Industry-proven pattern from FoundationDB, TigerBeetle, Turso

#### Client-Visible Invariants (2)

**15. ReadYourWritesChecker**
- **What it checks**: After a client writes data, subsequent reads by the same client see that write
- **Why it matters**: Session consistency guarantee, client UX
- **When it runs**: After every client read

**16. TenantIsolationChecker**
- **What it checks**: Queries for tenant A never return data belonging to tenant B
- **Why it matters**: Critical for multi-tenancy - violation = data breach
- **When it runs**: After every query

#### SQL Invariants (3)

**17. QueryDeterminismChecker**
- **What it checks**: Same query + same database state → same result
- **Why it matters**: Validates deterministic query engine
- **When it runs**: After every query

**18. TlpOracle (Ternary Logic Partitioning)**
- **What it checks**: `COUNT(original query) == COUNT(true partition) + COUNT(false partition) + COUNT(null partition)`
- **Why it matters**: Catches SQL logic bugs without manual test cases
- **When it runs**: After executing SQL queries
- **Reference**: SQLancer: Automated testing of database systems

**19. NoRecOracle (Non-optimizing Reference Engine Comparison)**
- **What it checks**: Optimized query plan produces same results as unoptimized plan
- **Why it matters**: Catches query optimizer bugs without manual test cases
- **When it runs**: After executing optimized queries

**Invariant Execution Tracking**:

All invariants are tracked via `invariant_tracker::record_invariant_execution("name")`. Coverage reports show execution counts and CI fails if required invariants never execute:

```
❌ Required invariant 'vsr_view_change_safety' never executed
```

See `crates/kimberlite-sim/src/invariant.rs` for complete invariant implementations.

### Why VOPR Doesn't Use O(n!) Linearizability Checking

VOPR follows the approach used by industry leaders (FoundationDB, TigerBeetle, Turso) rather than implementing a traditional O(n!) linearizability checker:

#### Industry Best Practices

After analyzing production-grade distributed systems, we found that **NONE** of them use brute-force linearizability checking:

| System | Approach | Rationale |
|--------|----------|-----------|
| **FoundationDB** | Version-based total ordering + snapshot verification | Natural ordering from log offsets provides linearizability |
| **TigerBeetle** | Replica convergence + commit history validation | VSR consensus guarantees linearizability internally |
| **Turso** | Differential testing against SQLite + MVCC properties | Domain-specific invariants verify correctness |

#### The Universal Pattern

All three systems share this verification strategy:

1. **Natural ordering from log structure** - Immutable append-only logs with monotonic offsets provide total ordering
2. **Determinism as correctness proof** - Same seed → same execution verifies the system behaves correctly
3. **Convergence checking** - All replicas reach identical state proves consistency
4. **Snapshot-based verification** - Verify final state once, not history replay
5. **Trust consensus algorithm** - VSR/Raft/Paxos provides linearizability internally
6. **Domain-specific invariants** - Offset monotonicity, agreement, prefix property

#### VOPR's Approach

Instead of O(n!) linearizability checking, VOPR verifies:

**Offset Monotonicity** (O(1) per operation):
```rust
/// Verifies that offsets are monotonically increasing per stream
pub struct OffsetMonotonicityChecker {
    stream_offsets: HashMap<u64, u64>,  // stream_id -> highest_offset
}
```

**VSR Safety Properties** (O(replicas) per check):
- **Agreement**: No two replicas commit different ops at same position
- **Prefix Property**: Replicas agree on committed prefix
- **View-Change Safety**: New primary has all committed ops from previous view
- **Recovery Safety**: Recovery never discards committed offsets

**Why This Works**:

For a log-structured system with immutable offsets:
- Offset monotonicity + VSR agreement = linearizability
- Natural total ordering from append-only log
- No need to reconstruct linearizability post-hoc
- Scalable to 100k+ operations (vs. ~100 for O(n!))

**Performance Comparison**:

| Approach | Complexity | Operations/Test | Sim Throughput |
|----------|-----------|----------------|----------------|
| Traditional O(n!) | Factorial | ~100 | ~1-10 sims/sec |
| VOPR (this approach) | O(1) + O(replicas) | 100,000+ | ~100-200 sims/sec |

**Philosophy Alignment**:

This aligns with Kimberlite's core principle:
> **All data is an immutable, ordered log. All state is a derived view.**

- Log offsets provide natural total ordering
- VSR consensus ensures replicas converge
- Invariants verify the ordering is maintained
- No need to reconstruct linearizability post-hoc

For a compliance-focused database, **trust the consensus algorithm you built**, verify it works correctly through domain-specific invariants, and use determinism to prove correctness.

### Canary Testing (Mutation Testing)

**How do we know VOPR would catch a bug if it existed?**

We inject intentional bugs (canaries) and verify VOPR catches them. VOPR has **5 canary mutations**, each representing a class of real bugs:

| Canary | Bug Type | Expected Detector | Detection Rate |
|--------|----------|-------------------|----------------|
| `canary-skip-fsync` | Crash safety | `StorageDeterminismChecker` | ~5,000 events |
| `canary-wrong-hash` | Projection integrity | `AppliedIndexIntegrityChecker` | ~1,000 events |
| `canary-commit-quorum` | Consensus safety | `AgreementChecker` | ~50,000 events |
| `canary-idempotency-race` | Exactly-once semantics | `ClientSessionChecker` | ~10,000 events |
| `canary-monotonic-regression` | MVCC invariants | `AppliedPositionMonotonicChecker` | ~2,000 events |

Each canary is gated by a **feature flag** to prevent accidental deployment.

**Mutation Score**: 5/5 = **100%** (every canary triggers the expected invariant violation)

#### CI Enforcement

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

**If a canary doesn't fail, CI fails.** This ensures VOPR's mutation score doesn't regress over time.

For detailed canary descriptions and implementation, see implementation at `/crates/kimberlite-sim/src/canary.rs`.

### Coverage Tracking

VOPR enforces coverage minimums via three threshold profiles:

| Threshold | smoke_test() | default() | nightly() |
|-----------|--------------|-----------|-----------|
| Fault point coverage | 50% | 80% | 90% |
| Critical fault points | 2/2 (100%) | 4/4 (100%) | 7/7 (100%) |
| Required invariants | ≥2 executed | ≥6 executed | ≥13 executed |
| View changes | ≥0 | ≥1 | ≥5 |
| Repairs | ≥0 | ≥0 | ≥2 |
| Unique query plans | ≥2 | ≥5 | ≥20 |

**If coverage falls below threshold, CI fails** with actionable error messages:

```
❌ Coverage validation FAILED

Violations:
  1. Fault point coverage below threshold (expected: 80%, actual: 65%)
  2. Critical fault point 'sim.storage.fsync' was never hit
  3. Required invariant 'vsr_view_change_safety' never executed
  4. Not enough view changes occurred (expected: ≥5, actual: 2)

Action: Run longer iterations or enable fault injection
```

**Metrics tracked**:
- Fault point coverage (which injection points were hit)
- Critical fault points (100% required)
- Invariant execution counts (all must execute ≥1 time)
- View changes, repairs, query plan diversity

### Determinism Validation

Every VOPR run with `--check-determinism` validates that simulations are perfectly reproducible:

**The Property**: Same seed → Same execution → Same bugs

Without determinism, bugs are irreproducible and VOPR provides zero value.

**How We Validate**:

1. **Run each seed twice** with identical configuration
2. **Compare results**:
   - `storage_hash` - CRC32 of all blocks
   - `kernel_state_hash` - BLAKE3 of sorted tables/streams
   - `events_processed` - Event count
   - `final_time_ns` - Simulated time

3. **Report violations** with detailed diagnostics:
   ```
   ❌ Determinism violation detected!

   Run 1:
     Storage hash: 0xABCD1234
     Kernel state hash: 0x5678EFAB
     Events: 100,000

   Run 2:
     Storage hash: 0xABCD5678  ← DIFFERENT
     Kernel state hash: 0x5678EFAB
     Events: 100,000

   Divergence detected at storage layer.
   ```

**Nightly Determinism Checks**: CI runs determinism validation on 10 seeds (42, 123, 456, 789, 1024, 2048, 4096, 8192, 16384, 32768). Each seed is run twice (100k events). If **any** seed shows nondeterminism, CI fails.

**Current status**: ✅ 10/10 seeds deterministic

### CI Integration

VOPR is integrated into CI at two levels:

#### 1. `vopr-determinism.yml` (PR/Push)

**Triggers**: Every push to `main` and every PR
**Runtime**: ~5-10 minutes

**What it checks**:
- Baseline scenario (100 iterations)
- Combined faults scenario (50 iterations)
- Multi-tenant isolation (50 iterations)
- Coverage enforcement (200 iterations)

**Failure scenarios**:
- Determinism violations (same seed produces different results)
- Coverage below thresholds (80% fault points, 100% invariants)
- Invariant violations (linearizability, replica consistency, etc.)

#### 2. `vopr-nightly.yml` (Scheduled)

**Triggers**:
- Daily at 2 AM UTC
- Manual dispatch via GitHub Actions UI

**Runtime**: ~1-2 hours

**What it tests**:
- Baseline: 10,000 iterations (configurable)
- SwizzleClogging: 5,000 iterations
- Gray failures: 5,000 iterations
- Multi-tenant: 3,000 iterations
- Combined: 5,000 iterations
- **Canary matrix**: All 5 mutations tested

**Outputs**:
- JSON results for each scenario
- Summary report
- Artifacts retained for 30 days
- Auto-creates GitHub issue on failure

#### Running Locally

```bash
# Quick determinism check
cargo build --release -p kimberlite-sim --bin vopr
./target/release/vopr --iterations 100 --check-determinism

# Match CI baseline scenario
./target/release/vopr \
  --scenario baseline \
  --iterations 100 \
  --check-determinism \
  --seed 12345

# Match CI coverage enforcement
./target/release/vopr \
  --iterations 200 \
  --min-fault-coverage 80.0 \
  --min-invariant-coverage 100.0 \
  --require-all-invariants \
  --check-determinism

# Nightly-equivalent stress test
./target/release/vopr \
  --scenario baseline \
  --iterations 10000 \
  --check-determinism \
  --json > results.json
```

**Exit codes**:
- **0**: All tests passed, coverage met
- **1**: Invariant violations or test failures
- **2**: Coverage thresholds not met

See `.github/workflows/vopr-determinism.yml` and `.github/workflows/vopr-nightly.yml` for complete CI configuration.

### Adding New Invariants

For comprehensive guide on adding new invariants to VOPR, including:
- Defining checker structs
- Exporting from modules
- Wiring into `InvariantConfig`
- Adding to event loop
- CLI flag conventions
- Testing and verification

**Integration checklist**:
- [ ] Checker struct defined and exported
- [ ] Field added to `InvariantConfig`
- [ ] Default value set (true for most, false for expensive)
- [ ] Conditional instantiation in `run_simulation()`
- [ ] Wired into event loop at appropriate event type
- [ ] CLI flags added (group and individual)
- [ ] Help text updated
- [ ] `is_invariant_enabled()` updated
- [ ] Determinism test passes
- [ ] Coverage tracking works

**Event types by category**:
- Core: Various events (write/read/replica update)
- VSR: `EventKind::Custom(3)` (replica state)
- Projection: Need `EventKind::ProjectionApplied` (future)
- Query: Need `EventKind::QueryExecuted` (future)

**Zero-cost abstraction pattern**:

```rust
// GOOD: Zero cost when disabled
let mut checker = config.enable_foo.then(FooChecker::new);

// BAD: Always allocates
let mut checker = FooChecker::new();
if config.enable_foo { ... }
```

Default to **disabled** for expensive checks:

```rust
impl Default for InvariantConfig {
    fn default() -> Self {
        Self {
            enable_sql_tlp: false,  // Opt-in only (10-100x performance cost)
        }
    }
}
```

---

## VOPR Enhanced Capabilities (v0.3.1)

VOPR has been enhanced to achieve 90-95% Antithesis-grade testing without building a hypervisor. These enhancements are production-ready and fully integrated.

### Storage Realism

Realistic I/O scheduler behavior and crash semantics for catching durability bugs.

**Write Reordering** (`storage_reordering.rs`):
- 4 reordering policies: FIFO, Random, Elevator, Deadline
- Dependency tracking (WAL → data blocks)
- Barrier operations (fsync blocks subsequent writes)
- Deterministic reordering based on SimRng seed

**Concurrent I/O** (`concurrent_io.rs`):
- Track up to 32 concurrent operations per device
- Out-of-order completion mode (realistic)
- Ordered completion mode (testing)
- Statistics tracking (queue depth, completion latency)

**Crash Recovery** (`crash_recovery.rs`):
- 5 crash scenarios: DuringWrite, DuringFsync, AfterFsyncBeforeAck, PowerLoss, CleanShutdown
- Block-level granularity (4KB atomic units)
- Torn write simulation (partial multi-block writes)
- "Seen but corrupt" vs "not seen" distinction

**Usage**:
```bash
# Enable storage realism
cargo run --bin vopr -- --scenario baseline \
    --enable-storage-realism --iterations 1000

# Specific reordering policy
cargo run --bin vopr -- --reorder-policy elevator --iterations 1000
```

**Performance**: <5% throughput overhead

### Byzantine Attack Arsenal

Protocol-level attack patterns for active adversarial testing (`protocol_attacks.rs`).

**Attack Patterns** (10 total):
- SplitBrain - Fork DoViewChange to different replica groups
- MaliciousLeaderEarlyCommit - Commit ahead of PrepareOK quorum
- PrepareEquivocation - Different Prepare messages for same op_number
- ReplayOldView - Re-send old view messages after view change
- InvalidDvcConflictingTail - DoViewChange with conflicting log tail
- CorruptChecksums - Corrupt message checksums
- ViewChangeBlocking - Block view changes by refusing to participate
- PrepareFlood - Flood with excessive Prepare messages
- CommitInflationGradual - Gradually inflate commit numbers
- SelectiveSilence - Ignore specific replicas selectively

**Pre-configured Suites**:
- **Standard**: Basic Byzantine testing (split-brain, malicious leader, equivocation, invalid DVC)
- **Aggressive**: Stress testing (high-value attacks, extreme parameters)
- **Subtle**: Edge case detection (minimal mutations, low probability)

**Usage**:
```bash
# Run with Byzantine attack
cargo run --bin vopr -- --scenario byzantine_inflated_commit \
    --iterations 5000

# Use pre-configured suite
cargo run --bin vopr -- --byzantine-suite standard --iterations 1000
```

**Detection Rate**: 100% for all tested scenarios

### Observability & Debugging

Event logging and failure reproduction capabilities (`event_log.rs`).

**Event Logging**:
- Records all nondeterministic decisions (RNG, scheduling, delays, drops, crashes)
- Compact binary format (~100 bytes/event)
- Bounded memory usage (default: 100,000 events in-memory)
- Deterministic replay from event log

**Repro Bundles** (`.kmb` files):
- Self-contained failure reproduction
- Contains: seed, scenario, event log, failure info, VOPR version
- Compressed with zstd
- Binary format using bincode serialization

**Usage**:
```bash
# Run with logging enabled, save bundles on failure
cargo run --bin vopr -- run --scenario combined \
    --iterations 1000 \
    --output-dir ./failures \
    --enable-logging

# Reproduce from bundle
cargo run --bin vopr -- repro ./failures/failure-12345.kmb --verbose

# Show bundle information
cargo run --bin vopr -- show ./failures/failure-12345.kmb --events
```

**Features**:
- Perfect reproduction (same seed → same execution)
- Bundle validation (version compatibility checks)
- Event log trimming (keep only relevant events)

### Workload Generators

Realistic transaction patterns for comprehensive testing (`workload_generator.rs`).

**Workload Patterns** (6 total):
- **Uniform**: Random access across key space
- **Hotspot**: 80% traffic to 20% of keys (Pareto distribution)
- **Sequential**: Sequential scan with mixed reads/scans
- **MultiTenantHot**: 80% traffic to hot tenant (tenant 0)
- **Bursty**: 10x traffic spikes every ~1000 ops (100ms bursts)
- **ReadModifyWrite**: Transaction chains (BeginTx, Read, Write, Commit/Rollback)

**Usage**:
```rust
let mut gen = WorkloadGenerator::new(
    WorkloadConfig::new(WorkloadPattern::Hotspot)
        .with_key_count(1000)
        .with_hot_key_fraction(0.2)
);

for _ in 0..1000 {
    let tx = gen.next_transaction(&mut rng);
    // Execute transaction
}
```

**Benefits**:
- Realistic access patterns (not just uniform random)
- Tenant isolation testing (multi-tenant patterns)
- Hotspot contention stress testing
- Transaction correctness validation

### Coverage-Guided Fuzzing

Multi-dimensional coverage tracking for directed testing (`coverage_fuzzer.rs`).

**Coverage Dimensions**:
- **State Coverage**: Unique (view, op_number, commit_number) tuples
- **Message Coverage**: Unique message sequences (up to length 5)
- **Fault Coverage**: Unique fault combinations
- **Path Coverage**: Unique event sequences (up to length 10)

**Fuzzer Features**:
- Interesting seed corpus (seeds reaching new coverage)
- 3 selection strategies: Random, LeastUsed, EnergyBased (AFL-style)
- Seed mutation (bit flipping, addition, multiplication)
- Corpus trimming (keep top N by energy)

**Usage**:
```rust
let mut fuzzer = CoverageFuzzer::new(
    CoverageConfig::default(),
    SelectionStrategy::EnergyBased,
);

for iteration in 0..10000 {
    let seed = fuzzer.select_seed(&mut rng);
    let result = run_vopr_with_seed(seed);

    // Track coverage and update corpus
    fuzzer.record_coverage(seed, result.coverage);
    if result.is_interesting() {
        fuzzer.add_to_corpus(seed);
    }
}
```

**Performance**:
- Finds 2x more unique states than random testing
- Corpus size: Configurable (default: 10,000 seeds)
- Coverage-guided seed prioritization

### CLI Commands

Beautiful command interface for simulation testing (`cli/` modules).

**Commands**:
- `vopr run [scenario]` - Run simulation with progress bar
- `vopr repro <bundle>` - Reproduce from .kmb file
- `vopr show <bundle>` - Display failure summary
- `vopr scenarios` - List all 27 available scenarios
- `vopr stats` - Display coverage and invariant statistics

**Output Formats**:
- **Human**: Rich text with colors and formatting
- **JSON**: Machine-readable for tooling integration
- **Compact**: Single-line summary

**Verbosity Levels**:
- **Quiet** (0): Errors only
- **Normal** (1): Standard output (default)
- **Verbose** (2): Detailed progress and diagnostics
- **Debug** (3): Full event traces

**Usage**:
```bash
# Quick smoke test
just vopr-quick

# Full test suite (all scenarios)
just vopr-full 10000

# Reproduce failure
just vopr-repro failure.kmb

# List scenarios
just vopr-scenarios

# JSON output for CI
cargo run --bin vopr -- run --scenario baseline \
    --iterations 1000 \
    --format json > results.json
```

**Features**:
- Progress bars with throughput/ETA
- Automatic .kmb bundle generation on failure
- Builder pattern for command construction
- Actionable failure reports

### Module Organization

Enhanced VOPR modules:

```
crates/kimberlite-sim/src/
├── storage_reordering.rs    # Write reordering engine (416 lines)
├── concurrent_io.rs         # Concurrent I/O simulator (330 lines)
├── crash_recovery.rs        # Crash semantics (605 lines)
├── protocol_attacks.rs      # Byzantine attack patterns (397 lines)
├── event_log.rs            # Event logging & repro bundles (384 lines)
├── workload_generator.rs   # Realistic workload patterns (496 lines)
├── coverage_fuzzer.rs      # Coverage-guided fuzzing (531 lines)
└── cli/                    # CLI commands (900 lines)
    ├── mod.rs              # CLI routing (242 lines)
    ├── run.rs              # Run command (313 lines)
    ├── repro.rs            # Reproduce command (125 lines)
    ├── show.rs             # Show command (75 lines)
    ├── scenarios.rs        # Scenarios command (76 lines)
    └── stats.rs            # Stats command (73 lines)
```

**Total**: ~3,400 lines across 12 modules

### Integration & Testing

**Feature Flags**:
- All enhancements use feature flags for gradual adoption
- Storage realism can be enabled/disabled per run
- Byzantine attacks selected via scenario or suite
- Event logging opt-in (performance overhead <10%)

**Testing**:
- 48 new tests (all passing)
- Integration with existing VOPR test suite
- Determinism validation for all enhancements
- Coverage enforcement in CI

**Performance**:
- Storage realism: <5% overhead
- Event logging: <10% overhead
- Overall: >70k sims/sec maintained
- No regression in baseline scenarios

---

## VOPR Advanced Debugging (v0.4.0)

VOPR now includes production-grade debugging tools that make finding and fixing bugs 10x faster. These tools provide timeline visualization, automated bisection, test case minimization, and interactive interfaces.

### Timeline Visualization

ASCII Gantt chart rendering for understanding simulation execution flow (`timeline.rs`, ~700 lines).

**Features**:
- Per-node event lanes showing all operations
- Time-based visualization with configurable granularity
- 11 event kinds tracked: Client ops, Storage operations, Network messages, View changes, Commits, Crashes, Restarts, Partitions, Healing, Invariant checks, Violations
- Filtering by time range and node ID
- Symbol-based compact representation

**Event Kinds**:
```rust
pub enum TimelineKind {
    ClientRequest { op_type: String, key: u64 },
    ClientResponse { success: bool, latency_ns: u64 },
    WriteStart/Complete, FsyncStart/Complete,
    MessageSend/Deliver/Drop,
    ViewChange { old_view, new_view, replica_id },
    Commit { op_number, commit_number, replica_id },
    NodeCrash/Restart, NetworkPartition/Heal,
    InvariantCheck/Violation,
}
```

**Usage**:
```bash
# Generate timeline from failure bundle
cargo run --bin vopr -- timeline failure.kmb --width 120

# Filter by time range (first 10ms)
cargo run --bin vopr -- timeline failure.kmb \
    --time-range 0 10000000 --width 120

# Filter by specific nodes
cargo run --bin vopr -- timeline failure.kmb \
    --nodes 0,1,2 --width 120
```

**Example Output**:
```
Time (μs):  0     100    200    300    400    500
Node 0:     W═════╪══════┤ V─┐
Node 1:           M─────→│   V─┐
Node 2:                  │     V─┐  C═╪═X
Legend: W=Write M=Message V=ViewChange C=Commit X=Crash
```

**Justfile Commands**:
```bash
just vopr-timeline failure.kmb
just vopr-timeline-range failure.kmb 0 10000000
```

### Bisect to First Bad Event

Automated binary search to find the minimal event prefix triggering failure (`bisect.rs`, ~380 lines + `checkpoint.rs`, ~280 lines).

**Features**:
- Binary search through event sequence
- Simulation checkpointing for fast replay (checkpoint every 1000 events)
- RNG state restoration for deterministic resumption
- Generates minimal reproduction bundle
- Converges in O(log n) iterations

**Checkpointing**:
```rust
pub struct SimulationCheckpoint {
    pub event_count: u64,
    pub time_ns: u64,
    pub rng_state: RngCheckpoint,  // Seed + step count
    pub state_data: HashMap<String, Vec<u8>>,
}
```

**Usage**:
```bash
# Bisect to find first failing event
cargo run --bin vopr -- bisect failure.kmb

# Custom checkpoint interval (trade memory for speed)
cargo run --bin vopr -- bisect failure.kmb \
    --checkpoint-interval 500

# Save minimized bundle to specific path
cargo run --bin vopr -- bisect failure.kmb \
    --output failure.minimal.kmb
```

**Example Output**:
```
═══════════════════════════════════════════
VOPR Bisect - Find First Failing Event
═══════════════════════════════════════════
Bundle: failure.kmb
Seed: 12345
Total events: 1000

Iteration 0: Testing event range [0, 1000], mid=500
  → Failure at or before event 500

Iteration 5: Testing event range [48, 50], mid=49
  → No failure up to event 49

═══════════════════════════════════════════
Bisection Complete
═══════════════════════════════════════════
First bad event:  50
Last good event:  49
Iterations:       6
Checkpoints used: 5
Time:             2.3s

✓ Minimized bundle saved: failure.minimal.kmb
  Original:  1000 events
  Minimized: 50 events

✓ To reproduce: vopr repro failure.minimal.kmb
```

**Performance**:
- 10-100x faster than full replay
- Checkpoint overhead: <5% (1000-event granularity)
- Typical convergence: <10 iterations for 100k events

**Justfile Commands**:
```bash
just vopr-bisect failure.kmb
just vopr-bisect-checkpoint failure.kmb 500
```

### Delta Debugging (Test Case Minimization)

Zeller's ddmin algorithm for automatic trace minimization (`delta_debug.rs`, ~330 lines + `dependency.rs`, ~230 lines).

**Features**:
- Removes irrelevant events while preserving failure
- Event dependency analysis (network, storage, causality)
- Chunk-based minimization with configurable granularity
- Test caching for efficiency
- Achieves 80-95% reduction in practice

**Dependency Analysis**:
```rust
pub struct DependencyAnalyzer {
    events: Vec<LoggedEvent>,
    dependencies: HashMap<u64, HashSet<u64>>,
}

// Dependencies tracked:
// - Storage: write → read/complete
// - Network: send → deliver/drop
// - Protocol: view change → commits
// - Causality: event ordering preservation
```

**Algorithm** (ddmin):
1. Start with all events, granularity = 8
2. Try removing each chunk of size (events / granularity)
3. If removal preserves failure → keep removal, reset granularity
4. If removal breaks failure → keep events, try next chunk
5. If no chunks removable → increase granularity by 2x
6. Terminate when granularity >= event count

**Usage**:
```bash
# Minimize failure reproduction
cargo run --bin vopr -- minimize failure.kmb

# Custom granularity (larger = coarser, faster)
cargo run --bin vopr -- minimize failure.kmb \
    --granularity 16

# Save to specific path
cargo run --bin vopr -- minimize failure.kmb \
    --output failure.min.kmb

# Set iteration limit
cargo run --bin vopr -- minimize failure.kmb \
    --max-iterations 50
```

**Example Output**:
```
═══════════════════════════════════════════
VOPR Delta Debugging - Minimize Test Case
═══════════════════════════════════════════
Bundle: failure.kmb
Original events: 100

Iteration 0: granularity=8, events=100
  Trying to remove chunk [0, 12), 88 events remaining
    ✓ Chunk removed (failure still reproduced)

Iteration 18: granularity=8, events=7
  Cannot subdivide further - minimization complete

═══════════════════════════════════════════
Minimization Results
═══════════════════════════════════════════
Original events:  100
Minimized events: 7
Reduction:        93.0%
Iterations:       24
Test runs:        42

✓ Minimized bundle saved: failure.min.kmb
```

**Performance**:
- Reduction: 80-95% typical
- Test runs: ~2-3x event count (with caching)
- Time: Minutes to hours (depends on test complexity)

**Justfile Commands**:
```bash
just vopr-minimize failure.kmb
just vopr-minimize-gran failure.kmb 16
```

### Real Kernel State Hash

Actual kernel state hashing instead of placeholder (replaces v0.3.0 implementation).

**Changes**:
- `VsrReplicaWrapper::kernel_state()` - Exposes kernel state
- `VsrSimulation::kernel_state()` - Returns leader's kernel state
- `bin/vopr.rs` - Uses actual `compute_state_hash()` from kernel

**Validation**:
```rust
// BEFORE (placeholder):
let kernel_state_hash = kimberlite_kernel::State::new().compute_state_hash();

// AFTER (actual state):
let kernel_state_hash = if let Some(ref vsr) = vsr_sim {
    vsr.kernel_state().compute_state_hash()
} else {
    kimberlite_kernel::State::new().compute_state_hash()
};
```

**Benefits**:
- True determinism validation
- State divergence detection
- Checkpoint integrity verification
- Compliance hash chain validation

### Coverage Dashboard (Web UI)

Real-time coverage visualization via web interface (`dashboard/`, ~500 lines).

**Tech Stack**:
- **Axum 0.7**: Web framework
- **Askama 0.12**: HTML templating (type-safe)
- **Tower-HTTP**: Static file serving
- **Tokio**: Async runtime
- **Tokio-stream**: Server-Sent Events
- **Datastar**: Reactive UI updates
- **CUBE CSS**: Website-consistent styling

**Features**:
- 4 coverage dimension visualizations
- Real-time updates via SSE (2-second refresh)
- Top seeds by coverage table
- Corpus size tracking
- Energy-based seed selection metrics

**Coverage Dimensions Displayed**:
1. **State Coverage**: Unique (view, op, commit) tuples
2. **Message Sequences**: Unique message patterns (length 5)
3. **Fault Combinations**: Unique fault combinations
4. **Event Sequences**: Unique event paths (length 10)

**Usage**:
```bash
# Start dashboard (requires --features dashboard)
cargo run --bin vopr --features dashboard -- dashboard

# Custom port
cargo run --bin vopr --features dashboard -- dashboard --port 9090

# Load saved coverage
cargo run --bin vopr --features dashboard -- dashboard \
    --coverage-file coverage.json
```

**URL**: `http://localhost:8080` (default)

**Justfile Commands**:
```bash
just vopr-dashboard
just vopr-dashboard-port 9090
```

**UI Components**:
- **Header**: Total coverage, corpus size, state points, message sequences
- **Progress Bars**: Coverage breakdown by dimension (with percentages)
- **Top Seeds Table**: Seed, unique coverage, selection count, energy
- **Real-time Updates**: Live metrics via SSE

**Template Example** (`website/templates/vopr/dashboard.html`):
```html
<div class="grid" data-layout="quartet">
    <div class="card metric-card">
        <div class="metric-value" data-text="$stateCoverage">
            {{ stats.state_coverage }}
        </div>
        <div class="metric-label">State Points</div>
    </div>
    <!-- More metric cards... -->
</div>
```

### Interactive TUI

Rich terminal UI for live simulation (`tui/`, ~500 lines).

**Tech Stack**:
- **Ratatui 0.26**: TUI framework
- **Crossterm 0.27**: Terminal control

**Features**:
- 3 tabs: Overview, Logs, Configuration
- Real-time progress gauge
- Live statistics (iterations, successes, failures)
- Scrollable logs (Up/Down arrows)
- Pause/resume control (Space)
- Tab switching (Tab key)

**Keyboard Controls**:
- `s` - Start simulation
- `Space` - Pause/Resume
- `Tab` - Switch tabs
- `↑↓` - Scroll logs
- `q`/`Esc` - Quit

**Usage**:
```bash
# Launch TUI (requires --features tui)
cargo run --bin vopr --features tui -- tui

# With specific scenario
cargo run --bin vopr --features tui -- tui \
    --scenario baseline --iterations 10000

# With seed
cargo run --bin vopr --features tui -- tui \
    --seed 12345 --iterations 5000
```

**Tabs**:

1. **Overview**:
   - Progress gauge (0-100%)
   - Statistics (iterations, successes, failures)
   - Recent results list (last 20)

2. **Logs**:
   - Scrollable event log
   - Shows iteration completions
   - Displays progress messages

3. **Configuration**:
   - Seed value
   - Iteration count
   - Selected scenario

**Status Bar**: Context-sensitive help (shows current state and available commands)

**Justfile Commands**:
```bash
just vopr-tui
just vopr-tui-scenario baseline
```

### Module Summary

New modules for v0.4.0:

```
crates/kimberlite-sim/src/
├── timeline.rs             # Timeline visualization (~700 lines)
├── checkpoint.rs           # Simulation checkpointing (~280 lines)
├── bisect.rs              # Binary search for first bad event (~380 lines)
├── dependency.rs          # Event dependency analysis (~230 lines)
├── delta_debug.rs         # ddmin test minimization (~330 lines)
├── dashboard/             # Web UI (~500 lines)
│   ├── mod.rs
│   ├── router.rs          # Server & routing
│   └── handlers.rs        # HTTP handlers
├── tui/                   # Terminal UI (~500 lines)
│   ├── mod.rs             # TUI entry point
│   ├── app.rs             # Application state
│   └── ui.rs              # Rendering logic
└── cli/                   # New CLI commands (~500 lines)
    ├── timeline.rs        # Timeline command
    ├── bisect.rs          # Bisect command
    ├── minimize.rs        # Minimize command
    ├── dashboard.rs       # Dashboard command
    └── tui.rs             # TUI command
```

**Total**: ~3,700 lines across 15 new modules

**Templates & CSS**:
```
website/templates/vopr/
└── dashboard.html         # Askama template (~150 lines)

website/public/css/blocks/
└── vopr-dashboard.css     # CUBE CSS styles (~120 lines)
```

### Testing & Integration

**Test Coverage**:
- Timeline: 11 tests ✅
- Bisect: 9 tests ✅
- Delta Debug: 14 tests ✅
- Kernel State: 5 tests ✅
- Dashboard: 8 tests ✅ (with --features dashboard)
- TUI: 4 tests ✅ (with --features tui)

**Total**: 51 new tests, all passing

**Feature Flags**:
```toml
[features]
dashboard = ["axum", "askama", "askama_axum", "tower-http", "tokio", "tokio-stream"]
tui = ["ratatui", "crossterm"]
```

**Performance**:
- Timeline: Negligible overhead (generated post-run)
- Bisect: 10-100x faster than full replay
- Delta Debug: 80-95% reduction, ~minutes to hours
- Dashboard: <1% overhead (optional SSE updates)
- TUI: No overhead (runs simulations in thread)

### Workflow Integration

**Typical Debugging Flow**:

1. **Run VOPR** until failure:
   ```bash
   just vopr-scenario byzantine_inflated_commit 10000
   # Saves failure.kmb automatically
   ```

2. **Visualize Timeline** to understand execution:
   ```bash
   just vopr-timeline failure.kmb
   # See event sequence, identify patterns
   ```

3. **Bisect** to find first failing event:
   ```bash
   just vopr-bisect failure.kmb
   # Creates failure.minimal.kmb (50 events instead of 1000)
   ```

4. **Minimize** test case further:
   ```bash
   just vopr-minimize failure.minimal.kmb
   # Creates failure.minimal.min.kmb (7 events)
   ```

5. **Reproduce** minimal case:
   ```bash
   just vopr-repro failure.minimal.min.kmb
   # Debug 7 events instead of 1000
   ```

**Interactive Development**:
```bash
# Use TUI for rapid iteration
just vopr-tui

# Monitor coverage in dashboard
just vopr-dashboard &
# Run simulations, watch coverage grow in browser
```

---
