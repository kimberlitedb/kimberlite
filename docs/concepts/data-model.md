# Data Model

Kimberlite's data model is based on a single principle: **all data is an immutable, ordered log; all state is a derived view**.

## Three Levels of Abstraction

Kimberlite presents three levels to developers, allowing you to work at the appropriate level of detail.

### Level 1: Tables (Most Developers Start Here)

Most developers interact with Kimberlite through a familiar table abstraction:

```sql
-- DDL defines the projection
CREATE TABLE records (
    id          BIGINT PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  TIMESTAMP,
    region      TEXT
);

-- DML appends to the log and updates projection
INSERT INTO records (id, name, created_at, region)
VALUES (1, 'Record Alpha', '2024-01-15 10:30:00', 'us-east');

-- Queries read from projection
SELECT * FROM records WHERE id = 1;
```

**Under the hood:**
- `INSERT` becomes an event appended to the tenant's log
- The projection engine applies the event to update the `records` projection
- `SELECT` reads from the materialized projection (B+tree)

### Level 2: Events (Audit/History)

When you need the audit trail, query the event stream directly:

```sql
-- See all events in a stream
SELECT * FROM __events
WHERE stream = 'records'
ORDER BY position DESC
LIMIT 10;

-- Point-in-time reconstruction
SELECT * FROM records AS OF POSITION 12345
WHERE id = 1;
```

Events are the source of truth. Tables are just a convenient view.

### Level 3: Custom Projections (Advanced)

For complex read models, define custom projections:

```sql
-- Projection with JOIN (computed at write time, not query time)
CREATE PROJECTION entity_summary AS
SELECT
    e.id,
    e.name,
    COUNT(a.id) as activity_count,
    MAX(a.timestamp) as last_activity
FROM entities e
LEFT JOIN activities a ON a.entity_id = e.id
GROUP BY e.id, e.name;

-- Query the pre-computed view
SELECT * FROM entity_summary WHERE id = 1;
```

Projections are maintained incrementally as events arrive, not computed at query time.

## Event Structure

Every event in the log has this structure:

```rust
struct Event {
    // Position in the log
    position: LogPosition,

    // Cryptographic chain
    prev_hash: Hash,
    hash: Hash,

    // Metadata
    tenant_id: TenantId,
    stream: StreamId,
    timestamp: Timestamp,
    caused_by: Option<EventId>,  // Correlation

    // Payload
    event_type: EventType,
    data: Bytes,

    // Integrity
    checksum: Crc32,
}
```

**Key fields:**

- **position** - Global sequence number in the log
- **prev_hash** - Links to previous event (tamper-evident chain)
- **hash** - Hash of this event's contents
- **tenant_id** - Which tenant owns this event
- **stream** - Logical grouping within tenant
- **timestamp** - When the event was committed
- **caused_by** - Optional correlation to parent event
- **event_type** - What kind of event (INSERT, UPDATE, DELETE, etc.)
- **data** - Application payload (serialized bytes)
- **checksum** - CRC32 for integrity

## The Log

The append-only log is the system of record. All state derives from it.

### Segment Structure

The log is divided into segments for manageability:

```
data/
├── log/
│   ├── 00000000.segment     # Segment 0: positions 0-999999
│   ├── 00000001.segment     # Segment 1: positions 1000000-1999999
│   ├── 00000002.segment     # Segment 2: current active segment
│   └── index.meta           # Segment index metadata
```

Each segment is a sequential file of records:

```
┌─────────────────────────────────────────────────────────────────┐
│ Segment File                                                     │
│                                                                  │
│  ┌──────────┬──────────┬──────────┬──────────┬─────────────┐   │
│  │ Record 0 │ Record 1 │ Record 2 │   ...    │  Record N   │   │
│  └──────────┴──────────┴──────────┴──────────┴─────────────┘   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Record Format

Each record is self-describing and integrity-checked:

```
┌──────────────────────────────────────────────────────────────────┐
│ Record (on disk)                                                  │
│                                                                   │
│  ┌─────────┬──────────┬───────────┬──────────┬────────────────┐ │
│  │ Length  │ Checksum │ Prev Hash │ Metadata │     Data       │ │
│  │ (4 B)   │ (4 B)    │ (32 B)    │ (var)    │    (var)       │ │
│  └─────────┴──────────┴───────────┴──────────┴────────────────┘ │
│                                                                   │
└──────────────────────────────────────────────────────────────────┘
```

- **Length**: Total record size in bytes (u32)
- **Checksum**: CRC32 of the entire record (excluding length field)
- **Prev Hash**: SHA-256 hash of the previous record (32 bytes)
- **Metadata**: Position, tenant, stream, timestamp, event type
- **Data**: Application payload (serialized bytes)

### Hash Chaining

Every record includes the hash of the previous record, creating a tamper-evident chain:

```
Record N-1          Record N            Record N+1
┌─────────┐         ┌─────────┐         ┌─────────┐
│ ...     │         │ ...     │         │ ...     │
│ hash ───┼────────►│ prev    │         │ prev    │
│         │         │ hash ───┼────────►│ hash    │
└─────────┘         └─────────┘         └─────────┘
```

If any record is modified, all subsequent hashes become invalid.

## Projections (Derived Views)

Projections are materialized views derived from the log.

### How Projections Work

```
┌─────────────────────────────────────────────────────────────────┐
│                        Append-Only Log                           │
│  [Event 1] → [Event 2] → [Event 3] → ... → [Event N]             │
└────────────────────┬───────────────────────────────────────────┘
                     │ Apply
                     ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Projection                                │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ B+Tree Index                                              │   │
│  │  ┌────┐  ┌────┐  ┌────┐                                  │   │
│  │  │ K1 │  │ K2 │  │ K3 │  ...                              │   │
│  │  └──┬─┘  └──┬─┘  └──┬─┘                                  │   │
│  │     │       │       │                                     │   │
│  │     ▼       ▼       ▼                                     │   │
│  │  [Row 1] [Row 2] [Row 3] ...                              │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

**Key properties:**

1. **Incrementally maintained**: New events update projections immediately
2. **Rebuildable**: Delete a projection and rebuild it from the log
3. **Multiple projections**: Different views for different query patterns
4. **MVCC**: Multiple versions for point-in-time queries

### Example: Table Projection

```sql
CREATE TABLE patients (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    dob DATE,
    region TEXT
);
```

**Behind the scenes:**
1. Creates a B+tree projection keyed by `id`
2. On `INSERT`: Add row to B+tree
3. On `UPDATE`: Modify row (keeping old version for MVCC)
4. On `DELETE`: Mark row as deleted (keeping tombstone)

## Time-Travel Queries

Query data as it existed at any point:

```sql
-- As of specific timestamp
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-15 10:30:00'
WHERE id = 123;

-- As of specific log position
SELECT * FROM patients
AS OF POSITION 1000
WHERE region = 'us-east';
```

**How it works:**
1. Each row version stores `created_at` position
2. Query scans B+tree, filters versions by position
3. Returns rows visible at that position

## Stream Model

Events are organized into streams:

```
Tenant 1
├── Stream A (e.g., "patients")
│   ├── Event 1
│   ├── Event 2
│   └── Event 3
├── Stream B (e.g., "appointments")
│   ├── Event 1
│   └── Event 2
└── Stream C (e.g., "billing")
    └── Event 1

Tenant 2
├── Stream A
│   └── Event 1
└── Stream B
    └── Event 1
```

**Stream properties:**
- Logical grouping within a tenant
- Enables per-stream queries
- Supports event correlation
- Maps to tables in SQL layer

## Idempotency

Every command includes an idempotency ID to prevent duplicates:

```rust
// Client generates ID before first attempt
let idempotency_id = IdempotencyId::generate();

// First attempt
let result = client.execute_with_id(idempotency_id, cmd).await;

// If network fails, retry with SAME ID
let result = client.execute_with_id(idempotency_id, cmd).await;
// Returns same result without re-executing
```

The log stores idempotency IDs, ensuring exactly-once semantics.

## Compaction (Future)

While the log is append-only, old segments can be **compacted** to reclaim space:

1. **Snapshot projection** at position P
2. **Delete log segments** before P
3. **Store snapshot** as new "base" for replays

This preserves audit trails while managing storage.

**Status:** Planned for v0.6.0. See [ROADMAP.md](../../ROADMAP.md).

## Key Takeaways

1. **Log is truth, state is cache**: Projections can always be rebuilt from the log
2. **Immutability**: Nothing is ever modified or deleted in the log
3. **Time-travel**: Query any point in history
4. **Tamper-evident**: Hash chains detect modifications
5. **Idempotent**: Commands can be safely retried

## Related Documentation

- **[Architecture](architecture.md)** - System overview
- **[Consensus](consensus.md)** - How VSR replicates the log
- **[Compliance](compliance.md)** - Audit trails and tamper evidence
- **[Storage Internals](../internals/architecture/storage.md)** - Low-level storage format

---

**Remember:** In Kimberlite, there is no "current state." There is only "what the log says." Everything else is just a convenient view.
