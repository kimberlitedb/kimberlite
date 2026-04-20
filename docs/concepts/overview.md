---
title: "Overview - What is Kimberlite?"
section: "concepts"
slug: "overview"
order: 1
---

# Overview - What is Kimberlite?

Kimberlite is a compliance-first database for regulated industries.

## Elevator Pitch

**For:** Developers building healthcare, finance, or legal applications
**Who need:** Immutable audit trails and provable correctness
**Kimberlite is:** A database built on append-only logs
**That:** Makes compliance natural rather than bolted-on
**Unlike:** Traditional databases that treat auditing as an afterthought
**Kimberlite:** Makes illegal states impossible to represent

## One Principle

Everything in Kimberlite derives from a single architectural principle:

```
All data is an immutable, ordered log.
All state is a derived view.
```

Or more precisely:

```
State = Apply(InitialState, Log)
```

This isn't a novel idea. Event sourcing, CQRS, and log-based systems have existed for decades. What's novel is making this the *only* way to use the database, optimizing every layer for this model, and targeting compliance-critical workloads.

## Why Does This Matter?

Traditional databases:
1. Update data in place
2. Add audit tables later
3. Hope nobody tampers with audit logs
4. Pray during regulatory audits

Kimberlite:
1. Append events to an immutable log
2. Derive state from events
3. Cryptographically chain events (tamper-evident)
4. Reconstruct any point-in-time state

**Result:** Compliance is a natural consequence of the architecture, not something you bolt on.

## How We Guarantee Correctness

Kimberlite achieves compliance through formal verification—mathematical proofs that guarantee correctness:

**6 verification layers:**
- **Protocol (30 theorems):** TLA+, Ivy, Alloy proofs for consensus safety, view changes, recovery
- **Cryptography (15 theorems):** Coq proofs for SHA-256, BLAKE3, AES-GCM, Ed25519, key hierarchy
- **Code (91 proofs):** Kani bounded model checking for offset monotonicity, isolation, hash chains
- **Types (80+ signatures):** Flux refinement types for compile-time safety (ready when Flux stabilizes)
- **Compliance (23 frameworks):** HIPAA, GDPR, SOC 2, PCI DSS, ISO 27001, FedRAMP and 17 more formally modeled
- **Traceability (100%):** Every theorem mapped from TLA+ → Rust → VOPR tests

**Why this matters:** Traditional databases rely on testing, which can't prove absence of bugs. Kimberlite uses mathematical proofs to guarantee correctness—the same techniques used for space missions and medical devices.

**→ [Deep dive into formal verification](/docs/internals/formal-verification)**

## Who Is This For?

### Industries

- **Healthcare:** Electronic Health Records (EHR), patient portals, clinical trials
- **Finance:** Transaction ledgers, trade audit trails, regulatory reporting
- **Legal:** Document management, case history, evidence chain-of-custody
- **Government:** Public records, regulatory compliance, transparency

### Use Cases

✅ **Good fit:**
- Audit-critical applications
- Systems requiring point-in-time reconstruction
- Multi-tenant SaaS with isolation requirements
- Applications where "what happened and when" matters more than "what's the current state"

❌ **Poor fit:**
- High-throughput analytics (use a data warehouse)
- General-purpose CRUD apps (use Postgres)
- Message queuing (use Kafka or RabbitMQ)
- Real-time leaderboards (use Redis)

## Core Features

### 1. Immutability

All data is append-only. Nothing is ever deleted or modified. "Deletion" is a new event that marks data as deleted—the original event remains in the log.

```rust
// There is no db.delete() - only append new events
db.append(Event::RecordDeleted { id: 123, reason: "Patient request (GDPR)" })?;
```

### 2. Audit Trail by Default

Every write is logged with:
- What changed (command)
- When it changed (timestamp)
- Who changed it (client ID)
- Why it changed (optional reason)
- What happened before (previous state hash)

### 3. Time-Travel Queries

Query data as it existed at any point in time:

```sql
-- What did we know about patient 123 on January 15th?
SELECT * FROM patients AS OF TIMESTAMP '2024-01-15 10:30:00'
WHERE id = 123;

-- What did the database look like 1000 operations ago?
SELECT * FROM patients AS OF POSITION 1000 WHERE region = 'us-east';
```

### 4. Multi-Tenancy First

Each tenant's data is:
- Physically isolated (separate log partitions)
- Separately encrypted (per-tenant keys)
- Regionally constrained (data sovereignty)
- Independently quotaed (storage and throughput limits)

### 5. Cryptographic Guarantees

- **Hash chains:** Every event links to the previous event's hash
- **Tamper evidence:** Modifying any event breaks all subsequent hashes
- **Dual-hash system:** SHA-256 for compliance (FIPS-approved), BLAKE3 for performance

### 6. Consensus (VSR)

Multi-node clusters use Viewstamped Replication (VSR) for fault tolerance:
- **f+1** tolerates f failures (3 nodes = 1 failure, 5 nodes = 2 failures)
- **Proven protocol:** Same as TigerBeetle's battle-tested consensus
- **Deterministic:** All replicas process operations in identical order

## What Kimberlite Is Not

Kimberlite is intentionally limited in scope:

- **Not Postgres:** No Postgres wire protocol, no full SQL support
- **Not an analytics engine:** Use a data warehouse for OLAP workloads
- **Not a message queue:** Use Kafka if you need pub/sub
- **Not a cache:** Use Redis for hot-path performance

These limitations exist to maintain **simplicity, auditability, and correctness**.

## Architecture in 30 Seconds

```
┌──────────────────────────────────────────────────┐
│  Client (SDK)                                    │
├──────────────────────────────────────────────────┤
│  Server (RPC protocol)                           │
├──────────────────────────────────────────────────┤
│  Consensus (VSR)                                 │
├──────────────────────────────────────────────────┤
│  Kernel (pure state machine: Cmd → State + FX)   │
├──────────────────────────────────────────────────┤
│  Append-Only Log (hash-chained, CRC32)           │
├──────────────────────────────────────────────────┤
│  Crypto (SHA-256, BLAKE3, AES-256-GCM, Ed25519)  │
└──────────────────────────────────────────────────┘
```

**Data flow:**
1. Client sends command
2. Consensus replicates to quorum
3. Log durably stores command
4. Kernel applies command (pure function)
5. Projections materialize state for queries
6. Client receives acknowledgment

See [Architecture](/docs/concepts/architecture) for details.

## Key Concepts

- **[Data Model](/docs/concepts/data-model)** - Append-only logs and derived projections
- **[Consensus](/docs/concepts/consensus)** - How VSR provides fault tolerance
- **[Compliance](/docs/concepts/compliance)** - Immutability, audit trails, tamper evidence
- **[Multi-tenancy](/docs/concepts/multitenancy)** - Tenant isolation and data sovereignty
- **[Pressurecraft](/docs/concepts/pressurecraft)** - Our coding philosophy

## Current Status (v0.4 — Developer Preview)

Kimberlite is a **Developer Preview**: stable enough for prototypes, learning, internal tools, and compliance research. **Not yet battle-tested at scale.** See `README.md` §Status and `ROADMAP.md` for the v0.5 → v1.0 trajectory.

**Core Infrastructure (shipped):**
- Append-only storage with CRC32 checksums and torn write protection
- Viewstamped Replication (VSR) consensus (Normal, ViewChange, Recovery, Repair, StateTransfer, Reconfiguration)
- Cryptographic hash chains (SHA-256 for compliance paths, BLAKE3 for hot paths)
- VOPR deterministic simulation testing — 74 scenario variants (~50 substantive, ~24 scaffolded for v0.5+), 19 invariant checkers, 5 canary mutations with 100% detection

**Compliance & Security (shipped in v0.4):**
- 23 compliance frameworks modelled in TLA+ specifications (proofs PR-gated via TLC; TLAPS runs nightly)
- RBAC and ABAC access control with pre-built HIPAA, FedRAMP, PCI policies
- Field-level data masking (5 strategies)
- Automatic audit logging on all mutations (immutable by construction)
- Consent management (8 purposes, kernel-enforced)
- Right to erasure (GDPR Art. 17) with 30-day deadlines and exemptions
- Data portability export (GDPR Art. 20) with HMAC-signed bundles
- 40-finding pre-launch security audit completed

**v0.5 targets (SDK wrappers shipped; server handlers return `NotImplemented` until v0.5.0):**
- `audit_query` — structured audit trail retrieval
- `export_subject` / `verify_export` — end-to-end subject export with verification receipts
- `breach_report_indicator` / `_query_status` / `_confirm` / `_resolve` — HIPAA §164.308(a)(6) breach workflow

**Developer Experience (shipped):**
- Core SQL: DDL, DML (INSERT/UPDATE/DELETE), SELECT with aggregates, GROUP BY/HAVING, DISTINCT, UNION, INNER/LEFT JOIN, CTEs, subqueries, window functions, MVCC time-travel via `AT OFFSET`
- Client SDKs: Rust (stable), TypeScript (stable, Node 18/20/22/24), Python (beta). Go deferred post-v0.4.
- Interactive REPL
- Migration system
- Multi-tenant isolation

**Planned v0.5+ SQL:** RIGHT/FULL OUTER JOIN; `AS OF TIMESTAMP` time-travel (v0.6).
**Planned v1.0 SQL:** Transactions (`BEGIN`/`COMMIT`/`ROLLBACK`).

**Not yet certified:** No SOC 2 Type II audit, HIPAA attestation, or GDPR regulatory audit has been completed. Kimberlite provides the *substrate* for HIPAA-ready / SOC 2-ready / GDPR-ready (Art. 17 + 20) workloads; production deployments remain the operator's responsibility until v1.0. See `ROADMAP.md`.

## Getting Started

- **[Start](/docs/start)** - Get running in 2 minutes
- **[Python Client](/docs/coding/python)** - Build with Python (beta)
- **[TypeScript Client](/docs/coding/typescript)** - Build with Node.js (stable)
- **[Rust Client](/docs/coding/rust)** - Build with Rust (stable)
- **Go Client** - Deferred post-v0.4; see [ROADMAP.md](/ROADMAP.md)

## Next Steps

- **Understand the concepts:** Read [Data Model](/docs/concepts/data-model), [Consensus](/docs/concepts/consensus), and [Compliance](/docs/concepts/compliance)
- **Build something:** Follow [Coding Guides](/docs/coding)
- **Deploy:** See [Operating Guides](/docs/operating)
- **Dive deep:** Explore [Internals](/docs/internals)

---

**TL;DR:** Kimberlite is a database where compliance isn't a feature—it's the foundation. All data is an immutable log, all state is derived, and correctness is provable.
