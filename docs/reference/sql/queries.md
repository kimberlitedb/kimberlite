# SQL Query Reference

SELECT syntax and query patterns.

**Status:** Core features in v0.6.0, advanced features in v0.7.0

## SELECT

Query data from projections.

### Basic Syntax

```sql
SELECT column, ...
FROM projection_name
[WHERE condition]
[ORDER BY column [ASC|DESC], ...]
[LIMIT count]
[OFFSET skip];
```

### Examples

**Simple query:**

```sql
SELECT id, name, date_of_birth
FROM patients
WHERE status = 'active';
```

**All columns:**

```sql
SELECT * FROM patients;
```

**Limit results:**

```sql
SELECT name FROM patients
ORDER BY name
LIMIT 10;
```

**Pagination:**

```sql
SELECT name FROM patients
ORDER BY name
LIMIT 10 OFFSET 20;  -- Page 3 (rows 21-30)
```

## WHERE Clause

Filter rows based on conditions.

### Comparison Operators

| Operator | Example | Description |
|----------|---------|-------------|
| `=` | `status = 'active'` | Equals |
| `!=`, `<>` | `status != 'inactive'` | Not equals |
| `<` | `age < 18` | Less than |
| `<=` | `age <= 65` | Less than or equal |
| `>` | `age > 18` | Greater than |
| `>=` | `age >= 65` | Greater than or equal |
| `BETWEEN` | `age BETWEEN 18 AND 65` | Range (inclusive) |
| `IN` | `status IN ('active', 'pending')` | In list |
| `LIKE` | `name LIKE 'Alice%'` | Pattern match |
| `IS NULL` | `email IS NULL` | Null check |
| `IS NOT NULL` | `email IS NOT NULL` | Not null check |

### Logical Operators

```sql
-- AND
SELECT * FROM patients
WHERE status = 'active' AND age > 18;

-- OR
SELECT * FROM patients
WHERE status = 'active' OR status = 'pending';

-- NOT
SELECT * FROM patients
WHERE NOT (status = 'inactive');

-- Precedence (AND before OR)
SELECT * FROM patients
WHERE status = 'active' AND (age < 18 OR age > 65);
```

### Pattern Matching

```sql
-- LIKE (case-sensitive)
SELECT * FROM patients WHERE name LIKE 'Alice%';  -- Starts with
SELECT * FROM patients WHERE name LIKE '%Smith';  -- Ends with
SELECT * FROM patients WHERE name LIKE '%John%';  -- Contains

-- ILIKE (case-insensitive, PostgreSQL extension)
SELECT * FROM patients WHERE name ILIKE 'alice%';

-- Escape special characters
SELECT * FROM patients WHERE name LIKE '50\%';  -- Literal '%'
```

### NULL Handling

```sql
-- IS NULL
SELECT * FROM patients WHERE email IS NULL;

-- IS NOT NULL
SELECT * FROM patients WHERE email IS NOT NULL;

-- COALESCE (default value)
SELECT COALESCE(email, 'no-email@example.com') FROM patients;

-- NULLIF (convert value to NULL)
SELECT NULLIF(status, '') FROM patients;  -- Empty string → NULL
```

## ORDER BY

Sort results.

### Syntax

```sql
SELECT * FROM patients
ORDER BY column [ASC|DESC] [NULLS FIRST|NULLS LAST], ...;
```

### Examples

**Ascending (default):**

```sql
SELECT * FROM patients
ORDER BY name;  -- A-Z
```

**Descending:**

```sql
SELECT * FROM patients
ORDER BY date_of_birth DESC;  -- Newest first
```

**Multiple columns:**

```sql
SELECT * FROM patients
ORDER BY status, name;  -- Status then name
```

**NULL handling:**

```sql
SELECT * FROM patients
ORDER BY email NULLS LAST;  -- NULLs at end
```

## Aggregates

Summarize data across rows.

### Aggregate Functions

| Function | Description | Example |
|----------|-------------|---------|
| `COUNT(*)` | Count rows | `SELECT COUNT(*) FROM patients` |
| `COUNT(column)` | Count non-null values | `SELECT COUNT(email) FROM patients` |
| `SUM(column)` | Sum numeric values | `SELECT SUM(amount) FROM payments` |
| `AVG(column)` | Average | `SELECT AVG(age) FROM patients` |
| `MIN(column)` | Minimum | `SELECT MIN(date_of_birth) FROM patients` |
| `MAX(column)` | Maximum | `SELECT MAX(date_of_birth) FROM patients` |

### Examples

**Count rows:**

```sql
SELECT COUNT(*) FROM patients;
SELECT COUNT(*) FROM patients WHERE status = 'active';
```

**Average:**

```sql
SELECT AVG(age) FROM patients;
```

**Min/Max:**

```sql
SELECT
  MIN(date_of_birth) AS oldest,
  MAX(date_of_birth) AS youngest
FROM patients;
```

## GROUP BY

Group rows and aggregate.

### Syntax

```sql
SELECT column, aggregate_function(column)
FROM projection_name
[WHERE condition]
GROUP BY column, ...
[HAVING aggregate_condition]
[ORDER BY column];
```

### Examples

**Count by status:**

```sql
SELECT status, COUNT(*) AS patient_count
FROM patients
GROUP BY status;
```

**Output:**
```
 status   | patient_count
----------+--------------
 active   | 1250
 inactive | 340
 pending  | 45
```

**Average by status:**

```sql
SELECT status, AVG(age) AS avg_age
FROM patients
GROUP BY status
ORDER BY avg_age DESC;
```

**Multiple grouping columns:**

```sql
SELECT status, EXTRACT(YEAR FROM date_of_birth) AS birth_year, COUNT(*)
FROM patients
GROUP BY status, EXTRACT(YEAR FROM date_of_birth);
```

### HAVING

Filter groups (post-aggregation):

```sql
-- Find statuses with >100 patients
SELECT status, COUNT(*) AS count
FROM patients
GROUP BY status
HAVING COUNT(*) > 100;

-- Average age >50
SELECT status, AVG(age) AS avg_age
FROM patients
GROUP BY status
HAVING AVG(age) > 50;
```

**WHERE vs HAVING:**
- `WHERE` filters rows before grouping
- `HAVING` filters groups after aggregation

```sql
SELECT status, COUNT(*) AS count
FROM patients
WHERE age > 18           -- Filter rows first
GROUP BY status
HAVING COUNT(*) > 100;   -- Then filter groups
```

## DISTINCT

Remove duplicate rows.

### Syntax

```sql
SELECT DISTINCT column, ...
FROM projection_name;
```

### Examples

**Unique values:**

```sql
SELECT DISTINCT status FROM patients;
```

**Output:**
```
 status
----------
 active
 inactive
 pending
```

**Multiple columns (unique combinations):**

```sql
SELECT DISTINCT status, city FROM patients;
```

**Count distinct:**

```sql
SELECT COUNT(DISTINCT status) FROM patients;
```

## JOINS

Combine rows from multiple projections.

### INNER JOIN

Return rows that match in both tables:

```sql
SELECT p.name, a.appointment_date
FROM patients p
INNER JOIN appointments a ON p.id = a.patient_id;
```

### LEFT JOIN

Return all rows from left table, matching from right:

```sql
SELECT p.name, a.appointment_date
FROM patients p
LEFT JOIN appointments a ON p.id = a.patient_id;
-- Includes patients with no appointments (appointment_date = NULL)
```

### Multiple Joins

```sql
SELECT
  p.name,
  a.appointment_date,
  d.doctor_name
FROM patients p
LEFT JOIN appointments a ON p.id = a.patient_id
LEFT JOIN doctors d ON a.doctor_id = d.id;
```

### Join Conditions

```sql
-- Equality
INNER JOIN appointments ON patients.id = appointments.patient_id

-- Multiple conditions
INNER JOIN appointments ON
  patients.id = appointments.patient_id
  AND appointments.status = 'scheduled'

-- Inequality (use with caution - can be slow)
INNER JOIN appointments ON patients.id < appointments.id
```

### Join Performance

- **Indexed columns:** Fast (hash or merge join)
- **Non-indexed columns:** Slow (nested loop join)
- **Best practice:** Index join columns

```sql
-- Fast (id is primary key, indexed)
SELECT * FROM patients p
JOIN appointments a ON p.id = a.patient_id;

-- Slow (status not indexed)
SELECT * FROM patients p
JOIN appointments a ON p.status = a.status;
```

## Subqueries

Nested SELECT statements (v0.7.0+).

### Scalar Subquery

Returns single value:

```sql
SELECT name, age, (
  SELECT COUNT(*) FROM appointments WHERE patient_id = patients.id
) AS appointment_count
FROM patients;
```

### EXISTS Subquery

Check if subquery returns rows:

```sql
SELECT * FROM patients p
WHERE EXISTS (
  SELECT 1 FROM appointments a
  WHERE a.patient_id = p.id
    AND a.status = 'scheduled'
);
```

### IN Subquery

Match against list:

```sql
SELECT * FROM patients
WHERE id IN (
  SELECT patient_id FROM appointments
  WHERE appointment_date > CURRENT_DATE
);
```

### NOT IN / NOT EXISTS

```sql
-- Patients with no appointments
SELECT * FROM patients p
WHERE NOT EXISTS (
  SELECT 1 FROM appointments a WHERE a.patient_id = p.id
);

-- Equivalent with LEFT JOIN
SELECT p.* FROM patients p
LEFT JOIN appointments a ON p.id = a.patient_id
WHERE a.patient_id IS NULL;
```

## Common Table Expressions (WITH)

Named subqueries for readability (v0.7.0+).

### Syntax

```sql
WITH cte_name AS (
  SELECT ...
)
SELECT * FROM cte_name;
```

### Examples

**Simple CTE:**

```sql
WITH active_patients AS (
  SELECT * FROM patients WHERE status = 'active'
)
SELECT name, age FROM active_patients
WHERE age > 65;
```

**Multiple CTEs:**

```sql
WITH
  active_patients AS (
    SELECT * FROM patients WHERE status = 'active'
  ),
  upcoming_appointments AS (
    SELECT * FROM appointments WHERE date > CURRENT_DATE
  )
SELECT p.name, a.date
FROM active_patients p
JOIN upcoming_appointments a ON p.id = a.patient_id;
```

**Recursive CTE (v0.8.0+):**

```sql
WITH RECURSIVE org_chart AS (
  -- Base case
  SELECT id, name, manager_id, 1 AS level
  FROM employees
  WHERE manager_id IS NULL

  UNION ALL

  -- Recursive case
  SELECT e.id, e.name, e.manager_id, oc.level + 1
  FROM employees e
  JOIN org_chart oc ON e.manager_id = oc.id
)
SELECT * FROM org_chart;
```

## Functions

Built-in functions for data transformation.

### String Functions

```sql
-- CONCAT
SELECT CONCAT(first_name, ' ', last_name) FROM patients;

-- LENGTH
SELECT name, LENGTH(name) FROM patients;

-- UPPER / LOWER
SELECT UPPER(name) FROM patients;

-- SUBSTRING
SELECT SUBSTRING(name FROM 1 FOR 5) FROM patients;

-- TRIM
SELECT TRIM(name) FROM patients;
```

### Date Functions

```sql
-- CURRENT_DATE
SELECT * FROM appointments WHERE date = CURRENT_DATE;

-- EXTRACT
SELECT EXTRACT(YEAR FROM date_of_birth) AS birth_year FROM patients;
SELECT EXTRACT(MONTH FROM date_of_birth) AS birth_month FROM patients;

-- DATE arithmetic
SELECT * FROM appointments WHERE date > CURRENT_DATE + INTERVAL '7 days';
```

### Math Functions

```sql
-- ROUND
SELECT ROUND(AVG(age), 2) FROM patients;

-- FLOOR / CEIL
SELECT FLOOR(age / 10) * 10 AS age_bucket FROM patients;

-- ABS
SELECT ABS(balance) FROM accounts;
```

### Conditional Functions

```sql
-- CASE
SELECT
  name,
  CASE
    WHEN age < 18 THEN 'Minor'
    WHEN age < 65 THEN 'Adult'
    ELSE 'Senior'
  END AS age_group
FROM patients;

-- COALESCE (first non-null)
SELECT COALESCE(email, phone, 'No contact') FROM patients;

-- NULLIF (return NULL if equal)
SELECT NULLIF(status, '') FROM patients;
```

## Time-Travel Queries

Query historical data (Kimberlite-specific).

### AS OF TIMESTAMP

```sql
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-15 10:30:00'
WHERE id = 123;
```

### AS OF POSITION

```sql
SELECT * FROM patients
AS OF POSITION 1000
WHERE id = 123;
```

See [Time-Travel Queries Recipe](../../coding/recipes/time-travel-queries.md).

## Performance Tips

### 1. Use Indexes

```sql
-- ✅ Fast (indexed)
SELECT * FROM patients WHERE id = 123;

-- ❌ Slow (not indexed)
SELECT * FROM patients WHERE notes LIKE '%keyword%';
```

### 2. Avoid SELECT *

```sql
-- ❌ Slow: Fetches all columns
SELECT * FROM patients;

-- ✅ Fast: Fetch only needed columns
SELECT id, name FROM patients;
```

### 3. Filter Early

```sql
-- ✅ Good: Filter before join
SELECT p.name, a.date
FROM patients p
JOIN (
  SELECT * FROM appointments WHERE date > CURRENT_DATE
) a ON p.id = a.patient_id;

-- ❌ Slow: Filter after join
SELECT p.name, a.date
FROM patients p
JOIN appointments a ON p.id = a.patient_id
WHERE a.date > CURRENT_DATE;
```

### 4. Use LIMIT

```sql
-- ✅ Fast: Only fetch what you need
SELECT * FROM patients LIMIT 100;

-- ❌ Slow: Fetches millions of rows
SELECT * FROM patients;
```

### 5. Avoid Correlated Subqueries

```sql
-- ❌ Slow: Subquery runs for each row
SELECT name, (
  SELECT COUNT(*) FROM appointments WHERE patient_id = patients.id
) FROM patients;

-- ✅ Fast: Join with aggregation
SELECT p.name, COUNT(a.id)
FROM patients p
LEFT JOIN appointments a ON p.id = a.patient_id
GROUP BY p.name;
```

## Best Practices

### 1. Always Use WHERE with DELETE/UPDATE

See [DML Reference](dml.md#where-clause-required).

### 2. Index Foreign Keys

```sql
CREATE INDEX appointments_patient_id_idx ON appointments (patient_id);
```

### 3. Use EXPLAIN for Slow Queries

```sql
EXPLAIN SELECT * FROM patients WHERE status = 'active';
```

**Output:**
```
Seq Scan on patients (cost=0..100 rows=1250)
  Filter: (status = 'active')
```

Add index if seeing `Seq Scan` on large tables.

### 4. Paginate Large Results

```sql
-- ✅ Good: Pagination
SELECT * FROM patients
ORDER BY id
LIMIT 100 OFFSET 0;  -- Page 1

-- ❌ Bad: No limit
SELECT * FROM patients;
```

## Related Documentation

- **[SQL Overview](overview.md)** - SQL architecture
- **[DDL Reference](ddl.md)** - CREATE/DROP PROJECTION
- **[DML Reference](dml.md)** - INSERT/UPDATE/DELETE
- **[Time-Travel Queries](../../coding/recipes/time-travel-queries.md)** - Historical queries
- **[Coding Recipes](../../coding/recipes/)** - Application patterns

---

**Key Takeaway:** Kimberlite SQL supports standard SELECT syntax with WHERE, ORDER BY, JOINs, and aggregates. Use time-travel queries to access historical data. Index frequently queried columns for performance.
