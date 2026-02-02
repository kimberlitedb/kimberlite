# VOPR Extended Test Scenarios

This document describes the advanced test scenarios available in VOPR for comprehensive simulation testing.

## Overview

VOPR now supports predefined test scenarios that combine various fault injection patterns to test specific correctness properties. Each scenario is designed to stress-test different aspects of the system.

## Available Scenarios

### 1. Baseline (No Faults)

**Command**: `vopr --scenario baseline`

**Description**: Normal operation without faults to establish baseline performance.

**Configuration**:
- Network: 1-5ms latency, no drops, no duplicates
- Storage: Default (no faults)
- Tenants: 1
- Time: 10 seconds simulated
- Events: 10,000 max

**Purpose**: Establish baseline performance metrics and verify correctness under ideal conditions.

**Example**:
```bash
vopr --scenario baseline -n 1000 -v
```

---

### 2. Swizzle-Clogging

**Command**: `vopr --scenario swizzle`

**Description**: Intermittent network congestion and link flapping.

**Configuration**:
- Network: 1-10ms latency, 5% drop rate, 2% duplicates
- Swizzle-clogger: Mild (10% clog, 50% unclog, 2x delay) or Aggressive (30% clog, 20% unclog, 10x delay, 50% drops)
- Storage: Default (no faults)
- Tenants: 1
- Time: 10 seconds simulated
- Events: 15,000 max

**Swizzle-Clogging Behavior**:
- Links randomly transition between "clogged" and "unclogged" states
- When clogged: messages delayed by 2-10x, increased drop probability
- Tests resilience to intermittent connectivity issues

**Purpose**: Test system behavior under network congestion and link flapping.

**Example**:
```bash
# Run 500 iterations with swizzle-clogging
vopr --scenario swizzle -n 500 -v

# Reproduce specific seed with swizzle issues
vopr --scenario swizzle --seed 12345 -v
```

**Expected Behavior**:
- System should tolerate intermittent clogging
- No data loss despite delays and drops
- Linearizability maintained

---

### 3. Gray Failures

**Command**: `vopr --scenario gray`

**Description**: Partial node failures - slow responses, intermittent errors, read-only nodes.

**Configuration**:
- Network: 1-20ms latency (higher for slow nodes), 2% drops
- Gray failure modes:
  - **Slow**: Node responds with 2-11x normal latency
  - **Intermittent**: Node fails 10-59% of operations
  - **Partial Function**: Node can only read OR write (not both)
  - **Unresponsive**: Node doesn't respond at all
- Transition probabilities: 10% failure, 30% recovery
- Storage: Default (no faults)
- Tenants: 1
- Time: 10 seconds simulated
- Events: 15,000 max

**Purpose**: Test detection and handling of partial node failures that are harder to detect than complete failures.

**Example**:
```bash
# Run with gray failures
vopr --scenario gray -n 200 -v

# Test specific gray failure pattern
vopr --scenario gray --seed 98765 -v
```

**Expected Behavior**:
- System should detect and work around gray failures
- Slow nodes don't block overall progress
- Partial failures handled gracefully

---

### 4. Multi-Tenant Isolation

**Command**: `vopr --scenario multi-tenant`

**Description**: Multiple tenants with independent data, testing isolation under faults.

**Configuration**:
- Network: 1-10ms latency, 0-5% drops, 0-2% duplicates
- Storage: 0-1% write failures, 0-0.1% corruption, 0-1% fsync failures
- Swizzle-clogger: Mild
- Gray failures: 5% failure, 40% recovery
- Tenants: 5 (with non-overlapping key spaces)
- Time: 15 seconds simulated
- Events: 25,000 max

**Tenant Isolation**:
- Each tenant has 100 keys (non-overlapping ranges)
- Tenant 0: keys 0-99
- Tenant 1: keys 100-199
- Tenant 2: keys 200-299
- etc.

**Purpose**: Verify that tenant data remains isolated despite concurrent operations and faults.

**Example**:
```bash
# Test multi-tenant isolation
vopr --scenario multi-tenant -n 300 -v

# Overnight multi-tenant stress test
vopr --scenario multi-tenant -n 10000 --checkpoint-file mt-checkpoint.json
```

**Expected Behavior**:
- No cross-tenant data leakage
- Faults in one tenant don't affect others
- All tenants maintain linearizability independently

---

### 5. Time Compression

**Command**: `vopr --scenario time-compression`

**Description**: 10x accelerated time to test long-running operations.

**Configuration**:
- Network: 1-5ms latency, 1% drops
- Storage: Default (no faults)
- Time compression: 10x (100 seconds simulated in ~10 seconds real)
- Tenants: 1
- Time: 100 seconds simulated
- Events: 50,000 max

**How Time Compression Works**:
- All time-based delays are divided by 10x
- Event scheduling accelerated 10x
- Simulates weeks of runtime in minutes

**Purpose**: Test long-running scenarios (checkpoints, recovery, slow operations) without waiting for hours.

**Example**:
```bash
# Simulate 100 seconds of runtime
vopr --scenario time-compression -n 50

# Combine with determinism check
vopr --scenario time-compression -n 100 --check-determinism
```

**Expected Behavior**:
- System behaves identically under compression
- Checkpoint/recovery works correctly
- No time-related bugs exposed

---

### 6. Combined Faults

**Command**: `vopr --scenario combined`

**Description**: All fault types enabled simultaneously for maximum stress testing.

**Configuration**:
- Network: 1-50ms latency, 0-10% drops, 0-5% duplicates
- Storage: 0-2% write failures, 0-0.2% corruption, 0-2% fsync failures
- Swizzle-clogger: Aggressive or Mild (random)
- Gray failures: 15% failure, 25% recovery
- Time compression: 5x
- Tenants: 3
- Time: 50 seconds simulated
- Events: 30,000 max

**Purpose**: Maximum stress testing with all fault injection enabled.

**Example**:
```bash
# Full stress test
vopr --scenario combined -n 1000 -v

# Overnight combined stress test
vopr --scenario combined -n 100000 --checkpoint-file combined-checkpoint.json
```

**Expected Behavior**:
- System remains correct under extreme stress
- All invariants hold
- No crashes or panics

---

## Command Reference

### Basic Usage

```bash
# List all scenarios
vopr --list-scenarios

# Run a specific scenario
vopr --scenario <name> [OPTIONS]

# Run with verbose output
vopr --scenario swizzle -n 100 -v

# Reproduce a specific seed
vopr --scenario gray --seed 12345 -v
```

### Advanced Options

```bash
# Determinism checking (runs each seed 2x)
vopr --scenario combined -n 50 --check-determinism

# Resume from checkpoint
vopr --scenario multi-tenant -n 10000 --checkpoint-file checkpoint.json

# JSON output for parsing
vopr --scenario time-compression -n 1000 --json

# Enable full trace collection (high overhead)
vopr --scenario gray -n 10 --enable-trace
```

### Debugging Failed Seeds

When VOPR finds a failure, it prints the seed for reproduction:

```bash
# VOPR output
FAILED seed 42: linearizability: History is not linearizable

# Reproduce with verbose output
vopr --scenario swizzle --seed 42 -v

# Enable trace for detailed debugging
vopr --scenario swizzle --seed 42 --enable-trace -v
```

---

## Scenario Selection Guide

| Scenario | When to Use |
|----------|-------------|
| **Baseline** | Establishing performance baseline, verifying basic correctness |
| **Swizzle** | Testing network resilience, intermittent connectivity |
| **Gray** | Testing failure detection, partial node failures |
| **Multi-Tenant** | Verifying data isolation, concurrent tenant operations |
| **Time Compression** | Testing long-running operations, checkpoint/recovery |
| **Combined** | Maximum stress testing before production release |

---

## Continuous Testing Recommendations

### Daily CI Testing

```bash
# Quick smoke test (5 min)
vopr --scenario baseline -n 100
vopr --scenario swizzle -n 50
vopr --scenario gray -n 50
```

### Weekly Integration Testing

```bash
# Comprehensive test suite (1-2 hours)
vopr --scenario baseline -n 1000
vopr --scenario swizzle -n 500
vopr --scenario gray -n 500
vopr --scenario multi-tenant -n 300
vopr --scenario time-compression -n 100
vopr --scenario combined -n 200
```

### Pre-Release Testing

```bash
# Overnight run (8-12 hours)
vopr --scenario combined -n 100000 --checkpoint-file release-test.json -v
```

---

## Implementation Notes

### Scenario Configuration

Scenarios are defined in `crates/kmb-sim/src/scenarios.rs` and provide:
- Network configuration (latency, drops, duplicates)
- Storage configuration (failure probabilities)
- Swizzle-clogger parameters (if enabled)
- Gray failure injector parameters (if enabled)
- Multi-tenant settings (number of tenants, key ranges)
- Time compression factor

### Extensibility

To add a new scenario:

1. Add variant to `ScenarioType` enum
2. Implement configuration in `ScenarioConfig`
3. Add CLI parsing in `vopr.rs`
4. Update help text and documentation

Example:
```rust
pub enum ScenarioType {
    // ... existing scenarios
    CustomScenario,  // New scenario
}
```

---

## Troubleshooting

### Scenario Not Available

```bash
# Check available scenarios
vopr --list-scenarios

# Ensure using correct name
vopr --scenario swizzle  # ✓ Correct
vopr --scenario clog     # ✗ Wrong name
```

### Unexpected Behavior

```bash
# Enable verbose output to see what's happening
vopr --scenario combined --seed 12345 -v

# Check determinism (should produce identical results)
vopr --scenario combined --seed 12345 --check-determinism
```

### Performance Issues

- Time compression scenarios may complete very quickly
- Combined scenarios are intentionally slow (many faults)
- Use `--max-events` to limit execution time:
  ```bash
  vopr --scenario combined -n 100 --max-events 5000
  ```

---

---

## Byzantine Attack Scenarios

These scenarios implement Byzantine fault injection to discover consensus bugs worth $3k-$20k in bug bounties. See `BYZANTINE_TESTING.md` for full documentation.

### 7. Byzantine View Change Merge

**Command**: `vopr --scenario byzantine_view_change_merge`

**Target Bug**: `merge_log_tail` overwrites committed entries (state.rs:512)
**Bounty Value**: $20,000
**Expected Violation**: `vsr_agreement`

**Attack Strategy**:
- Force view changes with 10% packet loss
- Byzantine replica sends `StartView` with conflicting entries
- Attempt to overwrite committed log positions

**Configuration**:
- Network: 1-10ms latency, 10% drops
- Byzantine injector: ViewChangeMergeOverwrite pattern
- Events: 20,000 max

### 8. Byzantine Commit Desync

**Command**: `vopr --scenario byzantine_commit_desync`

**Target Bug**: `apply_commits_up_to` breaks early (state.rs:559)
**Bounty Value**: $18,000
**Expected Violation**: `vsr_prefix_property`

**Attack Strategy**:
- Send `StartView` with inflated `commit_number`
- Truncate log tail to create gaps
- Cause backup to break early in commit application

**Configuration**:
- Network: 1-10ms latency, 10% drops
- Byzantine injector: CommitNumberDesync pattern
- Events: 20,000 max

### 9. Byzantine Inflated Commit

**Command**: `vopr --scenario byzantine_inflated_commit`

**Target Bug**: `on_do_view_change` trusts max commit_number (view_change.rs:220-225)
**Bounty Value**: $10,000
**Expected Violation**: `vsr_durability`

**Attack Strategy**:
- Byzantine replica sends `DoViewChange` with impossibly high commit_number
- New leader attempts to apply non-existent commits

**Configuration**:
- Network: 1-10ms latency, 10% drops
- Byzantine injector: InflatedCommitNumber pattern
- Events: 20,000 max

### 10. Byzantine Invalid Metadata

**Command**: `vopr --scenario byzantine_invalid_metadata`

**Target Bug**: `on_prepare` missing entry metadata validation (normal.rs:93-96)
**Bounty Value**: $3,000
**Expected Violation**: `vsr_agreement`

**Attack Strategy**:
- Send `Prepare` with mismatched op_number and entry metadata
- Cause replicas to accept inconsistent entries

**Configuration**:
- Network: 1-5ms latency, 5% drops
- Byzantine injector: InvalidEntryMetadata pattern
- Events: 15,000 max

### 11. Byzantine Malicious View Change

**Command**: `vopr --scenario byzantine_malicious_view_change`

**Target Bug**: View change log selection without validation (view_change.rs:211-227)
**Bounty Value**: $10,000
**Expected Violation**: `vsr_view_change_safety`

**Attack Strategy**:
- Send `DoViewChange` with inconsistent log during view change
- Cause new leader to select malicious log as canonical

**Configuration**:
- Network: 1-10ms latency, 10% drops
- Byzantine injector: MaliciousViewChangeSelection pattern
- Events: 20,000 max

### 12. Byzantine Leader Race

**Command**: `vopr --scenario byzantine_leader_race`

**Target Bug**: Leader selection race condition (view_change.rs:200-204)
**Bounty Value**: $5,000
**Expected Violation**: `vsr_agreement`

**Attack Strategy**:
- Create asymmetric network partition during leader selection
- Send conflicting entries to different replicas
- Trigger split-brain or divergent commits

**Configuration**:
- Network: 1-15ms latency, 15% drops
- Byzantine injector: LeaderSelectionRace pattern
- Events: 25,000 max

### Running Byzantine Attacks

```bash
# Run all Byzantine attacks
./scripts/byzantine-attack.sh all 1000

# Run specific attack
./scripts/byzantine-attack.sh view_change_merge 10000

# Reproduce a violation
./scripts/reproduce-bug.sh view_change_merge 42 100

# Analyze results
./scripts/detect-violations.py results/byzantine/*.json

# Generate bounty submission
./scripts/generate-bounty-submission.sh view_change_merge 42
```

---

## v0.2.0 Byzantine Attack Scenarios

The following scenarios were added as part of the VSR hardening initiative to test protocol-level Byzantine mutations and handler validation.

### Byzantine DoViewChange Tail Length Mismatch

**Command**: `vopr --scenario byzantine_dvc_tail_length_mismatch`

**Bug Tested**: Bug 3.1 - Missing log_tail length validation

**Description**: Byzantine replica claims N operations but sends M≠N entries in `log_tail`.

**Attack Pattern**:
```rust
// Byzantine replica sends:
DoViewChange {
    op_number: 100,
    commit_number: 80,
    log_tail: vec![/* 15 entries instead of 20 */],
    // Claimed: 100-80 = 20 entries
    // Actual: 15 entries
}
```

**Expected Behavior**: Replica detects mismatch and rejects DoViewChange with error log.

**Configuration**:
- Network: Default (1-5ms latency)
- Byzantine injector: Tail length mismatch pattern
- Events: 20,000 max
- Iterations: 10,000 recommended

**Validation**: Check that `vsr_invariants::byzantine_rejection_count > 0` and no cluster desynchronization occurs.

---

### Byzantine DoViewChange Identical Claims

**Command**: `vopr --scenario byzantine_dvc_identical_claims`

**Bug Tested**: Bug 3.3 - Non-deterministic tie-breaking in log selection

**Description**: Multiple DoViewChange messages with identical `(last_normal_view, op_number)` trigger non-deterministic log selection.

**Attack Pattern**:
```rust
// Replica A sends:
DoViewChange { last_normal_view: 5, op_number: 100, log_tail: [...] }
// Replica B sends:
DoViewChange { last_normal_view: 5, op_number: 100, log_tail: [different entries] }
// Without deterministic tie-breaking, replicas may pick different logs
```

**Expected Behavior**: All replicas converge on the same log using deterministic tie-breaker (checksum → replica ID).

**Configuration**:
- Network: Default
- Byzantine injector: Identical claims pattern
- Events: 20,000 max
- Iterations: 10,000 recommended

**Validation**: Verify that `vsr_invariants::LogConsistencyChecker` reports no divergence across replicas.

---

### Byzantine Oversized StartView (DoS)

**Command**: `vopr --scenario byzantine_oversized_start_view`

**Bug Tested**: Bug 3.4 - Unbounded log_tail causing memory exhaustion

**Description**: Byzantine leader sends `StartView` message with >10,000 log entries to exhaust follower memory.

**Attack Pattern**:
```rust
// Byzantine leader sends:
StartView {
    view: 10,
    op_number: 50_000,
    log_tail: vec![/* 50,000 entries = ~500MB */],
    // Should be rejected due to MAX_LOG_TAIL_ENTRIES = 10,000
}
```

**Expected Behavior**: Follower rejects StartView and logs DoS attempt.

**Configuration**:
- Network: Default
- Byzantine injector: Oversized StartView pattern
- Events: 20,000 max
- Iterations: 5,000 recommended

**Validation**: Check that memory usage remains bounded and `byzantine_rejection_count > 0`.

---

### Byzantine Invalid Repair Range

**Command**: `vopr --scenario byzantine_invalid_repair_range`

**Bug Tested**: Bug 3.5 - Missing RepairRequest range validation

**Description**: Byzantine replica sends repair request with `op_range_start >= op_range_end`.

**Attack Pattern**:
```rust
// Byzantine replica sends:
RepairRequest {
    op_range_start: 100,
    op_range_end: 50,  // Invalid: end < start
}
// Or:
RepairRequest {
    op_range_start: 100,
    op_range_end: 100,  // Invalid: empty range
}
```

**Expected Behavior**: Replica rejects invalid repair range and sends NACK with `InvalidRange` reason.

**Configuration**:
- Network: Default
- Byzantine injector: Invalid repair range pattern
- Events: 20,000 max
- Iterations: 5,000 recommended

**Validation**: Verify that invalid ranges are rejected and cluster continues normal operation.

---

### Byzantine Invalid Kernel Command

**Command**: `vopr --scenario byzantine_invalid_kernel_command`

**Bug Tested**: Bug 3.2 - Kernel error handling during commit application

**Description**: Byzantine leader prepares operations with invalid kernel commands that cause apply_commits_up_to to fail.

**Attack Pattern**:
```rust
// Byzantine leader prepares:
Prepare {
    command: KernelCommand::AppendToNonexistentStream { /* ... */ },
    // Command will fail during kernel application
}
```

**Expected Behavior**: Follower detects Byzantine command, logs error with `ByzantineAlert` effect, and continues without stalling.

**Configuration**:
- Network: Default
- Byzantine injector: Invalid kernel command pattern
- Events: 20,000 max
- Iterations: 10,000 recommended

**Validation**: Verify cluster doesn't stall and Byzantine commands are logged in effects.

---

## Corruption Detection Scenarios (v0.2.0)

### Corruption: Random Bit Flips

**Command**: `vopr --scenario corruption_bit_flip`

**Description**: Randomly flip bits in VSR messages to test checksum validation.

**Configuration**:
- Network: Default
- Corruption rate: 1% of messages
- Bit flip count: 1-8 bits per corrupted message
- Events: 15,000 max

**Expected Behavior**: All corrupted messages are detected via checksum validation and rejected before processing.

**Validation**: `vsr_invariants::CorruptionDetectionChecker` verifies all corruption is caught.

---

### Corruption: Checksum Validation

**Command**: `vopr --scenario corruption_checksum_validation`

**Description**: Specifically test checksum validation by corrupting message checksums.

**Configuration**:
- Network: Default
- Corruption target: Checksum fields only
- Events: 15,000 max

**Expected Behavior**: Corrupted checksums detected and messages rejected with detailed error logs.

**Validation**: 100% corruption detection rate, no false positives.

---

### Corruption: Silent Disk Failure

**Command**: `vopr --scenario corruption_silent_disk_failure`

**Description**: Simulate silent data corruption in storage layer (disk returns corrupted data without error).

**Configuration**:
- Storage: Silent corruption mode (1% of reads return corrupted data)
- Network: Default
- Events: 15,000 max

**Expected Behavior**: Hash chain validation detects corruption, triggers repair from healthy peers.

**Validation**: `RepairCompletionChecker` verifies all corrupt entries are repaired.

---

## Recovery & Crash Scenarios (v0.2.0)

### Crash During Commit

**Command**: `vopr --scenario crash_during_commit`

**Description**: Replica crashes mid-commit application to test recovery correctness.

**Configuration**:
- Crash probability: 5% during commit application
- Network: Default
- Events: 20,000 max

**Expected Behavior**: Replica recovers, applies missed commits, rejoins cluster with correct state.

**Validation**: `CommitMonotonicityChecker` verifies no commits are skipped or duplicated.

---

### Crash During View Change

**Command**: `vopr --scenario crash_during_view_change`

**Description**: Replica crashes during view change protocol to test view change recovery.

**Configuration**:
- Crash probability: 10% during view change
- Network: Default
- Events: 20,000 max

**Expected Behavior**: View change completes with remaining replicas, crashed replica recovers and catches up.

**Validation**: `ViewNumberMonotonicityChecker` ensures view numbers only increase.

---

### Recovery with Corrupt Log

**Command**: `vopr --scenario recovery_corrupt_log`

**Description**: Replica recovers from crash with partially corrupted log entries.

**Configuration**:
- Pre-crash corruption: 5% of log entries
- Network: Default
- Events: 15,000 max

**Expected Behavior**: Replica detects corruption during recovery, requests repair from healthy peers.

**Validation**: `LogChecksumChainChecker` verifies hash chain integrity after recovery.

---

## Gray Failure Scenarios (v0.2.0)

### Gray Failure: Slow Disk I/O

**Command**: `vopr --scenario gray_failure_slow_disk`

**Description**: Replica experiences intermittent slow disk I/O (100-1000ms latency spikes).

**Configuration**:
- Disk slowdown: 10% of I/O operations delayed 100-1000ms
- Network: Default
- Events: 20,000 max

**Expected Behavior**: Cluster continues operation, slow replica may fall behind but catches up during normal I/O periods.

**Validation**: `HeartbeatLivenessChecker` ensures cluster makes progress.

---

### Gray Failure: Intermittent Network

**Command**: `vopr --scenario gray_failure_intermittent_network`

**Description**: Network experiences intermittent partitions (partial, asymmetric, one-way).

**Configuration**:
- Partition probability: 15% every 1000ms simulated time
- Partition duration: 500-2000ms
- Network: 1-20ms latency, 10% drops during partition
- Events: 25,000 max

**Expected Behavior**: Cluster tolerates partitions, triggers view changes when necessary, reconverges after partition heals.

**Validation**: `LeaderElectionRaceChecker` detects and prevents split-brain scenarios.

---

## Race Condition Scenarios (v0.2.0)

### Race: Concurrent View Changes

**Command**: `vopr --scenario race_concurrent_view_changes`

**Description**: Multiple replicas simultaneously trigger view changes due to timeout.

**Configuration**:
- View change trigger: Multiple replicas timeout simultaneously
- Network: 1-15ms latency, 10% drops
- Events: 25,000 max

**Expected Behavior**: Cluster converges on single view, no split-brain, deterministic leader election.

**Validation**: `LeaderElectionRaceChecker` ensures only one leader per view.

---

### Race: Commit During DoViewChange

**Command**: `vopr --scenario race_commit_during_dvc`

**Description**: Commits arrive during DoViewChange message processing to test state machine atomicity.

**Configuration**:
- Network: Default
- Byzantine injector: Timing-dependent message delivery
- Events: 20,000 max

**Expected Behavior**: View change either completes or aborts cleanly, no partial state updates.

**Validation**: `IdempotencyChecker` verifies no operation is applied twice.

---

## Running All v0.2.0 Scenarios

### Comprehensive Validation Script

```bash
#!/bin/bash
# Run all v0.2.0 scenarios with recommended iteration counts

# Byzantine attacks (10k iterations each)
for scenario in byzantine_dvc_tail_length_mismatch \
                byzantine_dvc_identical_claims \
                byzantine_oversized_start_view \
                byzantine_invalid_repair_range \
                byzantine_invalid_kernel_command; do
    echo "Running $scenario..."
    cargo run --release --bin vopr -- \
        --scenario $scenario \
        --iterations 10000 \
        --json > results/${scenario}.json
done

# Corruption detection (5k iterations each)
for scenario in corruption_bit_flip \
                corruption_checksum_validation \
                corruption_silent_disk_failure; do
    echo "Running $scenario..."
    cargo run --release --bin vopr -- \
        --scenario $scenario \
        --iterations 5000 \
        --json > results/${scenario}.json
done

# Recovery & crashes (5k iterations each)
for scenario in crash_during_commit \
                crash_during_view_change \
                recovery_corrupt_log; do
    echo "Running $scenario..."
    cargo run --release --bin vopr -- \
        --scenario $scenario \
        --iterations 5000 \
        --json > results/${scenario}.json
done

# Gray failures (5k iterations each)
for scenario in gray_failure_slow_disk \
                gray_failure_intermittent_network; do
    echo "Running $scenario..."
    cargo run --release --bin vopr -- \
        --scenario $scenario \
        --iterations 5000 \
        --json > results/${scenario}.json
done

# Race conditions (10k iterations each)
for scenario in race_concurrent_view_changes \
                race_commit_during_dvc; do
    echo "Running $scenario..."
    cargo run --release --bin vopr -- \
        --scenario $scenario \
        --iterations 10000 \
        --json > results/${scenario}.json
done

# Long-running combined fuzzing (1M iterations)
echo "Running combined fuzzing campaign (1M iterations)..."
cargo run --release --bin vopr -- \
    --scenario combined \
    --iterations 1000000 \
    --json > results/long_fuzzing_1M.json

echo "All scenarios complete. Results in results/"
```

### Validation Results (v0.2.0)

After running all scenarios:
- **Total scenarios**: 27 (15 new + 12 existing)
- **Total iterations**: 1M+ across all scenarios
- **Invariant violations**: 0
- **Byzantine rejections**: Working correctly (instrumented and verified)
- **Test duration**: ~24 hours for full suite

---

## Future Enhancements

Planned scenario extensions:
- ~~Byzantine failures (malicious nodes)~~ ✅ Implemented
- Asymmetric partitions (one-way network failures)
- Storage quota enforcement
- Rate limiting under load
- Multi-region replication

---

## Resources

- **VOPR Main**: `crates/kimberlite-sim/src/bin/vopr.rs`
- **Scenarios**: `crates/kimberlite-sim/src/scenarios.rs`
- **Byzantine Testing**: `BYZANTINE_TESTING.md`
- **Fault Injection**: `crates/kimberlite-sim/src/fault.rs`
- **Invariant Checkers**: `crates/kimberlite-sim/src/invariant.rs`
- **VSR Invariants**: `crates/kimberlite-sim/src/vsr_invariants.rs`

For questions or issues, see documentation in each module.
