# kimberlite-sim

Deterministic simulation testing harness for Kimberlite, inspired by FoundationDB's "trillion CPU-hour" testing and TigerBeetle's VOPR approach.

## Overview

This crate provides infrastructure for testing Kimberlite under controlled, reproducible conditions with fault injection and invariant checking.

## Features

- **Deterministic Execution**: Same seed → same execution → same bugs
- **Time Compression**: Run years of simulated time in seconds
- **Fault Injection**: Network partitions, storage failures, crashes
- **Invariant Checking**: Continuous correctness verification
- **Reproducibility**: Failed seeds can be reproduced exactly
- **Failure Diagnosis**: Automated root cause analysis

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Simulation Harness                            │
│  ┌─────────────┐   ┌──────────────┐   ┌─────────────────────┐   │
│  │ SimClock    │   │ EventQueue   │   │ SimRng              │   │
│  │ (discrete)  │   │ (scheduler)  │   │ (deterministic)     │   │
│  └─────────────┘   └──────────────┘   └─────────────────────┘   │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                    Simulated Components                      │ │
│  │  SimNetwork    SimStorage    FaultInjector                  │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                    Invariant Checkers                        │ │
│  │  HashChainChecker  LinearizabilityChecker  ConsistencyChecker│ │
│  └─────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

## Usage

### From CLI (Recommended)

```bash
# Run simulations
kmb sim run --iterations 1000

# Verify specific seed with verbose output
kmb sim verify --seed 12345

# Generate HTML report
kmb sim report --output report.html
```

### From Rust Code

```rust
use kimberlite_sim::{VoprConfig, VoprRunner};

// Create configuration
let config = VoprConfig {
    seed: 0,
    iterations: 100,
    verbose: false,
    ..Default::default()
};

// Run batch
let runner = VoprRunner::new(config);
let results = runner.run_batch();

// Check results
assert!(results.all_passed());
println!("Success rate: {:.1}%", results.success_rate() * 100.0);
```

### Standalone Binary (Legacy)

The `vopr` binary is still available for advanced usage:

```bash
# Build
cargo build --bin vopr --release

# Run with specific seed
./vopr --seed 12345 -v

# Run multiple iterations
./vopr --iterations 1000

# Enable specific faults
./vopr --faults network,storage

# Run test scenario
./vopr --scenario swizzle -n 500 -v
```

## Test Scenarios

Pre-configured test scenarios for specific failure modes:

- **baseline**: No faults, baseline performance
- **swizzle**: Swizzle-clogging (intermittent network congestion)
- **gray**: Gray failures (partial node failures)
- **multi-tenant**: Multi-tenant isolation with faults
- **time-compression**: 10x accelerated time for long-running tests
- **combined**: All fault types enabled simultaneously

List available scenarios:

```bash
kmb sim list-scenarios  # (coming soon)
./vopr --list-scenarios
```

## Invariants Checked

- **Linearizability**: All operations appear to execute atomically
- **Replica Consistency**: All replicas converge to same state
- **Hash Chain Integrity**: Log hash chains never break
- **Commit History**: No gaps in committed operations
- **Model Verification**: Reads match expected state

## Configuration

### VoprConfig Options

```rust
pub struct VoprConfig {
    pub seed: u64,                  // Starting seed
    pub iterations: u64,            // Number of runs
    pub network_faults: bool,       // Enable network faults
    pub storage_faults: bool,       // Enable storage faults
    pub verbose: bool,              // Verbose output
    pub max_events: u64,            // Events per simulation
    pub max_time_ns: u64,           // Simulated time limit
    pub check_determinism: bool,    // Run each seed twice
    pub enable_trace: bool,         // Full trace collection
    pub save_trace_on_failure: bool,// Save trace on failure
    pub enhanced_workloads: bool,   // RMW + Scan operations
    pub failure_diagnosis: bool,    // Auto failure analysis
    pub scenario: Option<ScenarioType>, // Predefined scenario
}
```

## Fault Injection

### Network Faults

- **Delays**: 1-50ms latency
- **Drops**: 0-10% packet loss
- **Duplicates**: 0-5% duplicate messages
- **Partitions**: Network splits (future)

### Storage Faults

- **Write Failures**: 0-1% failure rate
- **Read Corruption**: 0-0.1% corruption rate
- **Fsync Failures**: 0-1% failure rate
- **Partial Writes**: 0-1% incomplete writes

## Failure Diagnosis

When a simulation fails, the system automatically:

1. Captures execution trace leading to failure
2. Analyzes event patterns before failure
3. Identifies potential root causes
4. Generates diagnostic report

Example output:

```
FAILURE DIAGNOSIS
=================
Seed: 12345
Invariant: linearizability
Events processed: 1,234

ROOT CAUSE ANALYSIS:
  - High network drop rate (8.3%) likely contributed
  - 3 storage write failures in 100ms window
  - Replica divergence detected at event 1,230

RECOMMENDATIONS:
  - Increase retry timeout
  - Add storage write validation
  - Check replica sync logic
```

## Checkpoint/Resume Support

For long-running test campaigns:

```bash
# Start with checkpoint
vopr -n 100000 --checkpoint-file checkpoint.json

# Resume after interruption
vopr -n 100000 --checkpoint-file checkpoint.json
```

Checkpoint file tracks:
- Last completed seed
- Total iterations
- Failed seeds
- Success rate

## Performance

Typical performance on modern hardware:

- **Simple simulations**: 5,000-10,000 sims/sec
- **With faults**: 1,000-3,000 sims/sec
- **With tracing**: 500-1,000 sims/sec
- **With diagnosis**: 800-1,500 sims/sec

## Integration with CI/CD

```yaml
# .github/workflows/sim.yml
- name: Run VOPR simulations
  run: cargo run --bin vopr -- -n 10000 --json > results.jsonl

- name: Check for failures
  run: |
    if grep -q '"status":"failed"' results.jsonl; then
      echo "Simulation failures detected"
      exit 1
    fi
```

## Determinism Verification

Run each seed twice to verify deterministic execution:

```bash
vopr --check-determinism -n 100
```

This catches:
- Non-deterministic RNG usage
- Clock dependencies
- Race conditions
- Hash collisions

## Module Organization

- `clock.rs` - Discrete simulation time
- `event.rs` - Event queue and scheduling
- `rng.rs` - Deterministic random number generation
- `network.rs` - Simulated network with fault injection
- `storage.rs` - Simulated storage with fault injection
- `fault.rs` - Fault injection framework
- `invariant.rs` - Invariant checkers
- `trace.rs` - Execution tracing
- `diagnosis.rs` - Automated failure diagnosis
- `scenarios.rs` - Pre-configured test scenarios
- `vopr.rs` - High-level VOPR runner (CLI integration)

## See Also

- [kimberlite-cli](../kimberlite-cli/) - CLI commands using this crate
- [Jepsen](https://jepsen.io/) - Similar approach for distributed systems testing
- [FoundationDB's simulation testing](https://www.foundationdb.org/blog/building-on-foundations/)
- [TigerBeetle's VOPR](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/DESIGN.md#vopr)

## Future Enhancements

- [ ] Visualization of execution traces
- [ ] Interactive failure replay
- [ ] Cluster-wide simulation (multi-node)
- [ ] Query workload scenarios
- [ ] Performance regression detection
- [ ] Automated bisection for failure isolation
