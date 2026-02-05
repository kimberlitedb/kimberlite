# SQL Overview

Kimberlite provides SQL access to the append-only log through projections.

## Current Status

**SQL Engine Status (v0.4.0):**
- ✅ Core append-only log API (stable)
- 🚧 SQL projection engine (in progress)
- 📅 Full SQL support planned for v0.6.0

## Accessing Data

### Event API (Current)

Direct access to the append-only log:

```rust
use kimberlite::Client;

// Append events
client.append(TenantId::new(1), StreamId::new(1, 100), event_data)?;

// Read events
let events = client.read_stream(TenantId::new(1), StreamId::new(1, 100))?;

// Query by position
let events = client.read_from_position(Position::new(1000))?;
```

See [Coding Guides](../../coding/) for language-specific examples.

### SQL Projections (v0.6.0+)

SQL access through materialized projections:

```sql
-- Create projection (materializes view of log)
CREATE PROJECTION patients AS
  SELECT
    data->>'id' AS id,
    data->>'name' AS name,
    data->>'dob' AS date_of_birth,
    position,
    timestamp
  FROM events
  WHERE tenant_id = 1
    AND stream_id = 100;

-- Query projection (standard SQL)
SELECT * FROM patients WHERE name LIKE 'Alice%';

-- Join projections
SELECT p.name, a.appointment_date
FROM patients p
JOIN appointments a ON p.id = a.patient_id;
```

## SQL Support Roadmap

### v0.6.0 - Core SQL (Q2 2024)
- ✅ CREATE PROJECTION with SELECT
- ✅ Basic SELECT with WHERE, ORDER BY, LIMIT
- ✅ Simple JOINs (INNER, LEFT)
- ✅ Aggregates (COUNT, SUM, AVG, MAX, MIN)
- ✅ GROUP BY, HAVING

### v0.7.0 - Advanced SQL (Q3 2024)
- Subqueries
- Common Table Expressions (WITH)
- Window functions
- UNION, INTERSECT, EXCEPT
- Advanced JOINs (RIGHT, FULL OUTER)

### v0.8.0 - DML (Q4 2024)
- INSERT INTO projections
- UPDATE projections
- DELETE FROM projections
- Transactions (BEGIN, COMMIT, ROLLBACK)

See [SQL Engine Design](../../internals/design/sql-engine.md) for technical details.

## Key Concepts

### 1. Log-First Architecture

All data lives in the append-only log. SQL projections are **derived views**:

```
┌─────────────────────────────────┐
│    Append-Only Log (Source)     │
│  Position | Tenant | Stream | Data
│      1    |   1    |  100   | {...}
│      2    |   1    |  100   | {...}
│      3    |   2    |  200   | {...}
└─────────────────────────────────┘
              ↓
      ┌──────────────┐
      │  Projection  │  ← Materialized View
      │  (patients)  │
      └──────────────┘
```

**Key property:** Projections can be rebuilt from the log at any time.

### 2. Projections are Eventually Consistent

Projections update asynchronously from the log:

```rust
// Append to log (immediately durable)
client.append(tenant, stream, event)?;  // Position 1000

// Projection updates asynchronously (~10ms)
let row = client.query("SELECT * FROM patients WHERE id = 123")?;
// May not include position 1000 yet
```

**Mitigation:** Wait for projection to catch up:

```rust
client.wait_for_position(Position::new(1000))?;
let row = client.query("SELECT * FROM patients WHERE id = 123")?;
// Guaranteed to include position 1000
```

### 3. Time-Travel Queries

Query projections as of any log position:

```sql
-- Current state
SELECT * FROM patients WHERE id = 123;

-- State at position 1000
SELECT * FROM patients
AS OF POSITION 1000
WHERE id = 123;

-- State at timestamp
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-15 10:30:00'
WHERE id = 123;
```

See [Time-Travel Queries Recipe](../../coding/recipes/time-travel-queries.md).

### 4. Multi-Tenant Isolation

SQL queries are automatically scoped to the client's tenant:

```rust
// Client authenticated as Tenant 1
let client = Client::connect_with_tenant("localhost:7000", TenantId::new(1))?;

// Can only see Tenant 1's data
client.query("SELECT * FROM patients")?;
// Returns only Tenant 1's patients, even if projection includes all tenants
```

See [Multi-Tenant Queries Recipe](../../coding/recipes/multi-tenant-queries.md) for cross-tenant access.

## SQL Dialects

Kimberlite SQL aims for PostgreSQL compatibility:

| Feature | Status | Notes |
|---------|--------|-------|
| Basic SELECT | ✅ | WHERE, ORDER BY, LIMIT |
| JOINs | ✅ | INNER, LEFT |
| Aggregates | ✅ | COUNT, SUM, AVG, MIN, MAX |
| Subqueries | 🚧 | v0.7.0 |
| CTEs (WITH) | 🚧 | v0.7.0 |
| Window functions | 🚧 | v0.7.0 |
| INSERT/UPDATE/DELETE | 🚧 | v0.8.0 |

**PostgreSQL-specific features NOT supported:**
- Stored procedures
- Triggers (use log-based triggers instead)
- User-defined functions
- Extensions (pg_crypto, etc.)

## Data Types

Kimberlite supports standard SQL types:

| SQL Type | Rust Type | Example |
|----------|-----------|---------|
| `BIGINT` | `i64` | `123456789` |
| `TEXT` | `String` | `'Alice Johnson'` |
| `BOOLEAN` | `bool` | `true` |
| `TIMESTAMP` | `DateTime<Utc>` | `'2024-01-15 10:30:00'` |
| `JSON` | `serde_json::Value` | `'{"key": "value"}'` |
| `BYTEA` | `Vec<u8>` | `E'\\xDEADBEEF'` |

**Note:** All numeric types are stored as `BIGINT` (64-bit). No `INTEGER`, `SMALLINT`, `REAL`, or `DOUBLE PRECISION`.

## Performance

### Projection Updates

Projections update asynchronously:
- **Latency:** ~10ms lag typical
- **Throughput:** 100k+ events/sec
- **Catchup:** Rebuilds from log at 1M+ events/sec

### Query Performance

Projections use standard indexing:
- **Point queries:** <1ms (indexed)
- **Range scans:** ~10k rows/ms
- **Aggregations:** ~1M rows/sec
- **Joins:** Depends on cardinality

**Best practices:**
- Index frequently queried columns
- Use WHERE clauses to reduce scan size
- Avoid SELECT * (fetch only needed columns)

## Limitations

### No Ad-Hoc Schemas

Projections must be defined upfront:

```sql
-- ❌ Cannot do this
SELECT data->>'new_field' FROM events;

-- ✅ Must create projection first
CREATE PROJECTION patients AS
  SELECT data->>'new_field' AS new_field
  FROM events;

SELECT new_field FROM patients;
```

### No Cross-Tenant Queries

Cannot JOIN across tenants without explicit grants:

```sql
-- ❌ Not allowed
SELECT t1.name, t2.appointments
FROM tenant_1.patients t1
JOIN tenant_2.appointments t2 ON t1.id = t2.patient_id;
```

See [Multi-Tenant Queries](../../coding/recipes/multi-tenant-queries.md) for data sharing API.

### No Mutable State

Projections are derived from immutable log:

```sql
-- ❌ Cannot update log entries
UPDATE events SET data = '{}' WHERE position = 1000;

-- ✅ Append new event
INSERT INTO events (tenant_id, stream_id, data)
VALUES (1, 100, '{"status": "updated"}');
```

## Migration from Event API

Existing Event API code will continue to work:

```rust
// v0.5.0: Event API (always supported)
client.append(tenant, stream, event)?;
let events = client.read_stream(tenant, stream)?;

// v0.6.0+: SQL projections (additional option)
client.query("SELECT * FROM patients WHERE id = 123")?;
```

Both APIs access the same underlying log.

## Related Documentation

- **[DDL Reference](ddl.md)** - CREATE/DROP PROJECTION, INDEX
- **[DML Reference](dml.md)** - INSERT/UPDATE/DELETE (v0.8.0+)
- **[Query Reference](queries.md)** - SELECT syntax
- **[SQL Engine Design](../../internals/design/sql-engine.md)** - Technical details

---

**Key Takeaway:** Kimberlite SQL provides familiar query interface over the append-only log. Projections are eventually consistent, rebuildable, and support time-travel queries. Full SQL support planned for v0.6.0.
