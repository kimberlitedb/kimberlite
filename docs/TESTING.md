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
│  │  - Linearizability               - Hash chain integrity     │ │
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

/// Client observed values must be linearizable
struct LinearizabilityChecker {
    history: Vec<Operation>,
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

See `crates/kimberlite-sim/SCENARIOS.md` for detailed configuration and usage examples for all 27 scenarios.

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

#### Client-Visible Invariants (3)

**14. LinearizabilityChecker**
- **What it checks**: Operations appear to execute atomically and in real-time order
- **Why it matters**: Strongest consistency guarantee, client-visible correctness
- **When it runs**: After every client operation
- **Reference**: Linearizability (Herlihy & Wing, 1990)

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
