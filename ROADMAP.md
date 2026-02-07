# Roadmap

## Overview

Kimberlite is evolving from a verified, compliance-complete engine (v0.4.1) into a production-ready database that developers can install, query, and deploy. This roadmap ruthlessly prioritizes developer-facing surface area over further internal infrastructure investment.

**Core thesis: Kimberlite has built the engine but not the car.** Developers cannot install it easily, cannot write JOIN queries, cannot see their data in Studio, and cannot run it in production. The path to V1.0 fixes that.

**Current State (v0.4.1):**
- Byzantine-resistant VSR consensus with 38 production assertions
- World-class DST platform (VOPR: 46 scenarios, 19 invariant checkers, 85k-167k sims/sec)
- Formal verification specs written (TLA+, Coq, Kani, Ivy, Alloy, Flux) — **CI not yet running proofs** (see v0.4.2)
- Dual-hash cryptography (SHA-256 + BLAKE3) with hardware acceleration
- Append-only log with CRC32 checksums, segment rotation (256MB), index WAL
- B+tree projection store with MVCC and SIEVE cache
- SQL query engine (SELECT, INSERT, UPDATE, DELETE, CREATE TABLE, aggregates, GROUP BY, DISTINCT)
- Server with JWT + API key auth, TLS, Prometheus metrics (12 metrics), health checks
- Multi-language SDKs (Python, TypeScript, Go, Rust) with tests and CI
- MCP server for LLM integration (4 tools)
- Secure data sharing layer with consent ledger
- **RBAC** with 4 roles, SQL rewriting, column filtering, row-level security
- **ABAC** with 12 condition types, 3 pre-built compliance policies (HIPAA, FedRAMP, PCI DSS)
- **Field-level data masking** with 5 strategies (Redact, Hash, Tokenize, Truncate, Null)
- **Consent management** with 8 purposes, kernel-level enforcement
- **Right to erasure** (GDPR Article 17) with 30-day deadlines and exemptions
- **Breach detection** with 6 indicators and 72-hour notification deadlines
- **Data portability export** (GDPR Article 20) with HMAC-SHA256 signing
- **Enhanced audit logging** with 13 action types across all compliance modules
- Performance: hardware-accelerated crypto, zero-copy frames, TCP_NODELAY, O(1) rate limiter, batch index writes
- **CI/CD: all 6 workflows failing** — clippy errors, missing paths, broken doc-tests, placeholder verification steps (see v0.4.2)

**Compliance Coverage (v0.4.3 target):**
- 22 frameworks formally specified across USA, EU, Australia, and cross-region (legal)
- HIPAA, HITECH, 21 CFR Part 11, SOX, GLBA, PCI DSS, CCPA/CPRA, FERPA, SOC 2, FedRAMP, NIST 800-53, CMMC, Legal Compliance, GDPR, NIS2, DORA, eIDAS, ISO 27001, Privacy Act/APPs, APRA CPS 234, Essential Eight, NDB, IRAP: **all 100%**

**Vision:**
Transform Kimberlite from a verified engine into a complete, accessible database product. Prioritize developer experience and distribution before performance optimization — nobody hits I/O bottlenecks if they cannot install the binary or write a JOIN query.

---

## Release Timeline

### v0.4.2 — CI/CD Health (Target: Feb 2026)

**Theme:** Get CI green. *"No feature work until the build is honest."*

All 6 CI/CD workflows currently fail. The Formal Verification workflow appears green but is a false positive — it uses placeholder `echo` statements and `continue-on-error: true`. This milestone is the immediate priority before any v0.5.0 feature work begins.

#### CI Workflow (`ci.yml`) — 7 fixes needed

| Issue | Location | Details |
|---|---|---|
| **Clippy: long literal separators** | `crates/kimberlite-crypto/src/verified/{aes_gcm,blake3,ed25519,key_hierarchy,sha256,proof_certificate}.rs` | 32 instances of large numeric literals lacking `_` separators |
| **Clippy: missing backticks in docs** | crypto + compliance crates | 4 doc-comment lint violations |
| **Clippy: uninlined format args** | sim crate | 2 instances |
| **Clippy: function too many lines** | `crates/kimberlite-compliance/src/report.rs:139` | Single function exceeds line limit |
| **Clippy: assert!(true)** | `crates/kimberlite-compliance/src/lib.rs:588` | No-op assertion |
| **Compilation error (`--all-features`)** | `crates/kimberlite-sim/tests/sim_canary_integration.rs` | Undeclared `SimStorage` / `StorageConfig` types |
| **55/58 doc-test failures** | `crates/kimberlite-doc-tests/` | See "Doc-Test Breakdown" below |
| **Unused dependencies** | Workspace `Cargo.toml` files | `askama_axum`, `kimberlite-sim-macros` in sim; `base64` in kimberlite |

#### Benchmarks (`bench.yml`) — 1 fix needed

| Issue | Location | Details |
|---|---|---|
| **Deprecated `criterion::black_box`** | `crates/kimberlite-bench/benches/{crypto,storage,kernel,wire,end_to_end}.rs` | Replace with `std::hint::black_box()` in all 5 bench files |

#### Build FFI (`build-ffi.yml`) — depends on CI fixes

- Depends on CI workflow passing first (FFI won't build if upstream crates have clippy errors with `-D warnings`)
- Python SDK: 15-job test matrix (5 Python × 3 OS) — needs FFI to build clean
- TypeScript SDK: 9-job test matrix (3 Node × 3 OS) — needs FFI to build clean
- Verify `target/kimberlite-ffi.h` C header generation

#### Documentation (`docs.yml`) — 4 fixes needed

| Issue | Location | Details |
|---|---|---|
| **Missing `docs/guides/*.md`** | `docs.yml` | Directory restructured — guides now live under `docs/coding/guides/` |
| **Missing `docs/SDK.md`** | `docs.yml` | File does not exist; SDK docs are in `sdks/` |
| **Missing `docs/PROTOCOL.md`** | `docs.yml` | File does not exist; protocol docs are in `docs/reference/protocol.md` |
| **Fix workflow paths** | `.github/workflows/docs.yml` | Update all path references to match current `docs/` directory structure |

#### Security (`security.yml`) — 2 fixes needed

| Issue | Location | Details |
|---|---|---|
| **Fragile SBOM generation** | `security.yml` | `find . -name "bom.json" \| head -1 \| xargs mv` fails silently if no file found |
| **Advisory ignores** | `security.yml` | Currently ignores RUSTSEC-2025-0141, RUSTSEC-2025-0134 — review and update as advisories evolve |

#### VOPR Nightly (`vopr-nightly.yml`) — 2 fixes needed

| Issue | Location | Details |
|---|---|---|
| **Missing script** | `vopr-nightly.yml:72` | References `scripts/validate-coverage.py` which does not exist (no `scripts/` directory) |
| **Git commit to repo** | `vopr-nightly.yml` | `vopr-trends/` commit step lacks permissions and violates clean root principle — move to `.artifacts/` or remove |

#### Formal Verification (`formal-verification.yml`) — 4 fixes needed

| Issue | Location | Details |
|---|---|---|
| **TLAPS placeholder** | `formal-verification.yml` | Steps are `echo` statements, not actual proof checking — create Docker env in `tools/formal-verification/docker/tlaps/`, run real TLAPS |
| **Alloy 6 syntax errors** | `specs/alloy/HashChain.als`, `specs/alloy/Quorum.als` | Replace `sub[a,b]` → `a.minus[b]`, `add[a,b]` → `a.plus[b]`, `div[a,b]` → `a.div[b]` for Alloy 6 |
| **Ivy installation fragile** | `formal-verification.yml` | Pin Ivy version, use Docker image from `tools/formal-verification/docker/ivy/Dockerfile` |
| **`continue-on-error: true` masking failures** | `formal-verification.yml` | Remove from all steps that should actually fail CI — currently gives false green status |

#### Doc-Test Breakdown (55 failures in `kimberlite-doc-tests`)

| Source file | Failures | Root cause |
|---|---|---|
| `encryption.md` | 12 | `SymmetricKey` doesn't exist (should be `EncryptionKey`), missing `base64` dep |
| `audit-trails.md` | 17 | References non-existent `Client` API, custom types not defined |
| `multi-tenant-queries.md` | 11 | References non-existent `Client` API, `GrantConfig`, `Permissions` |
| `data-classification.md` | 10 | `DataClassification` custom type invented by docs (actual type is `DataClass`) |
| `quickstarts/rust.md` | 8 | `tenant` variable undefined, missing imports |
| `time-travel-queries.md` | 4 | Missing `chrono` dep, incomplete code blocks |
| `guides/migrations.md` | 2 | Unicode box-drawing characters in code block |

#### Expected Impact

- CI goes from 0/7 green to 7/7 green
- Formal verification provides real signal (failures are real failures)
- Doc examples actually compile — contributors can trust the documentation
- Feature branches get honest feedback on every PR

---

### v0.4.3 — Compliance Framework Expansion (Target: Mar-Apr 2026)

**Theme:** Complete the moat. *"Compliance isn't a feature — it's the product."*

Kimberlite's core differentiator is compliance-by-construction backed by formal verification. This milestone expands coverage from 6 to 22 formally specified frameworks, fixes the documentation honesty gap, and ensures every framework is at 100%. The meta-framework approach (MetaFramework.tla) makes this efficient: 7 core properties already cover the database-layer requirements for most new frameworks.

**Target Coverage After v0.4.3:**

| Region | Framework | Vertical | Status |
|---|---|---|---|
| **USA** | HIPAA | Healthcare | 100% (existing) |
| **USA** | HITECH | Healthcare | 100% (new) |
| **USA** | 21 CFR Part 11 | Pharma/Medical Devices | 100% (new, requires ElectronicSignatureBinding) |
| **USA** | SOX | Finance | 100% (new) |
| **USA** | GLBA | Finance | 100% (new) |
| **USA** | PCI DSS | Finance/Retail | 100% (complete from 95%) |
| **USA** | CCPA/CPRA | All (California) | 100% (new) |
| **USA** | FERPA | Education | 100% (new) |
| **USA** | SOC 2 | Technology/Services | 100% (complete from 95%) |
| **USA** | FedRAMP | Government/Cloud | 100% (complete from 90%) |
| **USA** | NIST 800-53 | Government | 100% (new) |
| **USA** | CMMC | Defense Contractors | 100% (new) |
| **Cross-region** | Legal Compliance | Legal | 100% (new — legal hold, chain of custody, eDiscovery, ABA ethics) |
| **EU** | GDPR | All | 100% (existing) |
| **EU** | NIS2 | Critical Infrastructure | 100% (new) |
| **EU** | DORA | Finance | 100% (new) |
| **EU** | eIDAS | All (Digital Identity) | 100% (new, requires QualifiedTimestamping) |
| **EU** | ISO 27001 | All | 100% (complete from 95%) |
| **Australia** | Privacy Act/APPs | All | 100% (new) |
| **Australia** | APRA CPS 234 | Finance/Insurance | 100% (new) |
| **Australia** | Essential Eight | Government | 100% (new) |
| **Australia** | NDB Scheme | All | 100% (new) |
| **Australia** | IRAP | Government | 100% (new) |

#### Phase 0: Fix Documentation Honesty Gap (1-2 days)

| Task | Files | Details |
|---|---|---|
| **Correct overclaimed coverage** | `docs/concepts/compliance.md` | Change "Full" to "Architecturally Compatible" for unspecified frameworks; distinguish formally verified from compatible |
| **Update certification readiness** | `docs/compliance/certification-package.md` | Align percentages with actual formal specification status |

#### Phase 1: Complete Existing 6 Frameworks to 100% (1-2 weeks)

| Framework | Gap | Fix | Files |
|---|---|---|---|
| **SOC 2** (95→100%) | CC7.4 (Backup/Recovery) proof omitted; SLA metrics missing | Complete TLAPS proof; add SLA metrics to report | `specs/tla/compliance/SOC2.tla`, `crates/kimberlite-compliance/src/{lib,report}.rs` |
| **PCI DSS** (95→100%) | Req 4 (Transmission) not formally specified; tokenization audit trail | Add Req 4 predicate; add `TokenizationApplied` audit action | `specs/tla/compliance/PCI_DSS.tla`, `crates/kimberlite-compliance/src/{lib,audit}.rs` |
| **ISO 27001** (95→100%) | Security metrics reporting; FIPS 140-2 reference | Add metrics section; reference `IsFIPSValidated` from FedRAMP spec | `specs/tla/compliance/ISO27001.tla`, `crates/kimberlite-compliance/src/{lib,report}.rs` |
| **FedRAMP** (90→100%) | CM-2/CM-6 proofs; location audit trail; expand requirements from 3→12 | Complete TLAPS proofs; add `source_country` to audit events | `specs/tla/compliance/FedRAMP.tla`, `crates/kimberlite-compliance/src/{lib,audit}.rs` |

#### Phase 2: USA Frameworks — Mapping Only (2-3 weeks)

Each framework follows the pattern: TLA+ spec → ComplianceFramework variant → requirements fn → ABAC policy → MetaFramework update → docs.

| Framework | Key Implementation Details |
|---|---|
| **HITECH** | Extends HIPAA.tla; add minimum necessary field-level access ABAC policy; leverage existing breach module for 60-day notification (already stricter at 72h) |
| **CCPA/CPRA** | Map to consent + erasure + export modules; add data correction workflow ABAC condition; add `DataCorrectionAllowed` condition type |
| **GLBA** | Safeguards Rule maps to EncryptionAtRest + AccessControlEnforcement; 30-day breach notification to FTC |
| **SOX** | Sections 302/404 map to AuditCompleteness + HashChainIntegrity; add `RetentionPeriodAtLeast(2555)` for 7-year retention enforcement |
| **FERPA** | Student data privacy maps to TenantIsolation + AccessControlEnforcement; minimal new work |
| **NIST 800-53** | Control families AC/AU/SC/SI map to existing properties; extends FedRAMP patterns (FedRAMP is based on 800-53) |
| **CMMC** | NIST 800-171 derivative; maps same core properties; 3-level maturity model documentation |
| **Legal Compliance** | Formal spec for legal hold (prevent deletion during litigation), chain of custody (tamper-evident evidence trail via HashChainIntegrity), eDiscovery (searchable audit logs via AuditCompleteness), professional ethics (access controls + audit); add `LegalHoldActive` ABAC condition |

#### Phase 3: New Core Properties — 21 CFR Part 11 & eIDAS (3-4 weeks)

| Framework | New Core Property | Implementation |
|---|---|---|
| **21 CFR Part 11** | `ElectronicSignatureBinding` — per-record Ed25519 signature linking (FDA requires signatures bound to records, non-transferable) | New module `crates/kimberlite-compliance/src/signature_binding.rs`; `RecordSignature` struct with `SignatureMeaning` enum (Authorship, Review, Approval); `RecordSigned` audit action; `OperationalSequencing` ABAC condition for review-then-approve workflows |
| **eIDAS** | `QualifiedTimestamping` — RFC 3161 timestamps from qualified Trust Service Provider | New module `crates/kimberlite-compliance/src/qualified_timestamp.rs`; `TimestampToken` struct; follows FCIS (pure core validates, impure shell calls TSP); may need `rfc3161` workspace dependency |

Add `ExtendedComplianceSafety` to `ComplianceCommon.tla` (core + new properties) while keeping `CoreComplianceSafety` unchanged for backward compatibility.

#### Phase 4: EU & Australia Frameworks — Mapping Only (2-3 weeks)

| Framework | Key Implementation Details |
|---|---|
| **NIS2** | Article 21 security requirements; add `IncidentReportingDeadline(24)` ABAC condition for 24h early warning cadence; leverage breach module |
| **DORA** | ICT risk management maps to HashChainIntegrity + AuditCompleteness; resilience testing covered by VOPR (49 scenarios) |
| **Privacy Act/APPs** | 13 Australian Privacy Principles; maps to consent + erasure + export + access control; add data correction workflow |
| **APRA CPS 234** | Financial regulator; maps closely to ISO 27001; 72h incident notification (matches existing breach module deadline) |
| **Essential Eight** | ASD maturity model; admin privilege restriction maps to AccessControlEnforcement; document items outside DB scope (MFA, patching) |
| **NDB Scheme** | Mandatory breach notification; add `AssessmentPeriodTimer(30)` for 30-day assessment window; leverage breach module |
| **IRAP** | ISM controls for government data; extends FedRAMP patterns; data classification mapping to ISM levels |

#### Phase 5: Infrastructure & Documentation (1-2 weeks)

| Task | Details |
|---|---|
| **New ABAC conditions** | Add 6 new condition types to `Condition` enum: `RetentionPeriodAtLeast`, `DataCorrectionAllowed`, `IncidentReportingDeadline`, `FieldLevelRestriction`, `OperationalSequencing`, `LegalHoldActive` |
| **MetaFramework restructuring** | Extend `AllFrameworksCompliant` from 6→22 frameworks; add dependency mappings; add `ExtendedComplianceSafety` |
| **Certification package update** | Add framework-specific sections to `docs/compliance/certification-package.md` for all 22 frameworks |
| **Compliance concepts update** | Rewrite `docs/concepts/compliance.md` with complete 22-framework table organized by region and vertical |

#### Expected Impact

- Compliance coverage: 6 frameworks → **22 frameworks** at **100%** each
- Geographic coverage: USA-only → **USA + EU + Australia + cross-region (legal)**
- Vertical coverage: Healthcare + Generic → **Healthcare, Finance, Legal, Government, Defense, Education, Pharma, Critical Infrastructure**
- New TLA+ specifications: **16 new formal specs**
- New core properties: **2** (ElectronicSignatureBinding, QualifiedTimestamping)
- New ABAC policies: **16 pre-built compliance policies**
- Marketing: "The only database with formal verification for 22 compliance frameworks"

---

### v0.5.0 — Developer Experience & SQL Completeness (Target: Q2 2026)

**Theme:** Make it usable. *"Can I evaluate Kimberlite?"*

This release fills the critical gaps between "engine works" and "developer can build something." Every item unblocks real evaluation by real developers.

| Deliverable | Key Files | Impact |
|---|---|---|
| **SQL JOINs** (INNER, LEFT) | `kimberlite-query/src/{parser,planner,executor}.rs` | Unblocks real-world queries |
| **SQL HAVING, subqueries, CTEs, UNION** | Same files — remove explicit rejections | SQL completeness for analytics |
| **ALTER TABLE** (add/drop column) | `kimberlite-query/src/parser.rs` | Schema evolution |
| **Migration apply** | `kimberlite-migration/src/lib.rs`, CLI `migration.rs` | Schema management actually works |
| **Dev server actually starts** | `kimberlite-dev/src/{lib,server}.rs` | `kmb dev` does something |
| **Studio query execution** | `kimberlite-studio/src/routes/{api,sse}.rs` | Visual data browsing works |
| **REPL improvements** | `kimberlite-cli/src/commands/repl.rs` | Syntax highlighting, tab completion |
| **Finance example** | `examples/finance/` | Trade audit trail with SEC compliance |
| **Legal example** | `examples/legal/` | Chain of custody with immutable evidence |
| **Kernel effect handlers** | `kimberlite-kernel/src/runtime.rs` | Projection, audit, table/index metadata handlers needed for Studio + dev server |
| **BYTES data type in SQL parser** | `kimberlite/src/kimberlite.rs`, `kimberlite-query/src/parser.rs` | SQL completeness — type exists in enum but not parseable |
| **Migration rollback** | `kimberlite-cli/src/commands/migration.rs` | Schema management completeness |
| **VOPR subcommands** | `kimberlite-sim/src/bin/vopr.rs` | repro, show, timeline, bisect, minimize, dashboard, stats — all print "not yet implemented" |

**What's actually broken today:**
- `kimberlite-dev/src/lib.rs:91`: `// TODO: Actually start the server` — dev server prints messages but starts nothing
- `kimberlite-studio/src/routes/api.rs`: `// TODO: Execute query via kimberlite_client` — Studio cannot run queries
- `kimberlite-query/src/parser.rs:331`: JOINs explicitly rejected
- `kimberlite-query/src/parser.rs:281`: CTEs explicitly rejected
- `kimberlite-query/src/parser.rs:288`: Subqueries explicitly rejected
- `kimberlite-query/src/parser.rs:372`: HAVING explicitly rejected
- `kimberlite-migration/src/lib.rs`: Has create/list/validate but no `apply()` method
- `kimberlite-kernel/src/runtime.rs:92-108`: 6 effect handlers are no-op stubs (projection, audit, table/index metadata)
- `kimberlite/src/kimberlite.rs:1303`: BYTES type exists in DataType enum but not parseable in SQL
- `kimberlite-sim/src/bin/vopr.rs:2562`: 7 VOPR CLI subcommands print "not yet implemented"
- 30+ TODO comments across CLI tenant management, cluster ops, migration apply

**Expected Impact:**
- Developers can write real SQL against Kimberlite (JOINs, aggregates, subqueries)
- `kmb dev` starts a working local server with Studio
- Schema migrations can be applied, not just listed
- Three vertical examples (healthcare, finance, legal) demonstrate compliance use cases

---

### v0.6.0 — Distribution & SDK Publishing (Target: Q3 2026)

**Theme:** Make it accessible. *"Can I install Kimberlite in under 60 seconds?"*

The biggest adoption barrier is distribution. Release workflow already builds 5-platform binaries (Linux x86/ARM, macOS x86/ARM, Windows) and SDKs exist with tests — they just aren't published anywhere.

| Deliverable | Details | Impact |
|---|---|---|
| **Install script** | `curl -fsSL https://kimberlite.dev/install.sh \| sh` | Zero-friction install |
| **Homebrew** | `brew install kimberlitedb/tap/kimberlite` | macOS developers |
| **Docker image** | `docker pull ghcr.io/kimberlitedb/kimberlite` | Container users |
| **Python SDK on PyPI** | `pip install kimberlite` | Largest non-Rust audience |
| **TypeScript SDK on npm** | `npm install @kimberlite/client` | Web developers |
| **Go SDK published** | `go get github.com/kimberlitedb/kimberlite-go` | Enterprise developers |
| **Scaffolding** | `kmb init --template healthcare` | Project setup in seconds |
| **Website: install page** | Platform-specific tabs (curl, brew, docker, cargo) | Reduces confusion |
| **Website: comparison pages** | vs PostgreSQL, vs TigerBeetle, vs CockroachDB | Decision support |
| **Website: playground** | Browser-based REPL (WASM or hosted) | Try without install |

**Expected Impact:**
- Time to first query drops from "clone + cargo build" to under 60 seconds
- Python, TypeScript, and Go developers can use Kimberlite natively
- `kmb init` scaffolds compliance-ready projects

---

### v0.7.0 — Runtime Integration & Operational Maturity (Target: Q4 2026)

**Theme:** Make it reliable. *"Can I deploy Kimberlite to staging?"*

Many of these capabilities already exist in code but aren't wired to endpoints or CLI commands.

| Deliverable | Key Files | Impact |
|---|---|---|
| **Metrics HTTP endpoint** | `kimberlite-server/src/server.rs` — expose `/metrics` | Prometheus monitoring |
| **Health check endpoints** | Wire existing `health.rs` to `/health`, `/ready` | Kubernetes readiness |
| **Auth wiring** | `kimberlite-server/src/handler.rs` — validate tokens on handshake | Security enforcement |
| **VSR standby state application** | `kimberlite-vsr/` — apply ops, not just track commit number | Replication works end-to-end |
| **Client session idempotency** | `client_id` + `request_number` tracking | Exactly-once semantics |
| **Backup/restore** | Point-in-time snapshot + incremental | Data safety |
| **Tenant management CLI** | Wire `kmb tenant create/list/delete/info` to real APIs | Multi-tenant operations |
| **OpenTelemetry traces** | Distributed tracing across server + kernel | Observability |
| **Cluster node spawning & status** | `kimberlite-cli/src/commands/cluster.rs`, `kimberlite-cluster/src/node.rs` | Cluster operations — node process uses placeholder command |
| **Stream listing CLI** | `kimberlite-cli/src/commands/stream.rs` | Data discovery — requires server-side support |
| **VSR clock synchronization** | `kimberlite-vsr/src/replica/normal.rs` | Latency-aware replica selection via prepare send times or ping/pong |
| **Compliance certificate Ed25519 signing** | `kimberlite-compliance/src/certificate.rs` | Real cryptographic attestation — currently SHA-256 placeholder |
| **MCP tool implementations** | `kimberlite-mcp/src/handler.rs` | Hash verification and encryption are placeholders |

**What exists but isn't wired:**
- `kimberlite-server/src/metrics.rs`: 12 Prometheus metrics defined — not exposed via HTTP
- `kimberlite-server/src/health.rs`: Health checks implemented — not routed to endpoint
- `kimberlite-server/src/auth.rs`: JWT + API key auth module — token not validated during handshake
- `kimberlite-cluster/src/node.rs:59`: Node spawning uses placeholder command
- `kimberlite-compliance/src/certificate.rs:259`: Certificate signing uses SHA-256 placeholder instead of Ed25519
- `kimberlite-mcp/src/handler.rs:291,554`: Hash verification and encryption are placeholders

**Expected Impact:**
- Kimberlite can be deployed behind a load balancer with health checks
- Prometheus + Grafana dashboards work out of the box
- Auth is enforced, not just parsed
- Replication actually replicates data

---

### v0.8.0 — Performance & Advanced I/O (Target: Q1 2027)

**Theme:** Make it fast. *"Can Kimberlite handle our production workload?"*

Performance optimization comes after usability and distribution — developers must be using Kimberlite before bottlenecks matter.

| Deliverable | Details |
|---|---|
| **io_uring abstraction layer** | `kimberlite-io` crate, Linux 5.6+, sync fallback for macOS/Windows |
| **Thread-per-core runtime** | Pin streams to cores, per-core Storage/State/event loop |
| **Direct I/O for append path** | Bypass kernel page cache for write-heavy workloads |
| **Bounded queues with backpressure** | Little's Law-sized queues prevent OOM |
| **Log compaction** | Reclaim space from compacted segments |
| **Compression** | LZ4 (fast) and Zstd (high ratio) codecs |
| **Stage pipelining** | Overlap I/O, crypto, and state transitions |
| **Zero-copy deserialization** | Eliminate redundant copies on read path |
| **VSR write reordering repair** | `kimberlite-vsr/src/simulation.rs` — repair protocol for gaps caused by write reordering |
| **Java SDK published** | Maven Central distribution |

**Target Metrics:**

| Metric | Current Estimate | Target |
|---|---|---|
| Append throughput | ~10K events/sec | 200K+ events/sec |
| Read throughput | ~5 MB/s | 100+ MB/s |
| Append p99 latency | Unmeasured | <1ms |
| Context switches | High | Near zero (thread-per-core) |

---

### v0.9.0 — Production Hardening (Target: Q2 2027)

**Theme:** Make it production-ready. *"Can I trust Kimberlite with real data?"*

| Deliverable | Details |
|---|---|
| **Graceful shutdown / rolling upgrades** | Zero-downtime deployments |
| **Dynamic cluster reconfiguration** | Add/remove replicas without downtime |
| **Hot shard migration** | Rebalance tenants across nodes |
| **Tag-based rate limiting** | QoS per tenant (FoundationDB pattern) |
| **Subscribe operation** | Real-time event streaming (server-push) |
| **~~SOC 2 / PCI DSS / FedRAMP to 100%~~** | Moved to v0.4.3 |
| **Third-party security audit** | Independent verification of security posture |
| **Operational runbooks** | Playbooks for common failure modes |
| **Stream retention policies** | Automatic data deletion for compliance (HIPAA, GDPR) |
| **ML-based data classification** | `kimberlite-kernel/src/classification.rs` — content-based automated compliance tagging |
| **VOPR phase tracker assertions** | `kimberlite-sim/src/instrumentation/phase_tracker.rs` — execute triggered assertions for deeper validation |
| **VOPR fault registry integration** | `kimberlite-sim/src/instrumentation/fault_registry.rs` — integrate with SimFaultInjector for smarter injection |

---

### v1.0.0 — GA Release (Target: Q3 2027)

**Theme:** Make it official. *"Kimberlite is production-ready."*

| Deliverable | Details |
|---|---|
| **Wire protocol freeze** | Backwards-compatible from v1.0 onward |
| **Storage format freeze** | On-disk format stability guarantee |
| **API stability guarantees** | Semantic versioning for all public APIs |
| **Published benchmarks** | vs PostgreSQL for compliance workloads |
| **Third-party checkpoint attestation** | RFC 3161 TSA integration |
| **Academic paper** | Target: OSDI/SOSP/USENIX Security 2027 |
| **All compliance frameworks at 100%** | 22 frameworks complete (moved to v0.4.3) |
| **Migration guide** | Clear upgrade path from 0.x to 1.0 |
| **Complete vertical examples** | Healthcare, finance, legal — all production-quality |

---

## Post V1.0 — Managed Cloud Platform

**Theme:** Monetize operational burden, not features.

All V1.0 features remain Apache 2.0 open source forever. The cloud platform charges for operational convenience.

| Capability | Monetization Model |
|---|---|
| Managed clusters (provisioning, scaling, auto-healing) | Usage-based (compute + storage) |
| Global multi-region replication | Per-region pricing |
| Compliance automation (scheduled reports, SOC 2 evidence packages) | Per-report / tier-based |
| SSO/SAML/OIDC enterprise identity | Per-seat |
| Managed backups with point-in-time recovery | Storage-based |
| SLA guarantees (99.99% uptime) | Tier-based |
| Priority support (dedicated engineer, 1-hour response) | Per-seat |
| Team management and collaboration | Per-seat |
| Usage-based billing | Pay-as-you-go |

---

## OSS Core vs Managed Cloud Split

### Apache 2.0 OSS Core (everything in V1.0)

**Key principle:** Compliance features MUST stay in OSS. Gating compliance behind a paywall destroys trust for a compliance-first database.

| Category | Components |
|---|---|
| **Database Engine** | Kernel, Storage, Crypto, VSR consensus |
| **Query Engine** | Full SQL (SELECT, INSERT, UPDATE, DELETE, JOIN, GROUP BY, aggregates, subqueries, CTEs) |
| **Compliance** | ALL modules: RBAC, ABAC, masking, erasure, breach, audit, export, consent |
| **Access Control** | JWT, API keys, row-level security, column filtering |
| **SDKs** | Rust, Python, TypeScript, Go, Java |
| **CLI + REPL** | All 13+ commands |
| **Studio** | Web GUI for queries and schema browsing |
| **Migration** | SQL migration files, validation, tracking, apply |
| **Dev Server** | Single-command local development |
| **Docker** | Official images, Compose for dev + cluster |
| **Monitoring** | Prometheus metrics, health checks, OpenTelemetry |
| **Formal Verification** | All TLA+, Coq, Kani, Alloy, Ivy specs + proofs |
| **Testing** | VOPR (46 scenarios), property testing, fuzzing |
| **MCP Server** | LLM integration (4 tools) |
| **Clustering** | VSR replication, single-node + multi-node |

### Managed Cloud (Post V1.0, revenue)

| Category | What You Pay For |
|---|---|
| Managed clusters | We handle provisioning, scaling, patching, failover |
| Global replication | Multi-region with automatic conflict resolution |
| Compliance automation | Scheduled audit reports, evidence package generation |
| Enterprise identity | SSO/SAML/OIDC integration |
| Managed backups | Automated PITR with configurable retention |
| SLA guarantees | Contractual uptime commitments |
| Priority support | Dedicated engineer, 1-hour response time |

---

## Items Deferred from Previous ROADMAP

Items moved later based on adoption-first prioritization:

| Item | Previous Version | Now At | Rationale |
|---|---|---|---|
| io_uring abstraction | v0.6.0 | v0.8.0 | Nobody hits I/O bottlenecks if they can't install |
| Thread-per-core runtime | v0.6.0 | v0.8.0 | Performance optimization before usability |
| Direct I/O for append | v0.5.0 | v0.8.0 | Not blocking adoption |
| VOPR attack completions | v0.5.0 | v0.9.0 | 46 scenarios already excellent |
| Zero-copy deserialization | v0.6.0 | v0.8.0 | Performance optimization |
| Blockchain anchoring | v1.0.0 | Post-1.0 | Enterprise/cloud feature |
| HashMap optimization | v0.3.0 | v0.8.0 | Deferred — benchmarks needed to justify |
| Bounded queues | v0.5.0 | v0.8.0 | Runtime optimization, not adoption blocker |

## Items Promoted Earlier

Items moved earlier because they unblock adoption:

| Item | Previous Version | Now At | Rationale |
|---|---|---|---|
| CI/CD health (all workflows green) | Not scheduled | v0.4.2 | Must be green before any feature work |
| SQL JOINs / HAVING / subqueries | Not scheduled | v0.5.0 | Blocking for any real usage |
| Studio query execution | Not scheduled | v0.5.0 | Core DX tool is non-functional |
| Dev server implementation | Not scheduled | v0.5.0 | Core DX tool is non-functional |
| Migration apply | Not scheduled | v0.5.0 | Schema management incomplete |
| Binary distribution (curl, brew, docker) | Not scheduled | v0.6.0 | Biggest adoption barrier |
| SDK publishing (PyPI, npm, go get) | Not scheduled | v0.6.0 | Second biggest adoption barrier |
| Metrics HTTP endpoint | v0.5.0 | v0.7.0 | Already implemented, just needs wiring |
| Auth wiring | v0.5.0 | v0.7.0 | Already implemented, just needs wiring |
| Compliance framework expansion (6→22) | v0.9.0/v1.0.0 | v0.4.3 | Core product differentiator; meta-framework makes it tractable |
| SOC 2 / PCI DSS / FedRAMP to 100% | v0.9.0 | v0.4.3 | Prerequisite for compliance expansion |

---

## Completed Milestones

### Formal Verification — 6-Layer Defense-in-Depth (Complete: Feb 5, 2026)

**Achievement:** World's first database with complete 6-layer formal verification.

| Phase | Deliverables | Tools |
|---|---|---|
| **Phase 1: Protocol Specs** | 25 TLA+ theorems, 5 Ivy invariants, Alloy models | TLA+, TLAPS, Ivy, Alloy |
| **Phase 2: Crypto Proofs** | 5 specs, 31 theorems proven | Coq |
| **Phase 3: Code Verification** | 91 Kani proofs (100% passing) | Kani |
| **Phase 4: Type Enforcement** | 80+ Flux refinement signatures (documented) | Flux |
| **Phase 5: Compliance Modeling** | 8 TLA+ specs, compliance reporter | TLA+ |
| **Phase 6: Integration** | 100% traceability (19/19 theorems) | All |

**Total: 136+ proofs across 7 verification tools.**

**Documentation:**
- User guide: `docs/concepts/formal-verification.md`
- Technical details: `docs-internal/formal-verification/implementation-complete.md`
- Internals: `docs/internals/formal-verification/protocol-specifications.md`
- Traceability: `docs/traceability_matrix.md`

---

### v0.4.1 — Full Compliance Feature Set (Released: Feb 6, 2026)

- Field-level data masking (5 strategies, 724 LOC, 20 tests)
- Right to erasure / GDPR Article 17 (739 LOC, 11 tests)
- Breach detection / HIPAA § 164.404 (1000 LOC, 15 tests)
- Data portability / GDPR Article 20 (604 LOC, 10 tests)
- Enhanced audit logging / 13 action types (999 LOC, 12 tests)
- ABAC crate (1376 LOC, 35 tests)
- Consent + RBAC kernel integration (11 integration tests)
- VSR production readiness (repair budget, log scrubbing, standby)
- Optimistic concurrency control wired through append protocol
- Performance Phases 1-5 (crypto accel, segment rotation, SIEVE cache, zero-copy frames)
- Kani proof compilation fixes (91 errors → 0)

**Total: ~5,442 LOC, 109 tests, 7 documentation pages**

### v0.4.0 — VOPR Advanced Debugging (Released: Feb 3, 2026)

- Timeline visualization (ASCII Gantt, 11 event kinds)
- Bisect to first bad event (O(log n) convergence)
- Delta debugging (80-95% test case reduction)
- Real kernel state hash (BLAKE3, not placeholder)
- Coverage dashboard (Axum + SSE, 4 dimensions)
- Interactive TUI (ratatui, 3 tabs)

**Total: ~3,700 LOC, 51 tests**

### v0.3.x — VOPR Enhanced Testing (Released: 2025)

- VOPR VSR Mode — protocol-level Byzantine testing (~3,000 LOC)
- VOPR Enhancements — storage realism, Byzantine attacks, workloads (~3,400 LOC)
- 10 protocol-level attack patterns, 100% detection rate
- .kmb failure reproduction bundles
- Coverage-guided fuzzing
- Beautiful CLI (run, repro, show, scenarios, stats)

### Earlier Releases

See `CHANGELOG.md` for complete release history (v0.1.0 through v0.2.0).

---

## Protocol Enhancements

**See Also:** Wire protocol is specified in `docs/reference/protocol.md`

### Priority 1: Critical for Production (v0.7.0)

- **Rich event metadata in ReadEvents** — Return structured `Event` objects with offset, timestamp, checksum. Better SDK ergonomics and integrity verification.
- **Stream retention policies** — Add `retention_days` to `CreateStreamRequest`. Automatic data deletion for compliance (HIPAA, GDPR). Background compaction enforcement.

### Priority 2: Enhanced Functionality (v0.9.0)

- **Subscribe operation (real-time streaming)** — Server-initiated push for event streaming. Consumer group coordination. Credit-based flow control.
- **Checkpoint operation** — Create immutable point-in-time snapshots. Integration with `QueryAt` for audits. S3/object storage archival.
- **DeleteStream operation** — Soft-delete with compliance retention period. Physical deletion deferred. Audit trail preserved.

### Priority 3: Performance & Scale (v0.8.0+)

- **Compression support** — LZ4 (fast) and Zstd (high ratio). Negotiated during handshake.
- **Batch query operation** — Multiple SQL statements per request. Reduces round-trips for analytics.
- **Streaming read** — Server-push for large result sets. Avoids OOM with 16 MiB frame limit.

---

## Content & Adoption Strategy

### Blog Posts (aligned with releases)

- **v0.5.0:** "Building a SQL Query Engine in Rust: JOINs and B+Tree Indexes"
- **v0.5.0:** "Why Every Database Needs Formal Verification (And How We Proved 136 Properties)"
- **v0.6.0:** "Zero to Compliance Queries in 60 Seconds"
- **v0.7.0:** "How VOPR Found 47 Bugs Before Our Users Did"
- **v0.8.0:** "Achieving 200K Events/sec While Maintaining Audit Trails"

### Examples (v0.5.0+)

- `examples/healthcare/` — HIPAA-compliant patient records (exists, to be expanded)
- `examples/finance/` — Trade audit trail with SEC compliance
- `examples/legal/` — Chain of custody with immutable evidence tracking
- `examples/multi-tenant/` — Tenant isolation with ABAC policies

### Video Content (v0.6.0+)

- "Kimberlite in 5 Minutes" product demo
- "Building a HIPAA-Compliant App" tutorial series
- "Deterministic Simulation Testing Explained"

### Website Improvements

1. **Installation page** with platform tabs (curl, brew, docker, cargo)
2. **Comparison pages** ("Kimberlite vs PostgreSQL for Compliance", etc.)
3. **Interactive playground** (browser-based REPL)
4. **Pricing page** ("OSS is free forever. Cloud coming 2027.")
5. **Formal verification showcase** — dedicated page with visual proof explorer
6. **Doc freshness CI** — workflow that validates docs match code

---

## Verification Plan

After each release:
1. Run `just pre-commit` to verify no formatting/lint issues
2. Verify all file references in roadmap point to real paths
3. Confirm version ordering makes logical sense (dependencies flow correctly)
4. Cross-reference with `CHANGELOG.md` to ensure completed items are accurately reflected
5. Verify the OSS vs Cloud split clearly keeps compliance features in OSS
