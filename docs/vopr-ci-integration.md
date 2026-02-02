# VOPR CI Integration

This document explains how VOPR determinism checking is integrated into the CI pipeline.

## Overview

VOPR (Viewstamped Operation Replication) testing is integrated into CI at two levels:

1. **PR/Push Checks** - Fast determinism validation on every commit
2. **Nightly Stress Tests** - Long-running tests to catch rare bugs

## Workflows

### 1. `vopr-determinism.yml` (PR/Push)

**Triggers**: Every push to `main` and every PR

**Purpose**: Fast determinism validation

**What it checks**:
- Baseline scenario (100 iterations)
- Combined faults scenario (50 iterations)
- Multi-tenant isolation (50 iterations)
- Coverage enforcement (200 iterations)

**Runtime**: ~5-10 minutes

**Failure scenarios**:
- Determinism violations (same seed produces different results)
- Coverage below thresholds (80% fault points, 100% invariants)
- Invariant violations (linearizability, replica consistency, etc.)

### 2. `vopr-nightly.yml` (Scheduled)

**Triggers**:
- Daily at 2 AM UTC
- Manual dispatch via GitHub Actions UI

**Purpose**: Deep stress testing with high iteration counts

**What it tests**:
- Baseline: 10,000 iterations (configurable)
- SwizzleClogging: 5,000 iterations
- Gray failures: 5,000 iterations
- Multi-tenant: 3,000 iterations
- Combined: 5,000 iterations

**Runtime**: ~1-2 hours

**Outputs**:
- JSON results for each scenario
- Summary report
- Artifacts retained for 30 days
- Auto-creates GitHub issue on failure

## Running Locally

### Quick determinism check
```bash
cargo build --release -p kimberlite-sim --bin vopr
./target/release/vopr --iterations 100 --check-determinism
```

### Match CI baseline scenario
```bash
./target/release/vopr \
  --scenario baseline \
  --iterations 100 \
  --check-determinism \
  --seed 12345
```

### Match CI coverage enforcement
```bash
./target/release/vopr \
  --iterations 200 \
  --min-fault-coverage 80.0 \
  --min-invariant-coverage 100.0 \
  --require-all-invariants \
  --check-determinism
```

### Nightly-equivalent stress test
```bash
./target/release/vopr \
  --scenario baseline \
  --iterations 10000 \
  --check-determinism \
  --json > results.json
```

## Determinism Checks

The `--check-determinism` flag validates that simulations are perfectly reproducible:

1. **Runs each seed twice** with identical configuration
2. **Compares results**:
   - `storage_hash` - Final storage state
   - `kernel_state_hash` - Final kernel state
   - `events_processed` - Event count
   - `final_time_ns` - Simulated time

3. **Reports violations** with detailed diagnostics:
   ```
   determinism violation - storage_hash: [0x12...] != [0x34...], events_processed: 100 != 101
   ```

## Coverage Enforcement

CI requires:
- **80% fault point coverage** (critical paths: 100%)
- **100% invariant coverage** (all invariants run ≥1 time)
- **All critical invariants executed**

Example failure:
```
COVERAGE THRESHOLD FAILURES:
  ❌ Fault point coverage 75.0% below threshold 80.0%
  ❌ Critical invariants never ran: hash_chain_verify
```

## Investigated Failures

When VOPR fails in CI:

1. **Check artifacts**: Workflow uploads `vopr-results/` with traces
2. **Reproduce locally**: Use the seed from the failure
   ```bash
   ./target/release/vopr --seed <FAILED_SEED> -v
   ```
3. **Analyze trace**: VOPR saves failure traces automatically
4. **Bisect if needed**: Find which commit introduced nondeterminism

## Interpreting Results

### Success
```
Results:
  Successes: 100
  Failures: 0
  Time: 2.34s
  Rate: 42 sims/sec

Coverage Report:
  Fault Points: 3/3 (100.0%)
  Invariants:   7/7 (100.0%)
```

### Determinism Violation
```
Seed 12345: determinism violation - kernel_state_hash: [...] != [...]
```

**Action**: This is a critical bug. Same seed must produce identical results.

### Invariant Violation
```
Seed 54321: linearizability: History is not linearizable
```

**Action**: The system violated a correctness property. Investigate trace.

### Coverage Failure
```
COVERAGE THRESHOLD FAILURES:
  ❌ Invariant coverage 90.0% below threshold 100.0%
```

**Action**: Increase iterations or adjust scenario to hit all code paths.

## Adding New Scenarios to CI

1. **Add to `vopr-determinism.yml`** for fast checks:
   ```yaml
   - name: Run determinism check - New scenario
     run: |
       ./target/release/vopr \
         --scenario new_scenario \
         --iterations 50 \
         --check-determinism \
         --seed 11111
   ```

2. **Add to `vopr-nightly.yml`** for stress testing:
   ```yaml
   - name: New scenario stress test
     run: |
       ./target/release/vopr \
         --scenario new_scenario \
         --iterations 5000 \
         --check-determinism \
         --json > results-new-scenario.json
   ```

## Monitoring Nightly Results

### GitHub Actions UI
- Navigate to Actions → VOPR Nightly Stress Test
- View artifacts: `vopr-nightly-results-<run_number>`
- Download JSON results for analysis

### Automated Alerts
On nightly failure:
- GitHub issue auto-created
- Tagged with `vopr`, `nightly-failure`, `determinism`
- Includes run link and artifact info

### Trend Analysis
Compare results over time:
```bash
# Download recent nightly results
gh run download --name vopr-nightly-results-123
gh run download --name vopr-nightly-results-124

# Compare coverage trends
jq '.coverage.fault_point_coverage_percent' results-baseline.json
```

## Debugging Determinism Failures

### Step 1: Reproduce
```bash
./target/release/vopr --seed <FAILED_SEED> --check-determinism -v
```

### Step 2: Identify divergence point
VOPR reports which field differs:
- `storage_hash` → Storage layer nondeterminism
- `kernel_state_hash` → Kernel state nondeterminism
- `events_processed` → Event count mismatch
- `final_time_ns` → Clock nondeterminism

### Step 3: Check for common causes
- **Random number usage**: Ensure all RNG is seeded
- **System time calls**: Use `SimClock` not `SystemTime`
- **HashMap iteration**: Use `BTreeMap` for deterministic order
- **Async/threading**: Ensure deterministic scheduling
- **Floating point**: Avoid or use deterministic math

### Step 4: Add regression test
```rust
#[test]
fn test_scenario_determinism() {
    let config = VoprConfig {
        seed: <FAILED_SEED>,
        iterations: 2,
        check_determinism: true,
        // ...
    };
    let results = run_vopr(config);
    assert!(results.all_passed());
}
```

## Best Practices

1. **Always run determinism checks locally** before pushing
2. **Keep iterations low in PR checks** (<200) for fast feedback
3. **Use nightly for heavy stress testing** (>5000 iterations)
4. **Monitor coverage trends** to ensure test quality
5. **Investigate failures immediately** - determinism bugs worsen over time
6. **Add canary tests** for intentional bugs VOPR should catch

## Metrics to Track

From JSON output:
- `successes` / `failures` - Pass rate
- `fault_point_coverage_percent` - Code coverage
- `invariant_coverage_percent` - Invariant execution rate
- `elapsed_secs` - Performance regression
- `sims_per_sec` - Throughput

Goal: 100% success rate, 100% coverage, stable performance.

## Exit Codes

VOPR returns:
- **0**: All tests passed, coverage met
- **1**: Invariant violations or test failures
- **2**: Coverage thresholds not met

CI fails on any non-zero exit code.

## Troubleshooting

### "Coverage too low"
- Increase `--iterations`
- Adjust scenario to hit more code paths
- Check if new code added fault points

### "Determinism violation"
- Check for nondeterministic sources (see Step 3 above)
- Ensure same Rust version locally and CI
- Verify no floating point differences

### "Workflow timeout"
- Reduce iterations in nightly
- Optimize hot paths
- Check for infinite loops in fault injection

## Future Enhancements

Planned additions:
- [ ] Performance regression detection
- [ ] Coverage trend visualization
- [ ] Automatic bisection on failures
- [ ] Slack/Discord notifications
- [ ] Mutation testing integration
- [ ] LLM-generated scenario validation
