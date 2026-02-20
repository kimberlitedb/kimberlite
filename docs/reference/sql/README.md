---
title: "SQL Reference"
section: "reference/sql"
slug: "README"
order: 0
---

# SQL Reference

Comprehensive SQL reference for Kimberlite.

**Status:** Core SQL features implemented (SELECT, JOINs, GROUP BY, HAVING, UNION, DML, DDL). Subqueries and CTEs in progress.

## Documentation

- **[Overview](overview.md)** - SQL architecture and capabilities
- **[DDL](ddl.md)** - CREATE/DROP PROJECTION, INDEX
- **[DML](dml.md)** - INSERT/UPDATE/DELETE
- **[Queries](queries.md)** - SELECT syntax and patterns

## Quick Examples

### Create Projection

```sql
CREATE PROJECTION patients AS
  SELECT
    data->>'id' AS id,
    data->>'name' AS name,
    data->>'dob' AS date_of_birth
  FROM events
  WHERE tenant_id = 1
    AND stream_id = 100;
```

### Query Data

```sql
SELECT name, date_of_birth
FROM patients
WHERE name LIKE 'Alice%'
ORDER BY name
LIMIT 10;
```

### Time-Travel Query

```sql
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-15 10:30:00'
WHERE id = 123;
```

### Insert Data

```sql
INSERT INTO patients (id, name, date_of_birth)
VALUES (123, 'Alice Johnson', '1985-03-15');
```

## Feature Status

| Feature | Status |
|---------|--------|
| SELECT with WHERE, ORDER BY, LIMIT | ✅ Implemented |
| JOINs (INNER, LEFT) | ✅ Implemented |
| Aggregates (COUNT, SUM, AVG, MIN, MAX) | ✅ Implemented |
| GROUP BY + HAVING | ✅ Implemented |
| UNION / UNION ALL | ✅ Implemented |
| ALTER TABLE (ADD/DROP COLUMN) | ✅ Implemented |
| CREATE TABLE / DROP TABLE | ✅ Implemented |
| CREATE INDEX | ✅ Implemented |
| INSERT / UPDATE / DELETE | ✅ Implemented |
| Subqueries | 🚧 In Progress |
| CTEs (WITH) | 🚧 In Progress |
| Window functions | 📅 Planned |
| Transactions | 📅 Planned |

## Current Alternative

While SQL engine is in development, use the Event API:

```rust
use kimberlite::Client;

// Append events
client.append(tenant_id, stream_id, event_data)?;

// Read events
let events = client.read_stream(tenant_id, stream_id)?;
```

See [Coding Guides](../../coding/) for language-specific examples.
