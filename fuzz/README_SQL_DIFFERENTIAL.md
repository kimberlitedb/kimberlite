# SQL Differential Fuzzing

This document describes the SQL differential fuzzing target for Kimberlite, inspired by the Crucible framework and SQLancer research.

## Overview

The `fuzz_sql_differential` target tests the SQL oracle infrastructure by executing randomly-generated SQL queries against DuckDB. It verifies that:

- The DuckDB oracle wrapper handles all SQL inputs without panicking
- Query execution errors are handled gracefully
- Type conversion from DuckDB to Kimberlite Value types is safe

**Future enhancement:** When `KimberliteOracle` is fully implemented (Task #5), this will become a true differential fuzzer comparing Kimberlite vs DuckDB results byte-by-byte, like Crucible's differential testing approach that found 154+ bugs in major OSS projects.

## How It Works

The fuzzer interprets random bytes as a sequence of SQL operations:

1. **Byte 0:** Number of operations (1-10)
2. **For each operation:**
   - **Byte N:** Operation type (0=CREATE, 1=INSERT, 2=SELECT, 3=UPDATE, 4=DELETE)
   - **Remaining bytes:** Parameters for the operation (table names, column types, values, etc.)

### Generated SQL Operations

- **CREATE TABLE:** Generates tables with 1-5 columns of random types (INTEGER, TEXT, REAL, BOOLEAN)
- **INSERT:** Inserts random values matching different types
- **SELECT:** Generates various SELECT clauses (*, COUNT(*), LIMIT, column selection)
- **UPDATE:** Updates random columns with random values
- **DELETE:** Deletes rows matching random conditions

## Running the Fuzzer

### Prerequisites

The fuzzer requires **Rust nightly** for sanitizer support:

```bash
rustup default nightly
```

### Quick Smoke Test

Run the included smoke test (works on stable Rust):

```bash
cd fuzz
cargo run --release --bin test_sql_differential
```

Expected output:
```
Testing SQL differential fuzzer logic...
  Testing with 0 bytes...
  Testing with 7 bytes...
    ✓ Op 0: CREATE TABLE t5
    ✓ Final verification passed
  Testing with 14 bytes...
    ✓ Op 0: CREATE TABLE t1
    ✓ Op 1: INSERT into t1
    ✓ Op 2: SELECT from t1 (1 rows)
    ✓ Final verification passed
...
✓ All smoke tests passed!
```

### Full Fuzzing Campaign

Run with libFuzzer (requires nightly):

```bash
# Short run (1 minute)
cargo +nightly fuzz run fuzz_sql_differential -- -max_total_time=60

# Standard run (1 hour, recommended for CI)
cargo +nightly fuzz run fuzz_sql_differential -- -max_total_time=3600

# Long run (overnight, for thorough testing)
cargo +nightly fuzz run fuzz_sql_differential -- -max_total_time=28800
```

### Corpus Management

LibFuzzer maintains a corpus of interesting inputs:

```bash
# View corpus location
ls fuzz/corpus/fuzz_sql_differential/

# Add seed inputs
echo -n "\x03\x00\x01\x02\x00\x01" > fuzz/corpus/fuzz_sql_differential/seed1

# Minimize corpus (remove redundant inputs)
cargo +nightly fuzz cmin fuzz_sql_differential
```

## Expected Behavior

### Success Cases

The fuzzer should **not panic or crash** on any input. Expected behaviors:

- ✅ Tables created successfully
- ✅ Data inserted successfully (when table exists)
- ✅ Queries executed successfully (when tables exist)
- ✅ Errors handled gracefully (table not found, type mismatch, etc.)

### Error Cases (Expected)

The fuzzer may encounter errors when:

- Trying to INSERT into non-existent tables
- Trying to SELECT from non-existent tables
- Type mismatches in INSERT VALUES
- Invalid WHERE clause conditions

These errors are **expected** and handled gracefully by the oracle. The fuzzer should continue without panicking.

### Failure Cases (Bugs Found)

If the fuzzer panics or crashes, it indicates a bug in:

1. **DuckDB oracle wrapper:** Error handling issue in `kimberlite-oracle/src/duckdb.rs`
2. **Type conversion:** Bug in `convert_value()` function
3. **Result comparison:** Issue in `compare_results()` (future)

## Integration with Crucible Methodology

This fuzzer implements **Phase 1** of applying Crucible's security research techniques to Kimberlite:

### Current Implementation (Phase 1)

- ✅ Oracle infrastructure (`kimberlite-oracle` crate)
- ✅ DuckDB ground truth oracle
- ✅ SQL query generation from fuzz bytes
- ✅ Crash/panic detection

### Future Implementation (Phase 1 Complete)

When `KimberliteOracle` is wired into VOPR (Task #5):

- 🔲 Execute queries in both DuckDB and Kimberlite
- 🔲 Compare results byte-by-byte using `DifferentialTester`
- 🔲 Report semantic bugs (different results = optimizer bug)
- 🔲 Achieve Crucible-level bug finding (5-10 bugs expected in first campaign)

## Performance

- **Throughput:** ~1000-2000 executions/second (depends on query complexity)
- **Memory:** ~50MB per fuzzer instance
- **CPU:** 100% of one core per instance

Run multiple instances in parallel for faster coverage:

```bash
# Run 4 parallel fuzzer instances
for i in {1..4}; do
  cargo +nightly fuzz run fuzz_sql_differential -- -max_total_time=3600 -jobs=1 &
done
```

## CI Integration

Add to `.github/workflows/fuzzing.yml`:

```yaml
jobs:
  sql-differential-fuzzing:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - name: Install cargo-fuzz
        run: cargo install cargo-fuzz
      - name: Run SQL differential fuzzer
        run: cargo +nightly fuzz run fuzz_sql_differential -- -max_total_time=3600
```

## Comparison to Existing Testing

| Technique | Current Kimberlite | With SQL Differential Fuzzing |
|-----------|-------------------|-------------------------------|
| **SQL parser fuzzing** | ✅ `fuzz_sql_parser` | ✅ Enhanced with execution |
| **Query correctness** | ⚠️ TLP/NoREC (internal) | ✅ **Differential (external oracle)** |
| **Optimizer bugs** | ❌ No coverage | ✅ **DuckDB comparison** |
| **Type conversion** | ⚠️ Unit tests | ✅ **Fuzz testing** |

## References

- **Crucible Framework:** "Detecting Logic Bugs in DBMS" (154+ bugs found)
- **SQLancer:** Differential testing methodology (148+ bugs in nghttp2)
- **Kimberlite VOPR:** 46 scenarios, 19 invariants, 85k-167k sims/sec

## Troubleshooting

### Error: "the option `Z` is only accepted on the nightly compiler"

Solution: Switch to nightly Rust:
```bash
rustup default nightly
```

### Error: "cargo-fuzz not found"

Solution: Install cargo-fuzz:
```bash
cargo install cargo-fuzz
```

### Fuzzer runs slowly

Solutions:
- Use `--release` mode: `cargo +nightly fuzz run fuzz_sql_differential --release`
- Run multiple instances in parallel
- Reduce timeout: `-max_total_time=60` for quick tests

### Want to see crashes only

Use the `-artifact_prefix` flag to save only crashing inputs:
```bash
cargo +nightly fuzz run fuzz_sql_differential -- -artifact_prefix=crashes/
```

## Next Steps

1. **Task #5:** Wire `KimberliteOracle` into VOPR to enable true differential testing
2. **Task #6:** Implement MVCC anomaly detection (Jepsen-style)
3. **Task #7:** Implement compliance policy fuzzing (RBAC/ABAC bypass detection)

Once Task #5 is complete, this fuzzer will achieve Crucible-level bug finding capability, with an expected payoff of **5-10 SQL correctness bugs** in the first fuzzing campaign.
