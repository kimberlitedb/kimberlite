# Kimberlite Cookbook

Runnable, primitive-focused recipes for features the first downstream
consumer (notebar) repeatedly tripped on because nothing in
`examples/` pointed at them. Each recipe ships in TypeScript first
(notebar's stack), Python where cheap, Rust where useful.

AUDIT-2026-05 M-7 — closes the v0.7.0 ROADMAP item "Cookbook examples
for already-shipped primitives that downstream consumers keep
missing."

## Recipes

| Recipe | Languages | What it teaches |
|---|---|---|
| [`subscriptions/`](./subscriptions/) | TS, Python | `client.subscribe(streamId, { startOffset })` AsyncIterable, credit-based flow control, idempotent unsubscribe |
| [`secondary-index/`](./secondary-index/) | TS, Python | `CREATE INDEX ON projection(provider, providerMessageId)` + EXPLAIN-verified index scans on non-PK columns |
| [`consent-decline/`](./consent-decline/) | TS, Python | `recordConsent({ termsVersion, accepted: false })` decline flow + audit-trail verification |

## Running

Each recipe has its own README with prerequisites and a
single-command run line. Common pattern:

```bash
# Boot a local server in one shell:
just kmb-server-dev

# In another shell, run the recipe:
cd examples/cookbook/subscriptions/typescript
pnpm install
pnpm tsx main.ts
# Expect: KMB_COOKBOOK_OK
```

The `KMB_COOKBOOK_OK` stdout marker is the success signal CI gates
on. Don't print it unless every assertion in the recipe passed.

## CI

`/.github/workflows/examples-test.yml` (separate from the SDK
publish workflows so cookbook regressions can't block an SDK
release) spins up a fresh `kmb-server` and runs each recipe with
a 60-second wall-clock budget.
