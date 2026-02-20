---
title: "Repair Budget Management"
section: "internals"
slug: "repair-budget"
order: 7
---

# Repair Budget Management

**Module:** `crates/kimberlite-vsr/src/repair_budget.rs`
**TLA+ Spec:** `specs/tla/RepairBudget.tla`
**Kani Proofs:** `crates/kimberlite-vsr/src/kani_proofs.rs` (Proofs #30-32)
**VOPR Scenarios:** 3 scenarios (RepairBudgetPreventsStorm, RepairEwmaSelection, RepairSyncTimeout)

---

## Overview

Kimberlite's repair budget management prevents **repair storms** that cause cascading cluster failures. This issue was discovered and documented by TigerBeetle in production deployments.

### The TigerBeetle Repair Storm Bug

**Problem:**
When a replica lags behind the cluster (e.g., due to slow disk or network partition), it floods the cluster with unbounded repair requests to catch up. TigerBeetle's send queues are sized to only **4 messages** per connection. Unbounded repair requests overflow these queues, causing:

1. **Queue overflow** → dropped messages → more view changes
2. **Cascading failures** → other replicas slow down from repair load
3. **Cluster unavailability** → violates liveness guarantees

**Example:**
```
1. Replica 2 lags behind by 10,000 operations
2. Replica 2 sends repair requests for all 10,000 ops simultaneously
3. Each repair message adds to send queue (4-message limit)
4. Queue overflows → critical messages (PrepareOk, Commit) dropped
5. Primary thinks Replica 2 is down → triggers view change
6. New primary also overwhelmed → cascading failure
```

**Impact:** Production clusters become unavailable during repair operations, violating SLAs.

**Fix:** Credit-based rate limiting with EWMA latency tracking:
- Limit inflight repairs (max 2 per replica)
- Route repairs to fastest replicas (90% EWMA-based selection, 10% experiment)
- Expire stale requests (500ms timeout)
- Penalize slow replicas by increasing their EWMA on timeout

---

## Solution Architecture

### Core Design

```rust
pub struct RepairBudget {
    /// Per-replica latency tracking (EWMA)
    replicas: HashMap<ReplicaId, ReplicaLatency>,

    /// Our own replica ID (we don't send repairs to ourselves)
    self_replica_id: ReplicaId,

    /// Total cluster size
    cluster_size: usize,
}

struct ReplicaLatency {
    replica_id: ReplicaId,
    ewma_latency_ns: u64,           // EWMA latency in nanoseconds
    inflight_count: usize,           // Current inflight repairs
    inflight_requests: Vec<InflightRepair>,  // Request tracking
}
```

### Key Operations

**1. Replica Selection (EWMA-based routing)**
```rust
// 90% select fastest replica (minimum EWMA latency)
// 10% experiment with random replica (to detect recovery)
let replica_id = budget.select_replica(&mut rng)?;
```

**2. Send Repair Request**
```rust
// Check budget before sending
if budget.has_available_slots() {
    let replica_id = budget.select_replica(&mut rng)?;
    budget.record_repair_sent(replica_id, op_start, op_end, Instant::now());
    // ... send message ...
}
```

**3. Complete Repair (update EWMA)**
```rust
// On successful repair response
budget.record_repair_completed(replica_id, op_start, op_end, Instant::now());
// EWMA updated: alpha * new_sample + (1 - alpha) * old_ewma
```

**4. Expire Stale Requests**
```rust
// Called periodically (e.g., every 100ms)
let expired = budget.expire_stale_requests(Instant::now());
for (replica_id, op_start, op_end) in expired {
    // Retry repair request to different replica
}
```

---

## Implementation Details

### Constants

```rust
const MAX_INFLIGHT_PER_REPLICA: usize = 2;  // Prevents queue overflow
const REPAIR_TIMEOUT_MS: u64 = 500;          // Stale request expiry
const EWMA_ALPHA: f64 = 0.2;                 // Smoothing factor (TigerBeetle value)
const EXPERIMENT_CHANCE: f64 = 0.1;          // 10% random selection
```

### EWMA Latency Tracking

**Formula:** `EWMA = alpha * new_sample + (1 - alpha) * old_ewma`

- **Alpha = 0.2**: Balances responsiveness vs stability
- **Initial EWMA = 1ms**: Conservative default
- **Timeout penalty**: 2x current EWMA added on expiry

**Example:**
```
Initial EWMA = 1,000,000 ns (1ms)
Repair completes in 500µs:
  New EWMA = 0.2 * 500,000 + 0.8 * 1,000,000 = 900,000 ns (900µs)

Repair times out:
  Penalty = 2 * 900,000 = 1,800,000 ns
  New EWMA = 0.2 * 1,800,000 + 0.8 * 900,000 = 1,080,000 ns (1.08ms)
```

### Replica Selection Algorithm

```rust
1. Filter available replicas (inflight < MAX_INFLIGHT_PER_REPLICA)
2. If no replicas available → return None
3. Sort replicas by EWMA latency (ascending)
4. With 10% probability → select random replica (experiment)
5. Otherwise → select fastest replica (replicas[0])
```

**Rationale for 10% experiment:**
- Slow replicas may recover (e.g., disk cache warmed up)
- Periodic testing prevents starvation
- TigerBeetle-validated value balances exploration vs exploitation

---

## Formal Verification

### TLA+ Specification (`specs/tla/RepairBudget.tla`)

**Properties Verified:**

1. **BoundedInflight**: Per-replica inflight requests never exceed MAX_INFLIGHT_PER_REPLICA (2)
2. **FairRepair**: All replicas with available slots eventually receive repair requests
3. **NoRepairStorm**: Total inflight repairs across all replicas is bounded
4. **EwmaLatencyPositive**: EWMA latency values are always positive (prevents division by zero)
5. **RequestTimeoutEnforced**: Requests older than REPAIR_TIMEOUT_MS are eventually expired
6. **InflightCountMatches**: Inflight count equals the number of tracked request send times

**Model checked:** TLC verifies all invariants hold.

### Kani Proofs (3 proofs)

1. **Proof 30: Inflight requests bounded**
   - Property: No replica exceeds MAX_INFLIGHT_PER_REPLICA (2)
   - Verified: Prevents TigerBeetle repair storm bug

2. **Proof 31: Budget replenishment via request completion**
   - Property: Completing repairs releases inflight slots
   - Verified: Resource accounting is correct

3. **Proof 32: EWMA latency calculation correctness**
   - Property: EWMA formula produces valid positive values
   - Verified: No overflow/underflow in EWMA computation

### Production Assertions (4 assertions)

All use `assert!()` (not `debug_assert!()`) for production enforcement:

1. **Inflight limit enforcement** (`record_repair_sent:167`)
   - `inflight_count < MAX_INFLIGHT_PER_REPLICA`
   - Prevents send queue overflow (TigerBeetle bug fix)

2. **Inflight count matches request tracking** (`record_repair_sent:181`)
   - `inflight_count == inflight_requests.len()`
   - Accounting invariant (prevents resource leaks)

3. **EWMA reasonable bounds** (`update_ewma:400`)
   - `ewma_latency_ns > 0 && ewma_latency_ns < 10_000_000_000`
   - Ensures latency values stay within 0-10s range
   - Lower bound prevents division by zero
   - Upper bound indicates failure (10s is unreasonable for intra-cluster RPC)

4. **Stale request removal verification** (`expire_stale_requests:329`)
   - All remaining requests have `elapsed_ms < REPAIR_TIMEOUT_MS`
   - Prevents resource leaks from stuck requests

---

## VOPR Testing (3 scenarios)

### 1. RepairBudgetPreventsStorm

**Test:** Lagging replica with many pending repairs
**Verify:** Inflight limit enforced, no queue overflow
**Config:** 20s runtime, 50K events, 1 lagging replica

### 2. RepairEwmaSelection

**Test:** Multiple replicas with different latencies
**Verify:** Fastest replicas selected 90% of the time
**Config:** 15s runtime, 30K events, latency variance 100µs-10ms

### 3. RepairSyncTimeout

**Test:** Stale request expiry under network delays
**Verify:** Requests expire after 500ms, slots released
**Config:** 15s runtime, 40K events, 20% drop probability

**All scenarios pass:** 500K iterations per scenario, 0 violations

---

## Performance Characteristics

- **Replica selection:** O(R log R) where R = replicas (sorting by EWMA)
- **Record repair sent:** O(1) - append to inflight list
- **Record repair completed:** O(I) where I = inflight per replica (≤2)
- **Expire stale requests:** O(R * I) - check all inflight requests

**Memory per replica:** ~80 bytes (EWMA + 2 inflight requests)

**Typical overhead:** <0.5% for 3-replica cluster, <1% for 5-replica cluster

---

## Integration with VSR

### Lagging Replica (Repair Requester)

```rust
// On discovering lag (e.g., missing operation)
if repair_budget.has_available_slots() {
    let replica_id = repair_budget.select_replica(&mut rng)?;
    repair_budget.record_repair_sent(
        replica_id,
        op_range_start,
        op_range_end,
        Instant::now(),
    );
    send_repair_request(replica_id, op_range_start, op_range_end);
}

// Periodically check for stale requests
let expired = repair_budget.expire_stale_requests(Instant::now());
for (replica_id, op_start, op_end) in expired {
    // Retry with different replica
}
```

### Responding Replica (Repair Provider)

```rust
// On receiving repair response
repair_budget.record_repair_completed(
    replica_id,
    op_range_start,
    op_range_end,
    Instant::now(),
);
// EWMA latency updated automatically
```

---

## Debugging Guide

### Common Issues

**Issue:** Repairs not making progress (lagging replica stuck)
**Diagnosis:** Check `available_slots()` → if 0, all replicas at inflight limit
**Fix:** Verify responding replicas are processing repair requests, check for network issues

**Issue:** Slow replica never selected
**Diagnosis:** EWMA too high due to previous timeouts
**Fix:** Wait for 10% experiment chance to re-test, or restart replica to reset EWMA

**Issue:** Queue overflow still occurring
**Diagnosis:** MAX_INFLIGHT_PER_REPLICA too high for send queue size
**Fix:** Reduce MAX_INFLIGHT_PER_REPLICA (currently 2)

### Assertions That Catch Bugs

| Assertion | What It Catches | Line |
|-----------|----------------|------|
| `inflight < MAX_INFLIGHT_PER_REPLICA` | Send queue overflow (TigerBeetle bug) | 167 |
| `inflight_count == inflight_requests.len()` | Resource accounting error | 181 |
| `ewma_latency_ns > 0 && < 10s` | EWMA overflow/underflow or failure | 400 |
| `elapsed_ms < REPAIR_TIMEOUT_MS` | Stale request not removed | 329 |

---

## References

### Industry Implementations
- **TigerBeetle**: `src/vsr/repair_budget.zig` (discovered and fixed repair storm bug)
- **FoundationDB**: Rate-limited recovery (similar credit-based approach)

### Academic Papers
- EWMA smoothing: "Exponentially Weighted Moving Average" (statistical process control)
- Multi-armed bandit: ε-greedy exploration (10% experiment chance)

### Internal Documentation
- `docs/internals/vsr.md` - VSR implementation overview
- `docs/traceability_matrix.md` - TLA+ → Rust → VOPR traceability

---

## Future Work

- [ ] **Adaptive MAX_INFLIGHT_PER_REPLICA** (scale based on send queue depth)
- [ ] **Multi-priority repair** (critical ops repaired first)
- [ ] **Cross-datacenter awareness** (prefer local replicas for repair)
- [ ] **Repair batching** (coalesce small ranges into larger requests)

---

**Implementation Status:** ✅ Complete (Phase 1.3 - v0.4.0)
**Verification:** 3 Kani proofs, 3 VOPR scenarios, 1 TLA+ spec, 4 production assertions
**TigerBeetle Bug Fixed:** Repair storm causing queue overflow and cascading failures
