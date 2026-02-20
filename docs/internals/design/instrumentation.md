---
title: "VSR Instrumentation Architecture"
section: "internals/design"
slug: "instrumentation"
order: 1
---

# VSR Instrumentation Architecture

**Date:** 2026-02-05
**Phase:** 5 (Observability & Polish)
**Status:** Design Complete

## Executive Summary

This document specifies the production instrumentation architecture for Kimberlite VSR. The design provides comprehensive observability through structured metrics, OpenTelemetry integration, and performance profiling hooks.

**Key Requirements:**
- <1% performance overhead in production
- Standard observability formats (OpenTelemetry, Prometheus)
- Real-time operational visibility
- Historical performance analysis
- Compliance audit trail support

---

## Architecture Overview

```text
┌─────────────────────────────────────────────────────────────┐
│                    VSR Protocol Handlers                     │
│  (replica/normal.rs, view_change.rs, recovery.rs, etc.)    │
└──────────────────┬──────────────────────────────────────────┘
                   │ record_metric()
                   ▼
┌─────────────────────────────────────────────────────────────┐
│                  Instrumentation Layer                       │
│  ┌────────────┐  ┌────────────┐  ┌────────────────────┐   │
│  │ Histograms │  │  Counters  │  │  Gauges            │   │
│  │ (latency)  │  │ (ops/sec)  │  │  (queue depth)     │   │
│  └────────────┘  └────────────┘  └────────────────────┘   │
└──────────────────┬──────────────────────────────────────────┘
                   │ export()
                   ▼
┌─────────────────────────────────────────────────────────────┐
│              OpenTelemetry Exporter                          │
│  ┌────────────┐  ┌────────────┐  ┌────────────────────┐   │
│  │ Prometheus │  │   Jaeger   │  │  Custom Backends   │   │
│  └────────────┘  └────────────┘  └────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

---

## Metric Categories

### 1. Latency Metrics (Histograms)

Track end-to-end latency for critical operations.

| Metric | Description | Buckets (ms) |
|--------|-------------|--------------|
| `vsr_prepare_latency_ms` | Time from Prepare send to PrepareOk quorum | [0.1, 0.5, 1, 2, 5, 10, 25, 50, 100] |
| `vsr_commit_latency_ms` | Time from PrepareOk quorum to Commit broadcast | [0.1, 0.5, 1, 2, 5, 10, 25, 50, 100] |
| `vsr_client_latency_ms` | Total client request latency (Prepare → Commit → Apply) | [1, 5, 10, 25, 50, 100, 250, 500, 1000] |
| `vsr_view_change_latency_ms` | Time to complete view change | [10, 50, 100, 250, 500, 1000, 5000] |
| `vsr_recovery_latency_ms` | Time to recover from crash | [100, 500, 1000, 5000, 10000, 30000] |
| `vsr_state_transfer_latency_ms` | Time to complete state transfer | [1000, 5000, 10000, 30000, 60000] |
| `vsr_repair_latency_ms` | Time to repair single log entry | [1, 5, 10, 25, 50, 100] |

**Implementation:**
- Use logarithmic buckets for wide range coverage
- P50, P95, P99 automatically calculated
- Per-replica and cluster-wide aggregation

### 2. Throughput Metrics (Counters)

Track operation rates and volumes.

| Metric | Description | Labels |
|--------|-------------|--------|
| `vsr_operations_total` | Total operations committed | `{replica_id, status=success\|failure}` |
| `vsr_bytes_written_total` | Total bytes written to log | `{replica_id}` |
| `vsr_messages_sent_total` | Total VSR messages sent | `{replica_id, message_type}` |
| `vsr_messages_received_total` | Total VSR messages received | `{replica_id, message_type}` |
| `vsr_byzantine_rejections_total` | Total Byzantine messages rejected | `{replica_id, reason}` |
| `vsr_checksum_failures_total` | Total checksum validation failures | `{replica_id, component=log\|superblock}` |
| `vsr_repairs_total` | Total log repair operations | `{replica_id, status=success\|failure}` |
| `vsr_view_changes_total` | Total view changes initiated | `{replica_id, reason}` |

**Implementation:**
- Monotonically increasing counters
- Rate calculation via PromQL: `rate(vsr_operations_total[1m])`
- Per-second, per-minute, per-hour aggregation

### 3. Health Metrics (Gauges)

Track current cluster state and health.

| Metric | Description | Range |
|--------|-------------|-------|
| `vsr_replica_status` | Current replica status | 0=Normal, 1=ViewChange, 2=Recovering, 3=StateTransfer |
| `vsr_view_number` | Current view number | [0, ∞) |
| `vsr_commit_number` | Current commit number | [0, ∞) |
| `vsr_op_number` | Current op number | [0, ∞) |
| `vsr_log_size_bytes` | Log size in bytes | [0, ∞) |
| `vsr_log_entry_count` | Number of log entries | [0, ∞) |
| `vsr_quorum_size` | Required quorum size | [1, MAX_REPLICAS] |
| `vsr_cluster_size` | Current cluster size | [1, MAX_REPLICAS] |
| `vsr_replica_lag_operations` | Operations behind leader | [0, ∞) |
| `vsr_pending_requests` | Number of pending client requests | [0, ∞) |
| `vsr_prepare_ok_votes` | PrepareOk votes for current op | [0, cluster_size] |

**Implementation:**
- Updated on state changes
- Sampled every 1 second for time-series
- Alert thresholds configurable

### 4. Resource Metrics (Gauges)

Track system resource utilization.

| Metric | Description | Unit |
|--------|-------------|------|
| `vsr_memory_used_bytes` | Memory allocated by VSR | bytes |
| `vsr_network_bandwidth_bytes_per_sec` | Network I/O rate | bytes/sec |
| `vsr_disk_iops` | Disk operations per second | ops/sec |
| `vsr_disk_bandwidth_bytes_per_sec` | Disk I/O bandwidth | bytes/sec |
| `vsr_cpu_utilization_percent` | CPU usage by VSR threads | [0, 100] |

**Implementation:**
- System metrics via OS-specific APIs
- Sampled every 5 seconds
- Integration with system monitoring tools

### 5. Phase-Specific Metrics

**Clock Synchronization (Phase 1):**
| Metric | Description |
|--------|-------------|
| `vsr_clock_offset_ms` | Estimated clock offset from leader |
| `vsr_clock_samples_total` | Total clock samples collected |
| `vsr_clock_sync_errors_total` | Clock synchronization failures |

**Client Sessions (Phase 1):**
| Metric | Description |
|--------|-------------|
| `vsr_client_sessions_active` | Number of active client sessions |
| `vsr_client_sessions_evicted_total` | Total sessions evicted |
| `vsr_duplicate_requests_total` | Duplicate requests detected |

**Repair Budgets (Phase 2):**
| Metric | Description |
|--------|-------------|
| `vsr_repair_budget_available` | Available repair credits |
| `vsr_repair_ewma_latency_ms` | EWMA latency per replica |
| `vsr_repair_inflight_count` | Number of inflight repair requests |

**Log Scrubbing (Phase 3):**
| Metric | Description |
|--------|-------------|
| `vsr_scrub_tours_completed_total` | Total scrub tours completed |
| `vsr_scrub_corruptions_detected_total` | Total corruptions detected |
| `vsr_scrub_throughput_ops_per_sec` | Scrub throughput |

**Reconfiguration (Phase 4):**
| Metric | Description |
|--------|-------------|
| `vsr_reconfig_state` | 0=Stable, 1=Joint |
| `vsr_reconfig_transitions_total` | Total reconfigurations |
| `vsr_cluster_version_major` | Cluster software version (major) |
| `vsr_cluster_version_minor` | Cluster software version (minor) |

**Standby Replicas (Phase 4):**
| Metric | Description |
|--------|-------------|
| `vsr_standby_count` | Number of registered standbys |
| `vsr_standby_healthy_count` | Number of healthy standbys |
| `vsr_standby_lag_operations` | Standby lag behind cluster |
| `vsr_standby_promotions_total` | Total standby promotions |

---

## OpenTelemetry Integration

### Exporter Configuration

```rust
// Example: Configure OTLP exporter
let exporter = opentelemetry_otlp::new_exporter()
    .with_endpoint("http://otel-collector:4317")
    .with_protocol(Protocol::Grpc)
    .with_timeout(Duration::from_secs(5));

let meter = opentelemetry::global::meter("kimberlite-vsr");
```

### Supported Backends

1. **Prometheus** (pull-based)
   - Expose `/metrics` endpoint
   - Prometheus scrapes every 15 seconds
   - Standard Prometheus exposition format

2. **OTLP** (push-based)
   - Push to OpenTelemetry Collector
   - Batch export every 10 seconds
   - Supports Jaeger, Zipkin, etc.

3. **StatsD** (push-based)
   - UDP datagram export
   - Low overhead, fire-and-forget
   - Integration with Datadog, Grafana Cloud

### Metric Export Format

```
# HELP vsr_operations_total Total operations committed
# TYPE vsr_operations_total counter
vsr_operations_total{replica_id="0",status="success"} 12345
vsr_operations_total{replica_id="1",status="success"} 12340

# HELP vsr_prepare_latency_ms Time from Prepare send to PrepareOk quorum
# TYPE vsr_prepare_latency_ms histogram
vsr_prepare_latency_ms_bucket{replica_id="0",le="0.1"} 450
vsr_prepare_latency_ms_bucket{replica_id="0",le="0.5"} 890
vsr_prepare_latency_ms_bucket{replica_id="0",le="1"} 1200
vsr_prepare_latency_ms_bucket{replica_id="0",le="+Inf"} 1250
vsr_prepare_latency_ms_sum{replica_id="0"} 845.32
vsr_prepare_latency_ms_count{replica_id="0"} 1250
```

---

## Performance Profiling Hooks

### 1. Critical Path Timing

Measure consensus round-trip time:

```rust
// Start timer
let timer = Instant::now();

// ... consensus protocol ...

// Record duration
instrumentation::record_consensus_rtt(timer.elapsed());
```

**Profiling Points:**
- Prepare send → PrepareOk quorum → Commit broadcast
- Heartbeat round-trip time
- View change complete time
- Recovery complete time

### 2. Memory Allocation Tracking

Track allocations in hot paths:

```rust
#[cfg(feature = "profiling")]
{
    let before = get_allocated_bytes();

    // ... allocating operation ...

    let allocated = get_allocated_bytes() - before;
    instrumentation::record_allocation("log_append", allocated);
}
```

**Tracked Allocations:**
- Log entry allocation
- Message serialization
- State machine effects
- Repair buffer allocation

### 3. CPU Profiling Integration

Support for external profilers:

```rust
// Mark critical section start
#[cfg(feature = "profiling")]
instrumentation::profile_scope_start("prepare_handler");

// ... critical path code ...

// Mark critical section end
#[cfg(feature = "profiling")]
instrumentation::profile_scope_end("prepare_handler");
```

**Integration With:**
- `pprof` (flamegraph generation)
- `perf` (Linux perf tool)
- `Instruments` (macOS profiler)
- `cargo-flamegraph`

### 4. Network I/O Profiling

Track network bandwidth per message type:

```rust
instrumentation::record_network_send(
    message_type,
    message_size_bytes,
    destination_replica,
);
```

**Tracked Metrics:**
- Bytes sent/received per message type
- Message serialization time
- Network latency distribution
- Bandwidth utilization per replica

---

## Implementation Plan

### Phase 5.1: Core Metrics (~200 LOC)

**File:** `crates/kimberlite-vsr/src/instrumentation.rs`

```rust
// Extend existing file with production metrics

/// Production metrics (always available, not feature-gated)
pub struct Metrics {
    // Histograms
    prepare_latency: Histogram,
    commit_latency: Histogram,
    client_latency: Histogram,

    // Counters
    operations_total: Counter,
    bytes_written_total: Counter,
    messages_sent_total: CounterVec, // labeled by message_type

    // Gauges
    replica_status: Gauge,
    view_number: Gauge,
    commit_number: Gauge,
    log_size_bytes: Gauge,
}

impl Metrics {
    pub fn record_prepare_latency(&self, duration: Duration) {
        self.prepare_latency.observe(duration.as_secs_f64() * 1000.0);
    }

    pub fn increment_operations(&self, status: &str) {
        self.operations_total
            .with_label_values(&[status])
            .inc();
    }

    // ... more recording methods ...
}
```

### Phase 5.2: OpenTelemetry Export (~150 LOC)

**File:** `crates/kimberlite-vsr/src/instrumentation.rs`

```rust
pub struct OtelExporter {
    meter: Meter,
    exporter_type: ExporterType,
}

pub enum ExporterType {
    Prometheus { endpoint: String },
    Otlp { endpoint: String },
    StatsD { endpoint: String },
}

impl OtelExporter {
    pub fn new(exporter_type: ExporterType) -> Result<Self> {
        let meter = match exporter_type {
            ExporterType::Prometheus { ref endpoint } => {
                opentelemetry_prometheus::exporter()
                    .with_endpoint(endpoint)
                    .init()
            }
            ExporterType::Otlp { ref endpoint } => {
                opentelemetry_otlp::new_pipeline()
                    .metrics(runtime::Tokio)
                    .with_exporter(
                        opentelemetry_otlp::new_exporter()
                            .tonic()
                            .with_endpoint(endpoint)
                    )
                    .build()?
            }
            ExporterType::StatsD { ref endpoint } => {
                // StatsD implementation
                // ...
            }
        };

        Ok(Self { meter, exporter_type })
    }

    pub fn export(&self, metrics: &Metrics) -> Result<()> {
        // Export metrics to configured backend
        // ...
    }
}
```

### Phase 5.3: Profiling Hooks (~100 LOC)

**File:** `crates/kimberlite-vsr/src/instrumentation.rs`

```rust
#[cfg(feature = "profiling")]
pub mod profiling {
    use std::time::Instant;

    thread_local! {
        static SCOPE_STACK: RefCell<Vec<(&'static str, Instant)>> =
            RefCell::new(Vec::new());
    }

    pub fn profile_scope_start(name: &'static str) {
        SCOPE_STACK.with(|stack| {
            stack.borrow_mut().push((name, Instant::now()));
        });
    }

    pub fn profile_scope_end(name: &'static str) {
        SCOPE_STACK.with(|stack| {
            if let Some((scope_name, start)) = stack.borrow_mut().pop() {
                assert_eq!(scope_name, name, "mismatched profile scope");
                let duration = start.elapsed();
                record_profile_sample(name, duration);
            }
        });
    }

    fn record_profile_sample(name: &'static str, duration: Duration) {
        // Export to pprof/perf format
        // ...
    }
}

// Convenience macro
#[macro_export]
macro_rules! profile_scope {
    ($name:expr) => {
        #[cfg(feature = "profiling")]
        let _scope = $crate::instrumentation::profiling::ProfileScope::new($name);
    };
}
```

### Phase 5.4: Integration (~200 LOC)

Wire up metrics throughout VSR:

**File:** `crates/kimberlite-vsr/src/replica/normal.rs`

```rust
pub(crate) fn on_prepare(mut self, ...) -> (Self, ReplicaOutput) {
    let timer = Instant::now();

    // ... existing logic ...

    // Record latency
    METRICS.record_prepare_latency(timer.elapsed());
    METRICS.increment_operations("success");

    (self, output)
}
```

**Files to Modify:**
- `replica/normal.rs`: Prepare, PrepareOk, Commit, Heartbeat
- `replica/view_change.rs`: View change latency
- `replica/recovery.rs`: Recovery latency
- `log_scrubber.rs`: Scrub throughput
- `repair_budget.rs`: Repair latency

---

## Performance Overhead Analysis

### Microbenchmark Results (Expected)

| Operation | Without Metrics | With Metrics | Overhead |
|-----------|----------------|--------------|----------|
| Record counter | N/A | ~5 ns | 5 ns |
| Record histogram | N/A | ~25 ns | 25 ns |
| Record gauge | N/A | ~3 ns | 3 ns |
| Prepare handler | 150 μs | 150.1 μs | <0.1% |
| Commit handler | 80 μs | 80.05 μs | <0.1% |

**Total Overhead:** <1% in worst case

### Optimization Techniques

1. **Atomic Operations**: Use `AtomicU64` for counters (lock-free)
2. **Thread-Local Storage**: Reduce contention on histograms
3. **Batch Export**: Export metrics every 10 seconds, not per-operation
4. **Feature Gates**: Disable expensive profiling in production (`cfg(feature = "profiling")`)
5. **Lazy Initialization**: Only create metrics on first use

---

## Alert Thresholds (Monitoring Runbook)

### Critical Alerts

| Alert | Condition | Severity | Action |
|-------|-----------|----------|--------|
| Quorum Lost | `vsr_prepare_ok_votes < vsr_quorum_size` for 10s | P0 | Immediate investigation |
| High Latency | `vsr_client_latency_ms_p99 > 100ms` for 1m | P1 | Check network/disk |
| View Change Storm | `rate(vsr_view_changes_total[1m]) > 5` | P1 | Check leader health |
| Log Corruption | `vsr_checksum_failures_total > 0` | P0 | Trigger repair |
| Memory Leak | `vsr_memory_used_bytes` growing unbounded | P1 | Investigate |

### Warning Alerts

| Alert | Condition | Severity | Action |
|-------|-----------|----------|--------|
| Replica Lag | `vsr_replica_lag_operations > 1000` | P2 | Monitor |
| Repair Storm | `rate(vsr_repairs_total[1m]) > 100` | P2 | Check backup health |
| High Byzantine Rejection Rate | `rate(vsr_byzantine_rejections_total[1m]) > 10` | P2 | Investigate malicious replica |

---

## Dependencies

**Cargo.toml additions:**

```toml
[dependencies]
# OpenTelemetry
opentelemetry = "0.21"
opentelemetry-otlp = "0.14"
opentelemetry-prometheus = "0.14"
prometheus = "0.13"

# Optional: Profiling
[dev-dependencies]
pprof = { version = "0.13", features = ["flamegraph", "criterion"] }

[features]
profiling = []
```

---

## Testing Strategy

### Unit Tests

1. **Metric Recording**: Verify counters increment correctly
2. **Histogram Accuracy**: Verify percentiles calculated correctly
3. **Export Format**: Verify Prometheus exposition format
4. **Performance Overhead**: Microbenchmark metric recording

### Integration Tests

1. **End-to-End Metrics**: Run full VSR scenario, verify all metrics populated
2. **OTEL Export**: Export to real Prometheus instance, verify scraping
3. **Profiling Overhead**: Measure overhead with/without profiling enabled

### Load Tests

1. **Sustained Throughput**: 10k ops/sec for 1 hour, monitor metrics
2. **Burst Load**: 50k ops/sec for 1 minute, verify metric accuracy
3. **Memory Stability**: Run for 24 hours, verify no metric-related leaks

---

## Success Criteria

- ✅ <1% performance overhead in production
- ✅ All critical metrics exported to Prometheus
- ✅ OpenTelemetry integration working
- ✅ Grafana dashboard templates created
- ✅ Alert thresholds documented
- ✅ Profiling hooks integrated
- ✅ Zero metric-related bugs in production

---

## OpenTelemetry Integration (Implemented)

### Configuration

```rust
use kimberlite_vsr::instrumentation::{OtelConfig, OtelExporter};

let config = OtelConfig {
    otlp_endpoint: Some("http://localhost:4317".to_string()),
    export_interval_secs: 10,
    service_name: "kimberlite-vsr".to_string(),
    service_version: "0.4.0".to_string(),
};

let mut exporter = OtelExporter::new(config)?;
exporter.init_otlp()?;
```

### Supported Backends

1. **OTLP (OpenTelemetry Protocol)** - Push-based metrics to collector
   - Supports Prometheus, Jaeger, Zipkin backends
   - 10-second export interval (configurable)
   - Automatic batching and retry

2. **Prometheus** - Pull-based metrics scraping
   - Native exposition format via `METRICS.export_prometheus()`
   - Standard `/metrics` HTTP endpoint
   - Compatible with all Prometheus-compatible tools

3. **StatsD** - UDP push to StatsD daemon
   - Format: `metric_name:value|type`
   - Types: `c` (counter), `g` (gauge), `ms` (timer)
   - Zero-configuration, fire-and-forget

### Example: Periodic Export

```rust
use std::time::Duration;
use tokio::time;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut exporter = OtelExporter::new(OtelConfig::default())?;
    exporter.init_otlp()?;

    let mut interval = time::interval(Duration::from_secs(10));
    loop {
        interval.tick().await;
        exporter.export_metrics()?;
    }
}
```

### Example: StatsD Export

```rust
use std::net::UdpSocket;

let socket = UdpSocket::bind("0.0.0.0:0")?;
let exporter = OtelExporter::new(OtelConfig::default())?;

for line in exporter.export_statsd() {
    socket.send_to(line.as_bytes(), "localhost:8125")?;
}
```

### Feature Flag

OpenTelemetry integration is optional and requires the `otel` feature:

```toml
[dependencies]
kimberlite-vsr = { version = "0.4", features = ["otel"] }
```

This avoids pulling in OTLP dependencies (~40 crates) when not needed.

---

## References

- **OpenTelemetry Spec**: https://opentelemetry.io/docs/specs/otel/
- **Prometheus Best Practices**: https://prometheus.io/docs/practices/naming/
- **TigerBeetle Instrumentation**: Inspiration for low-overhead metrics
- **FoundationDB Metrics**: Latency histogram design

---

## Implementation Status

1. ✅ Design complete (this document)
2. ✅ Implement core metrics (~470 LOC)
3. ✅ Add OTEL export (~230 LOC)
4. ⏳ Add profiling hooks (~100 LOC)
5. ✅ Integrate into VSR (~35 LOC)
6. ⏳ Create Grafana dashboards
7. ⏳ Write monitoring runbook
8. ⏳ Write incident response playbook
