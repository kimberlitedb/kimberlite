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
  [AT OFFSET n];
```

> `AS OF TIMESTAMP '...'` is planned for v0.6.0 but not currently
> implemented — see [Time-travel](#time-travel) below.

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

### `LIKE` / `NOT LIKE` / `ILIKE` / `NOT ILIKE`

Pattern matching with `%` (zero or more characters) and `_` (single
character). Implemented iteratively so there is no exponential-backtracking
vulnerability.

`LIKE` is case-sensitive; `ILIKE` folds both the value and the pattern to
Unicode simple lowercase before matching. `NOT LIKE` / `NOT ILIKE` invert
the result and follow the same three-valued logic (a non-text cell matches
neither, consistent with `LIKE`).

```sql
SELECT * FROM patients WHERE name LIKE 'J%';
SELECT * FROM patients WHERE name LIKE '_ane%';
SELECT * FROM patients WHERE name NOT LIKE 'Test%';
SELECT * FROM patients WHERE name ILIKE 'jane%';       -- matches Jane, JANE, jAnE
SELECT * FROM patients WHERE email NOT ILIKE '%@test.com';
```

All four variants are available as of v0.5.0; v0.4.x and earlier rejected
`NOT LIKE` and did not parse `ILIKE`.

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

### Pagination with parameters

`LIMIT` and `OFFSET` accept either an integer literal or a `$N` parameter
placeholder. Bind a `BIGINT` (`Value::BigInt` from the SDK) at execution
time. Negative or non-integer bound values are rejected with a clear error.

```sql
-- Server-side pagination from any SDK
SELECT id, patient_id, status, updated_at
FROM clinical_note
WHERE patient_id = $1
ORDER BY updated_at DESC, id DESC
LIMIT $2;

-- Cursor-style pagination
SELECT id, updated_at
FROM clinical_note
WHERE patient_id = $1
  AND (updated_at < $2 OR (updated_at = $2 AND id < $3))
ORDER BY updated_at DESC, id DESC
LIMIT $4;
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

### Filtered aggregates (`FILTER (WHERE ...)`)

Per-aggregate row filters let one SELECT compute multiple
counts/sums/averages over different slices of the same group:

```sql
-- Clinical dashboard: total encounters and abnormal-result count per provider
SELECT
  provider_id,
  COUNT(*) AS total_encounters,
  COUNT(*) FILTER (WHERE result = 'abnormal') AS abnormal_count,
  AVG(duration_minutes) FILTER (WHERE encounter_type = 'urgent') AS avg_urgent_duration
FROM encounters
GROUP BY provider_id;
```

Each aggregate's `FILTER` is independent. `FILTER` is supported on
`COUNT(*)`, `COUNT(col)`, `SUM`, `AVG`, `MIN`, `MAX`.

## JOIN

Supported: `INNER JOIN`, `LEFT JOIN`, `RIGHT JOIN`, `FULL OUTER JOIN`, and
`CROSS JOIN`. Multi-table joins are applied via the `plan_join_query` +
`Materialize` wrapper, so `WHERE`, `ORDER BY`, `LIMIT`, and `CASE WHEN`
over the joined result all work.

The `USING(col1, col2, ...)` shorthand expands to
`ON left.colN = right.colN AND ...`. `CROSS JOIN` produces a Cartesian
product and is capped at 1,000,000 output rows to prevent runaway queries.

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

### `IN (SELECT ...)`, `EXISTS`, `NOT EXISTS`

Uncorrelated subquery predicates are pre-executed at query entry and
substituted into the outer query, so the subquery runs exactly once
regardless of outer cardinality:

```sql
-- Active patients with at least one encounter
SELECT id, name FROM patients
WHERE id IN (SELECT patient_id FROM encounters);

-- Patients with no recorded encounters
SELECT id, name FROM patients
WHERE NOT EXISTS (
  SELECT 1 FROM encounters WHERE encounters.patient_id = patients.id
);
```

Correlated subqueries (where the inner query references outer columns)
are not yet supported — rewrite them as a CTE join.

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

### Recursive CTEs

`WITH RECURSIVE` is supported via iterative fixed-point evaluation. The
anchor SELECT seeds the working set; the recursive arm runs against the
accumulating result until no new rows are produced or the iteration cap
(default 1,000) is hit. This pattern honours the workspace "no recursion"
lint and prevents runaway queries.

```sql
WITH RECURSIVE descendants AS (
  SELECT id, manager_id FROM employees WHERE id = 1     -- anchor
  UNION ALL
  SELECT e.id, e.manager_id                              -- recursive
  FROM employees e
  WHERE e.manager_id IN (SELECT id FROM descendants)
)
SELECT * FROM descendants;
```

Hitting the iteration cap returns a clear error rather than silently
truncating.

## Set operations

```sql
-- UNION (de-dupes) and UNION ALL (keeps duplicates)
SELECT id, name FROM patients WHERE active = true
UNION
SELECT id, name FROM archived_patients;

SELECT id FROM stream_a
UNION ALL
SELECT id FROM stream_b;

-- INTERSECT and EXCEPT also work with both bare and ALL forms
SELECT id FROM patients_2024 INTERSECT SELECT id FROM patients_2025;
SELECT id FROM all_users EXCEPT SELECT id FROM banned_users;
SELECT id FROM ledger_a INTERSECT ALL SELECT id FROM ledger_b;
SELECT id FROM ledger_a EXCEPT ALL SELECT id FROM ledger_b;
```

`UNION` / `INTERSECT` / `EXCEPT` deduplicate by row content. The `ALL`
variants preserve multiset semantics — useful for compliance reconciliation
where row multiplicities matter.

## Time-travel

Every write is an immutable log entry, so historical state is first-class.

### AT OFFSET (supported)

When you captured a log offset programmatically:

```sql
SELECT * FROM patients AT OFFSET 4200 WHERE id = 123;
```

The SDK-side equivalent is `client.queryAt(sql, params, offset)` (TypeScript,
Rust, Python).

### AS OF TIMESTAMP (not yet implemented)

> **Audit reference:** AUDIT-2026-04 L-4.

Timestamp-indexed time-travel is planned for v0.6.0 but **not currently
implemented**. The query parser recognises the `AS OF TIMESTAMP` grammar in
the syntax summary at the top of this page — callers that pass it today
will receive an `UnsupportedFeature` error, not a silently-ignored
clause. There is no timestamp-to-offset resolver in `kimberlite-query::executor`.

Until the v0.6.0 implementation lands, capture a log offset at the
point in time you need and issue `AT OFFSET n` (or the
`PreparedQuery::execute_at(offset)` API on the SDK). Common patterns:

- Snapshot the `log_offset` returned by every write response, persist it
  alongside the business record, and use it later as an audit anchor.
- For periodic checkpoints, run `SELECT current_offset()` on a cron and
  store `(wall_clock_timestamp, log_offset)` in a side table — this is
  the same shape the v0.6.0 resolver will expose.

The v1.0 compliance-query surface (`"what was the patient record on
2026-01-15?"`) will front-end this via the SDK once the resolver lands.
Until then, compliance queries must supply the offset explicitly.

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
