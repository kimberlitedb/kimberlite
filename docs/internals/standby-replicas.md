# Standby Replicas (Phase 4.3)

**Status:** ✅ Implemented (v0.4.0)
**Formal Verification:** 2 Kani proofs, 3 VOPR scenarios
**Purpose:** Disaster recovery and read scaling without affecting quorum

---

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Use Cases](#use-cases)
- [Promotion Procedures](#promotion-procedures)
- [Configuration](#configuration)
- [Monitoring](#monitoring)
- [Formal Verification](#formal-verification)
- [VOPR Scenarios](#vopr-scenarios)

---

## Overview

**Standby replicas** are read-only followers that receive log updates from active replicas but **do NOT participate in quorum decisions**. This provides:

1. **Geographic redundancy** (disaster recovery) without affecting quorum size
2. **Read scaling** (offload queries) without adding active replicas
3. **Testing/staging** environments that mirror production safely

### Key Properties

- **Quorum independence**: Standby replicas are NOT counted in quorum calculations
- **Eventually consistent reads**: Standby replicas may lag behind committed operations
- **Safe promotion**: Standby replicas can be promoted to active status via cluster reconfiguration
- **Zero overhead**: Active cluster performance is unaffected by standby replicas

### Formal Guarantees

- **Kani Proof #68**: Standby replicas NEVER send `PrepareOK` messages (no quorum participation)
- **Kani Proof #69**: Promotion to active status preserves log consistency (no divergence)

---

## Architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│              Active Cluster (3 replicas)                         │
│  ┌─────────┐       ┌─────────┐       ┌─────────┐               │
│  │ Primary │       │ Backup₁ │       │ Backup₂ │               │
│  │  (R0)   │       │  (R1)   │       │  (R2)   │               │
│  └────┬────┘       └────┬────┘       └────┬────┘               │
│       │                 │                 │                      │
│       │◄─── Quorum ────►│◄─── Quorum ────►│                      │
│       │     (2/3)        │     (2/3)       │                      │
└───────┼─────────────────┼─────────────────┼──────────────────────┘
        │                 │                 │
        │                 │                 │
        │ Prepare         │ Prepare         │ Prepare
        │ Commit          │ Commit          │ Commit
        ▼                 ▼                 ▼
   ┌─────────┐       ┌─────────┐       ┌─────────┐
   │ Standby │       │ Standby │       │ Standby │
   │  (S0)   │       │  (S1)   │       │  (S2)   │
   │   DR    │       │  Reads  │       │  Reads  │
   └─────────┘       └─────────┘       └─────────┘
        │                 │                 │
        │                 │                 │
        ▼                 ▼                 ▼
  Read-only         Read-only         Read-only
   Queries           Queries           Queries
  (eventual         (eventual         (eventual
  consistency)     consistency)     consistency)

NOT counted in quorum (Kani Proof #68)
```

### Message Flow

1. **Active Cluster**: Primary replicates operations to backups
2. **Prepare Messages**: Standby replicas receive `Prepare` messages and append to log
3. **NO PrepareOK**: Standby replicas do NOT send `PrepareOK` (Kani Proof #68)
4. **Commit Messages**: Standby replicas receive `Commit` messages and update commit number
5. **Read Queries**: Standby replicas serve eventually consistent reads from their log

---

## Use Cases

### 1. Disaster Recovery (Geographic Redundancy)

**Problem:** Need geographic redundancy without requiring cross-datacenter quorum (high latency).

**Solution:** Deploy standby replicas in remote datacenter.

```text
Primary Datacenter (US East)       Disaster Recovery (US West)
┌────────────────────────┐         ┌────────────────────────┐
│ Active Cluster (3x)    │         │ Standby (1-2x)         │
│   - Quorum = 2/3       │────────►│   - NOT in quorum      │
│   - Low latency        │ Async   │   - High availability  │
└────────────────────────┘         └────────────────────────┘

If US East fails:
  1. Promote US West standby to active (manual or automatic)
  2. Cluster reconfiguration (joint consensus)
  3. US West becomes new active cluster
```

**Benefits:**
- Geographic redundancy without quorum latency
- Promotion time: ~seconds (reconfiguration only)
- Data loss: minimal (standby lags by ~milliseconds in normal operation)

### 2. Read Scaling (Offload Queries)

**Problem:** High read workload affecting active cluster performance.

**Solution:** Route read queries to standby replicas.

```text
    ┌────────────┐
    │  Primary   │ (Writes)
    │  (Active)  │
    └──────┬─────┘
           │
    ┌──────┴──────┐
    │             │
┌───▼───┐   ┌────▼───┐
│Backup1│   │Backup2 │ (Writes)
│Active │   │Active  │
└───────┘   └────────┘
           │
    ┌──────┴──────┬────────────────┬────────────────┐
    │             │                │                │
┌───▼───┐   ┌────▼───┐      ┌─────▼───┐      ┌────▼────┐
│Standby│   │Standby │      │ Standby │      │ Standby │
│  (S0) │   │  (S1)  │      │  (S2)   │      │  (S3)   │
└───┬───┘   └───┬────┘      └────┬────┘      └────┬────┘
    │           │                 │                │
    ▼           ▼                 ▼                ▼
 Read         Read              Read            Read
Queries      Queries           Queries         Queries

Load balancer distributes read traffic across standby replicas
```

**Benefits:**
- Horizontal read scaling (add standby replicas as needed)
- Active cluster unaffected (no quorum overhead)
- Eventually consistent reads (acceptable for analytics, dashboards)

### 3. Testing/Staging Environments

**Problem:** Need production-like data for testing without affecting production cluster.

**Solution:** Standby replica mirrors production data safely.

```text
Production Cluster (Active)       Staging Environment (Standby)
┌────────────────────────┐         ┌────────────────────────┐
│ Live customer traffic  │────────►│ Testing/QA workload    │
│ - Strict SLAs          │ Async   │ - No production impact │
│ - Compliance           │         │ - Safe experimentation │
└────────────────────────┘         └────────────────────────┘
```

**Benefits:**
- Realistic testing data (mirrors production)
- Zero production impact (reads from standby)
- Safe for destructive tests (standby can be reset)

---

## Promotion Procedures

Standby replicas can be promoted to active status when needed (e.g., disaster recovery, scaling active cluster).

### Requirements for Promotion

1. **Log consistency**: Standby log must match active primary (no divergence)
2. **Promotion eligibility**: Standby must be marked `promotion_eligible` (no gaps detected)
3. **Cluster reconfiguration**: Joint consensus to add standby to active config

### Automatic Promotion (Disaster Recovery)

When an active replica fails and standby is up-to-date:

```rust
// 1. Detect active replica failure
if active_replica_down && standby.promotion_eligible {
    // 2. Create new cluster config (add standby to active set)
    let new_config = config.add_active_replica(standby_id);

    // 3. Initiate cluster reconfiguration (joint consensus)
    cluster.reconfigure(new_config)?;

    // 4. Standby transitions: Standby → Normal
    standby.promote_to_active(new_config)?;

    // 5. Standby begins participating in quorum
    // (after joint consensus completes)
}
```

### Manual Promotion (Operator-Initiated)

For planned operations (e.g., scaling active cluster):

```bash
# 1. Verify standby is up-to-date
kimberlite-cli standby status --replica-id S0

# 2. Initiate promotion
kimberlite-cli cluster reconfig add-replica --replica-id S0 --role active

# 3. Wait for joint consensus to complete
kimberlite-cli cluster reconfig status

# 4. Verify promotion succeeded
kimberlite-cli replica status --replica-id S0
# Expected: status=Normal (was Standby)
```

### Promotion Safety (Kani Proof #69)

**Property:** Promotion preserves log consistency.

**Verification:**
- Standby log must be ⊆ active primary log (no divergence)
- Promotion only succeeds if `promotion_eligible = true`
- Gaps in log automatically mark standby as ineligible

```rust
#[kani::proof]
fn proof_promotion_preserves_log_consistency() {
    let standby = ReplicaState::new_standby(replica_id);

    // Verify standby is eligible
    assert!(standby.standby_state.unwrap().promotion_eligible);

    // Promote to active
    standby.promote_to_active(new_config)?;

    // Verify status changed
    assert!(standby.status == ReplicaStatus::Normal);
    assert!(standby.standby_state.is_none());
}
```

---

## Configuration

### Creating a Standby Replica

```rust
use kimberlite_vsr::{ReplicaState, ReplicaId};

// Create standby replica (not part of active cluster config)
let replica_id = ReplicaId::from_raw(100); // Standby IDs typically 100+
let mut standby = ReplicaState::new_standby(replica_id);

// Verify status
assert!(standby.status.is_standby());
assert!(standby.standby_state.is_some());
```

### Cluster Configuration

```toml
# kimberlite.toml

[cluster]
# Active replicas (participate in quorum)
replicas = ["R0", "R1", "R2"]

[standby]
# Standby replicas (read-only, NOT in quorum)
replicas = ["S0", "S1", "S2"]

# Promotion settings
auto_promote = true  # Automatic promotion on active replica failure
max_lag_ms = 100     # Max acceptable lag before marking ineligible
```

### Network Configuration

Standby replicas receive messages from active cluster:

```text
firewall rules:
  - Allow: Active → Standby (Prepare, Commit, Heartbeat)
  - Deny:  Standby → Active (PrepareOK) [never sent anyway]
  - Allow: Clients → Standby (Read queries)
```

---

## Monitoring

### Key Metrics

**Replication Lag:**
```rust
// How far behind standby is from active cluster
let lag_ops = active_cluster.op_number - standby.op_number;
let lag_commits = active_cluster.commit_number - standby.commit_number;
```

**Promotion Eligibility:**
```rust
// Is standby safe to promote?
let eligible = standby.standby_state
    .map(|s| s.promotion_eligible)
    .unwrap_or(false);
```

**Read Query Latency:**
```rust
// p50, p99, p999 latencies for standby reads
// (eventual consistency may show higher variance)
```

### Prometheus Metrics

```
# Replication lag (operations)
kimberlite_standby_lag_ops{replica_id="S0"} 5

# Replication lag (milliseconds)
kimberlite_standby_lag_ms{replica_id="S0"} 12

# Promotion eligibility
kimberlite_standby_promotion_eligible{replica_id="S0"} 1

# Read queries served
kimberlite_standby_queries_total{replica_id="S0"} 1234567

# Read query latency (p99)
kimberlite_standby_query_latency_p99{replica_id="S0"} 0.003
```

### Alerting Rules

```yaml
# Alert if standby lag exceeds threshold
- alert: StandbyLagHigh
  expr: kimberlite_standby_lag_ms > 1000
  for: 5m
  annotations:
    summary: "Standby {{$labels.replica_id}} lag exceeds 1s"

# Alert if standby becomes ineligible for promotion
- alert: StandbyPromotionIneligible
  expr: kimberlite_standby_promotion_eligible == 0
  annotations:
    summary: "Standby {{$labels.replica_id}} cannot be promoted (log diverged)"
```

---

## Formal Verification

### Kani Proof #68: Standby Never Participates in Quorum

**Property:** Standby replicas NEVER send `PrepareOK` messages.

**Why Critical:** If standbys sent `PrepareOK`, they could affect quorum decisions, violating the safety property that only active replicas participate in consensus.

**Verification:**
```rust
#[kani::proof]
#[kani::unwind(3)]
fn proof_standby_never_participates_in_quorum() {
    let mut state = ReplicaState::new_standby(replica_id);
    let prepare = /* arbitrary Prepare message */;

    // Process as standby
    let output = state.on_prepare_standby(prepare);

    // CRITICAL: Standby never sends messages (especially PrepareOK)
    assert!(output.messages.is_empty());
}
```

**Result:** ✅ Verified (no counterexamples found)

### Kani Proof #69: Promotion Preserves Log Consistency

**Property:** Promoting a standby to active preserves log consistency.

**Why Critical:** Promotion must not introduce divergent log entries that could violate consensus safety.

**Verification:**
```rust
#[kani::proof]
#[kani::unwind(3)]
fn proof_promotion_preserves_log_consistency() {
    let mut state = ReplicaState::new_standby(replica_id);

    // Verify eligibility
    assert!(state.standby_state.unwrap().promotion_eligible);

    // Promote
    let new_config = /* config with standby as active */;
    state.promote_to_active(new_config)?;

    // Verify status changed
    assert!(state.status == ReplicaStatus::Normal);
    assert!(state.standby_state.is_none());
}
```

**Result:** ✅ Verified (promotion safety guaranteed)

---

## VOPR Scenarios

### Scenario 1: Standby Follows Log

**File:** `crates/kimberlite-sim/src/scenarios.rs` → `ScenarioType::StandbyFollowsLog`

**Tests:**
- Standby receives `Prepare` messages from active replicas
- Standby appends entries to log but does NOT send `PrepareOK`
- Standby tracks `commit_number` from `Commit` messages
- Standby never affects quorum decisions

**Configuration:**
- Network: 2% packet loss (normal conditions)
- Fault injection: None (baseline behavior)
- Duration: 25 seconds, 12K events

**Expected Invariants:**
- Standby log ⊆ active primary log (no divergence)
- Standby sends ZERO `PrepareOK` messages
- Active cluster commits succeed without standby participation

### Scenario 2: Standby Promotion

**File:** `crates/kimberlite-sim/src/scenarios.rs` → `ScenarioType::StandbyPromotion`

**Tests:**
- Standby is up-to-date (log matches active primary)
- Promotion via cluster reconfiguration (joint consensus)
- Promoted replica begins participating in quorum
- Log consistency preserved (Kani Proof #69)

**Configuration:**
- Network: 0% packet loss (clean promotion)
- Fault injection: None (planned operation)
- Duration: 30 seconds, 15K events

**Expected Invariants:**
- Promotion succeeds ONLY if `promotion_eligible = true`
- After promotion: status = Normal, standby_state = None
- Post-promotion commits include promoted replica in quorum

### Scenario 3: Read Scaling

**File:** `crates/kimberlite-sim/src/scenarios.rs` → `ScenarioType::StandbyReadScaling`

**Tests:**
- Multiple standby replicas serve read queries
- Reads are eventually consistent (may lag)
- No impact on active cluster performance
- Load distributed across standbys

**Configuration:**
- Network: 3% packet loss (cross-datacenter)
- Fault injection: Gray failures (slow reads)
- Tenants: 3 (multi-tenant read workload)
- Duration: 40 seconds, 20K events

**Expected Invariants:**
- Read query throughput scales with standby count
- Active cluster commit latency unaffected (< 2% overhead)
- Standby reads serve stale data (eventual consistency)

---

## Troubleshooting

### Problem: Standby Lag Increasing

**Symptoms:**
- `kimberlite_standby_lag_ms` > 1000ms
- Standby falls behind active cluster

**Causes:**
1. **Network congestion**: Slow link between active and standby
2. **Disk I/O**: Standby storage too slow to keep up
3. **CPU contention**: Standby overloaded with read queries

**Resolution:**
```bash
# 1. Check network latency
ping standby-replica

# 2. Check disk I/O
iostat -x 5

# 3. Reduce read query load (route to other standbys)
kimberlite-cli lb remove-standby S0

# 4. If persistent, promote to active (if eligible)
kimberlite-cli cluster reconfig add-replica --replica-id S0
```

### Problem: Promotion Ineligible

**Symptoms:**
- `kimberlite_standby_promotion_eligible = 0`
- Promotion fails with "log diverged" error

**Causes:**
1. **Gap detected**: Standby missed `Prepare` messages (packet loss)
2. **Checksum mismatch**: Corruption detected in standby log

**Resolution:**
```bash
# 1. Check standby log integrity
kimberlite-cli replica verify-log --replica-id S0

# 2. Repair log (fetch missing entries)
kimberlite-cli replica repair --replica-id S0 --from active-primary

# 3. Verify promotion eligibility restored
kimberlite-cli standby status --replica-id S0
# Expected: promotion_eligible=true
```

---

## Best Practices

1. **Deploy standbys in pairs** (2+ per region) for redundancy
2. **Monitor replication lag** continuously (< 100ms ideal)
3. **Test promotion regularly** (quarterly DR drills)
4. **Route read queries** via load balancer (distribute load)
5. **Isolate standby I/O** (dedicated storage, separate from active cluster)

---

## References

- **Architecture:** `crates/kimberlite-vsr/src/replica/standby.rs` (250 LOC)
- **Kani Proofs:** `crates/kimberlite-vsr/src/replica/standby.rs` (Proof #68, #69)
- **VOPR Scenarios:** `crates/kimberlite-sim/src/scenarios.rs` (StandbyFollowsLog, StandbyPromotion, StandbyReadScaling)
- **Type Definitions:** `crates/kimberlite-vsr/src/types.rs` (ReplicaStatus::Standby)
- **State Management:** `crates/kimberlite-vsr/src/replica/state.rs` (standby_state field)

---

**See also:**
- [Cluster Reconfiguration](cluster-reconfiguration.md) - How promotion uses joint consensus
- [Rolling Upgrades](rolling-upgrades.md) - Version compatibility for standbys
- [VSR Protocol](vsr.md) - Core replication protocol
