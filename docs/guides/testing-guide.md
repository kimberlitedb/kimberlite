# Testing Guide

This guide covers how to run tests, property-based tests, fuzzing, and VOPR simulation testing for Kimberlite.

## Quick Start

```bash
# Run all tests
just test

# Run with nextest (faster)
just nextest

# Run specific package tests
cargo test --package kimberlite-query

# Run single test
just test-one test_stream_id_roundtrip
```

## Test Categories

### Unit Tests

Unit tests live in `src/tests.rs` within each crate or inline with `#[cfg(test)]` modules.

```bash
# All unit tests
cargo test --workspace --lib

# Specific crate
cargo test --package kimberlite-types --lib
```

### Integration Tests

Integration tests are in `tests/` directories.

```bash
# All integration tests
cargo test --workspace --test '*'

# Specific integration test file
cargo test --package kimberlite-cli --test command_integration
```

### Property-Based Tests

Property tests use `proptest` to verify invariants across random inputs.

```bash
# Run with more test cases
PROPTEST_CASES=10000 cargo test --workspace

# Specific property test
cargo test --package kimberlite-types test_stream_id_roundtrip
```

**Key property tests:**
- `kimberlite-types`: StreamId ↔ TenantId roundtrip
- `kimberlite-query`: Key encoding/decoding, type coercion, aggregates

## Fuzzing

Kimberlite uses `cargo-fuzz` for automated fuzzing of critical parsers.

### Setup

```bash
# Install cargo-fuzz
cargo install cargo-fuzz
```

### Available Fuzz Targets

```bash
# List all fuzz targets
just fuzz-list
# Or:
cargo fuzz list
```

Current targets:
- `parse_sql` - SQL parser fuzzing

### Running Fuzzing

```bash
# Quick smoke test (1 minute)
just fuzz-smoke

# Run specific target for 1 hour
just fuzz parse_sql
# Or:
cargo fuzz run parse_sql -- -max_total_time=3600

# Run until crash is found
cargo fuzz run parse_sql
```

### Analyzing Crashes

```bash
# Fuzz crashes are saved to:
ls fuzz/artifacts/parse_sql/

# Reproduce a crash
cargo fuzz run parse_sql fuzz/artifacts/parse_sql/crash-...
```

## VOPR Simulation Testing

VOPR (Viewstamped Operation Replication) provides deterministic simulation testing with fault injection.

### Running VOPR

```bash
# Default scenario
just vopr

# Specific scenario
just vopr-scenario baseline 100000
just vopr-scenario multi_tenant_isolation 50000

# List available scenarios
just vopr-scenarios
# Or:
cargo run --bin vopr -- --list-scenarios
```

### Available Scenarios

- **Baseline** - Normal operation without faults
- **SwizzleClogging** - Intermittent network congestion
- **GrayFailures** - Partial node failures (slow responses, intermittent errors)
- **MultiTenantIsolation** - Multiple tenants with fault injection
- **TimeCompression** - 10x accelerated time
- **Combined** - All fault types enabled

### Extended VOPR Runs

```bash
# Overnight run (10M operations)
cargo run --bin vopr --release -- \
    --scenario multi_tenant_isolation \
    --operations 10000000 \
    --timeout 28800

# With tracing enabled
cargo run --bin vopr --release -- \
    --scenario combined \
    --operations 1000000 \
    --trace
```

### Understanding VOPR Output

```
Running VOPR scenario: MultiTenantIsolation
  Seed: 12345
  Operations: 100000

✓ Simulation completed successfully
  Events: 98542
  Simulated time: 15.23s
  Storage hash: 0x1a2b3c...
```

**Success indicators:**
- All invariants hold (no violations)
- Storage hash is deterministic (same seed → same hash)
- No crashes or panics

**Failure output:**
```
✗ Invariant violation: tenant_isolation
  Message: row belongs to tenant 2 but was returned to tenant 1
  At event: 45231
  Seed: 12345 (use this to reproduce)
```

## Invariant Checkers

VOPR includes several invariant checkers:

### Core Invariants
- **HashChainChecker** - Verifies hash chain integrity
- **LinearizabilityChecker** - Ensures operations appear atomic
- **ConsistencyChecker** - Verifies replica consistency

### Query Invariants
- **QueryDeterminismChecker** - Same query+params → same result
- **ReadYourWritesChecker** - Writes are visible to subsequent reads
- **TypeSafetyChecker** - Column types match schema
- **OrderByLimitChecker** - LIMIT applied after ORDER BY
- **AggregateCorrectnessChecker** - COUNT/SUM are correct
- **TenantIsolationChecker** - No cross-tenant data leakage

## Test Coverage

```bash
# Generate coverage report (requires tarpaulin)
cargo tarpaulin --workspace --out Html

# View coverage
open tarpaulin-report.html
```

## Continuous Integration

The CI pipeline runs:

```bash
# Format check
cargo fmt --all -- --check

# Clippy with strict lints
cargo clippy --workspace --all-targets -- -D warnings

# All tests
cargo test --workspace

# Documentation check
cargo doc --workspace --no-deps

# Smoke fuzz (limited time)
cd fuzz && ./smoke_test.sh
```

## Debugging Test Failures

### Property Test Failures

```bash
# Property tests print failing cases
PROPTEST_CASES=1000 cargo test test_stream_id_roundtrip

# Output includes:
# proptest: Shrinking failed
# minimal failing case: (tenant_id: 4294967295, local_id: 2147483647)
```

### VOPR Failures

```bash
# Reproduce with the same seed
cargo run --bin vopr -- --scenario combined --seed 12345

# Enable tracing for more detail
cargo run --bin vopr -- --scenario combined --seed 12345 --trace
```

### Integration Test Failures

```bash
# Run with verbose output
cargo test --package kimberlite-cli --test command_integration -- --nocapture

# Run specific failing test
cargo test --package kimberlite-cli cluster_init_creates_cluster_config -- --nocapture
```

## Performance Benchmarking

```bash
# Run benchmarks (requires nightly)
cargo +nightly bench

# Specific benchmark
cargo +nightly bench --package kimberlite-query hash_chain
```

## Best Practices

### Writing Tests

1. **Use descriptive names**: `test_stream_id_roundtrip_with_max_values`
2. **Test edge cases**: Zero, max values, boundaries
3. **Use property tests** for invariants that must hold for all inputs
4. **Keep tests fast**: Unit tests < 1ms, integration tests < 100ms

### Property Testing

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_stream_id_roundtrip(
        tenant_id in 0u64..=u64::from(u32::MAX),
        local_id in any::<u32>()
    ) {
        let stream_id = StreamId::from_tenant_and_local(
            TenantId::from(tenant_id),
            local_id
        );
        prop_assert_eq!(
            TenantId::from_stream_id(stream_id),
            TenantId::from(tenant_id)
        );
        prop_assert_eq!(stream_id.local_id(), local_id);
    }
}
```

### Fuzzing

1. **Start with short runs**: Validate the target works before long runs
2. **Use corpus**: Save interesting inputs to `fuzz/corpus/`
3. **Minimize crashes**: `cargo fuzz cmin` to minimize corpus
4. **Triage crashes**: Check if crash is exploitable or just input validation

### VOPR

1. **Start with Baseline**: Ensure correctness without faults
2. **Add faults incrementally**: SwizzleClogging → GrayFailures → Combined
3. **Use appropriate operation counts**:
   - Quick check: 10K operations
   - CI: 100K operations
   - Overnight: 10M+ operations
4. **Save seeds**: Failing seeds can reproduce bugs deterministically

## Troubleshooting

### Property Tests Flaking

```bash
# Run with fixed seed
PROPTEST_SEED=12345 cargo test test_name
```

### Fuzz Target Crashes Immediately

```bash
# Check it compiles
cargo fuzz build parse_sql

# Run with timeout
cargo fuzz run parse_sql -- -max_total_time=1
```

### VOPR Reports Timeout

```bash
# Increase timeout or reduce operations
cargo run --bin vopr -- \
    --scenario baseline \
    --operations 50000 \
    --timeout 600  # 10 minutes
```

## Resources

- [Rust Testing Guide](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Proptest Documentation](https://docs.rs/proptest/)
- [cargo-fuzz Guide](https://rust-fuzz.github.io/book/cargo-fuzz.html)
- [FoundationDB Simulation Testing](https://www.foundationdb.org/files/simulation-testing.pdf)

## See Also

- [TESTING.md](../TESTING.md) - Testing philosophy and strategy
- [VOPR_DEPLOYMENT.md](../VOPR_DEPLOYMENT.md) - Deploying VOPR in production
- [CLAUDE.md](../../CLAUDE.md) - Quick commands for Claude Code
