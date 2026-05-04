# Cookbook: Secondary index lookup by non-PK column

What this teaches:

- `CREATE INDEX ON <projection>(<columns>)` against any non-PK column
  combination — equality and range queries against indexed columns
  use `IndexScan` at planning time (verifiable via `EXPLAIN`), not a
  full `TableScan`.
- The planner's `find_usable_indexes` /
  `select_best_index` (`crates/kimberlite-query/src/planner.rs`)
  already do the work — consumers just need to declare the index.
- The composite-index recipe from the ROADMAP:
  `CREATE INDEX ON projection(provider, providerMessageId)`
  followed by `WHERE provider = ? AND providerMessageId = ?`.

Notebar's `repos/communications.ts` lookup-by-provider-message-id
path was originally hitting full table scans because the index
declaration step was missing — surfaced in load testing under v0.6.x.

Prerequisites:

```bash
just kmb-server-dev
```

Run:

```bash
cd typescript && pnpm install && pnpm tsx main.ts
# or
cd python && python main.py
```

Expected stdout (last line):

```
KMB_COOKBOOK_OK
```

The recipe runs `EXPLAIN` after the index is declared and asserts
that the plan contains `IndexScan` rather than `TableScan`. Pre-fix
notebar would have seen `TableScan` here.

## Related docs

- [`docs/coding/recipes/secondary-index.md`](../../../docs/coding/recipes/secondary-index.md)
- [`docs/reference/sql/queries.md`](../../../docs/reference/sql/queries.md)
