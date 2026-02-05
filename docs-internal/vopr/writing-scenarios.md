# Writing VOPR Scenarios

**Internal Documentation** - For Kimberlite contributors

This guide explains how to add new simulation scenarios to VOPR's deterministic testing framework.

## Overview

VOPR scenarios are defined in `crates/kimberlite-sim/src/scenarios.rs` and test specific fault combinations, protocol attacks, or system behaviors under stress.

## Scenario Structure

Each scenario consists of:

1. **ScenarioType enum variant** - Defines the scenario name
2. **Scenario configuration** - Specifies faults, workload, and invariants
3. **Documentation** - Describes what the scenario tests

## Adding a New Scenario

### Step 1: Define the Scenario Type

Add a new variant to the `ScenarioType` enum in `scenarios.rs`:

```rust
pub enum ScenarioType {
    // ... existing scenarios

    // Your new scenario
    MyNewScenario,
}
```

### Step 2: Implement the Scenario

Add configuration in the `scenario_config()` function:

```rust
ScenarioType::MyNewScenario => ScenarioConfig {
    description: "Description of what this tests".to_string(),
    faults: vec![
        FaultType::NetworkPartition { probability: 0.1 },
        FaultType::CrashRecovery { probability: 0.05 },
    ],
    workload: WorkloadType::MultiTenant {
        tenants: 10,
        streams_per_tenant: 100,
        operations_per_second: 1000,
    },
    invariants: vec![
        InvariantType::OffsetMonotonicity,
        InvariantType::ConsensusAgreement,
    ],
    duration_seconds: 300,
},
```

### Step 3: Test the Scenario

Run your new scenario:

```bash
cargo run --bin vopr -- run --scenario my_new_scenario --iterations 1000
```

### Step 4: Document the Scenario

Add the scenario to `docs-internal/vopr/scenarios.md` with:
- **Purpose**: What property/bug this scenario is designed to catch
- **Faults Injected**: List of fault types and probabilities
- **Invariants Tested**: Which invariants must hold
- **Example Usage**: How to run it and interpret results

## Scenario Design Guidelines

### Choose Meaningful Fault Combinations

Focus on realistic fault combinations that expose specific bugs:

```rust
// Good: Tests view change during network partition
faults: vec![
    FaultType::NetworkPartition { probability: 0.2 },
    FaultType::ClockSkew { max_drift_ms: 100 },
]

// Avoid: Random combination without clear purpose
faults: vec![
    FaultType::Everything { chaos_mode: true },
]
```

### Select Appropriate Invariants

Only check invariants relevant to what you're testing:

```rust
// Testing consensus safety
invariants: vec![
    InvariantType::ConsensusAgreement,
    InvariantType::ViewMonotonicity,
    InvariantType::CommitIndexProgress,
]

// Not needed for this scenario
// InvariantType::StorageCompaction, (only for storage tests)
```

### Use Realistic Workloads

Match workload patterns to the scenario purpose:

```rust
// Testing multi-tenant isolation
workload: WorkloadType::MultiTenant {
    tenants: 100,
    streams_per_tenant: 10,
    operations_per_second: 5000,
}

// Testing high-throughput single tenant
workload: WorkloadType::Uniform {
    streams: 1000,
    operations_per_second: 50000,
}
```

## Common Scenario Patterns

### Byzantine Attack Scenario

Tests protocol resilience against malicious replicas:

```rust
ScenarioType::ByzantineMyAttack => ScenarioConfig {
    description: "Tests resilience to [specific attack]".to_string(),
    faults: vec![
        FaultType::ByzantineAttack {
            attack_type: AttackType::PrepareEquivocation,
            malicious_replicas: 1,  // f = 1 for 4-node cluster
        },
    ],
    invariants: vec![
        InvariantType::ConsensusAgreement,  // Must still agree
        InvariantType::ByzantineDetection,   // Must detect attack
    ],
    duration_seconds: 600,
}
```

### Crash Recovery Scenario

Tests data durability after crashes:

```rust
ScenarioType::CrashMyCase => ScenarioConfig {
    description: "Tests recovery from crash during [operation]".to_string(),
    faults: vec![
        FaultType::CrashRecovery {
            probability: 0.1,
            crash_timing: CrashTiming::DuringCommit,
        },
    ],
    invariants: vec![
        InvariantType::OffsetMonotonicity,    // No offset gaps
        InvariantType::StorageIntegrity,       // Data not corrupted
        InvariantType::RecoveryCompleteness,   // All data recovered
    ],
    duration_seconds: 300,
}
```

### Gray Failure Scenario

Tests handling of partial/intermittent failures:

```rust
ScenarioType::GrayFailureMyCase => ScenarioConfig {
    description: "Tests detection of [gray failure type]".to_string(),
    faults: vec![
        FaultType::GrayFailure {
            failure_type: GrayFailureType::SlowDisk,
            severity: 0.5,  // 50% slower than normal
            intermittent: true,
        },
    ],
    invariants: vec![
        InvariantType::LatencyBounds,      // Must detect slow path
        InvariantType::ViewChangeTriggered, // Must view change
    ],
    duration_seconds: 900,
}
```

## Testing Your Scenario

### Quick Validation

Run a short test to verify basic functionality:

```bash
cargo run --bin vopr -- run \
  --scenario my_new_scenario \
  --iterations 100 \
  --verbose
```

### Full Validation

Run longer tests with coverage tracking:

```bash
cargo run --bin vopr -- run \
  --scenario my_new_scenario \
  --iterations 10000 \
  --coverage \
  --junit-output results.xml
```

### Reproduction Testing

Verify determinism by re-running with same seed:

```bash
# First run - note the seed in output
cargo run --bin vopr -- run --scenario my_new_scenario --seed 12345

# Second run - should produce identical results
cargo run --bin vopr -- run --scenario my_new_scenario --seed 12345
```

## Debugging Scenarios

### Enable Detailed Logging

```bash
RUST_LOG=vopr=debug cargo run --bin vopr -- run \
  --scenario my_new_scenario \
  --iterations 100
```

### Use Event Timeline

Generate timeline visualization:

```bash
cargo run --bin vopr -- timeline failure.kmb
```

### Minimize Failing Cases

Reduce a failing test case to minimal reproduction:

```bash
cargo run --bin vopr -- minimize failure.kmb
```

## Integration with CI

Add your scenario to the CI test suite in `.github/workflows/vopr.yml`:

```yaml
- name: Run VOPR - My New Scenario
  run: |
    cargo run --bin vopr -- run \
      --scenario my_new_scenario \
      --iterations 5000 \
      --junit-output vopr-my-scenario.xml
```

## Scenario Maintenance

### When to Update Scenarios

- **New features added**: Add scenarios testing new functionality
- **Bug found**: Add scenario reproducing the bug (before fixing)
- **Protocol changes**: Update affected scenarios
- **Performance improvements**: Adjust iteration counts if faster

### Deprecating Scenarios

If a scenario becomes redundant:

1. Mark as deprecated in enum docs:
   ```rust
   #[deprecated(since = "0.5.0", note = "Replaced by BetterScenario")]
   OldScenario,
   ```

2. Update documentation to note replacement

3. Remove after 2 major versions

## Examples

See existing scenarios in `scenarios.rs`:
- `baseline` - Simple happy-path scenario
- `byzantine_view_change_merge` - Complex attack scenario
- `crash_during_commit` - Recovery testing

## Further Reading

- [VOPR Overview](overview.md) - Testing philosophy and capabilities
- [Scenarios Reference](scenarios.md) - All 46 current scenarios
- [VOPR Deployment](deployment.md) - Running VOPR in production
- [Testing Strategy](../../docs-internal/contributing/testing-strategy.md) - Overall testing approach

---

**Questions?** Ask in #vopr-testing on the internal Slack workspace.
