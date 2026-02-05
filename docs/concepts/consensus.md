# Consensus - Viewstamped Replication (VSR)

Kimberlite uses Viewstamped Replication (VSR) for consensus, the same protocol used by TigerBeetle.

## What is Consensus?

**Consensus** ensures multiple replicas agree on the order of operations, even when some replicas fail or networks partition.

Without consensus:
- Replica A thinks operation X happened first
- Replica B thinks operation Y happened first
- ❌ Divergent state → data corruption

With consensus:
- All replicas agree: X, then Y
- ✅ Same order → same state

## Why VSR?

Kimberlite uses VSR instead of Raft or Paxos for several reasons:

| Property | VSR | Raft | Paxos |
|----------|-----|------|-------|
| **Simplicity** | ✅ Fewer states | ❌ More complex | ❌ Very complex |
| **Determinism** | ✅ Explicit view | ✅ Explicit term | ❌ Implicit rounds |
| **Recovery** | ✅ Strong repair | ⚠️ Log catch-up only | ❌ Weak |
| **Production proven** | ✅ TigerBeetle | ✅ Many systems | ✅ Google (Chubby) |

**Key advantage:** VSR was designed for *state machine replication*—exactly Kimberlite's use case.

## Cluster Topology

A Kimberlite cluster consists of `2f + 1` replicas to tolerate `f` failures:

```
f=1 (3 replicas):  Can tolerate 1 failure
f=2 (5 replicas):  Can tolerate 2 failures
f=3 (7 replicas):  Can tolerate 3 failures (typical production)
```

**Example 3-replica cluster:**

```
┌──────────┐     ┌──────────┐     ┌──────────┐
│ Replica  │     │ Replica  │     │ Replica  │
│   (P)    │     │   (B)    │     │   (B)    │
│  Leader  │     │ Backup   │     │ Backup   │
└──────────┘     └──────────┘     └──────────┘
      │               │                │
      └───────────────┼────────────────┘
                      │
                Consensus Group
```

**Roles:**
- **Primary (P):** Coordinates operations, assigns order
- **Backup (B):** Replicates operations, monitors primary health

## Normal Operation (Happy Path)

When everything works, VSR is simple:

```
Client      Primary       Backup 1      Backup 2
  │           │              │              │
  │ Request   │              │              │
  ├──────────►│              │              │
  │           │   Prepare    │   Prepare    │
  │           ├─────────────►├─────────────►│
  │           │   PrepareOK  │   PrepareOK  │
  │           │◄─────────────┤◄─────────────┤
  │           │              │              │
  │           │   Commit     │   Commit     │
  │           ├─────────────►├─────────────►│
  │           │              │              │
  │   Reply   │              │              │
  │◄──────────┤              │              │
```

**Steps:**

1. **Client Request**: Client sends command to primary
2. **Prepare**: Primary assigns position in log, broadcasts `Prepare` to backups
3. **PrepareOK**: Backups acknowledge with `PrepareOK`
4. **Commit**: Primary receives quorum (f+1), broadcasts `Commit`
5. **Apply**: All replicas apply the committed command
6. **Reply**: Primary sends result to client

**Key properties:**
- **Quorum:** Need f+1 PrepareOK messages (majority)
- **Order:** Primary assigns sequential positions
- **Durability:** Command must be on disk before PrepareOK
- **Determinism:** All replicas apply commands in identical order

## View Changes (Failure Handling)

When the primary fails, backups elect a new primary:

```
View 0:                View 1:
P, B1, B2              B1 (new P), B2, (old P offline)
```

**View change protocol:**

1. **Timeout**: Backup detects primary failure (no heartbeat)
2. **Start View Change**: Backup broadcasts `StartViewChange`
3. **Do View Change**: Replicas send their state to new primary
4. **Start View**: New primary synchronizes replicas, begins accepting requests

**Critical invariant:** View changes preserve all committed operations.

### Why Views Matter

Views provide **explicit epochs**:

```rust
struct Operation {
    view: View,      // Which primary assigned this operation
    position: u64,   // Position within that view
}
```

This allows replicas to detect stale messages:
- Message from view=5 arrives when cluster is in view=7 → ignored
- Prevents split-brain scenarios

## Repair Mechanisms

VSR includes mechanisms to repair replicas that have diverged:

### 1. Log Repair (Small Gaps)

**Scenario:** Backup missed a few messages.

```
Primary:  [Op 1] [Op 2] [Op 3] [Op 4]
Backup:   [Op 1] [Op 2] [  ?  ] [  ?  ]
                          ↑
                      RequestRepair
```

**Solution:** Backup requests missing operations, primary sends them.

### 2. State Transfer (Large Gaps)

**Scenario:** Backup is far behind (e.g., after extended downtime).

```
Primary:  [Op 1...10000]
Backup:   [Op 1...100]  (9900 operations behind)
```

**Solution:** Primary sends a snapshot + recent log tail.

### 3. Nack Protocol (Message Loss)

**Scenario:** Backup detects gap in sequence numbers.

```
Backup receives: Op 5, Op 7 (where's Op 6?)
Backup sends: Nack(6)
Primary resends: Op 6
```

## Single-Node Mode (Development)

For development and testing, Kimberlite supports single-node operation:

```rust
// In single-node mode, VSR degenerates to:
// 1. Append to local log
// 2. Apply immediately
// 3. Return result

impl SingleNodeReplicator {
    fn propose(&mut self, command: Command) -> Result<Position> {
        let position = self.log.append(command)?;
        Ok(position)
    }
}
```

No network, no consensus overhead. Perfect for local development.

## Safety Guarantees

VSR provides strong safety guarantees:

### 1. Agreement

**Guarantee:** If two replicas commit an operation at position P, they commit the same operation.

```
Replica A at P=100: INSERT patient_id=123
Replica B at P=100: INSERT patient_id=123 (must be identical)
```

### 2. Prefix Property

**Guarantee:** If replica A commits operation P, and replica B commits operation P', then either:
- A's log is a prefix of B's log, or
- B's log is a prefix of A's log

No divergent histories.

### 3. Total Order

**Guarantee:** All replicas process operations in the same order.

### 4. Durability

**Guarantee:** Committed operations survive f failures.

If the cluster has 5 replicas (f=2), any 2 replicas can fail without data loss.

## Liveness Guarantees

VSR also provides liveness (progress) guarantees:

### 1. Eventual Progress

**Guarantee:** If fewer than f replicas fail, the cluster eventually makes progress.

### 2. View Change Completion

**Guarantee:** View changes complete within bounded time (assuming asynchronous networks).

### 3. Repair Completion

**Guarantee:** Healthy replicas can repair lagging replicas.

## Fault Tolerance

| Cluster Size | f (tolerate) | Quorum | Explanation |
|--------------|--------------|--------|-------------|
| 1 replica    | 0            | 1      | No fault tolerance (dev only) |
| 3 replicas   | 1            | 2      | Tolerate 1 failure |
| 5 replicas   | 2            | 3      | Tolerate 2 failures (typical) |
| 7 replicas   | 3            | 4      | Tolerate 3 failures (high availability) |

**Why odd numbers?**

Even-sized clusters waste capacity:
- 4 replicas (f=1, quorum=3) → same fault tolerance as 3 replicas
- 6 replicas (f=2, quorum=4) → same fault tolerance as 5 replicas

## Byzantine Fault Tolerance

**Kimberlite does NOT provide BFT.** VSR assumes crash-fail replicas (replicas fail by stopping, not by behaving maliciously).

However, VOPR testing includes Byzantine scenarios to detect implementation bugs. See:
- [VOPR CLI Reference](../reference/cli/vopr.md)
- [VOPR Scenarios](../../docs-internal/vopr/scenarios.md) - Phase 1 documents 11 Byzantine attack scenarios

## Performance Characteristics

**Latency:**
- Single-node: ~1ms (log append only)
- 3-replica cluster (same datacenter): ~2-3ms
- 3-replica cluster (cross-AZ): ~5-10ms
- 5-replica cluster (cross-region): ~20-50ms

**Throughput:**
- Bottleneck: Primary's disk write bandwidth
- Typical: 10k-50k ops/sec per cluster (depends on operation size)

**Scalability:**
- VSR does not horizontally scale (not a distributed system like Cassandra)
- For higher throughput: Shard across multiple VSR groups
- See [Multi-tenancy](multitenancy.md) for sharding strategies

## Comparison with Other Protocols

### VSR vs Raft

| Feature | VSR | Raft |
|---------|-----|------|
| Leader election | View-based | Term-based |
| Log repair | Built-in state transfer | Log catch-up only |
| Complexity | Simpler (fewer states) | More complex |
| Production use | TigerBeetle, MemSQL | etcd, CockroachDB, many others |

**Bottom line:** VSR and Raft provide similar guarantees. VSR is slightly simpler, Raft has more tooling.

### VSR vs Paxos

| Feature | VSR | Paxos |
|---------|-----|-------|
| Model | State machine replication | Consensus on single values |
| Determinism | Explicit views | Implicit rounds |
| Understandability | ✅ Easier | ❌ Notoriously difficult |

**Bottom line:** Paxos is more general but harder to implement correctly. VSR is purpose-built for replicating state machines.

## Further Reading

- **[Data Model](data-model.md)** - What VSR replicates
- **[Multi-tenancy](multitenancy.md)** - How tenants map to VSR groups
- **[VSR Production Gaps](../internals/vsr-production-gaps.md)** - Known limitations
- **[Testing Overview](../internals/testing/overview.md)** - How we validate consensus safety

## Academic References

- **Original VSR paper:** Oki & Liskov (1988) - "Viewstamped Replication: A New Primary Copy Method to Support Highly-Available Distributed Systems"
- **VRR (Revisited):** Liskov & Cowling (2012) - "Viewstamped Replication Revisited"
- **TigerBeetle's experience:** [TigerBeetle blog](https://tigerbeetle.com/blog)

---

**Key Takeaway:** VSR ensures all replicas agree on operation order, even when failures occur. It's simpler than Raft, proven in production, and purpose-built for state machine replication—exactly what Kimberlite needs.
