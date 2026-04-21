---
title: "SQL Overview"
section: "reference/sql"
slug: "overview"
order: 1
---

# SQL Overview

Kimberlite speaks PostgreSQL-compatible SQL against its append-only log. Every
`INSERT`/`UPDATE`/`DELETE` appends an event; every `SELECT` reads the current
derived view, or a point-in-time snapshot via `AS OF TIMESTAMP` / `AT OFFSET`.

## Quick example

```sql
-- DDL
CREATE TABLE patients (
  id BIGINT PRIMARY KEY,
  name TEXT NOT NULL,
  dob TIMESTAMP,
  active BOOLEAN
);

-- DML (parameterized)
INSERT INTO patients (id, name, dob, active)
VALUES ($1, $2, $3, $4);

-- SELECT with JOIN, WHERE, GROUP BY, aggregates
SELECT p.name, COUNT(e.id) AS visits
FROM patients p
LEFT JOIN encounters e ON e.patient_id = p.id
WHERE p.active = true
GROUP BY p.name
HAVING COUNT(e.id) > 0
ORDER BY visits DESC
LIMIT 10;

-- Time-travel: state two weeks ago
SELECT * FROM patients AS OF TIMESTAMP '2024-01-15 10:30:00';
```

## Supported today (v0.4)

**DDL**

| Statement | Notes |
|---|---|
| `CREATE TABLE` | With `PRIMARY KEY`, `NOT NULL`, composite keys |
| `ALTER TABLE ... ADD COLUMN` / `DROP COLUMN` | |
| `DROP TABLE` | |
| `CREATE INDEX` | |

**DML**

| Statement | Notes |
|---|---|
| `INSERT` | Single-row and multi-row `VALUES (...), (...)`; `RETURNING` supported |
| `UPDATE` | With `WHERE`; `RETURNING` supported |
| `DELETE` | With `WHERE`; `RETURNING` supported |

**Queries**

| Feature | Notes |
|---|---|
| `SELECT` | Column projection or `*` |
| `WHERE` | `=`, `<`, `<=`, `>`, `>=`, `!=`, `AND`, `OR`, `NOT` |
| `IN`, `BETWEEN`, `LIKE` | `LIKE` uses iterative DP — ReDoS-safe |
| `CASE WHEN ... THEN ... ELSE ... END` | Searched form (not simple CASE) |
| `ORDER BY ... ASC` / `DESC` | |
| `LIMIT` / `OFFSET` | Literal or `$N` parameter (e.g. `LIMIT $2 OFFSET $3`) |
| `DISTINCT` | |
| `GROUP BY` + `HAVING` | |
| `COUNT`, `SUM`, `AVG`, `MIN`, `MAX` | `SUM` uses `checked_add`; `AVG` div-by-zero guarded |
| `UNION` / `UNION ALL` | |
| `INNER JOIN`, `LEFT JOIN` | Multi-table, including across subqueries |
| Subqueries | In `FROM`, `JOIN`, scalar position |
| CTEs (`WITH name AS (...)`) | Non-recursive |
| Window functions | `ROW_NUMBER`, `RANK`, `DENSE_RANK`, `LAG`, `LEAD`, `FIRST_VALUE`, `LAST_VALUE` with `PARTITION BY` / `ORDER BY` |
| Parameterized queries | `$1, $2, ...` (PostgreSQL-style) in `WHERE`, `LIMIT`, `OFFSET`, DML values |
| Point-in-time | `AT OFFSET n` (offset) and `AS OF TIMESTAMP '...'` / `FOR SYSTEM_TIME AS OF '...'` (wall-clock) |

**Resource limits** (configurable; defaults listed):

- Max JOIN output: 1,000,000 rows
- Max GROUP count: 100,000 groups

## Not supported in v0.4

| Feature | Status | Notes |
|---|---|---|
| Multi-statement transactions (`BEGIN`/`COMMIT`/`ROLLBACK`) | v1.0 | Single statements are atomic today. |
| `ALTER TABLE` (kernel execution) | Pending | Parser accepts `ADD COLUMN` / `DROP COLUMN`; kernel-side execution is the next gap. |
| `ON CONFLICT` / UPSERT | Planned (kernel work) | Requires deterministic conflict detection in the append-only log. |
| Correlated subqueries | Planned | Uncorrelated `IN (SELECT ...)` / `EXISTS` work today; correlated needs decorrelation or correlated-loop execution. |
| Scalar function projections (`UPPER`, `ROUND`, `EXTRACT`, `DATE_TRUNC`) | Planned | Needs a SELECT-projection scalar expression evaluator. |
| `ILIKE`, `NOT LIKE`, `NOT ILIKE` | Planned | Today only `LIKE` is supported. |
| `COALESCE` / `NULLIF` / `CAST` in WHERE | Planned | Same expression-evaluator gap as scalar projections. |
| Stored procedures, triggers, UDFs | Out of scope | Use application code + the append-only event API. |
| Extensions (`pg_crypto`, etc.) | Out of scope | |

Attempts to use unsupported syntax (e.g. `WITH RECURSIVE`) return a clean
error rather than silently misbehaving.

## Data types

| SQL type | Rust (wire) | Example |
|---|---|---|
| `BIGINT` | `i64` | `42`, `-1`, `9007199254740991` |
| `TEXT` | `String` (UTF-8) | `'Alice'`, `'Hello, 世界'` |
| `BOOLEAN` | `bool` | `true`, `false` |
| `TIMESTAMP` | `i64` nanoseconds since Unix epoch | `'2024-01-15 10:30:00'` |
| `NULL` | — | `NULL` |

> All numeric types compile to `BIGINT` on the wire today. `SMALLINT`, `INTEGER`,
> `REAL`, `DOUBLE PRECISION`, `DECIMAL`, `JSON`, `BYTEA` are not first-class
> types at the protocol layer yet — store encoded blobs in the append-only
> event API if you need them.

## Time-travel and audit

Every write is an immutable log entry. You can query state at any historical
point by offset or timestamp:

```sql
-- Current
SELECT * FROM patients WHERE id = 123;

-- At a specific log offset
SELECT * FROM patients AT OFFSET 4200 WHERE id = 123;

-- At a wall-clock time
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-15 10:30:00'
WHERE id = 123;
```

This replaces manual audit-table machinery: the audit trail *is* the primary
store.

## Multi-tenant isolation

Queries are automatically scoped to the tenant the client authenticated as.
Non-admin identities cannot observe rows from other tenants even if the SQL
would select them. See
[concepts/multitenancy](../../concepts/multitenancy.md).

## Related reference

- [DDL Reference](ddl.md) — `CREATE TABLE`, `ALTER TABLE`, `CREATE INDEX`
- [DML Reference](dml.md) — `INSERT`, `UPDATE`, `DELETE`
- [Query Reference](queries.md) — `SELECT`, joins, aggregates, time-travel
- [SQL Engine Design](../../internals/design/sql-engine.md) — internals

---

**Key takeaway:** Kimberlite SQL is a real PostgreSQL-compatible subset today —
INNER/LEFT/RIGHT/FULL/CROSS joins (with USING), aggregates with `FILTER (WHERE …)`,
non-recursive and recursive CTEs, IN/EXISTS subqueries, set operations
(UNION/INTERSECT/EXCEPT), JSON operators, parameterised queries (including
`LIMIT $N` / `OFFSET $N`), time-travel, and window functions all work.
Multi-statement transactions, scalar function projections, and `ALTER TABLE`
end-to-end execution are on the roadmap; attempts to use unsupported
syntax fail cleanly rather than silently misbehave.
