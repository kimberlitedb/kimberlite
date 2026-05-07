---
title: "Healthcare Quickstart (HIPAA-aligned)"
section: "coding/quickstarts"
slug: "healthcare"
order: 5
---

# Healthcare Quickstart

Build a HIPAA-aligned clinic backend on Kimberlite in about 10 minutes. This walkthrough drives the [`examples/healthcare/`](https://github.com/kimberlitedb/kimberlite/tree/main/examples/healthcare) reference: a real clinical schema (patients, providers, encounters, access grants, audit log) with a deliberate end-to-end story — append-only audit trail, RBAC-scoped access, time-travel queries, GDPR consent + Article 17 erasure, and real-time subscriptions.

> **Compliance posture.** Kimberlite ships the cryptographic and architectural primitives that HIPAA Technical Safeguards (45 CFR §164.312) demand. It is not a certified HIPAA service; you still own the BAA with your hosting provider, the pen test, and the audit firm engagement. See the [FAQ](/faq) for the full story.

## Prerequisites

- `kimberlite` binary on your `PATH` — see [Installation](/docs/start/installation)
- Python 3.10+ **or** Node.js 18+ **or** Rust 1.88+ (pick one for the SDK walkthrough)
- The repo cloned locally so you can run the example scripts

## Step 1 — Boot the clinic

From the repo root:

```bash
examples/healthcare/00-setup.sh
```

The script:
1. Wipes any prior `/tmp/kimberlite-clinic/` workspace.
2. Runs `kimberlite init` to create a fresh project.
3. Starts a development server on `127.0.0.1:5432` in the background and writes its PID to `server.pid`.
4. Applies [`schema.sql`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/healthcare/schema.sql) — `patients`, `providers`, `encounters`, `access_grants`, `audit_log`.
5. Applies [`01-seed.sql`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/healthcare/01-seed.sql) — four patients, four providers, encounters, access grants, audit log seed.

When you're done with the walkthrough, kill the server:

```bash
kill "$(cat /tmp/kimberlite-clinic/server.pid)"
```

## Step 2 — Run the audit-trail SQL walkthrough

Kimberlite's append-only event log means every write is replayable. The seed data includes both legitimate care events and a planted "after-hours access by an unauthorised provider" — exactly the pattern a HIPAA breach detector keys on.

```bash
kimberlite query --server 127.0.0.1:5432 -f examples/healthcare/02-audit-queries.sql
```

The queries demonstrate:
- **Q1 — All accesses to a specific patient's record.** Equivalent to a §164.312(b) audit control review.
- **Q2 — Access grants for a provider.** Who has been authorised to see what?
- **Q3 — After-hours access pattern.** Surface accesses outside `08:00–18:00` — the planted breach pattern.
- **Q4 — Unauthorised access detection.** Audit-log entries whose `provider_id` is not present in `access_grants` for the target patient.

Each query is annotated with the §164.312 clause it satisfies.

## Step 3 — Run the time-travel SQL walkthrough

Reconstruct the patient record as of any point in the past — useful for malpractice review or "what did the chart look like when the prescription was written?"

```bash
kimberlite query --server 127.0.0.1:5432 -f examples/healthcare/03-time-travel.sql
```

The queries use Kimberlite's two time-travel primitives:

- `SELECT … AT OFFSET n` — log-position-based, deterministic and exact.
- `SELECT … AS OF TIMESTAMP '…'` — wall-clock-based, resolved via the audit-log timestamp index.

## Step 4 — Run the SDK walkthrough (pick your language)

All three SDKs run the same storyline so you can compare ergonomics. Each script:

1. Creates a `Pool` (connection pooling) plus a dedicated admin `Client`.
2. Lists tables via `admin.list_tables()` — schema introspection.
3. Issues a typed query that maps rows into a `Patient` struct/class.
4. Builds a filtered query with the fluent `Query` builder.
5. Grants research consent for patient 1 (`compliance.consent.grant(subject_id, "Research")`) and verifies it (`consent.check`).
6. Issues a GDPR Article 17 erasure request (`compliance.erasure.request(subject_id)`).
7. Prints pool stats.

### Python

```bash
pip install -e sdks/python
python examples/healthcare/clinic.py
```

### TypeScript

```bash
cd sdks/typescript && npm install && npm run build && cd ../..
ts-node examples/healthcare/clinic.ts
```

### Rust

```bash
cd examples/rust && cargo run --example clinic
```

Expected output (TypeScript; the other two languages produce equivalent text):

```
✓ pool + admin client ready
✓ admin.listTables → 7 tables: _kimberlite_audit, _kimberlite_consent, access_grants, audit_log, encounters, patients, providers
✓ typed query → 4 active patients
  · #1 Jane Doe (MRN MRN-001234) → provider 1
  · #2 John Smith (MRN MRN-005678) → provider 2
  · #3 Alice Johnson (MRN MRN-009999) → provider 1
  · #4 Bob Williams (MRN MRN-013571) → provider 4
✓ query-builder → Dr. Chen has 1 patient(s)
✓ compliance.consent.grant → consentId=…
  · consent.check(patient:1, 'Research') → true
✓ subscribe → skipped (see docs/reference/sdk/typescript-api.md for a full example)
✓ erasure.request → requestId=… status=Pending
  · complete() skipped in demo — see docs/concepts/data-portability.md
✓ pool.stats → open=1 inUse=0 idle=1

✅ clinic walkthrough complete
```

## What you just exercised

| Capability | Demonstrated by |
|---|---|
| Tables + indexes for a real clinical schema | [`schema.sql`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/healthcare/schema.sql) |
| Providers, patients, access grants, encounters, audit log seed | [`01-seed.sql`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/healthcare/01-seed.sql) |
| HIPAA audit queries — who accessed what, when | [`02-audit-queries.sql`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/healthcare/02-audit-queries.sql) |
| Point-in-time queries for audit / malpractice review | [`03-time-travel.sql`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/healthcare/03-time-travel.sql) |
| Connection pooling + typed row mapping | `clinic.{rs,ts,py}` |
| Admin API — list tables, introspect schema | `clinic.{rs,ts,py}` |
| Query builder | `clinic.{rs,ts,py}` |
| Consent grant + check (GDPR Article 6 + HIPAA) | `clinic.{rs,ts,py}` |
| GDPR Article 17 "right to be forgotten" | `clinic.{rs,ts,py}` |

## HIPAA Technical Safeguards mapping (45 CFR §164.312)

| Clause | How Kimberlite supports it |
|---|---|
| (a)(1) Access Control | Multi-tenant isolation + `access_grants` table + RBAC/ABAC enforcement |
| (a)(2)(i) Unique User Identification | Provider + API-key-per-subject identity |
| (b) Audit Controls | Append-only log + queryable `audit_log` projection + server-walked `audit.verifyChain()` |
| (c)(1) Integrity | Hash-chained log entries (SHA-256) + CRC32 on every frame |
| (d) Authentication | JWT + API key with `client.admin.issue_api_key()` |
| (e) Transmission Security | TLS (configure via `kimberlite start --tls-cert …`) |

## What this quickstart does not demonstrate

- **`audit.verifyChain()`** — the server-walked SHA-256 chain attestation that detects audit-log tampering. Shipped in v0.8.0; see the [TypeScript API reference](/docs/reference/sdk/typescript-api) for a snippet.
- **Real-time subscriptions** — `client.subscribe(streamId, { startOffset })` is an `AsyncIterable<SubscriptionEvent>` with credit-based flow control. Pattern is shown inline in `clinic.{ts,py,rs}` but the live demo is skipped to keep the script idempotent.
- **Multi-tenant cross-clinic scenarios** — see [concepts/multitenancy](/docs/concepts/multitenancy) for the cross-tenant story.
- **Masking policy CRUD** — `CREATE MASKING POLICY` DDL composes with RBAC + break-glass; see [data-classification](/docs/concepts/data-classification).

## Next steps

- [Compliance concepts](/docs/concepts/compliance) — how Kimberlite's primitives map to HIPAA, GDPR, SOC 2, PCI DSS, ISO 27001, FedRAMP.
- [Consent management](/docs/concepts/consent-management) — GDPR Article 6 lawful-basis tracking on the wire.
- [Data portability](/docs/concepts/data-portability) — GDPR Article 20 + Article 17 erasure orchestration.
- [SDK parity matrix](/docs/reference/sdk/parity) — feature coverage across Rust, TypeScript, and Python.
- [Production deployment](/docs/operating/production-deployment) — what "production" looks like for v0.8.0 (single-node) and what's gated on v1.0.
