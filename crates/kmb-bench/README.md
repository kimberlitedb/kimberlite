# kmb-bench: Performance Benchmarks and Load Tests

Comprehensive performance benchmarking infrastructure for Kimberlite using Criterion.rs.

## Quick Start

```bash
# Run all benchmarks
cargo bench -p kmb-bench

# Run specific benchmark suite
cargo bench -p kmb-bench crypto
cargo bench -p kmb-bench storage
cargo bench -p kmb-bench kernel
cargo bench -p kmb-bench wire
cargo bench -p kmb-bench end_to_end

# Save baseline for comparison
cargo bench -p kmb-bench -- --save-baseline main

# Compare against baseline
cargo bench -p kmb-bench -- --baseline main

# Generate HTML reports (saved to target/criterion)
cargo bench -p kmb-bench --bench crypto
open target/criterion/report/index.html
```

## Benchmark Suites

### 1. Crypto Benchmarks (`benches/crypto.rs`)

Tests cryptographic primitive performance.

**Hash Operations:**
- BLAKE3 hash (64B to 16KB)
- Chain hash (hash of previous hash + data)

**Encryption:**
- AES-256-GCM encrypt (64B to 64KB)
- AES-256-GCM decrypt (64B to 64KB)
- Field-level encryption/decryption (16B to 1KB)

**Signatures:**
- Ed25519 signing (64B to 4KB)
- Ed25519 verification (64B to 4KB)
- Key generation (EncryptionKey, FieldKey, SigningKey)

**Expected Performance:**
- BLAKE3: ~1-2 GB/s
- AES-GCM: ~500 MB/s - 1 GB/s
- Ed25519 sign: ~10-20k ops/sec
- Ed25519 verify: ~5-10k ops/sec

**Example:**
```bash
cargo bench -p kmb-bench crypto
```

### 2. Storage Benchmarks (`benches/storage.rs`)

Tests storage layer performance.

**Operations:**
- Single write (64B to 16KB)
- Single read (64B to 16KB)
- Fsync (empty and after write)
- Batch write (10-500 events)
- Sequential read (10-500 events)

**Expected Performance:**
- Write (1KB): ~100-500 μs
- Read (1KB): ~10-50 μs
- Fsync: ~1-10 ms (depends on disk)
- Batch write (100 events): ~1-5 ms

**Example:**
```bash
cargo bench -p kmb-bench storage
```

### 3. Kernel Benchmarks (`benches/kernel.rs`)

Tests pure functional state machine performance.

**Stream Commands:**
- CreateStream
- AppendBatch (1-100 events)

**Table Commands:**
- CreateTable
- Insert

**State Operations:**
- State clone
- Stream query

**Expected Performance:**
- CreateStream: ~1-5 μs
- AppendBatch (10 events): ~5-20 μs
- Insert: ~2-10 μs
- State clone: ~1-10 μs

**Example:**
```bash
cargo bench -p kmb-bench kernel
```

### 4. Wire Protocol Benchmarks (`benches/wire.rs`)

Tests binary protocol serialization performance.

**Frame Operations:**
- Frame encode (64B to 16KB)
- Frame decode (64B to 16KB)

**Request Serialization:**
- CreateStream request
- AppendEvents request (1-100 events)
- Query request
- ReadEvents request

**Round-Trip:**
- Encode → Decode (1-50 events)

**Expected Performance:**
- Frame encode (1KB): ~500 ns - 2 μs
- Frame decode (1KB): ~1-3 μs
- Request serialize: ~1-10 μs
- Round-trip: ~2-20 μs

**Example:**
```bash
cargo bench -p kmb-bench wire
```

### 5. End-to-End Benchmarks (`benches/end_to_end.rs`)

Tests full system throughput from kernel to storage.

**Scenarios:**
- Full write path (kernel + storage)
- Write latency distribution (1000 writes with p50/p95/p99/p999)
- Sustained throughput (10,000 writes with fsync every 100)

**Metrics:**
- Per-operation latency (p50, p95, p99, p99.9)
- Sustained throughput (ops/sec)
- End-to-end latency including all layers

**Expected Performance:**
- Write latency p50: ~100-500 μs
- Write latency p99: ~1-5 ms
- Sustained throughput: ~1,000-10,000 ops/sec

**Example:**
```bash
cargo bench -p kmb-bench end_to_end
```

## Latency Tracking

The `LatencyTracker` utility provides histogram-based latency tracking:

```rust
use kmb_bench::LatencyTracker;
use std::time::Instant;

let mut tracker = LatencyTracker::new();

for _ in 0..1000 {
    let start = Instant::now();
    // ... perform operation ...
    let elapsed = start.elapsed();
    tracker.record(elapsed.as_nanos() as u64);
}

tracker.print_summary("MyOperation");
// Output:
// MyOperation Latency Statistics:
//   p50:       1234 ns (    1.23 μs)
//   p95:       5678 ns (    5.68 μs)
//   p99:       9012 ns (    9.01 μs)
//   p99.9:    12345 ns (   12.35 μs)
//   max:      20000 ns (   20.00 μs)
//   mean:      2500 ns (    2.50 μs)
```

## Baseline Comparison

Track performance changes over time by saving baselines:

```bash
# Save baseline before changes
cargo bench -p kmb-bench -- --save-baseline before-optimization

# Make code changes...

# Compare against baseline
cargo bench -p kmb-bench -- --baseline before-optimization

# Example output:
# blake3_hash/1024        time:   [498.23 ns 501.45 ns 504.98 ns]
#                         change: [-15.234% -12.567% -9.823%] (p = 0.00 < 0.05)
#                         Performance has improved.
```

## Continuous Benchmarking

### CI Integration

Add to your CI pipeline to catch performance regressions:

```yaml
# .github/workflows/bench.yml
name: Benchmarks
on: [push, pull_request]

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      - name: Run benchmarks
        run: cargo bench -p kmb-bench --no-fail-fast
```

### Pre-Commit Benchmarking

```bash
# Add to git pre-commit hook
#!/bin/bash
cargo bench -p kmb-bench crypto -- --quick
```

## Interpreting Results

### Criterion Output Format

```
blake3_hash/1024        time:   [498.23 ns 501.45 ns 504.98 ns]
                        thrpt:  [2.03 GB/s 2.04 GB/s 2.05 GB/s]
```

- **Lower bound**: 95% confidence interval minimum (498.23 ns)
- **Estimate**: Best estimate of mean (501.45 ns)
- **Upper bound**: 95% confidence interval maximum (504.98 ns)
- **thrpt**: Throughput calculation based on input size

### Regression Detection

Criterion automatically detects performance changes:

```
change: [-5.2345% -2.1234% +1.0123%] (p = 0.12 > 0.05)
No change in performance detected.

change: [+15.234% +18.567% +21.823%] (p = 0.00 < 0.05)
Performance has regressed.
```

- **p < 0.05**: Statistically significant change
- **Negative %**: Performance improvement (faster)
- **Positive %**: Performance regression (slower)

## Advanced Usage

### Custom Sample Sizes

```bash
# Run with more samples for accurate results (slower)
cargo bench -p kmb-bench crypto -- --sample-size 1000

# Run with fewer samples for quick check
cargo bench -p kmb-bench crypto -- --quick
```

### Specific Benchmarks

```bash
# Run only AES-GCM benchmarks
cargo bench -p kmb-bench crypto -- aes_gcm

# Run only 1KB size benchmarks
cargo bench -p kmb-bench -- /1024

# Run storage write benchmarks
cargo bench -p kmb-bench storage -- write
```

### Profiling Integration

```bash
# Generate flamegraph (requires cargo-flamegraph)
cargo install flamegraph
sudo cargo flamegraph --bench crypto

# Profile with perf (Linux only)
cargo bench -p kmb-bench crypto -- --profile-time=5
```

## Performance Targets

Based on PRESSURECRAFT.md requirements:

| Operation | Target | Measured | Status |
|-----------|--------|----------|--------|
| BLAKE3 1KB | < 1 μs | ~500 ns | ✅ |
| AES-GCM Encrypt 1KB | < 5 μs | ~2 μs | ✅ |
| Ed25519 Sign | < 100 μs | ~50 μs | ✅ |
| Storage Write 1KB | < 500 μs | ~200 μs | ✅ |
| Kernel AppendBatch | < 20 μs | ~10 μs | ✅ |
| Wire Roundtrip | < 10 μs | ~5 μs | ✅ |
| E2E Write p99 | < 5 ms | ~2 ms | ✅ |

## Memory Leak Detection

While Criterion doesn't directly test for memory leaks, you can use Valgrind or Miri:

### Valgrind (macOS/Linux)

```bash
# Install valgrind
# macOS: brew install valgrind
# Linux: sudo apt-get install valgrind

# Run benchmarks under valgrind
valgrind --leak-check=full cargo bench -p kmb-bench crypto -- --bench
```

### Miri (Undefined Behavior Detection)

```bash
# Install miri
rustup component add miri

# Run specific benchmark with miri
cargo miri test -p kmb-bench
```

## Troubleshooting

### "Could not create baseline directory"

```bash
# Ensure target directory is writable
mkdir -p target/criterion
chmod -R u+w target/criterion
```

### Benchmarks are too slow

```bash
# Use --quick for faster iteration
cargo bench -p kmb-bench -- --quick

# Or reduce sample size
cargo bench -p kmb-bench -- --sample-size 10
```

### High variance in results

- Close other applications consuming CPU
- Disable CPU frequency scaling (Linux):
  ```bash
  echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
  ```
- Run benchmarks multiple times and average

## Future Enhancements

Planned additions:
- Concurrent client benchmarks (multi-threaded stress testing)
- Network protocol benchmarks (full client-server)
- Query engine benchmarks (SQL parsing and execution)
- Index operation benchmarks (B-tree operations)
- Replication throughput benchmarks

## Resources

- **Criterion.rs**: https://bheisler.github.io/criterion.rs/book/
- **Flamegraph**: https://github.com/flamegraph-rs/flamegraph
- **HDR Histogram**: http://hdrhistogram.org/

For questions or issues, see the main Kimberlite README.
