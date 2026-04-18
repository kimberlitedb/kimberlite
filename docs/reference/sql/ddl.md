---
title: "SQL DDL Reference"
section: "reference/sql"
slug: "ddl"
order: 2
---

# SQL DDL Reference

Data Definition Language: create and modify table schemas and indexes. Every
DDL statement appends an event to the immutable log, so schema history is
preserved and can be reconstructed via time-travel queries.

## CREATE TABLE

### Syntax

```sql
CREATE TABLE [IF NOT EXISTS] table_name (
  column_name data_type [NOT NULL] [PRIMARY KEY],
  ...
  [PRIMARY KEY (column, ...)]
);
```

### Supported types

| SQL type | Wire representation | Notes |
|---|---|---|
| `BIGINT` | `i64` | All integers map here |
| `TEXT` | UTF-8 string | |
| `BOOLEAN` | `bool` | |
| `TIMESTAMP` | `i64` nanoseconds since Unix epoch | Accepts `'YYYY-MM-DD HH:MM:SS'` literals |

### Examples

**Simple table:**

```sql
CREATE TABLE patients (
  id BIGINT NOT NULL PRIMARY KEY,
  name TEXT NOT NULL,
  dob TIMESTAMP,
  active BOOLEAN
);
```

**Composite primary key:**

```sql
CREATE TABLE enrollments (
  patient_id BIGINT NOT NULL,
  provider_id BIGINT NOT NULL,
  enrolled_at TIMESTAMP NOT NULL,
  PRIMARY KEY (patient_id, provider_id)
);
```

**Guarded by `IF NOT EXISTS`:**

```sql
CREATE TABLE IF NOT EXISTS audit_log (
  id BIGINT NOT NULL PRIMARY KEY,
  event_at TIMESTAMP NOT NULL,
  actor TEXT NOT NULL,
  action TEXT NOT NULL
);
```

## ALTER TABLE

### ADD COLUMN

```sql
ALTER TABLE patients
  ADD COLUMN email TEXT;
```

Existing rows get `NULL` for the new column. `NOT NULL` without a default is
rejected; provide the default via a follow-up `UPDATE`.

### DROP COLUMN

```sql
ALTER TABLE patients
  DROP COLUMN email;
```

The column is removed from the current state view; historical data remains in
the append-only log and is still visible via `AS OF` queries that predate the
`DROP`.

## DROP TABLE

```sql
DROP TABLE [IF EXISTS] table_name;
```

Example:

```sql
DROP TABLE IF EXISTS temp_staging;
```

Dropping a table removes it from the current state view but does not delete
the underlying log entries; the schema and data are still reachable via
time-travel queries that predate the `DROP`.

## CREATE INDEX

```sql
CREATE INDEX [IF NOT EXISTS] index_name
  ON table_name (column [, column ...]);
```

Examples:

```sql
CREATE INDEX patients_name_idx ON patients (name);

CREATE INDEX encounters_patient_date_idx
  ON encounters (patient_id, encounter_date);
```

Indexes accelerate equality and range lookups. They are rebuilt automatically
from the log when a replica catches up.

## What is not supported (v0.4)

| Feature | Notes |
|---|---|
| `SMALLINT`, `INTEGER`, `REAL`, `DOUBLE PRECISION`, `DECIMAL` | Use `BIGINT` — numeric widening is deliberate. |
| `JSON`, `JSONB` | Store blobs via the append-only stream API instead. |
| `BYTEA` / binary columns | Same as above. |
| `CREATE VIEW`, `CREATE MATERIALIZED VIEW` | The log *is* the materialised source; derived state lives in the projection store. |
| Foreign keys (`REFERENCES`) | Enforce referential integrity in application code. |
| `DEFAULT <expr>` on columns | Provide values explicitly at `INSERT`. |
| `UNIQUE` constraint (besides PRIMARY KEY) | Enforce in application code for now. |
| Stored procedures / triggers / UDFs | Out of scope. |

Attempts to use unsupported syntax return a parser error rather than silently
accepting invalid state.

## Related

- [DML Reference](dml.md) — `INSERT`, `UPDATE`, `DELETE`
- [Query Reference](queries.md) — `SELECT`, joins, time-travel
- [SQL Overview](overview.md)
