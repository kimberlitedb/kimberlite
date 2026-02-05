# SQL Reference

Comprehensive SQL reference for Kimberlite.

**Status:** Core features planned for v0.6.0, advanced features in v0.7-0.9

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

## Feature Roadmap

| Feature | Version | Status |
|---------|---------|--------|
| CREATE PROJECTION | v0.6.0 | Planned |
| SELECT with WHERE | v0.6.0 | Planned |
| JOINs (INNER, LEFT) | v0.6.0 | Planned |
| Aggregates | v0.6.0 | Planned |
| CREATE INDEX | v0.6.0 | Planned |
| Subqueries | v0.7.0 | Planned |
| CTEs (WITH) | v0.7.0 | Planned |
| Window functions | v0.7.0 | Planned |
| INSERT/UPDATE/DELETE | v0.8.0 | Planned |
| Transactions | v0.9.0 | Planned |

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
