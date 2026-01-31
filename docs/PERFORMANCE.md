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

### Load Testing

For end-to-end performance:

```bash
# Run load test with varying client counts
for clients in 1 10 50 100; do
    kmb-bench --clients $clients --duration 60s --report latency.csv
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

io_uring adoption is planned for Phase 7+ when:
- Linux 5.6+ is standard in production environments
- tokio-uring or monoio reaches 1.0 stability
- Simulation testing infrastructure can mock io_uring

---

## Performance Optimization Roadmap

This roadmap draws inspiration from best-in-class systems (TigerBeetle, FoundationDB, Turso, Iggy) to guide systematic performance improvements while maintaining Kimberlite's compliance-first architecture.

### Current Performance Gaps

**Critical Bottlenecks Identified:**
1. **Storage I/O (HIGHEST IMPACT):**
   - Full-file reads on every query (`storage.rs:368`)
   - Index written after EVERY batch (`storage.rs:291`)
   - O(n) verification from offset 0 by default
   - No mmap or async I/O

2. **Crypto Configuration (QUICK WIN):**
   - Missing SIMD/hardware acceleration features in dependencies
   - Cipher instantiation per operation (`encryption.rs:937,987`)

3. **State Management (MEDIUM):**
   - BTreeMap with String keys instead of HashMap (`state.rs:56`)
   - No LRU caching for hot metadata

4. **Benchmark Infrastructure (FOUNDATIONAL):**
   - Criterion and hdrhistogram configured but **completely unused**
   - No `benches/` directories
   - No regression detection in CI

### Best-in-Class Patterns Applied

| System | Pattern | Application to Kimberlite |
|--------|---------|---------------------------|
| **TigerBeetle** | Batching (8K txns/batch) | Command batching in kernel |
| | io_uring zero-syscall I/O | Future async I/O layer |
| | LSM incremental compaction | Segment rotation strategy |
| | Static memory allocation | Effect vector pooling |
| **FoundationDB** | Tag-based rate limiting | Future QoS implementation |
| | Hot shard migration | Segment balancing |
| | Extensive design docs | Performance documentation |
| **Turso** | State machine I/O patterns | Async storage wrapper |
| | MVCC snapshot isolation | Future query optimization |
| | Comprehensive benchmarks | Phase 2 infrastructure |
| **Iggy** | Thread-per-core shared-nothing | Future server architecture |
| | Zero-copy deserialization | Already using `bytes::Bytes` ✓ |

### Target Metrics (Post-Optimization)

| Metric | Current | Target | Improvement |
|--------|---------|--------|-------------|
| Append throughput | ~10K events/sec | 100K+ events/sec | **10x** |
| Read throughput | ~5 MB/s | 50 MB/s | **10x** |
| Index writes | Per batch | Every 100 records | **10-100x fewer** |
| Latency p99 | Unmeasured | < 10ms | Baseline needed |
| Verification | O(n) from genesis | O(k) from checkpoint | **10-100x faster** |

---

## Phase 1: Quick Wins (< 1 Day)

### 1.1 Enable Crypto Hardware Acceleration (30 min)

**File:** `Cargo.toml` (workspace root, lines 62-70)

**Changes:**
```toml
# Before
sha2 = "0.11.0-rc.3"
aes-gcm = "0.11.0-rc.2"

# After
sha2 = { version = "0.11.0-rc.3", features = ["asm"] }
aes-gcm = { version = "0.11.0-rc.2", features = ["aes"] }
```

**Expected Impact:** 2-3x faster crypto (SHA-256: 3 GB/s → 8 GB/s on x86 with AES-NI)

**Testing:** All crypto tests pass unchanged, benchmark to verify speedup

---

### 1.2 Replace BTreeMap with HashMap for table_name_index (45 min)

**File:** `crates/kmb-kernel/src/state.rs:56`

**Problem:** O(log n) lookups with String comparison overhead

**Changes:**
```rust
// Before
pub struct State {
    table_name_index: BTreeMap<String, TableId>,
}

// After
pub struct State {
    table_name_index: HashMap<String, TableId>,
}
```

**Expected Impact:** O(log n) → O(1) lookups, 5-10x faster for 1000+ tables

**Testing:** All kernel tests pass, serialization roundtrip works

---

### 1.3 Remove Debug Assertion Log Re-scans (15 min)

**File:** `crates/kmb-storage/src/storage.rs:151-165`

**Problem:** Debug builds re-scan entire log after index rebuild (O(n²) behavior)

**Changes:**
```rust
// Before: Full log scan in debug mode
debug_assert_eq!(index.len(), count_records_in_log(&log_path));

// After: Cheaper postcondition
debug_assert!(index.len() > 0, "Index should not be empty");
```

**Expected Impact:** 10-100x faster debug builds for large logs

---

### 1.4 Pre-allocate Effect Vectors (30 min)

**File:** `crates/kmb-kernel/src/kernel.rs:27`

**Changes:**
```rust
// Before
let mut effects = Vec::new();

// After
let mut effects = Vec::with_capacity(3);  // Most commands produce 2-3 effects
```

**Expected Impact:** Eliminate 1-2 reallocations per command

---

### 1.5 Make Checkpoint-Optimized Reads Default (1 hour)

**Files:** `crates/kmb-storage/src/storage.rs:360-647`

**Problem:** `read_records_from` always verifies from offset 0 (O(n))

**Changes:**
```rust
// Rename checkpoint-optimized version to be default
pub fn read_records_from(...) -> Result<Vec<Record>> {
    // Use checkpoint-based verification (O(k) where k = distance to checkpoint)
}

// Deprecate old version for testing only
#[doc(hidden)]
pub fn read_records_from_genesis(...) -> Result<Vec<Record>> {
    // Always verify from offset 0 (O(n))
}
```

**Expected Impact:** 10-100x faster reads with checkpoints (every 1000 records)

---

## Phase 2: Benchmark Infrastructure (1-2 Days)

### 2.1 Create Criterion Benchmark Suites (4 hours)

**New Directory Structure:**
```
crates/kmb-storage/benches/
  storage_benchmark.rs       # Append, read, verification
  index_benchmark.rs         # Build, lookup, save/load

crates/kmb-kernel/benches/
  kernel_benchmark.rs        # Command processing, state updates

crates/kmb-crypto/benches/
  crypto_benchmark.rs        # Hash, encrypt, key operations
```

**Benchmark Scenarios:**

**Storage (`storage_benchmark.rs`):**
- Append throughput: single record, batches (10, 100, 1000 events)
- Read throughput: sequential (1K, 10K, 100K records), random access
- Verification overhead: genesis vs checkpoint
- Index operations: rebuild, lookup, save/load

**Kernel (`kernel_benchmark.rs`):**
- Command processing rate: CreateStream, AppendBatch (varying sizes)
- State serialization/deserialization
- Effect generation overhead

**Crypto (`crypto_benchmark.rs`):**
- Hash throughput: SHA-256 vs BLAKE3 (256B, 4KB, 64KB, 1MB payloads)
- Encryption: AES-256-GCM encrypt/decrypt
- Key operations: generation, wrapping/unwrapping
- Cipher instantiation overhead

**Implementation Pattern (following Turso):**
```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};

fn bench_append_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("append_batch");

    for batch_size in [1, 10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("records", batch_size),
            batch_size,
            |b, &size| {
                let mut storage = Storage::new_temp();
                let events = vec![Bytes::from(vec![0u8; 1024]); size];
                b.iter(|| storage.append_batch(STREAM_ID, events.clone(), ...));
            },
        );
    }

    group.finish();
}
```

**Deliverables:**
- `cargo bench` runs all benchmarks
- HTML reports in `target/criterion/`
- Baseline for regression detection

---

### 2.2 HDR Histogram Integration (2 hours)

**Purpose:** Capture latency distribution (p50, p95, p99, p99.9) not just averages

**Pattern:**
```rust
use hdrhistogram::Histogram;

fn bench_append_latency(c: &mut Criterion) {
    let mut hist = Histogram::<u64>::new(3).unwrap();

    c.bench_function("append_with_histogram", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let op_start = Instant::now();
                black_box(storage.append(...));
                hist.record(op_start.elapsed().as_nanos() as u64).unwrap();
            }
            start.elapsed()
        });
    });

    // Report percentiles
    println!("p50:   {}ns", hist.value_at_quantile(0.50));
    println!("p95:   {}ns", hist.value_at_quantile(0.95));
    println!("p99:   {}ns", hist.value_at_quantile(0.99));
    println!("p99.9: {}ns", hist.value_at_quantile(0.999));
}
```

**Export to CSV for trending:**
```rust
let mut writer = csv::Writer::from_path("latency_percentiles.csv")?;
writer.write_record(&["percentile", "latency_ns"])?;
for percentile in [0.5, 0.9, 0.95, 0.99, 0.999] {
    writer.write_record(&[
        percentile.to_string(),
        hist.value_at_quantile(percentile).to_string()
    ])?;
}
```

---

### 2.3 Benchmark CI Integration (3 hours)

**New File:** `.github/workflows/benchmark.yml`

```yaml
name: Benchmarks

on:
  pull_request:
    branches: [main]
  push:
    branches: [main]

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      # Run benchmarks and save results
      - name: Run benchmarks
        run: |
          cargo bench --workspace -- --save-baseline pr-${{ github.event.number }}

      # Compare with main baseline
      - name: Compare with main
        if: github.event_name == 'pull_request'
        run: |
          git fetch origin main:main
          git checkout main
          cargo bench --workspace -- --save-baseline main
          git checkout -
          cargo bench --workspace -- --baseline main

      # Upload reports
      - name: Upload benchmark results
        uses: actions/upload-artifact@v4
        with:
          name: criterion-reports
          path: target/criterion/

      # Fail if regression > 10%
      - name: Check for regressions
        run: |
          python scripts/check_benchmark_regression.py --threshold 0.10
```

**Regression Detection Script:** `scripts/check_benchmark_regression.py`
```python
#!/usr/bin/env python3
import json
import sys
import argparse

def check_regressions(threshold=0.10):
    # Parse Criterion JSON outputs
    # Compare baseline vs current
    # Exit 1 if any benchmark > threshold slower
    pass

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--threshold", type=float, default=0.10)
    args = parser.parse_args()
    sys.exit(check_regressions(args.threshold))
```

---

## Phase 3: Storage Layer Optimizations (3-5 Days)

### 3.1 Implement mmap Support for Large Segments (1 day)

**Problem:** `storage.rs:368` reads entire file into memory with `fs::read()`

**New File:** `crates/kmb-storage/src/mmap.rs`

```rust
use memmap2::Mmap;
use bytes::Bytes;
use std::fs::File;
use std::path::{Path, PathBuf};

pub struct MappedSegment {
    mmap: Mmap,
    path: PathBuf,
}

impl MappedSegment {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        Ok(Self {
            mmap,
            path: path.to_path_buf(),
        })
    }

    /// Zero-copy access to mmap'd data
    pub fn as_bytes(&self) -> &[u8] {
        &self.mmap
    }

    /// Create a Bytes handle (reference-counted, zero-copy)
    pub fn slice(&self, range: std::ops::Range<usize>) -> Bytes {
        Bytes::copy_from_slice(&self.mmap[range])
    }
}
```

**Changes to `storage.rs`:**
```rust
pub struct Storage {
    // ... existing fields
    mmap_cache: HashMap<StreamId, MappedSegment>,
    mmap_threshold: u64,  // Default: 1 MB
}

impl Storage {
    fn read_segment_data(&mut self, stream_id: StreamId) -> Result<Bytes> {
        let segment_path = self.segment_path(stream_id);
        let metadata = fs::metadata(&segment_path)?;

        if metadata.len() >= self.mmap_threshold {
            // Use mmap for large files
            let mapped = self.mmap_cache
                .entry(stream_id)
                .or_insert_with(|| MappedSegment::open(&segment_path).unwrap());
            Ok(Bytes::copy_from_slice(mapped.as_bytes()))
        } else {
            // Standard read for small files
            Ok(fs::read(&segment_path)?.into())
        }
    }
}
```

**Expected Impact:**
- 2-5x faster reads for large files (> 1 MB)
- Reduced memory pressure (OS manages pages)
- Better multi-threaded read performance

**Dependencies:** Add `memmap2 = "0.9"` to workspace `Cargo.toml`

**Testing:**
- Parametrize tests: mmap vs read
- Test edge cases: empty files, concurrent access
- Test segment rotation (munmap old, map new)

**Risks:** Platform-specific (Unix/Windows only), requires `unsafe` (well-encapsulated)

---

### 3.2 Batch Index Writes (Flush Every N Records) (6 hours)

**Problem:** `storage.rs:291` writes index after EVERY batch

**Design:**
```rust
pub struct Storage {
    // ... existing fields
    index_dirty: HashMap<StreamId, bool>,
    index_flush_threshold: usize,  // Default: 100 records
    index_flush_bytes: u64,        // Default: 1 MB
}

impl Storage {
    pub fn append_batch(...) -> Result<...> {
        // ... append records to log ...

        // Update index in memory
        for record in &records {
            index.append(byte_position);
        }
        self.index_dirty.insert(stream_id, true);

        // Flush only if threshold met
        let should_flush =
            index.len() % self.index_flush_threshold == 0 ||
            index.estimated_size() >= self.index_flush_bytes ||
            fsync;  // Always flush if fsync requested

        if should_flush {
            index.save(&index_path)?;
            self.index_dirty.insert(stream_id, false);
        }

        Ok(...)
    }

    /// Explicitly flush all dirty indexes
    pub fn flush_indexes(&mut self) -> Result<()> {
        for (stream_id, dirty) in &self.index_dirty {
            if *dirty {
                let index = self.index_cache.get(stream_id).unwrap();
                index.save(&self.index_path(*stream_id))?;
            }
        }
        self.index_dirty.clear();
        Ok(())
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        let _ = self.flush_indexes();  // Flush on shutdown
    }
}
```

**Recovery Strategy:**
- On startup, compare index record count with log
- If mismatch, rebuild index from last checkpoint
- Guarantee: index never ahead of log, at most N records behind

**Expected Impact:** 10-100x fewer index writes, amortized cost

**Testing:**
- Test crash recovery (index behind log)
- Test explicit flush
- Test threshold triggers
- Verify correctness after partial flush

---

### 3.3 Optimize Checkpoint-Based Verification (4 hours)

**Current:** `storage.rs:570-647` has checkpoint logic but not optimized

**Optimizations:**

1. **Persist checkpoint index to disk:**
   ```rust
   // Format: segment_000000.log.ckpt
   pub struct CheckpointFile {
       magic: [u8; 4],      // "CKPT"
       version: u8,
       reserved: [u8; 3],
       count: u64,
       offsets: Vec<Offset>,  // Checkpoint positions
       crc32: u32,
   }
   ```

2. **Parallel verification using rayon:**
   ```rust
   use rayon::prelude::*;

   pub fn read_records_verified_parallel(
       &mut self,
       stream_id: StreamId,
       from_offset: Offset,
       max_bytes: u64,
   ) -> Result<Vec<Record>> {
       let checkpoints = self.get_or_rebuild_checkpoint_index(stream_id)?;
       let chunks = checkpoints.chunks_between(from_offset, max_bytes);

       // Verify chunks in parallel
       chunks.par_iter()
           .map(|chunk| self.verify_chunk(chunk))
           .collect::<Result<Vec<_>, _>>()?
           .into_iter()
           .flatten()
           .collect()
   }
   ```

**Expected Impact:** 2-4x faster verification on multi-core (opt-in for reads > 1 MB)

**Testing:**
- Compare results with serial version
- Test edge cases (single chunk, empty ranges)

**Risk:** Parallel overhead for small reads - make opt-in

---

### 3.4 Add Async I/O with Tokio (2 days) - OPTIONAL

**New Crate:** `crates/kmb-storage-async/`

**Dependencies:**
```toml
tokio = { version = "1", features = ["fs", "io-util", "rt-multi-thread"] }
```

**Design:**
```rust
pub struct AsyncStorage {
    inner: Storage,  // Wraps sync storage
}

impl AsyncStorage {
    pub async fn append_batch_async(
        &mut self,
        stream_id: StreamId,
        events: Vec<Bytes>,
        expected_offset: Offset,
        prev_hash: Option<ChainHash>,
    ) -> Result<(Offset, ChainHash)> {
        let segment_path = self.segment_path(stream_id);

        // Async file I/O
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&segment_path)
            .await?;

        for event in events {
            file.write_all(&event).await?;
        }

        file.sync_all().await?;

        Ok((next_offset, final_hash))
    }
}
```

**Expected Impact:** 2-5x higher throughput under concurrent load

**Risks:** Complexity increase - make opt-in for high-throughput servers

---

### 3.5 Segment Rotation (1 day)

**Design:**
```rust
const MAX_SEGMENT_SIZE: u64 = 256 * 1024 * 1024;  // 256 MB

impl Storage {
    pub fn maybe_rotate_segment(&mut self, stream_id: StreamId) -> Result<()> {
        let current_size = self.segment_size(stream_id)?;

        if current_size >= MAX_SEGMENT_SIZE {
            self.rotate_segment(stream_id)?;
        }

        Ok(())
    }

    fn rotate_segment(&mut self, stream_id: StreamId) -> Result<()> {
        let segment_number = self.next_segment_number(stream_id);
        let new_path = self.segment_path_numbered(stream_id, segment_number);

        // Close current segment
        // Create new segment file
        // Update in-memory mapping
        // Create new index for new segment

        Ok(())
    }
}
```

**Segment Naming:**
- `segment_000000.log` → current
- `segment_000001.log` → after first rotation
- Each has index: `.log.idx`

**Expected Impact:** Bounded file sizes, better filesystem performance

---

## Phase 4: Kernel & Command Processing (2-3 Days)

### 4.1 Implement Command Batching (1 day)

**Problem:** `kernel.rs:26` processes one command at a time

**New Function:**
```rust
pub fn apply_committed_batch(
    mut state: State,
    commands: Vec<Command>
) -> Result<(State, Vec<Effect>), KernelError> {
    let mut all_effects = Vec::with_capacity(commands.len() * 3);

    for cmd in commands {
        let (new_state, effects) = apply_committed(state, cmd)?;
        state = new_state;
        all_effects.extend(effects);
    }

    // Merge consecutive appends to same stream
    let merged_effects = merge_storage_effects(all_effects);

    Ok((state, merged_effects))
}

fn merge_storage_effects(effects: Vec<Effect>) -> Vec<Effect> {
    let mut merged = Vec::new();
    let mut current_append: Option<Effect> = None;

    for effect in effects {
        match (current_append.take(), effect) {
            (Some(Effect::StorageAppend { stream_id: s1, events: e1, .. }),
             Effect::StorageAppend { stream_id: s2, events: e2, .. })
                if s1 == s2 => {
                // Merge events into single append
                let mut combined = e1;
                combined.extend(e2);
                current_append = Some(Effect::StorageAppend {
                    stream_id: s1,
                    events: combined,
                    ...
                });
            }
            (Some(append), other) => {
                merged.push(append);
                current_append = Some(other);
            }
            (None, effect) => {
                current_append = Some(effect);
            }
        }
    }

    if let Some(append) = current_append {
        merged.push(append);
    }

    merged
}
```

**Expected Impact:** 5-10x higher command throughput, matches TigerBeetle's batching

**Testing:**
- Batch vs single-command equivalence
- Test batch size limits (1, 10, 100, 1000)
- Test failure mid-batch (atomicity)

---

### 4.2 Add State Snapshots/Checkpoints (1 day)

**Problem:** State must be rebuilt from command log on restart

**New File:** `crates/kmb-kernel/src/snapshot.rs`

```rust
#[derive(Serialize, Deserialize)]
pub struct StateSnapshot {
    version: u8,
    offset: u64,  // Command log offset
    state: State,
    checksum: [u8; 32],  // SHA-256 of serialized state
}

impl StateSnapshot {
    pub fn create(state: &State, offset: u64) -> Self {
        let serialized = bincode::serialize(state).unwrap();
        let checksum = sha2::Sha256::digest(&serialized).into();

        Self {
            version: 1,
            offset,
            state: state.clone(),
            checksum,
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let data = bincode::serialize(self)?;

        // Atomic write: temp + fsync + rename
        let temp = path.with_extension("tmp");
        fs::write(&temp, data)?;
        File::open(&temp)?.sync_all()?;
        fs::rename(&temp, path)?;

        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read(path)?;
        let snapshot: Self = bincode::deserialize(&data)?;

        // Verify checksum
        let serialized = bincode::serialize(&snapshot.state)?;
        let computed = sha2::Sha256::digest(&serialized).into();

        if computed != snapshot.checksum {
            return Err(Error::CorruptedSnapshot);
        }

        Ok(snapshot)
    }
}
```

**Policy:**
- Snapshot every 10,000 commands
- Keep last 3 snapshots for redundancy
- File naming: `state_000000.snap`, `state_000001.snap`

**Recovery:**
```rust
pub fn recover_state() -> Result<State> {
    // Try latest snapshot
    if let Ok(snapshot) = StateSnapshot::load_latest() {
        // Replay commands from snapshot offset to current
        let state = replay_from(snapshot.state, snapshot.offset)?;
        return Ok(state);
    }

    // Fall back to full replay from genesis
    let state = replay_from(State::default(), 0)?;
    Ok(state)
}
```

**Expected Impact:** Near-instant startup (vs replaying millions of commands)

---

### 4.3 Cache Frequently Accessed State (4 hours)

**Dependencies:** Add `lru = "0.12"` to workspace

**Design:**
```rust
use lru::LruCache;

pub struct State {
    // ... existing fields

    // Cached hot paths
    stream_cache: LruCache<StreamId, Arc<StreamMetadata>>,
    table_cache: LruCache<String, Arc<TableMetadata>>,
}

impl State {
    pub fn get_stream_cached(&mut self, id: &StreamId) -> Option<Arc<StreamMetadata>> {
        if let Some(cached) = self.stream_cache.get(id) {
            return Some(Arc::clone(cached));
        }

        let meta = self.streams.get(id)?;
        let arc = Arc::new(meta.clone());
        self.stream_cache.put(*id, Arc::clone(&arc));
        Some(arc)
    }

    pub fn invalidate_stream(&mut self, id: &StreamId) {
        self.stream_cache.pop(id);
    }
}
```

**Expected Impact:** 2-5x faster hot metadata access

**Testing:**
- Test cache invalidation on updates
- Test cache eviction (LRU policy)
- Benchmark cache hit rate

---

## Phase 5: Crypto & Encoding (1-2 Days)

### 5.1 Cache Cipher Instances (2 hours)

**Problem:** `encryption.rs:937,987` instantiates cipher every encrypt/decrypt

**Design:**
```rust
use std::sync::OnceLock;

pub struct EncryptionKey {
    bytes: [u8; KEY_LENGTH],
    cipher: OnceLock<Aes256Gcm>,
}

impl EncryptionKey {
    fn cipher(&self) -> &Aes256Gcm {
        self.cipher.get_or_init(|| {
            Aes256Gcm::new_from_slice(&self.bytes)
                .expect("KEY_LENGTH is always valid")
        })
    }
}

pub fn encrypt(key: &EncryptionKey, nonce: &Nonce, plaintext: &[u8]) -> Ciphertext {
    let cipher = key.cipher();
    let data = cipher.encrypt(&nonce.0.into(), plaintext)
        .expect("AES-GCM encryption cannot fail");
    Ciphertext(data)
}
```

**Expected Impact:** 10-30% faster encrypt/decrypt

**Testing:**
- Roundtrip encrypt/decrypt
- Test multiple operations with same key
- Verify Zeroize still works

**Risk:** `OnceLock` adds overhead - custom `Drop` to zeroize cipher

---

### 5.2 Batch Record Encryption (6 hours)

**Design:**
```rust
use rayon::prelude::*;

pub fn encrypt_batch(
    key: &EncryptionKey,
    records: &[(u64, &[u8])],  // (position, plaintext)
) -> Vec<(Nonce, Ciphertext)> {
    records.par_iter()  // Parallel encryption
        .map(|(pos, plaintext)| {
            let nonce = Nonce::from_position(*pos);
            let ciphertext = encrypt(key, &nonce, plaintext);
            (nonce, ciphertext)
        })
        .collect()
}
```

**Integration:**
- Batch encrypt during `append_batch` before writing
- Use rayon for parallel encryption (AES-GCM is embarrassingly parallel)

**Expected Impact:** 2-4x faster encryption on multi-core

**Testing:**
- Compare batch vs individual
- Benchmark batch sizes
- Verify correctness

**Risk:** Overhead for small batches - make parallel for batch > 10

---

## Phase 6: Testing & Validation (Ongoing)

### 6.1 Performance Regression Tests

**Pattern:**
```rust
#[test]
fn perf_regression_append_1k_records() {
    let start = Instant::now();

    let mut storage = Storage::new_temp();
    for i in 0..1000 {
        storage.append_batch(...).unwrap();
    }

    let elapsed = start.elapsed();

    // Regression threshold: 10% slower than baseline
    const BASELINE_MS: u64 = 50;
    assert!(
        elapsed.as_millis() < BASELINE_MS * 110 / 100,
        "Regression: took {}ms, expected <{}ms",
        elapsed.as_millis(),
        BASELINE_MS * 110 / 100
    );
}
```

**Integration:** Run in CI, update baselines on improvements

---

### 6.2 Comparative Benchmarks vs TigerBeetle Patterns

**Scenarios:**
1. **Batching impact:** Measure throughput at batch_size [1, 10, 100, 1000, 8000]
2. **Verification overhead:** Checkpoint frequency analysis
3. **Memory patterns:** Profile allocations with dhat/heaptrack

**Deliverable:** Document comparing Kimberlite vs TigerBeetle/FoundationDB patterns

---

### 6.3 Load Testing with Realistic Workloads (2 days)

**New Tool:** `tools/load-test/`

**Scenarios:**

1. **Write-Heavy (Event Sourcing):**
   - 10K events/sec sustained
   - Measure: throughput, latency, tail latencies

2. **Read-Heavy (Audit Queries):**
   - Random historical reads
   - Sequential scans
   - Measure: cold vs warm cache

3. **Mixed Workload:**
   - 70% writes, 30% reads
   - Concurrent clients

**Pattern:**
```rust
use hdrhistogram::Histogram;
use rayon::prelude::*;

fn load_test_append(duration: Duration, concurrency: usize) {
    let hist = Arc::new(Mutex::new(Histogram::<u64>::new(3).unwrap()));

    (0..concurrency).into_par_iter().for_each(|_| {
        let start = Instant::now();
        while start.elapsed() < duration {
            let op_start = Instant::now();
            // Perform operation
            let latency = op_start.elapsed().as_micros() as u64;
            hist.lock().unwrap().record(latency).unwrap();
        }
    });

    let hist = hist.lock().unwrap();
    println!("p50: {}μs", hist.value_at_quantile(0.50));
    println!("p99: {}μs", hist.value_at_quantile(0.99));
    println!("p99.9: {}μs", hist.value_at_quantile(0.999));
}
```

---

### 6.4 Latency Percentile Tracking

**Infrastructure:**
- Export HDR histogram to Prometheus
- Grafana dashboard for p50/p95/p99/p99.9
- Track over time

---

## Critical Files for Implementation

### Highest Impact (Implement First)

1. **`crates/kmb-storage/src/storage.rs`** (CRITICAL)
   - Lines 368-409: Full-file reads → mmap (Phase 3.1)
   - Line 291: Index write every batch → batching (Phase 3.2)
   - Lines 225-302: append_batch → optimization targets

2. **`Cargo.toml`** (workspace root)
   - Lines 62-70: Add crypto SIMD features (Phase 1.1)
   - Add dependencies: `memmap2`, `lru` (Phase 3-4)

3. **`crates/kmb-kernel/src/state.rs`**
   - Line 56: BTreeMap → HashMap (Phase 1.2)
   - Add LRU cache fields (Phase 4.3)

4. **`crates/kmb-crypto/src/encryption.rs`**
   - Lines 937, 987: Cache cipher instances (Phase 5.1)
   - Add batch encryption support (Phase 5.2)

### New Files (Create in Phases)

5. **`crates/*/benches/*.rs`** (Phase 2) - Benchmark infrastructure
6. **`crates/kmb-storage/src/mmap.rs`** (Phase 3.1) - Memory mapping
7. **`crates/kmb-kernel/src/snapshot.rs`** (Phase 4.2) - State snapshots

---

## Success Metrics Summary

### Phase 1-2 (Foundation)
- ✓ Crypto ops 2-3x faster (SIMD enabled)
- ✓ Table lookups O(1) instead of O(log n)
- ✓ Benchmarks run in CI with regression detection
- ✓ Checkpoint-optimized reads as default

### Phase 3-4 (Core Performance)
- **Append throughput:** 10K → 100K events/sec (10x improvement)
- **Read throughput:** 5 MB/s → 50 MB/s (10x improvement)
- **Index I/O:** 10-100x fewer disk syncs
- **Verification:** O(n) → O(k) speedup
- **Startup:** Instant recovery with snapshots

### Phase 5-6 (Polish & Validation)
- **Encryption:** 20-30% faster with caching
- **Batch encryption:** 2-4x on multi-core
- **Latency p99:** < 10ms
- **Latency p99.9:** < 50ms

---

## Implementation Timeline

### Week 1: Foundation
- Days 1-2: Phase 1 (quick wins) + Phase 2.1 (benchmarks)
- Days 3-4: Phase 2.2-2.3 (HDR histogram + CI)
- Day 5: Baseline measurements, document current state

### Week 2-3: Core Optimizations
- Week 2: Phase 3 (storage layer)
- Week 3: Phase 4 (kernel & command processing)

### Week 4+: Advanced & Testing
- Week 4: Phase 5 (crypto optimizations)
- Ongoing: Phase 6 (testing, load testing, monitoring)

---

## Risk Mitigation

1. **Compliance First:** Never sacrifice SHA-256 verification or audit trail integrity
2. **Incremental:** Each phase independently valuable, can stop at any point
3. **Tested:** Benchmark + correctness tests for every change
4. **Reversible:** Feature flags for major architectural changes
5. **Profiled:** Measure with flamegraphs before optimizing
6. **Compatibility:** All optimizations maintain serialization format stability

---

## Next Steps (Immediate Action Items)

1. **Enable crypto SIMD features** (30 min) - Instant 2-3x crypto speedup
2. **Create first benchmark suite** (2 hours) - Establishes measurement baseline
3. **Profile current hot paths** (1 hour) - Use `just profile-vopr` to validate assumptions
4. **Run load test baseline** (1 hour) - Document current throughput/latency

**Total Quick Start:** ~4.5 hours to measurable improvements + baseline metrics
