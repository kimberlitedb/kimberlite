---
title: "Finance Quickstart (SEC / SOX / GLBA-aligned)"
section: "coding/quickstarts"
slug: "finance"
order: 6
---

# Finance Quickstart

Build an SEC 17a-4 / SOX / GLBA-aligned trading ledger on Kimberlite in about 10 minutes. This walkthrough drives the [`examples/finance/`](https://github.com/kimberlitedb/kimberlite/tree/main/examples/finance) reference: real-world schema (accounts, trades, positions, audit log) with an end-to-end story — append-only history that meets the "non-rewriteable, non-erasable storage" rule, point-in-time portfolio reconstruction for regulator subpoenas, and GDPR consent-basis tracking for EU-resident customers.

> **Compliance posture.** Kimberlite ships the cryptographic and architectural primitives that SEC 17a-4, SOX, GLBA, and PCI DSS demand. It is not a certified service; you still own the SOC 2 audit, the PCI scope reduction work, and the broker-dealer registration. See the [FAQ](/faq) for the full story.

## Prerequisites

- `kimberlite` binary on your `PATH` — see [Installation](/docs/start/installation)
- Python 3.10+ for the SDK walkthrough
- The repo cloned locally so you can run the example scripts

## Step 1 — Boot the ledger

From the repo root:

```bash
examples/finance/00-setup.sh
```

The script:
1. Wipes any prior `/tmp/kimberlite-finance/` workspace.
2. Runs `kimberlite init` to create a fresh project.
3. Starts a development server on `127.0.0.1:5432` in the background and writes its PID to `server.pid`.
4. Applies [`schema.sql`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/finance/schema.sql) — `accounts`, `trades`, `positions`, `audit_log` — with the seed data inlined (institutional + individual accounts, BUY/SELL trades on AAPL/MSFT/TSLA, derived positions, audit events).

When you're done with the walkthrough, kill the server:

```bash
kill "$(cat /tmp/kimberlite-finance/server.pid)"
```

## Step 2 — Run the SEC-style audit-trail walkthrough

```bash
kimberlite query --server 127.0.0.1:5432 -f examples/finance/audit_queries.sql
```

The queries demonstrate the SEC 17a-4 audit-trail pattern: every trade is
in the immutable log, the chain is walkable, and the audit-log table
projects the writes for fast querying.

## Step 3 — Reconstruct portfolios at any point in the past

```bash
kimberlite query --server 127.0.0.1:5432 -f examples/finance/03-time-travel.sql
```

Five queries demonstrating Kimberlite's two time-travel primitives:

- `AS OF TIMESTAMP '2024-01-15T23:59:59Z'` — wall-clock-based, resolved via the audit-log timestamp index. "What did the account hold at end of day on Jan 15?"
- `AT OFFSET n` — log-position-based, exact and deterministic. "What events were in this stream at sequence N?"

The fifth query proves cross-table point-in-time consistency: all four tables resolve to the same log offset for the same timestamp, so there is no torn read across `accounts`, `trades`, `positions`, and `audit_log` even mid-transaction.

## Step 4 — Run the SDK walkthrough

```bash
pip install -e sdks/python
python examples/finance/ledger.py
```

The script:

1. Creates a `Pool` (connection pooling) plus a dedicated admin `Client`.
2. Lists tables via `admin.list_tables()` — schema introspection.
3. Issues a typed query that maps rows into an `Account` dataclass.
4. Lists immutable trade history for account 1, projected into a `Trade` dataclass.
5. Builds a filtered query (BUY-side only) with the fluent `Query` builder.
6. Time-travels: queries `positions` as of `2024-01-15T23:59:59Z` to prove the TSLA position (opened on Jan 16) does not yet exist.
7. Grants `Contract` consent for an EU-resident account holder under GDPR Article 6 — the lawful basis a fintech leans on for customer onboarding.
8. Issues an Article 17 erasure request, with a note that SEC 17a-4's 6-year retention typically holds completion pending the retention window.
9. Prints pool stats.

Expected output:

```
✓ pool + admin client ready
✓ admin.list_tables → 5 tables: …
✓ typed query → 2 active account(s)
  · #1 ACCT-100234 (Institutional) → Apex Capital Management
  · #2 ACCT-100567 (Individual) → Sarah Chen
✓ typed query → account 1 has 3 trade(s) in immutable history
  · 2024-01-15 BUY 500 AAPL @ $189.50 = $94,750.00 by trader:jsmith
  · 2024-01-15 BUY 200 MSFT @ $390.00 = $78,000.00 by trader:jsmith
  · 2024-01-20 SELL 200 AAPL @ $192.00 = $38,400.00 by trader:jsmith
✓ query-builder → 3 BUY trade(s) recorded
✓ time-travel → 2 position(s) as of 2024-01-15 EOD
  · account 1: 500 AAPL @ avg $189.50
  · account 1: 200 MSFT @ avg $390.00
✓ compliance.consent.grant → basis=Contract consent_id=…
  · consent.check(account:2, 'Contract') → true
✓ erasure.request → request_id=… status=Pending
  · mark_progress for N stream(s)
  · complete() skipped — SEC 17a-4 retention applies in production
✓ pool.stats → open=1 in_use=0 idle=1

✅ ledger walkthrough complete
```

## What you just exercised

| Capability | Demonstrated by |
|---|---|
| Tables for accounts, trades, positions, audit log | [`schema.sql`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/finance/schema.sql) |
| SEC / SOX-aligned audit queries | [`audit_queries.sql`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/finance/audit_queries.sql) |
| Point-in-time portfolio reconstruction | [`03-time-travel.sql`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/finance/03-time-travel.sql) |
| Connection pooling + typed row mapping | [`ledger.py`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/finance/ledger.py) |
| Time-travel via the SDK | [`ledger.py`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/finance/ledger.py) |
| GDPR Article 6 consent under `Contract` basis | [`ledger.py`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/finance/ledger.py) |
| Article 17 erasure request orchestration | [`ledger.py`](https://github.com/kimberlitedb/kimberlite/blob/main/examples/finance/ledger.py) |

## SEC 17a-4 / SOX / GLBA mapping

### SEC Rule 17a-4 (Broker-Dealer Records)

| Requirement | How Kimberlite supports it |
|---|---|
| Non-rewriteable, non-erasable storage | Immutable append-only log |
| Records preserved for 3-6 years | Hash-chained events, no deletion |
| Immediate availability for 2 years | Point-in-time queries at any offset / timestamp |
| Audit trail of access | Built-in `audit_log` projection + `audit.verifyChain()` |

### SOX (Sarbanes-Oxley)

| Requirement | How Kimberlite supports it |
|---|---|
| Internal controls over financial reporting | RBAC + ABAC enforcement at the SQL rewriter |
| Audit trail for financial data | Immutable event log + server-walked chain attestation |
| Data integrity | CRC32 checksums + hash chains (SHA-256) |

### GLBA (Gramm-Leach-Bliley)

| Requirement | How Kimberlite supports it |
|---|---|
| Safeguards for customer financial data | Multi-tenant cryptographic isolation |
| Access controls | RBAC + ABAC policies |
| Data encryption at rest | AES-256-GCM with per-tenant keys |

## What this quickstart does not demonstrate

- **Multi-statement transactions** — `BEGIN`/`COMMIT`/`ROLLBACK` are planned post-v1.0. For double-entry ledger semantics today, post the debit and credit as a single batch append against a `journal_entries` event stream.
- **Real-time tick subscriptions** — the pattern is the same as the healthcare encounter feed; see the [TypeScript API reference](/docs/reference/sdk/typescript-api).
- **PCI DSS scope reduction via column masking** — `CREATE MASKING POLICY` composes with RBAC + break-glass; see [data-classification](/docs/concepts/data-classification).

## Next steps

- [Compliance concepts](/docs/concepts/compliance) — how Kimberlite's primitives map to SEC, SOX, GLBA, GDPR, PCI DSS.
- [Consent management](/docs/concepts/consent-management) — GDPR Article 6 lawful-basis tracking on the wire.
- [SDK parity matrix](/docs/reference/sdk/parity) — feature coverage across Rust, TypeScript, and Python.
- [Production deployment](/docs/operating/production-deployment) — what "production" looks like for v0.8.0 (single-node) and what's gated on v1.0.
