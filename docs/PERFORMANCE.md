# Performance Guidelines

Kimberlite prioritizes correctness over performance, but that doesn't mean we ignore performance. This document describes our performance philosophy, optimization priorities, and guidelines for writing efficient code.

---

## Table of Contents

1. [Philosophy](#philosophy)
2. [Optimization Priority Order](#optimization-priority-order)
3. [Mechanical Sympathy](#mechanical-sympathy)
4. [Batching and Amortization](#batching-and-amortization)
5. [Zero-Copy Patterns](#zero-copy-patterns)
6. [Memory Layout](#memory-layout)
7. [I/O Patterns](#io-patterns)
8. [Benchmarking](#benchmarking)
9. [Anti-Patterns](#anti-patterns)

---

## Philosophy

### Correctness First, Then Performance

Performance optimizations are worthless if the system is incorrect. Our priority order:

1. **Correct**: The system does what it's supposed to do
2. **Clear**: The code is understandable and maintainable
3. **Fast**: The system performs well

Never sacrifice correctness or clarity for performance without explicit justification.

### Measure Before Optimizing

> "Premature optimization is the root of all evil" — Donald Knuth

Before optimizing:
1. **Profile**: Identify the actual bottleneck
2. **Measure**: Establish a baseline
3. **Optimize**: Make targeted changes
4. **Verify**: Confirm improvement without regression

### Predictable is Better Than Fast

A system with predictable latency is often better than one with lower average latency but high variance:

```
System A: p50=5ms, p99=8ms, p99.9=12ms  ← Preferred
System B: p50=3ms, p99=50ms, p99.9=500ms
```

Design for consistent performance, not peak performance.

---

## Optimization Priority Order

When optimizing, work through resources in this order:

```
1. Network      Most expensive, highest latency
      ↓
2. Disk         Order of magnitude faster than network
      ↓
3. Memory       Order of magnitude faster than disk
      ↓
4. CPU          Order of magnitude faster than memory
```

### Why This Order?

| Resource | Latency | Bandwidth | Cost to Improve |
|----------|---------|-----------|-----------------|
| Network | ~1ms | ~1 GB/s | Very expensive |
| SSD | ~100μs | ~3 GB/s | Expensive |
| Memory | ~100ns | ~50 GB/s | Moderate |
| CPU | ~1ns | N/A | Cheap (just wait) |

An optimization that reduces network round-trips will almost always beat one that reduces CPU cycles.

### Practical Implications

**Network**: Batch requests, use persistent connections, minimize round-trips
```rust
// Bad: N round-trips
for item in items {
    client.write(item).await?;
}

// Good: 1 round-trip
client.write_batch(items).await?;
```

**Disk**: Sequential I/O, batch writes, use fsync strategically
```rust
// Bad: Sync after every write
for record in records {
    file.write(&record)?;
    file.sync_all()?;  // Expensive!
}

// Good: Batch writes, sync once
for record in records {
    file.write(&record)?;
}
file.sync_all()?;
```

**Memory**: Avoid allocations in hot paths, reuse buffers
```rust
// Bad: Allocate per iteration
for item in items {
    let buffer = Vec::new();  // Allocation!
    serialize_into(&mut buffer, item);
}

// Good: Reuse buffer
let mut buffer = Vec::with_capacity(estimated_size);
for item in items {
    buffer.clear();
    serialize_into(&mut buffer, item);
}
```

**CPU**: Last resort; usually not the bottleneck
```rust
// Only optimize CPU when profiling shows it's the bottleneck
// and after all higher-priority optimizations are done
```

---

## Mechanical Sympathy

Understanding how hardware works leads to better performance.

### CPU Cache Hierarchy

```
┌─────────────────────────────────────────────────────────────┐
│                        CPU Core                              │
│  ┌─────────┐                                                │
│  │ L1 Cache│  32KB, ~4 cycles                               │
│  │  (data) │                                                │
│  └────┬────┘                                                │
│       ▼                                                     │
│  ┌─────────┐                                                │
│  │ L2 Cache│  256KB, ~12 cycles                             │
│  └────┬────┘                                                │
│       ▼                                                     │
│  ┌─────────┐                                                │
│  │ L3 Cache│  8-30MB, ~40 cycles (shared)                   │
│  └────┬────┘                                                │
│       ▼                                                     │
│  ┌─────────┐                                                │
│  │   RAM   │  ~100+ cycles                                  │
│  └─────────┘                                                │
└─────────────────────────────────────────────────────────────┘
```

**Implications**:
- Keep hot data small (fits in L1/L2)
- Access data sequentially when possible (prefetching)
- Avoid pointer chasing (linked lists are cache-hostile)

### Cache-Friendly Data Structures

```rust
// Cache-hostile: Each node is a separate allocation
struct LinkedList<T> {
    head: Option<Box<Node<T>>>,
}
struct Node<T> {
    value: T,
    next: Option<Box<Node<T>>>,
}

// Cache-friendly: Contiguous memory
struct Vec<T> {
    ptr: *mut T,
    len: usize,
    cap: usize,
}
```

For B+trees, we use:
- Large node sizes (4KB = page size)
- Contiguous arrays within nodes
- Predictable memory access patterns

### Page Alignment

Storage is organized into 4KB pages matching OS page size:

```rust
const PAGE_SIZE: usize = 4096;

#[repr(C, align(4096))]
struct Page {
    data: [u8; PAGE_SIZE],
}
```

Benefits:
- Aligned I/O is faster
- Direct I/O (O_DIRECT) requires alignment
- Memory-mapped I/O works at page granularity

---

## Batching and Amortization

Batching amortizes fixed costs across multiple operations.

### Write Batching

```rust
// Without batching: O(n) fsyncs
async fn write_unbatched(log: &mut Log, records: Vec<Record>) -> Result<()> {
    for record in records {
        log.append(&record)?;
        log.sync()?;  // Each sync is ~10ms on HDD, ~100μs on SSD
    }
    Ok(())
}

// With batching: O(1) fsyncs
async fn write_batched(log: &mut Log, records: Vec<Record>) -> Result<()> {
    for record in records {
        log.append(&record)?;
    }
    log.sync()?;  // One sync for entire batch
    Ok(())
}
```

### Network Batching

```rust
// Consensus with batching
impl Consensus {
    async fn propose(&mut self, commands: Vec<Command>) -> Result<Vec<Position>> {
        // One network round-trip for many commands
        let batch = Batch::new(commands);
        let positions = self.replicate_batch(batch).await?;
        Ok(positions)
    }
}
```

### Read Batching (Prefetching)

```rust
// Prefetch likely-needed pages
impl BTree {
    fn get(&self, key: &Key) -> Option<Value> {
        // Prefetch siblings while traversing
        let path = self.find_path(key);
        for node in path.windows(2) {
            let parent = node[0];
            let child = node[1];
            // Prefetch sibling nodes (likely to be needed for next query)
            self.prefetch_siblings(parent, child);
        }
        self.get_from_path(path)
    }
}
```

---

## Zero-Copy Patterns

Avoid copying data when possible.

### Bytes Instead of Vec

```rust
use bytes::Bytes;

// Copying: Each read creates a new Vec
fn read_record_copying(storage: &Storage, offset: u64) -> Vec<u8> {
    let data = storage.read(offset);
    data.to_vec()  // Copy!
}

// Zero-copy: Bytes is reference-counted
fn read_record_zero_copy(storage: &Storage, offset: u64) -> Bytes {
    storage.read(offset)  // Returns Bytes, no copy
}
```

### Borrowed vs Owned APIs

```rust
// Prefer borrowing in read-only operations
impl Record {
    // Good: Returns reference
    fn data(&self) -> &[u8] {
        &self.data
    }

    // Avoid: Unnecessary copy
    fn data_owned(&self) -> Vec<u8> {
        self.data.clone()
    }
}
```

### Memory-Mapped Reads

For large sequential reads, memory mapping avoids copies:

```rust
use memmap2::MmapOptions;

fn read_segment_mmap(path: &Path) -> Result<Mmap> {
    let file = File::open(path)?;
    let mmap = unsafe { MmapOptions::new().map(&file)? };
    Ok(mmap)
}

// Access data directly from kernel page cache
let data = &mmap[offset..offset + len];
```

---

## Memory Layout

How data is laid out in memory affects performance.

### Struct Layout

```rust
// Bad: Poor alignment, padding wasted
struct BadLayout {
    a: u8,   // 1 byte + 7 padding
    b: u64,  // 8 bytes
    c: u8,   // 1 byte + 7 padding
    d: u64,  // 8 bytes
}  // Total: 32 bytes, but only 18 bytes of data

// Good: Ordered by size, minimal padding
struct GoodLayout {
    b: u64,  // 8 bytes
    d: u64,  // 8 bytes
    a: u8,   // 1 byte
    c: u8,   // 1 byte + 6 padding
}  // Total: 24 bytes, same 18 bytes of data
```

### Arena Allocation

For many small, same-lifetime allocations:

```rust
use bumpalo::Bump;

fn process_batch(records: &[Record]) {
    // Arena for batch-lifetime allocations
    let arena = Bump::new();

    for record in records {
        // Allocates from arena, no individual free
        let parsed = arena.alloc(parse_record(record));
        process(parsed);
    }
    // All allocations freed at once when arena drops
}
```

### Small String Optimization

For short strings, avoid heap allocation:

```rust
use smartstring::alias::String as SmartString;

// Standard String: Always heap-allocated
let s: std::string::String = "hello".to_string();  // Heap allocation

// SmartString: Inline for short strings
let s: SmartString = "hello".into();  // Inline, no allocation
```

---

## I/O Patterns

Efficient I/O is critical for database performance.

### Sequential vs Random

```
Sequential read:  ~500 MB/s (SSD), ~150 MB/s (HDD)
Random read:      ~50 MB/s (SSD), ~1 MB/s (HDD)
```

The append-only log is designed for sequential I/O:
- Writes are always sequential (append)
- Reads during recovery are sequential (scan)
- Random reads go through indexed projections

### Direct I/O

Bypass the kernel page cache for predictable latency:

```rust
use std::os::unix::fs::OpenOptionsExt;

let file = OpenOptions::new()
    .read(true)
    .write(true)
    .custom_flags(libc::O_DIRECT)
    .open(path)?;
```

When to use:
- Large sequential writes (log segments)
- When you manage your own cache

When NOT to use:
- Small random reads (kernel cache helps)
- Reads that benefit from prefetching

### Async I/O with io_uring (Linux)

For high-throughput I/O on Linux:

```rust
// Future: io_uring support for batched async I/O
// Reduces syscall overhead significantly
```

---

## Modeling and Measurement

### Little's Law for Capacity Planning

**C = T × L** (Concurrency = Throughput × Latency)

Little's Law is a fundamental queueing theory principle that helps size bounded queues and thread pools.

**Application to Kimberlite:**
- Target: 100K appends/sec with p99 < 10ms latency
- Required concurrency: C = 100K/sec × 0.01sec = 1000 concurrent operations
- Monitor: If actual concurrency >> 1000, latency is degrading (queue buildup)
- Use: Size bounded queues and thread pools based on Little's Law

**Implementation:**
```rust
// Bounded channel sized by Little's Law
let target_throughput = 100_000; // ops/sec
let target_latency_sec = 0.01;   // 10ms
let max_concurrent = (target_throughput as f64 * target_latency_sec) as usize;
let (tx, rx) = bounded_channel(max_concurrent);
```

**Validation:** Track actual queue depth in metrics. If depth approaches capacity under normal load, either increase capacity or reduce latency to maintain throughput.

**Reference:** From "Latency: Reduce delay in software systems" by Pekka Enberg (ScyllaDB, Turso)

---

## Benchmarking

### Criterion for Micro-Benchmarks

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_log_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("log_append");

    for size in [64, 256, 1024, 4096].iter() {
        group.bench_with_input(
            BenchmarkId::new("record_size", size),
            size,
            |b, &size| {
                let mut log = Log::new_in_memory();
                let record = Record::new(vec![0u8; size]);
                b.iter(|| log.append(&record));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_log_append);
criterion_main!(benches);
```

### Latency Histograms

Track latency distribution, not just averages:

```rust
use hdrhistogram::Histogram;

struct LatencyTracker {
    histogram: Histogram<u64>,
}

impl LatencyTracker {
    fn record(&mut self, latency_ns: u64) {
        self.histogram.record(latency_ns).unwrap();
    }

    fn report(&self) {
        println!("p50:   {} ns", self.histogram.value_at_quantile(0.50));
        println!("p90:   {} ns", self.histogram.value_at_quantile(0.90));
        println!("p99:   {} ns", self.histogram.value_at_quantile(0.99));
        println!("p99.9: {} ns", self.histogram.value_at_quantile(0.999));
    }
}
```

### Empirical Cumulative Distribution Function (eCDF)

eCDF visualizes latency distribution more clearly than histograms for identifying tail latency issues:

- **Flat line** = consistent latency (good)
- **Steep curve at tail** = tail latency spike (investigate)

**Pattern to detect:**
- 99% of requests < 5ms, but p99.9 = 100ms → investigate outliers
- eCDF shows exact percentile for any latency value

**Implementation:**
```rust
/// Export HDR histogram to eCDF format for plotting
fn export_ecdf(hist: &Histogram<u64>) -> Vec<(f64, u64)> {
    (0..=999)
        .map(|p| {
            let percentile = p as f64 / 1000.0;
            let latency = hist.value_at_quantile(percentile);
            (percentile, latency)
        })
        .collect()
}

/// Save eCDF data to CSV for Grafana/plotting
fn save_ecdf_csv(hist: &Histogram<u64>, path: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record(&["percentile", "latency_ns"])?;

    for (percentile, latency) in export_ecdf(hist) {
        wtr.write_record(&[percentile.to_string(), latency.to_string()])?;
    }

    wtr.flush()?;
    Ok(())
}
```

**Grafana Integration:** Plot eCDF curves over time to detect latency regressions visually. A shift right in the eCDF curve indicates degradation.

### Load Testing

For end-to-end performance:

```bash
# Run load test with varying client counts
for clients in 1 10 50 100; do
    kimberlite-bench --clients $clients --duration 60s --report latency.csv
done
```

---

## Anti-Patterns

### Avoid: Allocation in Hot Paths

```rust
// Bad: Allocates on every call
fn process(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();  // Allocation!
    // ...
    output
}

// Good: Reuse buffer
fn process_into(input: &[u8], output: &mut Vec<u8>) {
    output.clear();
    // ...
}
```

### Avoid: Unbounded Queues

```rust
// Bad: Can grow without limit
let (tx, rx) = unbounded_channel();

// Good: Backpressure when queue is full
let (tx, rx) = bounded_channel(1024);
```

### Avoid: Sync in Loop

```rust
// Bad: N fsyncs
for record in records {
    write(&record)?;
    fsync()?;
}

// Good: 1 fsync
for record in records {
    write(&record)?;
}
fsync()?;
```

### Avoid: String Formatting in Hot Paths

```rust
// Bad: Allocates string
tracing::debug!("Processing record: {:?}", record);

// Good: Use structured logging
tracing::debug!(record_id = %record.id, "Processing record");
```

### Avoid: Clone When Borrow Suffices

```rust
// Bad: Unnecessary clone
fn process(data: String) {
    println!("{}", data);
}
process(my_string.clone());

// Good: Borrow
fn process(data: &str) {
    println!("{}", data);
}
process(&my_string);
```

---

## Summary

Kimberlite's performance philosophy:

1. **Correctness first**: Never sacrifice correctness for speed
2. **Network → Disk → Memory → CPU**: Optimize in this order
3. **Batch operations**: Amortize fixed costs
4. **Zero-copy**: Avoid unnecessary data movement
5. **Measure everything**: Profile before optimizing
6. **Predictable latency**: Prefer consistency over peak performance

When in doubt, write correct, clear code first. Optimize only when profiling shows it's necessary, and only the specific bottleneck identified.

---

## Benchmark Infrastructure

### Running Benchmarks

```bash
# Run all benchmarks
just bench

# Run specific benchmark group
cargo bench --bench crypto
cargo bench --bench storage
cargo bench --bench kernel

# Compare against baseline (CI mode)
cargo bench -- --baseline main

# Generate HTML report
cargo bench -- --plotting-backend plotters
```

### Benchmark Targets

These targets guide optimization efforts and CI regression detection:

| Operation | Target | Regression Threshold |
|-----------|--------|---------------------|
| `chain_hash` (SHA-256, 1 KiB) | 500 MB/s | -10% |
| `internal_hash` (BLAKE3, 1 KiB) | 5 GB/s | -10% |
| `internal_hash_parallel` (BLAKE3, 1 MiB) | 15 GB/s | -10% |
| `encrypt` (AES-256-GCM, 1 KiB) | 2 GB/s | -10% |
| `record_to_bytes` | 1M ops/s | -10% |
| `append_batch(100, fsync=false)` | 500K TPS | -20% |
| `append_batch(100, fsync=true)` | 100K TPS | -20% |
| `apply_committed` | 500K ops/s | -20% |
| `read_record` (with index) | < 100μs p99 | +50% |

### Profiling

```bash
# Generate flamegraph (requires cargo-flamegraph)
just flamegraph

# Interactive profiling with samply
just profile

# Linux perf profiling
just perf

# Latency histogram report
cargo run --example latency_report
```

---

## Hardware Acceleration

### Enabling CPU-Specific Optimizations

Add to `.cargo/config.toml`:

```toml
# Enable native CPU features (AES-NI, AVX2, etc.)
[target.'cfg(any(target_arch = "x86_64", target_arch = "aarch64"))']
rustflags = ["-Ctarget-cpu=native"]

# For reproducible builds, target a specific CPU level:
# [target.x86_64-unknown-linux-gnu]
# rustflags = ["-Ctarget-cpu=haswell"]  # AES-NI + AVX2
```

### Verifying Hardware Acceleration

```bash
# Check if AES-NI is used
cargo bench --bench crypto 2>&1 | grep -i aes

# Compare hardware vs software performance
RUSTFLAGS="-Ctarget-cpu=generic" cargo bench --bench crypto -- encrypt
RUSTFLAGS="-Ctarget-cpu=native" cargo bench --bench crypto -- encrypt
```

### Expected Speedups

| Feature | CPU Requirement | Speedup |
|---------|----------------|---------|
| AES-NI | Intel Westmere+ (2010), AMD Bulldozer+ | 10-20x for AES-GCM |
| ARMv8 Crypto | Apple M1+, AWS Graviton2+ | 10-15x for AES-GCM |
| AVX2 | Intel Haswell+ (2013), AMD Excavator+ | 2-3x for BLAKE3 |
| AVX-512 | Intel Skylake-X+, AMD Zen4+ | 1.5-2x over AVX2 |

---

## I/O Performance

### Group Commit Configuration

Kimberlite supports configurable fsync strategies to balance durability and throughput:

```rust
pub enum SyncPolicy {
    /// fsync every record - safest, ~1K TPS
    EveryRecord,

    /// fsync every batch - balanced, ~50K TPS
    EveryBatch,

    /// Group commit with max delay - fastest, ~100K TPS
    /// Multiple batches share one fsync
    GroupCommit { max_delay: Duration },

    /// No fsync, rely on OS - dangerous, only for testing
    OnFlush,
}
```

**Trade-off guidance**:
- **Compliance-critical**: Use `EveryBatch` (each batch is durable)
- **High-throughput**: Use `GroupCommit(5ms)` (lose up to 5ms of data on crash)
- **Testing/Development**: Use `OnFlush` (fastest, not durable)

### Checkpoints

Checkpoints enable fast recovery and verified reads without full log replay:

```rust
pub struct Checkpoint {
    /// Log position covered by this checkpoint
    position: Offset,
    /// Hash at this position (chain verification can start here)
    hash: ChainHash,
    /// Sparse offset index snapshot
    index: OffsetIndex,
    /// Creation timestamp
    created_at: Timestamp,
    /// Ed25519 signature for tamper evidence
    signature: Signature,
}
```

Checkpoints are created:
- Every 10,000-100,000 records (configurable)
- On graceful shutdown
- On explicit request

---

## Latency Monitoring

### Built-in Histograms

Kimberlite tracks latency distributions for critical operations using HDR histograms:

```rust
// Record a latency measurement
metrics.record("append_latency_ns", elapsed.as_nanos() as u64);

// Get percentiles
let report = metrics.report("append_latency_ns");
println!("p50: {}ns, p99: {}ns, p999: {}ns",
    report.p50, report.p99, report.p999);
```

### Metrics to Track

| Metric | Description | Target p99 |
|--------|-------------|-----------|
| `append_latency_ns` | Time to append + fsync | < 1ms (SSD) |
| `read_latency_ns` | Time to read single record | < 100μs |
| `encrypt_latency_ns` | Time to encrypt record | < 10μs |
| `hash_latency_ns` | Time to compute chain hash | < 5μs |
| `apply_latency_ns` | Time for kernel apply | < 1μs |
| `projection_lag_ns` | Delay from commit to projection | < 10ms |

### Avoiding Coordinated Omission

When benchmarking, avoid the "coordinated omission" trap:
- Use closed-loop benchmarks with arrival rate tracking
- Record intended send time, not just response time
- Use HDR histogram's correction for queue delays

---

## Future: io_uring

io_uring provides 60% latency reduction for I/O-bound workloads on Linux 5.6+.

### Architecture Preparation

```rust
pub trait IoBackend: Send + Sync {
    fn append(&self, data: &[u8]) -> impl Future<Output = Result<u64>>;
    fn read(&self, offset: u64, len: usize) -> impl Future<Output = Result<Bytes>>;
    fn sync(&self) -> impl Future<Output = Result<()>>;
}

// Synchronous backend for DST compatibility
pub struct SyncIoBackend { ... }

// io_uring backend for production (Linux only)
#[cfg(target_os = "linux")]
pub struct IoUringBackend { ... }
```

### When to Adopt

io_uring adoption is planned for future versions when:
- Linux 5.6+ is standard in production environments
- tokio-uring or monoio reaches 1.0 stability
- Simulation testing infrastructure can mock io_uring

---

## Current Performance Characteristics (v0.2.0)

This section documents the baseline performance characteristics of Kimberlite v0.2.0, establishing a reference point for future optimization work.

### Baseline Measurements

**Append Performance** (single-threaded, sync I/O):
- Throughput: ~10K events/sec
- Latency: Unmeasured (benchmark infrastructure configured but unused)
- Bottleneck: Synchronous fsync per batch

**Read Performance**:
- Random reads: Unmeasured
- Sequential scans: Unmeasured
- Bottleneck: Full-file reads on queries (`storage.rs:368`)

**Verification Performance**:
- Hash chain verification: O(n) from genesis by default
- Checkpoint optimization: Available but not default behavior
- Verified read performance: Depends on distance to nearest checkpoint

**Index Performance**:
- Index writes: Per batch (`storage.rs:291`)
- No batching or amortization
- Potential for 10-100x improvement with optimized write strategy

### Known Bottlenecks

The following bottlenecks have been identified and documented for future optimization:

1. **Storage I/O (HIGHEST IMPACT)**:
   - Full-file reads on every query (`storage.rs:368`)
   - Index written after EVERY batch (`storage.rs:291`)
   - O(n) verification from offset 0 by default
   - No mmap or async I/O

2. **Crypto Configuration (QUICK WIN)**:
   - Missing SIMD/hardware acceleration features in dependencies
   - Cipher instantiation per operation (`encryption.rs:937,987`)
   - Hardware features available but not enabled (AES-NI, SHA extensions)

3. **State Management (MEDIUM)**:
   - BTreeMap with String keys instead of HashMap (`state.rs:56`)
   - No LRU caching for hot metadata
   - Inefficient for high-throughput workloads

4. **Benchmark Infrastructure (FOUNDATIONAL)**:
   - Criterion and hdrhistogram configured but **completely unused**
   - No `benches/` directories in crates
   - No regression detection in CI
   - Cannot measure impact of optimizations

### Performance Philosophy

**Current Priority: Correctness > Performance**

v0.2.0 focuses on:
- Byzantine-resistant consensus
- Cryptographic integrity
- Compliance guarantees
- Comprehensive testing

Performance optimizations are deferred to ensure correctness is established first.

**Measurement Before Optimization**:

Before any performance work, we will:
1. Establish baseline benchmarks with Criterion
2. Add latency instrumentation (p50/p90/p99/p999 with hdrhistogram)
3. Profile hot paths with flamegraphs
4. Identify actual bottlenecks with data

Premature optimization is avoided. All optimization decisions will be data-driven.

### Optimization Roadmap

Planned performance improvements are documented in [ROADMAP.md](../ROADMAP.md#performance-optimization-roadmap).

**High-Priority Optimizations** (v0.3.0):
- Enable crypto hardware acceleration (2-3x improvement)
- Benchmark infrastructure (baseline metrics)
- Index write batching (10-100x fewer writes)
- HashMap for hot paths (O(1) vs O(log n))

**Medium-Priority Optimizations** (v0.4.0):
- Memory-mapped log files (mmap)
- LRU cache for metadata
- Direct I/O for append path
- Advanced cache replacement (SIEVE)

**Long-Term Optimizations** (v0.5.0+):
- io_uring async I/O (Linux)
- Thread-per-core architecture
- Tenant-level parallelism
- Zero-copy deserialization

See [ROADMAP.md](../ROADMAP.md) for detailed optimization phases and target metrics.

---

**Document Version**: 2.0 (Roadmap Extracted)
**Last Updated**: 2026-02-02
**Current State**: Trimmed to current practices only (was ~3,900 lines, now ~900 lines)

