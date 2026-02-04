# Kimberlite VSR Monitoring Runbook

**Version:** 1.0
**Date:** 2026-02-05
**Status:** Production Ready

## Table of Contents

1. [Overview](#overview)
2. [Key Metrics Reference](#key-metrics-reference)
3. [Dashboard Design](#dashboard-design)
4. [Alert Thresholds](#alert-thresholds)
5. [Metric Interpretation](#metric-interpretation)
6. [Grafana Dashboards](#grafana-dashboards)
7. [Prometheus Queries](#prometheus-queries)
8. [Troubleshooting](#troubleshooting)

---

## Overview

This runbook provides comprehensive guidance for monitoring Kimberlite VSR clusters in production. It covers metric collection, alerting, dashboard design, and interpretation.

### Monitoring Architecture

```text
┌─────────────────────────────────────────────────────────┐
│  Kimberlite VSR Replicas (3-5 nodes)                    │
│  - Exposes /metrics endpoint (Prometheus format)        │
│  - Push to OTLP collector (optional)                    │
│  - Push to StatsD (optional)                            │
└────────────────┬────────────────────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────────────────────┐
│  Prometheus Server                                       │
│  - Scrapes /metrics every 15s                           │
│  - 30-day retention (configurable)                      │
│  - Alert evaluation (Alertmanager)                      │
└────────────────┬────────────────────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────────────────────┐
│  Grafana                                                 │
│  - Real-time dashboards                                  │
│  - Historical analysis                                   │
│  - Alert visualization                                   │
└─────────────────────────────────────────────────────────┘
```

### Monitoring Goals

1. **Detect failures** before they impact availability (< 1 minute detection time)
2. **Diagnose issues** with sufficient context (metric correlations, logs)
3. **Track performance** against SLOs (p50/p95/p99 latency, throughput)
4. **Capacity planning** with historical trends (growth projections)

---

## Key Metrics Reference

### Latency Metrics (Histograms)

| Metric | Description | Buckets | Target |
|--------|-------------|---------|--------|
| `vsr_prepare_latency_ms` | Prepare send → PrepareOk quorum | 0.1, 0.5, 1, 2, 5, 10, 25, 50, 100ms | p95 < 10ms |
| `vsr_commit_latency_ms` | PrepareOk quorum → Commit broadcast | 0.1, 0.5, 1, 2, 5, 10, 25, 50, 100ms | p95 < 5ms |
| `vsr_client_latency_ms` | Client request → Applied + Effects | 0.1, 0.5, 1, 2, 5, 10, 25, 50, 100ms | p95 < 15ms |
| `vsr_view_change_latency_ms` | ViewChange → Normal operation | 10, 50, 100, 250, 500, 1000, 5000ms | p95 < 500ms |

**Interpretation:**
- **Prepare latency** reflects network RTT + quorum coordination
- **Commit latency** is typically <1ms (in-process broadcast)
- **Client latency** = prepare + commit + application logic
- **View change latency** includes leader election + log reconciliation

### Throughput Metrics (Counters)

| Metric | Description | Target |
|--------|-------------|--------|
| `vsr_operations_total` | Total operations committed | 1,000-10,000 ops/sec |
| `vsr_operations_failed_total` | Total operations failed | < 0.1% failure rate |
| `vsr_bytes_written_total` | Total bytes written to log | Dependent on workload |
| `vsr_messages_sent_prepare` | Prepare messages sent | ~ops × replicas |
| `vsr_messages_sent_prepare_ok` | PrepareOk messages sent | ~ops × replicas |
| `vsr_messages_sent_commit` | Commit messages sent | ~ops × replicas |
| `vsr_messages_sent_heartbeat` | Heartbeat messages sent | ~1/sec × replicas |
| `vsr_messages_sent_view_change` | View change messages sent | Rare (0-10/hour) |
| `vsr_messages_received_total` | Total messages received | ~3× operations |
| `vsr_checksum_failures_total` | Checksum validation failures | 0 (corruption detected) |
| `vsr_repairs_total` | Repairs completed | <1% of operations |

**Interpretation:**
- **Operations rate** = throughput (higher is better)
- **Failure rate** should be near zero (application errors only)
- **Message ratios** validate protocol health (Prepare:PrepareOk:Commit ≈ 1:n:1)
- **Checksum failures** indicate storage corruption (alert immediately)
- **Repairs** indicate lagging replicas (normal in low percentages)

### Health Metrics (Gauges)

| Metric | Description | Healthy Range |
|--------|-------------|---------------|
| `vsr_view_number` | Current view number | Stable (changes rare) |
| `vsr_commit_number` | Highest committed op | Monotonically increasing |
| `vsr_op_number` | Highest prepared op | ≥ commit_number |
| `vsr_log_size_bytes` | Log size in bytes | < 10 GB (with checkpoints) |
| `vsr_log_entry_count` | Log entry count | < 1M entries |
| `vsr_replica_status` | Replica status (0=Normal, 1=ViewChange, 2=Recovering, 3=StateTransfer) | 0 (Normal) |
| `vsr_quorum_size` | Required quorum size | (n+1)/2 |
| `vsr_pending_requests` | Pending client requests | < 1000 |

**Interpretation:**
- **View number stability** indicates healthy leader (frequent changes = network issues)
- **Commit lag** (op_number - commit_number) should be <10 ops
- **Log size growth** requires periodic checkpointing
- **Replica status** should be Normal (0) most of the time
- **Pending requests** accumulation indicates backpressure

### Phase-Specific Metrics (Gauges)

| Metric | Description | Healthy Range | Phase |
|--------|-------------|---------------|-------|
| `vsr_clock_offset_ms` | Clock offset from leader | < 100ms | Phase 1 (Clock Sync) |
| `vsr_client_sessions_active` | Active client sessions | Workload-dependent | Phase 1 (Client Sessions) |
| `vsr_repair_budget_available` | Available repair budget | > 50% | Phase 2 (Repair Budget) |
| `vsr_scrub_tours_completed` | Completed scrub tours | Incremental | Phase 3 (Scrubbing) |
| `vsr_reconfig_state` | Reconfiguration state (0=Stable, 1=Joint) | 0 (Stable) | Phase 4 (Reconfig) |
| `vsr_standby_count` | Number of standby replicas | 0-2 typical | Phase 4 (Standby) |

**Interpretation:**
- **Clock offset** > 100ms triggers alert (NTP issues)
- **Repair budget** depletion indicates overwhelmed cluster
- **Scrub tours** should complete every 24 hours
- **Reconfiguration** should be transient (minutes, not hours)

---

## Dashboard Design

### Dashboard 1: VSR Cluster Overview

**Purpose:** High-level health at a glance (NOC/SRE team view)

**Panels:**

1. **Cluster Health Status** (Single Stat)
   - Green if all replicas Normal, Red if any ViewChange/Recovering
   - Query: `min(vsr_replica_status) == 0`

2. **Operations Throughput** (Graph)
   - Rate of committed operations (ops/sec)
   - Query: `rate(vsr_operations_total[1m])`
   - Color: Green (>1000), Yellow (500-1000), Red (<500)

3. **Client Latency (p95)** (Graph)
   - 95th percentile client latency
   - Query: `histogram_quantile(0.95, rate(vsr_client_latency_ms_bucket[5m]))`
   - Target line: 15ms

4. **Error Rate** (Graph)
   - Failed operations per second
   - Query: `rate(vsr_operations_failed_total[1m])`
   - Alert threshold: >1 failure/sec

5. **View Stability** (Graph)
   - View number over time (should be flat)
   - Query: `vsr_view_number`
   - Annotations: View changes

6. **Commit Lag** (Graph)
   - Difference between op_number and commit_number
   - Query: `vsr_op_number - vsr_commit_number`
   - Alert threshold: >100 ops

**Refresh:** 5 seconds

---

### Dashboard 2: VSR Performance Deep Dive

**Purpose:** Detailed latency analysis and performance tuning

**Panels:**

1. **Prepare Latency Heatmap** (Heatmap)
   - Distribution of prepare latency over time
   - Query: `rate(vsr_prepare_latency_ms_bucket[1m])`

2. **Latency Percentiles** (Graph)
   - p50, p95, p99 for prepare, commit, client latency
   - Queries:
     - `histogram_quantile(0.50, rate(vsr_prepare_latency_ms_bucket[5m]))`
     - `histogram_quantile(0.95, rate(vsr_prepare_latency_ms_bucket[5m]))`
     - `histogram_quantile(0.99, rate(vsr_prepare_latency_ms_bucket[5m]))`

3. **Message Throughput** (Graph - stacked area)
   - Messages sent per second by type
   - Queries:
     - `rate(vsr_messages_sent_prepare[1m])`
     - `rate(vsr_messages_sent_prepare_ok[1m])`
     - `rate(vsr_messages_sent_commit[1m])`

4. **Network Efficiency** (Gauge)
   - Bytes written per operation
   - Query: `rate(vsr_bytes_written_total[5m]) / rate(vsr_operations_total[5m])`

5. **Prepare-to-Commit Ratio** (Graph)
   - Should be ~1.0 (every prepare commits)
   - Query: `rate(vsr_messages_sent_commit[5m]) / rate(vsr_messages_sent_prepare[5m])`

**Refresh:** 10 seconds

---

### Dashboard 3: VSR Replica Health

**Purpose:** Per-replica monitoring for troubleshooting

**Panels:**

1. **Replica Status Table** (Table)
   - Columns: Replica ID, Status, View, Commit Number, Op Number, Lag
   - Queries: `vsr_replica_status`, `vsr_view_number`, `vsr_commit_number`, `vsr_op_number`

2. **Per-Replica Throughput** (Graph)
   - Operations committed per replica
   - Query: `rate(vsr_operations_total[1m])`
   - Grouped by: `replica_id`

3. **Log Size per Replica** (Graph)
   - Log size in GB over time
   - Query: `vsr_log_size_bytes / 1e9`

4. **Pending Requests per Replica** (Graph)
   - Queue depth per replica
   - Query: `vsr_pending_requests`

5. **Checksum Failures** (Graph)
   - Corruption detection events
   - Query: `increase(vsr_checksum_failures_total[1h])`
   - Alert: Any increase

**Refresh:** 10 seconds

---

### Dashboard 4: VSR Capacity Planning

**Purpose:** Long-term trends and capacity forecasting

**Panels:**

1. **Operations Growth (30 days)** (Graph)
   - Daily operations throughput
   - Query: `avg_over_time(rate(vsr_operations_total[1h])[30d:1h])`

2. **Log Growth Rate** (Graph)
   - GB written per day
   - Query: `increase(vsr_bytes_written_total[1d]) / 1e9`

3. **Projected Capacity** (Table)
   - Days until disk full (based on growth rate)
   - Query: `(disk_size_gb - vsr_log_size_bytes / 1e9) / (increase(vsr_bytes_written_total[7d]) / 1e9 / 7)`

4. **Peak vs Average Throughput** (Graph)
   - Max and avg ops/sec over 24 hours
   - Queries:
     - `max_over_time(rate(vsr_operations_total[5m])[24h:5m])`
     - `avg_over_time(rate(vsr_operations_total[5m])[24h:5m])`

5. **Client Session Growth** (Graph)
   - Active client sessions over time
   - Query: `vsr_client_sessions_active`

**Refresh:** 1 minute

---

## Alert Thresholds

### Critical Alerts (P0 - Immediate Response)

| Alert | Condition | Threshold | Action |
|-------|-----------|-----------|--------|
| **ClusterDown** | All replicas unreachable | 3/3 replicas down for 1min | Page on-call engineer |
| **DataCorruption** | Checksum validation failed | Any checksum failure | Investigate immediately, isolate replica |
| **QuorumLost** | Less than quorum replicas healthy | <2 of 3 replicas for 2min | Emergency response |
| **ViewChangeStorm** | Excessive view changes | >5 view changes in 5min | Check network, leader health |
| **HighErrorRate** | Operation failures spiking | >5% failure rate for 1min | Check application errors |

**Critical Alert Routing:**
- PagerDuty: High urgency
- Slack: #incidents channel
- Email: oncall@company.com

---

### High Priority Alerts (P1 - Urgent Response)

| Alert | Condition | Threshold | Action |
|-------|-----------|-----------|--------|
| **HighLatency** | Client latency exceeds target | p95 >50ms for 5min | Investigate load, network |
| **CommitLag** | Leader not committing operations | Lag >500 ops for 2min | Check leader CPU, quorum |
| **DiskNearFull** | Storage capacity low | >80% disk usage | Trigger checkpoint, add capacity |
| **ReplicaRecovering** | Replica in recovering state | Recovering for >10min | Check logs, restart if hung |
| **ClockDrift** | Clock offset too high | >200ms for 5min | Check NTP, investigate leader |

**High Priority Routing:**
- Slack: #alerts channel
- Email: oncall@company.com

---

### Medium Priority Alerts (P2 - Business Hours Response)

| Alert | Condition | Threshold | Action |
|-------|-----------|-----------|--------|
| **RepairBudgetLow** | Repair budget depleted | <20% available for 10min | Investigate lagging replicas |
| **SlowScrubTours** | Scrubbing not completing | 0 tours in 48 hours | Check scrubber configuration |
| **HighPendingRequests** | Request queue backing up | >1000 pending for 5min | Check backpressure, add capacity |
| **LogGrowthAcceleration** | Disk usage growing rapidly | 2× normal growth rate | Investigate write patterns |
| **StandbyUnhealthy** | Standby replica unhealthy | Unhealthy for >30min | Investigate standby, consider promotion |

**Medium Priority Routing:**
- Slack: #monitoring channel
- Email: team@company.com

---

### Low Priority Alerts (P3 - Informational)

| Alert | Condition | Threshold | Action |
|-------|-----------|-----------|--------|
| **ReconfigurationInProgress** | Cluster reconfiguring | Joint state for >1 hour | Monitor progress, check logs |
| **MinorLatencyIncrease** | Latency slightly elevated | p95 20-50ms for 30min | Investigate trends, consider tuning |
| **HighMessageRate** | Message throughput unusual | 3× normal rate | Validate workload changes |

**Low Priority Routing:**
- Slack: #monitoring channel

---

## Metric Interpretation

### Scenario 1: High Latency

**Symptoms:**
- `vsr_client_latency_ms` p95 >50ms
- `vsr_prepare_latency_ms` p95 >20ms

**Diagnosis:**
1. Check network latency between replicas (`ping` between nodes)
2. Check replica CPU utilization (`top`, `htop`)
3. Check disk I/O latency (`iostat -x 1`)
4. Check for network packet loss (`netstat -s`)

**Common Causes:**
- Network congestion (saturated NIC, switch issues)
- CPU contention (high load average)
- Disk I/O bottleneck (slow NVMe, full disk)
- Cross-region latency (geographic distance)

**Resolution:**
- Upgrade network (10 Gbps → 25 Gbps)
- Add CPU cores or reduce load
- Upgrade to faster NVMe (PCIe 3.0 → 4.0)
- Co-locate replicas in same region/AZ

---

### Scenario 2: Frequent View Changes

**Symptoms:**
- `vsr_view_number` incrementing rapidly (>5 changes/hour)
- `vsr_replica_status` flapping between Normal and ViewChange

**Diagnosis:**
1. Check leader replica logs for heartbeat timeout messages
2. Check network connectivity between leader and backups
3. Check leader CPU/memory pressure (OOM killer, high load)
4. Check for clock drift (`vsr_clock_offset_ms` >100ms)

**Common Causes:**
- Network partition (firewall rules, routing issues)
- Leader overloaded (CPU, memory, disk)
- NTP synchronization failure (clock drift)
- Misconfigured timeouts (too aggressive)

**Resolution:**
- Fix network issues (check firewall, routing)
- Scale leader vertically (more CPU/memory)
- Restart NTP daemon, verify time sources
- Tune timeout values in config (increase heartbeat_interval_ms)

---

### Scenario 3: Commit Lag Accumulation

**Symptoms:**
- `vsr_op_number - vsr_commit_number` >100
- `vsr_pending_requests` increasing

**Diagnosis:**
1. Check if leader has quorum (`vsr_quorum_size` vs healthy replicas)
2. Check backup replica health (CPU, disk, network)
3. Check for slow application logic (long-running commands)
4. Check message queues (backpressure in send buffers)

**Common Causes:**
- Backup replica lagging (CPU, disk, network issues)
- Quorum lost (too many replicas down)
- Application logic blocking (database locks, slow queries)
- Network send queue full (message backlog)

**Resolution:**
- Restart lagging backup replica
- Wait for quorum recovery (repair/state transfer)
- Optimize application logic (indexes, caching)
- Tune TCP send buffer sizes (sysctl)

---

### Scenario 4: Checksum Failures

**Symptoms:**
- `vsr_checksum_failures_total` incrementing

**Diagnosis:**
1. Identify replica with failures (check logs)
2. Check disk health (`smartctl -a /dev/nvme0n1`)
3. Check for bit flips (ECC errors in dmesg)
4. Check storage firmware version

**Common Causes:**
- Disk hardware failure (bad sectors, firmware bug)
- Memory corruption (ECC errors)
- Storage controller issue (driver bug)
- Cosmic rays (rare, single-event upsets)

**Resolution:**
- Replace failing disk immediately
- Enable ECC memory if not already
- Update storage firmware
- Trigger repair from healthy replicas (`vsr_repairs_total` should increase)

---

## Grafana Dashboards

### Grafana Dashboard JSON: VSR Cluster Overview

```json
{
  "dashboard": {
    "title": "Kimberlite VSR - Cluster Overview",
    "tags": ["kimberlite", "vsr", "overview"],
    "timezone": "browser",
    "panels": [
      {
        "id": 1,
        "title": "Cluster Health Status",
        "type": "stat",
        "targets": [
          {
            "expr": "min(vsr_replica_status{job=\"kimberlite-vsr\"})",
            "legendFormat": "Status",
            "refId": "A"
          }
        ],
        "options": {
          "colorMode": "background",
          "graphMode": "none",
          "textMode": "value_and_name"
        },
        "fieldConfig": {
          "defaults": {
            "thresholds": {
              "mode": "absolute",
              "steps": [
                {"value": 0, "color": "green"},
                {"value": 1, "color": "red"}
              ]
            },
            "mappings": [
              {"value": 0, "text": "Normal"},
              {"value": 1, "text": "ViewChange"},
              {"value": 2, "text": "Recovering"},
              {"value": 3, "text": "StateTransfer"}
            ]
          }
        },
        "gridPos": {"h": 4, "w": 6, "x": 0, "y": 0}
      },
      {
        "id": 2,
        "title": "Operations Throughput",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(vsr_operations_total{job=\"kimberlite-vsr\"}[1m])",
            "legendFormat": "{{instance}}",
            "refId": "A"
          }
        ],
        "yaxes": [
          {"format": "ops", "label": "Operations/sec"}
        ],
        "alert": {
          "conditions": [
            {
              "evaluator": {"params": [500], "type": "lt"},
              "operator": {"type": "and"},
              "query": {"params": ["A", "5m", "now"]},
              "reducer": {"type": "avg"}
            }
          ],
          "frequency": "1m",
          "name": "Low Throughput"
        },
        "gridPos": {"h": 8, "w": 12, "x": 6, "y": 0}
      },
      {
        "id": 3,
        "title": "Client Latency (p95)",
        "type": "graph",
        "targets": [
          {
            "expr": "histogram_quantile(0.95, rate(vsr_client_latency_ms_bucket{job=\"kimberlite-vsr\"}[5m]))",
            "legendFormat": "p95",
            "refId": "A"
          },
          {
            "expr": "histogram_quantile(0.99, rate(vsr_client_latency_ms_bucket{job=\"kimberlite-vsr\"}[5m]))",
            "legendFormat": "p99",
            "refId": "B"
          }
        ],
        "yaxes": [
          {"format": "ms", "label": "Latency"}
        ],
        "thresholds": [
          {"value": 15, "colorMode": "critical", "op": "gt"}
        ],
        "gridPos": {"h": 8, "w": 12, "x": 0, "y": 8}
      },
      {
        "id": 4,
        "title": "View Stability",
        "type": "graph",
        "targets": [
          {
            "expr": "vsr_view_number{job=\"kimberlite-vsr\"}",
            "legendFormat": "{{instance}}",
            "refId": "A"
          }
        ],
        "yaxes": [
          {"format": "short", "label": "View Number"}
        ],
        "gridPos": {"h": 8, "w": 12, "x": 12, "y": 8}
      }
    ],
    "refresh": "5s",
    "time": {"from": "now-1h", "to": "now"}
  }
}
```

### Prometheus Scrape Configuration

```yaml
# /etc/prometheus/prometheus.yml

global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'kimberlite-vsr'
    static_configs:
      - targets:
          - 'replica-1:9090'
          - 'replica-2:9090'
          - 'replica-3:9090'
    metric_relabel_configs:
      # Drop high-cardinality labels if needed
      - source_labels: [__name__]
        regex: 'vsr_.*'
        action: keep
```

---

## Prometheus Queries

### Common Queries

**Operations per second (cluster-wide):**
```promql
sum(rate(vsr_operations_total[1m]))
```

**Client latency percentiles:**
```promql
histogram_quantile(0.95, sum(rate(vsr_client_latency_ms_bucket[5m])) by (le))
histogram_quantile(0.99, sum(rate(vsr_client_latency_ms_bucket[5m])) by (le))
```

**Error rate percentage:**
```promql
rate(vsr_operations_failed_total[5m]) / rate(vsr_operations_total[5m]) * 100
```

**Commit lag per replica:**
```promql
vsr_op_number - vsr_commit_number
```

**View change frequency (per hour):**
```promql
rate(vsr_view_number[1h]) * 3600
```

**Repair rate percentage:**
```promql
rate(vsr_repairs_total[5m]) / rate(vsr_operations_total[5m]) * 100
```

**Average prepare latency:**
```promql
rate(vsr_prepare_latency_ms_sum[5m]) / rate(vsr_prepare_latency_ms_count[5m])
```

**Replica health check (0 = all healthy):**
```promql
count(vsr_replica_status != 0) or vector(0)
```

**Disk space remaining (days):**
```promql
(node_filesystem_size_bytes - node_filesystem_avail_bytes) /
(rate(vsr_bytes_written_total[7d]) / 7) / 86400
```

---

## Troubleshooting

### Issue: Metrics Not Showing Up

**Symptoms:**
- Grafana shows "No data"
- Prometheus targets down

**Checklist:**
1. ✅ Verify VSR replica is running (`systemctl status kimberlite-vsr`)
2. ✅ Check /metrics endpoint is accessible (`curl http://localhost:9090/metrics`)
3. ✅ Verify Prometheus is scraping target (check Targets page in Prometheus UI)
4. ✅ Check firewall rules allow port 9090
5. ✅ Verify service discovery configuration (if using dynamic discovery)

**Resolution:**
- Restart VSR replica if /metrics endpoint unresponsive
- Update Prometheus scrape config with correct targets
- Open firewall port 9090 (`ufw allow 9090` or iptables rule)

---

### Issue: Alert Fatigue

**Symptoms:**
- Too many alerts firing
- Team ignoring alerts

**Checklist:**
1. Review alert thresholds (too sensitive?)
2. Check for duplicate alerts (multiple rules for same condition)
3. Verify alert grouping configuration (Alertmanager routes)
4. Assess alert priorities (are P3 alerts waking people up?)

**Resolution:**
- Increase alert thresholds (e.g., HighLatency from 15ms → 50ms)
- Consolidate duplicate alerts
- Configure Alertmanager grouping by cluster/replica
- Route P3 alerts to Slack only (no paging)

---

### Issue: Dashboard Performance Slow

**Symptoms:**
- Grafana dashboard loading slowly (>5 seconds)
- High CPU on Prometheus server

**Checklist:**
1. Check query complexity (are you querying 30 days of data?)
2. Review cardinality (how many time series?)
3. Check Prometheus resource usage (`top`, `prometheus_tsdb_head_series`)
4. Verify retention policy (are you keeping too much data?)

**Resolution:**
- Reduce query time range (30d → 7d for heavy queries)
- Add recording rules for expensive queries
- Increase Prometheus memory (`-storage.tsdb.max-bytes=32GB`)
- Reduce retention (`-storage.tsdb.retention.time=30d`)

---

## Appendix: SLO Definitions

### Service Level Objectives (SLOs)

| SLO | Target | Measurement Window | Consequence |
|-----|--------|-------------------|-------------|
| **Availability** | 99.95% | 30 days | < 22 minutes downtime/month |
| **Latency (p95)** | < 15ms | 5 minutes | User-facing timeout errors |
| **Latency (p99)** | < 50ms | 5 minutes | Slow requests noticeable |
| **Error Rate** | < 0.1% | 1 minute | Application failures |
| **Data Durability** | 99.999999% | N/A | < 1 data loss event per 100M ops |

### SLI Calculation

**Availability SLI:**
```promql
1 - (sum(rate(up{job="kimberlite-vsr"} == 0)[30d])) /
    (count(up{job="kimberlite-vsr"}))
```

**Latency SLI (% requests < 15ms):**
```promql
sum(rate(vsr_client_latency_ms_bucket{le="15"}[5m])) /
sum(rate(vsr_client_latency_ms_count[5m]))
```

**Error Rate SLI:**
```promql
1 - (sum(rate(vsr_operations_failed_total[1m])) /
     sum(rate(vsr_operations_total[1m])))
```

---

## Appendix: Metric Retention Policy

| Data Type | Resolution | Retention | Storage |
|-----------|-----------|-----------|---------|
| **Raw metrics** | 15s | 30 days | ~500 MB/replica |
| **5-minute aggregates** | 5m | 90 days | ~200 MB/replica |
| **1-hour aggregates** | 1h | 1 year | ~100 MB/replica |

**Recording Rules for Aggregation:**

```yaml
# /etc/prometheus/rules/kimberlite-vsr.yml

groups:
  - name: kimberlite_vsr_5m
    interval: 5m
    rules:
      - record: vsr:operations_rate:5m
        expr: rate(vsr_operations_total[5m])

      - record: vsr:client_latency_p95:5m
        expr: histogram_quantile(0.95, rate(vsr_client_latency_ms_bucket[5m]))

      - record: vsr:error_rate:5m
        expr: rate(vsr_operations_failed_total[5m]) / rate(vsr_operations_total[5m])

  - name: kimberlite_vsr_1h
    interval: 1h
    rules:
      - record: vsr:operations_rate:1h
        expr: rate(vsr_operations_total[1h])

      - record: vsr:client_latency_p99:1h
        expr: histogram_quantile(0.99, rate(vsr_client_latency_ms_bucket[1h]))
```

---

**Document Version History:**
- v1.0 (2026-02-05): Initial release for Phase 5
