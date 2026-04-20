---
title: "SQL Reference"
section: "reference/sql"
slug: "README"
order: 0
---

# SQL Reference

PostgreSQL-compatible SQL for Kimberlite.

## Documentation

- **[Overview](overview.md)** — architecture, supported types, what works today
- **[DDL](ddl.md)** — `CREATE TABLE`, `ALTER TABLE`, `DROP TABLE`, `CREATE INDEX`
- **[DML](dml.md)** — `INSERT`, `UPDATE`, `DELETE` (with `RETURNING`, parameterised)
- **[Queries](queries.md)** — `SELECT`, joins, aggregates, CTEs, `CASE`, `LIKE`, time-travel
- **[JSON Operators](json.md)** — `->`, `->>`, `@>` for querying inside JSON columns

## Quick examples

### Create and populate a table

```sql
CREATE TABLE patients (
  id BIGINT NOT NULL PRIMARY KEY,
  name TEXT NOT NULL,
  dob TIMESTAMP,
  active BOOLEAN
);

INSERT INTO patients (id, name, dob, active)
VALUES
  (1, 'Jane Doe', '1985-03-15 00:00:00', true),
  (2, 'John Smith', '1972-08-22 00:00:00', true);
```

### Query with `JOIN`, `GROUP BY`, `HAVING`

```sql
SELECT p.name, COUNT(e.id) AS visits
FROM patients p
LEFT JOIN encounters e ON e.patient_id = p.id
WHERE p.active = true
GROUP BY p.name
HAVING COUNT(e.id) > 0
ORDER BY visits DESC
LIMIT 10;
```

### Time-travel query

```sql
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-15 10:30:00'
WHERE id = 1;
```

### Parameterised

```sql
SELECT * FROM patients WHERE id = $1 AND active = $2;
```

## Feature status (v0.4)

| Feature | Status |
|---|---|
| `SELECT` w/ `WHERE` / `ORDER BY` / `DISTINCT` | ✅ |
| `LIMIT` / `OFFSET` (literal or `$N`) | ✅ |
| `INNER` / `LEFT` / `RIGHT` / `FULL OUTER` / `CROSS` JOIN | ✅ |
| `JOIN ... USING(col, ...)` | ✅ |
| Aggregates (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`) | ✅ |
| Aggregate `FILTER (WHERE ...)` | ✅ |
| `GROUP BY` + `HAVING` | ✅ |
| `UNION` / `UNION ALL` / `INTERSECT` / `INTERSECT ALL` / `EXCEPT` / `EXCEPT ALL` | ✅ |
| Subqueries (FROM, JOIN, scalar) | ✅ |
| `IN (SELECT ...)`, `EXISTS`, `NOT EXISTS` (uncorrelated) | ✅ |
| Correlated subqueries | 📅 Planned |
| CTEs (`WITH`, non-recursive) | ✅ |
| `WITH RECURSIVE` (iterative fixed-point, depth cap 1000) | ✅ |
| Window functions (`OVER`, `PARTITION BY`, `ROW_NUMBER`, `RANK`, `LAG`, …) | ✅ |
| `CASE WHEN` (searched and simple) | ✅ |
| JSON operators (`->`, `->>`, `@>`) | ✅ |
| `BETWEEN`, `LIKE`, `IN`, `IS NULL` | ✅ |
| `CREATE TABLE`, `DROP TABLE`, `CREATE INDEX` | ✅ |
| `ALTER TABLE` | ⚠️ parser-only; kernel execution pending |
| `INSERT` / `UPDATE` / `DELETE` with `RETURNING` | ✅ |
| Parameterised queries (`$1, $2, ...`) — `WHERE`, `LIMIT`, `OFFSET`, DML values | ✅ |
| `AS OF TIMESTAMP`, `AT OFFSET` (time-travel) | ✅ |
| Multi-statement transactions | 📅 Planned v1.0 |
| `ON CONFLICT` / UPSERT | 📅 Planned (kernel work) |
| Scalar function projections (`UPPER`, `ROUND`, `EXTRACT`, ...) | 📅 Planned |
| `ILIKE`, `NOT LIKE`, `NOT ILIKE` | 📅 Planned |
| `COALESCE` / `NULLIF` / `CAST` in WHERE | 📅 Planned |

Unsupported syntax fails at parse time with a clear error, not silently.
