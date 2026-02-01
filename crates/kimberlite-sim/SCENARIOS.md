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

## Future Enhancements

Planned scenario extensions:
- Byzantine failures (malicious nodes)
- Asymmetric partitions (one-way network failures)
- Storage quota enforcement
- Rate limiting under load
- Multi-region replication

---

## Resources

- **VOPR Main**: `crates/kmb-sim/src/bin/vopr.rs`
- **Scenarios**: `crates/kmb-sim/src/scenarios.rs`
- **Fault Injection**: `crates/kmb-sim/src/fault.rs`
- **Invariant Checkers**: `crates/kmb-sim/src/invariant.rs`

For questions or issues, see `/Users/jaredreyes/Developer/rust/kimberlite/crates/kmb-sim/README.md`.
