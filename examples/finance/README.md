# Finance / Ledger Example

A complete walkthrough of an SEC 17a-4 / SOX / GLBA-aligned trading
ledger backed by Kimberlite. Mirror of the healthcare clinic example —
same SDK shape, swapped domain. Every primitive a regulated fintech
needs on day one is exercised end-to-end: append-only audit trail,
cryptographic tamper evidence, point-in-time portfolio reconstruction,
GDPR Article 6 consent under a `Contract` lawful basis, GDPR Article 17
erasure (with SEC retention caveats), and connection pooling.

## What this example covers

| Capability | Shown in |
|---|---|
| Tables for accounts, trades, positions, audit log | [`schema.sql`](schema.sql) |
| Sample accounts, trades, positions, audit log | [`schema.sql`](schema.sql) (inline) |
| SEC / SOX-aligned audit queries | [`audit_queries.sql`](audit_queries.sql) |
| Point-in-time portfolio reconstruction | [`03-time-travel.sql`](03-time-travel.sql) |
| Connection pooling + typed row mapping (Python) | [`ledger.py`](ledger.py) |
| Admin API — list tables, introspect schema | [`ledger.py`](ledger.py) |
| Query builder | [`ledger.py`](ledger.py) |
| Time-travel via the SDK (`AS OF TIMESTAMP`) | [`ledger.py`](ledger.py) |
| Consent grant + check (GDPR Article 6, `Contract` basis) | [`ledger.py`](ledger.py) |
| GDPR Article 17 erasure (with SEC retention note) | [`ledger.py`](ledger.py) |

## Setup

From the repo root:

```bash
examples/finance/00-setup.sh
```

That script:
1. Creates a fresh project at `/tmp/kimberlite-finance`
2. Starts a dev server on `127.0.0.1:5432` (override with `ADDR=…`)
3. Applies `schema.sql` (which has seed data inlined)
4. Prints the next-steps menu

Stop the server when you're done:

```bash
kill "$(cat /tmp/kimberlite-finance/server.pid)"
```

## Run the SQL walkthroughs

```bash
kimberlite query --server 127.0.0.1:5432 -f audit_queries.sql
kimberlite query --server 127.0.0.1:5432 -f 03-time-travel.sql
```

Each file is annotated with what the query proves under SEC 17a-4 /
SOX / GLBA.

## Run the SDK walkthrough

```bash
pip install -e sdks/python
python examples/finance/ledger.py
```

The script:
1. Creates a `Pool` plus a dedicated admin `Client`.
2. Lists the schema (`admin.list_tables`).
3. Queries active accounts with typed row mapping into an `Account` dataclass.
4. Lists immutable trade history for account 1.
5. Builds a filtered query (BUY-side only) with the fluent `Query` builder.
6. Reconstructs positions as of an earlier date using `AS OF TIMESTAMP`.
7. Grants `Contract` consent for an EU-resident account (GDPR Article 6).
8. Issues an Article 17 erasure request (with a note that SEC retention
   typically holds completion pending the 6-year window).
9. Prints pool stats.

## SEC 17a-4 / SOX / GLBA mapping

### SEC Rule 17a-4 (Broker-Dealer Records)

| Requirement | Kimberlite Feature |
|-------------|--------------------|
| Non-rewriteable, non-erasable storage | Immutable append-only log |
| Records preserved for 3-6 years | Hash-chained events, no deletion |
| Immediate availability for 2 years | Point-in-time queries at any offset / timestamp |
| Audit trail of access | Built-in `audit_log` projection + `audit.verifyChain()` |

### SOX (Sarbanes-Oxley)

| Requirement | Kimberlite Feature |
|-------------|--------------------|
| Internal controls over financial reporting | RBAC with role-based access |
| Audit trail for financial data | Immutable event log + server-walked chain attestation |
| Data integrity | CRC32 checksums + hash chains |

### GLBA (Gramm-Leach-Bliley)

| Requirement | Kimberlite Feature |
|-------------|--------------------|
| Safeguards for customer financial data | Multi-tenant cryptographic isolation |
| Access controls | RBAC + ABAC policies |
| Data encryption at rest | AES-256-GCM with per-tenant keys |

## What this example does not demonstrate

- **Multi-statement transactions.** Kimberlite v0.8.0 doesn't yet ship
  `BEGIN`/`COMMIT`/`ROLLBACK` (planned post-v1.0). For double-entry
  ledger semantics, post the debit and credit as a single batch append
  against a `journal_entries` event stream — see
  [`docs/coding/recipes/audit-trails`](../../docs/coding/recipes/audit-trails.md).
- **Real-time tick subscriptions.** The pattern is the same as the
  healthcare example's encounter feed; see
  [`docs/reference/sdk/python-api`](../../docs/reference/sdk/python-api.md).
- **Cross-tenant book reconciliation.** Keep this example single-tenant;
  see [`docs/concepts/multitenancy`](../../docs/concepts/multitenancy.md)
  for cross-tenant flows.

## Reference

- [docs/concepts/compliance](../../docs/concepts/compliance.md) — full framework coverage
- [docs/concepts/consent-management](../../docs/concepts/consent-management.md)
- [docs/concepts/data-portability](../../docs/concepts/data-portability.md)
- [docs/reference/sdk/parity](../../docs/reference/sdk/parity.md) — feature matrix across SDKs
