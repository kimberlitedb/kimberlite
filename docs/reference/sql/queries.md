---
title: "SQL Query Reference"
section: "reference/sql"
slug: "queries"
order: 4
---

# SQL Query Reference

Everything `SELECT`-shaped: joins, aggregates, CTEs, `CASE`, `LIKE`, and
time-travel.

## SELECT

### Syntax

```sql
SELECT [DISTINCT] select_list
  FROM from_list
  [WHERE condition]
  [GROUP BY expression [, ...]]
  [HAVING condition]
  [ORDER BY expression [ASC | DESC] [, ...]]
  [LIMIT n [OFFSET m]]
  [AS OF TIMESTAMP 'YYYY-MM-DD HH:MM:SS' | AT OFFSET n];
```

### Basic projection

```sql
SELECT * FROM patients;
SELECT id, name FROM patients;
SELECT id, name AS patient_name FROM patients;
SELECT DISTINCT specialty FROM providers;
```

## WHERE

Operators: `=`, `!=`, `<`, `<=`, `>`, `>=`, plus `AND`, `OR`, `NOT`.

```sql
SELECT * FROM patients WHERE id = 1;
SELECT * FROM patients WHERE active = true AND dob < '1980-01-01 00:00:00';
SELECT * FROM patients WHERE NOT active;
```

### `IN`

```sql
SELECT * FROM patients WHERE id IN (1, 2, 3);
```

### `BETWEEN`

Desugars internally to `>= AND <=`:

```sql
SELECT *
FROM encounters
WHERE encounter_date BETWEEN '2024-01-01 00:00:00' AND '2024-12-31 23:59:59';
```

### `LIKE`

Pattern matching with `%` (zero or more characters) and `_` (single
character). Implemented iteratively so there is no exponential-backtracking
vulnerability.

```sql
SELECT * FROM patients WHERE name LIKE 'J%';
SELECT * FROM patients WHERE name LIKE '_ane%';
```

### `IS NULL` / `IS NOT NULL`

```sql
SELECT * FROM patients WHERE email IS NULL;
SELECT * FROM patients WHERE email IS NOT NULL;
```

### Parameterized

Use `$1, $2, ...`:

```sql
SELECT * FROM patients WHERE id = $1 AND active = $2;
```

## CASE WHEN

Searched form (conditions per branch):

```sql
SELECT
  id,
  name,
  CASE
    WHEN dob < '1960-01-01 00:00:00' THEN 'senior'
    WHEN dob < '1990-01-01 00:00:00' THEN 'adult'
    ELSE 'young'
  END AS age_bucket
FROM patients;
```

Simple `CASE` (single expression with `WHEN value THEN ...`) is not supported
— rewrite as the searched form above.

## ORDER BY / LIMIT / OFFSET

```sql
SELECT * FROM patients
ORDER BY name ASC
LIMIT 20 OFFSET 40;

SELECT id, name FROM patients ORDER BY dob DESC, name ASC;
```

## Aggregates

Supported: `COUNT(*)`, `COUNT(col)`, `SUM`, `AVG`, `MIN`, `MAX`.

- `SUM` uses checked addition (overflow → error, not silent wrap).
- `AVG` guards division-by-zero.

```sql
SELECT
  provider_id,
  COUNT(*) AS visits,
  AVG(duration_minutes) AS avg_duration
FROM encounters
GROUP BY provider_id;
```

### HAVING

Filters after `GROUP BY`:

```sql
SELECT provider_id, COUNT(*) AS visits
FROM encounters
GROUP BY provider_id
HAVING COUNT(*) > 100;
```

## JOIN

Supported: `INNER JOIN` (default `JOIN`) and `LEFT JOIN`. Multi-table joins
are applied via the `plan_join_query` + `Materialize` wrapper, so `WHERE`,
`ORDER BY`, `LIMIT`, and `CASE WHEN` over the joined result all work.

```sql
SELECT p.name, e.encounter_date, pr.name AS provider
FROM patients p
JOIN encounters e ON e.patient_id = p.id
JOIN providers  pr ON pr.id = e.provider_id
WHERE p.active = true
ORDER BY e.encounter_date DESC
LIMIT 50;
```

`LEFT JOIN` keeps rows from the left side even if there is no match:

```sql
SELECT p.name, COUNT(e.id) AS encounter_count
FROM patients p
LEFT JOIN encounters e ON e.patient_id = p.id
GROUP BY p.name
ORDER BY encounter_count DESC;
```

`RIGHT JOIN` and `FULL OUTER JOIN` are not supported; swap the sides and
use `LEFT JOIN`.

## Subqueries

Allowed in the `FROM` clause, inside `JOIN`, or in scalar positions:

```sql
-- Scalar subquery
SELECT name,
       (SELECT COUNT(*) FROM encounters e WHERE e.patient_id = p.id) AS visits
FROM patients p;

-- Derived table
SELECT dept, AVG(salary) AS avg_salary
FROM (
  SELECT dept, salary FROM employees WHERE active = true
) sub
GROUP BY dept;
```

## Common Table Expressions (CTEs)

Non-recursive `WITH` clauses work today:

```sql
WITH recent_encounters AS (
  SELECT patient_id, COUNT(*) AS cnt
  FROM encounters
  WHERE encounter_date > '2024-01-01 00:00:00'
  GROUP BY patient_id
)
SELECT p.name, r.cnt
FROM patients p
JOIN recent_encounters r ON r.patient_id = p.id
WHERE r.cnt > 2
ORDER BY r.cnt DESC;
```

`WITH RECURSIVE` is deliberately rejected — bounded recursion is a
correctness concern under the functional-core/imperative-shell pattern.

## UNION / UNION ALL

```sql
SELECT id, name FROM patients WHERE active = true
UNION
SELECT id, name FROM archived_patients;

SELECT id FROM stream_a
UNION ALL
SELECT id FROM stream_b;
```

`UNION ALL` keeps duplicates; `UNION` de-dupes.

## Time-travel

Every write is an immutable log entry, so historical state is first-class.

### AS OF TIMESTAMP

```sql
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-15 10:30:00'
WHERE id = 123;
```

### AT OFFSET

When you captured a log offset programmatically:

```sql
SELECT * FROM patients AT OFFSET 4200 WHERE id = 123;
```

Combine with joins for audit queries:

```sql
SELECT p.name, e.encounter_date
FROM patients p
JOIN encounters e ON e.patient_id = p.id
AS OF TIMESTAMP '2024-01-15 10:30:00';
```

The SDK-side equivalents are `client.queryAt(sql, params, offset)` (TypeScript,
Rust, Python).

## What is not supported (v0.4)

| Feature | Alternative |
|---|---|
| Window functions (`OVER`, `PARTITION BY`, `ROW_NUMBER`, `RANK`, `LAG`, …) | Rewrite with `GROUP BY` + self-join, or compute application-side. Planned v0.5.0. |
| `WITH RECURSIVE` | Bounded iteration application-side. Deliberate omission. |
| Multi-statement transactions (`BEGIN` / `COMMIT` / `ROLLBACK`) | Single statements are atomic. v1.0. |
| `RIGHT JOIN`, `FULL OUTER JOIN` | Swap sides, use `LEFT JOIN`. |
| Simple `CASE` (`CASE x WHEN 1 THEN ...`) | Use searched `CASE WHEN x = 1 THEN ...`. |
| `CAST(... AS ...)`, `EXTRACT(... FROM ...)` | Compute application-side. |
| `ARRAY`, `JSON`, `JSONB` types | Store via the append-only stream API. |

Unsupported syntax fails cleanly at parse time.

## Resource limits

| Limit | Default |
|---|---|
| Max JOIN output rows | 1,000,000 |
| Max GROUP count | 100,000 |

## Related

- [DDL Reference](ddl.md) — table creation
- [DML Reference](dml.md) — `INSERT`, `UPDATE`, `DELETE`
- [SQL Overview](overview.md) — architecture, supported types
