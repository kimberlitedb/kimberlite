# Kimberlite Documentation

Welcome to Kimberlite, the **world's first database with complete 6-layer formal verification**. This documentation follows a progressive disclosure model to help you find what you need quickly.

## What Makes Kimberlite Different?

Kimberlite is the most formally verified database system ever built, with **136+ machine-checked proofs** spanning from high-level protocol specifications down to low-level code implementation:

- **Protocol Verification:** 25 TLA+ theorems + 5 Ivy Byzantine invariants proven
- **Cryptographic Verification:** 15+ Coq theorems for SHA-256, BLAKE3, AES-GCM, Ed25519
- **Code Verification:** 91 Kani bounded model checking proofs
- **Type-Level Safety:** 80+ Flux refinement type signatures (ready when Flux stabilizes)
- **Compliance Modeling:** 6 frameworks (HIPAA, GDPR, SOC 2, PCI DSS, ISO 27001, FedRAMP) with meta-framework
- **100% Traceability:** Every theorem mapped from TLA+ → Rust → VOPR tests

**→ [Learn more about Kimberlite's Formal Verification](concepts/formal-verification.md)**

## Documentation Sections

### [Start](start/) - Get Running in <10 Minutes
New to Kimberlite? Start here to get up and running quickly.

- [Quick Start](start/quick-start.md) - Fastest path to a working system
- [Installation](start/installation.md) - Installation options for all platforms
- [First Application](start/first-app.md) - Build your first healthcare compliance app

### [Concepts](concepts/) - Understanding Kimberlite
Learn the strategic "why" behind Kimberlite's design and approach.

- [Overview](concepts/overview.md) - What is Kimberlite and why does it exist?
- [**Formal Verification**](concepts/formal-verification.md) - 6-layer verification stack (unique differentiator)
- [Architecture](concepts/architecture.md) - High-level system design
- [Data Model](concepts/data-model.md) - Append-only log and projections
- [Consensus](concepts/consensus.md) - Viewstamped Replication explained
- [Compliance](concepts/compliance.md) - Compliance-first approach
- [Multi-tenancy](concepts/multitenancy.md) - Tenant isolation and security
- [Pressurecraft](concepts/pressurecraft.md) - Our coding philosophy

### [Coding](coding/) - Building Applications
Practical guides for building applications with Kimberlite.

**Quickstarts** (language-specific getting started):
- [Python](coding/quickstarts/python.md)
- [TypeScript](coding/quickstarts/typescript.md)
- [Rust](coding/quickstarts/rust.md)
- [Go](coding/quickstarts/go.md)

**Guides** (how-to guides):
- [Connection Pooling](coding/guides/connection-pooling.md)
- [Schema Migrations](coding/guides/migrations.md)
- [Testing Applications](coding/guides/testing.md)
- [Shell Completions](coding/guides/shell-completions.md)

**Recipes** (code examples for common patterns):
- [Time-Travel Queries](coding/recipes/time-travel-queries.md)
- [Audit Trails](coding/recipes/audit-trails.md)
- [Encryption](coding/recipes/encryption.md)
- [Data Classification](coding/recipes/data-classification.md)
- [Multi-Tenant Queries](coding/recipes/multi-tenant-queries.md)

### [Operating](operating/) - Deployment & Operations
Running Kimberlite in production environments.

- [Deployment](operating/deployment.md) - Docker, Kubernetes, bare metal
- [Configuration](operating/configuration.md) - Configuration reference
- [Monitoring](operating/monitoring.md) - Observability and metrics
- [Security](operating/security.md) - TLS, authentication, hardening
- [Performance](operating/performance.md) - Tuning and optimization
- [Troubleshooting](operating/troubleshooting.md) - Common issues and solutions

**Cloud Platforms:**
- [AWS](operating/cloud/aws.md)
- [GCP](operating/cloud/gcp.md)
- [Azure](operating/cloud/azure.md)

### [Reference](reference/) - API Documentation
Exhaustive reference documentation for all APIs and protocols.

**CLI Tools:**
- [VOPR](reference/cli/vopr.md) - Simulation testing tool (10 commands)

**SQL:**
- [Overview](reference/sql/overview.md) - SQL support overview
- [DDL](reference/sql/ddl.md) - CREATE/DROP TABLE/INDEX
- [DML](reference/sql/dml.md) - INSERT/UPDATE/DELETE
- [Queries](reference/sql/queries.md) - SELECT and query syntax

**SDKs:**
- [Python API](reference/sdk/python-api.md)
- [TypeScript API](reference/sdk/typescript-api.md)
- [Rust API](reference/sdk/rust-api.md)
- [Go API](reference/sdk/go-api.md)

**Protocols:**
- [Wire Protocol](reference/protocol.md) - Network protocol specification
- [Agent Protocol](reference/agent-protocol.md) - LLM agent integration

### [Internals](internals/) - Deep Technical Details
For contributors and those who want to understand how Kimberlite works internally.

**Architecture:**
- [Crate Structure](internals/architecture/crate-structure.md)
- [Kernel](internals/architecture/kernel.md)
- [Storage](internals/architecture/storage.md)
- [Cryptography](internals/architecture/crypto.md)

**Testing:**
- [Testing Overview](internals/testing/overview.md)
- [Assertions](internals/testing/assertions.md)
- [Property Testing](internals/testing/property-testing.md)

**Design Documents:**
- [Instrumentation](internals/design/instrumentation.md)
- [Reconfiguration](internals/design/reconfiguration.md)
- [LLM Integration](internals/design/llm-integration.md)
- [Data Sharing](internals/design/data-sharing.md)
- [SDK Design](internals/design/sdk.md)
- [SQL Engine Design](internals/design/sql-engine.md)

**Implementation Details:**
- [Compliance Implementation](internals/compliance-implementation.md)
- [VSR Production Gaps](internals/vsr-production-gaps.md)

---

## For Contributors

If you're contributing to Kimberlite, see [/docs-internal](../docs-internal/) for internal documentation including:
- VOPR testing infrastructure (46 scenarios, deployment, debugging)
- Contributor guides (getting started, code review, release process)
- Internal design discussions and team processes

---

## Additional Resources

- [Roadmap](../ROADMAP.md) - Future plans and version targets
- [Changelog](../CHANGELOG.md) - Release history
- [GitHub Repository](https://github.com/kimberlite-vsr/kimberlite)
- [Website](https://kimberlite.dev)
