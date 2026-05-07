---
title: "SQL DML Reference"
section: "reference/sql"
slug: "dml"
order: 3
---

# SQL DML Reference

Data Manipulation Language: insert, update, delete. Each statement appends an
event to the append-only log — you can always reconstruct prior state with
[time-travel queries](queries.md#time-travel).

## INSERT

### Syntax

```sql
INSERT INTO table_name (column, ...)
  VALUES (value, ...) [, (value, ...) ...]
  [RETURNING column | *];
```

### Single row

```sql
INSERT INTO patients (id, name, dob, active)
  VALUES (1, 'Jane Doe', '1985-03-15 00:00:00', true);
```

### Multiple rows

```sql
INSERT INTO patients (id, name, dob)
  VALUES
    (2, 'John Smith',  '1972-08-22 00:00:00'),
    (3, 'Alice Johnson', '1990-11-05 00:00:00'),
    (4, 'Bob Williams', '1988-04-17 00:00:00');
```

### Parameterized (`$1, $2, ...`)

Use PostgreSQL-style positional placeholders, not `?`:

```sql
INSERT INTO patients (id, name, dob, active)
  VALUES ($1, $2, $3, $4);
```

The client SDKs (`ValueBuilder` in TypeScript, typed bindings in Python/Rust)
wrap values with their SQL type.

### RETURNING

```sql
INSERT INTO patients (id, name)
  VALUES (5, 'Carol White')
  RETURNING id, name;
```

Returns the inserted row(s). Use `RETURNING *` for all columns.

## UPDATE

### Syntax

```sql
UPDATE table_name
  SET column = value [, column = value ...]
  [WHERE condition]
  [RETURNING column | *];
```

### Examples

```sql
-- Single row
UPDATE patients
  SET active = false
  WHERE id = 1;

-- Multiple columns
UPDATE patients
  SET name = $1, dob = $2
  WHERE id = $3;

-- With RETURNING
UPDATE patients
  SET active = false
  WHERE dob < '1950-01-01 00:00:00'
  RETURNING id, name;
```

A missing `WHERE` clause updates every row in the table — the parser does not
require it, so be explicit.

## DELETE

### Syntax

```sql
DELETE FROM table_name
  [WHERE condition]
  [RETURNING column | *];
```

### Examples

```sql
-- Targeted
DELETE FROM patients WHERE id = 1;

-- With RETURNING (useful for audit)
DELETE FROM patients
  WHERE active = false
  RETURNING id, name;
```

A `DELETE` removes the row from the current state view but the log entry that
created it remains. You can still see the row via an `AS OF TIMESTAMP` /
`AT OFFSET` query predating the deletion. This is the foundation of
[right-to-erasure](../../concepts/data-portability.md): explicit erasure is a
separate flow that tombstones the source event.

## NULL handling

- `NULL` is a literal: `INSERT INTO t (id, name) VALUES (1, NULL);`
- `col IS NULL` / `col IS NOT NULL` in `WHERE` clauses.
- Comparisons with `NULL` yield `NULL` (three-valued logic).

```sql
UPDATE patients SET email = NULL WHERE id = 1;
SELECT * FROM patients WHERE email IS NULL;
```

## Atomicity

Each single statement is atomic. Multi-statement transactions with
`BEGIN`/`COMMIT`/`ROLLBACK` are planned **post-v1.0** — they intentionally
sit behind the v1.0 checklist gates so the immutable-log + derived-view
contract stays clean for the initial OSS release. The parser currently
rejects them. Until then, design workflows so each step is idempotent;
see the outbox-pattern recipe (cookbook) for cross-table coordination.

## Resource limits

| Limit | Default | Notes |
|---|---|---|
| Max JOIN output rows | 1,000,000 | Affects `INSERT ... SELECT` with joins. |
| Max GROUP count | 100,000 | Affects aggregate rewrites. |

Queries that exceed these return a clean error rather than running unbounded.

## Related

- [Query Reference](queries.md) — `SELECT`, joins, time-travel
- [DDL Reference](ddl.md) — table creation
- [SQL Overview](overview.md)
