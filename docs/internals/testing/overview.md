---
title: "Testing Strategy"
section: "internals/testing"
slug: "overview"
order: 1
---

# Testing Strategy

Kimberlite is a compliance-critical system. Our testing strategy prioritizes finding bugs that could compromise data integrity, consensus correctness, or audit trail reliability. This document describes our approach, inspired by TigerBeetle's deterministic simulation testing.

---

## Table of Contents

1. [Philosophy](#philosophy)
2. [Testing Pyramid](#testing-pyramid)
3. [Deterministic Simulation Testing (DST)](#deterministic-simulation-testing-dst)
4. [VOPR Architecture](#vopr-architecture)
5. [Assertion Strategy](#assertion-strategy)
6. [Property-Based Testing](#property-based-testing)
7. [Integration Testing](#integration-testing)
8. [Running Tests](#running-tests)
9. [Debugging Failures](#debugging-failures)

---

## Philosophy

### Test the Implementation, Not a Model

We test the actual production code, not a simplified model of it:

- **TLA+ is for design**: Formal specifications help us think, but they don't find implementation bugs
- **Simulation tests real code**: Our simulator runs the actual consensus and storage code
- **No mocks in the core**: The kernel and consensus layers use real implementations, not test doubles

### Simulation > Formal Proofs for Bug Finding

TigerBeetle's experience shows that deterministic simulation testing finds more bugs than formal methods alone:

- Formal proofs verify the algorithm is correct
- Simulation finds the bugs in the implementation of that algorithm
- Most bugs are in edge cases: recovery, network partitions, disk failures

### Assertions Are Safety Nets

Assertions catch bugs early, but they're not a substitute for understanding:

```rust
// Good: Assertion documents and checks invariant
fn apply_committed(state: &mut State, entry: LogEntry) {
    debug_assert!(entry.position == state.commit_index + 1,
        "gap in committed entries: expected {}, got {}",
        state.commit_index + 1, entry.position);
    // ...
}

// Bad: Assertion without understanding
fn apply_committed(state: &mut State, entry: LogEntry) {
    assert!(entry.is_valid());  // What does "valid" mean here?
    // ...
}
```

---

## Testing Pyramid

Our testing strategy uses multiple layers:

```
                    ┌───────────────┐
                    │  Simulation   │  VOPR: Full cluster under faults
                    │   (DST)       │  Hours of simulated time
                    └───────┬───────┘
                            │
                    ┌───────┴───────┐
                    │   Property    │  Proptest: Randomized invariant checking
                    │    Tests      │  Hundreds of cases per test
                    └───────┬───────┘
                            │
            ┌───────────────┴───────────────┐
            │       Integration Tests       │  Multi-component, real I/O
            │                               │  Happy paths + edge cases
            └───────────────┬───────────────┘
                            │
    ┌───────────────────────┴───────────────────────┐
    │                  Unit Tests                    │  Single functions
    │                                               │  Fast, deterministic
    └───────────────────────────────────────────────┘
```

### Time Investment

| Layer | % of Tests | Run Time | When to Run |
|-------|------------|----------|-------------|
| Unit | 60% | Milliseconds | Every save |
| Integration | 20% | Seconds | Pre-commit |
| Property | 15% | Minutes | CI |
| Simulation | 5% | Hours | Nightly/Weekly |

---

## Deterministic Simulation Testing (DST)

DST is our primary tool for testing consensus and replication. It allows us to:

1. **Run thousands of nodes** in a single process
2. **Inject faults** precisely and reproducibly
3. **Control time** to test timeouts and leader election
4. **Reproduce failures** with seeds

### Why Deterministic?

A test is deterministic if, given the same inputs, it produces the same outputs. For simulation testing, this means:

- **Same seed → Same execution**: Every message, fault, and timeout happens identically
- **Reproducible bugs**: A failing seed always fails the same way
- **Debuggable**: Step through the exact sequence that caused failure

### How It Works

The simulator replaces all sources of non-determinism:

```rust
// Production code uses traits for external dependencies
trait Clock {
    fn now(&self) -> Timestamp;
}

trait Network {
    fn send(&self, to: NodeId, msg: Message);
    fn recv(&self) -> Option<(NodeId, Message)>;
}

trait Storage {
    fn write(&self, offset: u64, data: &[u8]) -> io::Result<()>;
    fn read(&self, offset: u64, len: usize) -> io::Result<Vec<u8>>;
}

// Simulator provides deterministic implementations
struct SimulatedClock {
    current_time: u64,
}

struct SimulatedNetwork {
    messages: VecDeque<(Timestamp, NodeId, NodeId, Message)>,
    rng: StdRng,  // Seeded RNG for delays
}

struct SimulatedStorage {
    data: HashMap<u64, Vec<u8>>,
    pending_failures: Vec<FaultSpec>,
}
```

---

## VOPR Architecture

VOPR (Kimberlite OPerations Randomizer) is our deterministic simulator, inspired by TigerBeetle's VOPR.

### Components

```
┌─────────────────────────────────────────────────────────────────┐
│                           VOPR                                   │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │                      Supervisor                             │ │
│  │  - Drives simulation clock                                  │ │
│  │  - Schedules faults                                         │ │
│  │  - Runs checkers                                            │ │
│  └────────────────────────────────────────────────────────────┘ │
│                              │                                   │
│              ┌───────────────┼───────────────┐                  │
│              ▼               ▼               ▼                  │
│  ┌──────────────────┐ ┌──────────────┐ ┌──────────────┐        │
│  │  Simulated Node  │ │  Simulated   │ │  Simulated   │        │
│  │       0          │ │    Node 1    │ │   Node 2     │        │
│  │                  │ │              │ │              │        │
│  │  ┌────────────┐  │ │ ┌──────────┐ │ │ ┌──────────┐ │        │
│  │  │  Runtime   │  │ │ │ Runtime  │ │ │ │ Runtime  │ │        │
│  │  └────────────┘  │ │ └──────────┘ │ │ └──────────┘ │        │
│  │  ┌────────────┐  │ │ ┌──────────┐ │ │ ┌──────────┐ │        │
│  │  │   Kernel   │  │ │ │  Kernel  │ │ │ │  Kernel  │ │        │
│  │  └────────────┘  │ │ └──────────┘ │ │ └──────────┘ │        │
│  │  ┌────────────┐  │ │ ┌──────────┐ │ │ ┌──────────┐ │        │
│  │  │  Storage   │  │ │ │ Storage  │ │ │ │ Storage  │ │        │
│  │  └────────────┘  │ │ └──────────┘ │ │ └──────────┘ │        │
│  └──────────────────┘ └──────────────┘ └──────────────┘        │
│                              │                                   │
│              ┌───────────────┴───────────────┐                  │
│              ▼                               ▼                  │
│  ┌──────────────────────┐   ┌──────────────────────────────┐   │
│  │  Simulated Network   │   │    Simulated Time            │   │
│  │  - Message queue     │   │    - Discrete events         │   │
│  │  - Partition faults  │   │    - Timeout scheduling      │   │
│  │  - Delay injection   │   │    - Deterministic ordering  │   │
│  └──────────────────────┘   └──────────────────────────────┘   │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │                     Fault Injector                          │ │
│  │  - Node crashes                  - Message corruption       │ │
│  │  - Network partitions            - Bit flips in storage     │ │
│  │  - Message reordering            - Slow disks               │ │
│  │  - Message drops                 - Full disks               │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │                     Invariant Checkers                      │ │
│  │  - Offset monotonicity           - Hash chain integrity     │ │
│  │  - Log consistency               - MVCC correctness         │ │
│  │  - Replica convergence           - Projection consistency   │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Fault Types

VOPR can inject various fault types, including advanced patterns inspired by FoundationDB and TigerBeetle.

**Network Faults**:
```rust
enum NetworkFault {
    /// Drop a specific message
    DropMessage { from: NodeId, to: NodeId },

    /// Partition nodes into groups that can't communicate
    Partition { groups: Vec<Vec<NodeId>> },

    /// Delay messages by a random amount
    Delay { min_ms: u64, max_ms: u64 },

    /// Reorder messages (deliver out of send order)
    Reorder,

    /// Duplicate a message
    Duplicate { from: NodeId, to: NodeId },

    /// Corrupt message contents
    Corrupt { bit_flip_probability: f64 },

    /// Swizzle-clog: randomly clog/unclog network to specific nodes
    /// Inspired by FoundationDB's trillion CPU-hour testing
    SwizzleClog {
        /// Nodes to clog (messages queued but not delivered)
        clogged_nodes: Vec<NodeId>,
        /// How long to maintain the clog
        duration_ms: u64,
    },
}
```

**Storage Faults**:
```rust
enum StorageFault {
    /// Fail a write operation
    WriteFailure { offset: u64 },

    /// Fail a read operation
    ReadFailure { offset: u64 },

    /// Return corrupted data on read
    Corruption { offset: u64, bit_flip_probability: f64 },

    /// Simulate slow disk (delay I/O)
    SlowDisk { delay_ms: u64 },

    /// Simulate full disk (writes fail with ENOSPC)
    DiskFull,

    /// Partial write (write less than requested)
    PartialWrite { max_bytes: usize },
}
```

**Node Faults**:
```rust
enum NodeFault {
    /// Node crashes and restarts with persistent state
    CrashRestart,

    /// Node crashes and restarts with clean state
    CrashRecover,

    /// Node hangs (stops processing but doesn't crash)
    Hang { duration_ms: u64 },

    /// Node becomes slow (processes at reduced speed)
    Slow { factor: f64 },
}
```

**Gray Failures** (TigerBeetle-inspired):

Gray failures are partial failures that are harder to detect than complete crashes:

```rust
enum GrayFailure {
    /// Node responds slowly (simulates overloaded node)
    SlowResponses {
        /// Delay factor (2.0 = 2x normal latency)
        delay_factor: f64,
    },

    /// Writes partially succeed (simulates disk issues)
    PartialWrites {
        /// Probability that any write succeeds
        success_rate: f64,
    },

    /// Network intermittently available
    IntermittentNetwork {
        /// Probability network is available at any moment
        availability: f64,
    },

    /// Node processes some messages but drops others
    SelectiveProcessing {
        /// Message types to drop
        dropped_types: Vec<MessageType>,
    },
}
```

Gray failures are particularly dangerous because:
- Nodes appear healthy (respond to heartbeats)
- Timeouts may not trigger (responses arrive, just slowly)
- State can diverge subtly over time

### Invariant Checkers

After each step, VOPR runs invariant checks:

```rust
trait InvariantChecker {
    /// Check invariant, return error if violated
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation>;
}

/// All committed entries must be identical across replicas
struct LogConsistencyChecker;

impl InvariantChecker for LogConsistencyChecker {
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation> {
        let commit_index = state.min_commit_index();

        for i in 0..=commit_index {
            let entries: Vec<_> = state.nodes
                .iter()
                .map(|n| n.log.get(i))
                .collect();

            // All non-None entries at position i must be identical
            let first = entries.iter().find_map(|e| e.as_ref());
            for entry in &entries {
                if let Some(e) = entry {
                    if Some(e) != first {
                        return Err(InvariantViolation::LogDivergence {
                            position: i,
                            entries: entries.clone(),
                        });
                    }
                }
            }
        }

        Ok(())
    }
}

/// Projections must match log contents
struct ProjectionConsistencyChecker;

/// Hash chain must be valid
struct HashChainChecker;

/// Byte-for-byte replica comparison (TigerBeetle-inspired)
/// Verifies that all caught-up replicas have identical storage
struct ByteIdenticalReplicaChecker;

impl InvariantChecker for ByteIdenticalReplicaChecker {
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation> {
        // Find replicas that are caught up (same commit index)
        let max_commit = state.nodes.iter()
            .map(|n| n.commit_index)
            .max()
            .unwrap_or(0);

        let caught_up: Vec<_> = state.nodes.iter()
            .filter(|n| n.commit_index == max_commit)
            .collect();

        if caught_up.len() < 2 {
            return Ok(()); // Need at least 2 replicas to compare
        }

        // Compare storage byte-for-byte
        let reference = &caught_up[0].storage;
        for replica in &caught_up[1..] {
            if replica.storage.as_bytes() != reference.as_bytes() {
                return Err(InvariantViolation::ReplicaDivergence {
                    commit_index: max_commit,
                    replicas: caught_up.iter().map(|n| n.id).collect(),
                });
            }
        }

        Ok(())
    }
}
```

### Swizzle-Clogging Tests

Swizzle-clogging (from FoundationDB) randomly clogs and unclogs network connections to find partition edge cases:

```rust
/// Swizzle-clogger randomly blocks/unblocks network to nodes
pub struct SwizzleClogger {
    rng: StdRng,
    clogged: HashSet<NodeId>,
}

impl SwizzleClogger {
    /// Clog a random subset of nodes
    pub fn clog_random_subset(&mut self, nodes: &[NodeId], count: usize) {
        let selected: Vec<_> = nodes.choose_multiple(&mut self.rng, count).collect();
        for node in selected {
            self.clogged.insert(*node);
        }
    }

    /// Unclog nodes in random order (not necessarily FIFO)
    pub fn unclog_random_order(&mut self) {
        let to_unclog: Vec<_> = self.clogged.iter().cloned().collect();
        for node in to_unclog.choose_multiple(&mut self.rng, self.rng.gen_range(1..=to_unclog.len())) {
            self.clogged.remove(node);
        }
    }

    /// Check if node is clogged
    pub fn is_clogged(&self, node: NodeId) -> bool {
        self.clogged.contains(&node)
    }
}
```

**What swizzle-clogging finds**:
- Race conditions during partition healing
- View change edge cases when leader becomes reachable
- Message ordering bugs when clogged messages arrive in bursts
- Timeout tuning issues

### Enhanced Fault Categories

VOPR distinguishes between different types of storage faults for Protocol-Aware Recovery (PAR):

```rust
/// Prepare status for PAR protocol
pub enum PrepareStatus {
    /// This prepare was never received by this replica
    NotSeen,

    /// Prepare was received and has valid checksum
    Seen(Checksum),

    /// Prepare was received but checksum validation failed
    Corrupt,
}
```

**PAR Truncation Rule**: A prepare can only be truncated if 4+ of 6 replicas report `NotSeen`. This prevents truncating prepares that might have been committed (if a replica has `Seen` or `Corrupt`, the prepare might be committed).

```rust
fn can_safely_truncate(prepare_id: PrepareId, statuses: &[PrepareStatus]) -> bool {
    let not_seen_count = statuses.iter()
        .filter(|s| matches!(s, PrepareStatus::NotSeen))
        .count();

    // Require 4+ replicas to confirm prepare was never seen
    // (with 6 replicas, this means at most 2 might have seen it,
    // which is below commit quorum of 4)
    not_seen_count >= 4
}
```

### Time Compression

VOPR uses simulated time with compression ratios of 10:1 or higher:

```rust
pub struct SimulatedTime {
    /// Current simulated time in nanoseconds
    current: u64,
    /// Compression ratio (10 = 10x faster than real time)
    compression_ratio: u64,
}

impl SimulatedTime {
    /// Advance time by the given duration
    pub fn advance(&mut self, duration: Duration) {
        self.current += duration.as_nanos() as u64 / self.compression_ratio;
    }

    /// Sleep until the next scheduled event
    pub fn sleep_until_next_event(&mut self, scheduler: &EventScheduler) {
        if let Some(next) = scheduler.peek_next_time() {
            self.current = next;
        }
    }
}
```

Time compression allows testing hours of simulated operation in minutes of wall-clock time.

### Model Verification

VOPR's model verification ensures that the simulated database state matches expected values even under extreme fault injection. This validates read-your-writes semantics and durability guarantees.

**How It Works**:
- Maintains an in-memory model (`KimberliteModel`) tracking both pending (unfsynced) and durable (fsynced) writes
- After each operation, compares actual storage state against model expectations
- Verifies that reads match what was written, even with write reordering, fsync failures, and crashes

**Key Features**:
- **Read-your-writes guarantee**: Maintained even with write reordering enabled
- **Fsync failure handling**: Pending writes cleared from model when fsync fails, matching storage behavior
- **Checkpoint recovery**: Full crash/recovery cycle testing with state synchronization
- **Strict verification**: Zero tolerance for inconsistencies, aligning with Kimberlite's compliance focus

**Alignment with Formal Specs**:
The model verification directly tests assumptions made in `specs/tla/Recovery.tla`:
- Committed entries persist through crashes (Recovery.tla:108)
- Uncommitted entries may be lost on fsync failure (Recovery.tla:112)
- Recovery protocol restores committed state from quorum (Recovery.tla:118-199)

**Example Verification**:
```rust
// After a write succeeds, record it in the model
model.apply_pending_write(key, value);

// Later, when reading:
let actual = storage.read(key, &mut rng);
assert!(model.verify_read(key, actual),
    "Storage returned {actual:?} but model expected {:?}",
    model.get(key));
```

**Performance**: Model verification adds <0.1% overhead while catching critical bugs that would otherwise manifest as data loss or corruption in production.

### Running VOPR

```bash
# Run simulation with random seed
cargo run --bin vopr

# Run with specific seed (for reproduction)
cargo run --bin vopr -- --seed 12345678

# Run for longer (default: 1000 operations)
cargo run --bin vopr -- --operations 100000

# Run with more aggressive faults
cargo run --bin vopr -- --fault-probability 0.1

# Run continuously, report statistics
cargo run --bin vopr -- --continuous --report-interval 60
```

### VOPR Predefined Scenarios

VOPR includes 27 predefined test scenarios across 5 categories:

```bash
# List all available scenarios
cargo run --bin vopr -- --list-scenarios

# Run a specific scenario
cargo run --bin vopr -- --scenario baseline           # Clean (no faults)
cargo run --bin vopr -- --scenario byzantine_dvc_tail_length_mismatch
cargo run --bin vopr -- --scenario corruption_bit_flip
cargo run --bin vopr -- --scenario crash_during_commit
```

**Scenario Categories**:

| Category | Count | Description |
|----------|-------|-------------|
| **Byzantine Attacks** | 5 | Protocol-level Byzantine mutations testing VSR handler validation |
| **Corruption Detection** | 3 | Bit flips, checksum validation, silent disk failures |
| **Recovery & Crashes** | 3 | Crash during commit/view change, recovery with corrupt log |
| **Gray Failures** | 2 | Slow disk I/O, intermittent network partitions |
| **Race Conditions** | 2 | Concurrent view changes, commit during DoViewChange |
| **Network & General** | 12 | Original scenarios (baseline, swizzle-clogging, multi-tenant, etc.) |

**High-Priority Byzantine Attack Scenarios** (added in v0.2.0):

| Scenario | Bug Tested | Expected Behavior |
|----------|------------|-------------------|
| `byzantine_dvc_tail_length_mismatch` | Bug 3.1 | Reject DoViewChange with log_tail length ≠ claimed ops |
| `byzantine_dvc_identical_claims` | Bug 3.3 | Deterministic tie-breaking via checksum → replica ID |
| `byzantine_oversized_start_view` | Bug 3.4 | Reject StartView with >10k log entries (DoS protection) |
| `byzantine_invalid_repair_range` | Bug 3.5 | Reject RepairRequest with invalid ranges |
| `byzantine_invalid_kernel_command` | Bug 3.2 | Gracefully handle Byzantine commands during commit |

**Running Comprehensive Validation**:

```bash
# Byzantine attack scenarios (10k iterations each)
for scenario in byzantine_dvc_tail_length_mismatch \
                byzantine_dvc_identical_claims \
                byzantine_oversized_start_view \
                byzantine_invalid_repair_range \
                byzantine_invalid_kernel_command; do
    cargo run --release --bin vopr -- \
        --scenario $scenario \
        --iterations 10000 \
        --json > results/${scenario}.json
done

# Corruption detection scenarios (5k iterations each)
for scenario in corruption_bit_flip \
                corruption_checksum_validation \
                corruption_silent_disk_failure; do
    cargo run --release --bin vopr -- \
        --scenario $scenario \
        --iterations 5000 \
        --json > results/${scenario}.json
done

# Long-running fuzzing campaign (1M iterations)
cargo run --release --bin vopr -- \
    --scenario combined \
    --iterations 1000000 \
    --json > results/long_fuzzing_1M.json
```

**Validation Results** (v0.2.0):
- Total scenarios: 27 (up from 12)
- Iterations tested: 1M+ across all scenarios
- Invariant violations: 0
- Byzantine rejections: Working correctly (instrumented and verified)

See `docs-internal/vopr/scenarios.md` for detailed configuration and usage examples for all 46 scenarios.

---

## Assertion Strategy

Assertions are our first line of defense against bugs.

### Assertion Density Goal

**Every function should have at least 2 assertions**: one precondition and one postcondition.

```rust
fn write_record(log: &mut Log, record: &Record) -> LogPosition {
    // Precondition: record must be valid
    assert!(record.checksum == crc32(&record.data),
        "record has invalid checksum");

    // Precondition: log must be writable
    assert!(!log.is_sealed(),
        "cannot write to sealed log");

    let position = log.append(record);

    // Postcondition: position must be sequential
    assert!(position == log.last_position,
        "write returned non-sequential position");

    // Postcondition: record must be readable
    debug_assert!(log.read(position).is_ok(),
        "written record not immediately readable");

    position
}
```

### Paired Assertions

Write assertions in pairs—one at the write site, one at the read site:

```rust
// Write site
fn commit_entry(log: &mut Log, entry: &Entry) {
    // Compute hash chain
    let prev_hash = log.last_hash();
    let hash = sha256(&[prev_hash.as_bytes(), &entry.to_bytes()]);

    // Write with assertion
    assert!(entry.hash == hash, "entry hash mismatch at write site");
    log.append(entry);
}

// Read site
fn read_entry(log: &Log, position: LogPosition) -> Entry {
    let entry = log.get(position).expect("entry must exist");
    let prev_hash = if position == 0 {
        Hash::zero()
    } else {
        log.get(position - 1).expect("prev entry must exist").hash
    };

    // Paired assertion
    let expected_hash = sha256(&[prev_hash.as_bytes(), &entry.to_bytes()]);
    assert!(entry.hash == expected_hash,
        "hash chain broken at position {}", position);

    entry
}
```

### Compound Assertions

Split compound conditions for better error messages:

```rust
// Bad: Compound assertion
assert!(entry.position == expected && entry.term == current_term);

// Good: Split assertions
assert!(entry.position == expected,
    "position mismatch: expected {}, got {}", expected, entry.position);
assert!(entry.term == current_term,
    "term mismatch: expected {}, got {}", current_term, entry.term);
```

### Debug vs Release

- `assert!()`: Critical invariants, always checked
- `debug_assert!()`: Expensive checks, debug builds only

```rust
// Always check: corruption would be catastrophic
assert!(record.checksum == crc32(&record.data));

// Debug only: O(n) validation too expensive for production
debug_assert!(log.entries.windows(2).all(|w| w[0].position < w[1].position));
```

### Production Assertions (38 Promoted)

As part of our VSR hardening initiative, we promoted 38 critical `debug_assert!()` calls to production `assert!()` for runtime safety enforcement.

**Categories**:
- **Cryptography (25)**: All-zero detection, key hierarchy integrity, ciphertext validation
- **Consensus (9)**: Leader-only operations, view/commit monotonicity, quorum validation
- **State Machine (4)**: Stream existence, effect counts, offset monotonicity

**Why Production Assertions**:
- Detect corruption BEFORE it propagates
- Catch Byzantine attacks in real-time
- Provide forensic evidence of failure mode
- Negligible performance impact (<0.1% throughput regression)

**Testing**: Every assertion has a corresponding `#[should_panic]` test in `crates/kimberlite-crypto/src/tests_assertions.rs`.

**Performance Impact**:
- Throughput: <0.1% regression
- p99 latency: +1μs
- p50 latency: <1μs

See `docs/ASSERTIONS.md` for complete guide on production assertion strategy.

**Example Test**:
```rust
#[test]
#[should_panic(expected = "encryption key is all zeros")]
fn test_encryption_key_rejects_all_zeros() {
    let zero_key = EncryptionKey([0u8; KEY_LENGTH]);
    encrypt(b"secret", &zero_key);  // Should panic
}
```

---

## Property-Based Testing

We use `proptest` for randomized invariant checking.

### Approach

Property tests generate random inputs and verify that invariants hold:

```rust
use proptest::prelude::*;

proptest! {
    /// Any sequence of operations should maintain log invariants
    #[test]
    fn log_invariants_hold(ops in prop::collection::vec(log_op_strategy(), 0..100)) {
        let mut log = Log::new_in_memory();

        for op in ops {
            match op {
                LogOp::Append(record) => {
                    let result = log.append(&record);
                    prop_assert!(result.is_ok());
                }
                LogOp::Read(position) => {
                    if position < log.len() {
                        let result = log.read(position);
                        prop_assert!(result.is_ok());
                    }
                }
            }
        }

        // Invariant: hash chain must be valid
        prop_assert!(log.verify_hash_chain().is_ok());

        // Invariant: all records must be readable
        for i in 0..log.len() {
            prop_assert!(log.read(i).is_ok());
        }
    }
}

fn log_op_strategy() -> impl Strategy<Value = LogOp> {
    prop_oneof![
        any::<Vec<u8>>().prop_map(|data| LogOp::Append(Record::new(data))),
        any::<u64>().prop_map(LogOp::Read),
    ]
}
```

### What to Property Test

| Component | Properties |
|-----------|------------|
| Log | Hash chain integrity, sequential positions, CRC validity |
| B+Tree | Sorted order, balanced height, key uniqueness |
| MVCC | Version visibility, no phantom reads |
| Consensus | Agreement, validity, termination |

---

## Integration Testing

Integration tests verify multi-component behavior with real I/O.

### Patterns

**Setup/Teardown with tempdir**:
```rust
#[test]
fn test_log_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.log");

    // Write
    {
        let mut log = Log::open(&path).unwrap();
        log.append(&Record::new(b"hello")).unwrap();
        log.append(&Record::new(b"world")).unwrap();
    }

    // Read (new instance)
    {
        let log = Log::open(&path).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log.read(0).unwrap().data, b"hello");
        assert_eq!(log.read(1).unwrap().data, b"world");
    }
}
```

**Async tests with tokio**:
```rust
#[tokio::test]
async fn test_client_server_round_trip() {
    let server = TestServer::start().await;
    let client = Client::connect(server.addr()).await.unwrap();

    let position = client.append("test-stream", b"data").await.unwrap();
    let record = client.read(position).await.unwrap();

    assert_eq!(record.data, b"data");
}
```

---

## Fuzzing

Fuzzing uses randomized inputs to find crashes, panics, and edge cases in parsing and cryptographic code.

### Fuzz Targets

Kimberlite includes two fuzz targets:

1. **`fuzz_wire_deserialize`**: Wire protocol parsing (Frame, Request, Response)
2. **`fuzz_crypto_encrypt`**: AES-256-GCM encryption round-trips and error handling

### Running Fuzz Tests

```bash
# Install cargo-fuzz (requires nightly Rust)
cargo install cargo-fuzz

# List available fuzz targets
cargo fuzz list

# Run a fuzz target (Ctrl+C to stop)
cargo fuzz run fuzz_wire_deserialize

# Run with specific iteration count
cargo fuzz run fuzz_wire_deserialize -- -runs=10000

# Run with specific seed for reproduction
cargo fuzz run fuzz_wire_deserialize -- -seed=1234567890

# Run in parallel (4 jobs)
cargo fuzz run fuzz_wire_deserialize -- -jobs=4
```

### CI Smoke Testing

For fast CI validation, run a limited number of iterations:

```bash
# Smoke test (10K iterations, ~30 seconds)
cd fuzz && ./smoke_test.sh
```

### Corpus Management

Fuzzing automatically saves interesting inputs to `fuzz/corpus/`:

```bash
# View corpus files
ls -lh fuzz/corpus/fuzz_wire_deserialize/

# Clear corpus to start fresh
rm -rf fuzz/corpus/fuzz_wire_deserialize/*

# Run with custom seed corpus
cargo fuzz run fuzz_wire_deserialize fuzz/corpus/fuzz_wire_deserialize/
```

### Reproducing Crashes

When fuzzing finds a crash, it saves the input to `fuzz/artifacts/`:

```bash
# Reproduce a crash
cargo fuzz run fuzz_wire_deserialize fuzz/artifacts/fuzz_wire_deserialize/crash-abc123...

# Debug with gdb/lldb
cargo fuzz run --debug fuzz_wire_deserialize fuzz/artifacts/...
```

See `fuzz/README.md` for detailed documentation.

---

## Performance Benchmarking

Kimberlite uses Criterion.rs for statistical performance benchmarking.

### Benchmark Suites

| Suite | File | What It Tests |
|-------|------|---------------|
| `crypto` | `benches/crypto.rs` | Hash, encryption, signing operations |
| `kernel` | `benches/kernel.rs` | State machine transitions |
| `storage` | `benches/storage.rs` | Append-only log operations |
| `wire` | `benches/wire.rs` | Protocol serialization |
| `end_to_end` | `benches/end_to_end.rs` | Full system throughput |

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench -p kimberlite-bench

# Run specific suite
cargo bench -p kimberlite-bench --bench crypto
cargo bench -p kimberlite-bench --bench storage

# Quick mode (fewer samples, faster)
cargo bench -p kimberlite-bench -- --quick

# Run specific benchmark
cargo bench -p kimberlite-bench --bench crypto -- blake3_hash

# Save baseline for comparison
cargo bench -p kimberlite-bench -- --save-baseline main

# Compare against baseline
cargo bench -p kimberlite-bench -- --baseline main
```

### Interpreting Results

```
blake3_hash/1024        time:   [498.23 ns 501.45 ns 504.98 ns]
                        thrpt:  [2.03 GB/s 2.04 GB/s 2.05 GB/s]
```

- **time**: 95% confidence interval (lower, estimate, upper)
- **thrpt**: Throughput calculated from input size

**Regression detection**:

```
change: [+15.234% +18.567% +21.823%] (p = 0.00 < 0.05)
Performance has regressed.
```

### Performance Targets

| Operation | Target | Measured | Status |
|-----------|--------|----------|--------|
| BLAKE3 1KB | < 1 µs | ~500 ns | ✅ 2x better |
| AES-GCM Encrypt 1KB | < 5 µs | ~2 µs | ✅ 2.5x better |
| Ed25519 Sign | < 100 µs | ~10-20 µs | ✅ 5-10x better |
| Storage Write 1KB | < 500 µs | ~380 µs | ✅ Met |
| Kernel AppendBatch | < 20 µs | ~1.5 µs | ✅ 13x better |
| E2E Write p99 | < 5 ms | ~190 µs | ✅ 26x better |

See `crates/kimberlite-bench/README.md` for detailed usage and CI integration.

---

## Running Tests

### Unit Tests

```bash
# Run all unit tests
cargo test --workspace

# Run tests for specific crate
cargo test -p kimberlite-storage

# Run specific test
cargo test -p kimberlite-kernel test_apply_committed

# Run with output
cargo test -- --nocapture
```

### Property Tests

```bash
# Run property tests (more cases than default)
PROPTEST_CASES=1000 cargo test --workspace

# Run with specific seed for reproduction
PROPTEST_CASES=1 cargo test my_property_test -- --seed 0xdeadbeef
```

### Simulation

```bash
# Run VOPR simulator
cargo run --bin vopr --release
# Or use just:
just vopr

# List available scenarios
just vopr-scenarios

# Run specific scenario
just vopr-scenario swizzle-clogging 1000

# Run all scenarios
just vopr-all-scenarios 100

# Run with specific seed
just vopr-seed 0x1234567890abcdef

# Run extended simulation
cargo run --bin vopr --release -- --operations 1000000 --timeout 3600
```

### Fuzzing

```bash
# List fuzz targets
just fuzz-list

# Run fuzzer (Ctrl+C to stop)
just fuzz fuzz_wire_deserialize

# Run smoke test (10K iterations, for CI)
just fuzz-smoke

# Run with specific iteration count
just fuzz-iterations fuzz_crypto_encrypt 100000

# Run all fuzz targets
just fuzz-all
```

### Benchmarks

```bash
# Run all benchmarks
just bench

# Run in quick mode (faster, fewer samples)
just bench-quick

# Run specific suite
just bench-suite crypto
just bench-suite-quick storage

# Save baseline
just bench-baseline before-optimization

# Compare against baseline
just bench-compare before-optimization

# Run all suites and open HTML report
just bench-report
```

### CI Pipeline

```yaml
test:
  # Fast: unit tests
  - cargo test --workspace

  # Medium: property tests with more cases
  - PROPTEST_CASES=500 cargo test --workspace

  # Slow: short simulation
  - cargo run --bin vopr --release -- --operations 10000

nightly:
  # Extended simulation
  - cargo run --bin vopr --release -- --operations 10000000 --timeout 28800
```

---

## Debugging Failures

### Reproducing VOPR Failures

When VOPR finds a failure, it prints the seed:

```
VOPR: Invariant violation detected!
      Seed: 0x1234567890abcdef
      Operation: 4532
      Violation: LogDivergence at position 1234

To reproduce:
  cargo run --bin vopr -- --seed 0x1234567890abcdef
```

Run with the seed to reproduce exactly:

```bash
cargo run --bin vopr -- --seed 0x1234567890abcdef
```

### Shrinking

VOPR attempts to find a minimal reproduction:

```
VOPR: Shrinking failure...
      Original: 4532 operations
      Shrunk:   23 operations

Minimal reproduction seed: 0x1234567890abcdef_shrunk_23
```

### Debugging with Traces

Enable detailed tracing to understand what happened:

```bash
RUST_LOG=vopr=trace cargo run --bin vopr -- --seed 0x1234...
```

### Common Failure Patterns

| Symptom | Likely Cause |
|---------|--------------|
| LogDivergence | Bug in consensus prepare/commit |
| HashChainBroken | Bug in hash computation or storage corruption handling |
| LinearizabilityViolation | Bug in read consistency implementation |
| ProjectionInconsistent | Bug in projection apply logic |
| Timeout | Liveness bug in leader election |

---

## Summary

Kimberlite's testing strategy is built on layers:

1. **Unit tests**: Fast, run constantly, catch obvious bugs
2. **Property tests**: Randomized, find edge cases
3. **Integration tests**: Real I/O, verify component interactions
4. **Simulation tests**: Find consensus and replication bugs under faults

Advanced patterns from FoundationDB and TigerBeetle enhance our simulation:

- **Swizzle-clogging**: Random network clog/unclog to find partition edge cases
- **Gray failures**: Partial failures (slow, intermittent) that evade simple detection
- **Byte-identical replica checkers**: Verify caught-up replicas match exactly
- **PAR fault categories**: Distinguish "not seen" vs "seen but corrupt"
- **Time compression**: 10x+ speedup for extended simulation runs

The goal is not 100% code coverage, but confidence that:
- The log is always consistent
- Committed data is never lost
- Hash chains are never broken
- Projections match the log
- Replicas are byte-identical when caught up
- Recovery never truncates committed data
- The system recovers from any fault combination

When in doubt, add an assertion. When that assertion fires in simulation, you've found a bug before it reached production.

---


---

**See Also:**
- **[VOPR Deep Dive](../../../docs-internal/vopr/overview.md)** (Internal) - Detailed VOPR implementation and debugging
- **[VOPR Scenarios](../../../docs-internal/vopr/scenarios.md)** (Internal) - All 46 test scenarios
- **[Assertions Guide](assertions.md)** - Production assertion patterns
- **[Property Testing](property-testing.md)** - Proptest strategies
