# Cookbook: Time-travel queries

What this teaches:

- `SELECT … AS OF TIMESTAMP '<iso>'` reconstructs the projection state
  at any point in the past.
- `client.queryAt(sql, params, at)` is the SDK ergonomic surface for the
  same query. `at` accepts a `Date`, a `string` ISO-8601, or an
  `AtClause` for log-position-based time travel.
- Two queries against the same `AS OF TIMESTAMP` resolve to the same
  log offset — there is no torn read.

Prerequisites:

```bash
just kmb-server-dev   # runs on 5432
```

Run:

```bash
cd typescript && pnpm install && pnpm tsx main.ts
```

Expected stdout (last line):

```
KMB_COOKBOOK_OK
```

## Why this matters

Compliance audits ("show me the chart at 2023-08-12 14:30 UTC"),
malpractice review, end-of-quarter reporting — every workflow that
needs to reconstruct historical state goes through this primitive.

## Related docs

- [`docs/coding/recipes/time-travel-queries.md`](../../../docs/coding/recipes/time-travel-queries.md)
- [`docs/reference/sql/queries.md`](../../../docs/reference/sql/queries.md) — `AS OF TIMESTAMP` syntax reference
- [`examples/healthcare/03-time-travel.sql`](../../healthcare/03-time-travel.sql) — clinical-domain walkthrough
- [`examples/finance/03-time-travel.sql`](../../finance/03-time-travel.sql) — finance-domain walkthrough
