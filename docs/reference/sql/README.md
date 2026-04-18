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
| `SELECT` w/ `WHERE` / `ORDER BY` / `LIMIT` / `OFFSET` / `DISTINCT` | ✅ |
| `INNER JOIN`, `LEFT JOIN` (multi-table) | ✅ |
| Aggregates (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`) | ✅ |
| `GROUP BY` + `HAVING` | ✅ |
| `UNION` / `UNION ALL` | ✅ |
| Subqueries (FROM, JOIN, scalar) | ✅ |
| CTEs (`WITH`, non-recursive) | ✅ |
| `CASE WHEN` (searched) | ✅ |
| `BETWEEN`, `LIKE`, `IN`, `IS NULL` | ✅ |
| `CREATE` / `ALTER` / `DROP TABLE`, `CREATE INDEX` | ✅ |
| `INSERT` / `UPDATE` / `DELETE` with `RETURNING` | ✅ |
| Parameterised queries (`$1, $2, ...`) | ✅ |
| `AS OF TIMESTAMP`, `AT OFFSET` (time-travel) | ✅ |
| Window functions (`OVER`, `PARTITION BY`) | 📅 Planned v0.5.0 |
| `WITH RECURSIVE` | ❌ Deliberately rejected |
| Multi-statement transactions | 📅 Planned v1.0 |
| `RIGHT JOIN`, `FULL OUTER JOIN` | 📅 Planned |

Unsupported syntax fails at parse time with a clear error, not silently.
