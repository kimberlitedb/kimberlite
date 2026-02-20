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

## Current Status (v1.0.0)

Kimberlite is production-ready:

**Core Infrastructure:**
- Append-only storage with CRC32 checksums and torn write protection
- Viewstamped Replication (VSR) consensus
- Cryptographic hash chains (SHA-256, BLAKE3)
- VOPR deterministic simulation testing (46 scenarios, 19 invariants)

**Compliance & Security:**
- 23 compliance frameworks (HIPAA, GDPR, SOX, PCI-DSS, and 19 more)
- RBAC and ABAC access control
- Field-level data masking
- Automatic audit trails
- Consent management
- Breach notification tracking

**Developer Experience:**
- Full SQL support (DDL, DML, queries)
- Client SDKs (Python, TypeScript, Rust, Go)
- Interactive REPL
- Migration system
- Multi-tenant isolation

## Getting Started

- **[Start](/docs/start)** - Get running in 2 minutes
- **[Python Client](/docs/coding/python)** - Build with Python
- **[TypeScript Client](/docs/coding/typescript)** - Build with Node.js
- **[Rust Client](/docs/coding/rust)** - Build with Rust
- **[Go Client](/docs/coding/go)** - Build with Go

## Next Steps

- **Understand the concepts:** Read [Data Model](/docs/concepts/data-model), [Consensus](/docs/concepts/consensus), and [Compliance](/docs/concepts/compliance)
- **Build something:** Follow [Coding Guides](/docs/coding)
- **Deploy:** See [Operating Guides](/docs/operating)
- **Dive deep:** Explore [Internals](/docs/internals)

---

**TL;DR:** Kimberlite is a database where compliance isn't a feature—it's the foundation. All data is an immutable log, all state is derived, and correctness is provable.
