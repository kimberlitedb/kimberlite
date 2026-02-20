---
title: "SQL DDL Reference"
section: "reference/sql"
slug: "ddl"
order: 2
---

# SQL DDL Reference

Data Definition Language for creating projections and indexes.

**Status:** Planned for v0.6.0

## CREATE PROJECTION

Create a materialized view of the append-only log.

### Syntax

```sql
CREATE PROJECTION projection_name AS
  SELECT
    expression [AS alias],
    ...
  FROM events
  WHERE condition
  [ORDER BY column]
  [PARTITION BY tenant_id];
```

### Examples

**Basic projection:**

```sql
CREATE PROJECTION patients AS
  SELECT
    data->>'id' AS id,
    data->>'name' AS name,
    data->>'date_of_birth' AS dob,
    position,
    timestamp
  FROM events
  WHERE tenant_id = 1
    AND stream_id = 100;
```

**Multi-tenant projection:**

```sql
CREATE PROJECTION all_patients AS
  SELECT
    tenant_id,
    data->>'id' AS id,
    data->>'name' AS name
  FROM events
  WHERE stream_id = 100
  PARTITION BY tenant_id;
```

**Filtered projection:**

```sql
CREATE PROJECTION active_patients AS
  SELECT
    data->>'id' AS id,
    data->>'name' AS name,
    data->>'status' AS status
  FROM events
  WHERE tenant_id = 1
    AND stream_id = 100
    AND data->>'status' = 'active';
```

**Denormalized projection:**

```sql
CREATE PROJECTION patient_appointments AS
  SELECT
    p.data->>'id' AS patient_id,
    p.data->>'name' AS patient_name,
    a.data->>'date' AS appointment_date,
    a.data->>'doctor' AS doctor
  FROM events p
  JOIN events a ON p.data->>'id' = a.data->>'patient_id'
  WHERE p.stream_id = 100
    AND a.stream_id = 200;
```

### Options

| Option | Description | Default |
|--------|-------------|---------|
| `PARTITION BY` | Partition projection by column (typically `tenant_id`) | None |
| `ORDER BY` | Physical sort order for range queries | None |
| `WITH (option=value)` | Storage options | See below |

**Storage options:**

```sql
CREATE PROJECTION patients AS
  SELECT ...
  FROM events
  WITH (
    compression = 'zstd',        -- Compression: none, zstd, lz4
    cache_size = '1GB',          -- Projection cache size
    checkpoint_interval = 10000  -- Entries between checkpoints
  );
```

### Permissions

- Requires `CREATE PROJECTION` permission
- Projection inherits tenant isolation from `WHERE tenant_id = ?`

## DROP PROJECTION

Delete a projection (does not affect underlying log).

### Syntax

```sql
DROP PROJECTION [IF EXISTS] projection_name;
```

### Examples

```sql
-- Drop projection
DROP PROJECTION patients;

-- Drop if exists (no error if missing)
DROP PROJECTION IF EXISTS patients;
```

### Behavior

- Deletes projection metadata and materialized data
- Does **not** delete events from the log
- Projection can be recreated with `CREATE PROJECTION`

## CREATE INDEX

Create an index on a projection for faster queries.

### Syntax

```sql
CREATE INDEX index_name
  ON projection_name (column [ASC|DESC], ...)
  [WHERE condition];
```

### Examples

**Simple index:**

```sql
CREATE INDEX patients_name_idx
  ON patients (name);
```

**Composite index:**

```sql
CREATE INDEX patients_status_dob_idx
  ON patients (status, date_of_birth);
```

**Partial index:**

```sql
CREATE INDEX active_patients_idx
  ON patients (name)
  WHERE status = 'active';
```

**Unique index:**

```sql
CREATE UNIQUE INDEX patients_id_idx
  ON patients (id);
```

### Index Types

| Type | Syntax | Use Case |
|------|--------|----------|
| **B-tree** (default) | `CREATE INDEX` | Range queries, equality |
| **Hash** | `CREATE INDEX ... USING HASH` | Equality only (faster) |
| **GIN** | `CREATE INDEX ... USING GIN` | JSON, arrays |

**Examples:**

```sql
-- Hash index (equality only)
CREATE INDEX patients_id_hash_idx
  ON patients USING HASH (id);

-- GIN index for JSON queries
CREATE INDEX patients_metadata_idx
  ON patients USING GIN (metadata);
```

### Performance

- **Index build time:** ~1M rows/sec
- **Index size:** ~50% of data size (B-tree)
- **Query speedup:** 10-1000x for indexed columns

**Best practices:**
- Index frequently queried columns
- Use composite indexes for multi-column queries
- Partial indexes for subset queries
- Avoid over-indexing (slows writes)

## DROP INDEX

Delete an index.

### Syntax

```sql
DROP INDEX [IF EXISTS] index_name;
```

### Examples

```sql
-- Drop index
DROP INDEX patients_name_idx;

-- Drop if exists
DROP INDEX IF EXISTS patients_name_idx;
```

## ALTER PROJECTION

Modify an existing projection (v0.7.0+).

### Syntax

```sql
-- Add column (rebuilds projection)
ALTER PROJECTION projection_name
  ADD COLUMN column_name AS expression;

-- Drop column
ALTER PROJECTION projection_name
  DROP COLUMN column_name;

-- Rename projection
ALTER PROJECTION old_name
  RENAME TO new_name;
```

### Examples

```sql
-- Add computed column
ALTER PROJECTION patients
  ADD COLUMN age AS EXTRACT(YEAR FROM CURRENT_DATE) - EXTRACT(YEAR FROM dob);

-- Drop column
ALTER PROJECTION patients
  DROP COLUMN middle_name;

-- Rename
ALTER PROJECTION patients
  RENAME TO all_patients;
```

**Note:** Adding/dropping columns rebuilds the projection from the log.

## REFRESH PROJECTION

Manually rebuild a projection from the log.

### Syntax

```sql
REFRESH PROJECTION projection_name
  [FROM POSITION position]
  [CONCURRENTLY];
```

### Examples

```sql
-- Full rebuild
REFRESH PROJECTION patients;

-- Rebuild from position
REFRESH PROJECTION patients
  FROM POSITION 1000;

-- Rebuild without blocking queries
REFRESH PROJECTION patients
  CONCURRENTLY;
```

### When to Use

Projections update automatically, but manual refresh is useful for:
- Recovery after corruption
- Forcing re-evaluation of projection logic
- Performance testing

## SHOW PROJECTIONS

List all projections for current tenant.

### Syntax

```sql
SHOW PROJECTIONS;
```

### Output

```
 name            | tenant_id | position  | lag | size_mb
-----------------+-----------+-----------+-----+---------
 patients        | 1         | 12345     | 0   | 128
 appointments    | 1         | 12340     | 5   | 64
 active_patients | 1         | 12345     | 0   | 32
```

### Columns

- `name` - Projection name
- `tenant_id` - Tenant owning projection
- `position` - Current projection position
- `lag` - Log position - projection position
- `size_mb` - Projection size on disk

## DESCRIBE PROJECTION

Show projection schema and metadata.

### Syntax

```sql
DESCRIBE PROJECTION projection_name;
```

### Output

```
 column          | type      | nullable
-----------------+-----------+----------
 id              | BIGINT    | NO
 name            | TEXT      | YES
 date_of_birth   | TIMESTAMP | YES
 position        | BIGINT    | NO
 timestamp       | TIMESTAMP | NO
```

## System Catalogs

Query projection metadata:

```sql
-- All projections
SELECT * FROM __projections;

-- Projection columns
SELECT * FROM __projection_columns WHERE projection_name = 'patients';

-- Projection indexes
SELECT * FROM __projection_indexes WHERE projection_name = 'patients';

-- Projection statistics
SELECT * FROM __projection_stats WHERE projection_name = 'patients';
```

## Best Practices

### 1. Partition by Tenant

```sql
-- ✅ Good: Explicit tenant filtering
CREATE PROJECTION patients AS
  SELECT ...
  FROM events
  WHERE tenant_id = 1
  PARTITION BY tenant_id;

-- ❌ Bad: No tenant filtering
CREATE PROJECTION patients AS
  SELECT ...
  FROM events;
```

### 2. Index Selectively

```sql
-- ✅ Good: Index frequently queried columns
CREATE INDEX patients_name_idx ON patients (name);
CREATE INDEX patients_status_dob_idx ON patients (status, date_of_birth);

-- ❌ Bad: Over-indexing
CREATE INDEX patients_idx_1 ON patients (name);
CREATE INDEX patients_idx_2 ON patients (date_of_birth);
CREATE INDEX patients_idx_3 ON patients (status);
CREATE INDEX patients_idx_4 ON patients (created_at);
-- Too many indexes slow writes
```

### 3. Use Partial Indexes

```sql
-- ✅ Good: Index only active records
CREATE INDEX active_patients_idx
  ON patients (name)
  WHERE status = 'active';

-- Smaller index, faster queries for active patients
```

### 4. Rebuild from Log When Changing Schema

```sql
-- Schema change requires rebuild
DROP PROJECTION patients;
CREATE PROJECTION patients AS
  SELECT
    data->>'id' AS id,
    data->>'name' AS name,
    data->>'new_field' AS new_field  -- Added field
  FROM events
  WHERE tenant_id = 1;

-- Projection rebuilds from log automatically
```

## Related Documentation

- **[SQL Overview](overview.md)** - SQL architecture
- **[DML Reference](dml.md)** - INSERT/UPDATE/DELETE
- **[Query Reference](queries.md)** - SELECT syntax
- **[SQL Engine Design](..//docs/internals/design/sql-engine)** - Technical details

---

**Key Takeaway:** CREATE PROJECTION materializes views of the log. Projections can be rebuilt at any time. Index frequently queried columns for performance.
