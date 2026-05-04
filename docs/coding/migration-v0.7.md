# Migration: v0.6.2 → v0.7.0

v0.7.0 is the production-validation release. Every v0.6.2 → v0.7.0
upgrade path is small — most deltas surface as new SDK methods, new
SQL functions, or relaxed error paths. The one breaking change is
the Python SDK floor bump.

## Breaking changes (1)

### Python SDK floor: 3.9 → 3.10

`pyproject.toml`:

```diff
- requires-python = ">=3.9"
+ requires-python = ">=3.10"
```

If you're on Python 3.9, hold at `kimberlite==0.6.2` until you
upgrade. Python 3.9 reached EOL 2025-10. The bump unlocks
`X | None` runtime syntax (PEP 604) which the typing-refresh path
in `sdks/python/kimberlite/` uses going forward.

## New SQL surface (no migration required)

These are additive — existing queries keep working unchanged.

- `MOD(a, b)`, `POWER(base, exp)`, `SQRT(x)` — numeric scalars
- `SUBSTRING(s FROM start [FOR length])` — Unicode char-correct
- `EXTRACT(field FROM ts)`, `DATE_TRUNC(field, ts)` — date/time scalars
- `NOW()`, `CURRENT_TIMESTAMP`, `CURRENT_DATE` — sentinel parsing
  works; production execution lands in v0.8.0 (currently panics
  loudly if used; pin tests against `#[should_panic]`).

See [`docs/reference/sql/scalar-functions.md`](../reference/sql/scalar-functions.md)
for the full reference.

## Deprecated `MAX_GROUP_COUNT` const

The hard-coded 100 000 group cap is replaced by `AggregateMemoryBudget`.
For most consumers this is invisible — the new default (256 MiB ≈ 1M
groups) is strictly more generous. Consumers tuning the limit:

```rust
// v0.6.2 (gone)
// const MAX_GROUP_COUNT: usize = 100_000;

// v0.7.0 — configurable on the engine
use kimberlite_types::AggregateMemoryBudget;
let engine = QueryEngine::new(schema)
    .with_aggregate_budget(AggregateMemoryBudget::try_new(512 * 1024 * 1024)?);
```

The error variant is `QueryError::AggregateMemoryExceeded { budget,
observed }` instead of a panic with a const message.

## Catalog staleness fix (no migration required)

Pre-v0.7.0 workaround was unique-per-test table names. You can now
use the natural `DROP TABLE t; CREATE TABLE t (...); INSERT INTO
t (...)` pattern within a single connection. Existing fixtures with
suffix-per-invocation table names keep working.

## Inverted-range planner short-circuit (no migration required)

Predicate combinations like `WHERE x > 5 AND x < 3` now route to a
zero-row table scan with an `AlwaysFalse` filter at planning time
instead of an inverted-range scan that the store had to clamp.
Observable change: `EXPLAIN` plans for such queries show
`TableScan` with `filter: AlwaysFalse` rather than `RangeScan`. The
result set (empty) is unchanged.

## Cookbook examples

If you previously hit any of these footguns, the new
`examples/cookbook/` recipes are the canonical reference:

- Real-time subscriptions (push, not poll) →
  `examples/cookbook/subscriptions/`
- Secondary-index lookup on a non-PK column →
  `examples/cookbook/secondary-index/`
- Consent decline with `accepted: false` + audit-trail verification
  → `examples/cookbook/consent-decline/`

## Verifying the release

```bash
git tag -v v0.7.0
```

The signing public key is at `keys/release-signing.asc`. Full
verification flow is documented in
[`docs/operating/verifying-releases.md`](../operating/verifying-releases.md).
