# Correlated subqueries (v0.6.0)

Kimberlite's SQL layer supports four correlated-subquery shapes in the `WHERE`
clause:

| Shape                        | Semantics                                       |
|------------------------------|-------------------------------------------------|
| `EXISTS (SELECT ...)`        | true when inner produces at least one row       |
| `NOT EXISTS (SELECT ...)`    | true when inner is empty                        |
| `col IN (SELECT col FROM ...)` | true when outer `col` matches any inner row   |
| `col NOT IN (SELECT col ...)`| true when outer `col` does not match any inner  |

Each form may reference columns from the enclosing (outer) query.
Healthcare reporting workloads are the motivating use case, e.g.

```sql
SELECT p.* FROM patient_current p
WHERE EXISTS (
  SELECT 1 FROM consent_current c
  WHERE c.subject_id = p.id
    AND c.purpose = 'HealthcareDelivery'
    AND c.withdrawn_at IS NULL
)
```

## Execution strategies

The planner picks one of three strategies in decreasing order of efficiency.

### 1. Uncorrelated pre-execution (unchanged since v0.5.0)

When the inner subquery references no outer columns, `pre_execute_subqueries()`
runs it once, materialises the result, and substitutes:

- `IN (SELECT)` → `IN (literal, literal, ...)`
- `EXISTS (...)` → `Always(true)` if rows, `Always(false)` if empty
- `NOT EXISTS (...)` → inverse of `EXISTS`

This path stays on the fast predicate lane and is unchanged by the v0.6.0 work.

### 2. Semi-join decorrelation (v0.6.0+)

When a correlated `EXISTS` / `NOT EXISTS` has a single equijoin linking outer
and inner tables (e.g. `WHERE c.subject_id = p.id`), it is rewritten to a
semi-join. Concretely, we pre-execute the inner query projecting only the join
column with any non-correlated inner predicates applied, then substitute the
result set as:

- `EXISTS` → `outer_col IN (inner_col1, inner_col2, ...)`
- `NOT EXISTS` → `outer_col NOT IN (...)`

This reuses the existing `Predicate::In` / `Predicate::NotIn` fast path.

Conditions for decorrelation:

1. Exactly one correlated equality of the form `inner.x = outer.y` (or swapped).
2. Inner subquery has no `GROUP BY`, aggregates, `LIMIT`, or `ORDER BY`.
3. Inner subquery projects exactly one column (or is `EXISTS`, in which case
   the column is synthesised from the correlated equality).
4. No nested correlated subqueries (v0.6.1).

### 3. Correlated loop fallback

When decorrelation conditions are not met, the engine falls back to a nested
loop:

1. Execute the outer query with the correlated predicate stripped (other
   predicates still apply).
2. For each surviving outer row, re-plan the inner subquery with outer-column
   references substituted by the outer row's value, execute it, and evaluate
   the predicate (`EXISTS` / `NOT EXISTS` / `IN` / `NOT IN`).
3. Row passes or fails the overall filter based on the per-row subquery
   outcome.

Re-planning is memoised per query; only the parameter values change between
iterations, so parse + plan cost is amortised.

## Scope stack and column resolution

The planner threads a `PlannerScope` through nested `plan_query` calls. Each
scope owns a list of (alias → `TableDef`) bindings visible at its level. When a
column reference is resolved:

1. Look it up in the innermost scope's tables. If found, it binds there.
2. Otherwise, walk upward through enclosing scopes. The first match becomes an
   **outer reference**, tagged with `scope_depth` (0 = innermost).
3. If no scope resolves the column, return `QueryError::ColumnNotFound`.

Qualified references (`alias.column`) short-circuit to the scope whose alias
matches, skipping shadowing in nearer scopes.

## Cardinality guard

Correlated-loop queries multiply outer row count by inner row cost. To avoid
accidental Cartesian blow-up, we cap `outer_rows * inner_rows_per_iter` at a
conservative upper bound:

```
max_correlated_row_evaluations: u64 = 10_000_000
```

Before the loop runs, the engine estimates outer and inner row counts (using
the store's size hints where available, otherwise pessimistic upper bounds)
and compares the product to the cap. If it exceeds, the query fails fast with
`QueryError::CorrelatedCardinalityExceeded` rather than consuming memory.

Operators can tune the cap via `QueryEngine::with_correlated_cap(u64)`.

## Known limitations

- Correlated subqueries are supported only in `WHERE` (not `SELECT` list, not
  `HAVING`, not `ORDER BY`).
- The inner subquery may not itself contain a correlated sub-subquery that
  references a third, doubly-outer scope (scope depth capped at 2).
- `ANY` / `ALL` / `SOME` scalar-comparison subqueries are not supported —
  rewrite them as `IN (SELECT)` or `EXISTS (SELECT)` manually.
- The scalar-subquery form `WHERE col = (SELECT scalar FROM ...)` is not
  supported (v0.7 item).
- The correlated-loop executor does not push projection/limit/sort into the
  inner re-plan — it always re-executes from the full inner predicate tree.

## Examples

```sql
-- Correlated EXISTS: uses the semi-join fast path.
SELECT p.id
FROM patient_current p
WHERE EXISTS (SELECT 1 FROM consent_current c WHERE c.subject_id = p.id);

-- Correlated NOT IN: uses anti-join fast path.
SELECT o.order_id
FROM orders o
WHERE o.user_id NOT IN (SELECT u.id FROM users u WHERE u.active = false);

-- Correlated with a non-equijoin predicate: falls back to the loop.
SELECT p.id FROM patient_current p
WHERE EXISTS (
  SELECT 1 FROM encounter e
  WHERE e.patient_id = p.id AND e.created_at > p.last_seen
);
```
