# Cookbook: Multi-tenant isolation

What this teaches:

- Tenant isolation is enforced at the protocol layer — there is no SQL
  syntax that crosses tenant boundaries.
- Two clients connecting with different `tenantId`s see entirely
  separate projection stores; the same `id` PK in `patients` exists
  independently in each tenant's view.
- Cross-tenant data sharing requires the explicit data-sharing API
  (not SQL). See `docs/coding/recipes/multi-tenant-queries.md`.

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

For SaaS / multi-clinic / multi-broker deployments, tenant isolation
is the single most-asked-about compliance question. Kimberlite ships
cryptographic boundaries per tenant — one tenant's PHI / PII is
physically inaccessible to another tenant's queries, no matter what
SQL they write.

## Related docs

- [`docs/coding/recipes/multi-tenant-queries.md`](../../../docs/coding/recipes/multi-tenant-queries.md)
- [`docs/concepts/multitenancy.md`](../../../docs/concepts/multitenancy.md)
