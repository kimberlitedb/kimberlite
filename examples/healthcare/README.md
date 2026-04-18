# Healthcare / Clinic Management Example

A complete walkthrough of a HIPAA-aware clinic backed by Kimberlite.
This is the reference example for compliance-heavy workloads: every
primitive the platform offers — append-only audit trail, RBAC-scoped
data access, time-travel queries, GDPR consent and erasure, real-time
subscriptions — is exercised end-to-end.

## What this example covers

| Capability | Shown in |
|---|---|
| Tables + indexes for a real clinical schema | [`schema.sql`](schema.sql) |
| Providers, patients, access grants, encounters, audit log seed | [`01-seed.sql`](01-seed.sql) |
| HIPAA audit queries — who accessed what, when | [`02-audit-queries.sql`](02-audit-queries.sql) |
| Point-in-time queries for audit / malpractice review | [`03-time-travel.sql`](03-time-travel.sql) |
| Connection pooling + typed row mapping (Rust/TS/Python) | `clinic.{rs,ts,py}` |
| Admin API — list tables, introspect schema | `clinic.{rs,ts,py}` |
| Query builder | `clinic.{rs,ts,py}` |
| Consent grant + check (GDPR Art 6 + HIPAA) | `clinic.{rs,ts,py}` |
| Real-time subscribe (pattern documented; live demo skipped for idempotence) | `clinic.{rs,ts,py}` + [TypeScript API reference](../../docs/reference/sdk/typescript-api.md) |
| GDPR Article 17 "right to be forgotten" | `clinic.{rs,ts,py}` |

## Setup

From the repo root:

```bash
examples/healthcare/00-setup.sh
```

That script:
1. Creates a fresh project at `/tmp/kimberlite-clinic`
2. Starts a dev server on `127.0.0.1:5432` (override with `ADDR=...`)
3. Applies `schema.sql`
4. Applies `01-seed.sql`
5. Prints the next-steps menu

Stop the server when you're done:

```bash
kill "$(cat /tmp/kimberlite-clinic/server.pid)"
```

## Run the SQL walkthroughs

```bash
kimberlite query --server 127.0.0.1:5432 -f 02-audit-queries.sql
kimberlite query --server 127.0.0.1:5432 -f 03-time-travel.sql
```

Each file is annotated with what the query proves.

## Run the SDK walkthroughs

Each `clinic.*` script is self-contained and demonstrates the same
flow, so you can compare across languages.

### TypeScript

```bash
cd sdks/typescript && npm install && npm run build
cd ../..
ts-node examples/healthcare/clinic.ts
```

### Python

```bash
pip install -e sdks/python
python examples/healthcare/clinic.py
```

### Rust

```bash
cd examples/rust
cargo run --example clinic
```

All three scripts follow the same storyline:

1. Create a pool + a dedicated admin client.
2. List the schema (`admin.listTables`).
3. Query active patients with typed row mapping.
4. Compose a filter with the query builder.
5. Grant research consent for a subject; check it; list.
6. Create a stream, subscribe, publish events, observe real-time delivery.
7. Request erasure; mark progress on affected streams (complete is
   deferred to the app, which actually erases rows).
8. Print pool stats.

Example output (TypeScript; the other two languages are equivalent):

```
✓ pool + admin client ready
✓ admin.listTables → 7 tables: _kimberlite_audit, _kimberlite_consent, access_grants, audit_log, encounters, patients, providers
✓ typed query → 4 active patients
  · #1 Jane Doe (MRN MRN-001234) → provider 1
  · #2 John Smith (MRN MRN-005678) → provider 2
  · #3 Alice Johnson (MRN MRN-009999) → provider 1
  · #4 Bob Williams (MRN MRN-013571) → provider 4
✓ query-builder → Dr. Chen has 1 patient(s)
✓ compliance.consent.grant → consentId=aa83f90c-a009-4746-adfb-f02fde19783e
  · consent.check(patient:1, 'Research') → true
✓ subscribe → skipped (see docs/reference/sdk/typescript-api.md for a full example)
✓ erasure.request → requestId=0a8f7ba0-3ef4-4153-97d5-9870a0e94e95 status=Pending
  · complete() skipped in demo — see docs/concepts/data-portability.md
✓ pool.stats → open=1 inUse=0 idle=1

✅ clinic walkthrough complete
```

## What this example does **not** demonstrate

- **Masking policy CRUD.** The underlying mechanism exists in
  `kimberlite-rbac::masking`, but wire-protocol-level policy mutation
  lands in v0.5.1. For now, role-scoped views are enforced via the
  server's RBAC/ABAC SQL rewriter.
- **Breach detection alerts.** Wire surface shipped in v0.5.0; server
  handlers arrive in v0.5.1 (`docs/reference/sdk/parity.md`). The
  `02-audit-queries.sql` Q3 + Q4 show the *input* patterns a breach
  detector would key on.
- **Multi-tenant cross-clinic scenarios.** Keep it to one tenant
  for this walkthrough; see [concepts/multitenancy](../../docs/concepts/multitenancy.md)
  for the cross-tenant story.

## HIPAA Technical Safeguards mapping

| 45 CFR §164.312 | Demonstrated by |
|---|---|
| (a)(1) Access Control | Multi-tenant isolation + `access_grants` table + RBAC/ABAC |
| (a)(2)(i) Unique User Identification | Provider + API-key per-subject identity |
| (b) Audit Controls | Append-only log underneath + `audit_log` projection |
| (c)(1) Integrity | Hash-chained log entries (SHA-256) + CRC32 on every frame |
| (d) Authentication | JWT + API key with `client.admin.issueApiKey()` |
| (e) Transmission Security | TLS (configure via `kimberlite start --tls-cert …`) |

## Reference

- [docs/concepts/compliance](../../docs/concepts/compliance.md) — full framework coverage
- [docs/concepts/consent-management](../../docs/concepts/consent-management.md)
- [docs/concepts/data-portability](../../docs/concepts/data-portability.md)
- [docs/reference/sdk/parity](../../docs/reference/sdk/parity.md) — feature matrix across SDKs
