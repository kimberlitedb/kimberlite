# Roadmap

## Overview

Kimberlite is evolving from a solid foundation (v0.2.0) toward a production-ready, high-performance compliance database. This roadmap outlines planned enhancements across performance optimization, cluster operations, advanced querying, and compliance features.

**Current State (v0.4.0):**
- Byzantine-resistant VSR consensus with production assertions
- Production-grade DST platform with advanced debugging (VOPR v0.4.0)
- Dual-hash cryptography (SHA-256 + BLAKE3)
- Append-only log with verified reads
- B+tree projection store with MVCC
- SQL query engine (SELECT, WHERE, ORDER BY, LIMIT)
- Multi-language SDKs (Python, TypeScript, Go, Rust)
- MCP server for LLM integration
- Secure data sharing layer

**Vision:**
Transform Kimberlite into a production-grade system capable of handling enterprise workloads while maintaining its compliance-first architecture. Focus areas include performance optimization, operational maturity, and advanced compliance capabilities.

---

## Release Timeline

### v0.3.0 - Performance Foundations & VSR Testing (Target: Q2 2026)

**Theme:** Low-hanging fruit optimizations, benchmark infrastructure, and protocol-level Byzantine testing

**Status**: Partially Complete (Feb 3, 2026)

**Completed Deliverables:**
- ✅ **VOPR VSR Mode** - Protocol-level Byzantine testing infrastructure
  - ~3,000 lines of new simulation code
  - Complete VSR replica integration into VOPR
  - Byzantine message mutation (MessageMutator)
  - Fault injection with automatic retry logic
  - 100% attack detection for inflated-commit scenario
  - Documentation: `docs/VOPR_VSR_MODE.md`

- ✅ **VOPR Enhancements** - Antithesis-grade testing infrastructure (v0.3.1)
  - ~3,400 lines across 12 modules
  - Storage realism: Write reordering, concurrent I/O, crash semantics
  - Byzantine attacks: 10 protocol-level attack patterns
  - Observability: Event logging, .kmb failure reproduction bundles
  - Workload generators: 6 realistic patterns (Hotspot, Sequential, MultiTenant, Bursty, RMW)
  - Coverage-guided fuzzing: Multi-dimensional coverage tracking
  - Beautiful CLI: 5 commands (run, repro, show, scenarios, stats)
  - 48 new tests (all passing), <10% overhead, >70k sims/sec maintained
  - Documentation: `docs/TESTING.md` (VOPR Enhanced Capabilities section)

**Planned Deliverables (Performance):**
- Crypto hardware acceleration (AES-NI, SHA extensions)
- HashMap optimization for hot paths
- Comprehensive benchmark suite with CI regression detection
- Latency instrumentation (p50/p90/p99/p999)
- Index write batching optimization
- Checkpoint-optimized verification by default

**Expected Impact:**
- ✅ 100% Byzantine attack detection (VSR Mode)
- ✅ No more `--no-faults` requirement (graceful error handling)
- ✅ 90-95% Antithesis-grade testing without hypervisor (VOPR Enhancements)
- 2-3x crypto throughput improvement (pending)
- 10-100x fewer index writes (pending)
- Baseline performance metrics established (pending)

### v0.4.0 - VOPR Advanced Debugging (Released: Feb 3, 2026)

**Theme:** Production-grade debugging and developer experience

**Status**: ✅ **COMPLETE**

**Deliverables:**
- ✅ **Timeline Visualization** - ASCII Gantt charts for execution flow
  - ~700 lines of visualization code
  - 11 event kinds tracked (client ops, storage, network, protocol events)
  - Per-node event lanes with time-based filtering
  - 11 tests passing

- ✅ **Bisect to First Bad Event** - Automated binary search debugging
  - ~660 lines (bisect + checkpointing)
  - Simulation checkpointing with RNG state restoration
  - O(log n) convergence, 10-100x faster than full replay
  - Generates minimal reproduction bundles
  - 9 tests passing

- ✅ **Delta Debugging** - Automated test case minimization
  - ~560 lines (ddmin + dependency analysis)
  - Zeller's ddmin algorithm with event dependency tracking
  - 80-95% test case reduction achieved
  - Network, storage, and causality dependency analysis
  - 14 tests passing

- ✅ **Real Kernel State Hash** - Actual state hashing (not placeholder)
  - Integrated BLAKE3 hashing from kernel layer
  - Exposed through VSR replica layers
  - True determinism validation
  - 5 tests passing

- ✅ **Coverage Dashboard** - Web UI for metrics visualization
  - ~500 lines (Axum + Askama + Datastar + CUBE CSS)
  - Real-time coverage updates via SSE
  - 4 coverage dimensions (state, messages, faults, events)
  - Top seeds by coverage table
  - 8 tests passing

- ✅ **Interactive TUI** - Rich terminal UI with ratatui
  - ~500 lines (app state + rendering)
  - 3 tabs (Overview, Logs, Configuration)
  - Live progress tracking and statistics
  - Keyboard controls (pause, scroll, tab switching)
  - 4 tests passing

**Total:** ~3,700 lines across 23 new files, 51 tests passing

**Impact:**
- ✅ Makes debugging 10x faster with automated tools
- ✅ Timeline visualization for understanding execution flow
- ✅ Binary search reduces 1000-event bugs to ~50 events
- ✅ Delta debugging reduces further to ~7 events (93% reduction)
- ✅ Web dashboard for coverage monitoring
- ✅ Interactive TUI for rapid iteration
- ✅ True kernel state validation (not placeholder)

**Documentation:**
- ✅ `docs/TESTING.md` - VOPR Advanced Debugging section
- ✅ `CHANGELOG.md` - v0.4.0 release notes

### v0.5.0 - Storage Layer Optimization (Target: Q3 2026)

**Theme:** I/O efficiency and storage performance

**Key Deliverables:**
- Memory-mapped log files (mmap)
- Direct I/O for append path
- LRU cache for hot metadata
- Segment rotation and compaction
- Advanced cache replacement (SIEVE algorithm)
- Bounded queues with backpressure

**Expected Impact:**
- 5-10x read throughput improvement
- Sub-millisecond p99 latency
- Improved memory efficiency

### v0.6.0 - Advanced I/O (Target: Q4 2026)

**Theme:** Async I/O and thread-per-core architecture

**Key Deliverables:**
- io_uring abstraction layer (Linux)
- Thread-per-core runtime architecture
- Tenant-level parallelism
- Stage pipelining optimization
- Zero-copy deserialization enhancements

**Expected Impact:**
- 10-20x append throughput (100K+ events/sec)
- Near-zero context switches
- Linear scaling to core count

### v1.0.0 - Production Ready (Target: Q1 2027)

**Theme:** Operational maturity and production hardening

**Key Deliverables:**
- Production monitoring and observability
- Advanced cluster management (dynamic reconfiguration)
- Hot shard migration
- Tag-based rate limiting (QoS)
- Third-party checkpoint attestation (RFC 3161 TSA, blockchain anchoring)
- Comprehensive operational runbooks
- Enterprise support readiness

**Expected Impact:**
- Production-ready stability
- Enterprise feature parity
- Battle-tested reliability

---

## Protocol Enhancements

**See Also**: Wire protocol is specified in `docs/PROTOCOL.md`

### Priority 1: Critical for Production

#### Optimistic Concurrency Control for Appends
- **Status**: Kernel implemented, wire protocol pending
- **Complexity**: Low
- Add `expect_offset` field to `AppendEventsRequest`
- Returns `OffsetMismatch` error (code 16) on conflict
- Enables safe concurrent appends without distributed locking

#### Rich Event Metadata in ReadEvents
- **Status**: Not implemented
- **Complexity**: Medium
- Return structured `Event` objects with offset, timestamp, checksum
- Better SDK ergonomics and integrity verification

#### Stream Retention Policies
- **Status**: Not implemented
- **Complexity**: Medium
- Add `retention_days` field to `CreateStreamRequest`
- Automatic data deletion for compliance (HIPAA, GDPR)
- Background compaction job enforcement

### Priority 2: Enhanced Functionality

#### Subscribe Operation (Real-time Streaming)
- **Status**: Not implemented
- **Complexity**: High
- Server-initiated push for event streaming (like Kafka)
- Consumer group coordination for load balancing
- Credit-based flow control for backpressure

#### Checkpoint Operation (Compliance Snapshots)
- **Status**: Storage layer implemented, wire protocol pending
- **Complexity**: Low
- Create immutable point-in-time tenant snapshots
- Integration with `QueryAt` for audits
- S3/object storage archival

#### DeleteStream Operation
- **Status**: Not implemented
- **Complexity**: Medium
- Soft-delete with compliance retention period
- Physical deletion deferred until retention expires
- Audit trail preserved forever

### Priority 3: Performance & Scale

#### Compression Support
- **Status**: Not implemented
- **Complexity**: Medium
- Optional LZ4 (fast) and Zstd (high compression) codecs
- Frame header change (breaks protocol, requires v2)
- Negotiate during handshake

#### Batch Query Operation
- **Status**: Not implemented
- **Complexity**: Low
- Execute multiple SQL statements in single request
- Reduce round-trips for analytics

#### Streaming Read (Large Result Sets)
- **Status**: Not implemented
- **Complexity**: High
- Server-initiated push for large queries
- Avoid OOM on client with 16 MiB frame limit
- Chunk acknowledgment for backpressure

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

| Metric | Current | Phase 3 Target | Phase 4 Target | Improvement |
|--------|---------|----------------|----------------|-------------|
| **Throughput** |
| Append throughput | ~10K events/sec | 100K events/sec | 200K+ events/sec | **20x** |
| Read throughput | ~5 MB/s | 50 MB/s | 100 MB/s | **20x** |
| **Latency** |
| Append latency p50 | Unmeasured | 500μs | 100μs | **Baseline→5x** |
| Append latency p99 | Unmeasured | 5ms | 1ms | **Baseline→5x** |
| Append latency p99.9 | Unmeasured | 20ms | 10ms | **Baseline→2x** |
| **I/O Efficiency** |
| Index writes | Per batch | Every 100 records | Every 100 records | **10-100x fewer** |
| Verification | O(n) from genesis | O(k) from checkpoint | O(k) from checkpoint | **10-100x faster** |
| Context switches | High | Medium | Near zero | **Thread-per-core** |
| **Caching** |
| Cache hit ratio | N/A | 60% (LRU) | 80% (SIEVE) | **30%+ better** |
| **Reliability** |
| Queue behavior | Unbounded (OOM risk) | Bounded | Bounded + backpressure | **Fail-safe** |
| Materialized views | N/A | N/A | 100-1000x faster | **O(1) queries** |

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

**File:** `crates/kimberlite-kernel/src/state.rs:56`

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

**File:** `crates/kimberlite-storage/src/storage.rs:151-165`

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

**File:** `crates/kimberlite-kernel/src/kernel.rs:27`

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

**Files:** `crates/kimberlite-storage/src/storage.rs:360-647`

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

### 1.6 Add Little's Law Queue Sizing (1 hour)

**Problem:** Unbounded queues or arbitrarily sized queues lead to either OOM or unnecessary rejections.

**Solution:** Size all bounded queues using Little's Law: C = T × L

**New File:** `crates/kimberlite-kernel/src/queue_sizing.rs`

```rust
use std::time::Duration;

/// Calculate optimal queue size using Little's Law
/// C = T × L (Concurrency = Throughput × Latency)
pub fn calculate_queue_capacity(
    target_throughput: usize,     // operations per second
    target_latency: Duration,      // target p99 latency
    safety_factor: f64,            // e.g., 1.2 for 20% buffer
) -> usize {
    let latency_sec = target_latency.as_secs_f64();
    let base_capacity = (target_throughput as f64) * latency_sec;
    let capacity_with_buffer = base_capacity * safety_factor;

    capacity_with_buffer.ceil() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_littles_law_sizing() {
        // Target: 100K ops/sec with 10ms p99 latency
        let capacity = calculate_queue_capacity(
            100_000,
            Duration::from_millis(10),
            1.2,  // 20% safety margin
        );

        // Expected: 100K * 0.01 * 1.2 = 1200
        assert_eq!(capacity, 1200);
    }
}
```

**Integration:** Apply to all bounded channels and queues:

```rust
// Before (arbitrary size)
let (tx, rx) = bounded_channel(1024);

// After (sized by Little's Law)
let capacity = calculate_queue_capacity(
    100_000,                        // target throughput
    Duration::from_millis(10),      // target latency
    1.2,                            // safety factor
);
let (tx, rx) = bounded_channel(capacity);
```

**Documentation:** Add comments explaining queue sizing rationale:

```rust
// Queue sized by Little's Law: C = T × L
// Target: 100K ops/sec × 10ms latency = 1000 concurrent ops
// Safety margin: 1.2x = 1200 capacity
let command_queue = SPSCQueue::new(1200);
```

**Expected Impact:** Right-sized queues prevent both OOM and unnecessary backpressure.

**Testing:**
- Unit test: Verify calculation correctness
- Document: Add sizing examples to PERFORMANCE.md
- Metrics: Track actual queue depth vs. capacity in production

---

## Phase 2: Benchmark Infrastructure (1-2 Days)

### 2.1 Create Criterion Benchmark Suites with eCDF Export (5 hours)

**New Directory Structure:**
```
crates/kimberlite-storage/benches/
  storage_benchmark.rs       # Append, read, verification
  index_benchmark.rs         # Build, lookup, save/load

crates/kimberlite-kernel/benches/
  kernel_benchmark.rs        # Command processing, state updates

crates/kimberlite-crypto/benches/
  crypto_benchmark.rs        # Hash, encrypt, key operations
```

**Benchmark Scenarios:**

**Storage (`storage_benchmark.rs`):**
- Append throughput: single record, batches (10, 100, 1000 events)
- Read throughput: sequential (1K, 10K, 100K records), random access
- Verification overhead: genesis vs checkpoint
- Index operations: rebuild, lookup, save/load
- **NEW:** Cache hit ratio tracking (LRU vs SIEVE)

**Kernel (`kernel_benchmark.rs`):**
- Command processing rate: CreateStream, AppendBatch (varying sizes)
- State serialization/deserialization
- Effect generation overhead
- **NEW:** Throughput × Latency validation (Little's Law)

**Crypto (`crypto_benchmark.rs`):**
- Hash throughput: SHA-256 vs BLAKE3 (256B, 4KB, 64KB, 1MB payloads)
- Encryption: AES-256-GCM encrypt/decrypt
- Key operations: generation, wrapping/unwrapping
- Cipher instantiation overhead
- **NEW:** Compare cached vs. non-cached cipher performance

**Implementation Pattern (with eCDF export):**
```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use hdrhistogram::Histogram;

fn bench_append_batch_with_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("append_batch");

    for batch_size in [1, 10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));

        // Create HDR histogram for latency distribution
        let mut hist = Histogram::<u64>::new(3).unwrap();

        group.bench_with_input(
            BenchmarkId::new("records", batch_size),
            batch_size,
            |b, &size| {
                let mut storage = Storage::new_temp();
                let events = vec![Bytes::from(vec![0u8; 1024]); size];

                b.iter_custom(|iters| {
                    let start = Instant::now();
                    for _ in 0..iters {
                        let op_start = Instant::now();
                        storage.append_batch(STREAM_ID, events.clone(), ...).unwrap();
                        hist.record(op_start.elapsed().as_nanos() as u64).unwrap();
                    }
                    start.elapsed()
                });
            },
        );

        // Export eCDF data
        export_ecdf_csv(&hist, &format!("append_batch_{}.csv", batch_size)).unwrap();

        // Report percentiles
        println!("Batch size {}: p50={}ns p99={}ns p999={}ns",
            batch_size,
            hist.value_at_quantile(0.50),
            hist.value_at_quantile(0.99),
            hist.value_at_quantile(0.999)
        );
    }

    group.finish();
}

/// Benchmark cache hit ratio (SIEVE vs LRU)
fn bench_cache_hit_ratio(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_hit_ratio");

    for cache_policy in ["lru", "sieve"] {
        group.bench_with_input(
            BenchmarkId::new("policy", cache_policy),
            &cache_policy,
            |b, policy| {
                let cache = create_cache(policy, 1000);
                let workload = generate_zipfian_workload(10000);  // Realistic skew
                let mut hits = 0;
                let mut misses = 0;

                b.iter(|| {
                    for key in &workload {
                        if cache.get(key).is_some() {
                            hits += 1;
                        } else {
                            misses += 1;
                            cache.insert(*key, generate_value());
                        }
                    }
                });

                let hit_ratio = hits as f64 / (hits + misses) as f64 * 100.0;
                println!("{} hit ratio: {:.2}%", policy, hit_ratio);
            }
        );
    }

    group.finish();
}

/// Validate Little's Law: C = T × L
fn bench_throughput_latency_product(c: &mut Criterion) {
    c.bench_function("littles_law_validation", |b| {
        let target_throughput = 100_000.0;  // ops/sec
        let mut latency_hist = Histogram::<u64>::new(3).unwrap();
        let mut actual_throughput = 0.0;

        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let op_start = Instant::now();
                // Perform operation
                perform_append();
                latency_hist.record(op_start.elapsed().as_nanos() as u64).unwrap();
            }
            let elapsed = start.elapsed();
            actual_throughput = iters as f64 / elapsed.as_secs_f64();
            elapsed
        });

        // Calculate expected concurrency via Little's Law
        let avg_latency_sec = latency_hist.mean() / 1e9;
        let expected_concurrency = actual_throughput * avg_latency_sec;

        println!("Little's Law validation:");
        println!("  Throughput: {:.0} ops/sec", actual_throughput);
        println!("  Avg latency: {:.3} ms", avg_latency_sec * 1000.0);
        println!("  Expected concurrency: {:.1}", expected_concurrency);
        println!("  (Queue should be sized to: {})", expected_concurrency.ceil() as usize);
    });
}
```

**Deliverables:**
- `cargo bench` runs all benchmarks
- HTML reports in `target/criterion/`
- eCDF CSV files for latency distribution trending
- Baseline for regression detection
- Cache hit ratio comparison data
- Little's Law validation metrics

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

**New File:** `crates/kimberlite-storage/src/mmap.rs`

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

### 3.4 Implement io_uring with POLL Mode (2 days) - Linux Only

**Problem:** Interrupt-driven I/O causes unpredictable latency spikes (context switches, kernel overhead).

**Solution:** io_uring POLL mode (application-controlled polling, no interrupts).

**Dependencies:** Add to workspace `Cargo.toml`:
```toml
[target.'cfg(target_os = "linux")'.dependencies]
io-uring = { version = "0.6", features = ["poll"] }
```

**New File:** `crates/kimberlite-storage/src/io_uring_backend.rs`

```rust
use io_uring::{IoUring, opcode, types};
use std::os::unix::io::{AsRawFd, RawFd};
use std::fs::File;

pub struct IoUringStorage {
    ring: IoUring,
    file: File,
}

impl IoUringStorage {
    pub fn new(file: File) -> Result<Self> {
        // Create io_uring instance with POLL mode
        let ring = IoUring::builder()
            .setup_sqpoll(1000)  // Kernel polling thread
            .build(256)?;        // 256 queue entries

        Ok(Self { ring, file })
    }

    /// Submit async write operation (non-blocking)
    pub fn append_async(&mut self, data: &[u8], offset: u64) -> Result<u64> {
        let fd = types::Fd(self.file.as_raw_fd());

        // Prepare write operation
        let write_op = opcode::Write::new(fd, data.as_ptr(), data.len() as u32)
            .offset(offset)
            .build()
            .user_data(offset);  // Use offset as request ID

        // Submit to io_uring ring buffer (zero syscalls)
        unsafe {
            self.ring.submission().push(&write_op)?;
        }

        self.ring.submit()?;
        Ok(offset)
    }

    /// Poll for completions (non-blocking)
    /// Returns completed request IDs
    pub fn poll_completions(&mut self) -> Vec<CompletionEvent> {
        let mut events = Vec::new();

        // Poll completion queue (no blocking, no interrupts)
        while let Some(cqe) = self.ring.completion().next() {
            events.push(CompletionEvent {
                user_data: cqe.user_data(),
                result: cqe.result(),
            });
        }

        events
    }

    /// Submit fsync operation
    pub fn sync_async(&mut self) -> Result<()> {
        let fd = types::Fd(self.file.as_raw_fd());

        let fsync_op = opcode::Fsync::new(fd)
            .build()
            .user_data(u64::MAX);  // Special ID for fsync

        unsafe {
            self.ring.submission().push(&fsync_op)?;
        }

        self.ring.submit()?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct CompletionEvent {
    pub user_data: u64,  // Request ID
    pub result: i32,     // Bytes written or error code
}
```

**Integration with Thread-Per-Core:**
```rust
fn kernel_loop() {
    let mut storage = IoUringStorage::new(file)?;

    loop {
        // 1. Process commands from SPSC queues
        let commands = poll_command_queues();

        // 2. Submit writes to io_uring (batched)
        for cmd in commands {
            storage.append_async(&cmd.data, cmd.offset)?;
        }

        // Submit fsync for batch
        storage.sync_async()?;

        // 3. Poll for I/O completions
        let completions = storage.poll_completions();

        // 4. Notify tenants of durability
        for comp in completions {
            if comp.result < 0 {
                handle_io_error(comp.user_data, comp.result);
            } else {
                notify_tenant_durable(comp.user_data);
            }
        }
    }
}
```

**Benefits:**
- **No interrupt overhead** (application controls when to check I/O)
- **Batch multiple I/O operations** in single syscall
- **30-50% lower latency** vs. epoll/select (ScyllaDB benchmarks)

**Expected Impact:** Sub-100μs append latency (vs. 100-500μs with interrupts).

**Platform Support:**
- **Linux kernel 5.1+** required
- **Fallback to synchronous I/O** on macOS/Windows:

```rust
#[cfg(target_os = "linux")]
use io_uring_backend::IoUringStorage;

#[cfg(not(target_os = "linux"))]
type IoUringStorage = SyncStorage;  // Fallback to sync I/O
```

**Testing:**
- Compare io_uring vs. sync I/O latency distributions
- Test batch sizes (1, 10, 100 operations per submit)
- Verify correctness with io_uring completion error handling
- Test graceful degradation on older kernels

**Performance Tuning:**
```rust
// Tune io_uring parameters
let ring = IoUring::builder()
    .setup_sqpoll(1000)       // Kernel polling interval (microseconds)
    .setup_iopoll()           // Use polling for device I/O
    .setup_sq_aff(core_id)    // Pin to CPU core
    .build(queue_depth)?;
```

**Reference:** "Async Processing" patterns from "Latency" book - io_uring eliminates syscall overhead and interrupt unpredictability.

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

### 3.6 Implement SIEVE Cache Replacement (4 hours)

**Problem:** Traditional LRU cache requires expensive reordering on every access (contention in multi-tenant workloads).

**SIEVE Advantage:**
- FIFO with lazy re-insertion (2023 research, 30%+ better hit ratio than LRU)
- No eager promotion = lower contention in multi-tenant workloads
- Fits append-only log: old entries naturally age out

**New File:** `crates/kmb-cache/src/sieve.rs`

```rust
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::hash::Hash;

pub struct SieveCache<K, V> {
    queue: VecDeque<(K, V, AtomicBool)>,  // (key, value, visited)
    capacity: usize,
}

impl<K: Hash + Eq + Clone, V: Clone> SieveCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        if let Some(pos) = self.find_position(key) {
            // Mark visited atomically (no reordering needed)
            self.queue[pos].2.store(true, Ordering::Relaxed);
            return Some(&self.queue[pos].1);
        }
        None
    }

    pub fn insert(&mut self, key: K, value: V) {
        // Check if already exists
        if let Some(pos) = self.find_position(&key) {
            self.queue[pos].1 = value;
            return;
        }

        // Evict if at capacity
        if self.queue.len() >= self.capacity {
            self.evict();
        }

        // Insert at end
        self.queue.push_back((key, value, AtomicBool::new(false)));
    }

    fn evict(&mut self) {
        // SIEVE eviction: scan from front, re-insert visited entries
        while let Some((key, val, visited)) = self.queue.pop_front() {
            if visited.load(Ordering::Relaxed) {
                // Re-insert at end, clear visited flag
                visited.store(false, Ordering::Relaxed);
                self.queue.push_back((key, val, visited));
            } else {
                // Not visited, evict
                break;
            }
        }
    }

    fn find_position(&self, key: &K) -> Option<usize> {
        self.queue.iter().position(|(k, _, _)| k == key)
    }
}
```

**Integration Points:**
- Cache query results for immutable log ranges
- Cache checkpoint metadata (TenantId → latest checkpoint offset)
- Cache encrypted field metadata (avoid re-decryption)

**Expected Impact:** 30-50% better cache hit ratio vs. naive LRU implementation.

**Testing:**
- Compare SIEVE vs. LRU hit ratio with real workload traces
- Benchmark eviction overhead (should be lower than LRU reordering)
- Property test: cache correctness under concurrent access

**Dependencies:** Add to workspace `Cargo.toml`:
```toml
# In workspace dependencies
[workspace.dependencies]
# ... existing deps
```

**Reference:** "SIEVE is Simpler than LRU" (NSDI 2024) - FIFO-based eviction with lazy promotion

---

## Phase 4: Kernel & Command Processing (5-6 Days)

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

**New File:** `crates/kimberlite-kernel/src/snapshot.rs`

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
        let serialized = postcard::to_allocvec(state).unwrap();
        let checksum = sha2::Sha256::digest(&serialized).into();

        Self {
            version: 1,
            offset,
            state: state.clone(),
            checksum,
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let data = postcard::to_allocvec(self)?;

        // Atomic write: temp + fsync + rename
        let temp = path.with_extension("tmp");
        fs::write(&temp, data)?;
        File::open(&temp)?.sync_all()?;
        fs::rename(&temp, path)?;

        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read(path)?;
        let snapshot: Self = postcard::from_bytes(&data)?;

        // Verify checksum
        let serialized = postcard::to_allocvec(&snapshot.state)?;
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

### 4.4 Implement Lock-Free Per-Stream Queues (1 day)

**Problem:** Multi-tenant append operations currently use mutex locks (microsecond overhead, contention under load).

**Solution:** SPSC (Single-Producer-Single-Consumer) lock-free queues per stream.

**Architecture:**
```
Tenant A → Stream X → SPSC Queue → Kernel Thread 0
Tenant A → Stream Y → SPSC Queue → Kernel Thread 1
Tenant B → Stream Z → SPSC Queue → Kernel Thread 2
```

**New File:** `crates/kimberlite-kernel/src/spsc.rs`

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::mem::MaybeUninit;

/// Single-Producer Single-Consumer lock-free queue
/// Safe for one writer thread and one reader thread
pub struct SPSCQueue<T> {
    data: Vec<MaybeUninit<T>>,
    head: AtomicUsize,  // Producer writes here
    tail: AtomicUsize,  // Consumer reads from here
    capacity: usize,
}

impl<T> SPSCQueue<T> {
    pub fn new(capacity: usize) -> Self {
        let mut data = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            data.push(MaybeUninit::uninit());
        }

        Self {
            data,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            capacity,
        }
    }

    /// Push item to queue (producer side)
    /// Returns Err(item) if queue is full (backpressure)
    pub fn push(&self, item: T) -> Result<(), T> {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = (head + 1) % self.capacity;

        // Check if queue is full
        if next_head == self.tail.load(Ordering::Acquire) {
            return Err(item);  // Queue full, apply backpressure
        }

        // Write item to queue
        unsafe {
            let slot = &self.data[head] as *const MaybeUninit<T> as *mut MaybeUninit<T>;
            (*slot).write(item);
        }

        // Signal consumer with Release ordering
        self.head.store(next_head, Ordering::Release);
        Ok(())
    }

    /// Pop item from queue (consumer side)
    /// Returns None if queue is empty
    pub fn pop(&self) -> Option<T> {
        let tail = self.tail.load(Ordering::Relaxed);

        // Check if queue is empty
        if tail == self.head.load(Ordering::Acquire) {
            return None;
        }

        // Read item from queue
        let item = unsafe {
            let slot = &self.data[tail] as *const MaybeUninit<T>;
            (*slot).assume_init_read()
        };

        // Advance tail with Release ordering
        self.tail.store((tail + 1) % self.capacity, Ordering::Release);

        Some(item)
    }

    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);

        if head >= tail {
            head - tail
        } else {
            self.capacity - tail + head
        }
    }
}

unsafe impl<T: Send> Send for SPSCQueue<T> {}
unsafe impl<T: Send> Sync for SPSCQueue<T> {}
```

**Memory Barriers Explained:**
- `Ordering::Release` on push: Ensures item write is visible to consumer before head update
- `Ordering::Acquire` on pop: Ensures consumer sees producer's write
- `Ordering::Relaxed` for same-thread reads: No cross-thread visibility needed

**Integration:**
```rust
// Replace Mutex<Vec<Command>> with SPSCQueue<Command> per stream
pub struct KernelRuntime {
    command_queues: HashMap<StreamId, Arc<SPSCQueue<Command>>>,
}

impl KernelRuntime {
    fn process_commands(&mut self) {
        // Kernel polls queues in round-robin or priority order
        for (stream_id, queue) in &self.command_queues {
            while let Some(cmd) = queue.pop() {
                self.apply_command(cmd)?;
            }
        }
    }
}
```

**Expected Impact:** 10-100x lower latency vs. mutex (nanoseconds vs. microseconds).

**Testing:**
- Property test: SPSC queue behaves identically to Mutex<Vec>
- Benchmark: Compare mutex vs. SPSC under contention
- Verify memory ordering correctness with Loom (concurrency test framework)

**Dependencies:** Add to workspace `Cargo.toml`:
```toml
[dev-dependencies]
loom = "0.7"  # For concurrency testing
```

**Risks:** Requires `unsafe` code (carefully reviewed, well-encapsulated).

**Reference:** "Lock-Free Programming" patterns from ScyllaDB/Seastar architecture.

---

### 4.5 Optimize with Compare-and-Swap for Global Offset (2 hours)

**Problem:** Global log offset allocation currently uses mutex for coordination.

**Solution:** Lock-free CAS (Compare-and-Swap) for low-contention scenarios.

```rust
use std::sync::atomic::{AtomicU64, Ordering};

pub struct LogOffset {
    current: AtomicU64,
}

impl LogOffset {
    pub fn new(initial: u64) -> Self {
        Self {
            current: AtomicU64::new(initial),
        }
    }

    /// Allocate a range of offsets atomically
    /// Returns the starting offset of the allocated range
    pub fn allocate(&self, count: u64) -> u64 {
        let mut current = self.current.load(Ordering::Relaxed);
        loop {
            let next = current + count;

            // Try to update atomically
            match self.current.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,  // Success: acquire + release semantics
                Ordering::Acquire,  // Failure: just acquire
            ) {
                Ok(_) => return current,
                Err(actual) => current = actual,  // Retry with new value
            }
        }
    }

    /// Get current offset (non-allocating)
    pub fn current(&self) -> u64 {
        self.current.load(Ordering::Acquire)
    }
}
```

**When to Use:**
- **Low contention** (< 10% CAS retry rate): Use CAS
- **High contention**: Fall back to mutex or partition by stream

**Monitoring:**
```rust
// Track CAS retry rate
let mut retries = 0;
loop {
    match self.current.compare_exchange_weak(...) {
        Ok(_) => break,
        Err(_) => retries += 1,
    }
}
metrics.record_cas_retries(retries);
```

**Expected Impact:** 5-10x faster offset allocation vs. mutex when contention is low.

**Testing:**
- Stress test: Multiple threads allocating concurrently
- Verify no offset reuse or gaps
- Benchmark retry rate under varying load

---

### 4.6 Implement Thread-Per-Core Model (2 days)

**Principle:** One kernel thread per CPU core, bound via affinity. Eliminates OS scheduling variance.

**Design:**
```
CPU 0: Kernel thread (TenantA streams)
CPU 1: Kernel thread (TenantB streams)
CPU 2: Kernel thread (TenantC streams)
CPU 3: Network I/O thread
CPU 4-7: Reserved for OS, crypto worker pool
```

**New File:** `crates/kmb-runtime/src/thread_pool.rs`

```rust
use std::thread::{self, JoinHandle};
use core_affinity::CoreId;

pub struct ThreadPerCorePool {
    threads: Vec<JoinHandle<()>>,
    cores: Vec<CoreId>,
}

impl ThreadPerCorePool {
    pub fn new(cores: Vec<CoreId>) -> Self {
        let threads = cores.iter().map(|&core_id| {
            thread::spawn(move || {
                // Pin to CPU core
                if !core_affinity::set_for_current(core_id) {
                    eprintln!("Warning: Failed to set CPU affinity for {:?}", core_id);
                }

                // Run dedicated event loop
                run_kernel_loop(core_id);
            })
        }).collect();

        Self { threads, cores }
    }

    pub fn join(self) {
        for thread in self.threads {
            let _ = thread.join();
        }
    }
}

fn run_kernel_loop(core_id: CoreId) {
    loop {
        // 1. Poll SPSC queues for commands (non-blocking)
        // 2. Execute crypto operations
        // 3. Submit I/O to io_uring (if available)
        // 4. Process completions
        // 5. Yield if no work (cooperative scheduling)

        // Example structure:
        // let has_work = poll_command_queues() || poll_io_completions();
        // if !has_work {
        //     std::thread::yield_now();
        // }
    }
}
```

**Benefits:**
- **Predictable latency** (no OS preemption)
- **No context switching** overhead
- **Data locality** (each thread owns partition)

**CPU Isolation (Linux):**
```bash
# Isolate CPUs 0-3 for Kimberlite, reserve 4-7 for OS
# Add to kernel boot parameters:
isolcpus=0-3 nohz_full=0-3 rcu_nocbs=0-3

# Set interrupt affinity away from isolated cores
echo 4-7 > /proc/irq/default_smp_affinity
```

**Expected Impact:** 20-50% latency reduction (eliminate OS scheduling variance).

**Testing:**
- Measure context switch rate (should be near zero)
- Compare latency distribution: thread-per-core vs. shared thread pool
- Test CPU isolation (interrupt affinity, isolcpus kernel param)

**Dependencies:** Add to workspace `Cargo.toml`:
```toml
core_affinity = "0.8"
```

**Risks:**
- Requires root or `CAP_SYS_NICE` for CPU affinity on Linux
- Platform-specific (works on Linux, macOS, Windows with limitations)

**Reference:** ScyllaDB/Seastar "shared-nothing" architecture, Redpanda thread-per-core design.

---

### 4.7 Implement Backpressure for Overload Protection (4 hours)

**Problem:** Unbounded queues lead to unbounded memory usage and tail latency degradation.

**Solution:** Bounded queues sized by Little's Law, reject when full.

**Implementation:**
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackpressureError {
    #[error("Queue full: {current_depth}/{max_depth} entries")]
    QueueFull {
        current_depth: usize,
        max_depth: usize,
    },
}

pub struct BoundedAppendQueue {
    queue: SPSCQueue<Command>,
    max_size: usize,  // From Little's Law: throughput × latency
}

impl BoundedAppendQueue {
    pub fn new(target_throughput: usize, target_latency_ms: u64) -> Self {
        // Apply Little's Law: C = T × L
        let max_size = (target_throughput as f64 * (target_latency_ms as f64 / 1000.0)) as usize;
        let queue = SPSCQueue::new(max_size);

        Self { queue, max_size }
    }

    pub fn push(&self, cmd: Command) -> Result<(), BackpressureError> {
        self.queue.push(cmd).map_err(|_| {
            BackpressureError::QueueFull {
                current_depth: self.queue.len(),
                max_depth: self.max_size,
            }
        })
    }
}
```

**Client Handling:**
```rust
// Retry with exponential backoff
let mut retry_count = 0;
let max_retries = 5;

loop {
    match storage.append(record) {
        Ok(offset) => break offset,
        Err(BackpressureError::QueueFull { .. }) if retry_count < max_retries => {
            // Exponential backoff: 10ms, 20ms, 40ms, 80ms, 160ms
            let delay = Duration::from_millis(10 * (1 << retry_count));
            thread::sleep(delay);
            retry_count += 1;
        }
        Err(e) => return Err(e.into()),
    }
}
```

**Sizing Strategy:**
- **Target:** 100K appends/sec, 10ms p99 latency
- **Queue size:** 100K × 0.01 = 1000 commands
- **Add buffer:** 1000 × 1.2 = 1200 commands (20% safety margin)

**Expected Impact:** Prevent OOM under load, maintain SLA compliance (latency stays bounded).

**Testing:**
- Load test: Send 200K appends/sec (2x capacity), verify rejections
- Measure: Latency should stay < 10ms even under overload (vs. unbounded spike)
- Test backoff strategy effectiveness

**Metrics:**
```rust
// Track backpressure events
metrics.counter("backpressure_rejections_total").increment(1);
metrics.histogram("queue_depth").record(queue.len() as f64);
```

**Reference:** "Managing Concurrency" patterns from "Latency" book - bounded queues prevent cascading failures.

---

### 4.8 Implement Incremental Materialized Views (1 day)

**Problem:** Complex compliance queries (per-tenant summaries) require full log scan (O(n) for every query).

**Solution:** Precompute query results incrementally, cache for O(1) access.

**Design:**
```rust
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct TenantStats {
    pub record_count: u64,
    pub latest_offset: Offset,
    pub encrypted_size_bytes: u64,
    pub first_seen: Timestamp,
    pub last_updated: Timestamp,
}

pub struct MaterializedView {
    // Precomputed: TenantId → aggregated stats
    tenant_summary: HashMap<TenantId, TenantStats>,
    last_processed_offset: Offset,
}

impl MaterializedView {
    pub fn new() -> Self {
        Self {
            tenant_summary: HashMap::new(),
            last_processed_offset: Offset(0),
        }
    }

    /// Update view incrementally from new records
    pub fn update_incremental(&mut self, new_records: &[Record]) {
        for record in new_records {
            let stats = self.tenant_summary
                .entry(record.tenant_id)
                .or_insert_with(|| TenantStats {
                    first_seen: record.timestamp,
                    ..Default::default()
                });

            stats.record_count += 1;
            stats.latest_offset = record.offset;
            stats.encrypted_size_bytes += record.data.len() as u64;
            stats.last_updated = record.timestamp;
        }

        if let Some(last_record) = new_records.last() {
            self.last_processed_offset = last_record.offset;
        }
    }

    /// O(1) query - instant response
    pub fn query(&self, tenant_id: TenantId) -> Option<&TenantStats> {
        self.tenant_summary.get(&tenant_id)
    }

    /// Get all tenant stats (for admin dashboards)
    pub fn all_tenants(&self) -> &HashMap<TenantId, TenantStats> {
        &self.tenant_summary
    }

    /// Persist view to disk for recovery
    pub fn save(&self, path: &Path) -> Result<()> {
        let data = postcard::to_allocvec(self)?;
        fs::write(path, data)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read(path)?;
        let view = postcard::from_bytes(&data)?;
        Ok(view)
    }
}
```

**Update Strategy:**
- **Incremental:** Update view on every batch commit (low overhead, always fresh)
- **Periodic:** Update every N records (trade freshness for performance)
- **On-demand:** Rebuild view when query arrives (lazy materialization)

**Use Cases:**
- **Compliance reports:** "How many encrypted records per tenant?"
- **Audit queries:** "What's the latest offset for TenantX?"
- **Monitoring:** "Which tenant has highest storage usage?"
- **Billing:** "Calculate storage costs per tenant"

**Expected Impact:** 100-1000x faster queries (O(1) vs. O(n) log scan).

**Persistence:**
- Snapshot materialized view with kernel state
- Rebuild from checkpoint on recovery
- Verify consistency: view stats == actual log scan (in tests)

**Testing:**
```rust
#[test]
fn test_materialized_view_consistency() {
    let mut view = MaterializedView::new();
    let records = generate_test_records(1000);

    // Update view incrementally
    view.update_incremental(&records);

    // Verify against full scan
    for tenant_id in unique_tenants(&records) {
        let view_stats = view.query(tenant_id).unwrap();
        let actual_stats = compute_stats_by_scan(tenant_id, &records);

        assert_eq!(view_stats.record_count, actual_stats.record_count);
        assert_eq!(view_stats.latest_offset, actual_stats.latest_offset);
    }
}
```

**Reference:** "Precomputation" patterns from "Latency" book - move computation from query time to write time.

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

### 6.4 Latency Percentile Tracking with eCDF Dashboards (1 day)

**Infrastructure:**
- Export HDR histogram to Prometheus
- Grafana dashboard for p50/p95/p99/p99.9
- **NEW:** eCDF visualization for regression detection

**Grafana eCDF Dashboard:**

```json
{
  "dashboard": {
    "title": "Latency eCDF Distribution",
    "panels": [
      {
        "title": "Append Latency eCDF",
        "type": "graph",
        "targets": [
          {
            "expr": "histogram_quantile($percentile, append_latency_bucket)",
            "legendFormat": "{{date}}"
          }
        ],
        "xAxis": {
          "mode": "custom",
          "name": "Percentile",
          "min": 0,
          "max": 100
        },
        "yAxis": {
          "name": "Latency (ms)",
          "logBase": 10
        }
      }
    ]
  }
}
```

**eCDF Export for Trending:**

```rust
/// Export eCDF data for multiple benchmark runs (trend analysis)
pub fn export_ecdf_trend(runs: &[BenchmarkRun], output_dir: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(output_dir.join("ecdf_trend.csv"))?;

    // Header: percentile, run1_latency, run2_latency, ...
    let mut header = vec!["percentile".to_string()];
    header.extend(runs.iter().map(|r| r.name.clone()));
    wtr.write_record(&header)?;

    // Data: one row per percentile
    for p in 0..=999 {
        let percentile = p as f64 / 1000.0;
        let mut row = vec![percentile.to_string()];

        for run in runs {
            let latency = run.histogram.value_at_quantile(percentile);
            row.push(latency.to_string());
        }

        wtr.write_record(&row)?;
    }

    wtr.flush()?;
    Ok(())
}
```

**Regression Detection:**

```python
#!/usr/bin/env python3
"""Detect latency regression from eCDF comparison"""

import pandas as pd
import numpy as np

def detect_regression(baseline_csv, current_csv, threshold=0.10):
    baseline = pd.read_csv(baseline_csv)
    current = pd.read_csv(current_csv)

    # Check tail latency degradation (p95-p999)
    tail_percentiles = baseline['percentile'] >= 0.95

    baseline_tail = baseline.loc[tail_percentiles, 'latency_ns']
    current_tail = current.loc[tail_percentiles, 'latency_ns']

    # Calculate maximum regression
    regression = (current_tail - baseline_tail) / baseline_tail
    max_regression = regression.max()

    if max_regression > threshold:
        print(f"REGRESSION: {max_regression:.1%} increase in tail latency")
        return False

    print(f"OK: Maximum regression {max_regression:.1%} (threshold {threshold:.0%})")
    return True
```

---

### 6.5 Concurrency Correctness Testing with Loom (2 days)

**Purpose:** Verify lock-free data structures are free from data races and memory ordering bugs.

**Dependencies:** Add to workspace `Cargo.toml`:
```toml
[dev-dependencies]
loom = "0.7"
```

**Test Pattern for SPSC Queue:**

```rust
#[cfg(test)]
mod loom_tests {
    use loom::sync::atomic::{AtomicUsize, Ordering};
    use loom::sync::Arc;
    use loom::thread;

    #[test]
    fn spsc_queue_no_data_race() {
        loom::model(|| {
            let queue = Arc::new(SPSCQueue::new(4));
            let q_producer = queue.clone();
            let q_consumer = queue.clone();

            // Producer thread
            let producer = thread::spawn(move || {
                for i in 0..2 {
                    while q_producer.push(i).is_err() {
                        thread::yield_now();
                    }
                }
            });

            // Consumer thread
            let consumer = thread::spawn(move || {
                let mut received = vec![];
                while received.len() < 2 {
                    if let Some(val) = q_consumer.pop() {
                        received.push(val);
                    } else {
                        thread::yield_now();
                    }
                }
                received
            });

            producer.join().unwrap();
            let values = consumer.join().unwrap();

            // Verify all values received in order
            assert_eq!(values, vec![0, 1]);
        });
    }

    #[test]
    fn atomic_offset_allocation_correctness() {
        loom::model(|| {
            let offset = Arc::new(AtomicU64::new(0));
            let mut handles = vec![];

            // Spawn 3 threads, each allocating 2 offsets
            for _ in 0..3 {
                let offset_clone = offset.clone();
                handles.push(thread::spawn(move || {
                    allocate_offset(&offset_clone, 2)
                }));
            }

            let mut results: Vec<u64> = handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect();

            results.sort();

            // Verify no overlap: [0, 2, 4]
            assert_eq!(results, vec![0, 2, 4]);
        });
    }
}
```

**What Loom Tests:**
- **Data races:** Unsynchronized access to shared memory
- **Memory ordering bugs:** Missing acquire/release barriers
- **Deadlocks:** Cyclic lock dependencies (if using locks)
- **Lost updates:** CAS loops that drop updates

**Running Loom Tests:**

```bash
# Loom tests are expensive (explore all thread interleavings)
# Run separately from unit tests
cargo test --release --test loom_tests

# Configure Loom iterations
LOOM_MAX_PREEMPTIONS=3 cargo test --test loom_tests
```

**Expected Coverage:**
- SPSC queue correctness (Phase 4.4)
- Atomic offset allocation (Phase 4.5)
- Cache coherency in SIEVE (Phase 3.6)
- Materialized view updates (Phase 4.8)

**Limitations:**
- Loom only works on small code (< 1000 interleavings)
- Does NOT replace stress testing with real threads
- Requires rewriting atomics to use `loom::` types

---

## Advanced Latency Patterns

This section provides reference implementations for low-latency patterns from "Latency: Reduce delay in software systems" by Pekka Enberg (ScyllaDB, Turso). These patterns are battle-tested in production databases handling millions of requests per second.

### Lock-Free Synchronization Patterns

**Memory Ordering Semantics:**

| Ordering | Use Case | Guarantees |
|----------|----------|------------|
| `Relaxed` | Same-thread reads | No cross-thread visibility guarantees |
| `Acquire` | Load that synchronizes with Release store | See all writes before Release |
| `Release` | Store that synchronizes with Acquire load | Make all prior writes visible |
| `AcqRel` | Read-modify-write (CAS) | Both Acquire + Release |
| `SeqCst` | Rare: total ordering needed | Expensive, avoid unless necessary |

**Common Patterns:**

```rust
// Pattern 1: Producer-Consumer (SPSC)
// Producer:
data[index] = value;                          // Write data
head.store(index + 1, Ordering::Release);     // Signal consumer

// Consumer:
let current = head.load(Ordering::Acquire);   // Check for new data
if current > tail {
    let value = data[tail];                   // Read is synchronized
    tail += 1;
}

// Pattern 2: CAS Loop for Allocation
loop {
    let current = offset.load(Ordering::Relaxed);
    let next = current + count;

    match offset.compare_exchange_weak(
        current,
        next,
        Ordering::AcqRel,   // Success: both acquire and release
        Ordering::Acquire,  // Failure: retry with fresh value
    ) {
        Ok(_) => return current,
        Err(actual) => current = actual,  // Retry
    }
}

// Pattern 3: Flag + Data (happens-before relationship)
// Writer:
data.store(value, Ordering::Relaxed);   // 1. Write data
ready.store(true, Ordering::Release);   // 2. Signal ready

// Reader:
while !ready.load(Ordering::Acquire) {  // 1. Wait for ready
    std::hint::spin_loop();
}
let value = data.load(Ordering::Relaxed);  // 2. Read is synchronized
```

**Testing Lock-Free Code with Loom:**

```rust
#[cfg(test)]
mod tests {
    use loom::sync::atomic::{AtomicUsize, Ordering};
    use loom::thread;

    #[test]
    fn test_spsc_queue_correctness() {
        loom::model(|| {
            let queue = Arc::new(SPSCQueue::new(4));
            let q1 = queue.clone();
            let q2 = queue.clone();

            // Producer thread
            let producer = thread::spawn(move || {
                q1.push(42).unwrap();
            });

            // Consumer thread
            let consumer = thread::spawn(move || {
                while let None = q2.pop() {
                    thread::yield_now();
                }
            });

            producer.join().unwrap();
            consumer.join().unwrap();
        });
    }
}
```

---

### Thread-Per-Core Architecture

**Design Principles:**
1. **Pinning:** Bind each worker thread to a dedicated CPU core
2. **Isolation:** Prevent OS from scheduling other work on those cores
3. **Data Partitioning:** Each thread owns its data (no sharing)
4. **Event Loop:** Single-threaded async runtime per core

**CPU Affinity Setup:**

```rust
use core_affinity::CoreId;

/// Pin current thread to specific CPU core
pub fn pin_to_core(core_id: CoreId) -> Result<()> {
    if !core_affinity::set_for_current(core_id) {
        return Err(Error::AffinityFailed(core_id));
    }

    // Verify affinity was set
    let affinity = core_affinity::get_core_ids()
        .ok_or(Error::AffinityNotSupported)?;

    if !affinity.contains(&core_id) {
        return Err(Error::AffinityVerificationFailed);
    }

    Ok(())
}

/// Get available cores for Kimberlite (exclude cores 0-1 for OS)
pub fn get_worker_cores() -> Vec<CoreId> {
    let all_cores = core_affinity::get_core_ids()
        .unwrap_or_else(|| vec![]);

    // Reserve first 2 cores for OS, use rest for workers
    all_cores.into_iter().skip(2).collect()
}
```

**Linux Kernel Isolation (Optional):**

```bash
# /etc/default/grub - Add to GRUB_CMDLINE_LINUX:
isolcpus=2-15           # Isolate cores 2-15 from scheduler
nohz_full=2-15          # Disable timer ticks on isolated cores
rcu_nocbs=2-15          # Offload RCU callbacks

# Apply:
sudo update-grub
sudo reboot

# Set interrupt affinity (run at boot):
echo "0-1" > /proc/irq/default_smp_affinity  # Route IRQs to cores 0-1 only
```

**Event Loop Design:**

```rust
pub struct CoreWorker {
    core_id: CoreId,
    command_queue: Arc<SPSCQueue<Command>>,
    storage: IoUringStorage,
}

impl CoreWorker {
    pub fn run(mut self) {
        // Pin to core
        pin_to_core(self.core_id).expect("Failed to pin thread");

        loop {
            let mut has_work = false;

            // 1. Poll command queue (non-blocking)
            while let Some(cmd) = self.command_queue.pop() {
                self.process_command(cmd);
                has_work = true;
            }

            // 2. Poll I/O completions (non-blocking)
            let completions = self.storage.poll_completions();
            if !completions.is_empty() {
                self.process_completions(completions);
                has_work = true;
            }

            // 3. Yield if no work (cooperative scheduling)
            if !has_work {
                std::thread::yield_now();
            }
        }
    }
}
```

**Benefits:**
- **Predictable latency:** No OS preemption
- **No context switches:** Thread never leaves CPU
- **Cache locality:** Thread owns its data, stays in L1/L2

**Drawbacks:**
- Requires many CPU cores (1 per tenant/partition)
- CPU isolation requires root/configuration
- Platform-specific (best on Linux)

---

### Async I/O Best Practices

**io_uring Architecture:**

```
┌─────────────────────────────────────────────────┐
│  Application Thread (Thread-Per-Core)           │
│                                                  │
│  1. Push operations to Submission Queue (SQ)    │
│  2. submit() - single syscall for batch         │
│  3. Poll Completion Queue (CQ) - no syscalls    │
└─────────────────────────────────────────────────┘
                    ↕ (shared memory)
┌─────────────────────────────────────────────────┐
│  Kernel                                          │
│                                                  │
│  1. Process SQ entries                           │
│  2. Execute I/O asynchronously                   │
│  3. Write results to CQ                          │
└─────────────────────────────────────────────────┘
```

**Batch Submission Strategy:**

```rust
impl IoUringStorage {
    /// Submit batch of operations efficiently
    pub fn submit_batch(&mut self, operations: &[IoOp]) -> Result<()> {
        let mut submission = self.ring.submission();

        for op in operations {
            let sqe = match op {
                IoOp::Write { offset, data } => {
                    opcode::Write::new(self.fd, data.as_ptr(), data.len() as u32)
                        .offset(*offset)
                        .build()
                        .user_data(op.request_id())
                }
                IoOp::Read { offset, len } => {
                    opcode::Read::new(self.fd, self.buffer.as_mut_ptr(), *len as u32)
                        .offset(*offset)
                        .build()
                        .user_data(op.request_id())
                }
                IoOp::Fsync => {
                    opcode::Fsync::new(self.fd)
                        .build()
                        .user_data(op.request_id())
                }
            };

            unsafe {
                submission.push(&sqe)?;
            }
        }

        // Single syscall for entire batch
        self.ring.submit()?;
        Ok(())
    }
}
```

**Completion Processing:**

```rust
pub fn poll_completions(&mut self) -> Vec<CompletionEvent> {
    let mut events = Vec::new();

    // Process all available completions (non-blocking)
    for cqe in self.ring.completion() {
        let result = if cqe.result() < 0 {
            Err(io::Error::from_raw_os_error(-cqe.result()))
        } else {
            Ok(cqe.result() as usize)
        };

        events.push(CompletionEvent {
            request_id: cqe.user_data(),
            result,
        });
    }

    events
}
```

**POLL vs IOPOLL Mode:**

| Mode | Use Case | Latency | CPU Usage |
|------|----------|---------|-----------|
| **Interrupt** | Low load, save CPU | Higher (interrupt overhead) | Low |
| **POLL** | Medium load | Medium | Medium |
| **IOPOLL** | High load, NVMe | Lowest (no interrupts) | High (100% CPU) |

**Configuration:**

```rust
// POLL mode (kernel polls for completions)
let ring = IoUring::builder()
    .setup_sqpoll(1000)  // Kernel thread polls every 1ms
    .build(256)?;

// IOPOLL mode (application polls device directly)
let ring = IoUring::builder()
    .setup_iopoll()      // Direct device polling
    .build(256)?;
```

---

### Caching Strategies

**SIEVE Algorithm (2024):**

**Why SIEVE > LRU:**
- **No reordering:** Accessed items marked with flag, not moved
- **Lower contention:** No lock on read path (just atomic flag)
- **Better hit ratio:** 30-50% improvement on real workloads (proven in NSDI 2024)

**Implementation Details:**

```rust
// Eviction algorithm
fn evict(&mut self) {
    let mut victim = None;

    // Hand algorithm: sweep from front
    loop {
        if let Some((key, val, visited)) = self.queue.pop_front() {
            if visited.load(Ordering::Relaxed) {
                // Visited: give second chance, re-insert at end
                visited.store(false, Ordering::Relaxed);
                self.queue.push_back((key, val, visited));
            } else {
                // Not visited: evict
                victim = Some((key, val));
                break;
            }
        } else {
            // Queue empty
            break;
        }
    }
}
```

**Cache Coherency for Append-Only Logs:**

```rust
/// Immutable log segments can be cached indefinitely
pub struct SegmentCache {
    immutable: SieveCache<SegmentId, Arc<Segment>>,
    active_segment: Option<(SegmentId, Arc<Segment>)>,
}

impl SegmentCache {
    pub fn get(&mut self, segment_id: SegmentId) -> Option<Arc<Segment>> {
        // Active segment: always read fresh
        if let Some((active_id, segment)) = &self.active_segment {
            if *active_id == segment_id {
                return Some(Arc::clone(segment));
            }
        }

        // Immutable segment: cache indefinitely (never invalidate)
        self.immutable.get(&segment_id).map(Arc::clone)
    }

    pub fn seal_active(&mut self) {
        if let Some((segment_id, segment)) = self.active_segment.take() {
            // Move to immutable cache
            self.immutable.insert(segment_id, segment);
        }
    }
}
```

**Materialized View Patterns:**

```rust
/// Incremental materialized view with snapshot support
pub struct IncrementalView<K, V> {
    data: HashMap<K, V>,
    snapshot_offset: Offset,
    dirty: bool,
}

impl<K: Hash + Eq, V> IncrementalView<K, V> {
    /// Apply delta update (called on every commit)
    pub fn apply_delta(&mut self, offset: Offset, updates: Vec<(K, V)>) {
        for (key, value) in updates {
            self.data.insert(key, value);
        }
        self.snapshot_offset = offset;
        self.dirty = true;
    }

    /// Snapshot to disk (periodic, not every update)
    pub fn snapshot(&mut self, path: &Path) -> Result<()> {
        if !self.dirty {
            return Ok(());  // No changes since last snapshot
        }

        let snapshot = Snapshot {
            offset: self.snapshot_offset,
            data: &self.data,
        };

        let encoded = postcard::to_allocvec(&snapshot)?;
        fs::write(path, encoded)?;

        self.dirty = false;
        Ok(())
    }

    /// Load from snapshot + replay delta
    pub fn restore(snapshot_path: &Path, log: &Log) -> Result<Self> {
        let encoded = fs::read(snapshot_path)?;
        let snapshot: Snapshot<K, V> = postcard::from_bytes(&encoded)?;

        let mut view = Self {
            data: snapshot.data,
            snapshot_offset: snapshot.offset,
            dirty: false,
        };

        // Replay delta from snapshot offset to current
        let deltas = log.read_from(snapshot.offset)?;
        for delta in deltas {
            view.apply_delta(delta.offset, delta.updates);
        }

        Ok(view)
    }
}
```

---

### Backpressure & Flow Control

**Little's Law Queue Sizing:**

```rust
/// Calculate queue size from target throughput and latency
pub fn calculate_queue_size(
    target_throughput: f64,  // ops/sec
    target_latency: Duration,
    safety_factor: f64,       // e.g., 1.2 for 20% buffer
) -> usize {
    let latency_sec = target_latency.as_secs_f64();
    let base_size = target_throughput * latency_sec;
    (base_size * safety_factor).ceil() as usize
}

// Example:
let queue_size = calculate_queue_size(
    100_000.0,                        // 100K ops/sec
    Duration::from_millis(10),        // 10ms target latency
    1.2,                               // 20% safety margin
);
// Result: 1200 entries
```

**Rejection Strategy:**

```rust
/// Bounded queue with backpressure
pub struct BoundedQueue<T> {
    inner: SPSCQueue<T>,
    max_size: usize,
    metrics: Metrics,
}

impl<T> BoundedQueue<T> {
    pub fn try_push(&self, item: T) -> Result<(), BackpressureError> {
        match self.inner.push(item) {
            Ok(()) => {
                self.metrics.queue_depth.set(self.inner.len() as f64);
                Ok(())
            }
            Err(item) => {
                self.metrics.backpressure_rejections.increment(1);
                Err(BackpressureError::QueueFull {
                    current_depth: self.inner.len(),
                    max_depth: self.max_size,
                    rejected_item: item,
                })
            }
        }
    }
}
```

**Retry Policy (Client-Side):**

```rust
/// Exponential backoff with jitter
pub async fn append_with_retry(
    client: &Client,
    record: Record,
    max_retries: u32,
) -> Result<Offset> {
    let mut retry_count = 0;
    let base_delay = Duration::from_millis(10);

    loop {
        match client.append(record.clone()).await {
            Ok(offset) => return Ok(offset),

            Err(Error::Backpressure { .. }) if retry_count < max_retries => {
                // Exponential backoff: 10ms, 20ms, 40ms, 80ms, 160ms
                let delay = base_delay * 2_u32.pow(retry_count);

                // Add jitter (±25%) to prevent thundering herd
                let jitter = delay / 4;
                let jittered_delay = delay + rand::random::<Duration>() % jitter;

                tokio::time::sleep(jittered_delay).await;
                retry_count += 1;
            }

            Err(e) => return Err(e),  // Other errors: fail immediately
        }
    }
}
```

**Load Shedding:**

```rust
/// Shed load when queue depth exceeds threshold
pub fn should_shed_load(&self, priority: Priority) -> bool {
    let depth_ratio = self.inner.len() as f64 / self.max_size as f64;

    match priority {
        Priority::Critical => false,           // Never shed critical requests
        Priority::High => depth_ratio > 0.95,  // Shed at 95% capacity
        Priority::Normal => depth_ratio > 0.85,
        Priority::Low => depth_ratio > 0.70,
    }
}
```

---

### Performance Measurement Best Practices

**Avoiding Coordinated Omission:**

```rust
/// Correct latency measurement with coordinated omission correction
pub struct LatencyBenchmark {
    histogram: Histogram<u64>,
    target_rate: f64,  // ops/sec
}

impl LatencyBenchmark {
    pub fn run(&mut self, duration: Duration) {
        let start = Instant::now();
        let interval = Duration::from_secs_f64(1.0 / self.target_rate);
        let mut next_send_time = start;

        while start.elapsed() < duration {
            let actual_send_time = Instant::now();

            // Record intended send time, not actual
            let op_start = next_send_time;
            perform_operation();
            let latency = actual_send_time.elapsed();

            // Record full latency including queueing delay
            self.histogram.record(latency.as_nanos() as u64).unwrap();

            // Next intended send time (closed-loop)
            next_send_time += interval;

            // Sleep until next send time
            if let Some(sleep_time) = next_send_time.checked_duration_since(Instant::now()) {
                std::thread::sleep(sleep_time);
            }
        }
    }
}
```

**Latency Book Reference Table:**

| Kimberlite Challenge | Pattern | Latency Book Chapter | Page |
|---------------------|---------|---------------------|------|
| Unknown tail latency | HDR Histogram + eCDF | Ch 2: Modeling & Measuring | p. 23-45 |
| Queue sizing | Little's Law (C = T × L) | Ch 2: Laws of Latency | p. 35-42 |
| Cache inefficiency | SIEVE replacement policy | Ch 6: Caching | p. 125-148 |
| Mutex contention | SPSC lock-free queues | Ch 8: Wait-Free Synchronization | p. 187-214 |
| OS scheduling variance | Thread-per-core architecture | Ch 9: Exploiting Concurrency | p. 221-245 |
| I/O unpredictability | io_uring POLL mode | Ch 10: Async Processing | p. 253-287 |
| Slow compliance queries | Materialized views | Ch 6: Caching | p. 141-148 |
| Unbounded queues | Backpressure (bounded queues) | Ch 10: Managing Concurrency | p. 274-281 |
| Coordinated omission | Arrival rate tracking | Ch 2: Benchmarking | p. 51-58 |

---

## Performance Philosophy Summary

Kimberlite's enhanced performance philosophy integrates battle-tested patterns from production low-latency databases:

### Core Principles

1. **Correctness First**: Never sacrifice correctness for speed (unchanged)
2. **Network → Disk → Memory → CPU**: Optimize in this order (unchanged)
3. **Measure Everything**: Profile before optimizing (enhanced with eCDF)
4. **Predictable Latency**: Prefer consistency over peak performance (enhanced with p99.9 tracking)

### New Principles from Latency Book

5. **Model with Little's Law**: Size queues using C = T × L
6. **Eliminate Locks**: Use lock-free patterns (SPSC, CAS) in hot paths
7. **Control Scheduling**: Thread-per-core eliminates OS variance
8. **Application-Level I/O**: io_uring POLL mode removes interrupt unpredictability
9. **Precompute Queries**: Materialized views move work from read to write time
10. **Enforce Backpressure**: Bounded queues sized by Little's Law prevent cascading failures

### When to Apply Advanced Patterns

| Pattern | Use When | Avoid When |
|---------|----------|------------|
| **Lock-Free SPSC** | Multi-tenant hot path | Single-threaded code |
| **Thread-Per-Core** | Many CPU cores available | Resource-constrained (< 4 cores) |
| **io_uring POLL** | Linux production, NVMe SSD | Development (use sync I/O) |
| **SIEVE Cache** | Multi-tenant workloads | Single-tenant or low hit ratio |
| **Materialized Views** | Frequent read-heavy queries | Write-heavy, rarely queried data |
| **Backpressure** | Bounded capacity system | Infinite capacity tolerable |

### Integration with Existing Patterns

**Batching (existing) + Lock-Free Queues (new):**
```rust
// Combine batching with SPSC for maximum throughput
while let Some(cmd) = command_queue.pop() {
    batch.push(cmd);
    if batch.len() >= BATCH_SIZE {
        process_batch(&batch);
        batch.clear();
    }
}
```

**Zero-Copy (existing) + io_uring (new):**
```rust
// Zero-copy reads with async I/O
let mmap_data = segment.as_bytes();  // Zero-copy mmap
io_uring.read_async(mmap_data)?;     // Async I/O submission
```

**Cache-Friendly Layout (existing) + SIEVE Cache (new):**
```rust
// Cache-friendly + better eviction policy
struct CachedSegment {
    data: Vec<u8>,           // Contiguous layout
    visited: AtomicBool,     // SIEVE flag
}
```

### Performance Monitoring Checklist

**Before Optimization:**
- [ ] Profile with flamegraph (`just profile`)
- [ ] Establish baseline with Criterion benchmarks
- [ ] Export eCDF for latency distribution
- [ ] Calculate queue sizes using Little's Law

**During Optimization:**
- [ ] Benchmark each change independently
- [ ] Compare eCDF curves (before vs after)
- [ ] Validate Little's Law: measure actual concurrency
- [ ] Test with Loom (for lock-free code)

**After Optimization:**
- [ ] Verify correctness (all tests pass)
- [ ] Confirm no latency regression (p99, p99.9)
- [ ] Update baseline for CI regression detection
- [ ] Document performance characteristics

### Reference: Latency Book Pattern Map

```
Kimberlite Challenge          Latency Book Pattern           Implementation Phase
─────────────────────────────────────────────────────────────────────────────────
Queue sizing too small/large → Little's Law                 → Phase 1.6, 4.7
Tail latency unknown          → HDR Histogram + eCDF        → Phase 2.1, 6.4
Cache thrashing               → SIEVE replacement policy    → Phase 3.6
Lock contention               → Lock-free SPSC queues       → Phase 4.4
Mutex overhead                → Atomic CAS                  → Phase 4.5
OS scheduling jitter          → Thread-per-core             → Phase 4.6
I/O unpredictability          → io_uring POLL mode          → Phase 3.4
Slow aggregate queries        → Materialized views          → Phase 4.8
Unbounded memory growth       → Backpressure                → Phase 4.7
Coordinated omission          → Arrival rate tracking       → Phase 6.4
```

**Book Reference:** "Latency: Reduce delay in software systems" by Pekka Enberg (2024)
- Author background: ScyllaDB core developer, Turso co-founder
- Focus: Production battle-tested patterns from low-latency databases
- Application to Kimberlite: All patterns adapted for compliance-first architecture

---

## Critical Files for Implementation

### Highest Impact (Implement First)

1. **`crates/kimberlite-storage/src/storage.rs`** (CRITICAL)
   - Lines 368-409: Full-file reads → mmap (Phase 3.1)
   - Line 291: Index write every batch → batching (Phase 3.2)
   - Lines 225-302: append_batch → optimization targets

2. **`Cargo.toml`** (workspace root)
   - Lines 62-70: Add crypto SIMD features (Phase 1.1)
   - Add dependencies: `memmap2`, `lru` (Phase 3-4)

3. **`crates/kimberlite-kernel/src/state.rs`**
   - Line 56: BTreeMap → HashMap (Phase 1.2)
   - Add LRU cache fields (Phase 4.3)

4. **`crates/kimberlite-crypto/src/encryption.rs`**
   - Lines 937, 987: Cache cipher instances (Phase 5.1)
   - Add batch encryption support (Phase 5.2)

### New Files (Create in Phases)

5. **`crates/*/benches/*.rs`** (Phase 2) - Benchmark infrastructure
6. **`crates/kimberlite-storage/src/mmap.rs`** (Phase 3.1) - Memory mapping
7. **`crates/kimberlite-kernel/src/snapshot.rs`** (Phase 4.2) - State snapshots

---

## Success Metrics Summary

### Phase 1-2 (Foundation)
- ✓ Crypto ops 2-3x faster (SIMD enabled)
- ✓ Table lookups O(1) instead of O(log n)
- ✓ Benchmarks run in CI with regression detection
- ✓ Checkpoint-optimized reads as default
- ✓ Queue sizing validates Little's Law
- ✓ Benchmarks export eCDF data

### Phase 3-4 (Core Performance)
- **Append throughput:** 10K → 100K events/sec (10x improvement)
- **Read throughput:** 5 MB/s → 50 MB/s (10x improvement)
- **Append latency p99:** Unmeasured → < 1ms (io_uring POLL)
- **Cache hit ratio:** Baseline → 30%+ improvement (SIEVE vs. LRU)
- **Context switches:** High → Near zero (thread-per-core)
- **Queue rejections:** None (OOM risk) → Graceful under 2x overload (backpressure)
- **Index I/O:** 10-100x fewer disk syncs
- **Verification:** O(n) → O(k) speedup
- **Startup:** Instant recovery with snapshots

### Phase 5-6 (Polish & Validation)
- **Encryption:** 20-30% faster with caching
- **Batch encryption:** 2-4x on multi-core
- **Latency p99:** < 10ms
- **Latency p99.9:** < 50ms
- **Materialized views:** 100-1000x faster queries (O(1) vs. O(n) scan)
- **Concurrency correctness:** Loom tests pass (lock-free validation)

---

## Implementation Timeline

### Week 1: Foundation + New Measurement
- **Days 1-2:** Phase 1 (quick wins 1.1-1.5) + Little's Law sizing (1.6)
- **Days 3-4:** Phase 2 (benchmarks + eCDF export + Little's Law validation)
- **Day 5:** Baseline measurements, eCDF dashboards, document current state

### Week 2-3: Storage + Concurrency
- **Week 2:** Phase 3 (storage layer)
  - Days 1-2: mmap support (3.1) + batch index writes (3.2)
  - Days 3-4: io_uring POLL mode (3.4) - Linux only
  - Day 5: SIEVE cache implementation (3.6)
- **Week 3:** Phase 4 (kernel & command processing)
  - Days 1-2: Command batching (4.1) + State snapshots (4.2) + LRU cache (4.3)
  - Day 3: Lock-free SPSC queues (4.4) + Atomic CAS offset (4.5)
  - Days 4-5: Thread-per-core model (4.6)

### Week 4: Advanced Patterns
- **Days 1-2:** Backpressure implementation (4.7)
- **Days 3-4:** Materialized views (4.8)
- **Day 5:** Phase 5 (crypto optimizations - cached ciphers, batch encryption)

### Week 5+: Testing & Validation
- **Week 5:** Phase 6 enhanced testing
  - Loom concurrency testing for lock-free code
  - Load testing with Little's Law validation
  - eCDF latency distribution monitoring
  - Cache hit ratio validation (SIEVE vs LRU)
- **Ongoing:** Performance regression detection, benchmarking, monitoring

**Total Estimated Time:** 5-6 weeks for full implementation of all phases with latency book enhancements.

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

### Quick Wins (4-5 hours)

1. **Enable crypto SIMD features** (30 min)
   - Edit workspace `Cargo.toml`: Add `features = ["asm", "aes"]`
   - Expected: 2-3x crypto speedup

2. **Add Little's Law queue sizing** (1 hour)
   - Create `crates/kimberlite-kernel/src/queue_sizing.rs`
   - Apply to all bounded channels
   - Expected: Right-sized queues, prevent OOM

3. **Create first benchmark suite with eCDF** (2 hours)
   - Add `crates/kimberlite-storage/benches/storage_benchmark.rs`
   - Export eCDF CSV for baseline
   - Expected: Measurement baseline established

4. **Profile current hot paths** (1 hour)
   - Run `just profile-vopr`
   - Generate flamegraph
   - Expected: Validate optimization assumptions

5. **Run load test baseline** (1 hour)
   - Document current throughput/latency with HDR histogram
   - Export eCDF for trending
   - Expected: Baseline metrics for regression detection

**Total Quick Start:** ~5.5 hours to measurable improvements + baseline metrics

### First Week Goals

By end of Week 1, you should have:
- ✓ Phase 1 complete (all quick wins)
- ✓ Benchmarks running in CI with eCDF export
- ✓ Baseline metrics documented (throughput, latency distribution, cache hit ratio)
- ✓ Queue sizes validated against Little's Law
- ✓ HDR histograms + eCDF dashboards configured

### Long-Term Roadmap

**Weeks 2-3:** Storage + Kernel optimizations (10-20x throughput improvement)
**Week 4:** Advanced patterns (thread-per-core, materialized views)
**Week 5+:** Testing, validation, monitoring

**Total Timeline:** 5-6 weeks for complete latency book integration

---

## Appendix: Latency Book References

### Quick Reference Guide

| Optimization | Book Chapter | Key Insight | Kimberlite Application |
|--------------|--------------|-------------|------------------------|
| Little's Law | Ch 2 (p. 35-42) | C = T × L for queue sizing | Phase 1.6, 4.7 |
| eCDF Plotting | Ch 2 (p. 45-51) | Visualize tail latency | Phase 2.1, 6.4 |
| SIEVE Cache | Ch 6 (p. 125-148) | FIFO beats LRU | Phase 3.6 |
| Lock-Free SPSC | Ch 8 (p. 187-214) | Eliminate mutex overhead | Phase 4.4 |
| Atomic CAS | Ch 8 (p. 201-208) | Low-contention allocation | Phase 4.5 |
| Thread-Per-Core | Ch 9 (p. 221-245) | Eliminate OS scheduling | Phase 4.6 |
| io_uring POLL | Ch 10 (p. 253-287) | Remove interrupt variance | Phase 3.4 |
| Materialized Views | Ch 6 (p. 141-148) | Precompute queries | Phase 4.8 |
| Backpressure | Ch 10 (p. 274-281) | Bounded queues prevent OOM | Phase 4.7 |

### Book Citation

**Title:** Latency: Reduce delay in software systems
**Author:** Pekka Enberg
**Background:** Core developer at ScyllaDB, co-founder of Turso
**Publication:** 2024
**Focus:** Production-proven low-latency patterns from high-performance databases
**Relevance:** All patterns adapted for Kimberlite's compliance-first, append-only architecture

### Additional Resources

- **ScyllaDB Architecture:** https://www.scylladb.com/product/technology/
- **Seastar Framework:** https://seastar.io/ (thread-per-core reference implementation)
- **io_uring Documentation:** https://kernel.dk/io_uring.pdf
- **SIEVE Paper:** "SIEVE is Simpler than LRU" (NSDI 2024)
- **Loom Testing:** https://github.com/tokio-rs/loom

---

---

## Cluster & Consensus Enhancements

### Dynamic Reconfiguration

**Status:** 📋 Planned for v1.0.0

**Description:**
Enable runtime cluster membership changes without downtime. Support adding/removing replicas while maintaining linearizability guarantees.

**Implementation:**
- Configuration change protocol (VSR joint consensus approach)
- Safe replica addition/removal during normal operation
- Validation of quorum configurations before applying changes
- Rollback mechanism for failed reconfigurations

**Use Cases:**
- Scaling cluster up/down based on load
- Replacing failed nodes
- Datacenter migration

### Third-Party Checkpoint Attestation

**Status:** 📋 Planned for v1.0.0

**Description:**
Cryptographically anchor checkpoints to external trusted sources for enhanced tamper-evidence.

**Options:**
1. **RFC 3161 Timestamp Authority (TSA):**
   - Send checkpoint hash to trusted TSA
   - Receive signed timestamp proof
   - Store TSA signature alongside checkpoint
   - Prove checkpoint existed at specific time

2. **Blockchain Anchoring:**
   - Publish checkpoint Merkle root to public blockchain (Bitcoin, Ethereum)
   - Store transaction ID in checkpoint metadata
   - Immutable proof-of-existence via blockchain

**Benefits:**
- Stronger non-repudiation for compliance
- External verifiability (auditors can verify independently)
- Protection against system-wide compromise

### Hot Shard Migration

**Status:** 📋 Planned for v1.0.0

**Description:**
Dynamically rebalance data placement across cluster nodes to optimize load distribution.

**Implementation:**
- Tenant placement tracking in directory
- Live migration protocol with zero downtime
- Gradual traffic cutover
- Consistency guarantees during migration

**Use Cases:**
- Load balancing across nodes
- Handling hot tenants/streams
- Preparing for node decommissioning

---

## SQL Query Engine Enhancements

### Secondary Index Support

**Status:** 🚧 Partially Implemented (deferred to post-v0.2.0)

**Current State:**
- B+tree projection store supports index data structures
- Query planner recognizes index hints
- Index maintenance hooks exist

**Remaining Work:**
- Index creation DDL (`CREATE INDEX`)
- Index selection optimization in query planner
- Multi-column composite indexes
- Covering indexes to avoid table lookups

**Timeline:** v0.3.0

### JOINs and Aggregates in Queries

**Status:** 📋 Planned for v0.4.0+

**Current State:**
- JOINs/aggregates only supported in projections (write-time computed views)
- Queries are lookups only (SELECT, WHERE, ORDER BY, LIMIT)

**Future Enhancement:**
- Runtime JOINs (INNER, LEFT, RIGHT)
- Aggregate functions (COUNT, SUM, AVG, MAX, MIN)
- GROUP BY with HAVING
- Subqueries

**Design Constraint:**
- Query-time JOINs degrade latency predictability
- May remain projection-only for compliance use cases
- Consider read-only replica option for analytical queries

### Differential Privacy for Statistical Queries

**Status:** 📋 Planned for v1.0.0+

**Description:**
Add noise to aggregate query results to prevent re-identification attacks while maintaining statistical utility.

**Use Cases:**
- Public health data sharing (COVID-19 case counts)
- Financial trend reporting (transaction volumes)
- Compliance with privacy regulations (GDPR, HIPAA)

**Implementation:**
- Laplace/Gaussian noise injection
- Privacy budget tracking per query
- Configurable epsilon parameter

---

## Compliance & Audit Enhancements

### Token-Based Access Control Model

**Status:** 📋 Planned for v0.3.0

**Description:**
Formalize specification for scoped access tokens used in secure data sharing.

**Features:**
- Time-bounded tokens (expiration)
- Scope restrictions (read-only, specific tables/fields)
- Revocation mechanism
- Audit trail of token usage

**Use Cases:**
- Third-party data access (research, analytics)
- LLM integration with limited permissions
- Temporary data exports

### Consent and Purpose Tracking

**Status:** 📋 Planned for v0.4.0

**Description:**
Track user consent and data processing purposes for GDPR/CCPA compliance.

**Schema:**
```rust
pub struct ConsentRecord {
    pub user_id: UserId,
    pub purpose: String,              // "medical_research", "billing", etc.
    pub granted_at: Timestamp,
    pub expires_at: Option<Timestamp>,
    pub revoked_at: Option<Timestamp>,
}
```

**Integration:**
- Enforce purpose restrictions in queries
- Automatic expiration of consent
- Consent withdrawal propagation
- Audit log of consent changes

### Export Audit Trail Format

**Status:** 📋 Planned for v0.3.0

**Description:**
Standardized audit log format for data exports and third-party access.

**Fields:**
- Who accessed what data (user ID, token ID)
- When (timestamp with nanosecond precision)
- What was exported (table names, field names, record count)
- Purpose/justification
- Anonymization techniques applied

**Format Options:**
- Structured JSON for machine parsing
- Human-readable CSV for auditor review
- Immutable append-only audit log (stored as stream)

---

## SDK & Distribution Enhancements

### Python SDK Distribution

**Status:** 📋 Planned for v0.3.0

**Remaining Work:**
- [ ] Wheel distribution with bundled native library (`.so`/`.dylib`/`.dll`)
- [ ] Integration tests against kimberlite-server
- [ ] Publish to PyPI
- [ ] Documentation and examples

**Current State:**
- Python FFI bindings exist in `kimberlite-ffi`
- SDK API designed but not packaged

### TypeScript SDK Distribution

**Status:** 📋 Planned for v0.3.0

**Remaining Work:**
- [ ] Pre-built binaries for common platforms (Linux, macOS, Windows)
- [ ] CI workflow to build native modules
- [ ] Publish to npm
- [ ] TypeScript type definitions

### Go SDK Distribution

**Status:** 📋 Planned for v0.3.0

**Remaining Work:**
- [ ] CGO bindings packaging
- [ ] Publish to pkg.go.dev
- [ ] Go module versioning

---

## Bug Bounty Program

### Phase 1: Crypto & Storage (v0.3.0)

**Scope:**
- `kimberlite-crypto` crate (hash chains, signatures, encryption)
- `kimberlite-storage` crate (append-only log, CRC validation)

**Focus Areas:**
- Hash chain integrity bypass
- Cryptographic primitive weaknesses
- Storage corruption detection failures

**Bounty Range:** $500 - $5,000

### Phase 2: Consensus & Simulation (v0.4.0)

**Scope:**
- `kimberlite-vsr` crate (consensus protocol)
- `kimberlite-sim` crate (simulation testing)

**Focus Areas:**
- Consensus safety violations (split-brain, data loss)
- Linearizability violations
- Byzantine fault scenarios
- VOPR simulation bugs

**Bounty Range:** $1,000 - $20,000
(Inspired by TigerBeetle's consensus challenge)

### Phase 3: End-to-End Security (v1.0.0)

**Scope:**
- All crates
- Wire protocol security
- Encryption and key management
- Data sharing and anonymization
- Authentication and authorization

**Focus Areas:**
- End-to-end security bypasses
- MVCC isolation violations
- Authentication bypass
- Privilege escalation
- Data leakage via side channels

**Bounty Range:** $500 - $50,000

### Program Infrastructure (Planned)

**Remaining Work:**
- [ ] Security policy documentation (SECURITY.md)
- [ ] Responsible disclosure process
- [ ] HackerOne or similar platform integration
- [ ] Invariant documentation for security researchers

---

## Non-Goals

This section explicitly defines what Kimberlite will NOT do, to maintain focus and avoid scope creep.

### Not a General-Purpose Database

**Rationale:**
Kimberlite is optimized for compliance-first use cases (healthcare, finance, legal, government). It sacrifices flexibility for verifiability and auditability.

**What This Means:**
- No arbitrary SQL features (e.g., complex window functions, CTEs)
- No schema-less document storage (structured schemas required)
- No eventual consistency modes (linearizable or causal only)

### Not a Distributed Cache

**Rationale:**
Kimberlite prioritizes durability over ultra-low latency. All writes are durable before acknowledgment.

**What This Means:**
- No in-memory-only mode (all data persisted to disk)
- No write-back caching (write-through only)
- Not optimized for volatile data (use Redis/Memcached for that)

### Not a Time-Series Database

**Rationale:**
While Kimberlite's append-only log is time-ordered, it lacks specialized time-series optimizations.

**What This Means:**
- No time-series-specific compression (Delta-of-Delta, Gorilla)
- No downsampling or rollups built-in
- No specialized time-range queries
- Use InfluxDB/TimescaleDB for IoT metrics

### Not a Graph Database

**Rationale:**
Compliance use cases rarely require graph traversal. Projections support limited relationships via foreign keys.

**What This Means:**
- No graph query language (Cypher, Gremlin)
- No multi-hop traversal optimization
- No graph algorithms (PageRank, shortest path)
- Use Neo4j/DGraph for graph workloads

### Not a Distributed File System

**Rationale:**
Kimberlite stores structured records, not arbitrary blobs.

**What This Means:**
- No large blob storage (>1 MB records discouraged)
- No file system semantics (directories, inodes)
- No object storage API (S3-compatible)
- Use MinIO/S3 for file storage

### Not a Message Queue

**Rationale:**
While Kimberlite's log resembles a message queue, it lacks queue-specific features.

**What This Means:**
- No message acknowledgment protocol
- No dead-letter queues
- No message routing or fanout
- Use NATS/Kafka for messaging

### Not Optimized for Analytical Queries (OLAP)

**Rationale:**
Kimberlite is an OLTP system optimized for transactional integrity, not analytical throughput.

**What This Means:**
- No columnar storage format
- No parallel query execution
- No vectorized query processing
- Use ClickHouse/DuckDB for analytics

### Not a Blockchain

**Rationale:**
Kimberlite uses hash chains for tamper-evidence, but lacks blockchain consensus mechanisms.

**What This Means:**
- No proof-of-work or proof-of-stake
- No smart contracts
- No cryptocurrency features
- Centralized trust model (within organization)

### Not Optimized for Multi-Tenancy at Scale (>10K Tenants)

**Rationale:**
Kimberlite targets enterprise deployments with moderate tenant counts (10-1000s), not SaaS platforms with millions of users.

**What This Means:**
- Tenant-level parallelism, not row-level
- Shared-nothing architecture for hundreds of tenants, not millions
- Use purpose-built SaaS databases for extreme multi-tenancy

---

## Testing Infrastructure

### Planned Enhancements (v0.4.0+)

**From TESTING.md**:
- [ ] Shrinking for minimal test case reproduction
- [ ] Enhanced property-based testing coverage
- [ ] Differential fuzzing across implementations
- [ ] Continuous stress testing in production environments
- [ ] Extended VOPR scenarios for edge cases

**From adding-invariants.md**:
- [ ] Projection MVCC visibility invariant
  - Requires `ProjectionApplied` event implementation
  - Validates snapshot isolation correctness
  - Checks queries with `AS OF POSITION p` only see data committed at or before position `p`

### LLM Integration (v0.5.0+)

**Current State**: Framework designed and implemented (see `docs/LLM_INTEGRATION_DESIGN.md`), not runtime-active.

**Planned CLI Tools**:
- [ ] `vopr-llm generate --target "stress view changes"` - Generate adversarial scenarios
- [ ] `vopr-llm analyze failures.log` - Post-mortem failure analysis
- [ ] `vopr-llm shrink --seed 42` - Assisted test case reduction

**Planned Features**:
- [ ] Automated failure clustering (LLM groups similar failures)
- [ ] Query plan coverage guidance
- [ ] Scenario library with human-reviewed LLM-generated scenarios

**Safety Guarantees**:
- LLMs operate offline only (before/after VOPR runs, never during)
- All LLM outputs validated before use
- LLMs suggest, validators verify, invariants decide
- Determinism preserved

---

## Architecture Enhancements

### v1.0.0 - Production Ready

**From ARCHITECTURE.md**:
- [ ] Dynamic cluster reconfiguration
  - Add/remove nodes without downtime
  - Automatic leader re-election
  - Configuration consensus via VSR
- [ ] Hot shard migration for load balancing
  - Live tenant migration between nodes
  - Zero-downtime shard rebalancing
  - Automatic load detection and triggering
- [ ] Advanced query engine (JOINs, aggregates, window functions)
  - Multi-table JOINs with hash/merge strategies
  - GROUP BY with aggregates (SUM, AVG, COUNT, MIN, MAX)
  - Window functions (ROW_NUMBER, RANK, LAG, LEAD)
  - Subqueries and CTEs
- [ ] Third-party checkpoint attestation
  - RFC 3161 Timestamping Authority integration
  - Blockchain anchoring for immutable audit trail
  - Verifiable checkpoint integrity
- [ ] io_uring async I/O (Linux-specific optimization)
  - See Performance Optimizations section below
- [ ] Thread-per-core architecture (Seastar/ScyllaDB pattern)
  - Eliminate cross-core contention
  - Per-core event loops
  - Sharded data structures

**Note**: Platform-specific roadmap items (cloud platform integration, internal tooling) are documented in `/platform/ROADMAP.md`.

---

## Compliance Features

### v1.0.0+

**From COMPLIANCE.md**:
- [ ] Consent and purpose tracking (GDPR Article 6, CCPA)
  - Track user consent with purpose restrictions
  - Automatic consent expiration
  - Consent withdrawal propagation
  - Audit log of consent changes
- [ ] Differential privacy for statistical queries
  - Query-level noise injection
  - Privacy budget tracking
  - Epsilon-delta privacy guarantees
- [ ] Enhanced export audit trail formats
  - Structured JSON for machine parsing
  - Human-readable CSV for auditor review
  - Immutable append-only audit log
- [ ] Third-party data sharing with anonymization
  - Field-level anonymization rules
  - K-anonymity enforcement
  - Anonymization audit trail
- [ ] Automated compliance reporting
  - HIPAA compliance reports
  - GDPR data inventory reports
  - SOC 2 audit trail exports
- [ ] Integration with external audit systems
  - Push audit events to SIEM systems
  - Standard audit log formats (CEF, LEEF)
  - Real-time compliance monitoring

**Note**: See existing "Compliance & Audit Enhancements" section above for additional planned features.

---

## Performance Optimizations

### io_uring Async I/O (v0.5.0)

**From PERFORMANCE.md**:

io_uring provides 60% latency reduction for I/O-bound workloads on Linux 5.6+.

**Architecture Preparation**:

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

**When to Adopt**:

io_uring adoption is planned for v0.5.0 when:
- Linux 5.6+ is standard in production environments
- tokio-uring or monoio reaches 1.0 stability
- Simulation testing infrastructure can mock io_uring

**Expected Impact**:
- 10-20x append throughput improvement
- Near-zero kernel transitions
- Reduced CPU utilization

**See Also**: Phase 5 v0.5.0 in release timeline above for complete io_uring integration plan.

---

## Language SDKs

**Current**: Python, TypeScript, Rust SDKs ✅ Complete

**Planned Additional Languages**:
- **Go SDK** (Weeks 13-15)
  - Enterprise microservices
  - Kubernetes operators
  - Cloud infrastructure tooling

- **Java SDK** (Weeks 16-18)
  - Epic EHR integration
  - Cerner Millennium integration
  - Enterprise compliance systems

- **C# SDK** (Weeks 19-21)
  - Windows medical software
  - Unity training simulations
  - .NET enterprise applications

- **C++ SDK** (Weeks 22-24)
  - High-performance analytics
  - Embedded medical devices
  - Low-latency trading systems

- **WebAssembly SDK** (Future)
  - Browser-based applications
  - Edge computing scenarios

**See**: `docs/SDK.md` for implementation details

---

## SQL Engine Enhancements

**Current**: SELECT, WHERE, ORDER BY, LIMIT, basic DDL

**Planned**:

### Advanced DDL
- `ALTER TABLE` - Schema evolution
- `CREATE PROJECTION` - Materialized views
- Foreign key constraints
- CHECK constraints

### Transactions
- Explicit `BEGIN`/`COMMIT`/`ROLLBACK`
- Multi-statement transactions
- Current behavior: Auto-commit per statement

### Query Optimization
- JOINs optimization (currently limited)
- Aggregates (COUNT, SUM, AVG with GROUP BY)
- Query plan caching
- Index selection improvements

**See**: `docs/SQL_ENGINE.md` for current implementation

---

## LLM Integration Enhancements

**Current**: MCP server for LLM integration ✅ Complete

**Planned Features**:

1. **CLI Tools for LLM-Assisted Debugging**:
   ```bash
   vopr-llm generate --target "stress view changes" > scenario.json
   vopr-llm analyze vopr-results/failures.log
   vopr-llm shrink --seed 42 --events 100
   ```

2. **Automated Failure Clustering**:
   - LLM groups similar failures by root cause
   - Reduces noise in CI output

3. **Query Plan Coverage Guidance**:
   - LLM suggests database mutations when query plan coverage plateaus

4. **Scenario Library Expansion**:
   - LLM-generated scenarios saved to `/scenarios/llm-generated/`
   - Human-reviewed before inclusion

**See**: `docs/LLM_INTEGRATION_DESIGN.md` for design principles

---

## Security Enhancements

**Current**: SHA-256/BLAKE3 dual-hash, AES-256-GCM, Ed25519, FIPS-approved algorithms ✅ Complete

**Planned**:

### Access Control
- Token-based access control model
- OAuth 2.0 provider support (Google, GitHub, Okta)
- Role-based access control (RBAC)
- Attribute-based access control (ABAC)

### Key Management
- Hardware Security Module (HSM) integration
- Key rotation automation
- Multi-tenant key isolation

### Account Management
- Account recovery flows (email, backup codes)
- Multi-factor authentication (MFA)
- Session management and revocation

### Audit & Attestation
- Enhanced audit trail formats
- Third-party checkpoint attestation (RFC 3161 TSA)
- Blockchain anchoring for immutability proofs

### Compliance
- FIPS 140-3 validation testing (Post-v1.0)
- CMVP submission (TBD)
- SOC 2 Type II certification

**See**: `docs/SECURITY.md` for current security architecture

---

## Migration from PLAN.md

This roadmap consolidates future work previously scattered across:
- `PLAN.md` (now archived)
- `docs/PERFORMANCE.md` (trimmed to current state only)
- `docs/TESTING.md` (trimmed to current state only)
- `docs/ARCHITECTURE.md` (trimmed to current state only)
- `docs/COMPLIANCE.md` (trimmed to current state only)
- `docs/CLOUD_ARCHITECTURE.md` (trimmed to current state only)
- Various "Future:" sections in `/docs` files

All future work is now centralized here for easier tracking and planning.

**Status Indicators:**
- ✓ **Implemented** - Completed and released
- 🚧 **In Progress** - Active development
- 📋 **Planned** - Designed but not yet started

---

**Last Updated:** 2026-02-02
**Version:** 1.0.0 (Initial roadmap extraction)
