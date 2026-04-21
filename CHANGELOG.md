# Changelog

All notable changes to Kimberlite are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Kimberlite uses [Semantic Versioning](https://semver.org/). Breaking
changes are called out in a `### Breaking changes` section at the top
of each release block — see [`VERSIONING.md`](./VERSIONING.md) for the
SemVer policy and deprecation windows.

Internal audit trails and design campaigns live under
[`docs-internal/`](./docs-internal/) and are not duplicated here. The
git log is the authoritative per-commit record; this file is the
user-facing narrative.

## [Unreleased]

_Accretion slot for v0.7.0 work. See [`ROADMAP.md`](./ROADMAP.md#v070--in-flight)
for the planned scope._

---

## [0.6.0] — 2026-04-21

**Theme:** feature-complete SQL + SDK + compliance surface. Every
primitive a healthcare-grade clinic app, finance ledger, or legal
case-management app needs is live and kernel-side — no in-memory
fakes, no parallel audit streams, no regex SQL parsers, no
hand-rolled erasure orchestrators.

### Breaking changes

- **Wire protocol v3 → v4.** `PROTOCOL_VERSION` bumped so
  `ConsentGrantRequest` and `ConsentRecord` can carry
  `basis: Option<ConsentBasis>` (GDPR Art 6 justification). v3
  clients against a v4 server are rejected at frame-header decode
  with a clean "unsupported protocol version" error; v4 servers
  do not speak v3. Upgrade both sides together. 4-cell compat
  matrix tested in `crates/kimberlite-wire/src/tests.rs::v3_v4_compat`.
  Migration details: [`docs/coding/migration-v0.6.md`](./docs/coding/migration-v0.6.md).
- **ABAC rule-name uniqueness.** `AbacPolicy::with_rule` now returns
  `Result<Self, AbacError>` and rejects rules whose name duplicates
  an existing rule. Previously duplicates were silently tolerated.
  Rule names are the audit-log identifier — HIPAA/SOX/FedRAMP
  reviews need this to be stable. Callers with duplicate-named
  rules must rename or deduplicate.

### Added

**SQL surface**

- **`ON CONFLICT` / UPSERT.** `INSERT INTO t (…) VALUES (…) ON
  CONFLICT (pk) DO UPDATE SET col = EXCLUDED.col` and
  `ON CONFLICT (pk) DO NOTHING`. Kernel-level `Command::Upsert` with
  a single atomic `UpsertApplied` event carrying a `resolution:
  Inserted | Updated | NoOp` discriminator — no dual-write window.
  Composes with `RETURNING`.
- **Correlated subqueries.** `EXISTS`, `NOT EXISTS`, `IN (SELECT)`,
  `NOT IN (SELECT)` with outer column references. Decorrelation to
  semi-join when provable; correlated-loop fallback otherwise.
  Configurable cardinality guard (`max_correlated_row_evaluations`,
  default 10M) rejects pathological shapes with
  `CorrelatedCardinalityExceeded`.
- **`AS OF TIMESTAMP` runtime resolver.** v0.5.0 shipped the parser +
  `QueryEngine::query_at_timestamp` with a caller-supplied resolver.
  v0.6.0 ships the default audit-log-backed timestamp→offset resolver
  so inline SQL works without the caller plumbing one. SDK
  `queryAt()` now accepts ISO-8601 / `Date` / nanos-bigint in
  addition to the existing event-offset form across TS + Python +
  Rust. Error surface: `AsOfBeforeRetentionHorizon` when asked older
  than the retained window.

**Compliance**

- **End-to-end `ConsentBasis`.** Wire v4 plus SDK surface for GDPR
  Art 6 basis (Consent, Contract, LegalObligation, VitalInterests,
  PublicTask, LegitimateInterests). `client.compliance.consent.grant({
  subjectId, purpose, basis })` across all three SDKs. Basis
  round-trips through the audit log and compliance exports.
- **`client.compliance.eraseSubject(subjectId)` auto-discovery.** The
  kernel now walks streams tagged `PHI` / `PII` with a `subject_id`
  column automatically. Override still available via
  `eraseSubject(subjectId, { streams: [...] })`. Idempotent — second
  call returns the existing signed receipt + emits a "second-call-noop"
  audit record. New VOPR scenario `EraseSubjectWithCrash` verifies
  crash-in-progress + recovery produces a valid hash-chain + signed
  proof.
- **Audit-log SDK query surface.** `client.compliance.audit.query({
  subjectId?, actor?, action?, fromTs?, toTs?, limit? })` across
  TS + Python + Rust. Returns structured rows with
  `changedFieldNames` only — never `before`/`after` values —
  enforced at the `kimberlite-wire` serialisation boundary.
  Server-side filtering with an index on
  `(tenant_id, subject_id, occurred_at)`. Streaming form over the
  existing Subscribe primitive.
- **Column-level masking policy CRUD.** DDL: `CREATE MASKING POLICY`,
  `ALTER TABLE t ALTER COLUMN c SET MASKING POLICY`, `DROP MASKING
  POLICY`. Reuses the `FieldMask` substrate (5 strategies shipped in
  v0.4.x: Redact, Hash, Tokenize, Truncate, Null). Planner
  composition: RBAC filter → mask → break-glass override. New
  kernel commands `CreateMaskingPolicy` / `AttachMaskingPolicy` /
  `DetachMaskingPolicy` / `DropMaskingPolicy` — all audit-logged.
  SDK: `client.admin.maskingPolicy.{create,alter,drop,list}()`
  across TS + Python + Rust. VOPR scenario `MaskingRoleTransition`
  proves no unredacted leakage across role transitions.

**Testing**

- **`StorageBackend` trait + `MemoryStorage` impl.** Extracted
  `trait StorageBackend` from the concrete `Storage` struct.
  `MemoryStorage` — no disk IO, hash-chain in-memory, deterministic
  replay. `Kimberlite::in_memory()` constructor alongside
  `Kimberlite::open(path)`; no breaking change. Test harness grows
  `Backend::InMemory` variant; TS + Python test modules accept
  `{ backend: 'memory' }`. 17.7× TempDir baseline speedup on
  Apple M-series in release.
- **ALTER TABLE VOPR crash scenario.** `AlterTableCrashRecovery` —
  add column + concurrent INSERTs + storage-crash mid-ALTER;
  recovery preserves `schema_version` monotonicity + event ordering.
  Additional doc-tests for `ADD COLUMN + SELECT *` round-trip.

### Safety

- Four new production `assert!` postconditions promoted in
  `crates/kimberlite-kernel/src/kernel.rs` covering the
  `Create/Attach/Detach/Drop MaskingPolicy` state-machine
  transitions (lines 1154, 1223, 1269, 1310). Each is paired with
  a `#[should_panic]` mirror test per
  [`docs/internals/testing/assertions-inventory.md`](./docs/internals/testing/assertions-inventory.md).

### Dependencies

- `sqlparser` 0.54 → 0.61. Prerequisite for ON CONFLICT syntax.
  16 breaking-change categories migrated. 470 `kimberlite-query`
  tests pass.
- `printpdf` 0.7 → 0.9. Full rewrite of
  `crates/kimberlite-compliance/src/report.rs` (427 lines) against
  the new layout-tree / Op-based model. Compliance-report output
  identical.
- `aws-lc-rs` 1.15 → 1.16 (pulls `aws-lc-sys` 0.37 → 0.40). Closes
  RUSTSEC-2026-0044..0048.
- `rustls-webpki` 0.103.9 → 0.103.13. Closes RUSTSEC-2026-0049,
  2026-0098, 2026-0099.
- `tar` 0.4.44 → 0.4.45. Closes RUSTSEC-2026-0067, 2026-0068.

### Infrastructure

- **EPYC nightly campaigns.** Three systemd timer pairs on the
  Hetzner EPYC box (`caterwaul`): VOPR nightly (daily 19:00 UTC,
  50k iterations ~2-4h), FV nightly-lite (daily 01:00 UTC, MIRI +
  Kani-smoke, ~35m), FV weekly (Sat 04:00 UTC, Alloy + Ivy + Coq +
  Kani full depth). 12 new `just` recipes.
- **Release-pipeline justfile.** 25-crate dep-ordered publish, 10-gate
  `release-dry-run` (version consistency, CHANGELOG section present,
  workspace build, clippy `-D warnings`, lib tests, doc-tests,
  publish dry-run, fuzz smoke, VOPR smoke, `cargo deny check
  advisories`), SDK-inclusive `bump-version` including
  sub-workspace Cargo.locks.

### Fixed

- `fix(query): reject deeply-nested SQL to prevent DoS via parser
  backtracking` (PR #87). New `depth_check` module with
  `MAX_SQL_NESTING_DEPTH=50` and `MAX_SQL_NOT_TOKENS=100` pre-parse
  guards. Rejection happens in microseconds.
- `fix(fuzz,chaos): eliminate false positives in nightly campaigns`
  (PR #86). Four surgical fixes silencing ~35 spurious crashes/run.
- `fix(upsert): clippy items-after-statements in convergence test`,
  `fix(client): collapse identical Query/QueryAt match arms`,
  `fix(parser): replace wildcard-for-single-variant in upsert
  tests` — drive-by lint fixes surfaced by the pre-publish audit.

### Documentation

- New `docs/coding/migration-v0.6.md` — v0.5.1 → v0.6.0 upgrade
  checklist + new-surface quick-reference.
- New `docs/reference/sql/correlated-subqueries.md` — design doc
  covering semi-join decorrelation, correlated-loop fallback,
  cardinality guard.
- New `docs/reference/sql/masking-policies.md` — DDL surface +
  composition with RBAC + break-glass.
- `docs/reference/sql/queries.md` — `AS OF TIMESTAMP` /
  `FOR SYSTEM_TIME AS OF` documented end-to-end.
- `docs/reference/sdk/parity.md` — `ConsentBasis`, audit query
  surface, and masking policy rows all flipped to ✅.
- `ROADMAP.md` rewritten for clarity — current / v0.7.0 / v1.0
  checklist / deferred / post-v1.0 cloud.
- `CHANGELOG.md` restructured — one `[Unreleased]` block, SemVer
  descending order, user-facing narrative only. Internal audit
  content moved to `docs-internal/audit/2026-Q2-release-readiness.md`
  and `docs-internal/design-docs/active/fuzz-to-types-hardening-apr-2026.md`.

---

## [0.5.1] — 2026-04-21

**Theme:** DX point release closing papercuts surfaced post-v0.5.0.
No architectural surface expands — storage-trait refactor,
wire-protocol bump, and new kernel commands stay in v0.6.0.

### Added

**SQL**

- **Scalar functions in `SELECT` projection.** `UPPER`, `LOWER`,
  `LENGTH`, `TRIM`, `CONCAT` / `||`, `ABS`, `ROUND(x)`,
  `ROUND(x, scale)`, `CEIL` / `CEILING`, `FLOOR`, `COALESCE`,
  `NULLIF`, `CAST`. New `parse_scalar_columns_from_select_items`
  pass parallels the existing aggregate / CASE / window passes.
  Un-aliased projections synthesise PostgreSQL-style default names.
- **Scalar predicates in `WHERE`.** `UPPER(name) = 'ALICE'`,
  `COALESCE(x, 0) > 10`, `CAST(s AS INTEGER) = $1` all route
  through a new `Predicate::ScalarCmp` variant. `!=` / `<>` route
  through ScalarCmp too (previously rejected).
- **`CAST(x AS <type>)`.** Integer widening is lossless; narrowing
  checks for overflow. `Text → Integer` uses `str::parse` and
  errors rather than silently coercing bad input to 0. `Real →
  Integer` truncates toward zero with explicit NaN/±∞ rejection.
  `Text → Boolean` accepts case-insensitive literals.
- **`NOT IN (list)` / `NOT BETWEEN low AND high`.** Both correctly
  surface `false` for `NULL` cells (SQL three-valued logic).
- **ILIKE / NOT LIKE / NOT ILIKE.** Three-valued NULL semantics
  preserved; Unicode simple lowercase for case-insensitive match.

**Testing**

- **`kimberlite-test-harness` crate (phase 1).** Wraps
  `Kimberlite::open(tempdir)` + in-process server behind a
  `TestKimberlite::builder()` API. Real parser, real kernel, real
  storage. Deterministic `Drop` that joins the polling thread with
  a 3s grace. Back-ported four e2e test files.
- **`@kimberlitedb/client/testing` + `kimberlite.testing`.** TS
  subpath export and Python module exposing
  `createTestKimberlite` / `create_test_kimberlite`. Both shell out
  to the shared `kimberlite-test-harness-cli` binary — one Rust
  codebase serves both SDKs.
- **`fuzz_scalar_expr` target** isolating the evaluator from
  planner/executor noise. Byte input → `arbitrary`-derived Shape →
  `ScalarExpr` tree (bounded depth) → twice-evaluated against a
  fixed row. Asserts determinism.

### Changed

- **SDK package rename.** `@kimberlite/client` → `@kimberlitedb/client`
  (npm org alignment). Auto-reconnect and retry built in; new
  `mapKimberliteError` error taxonomy.

### Fixed

- Regression test for SELECT alias preservation — the bare-column
  path was already correct as of v0.5.0, but a pinned test prevents
  silent regression.

---

## [0.5.0] — 2026-04-21

**Theme:** SQL coverage uplift + Phase 6 compliance endpoints +
test harness + nightly testing discipline.

Bundles three concurrent streams: the nine-phase SQL coverage
uplift (below), the April 2026 release-readiness audit deferred
items (see `docs-internal/audit/2026-Q2-release-readiness.md`), and
the AUDIT-2026-04 remediation wave.

### Added

**SQL — nine-phase coverage uplift**

- **Parameterised `LIMIT` / `OFFSET`** (`$N` placeholders). OFFSET
  is now actually applied — pre-fix `query.offset` was never read
  from the parsed AST, so `SELECT … OFFSET 50` silently returned
  rows from the start. Negative/non-integer bounds rejected with a
  clear error rather than panicking.
- **Simple `CASE` form** (`CASE x WHEN v1 THEN r1 … END`) — desugars
  to searched `CASE`.
- **Uncorrelated subqueries** — `IN (SELECT …)`, `EXISTS (…)`,
  `NOT EXISTS (…)`. Pre-execute pass walks the predicate tree, runs
  each subquery once before planning the outer query.
- **`INTERSECT` / `EXCEPT`** (plus existing `UNION`). `ALL` variants
  preserve multiset semantics; bare form deduplicates.
- **`RIGHT JOIN` / `FULL OUTER JOIN` / `CROSS JOIN` / `USING`.**
  CROSS JOIN has a cardinality guard (`MAX_JOIN_OUTPUT_ROWS = 1M`).
- **Aggregate `FILTER (WHERE …)`** on `COUNT(*)`, `COUNT(col)`,
  `SUM`, `AVG`, `MIN`, `MAX`.
- **JSON operators** `->`, `->>`, `@>` via new `Predicate::JsonExtractEq`
  and `Predicate::JsonContains`. Containment is recursive for
  objects, multiset-subset for arrays.
- **`WITH RECURSIVE`** with `MAX_RECURSIVE_DEPTH = 1000` iteration
  cap (honours the workspace "no recursion" lint).
- **`ALTER TABLE ADD / DROP COLUMN` end-to-end.** Kernel commands
  + paired `#[should_panic]` coverage on `schema_version`
  monotonicity assertions. Parser-only support shipped in v0.4.x;
  v0.5.0 lands the kernel path.

**Compliance (AUDIT-2026-04 remediation)**

- **Phase 6 server handlers** — `audit_query`, `export_subject`,
  `verify_export`, `breach_*`. Previously stub; now wired end-to-end
  with SDK wrappers.
- **`ErasureExecutor` trait + signed `ErasureCompleted` witness**
  emitted as a single kernel effect (AUDIT-2026-04 C-1).
  `client.compliance.eraseSubject()` surface in all three SDKs.
  (Auto-discovery of affected streams: v0.6.0.)
- **Tamper-evident `ComplianceAuditLog`** backed by `trait AuditStore`
  + durable storage (AUDIT-2026-04 H-2 / C-4).
- **Tenant isolation checker wired into VOPR** via new
  `EventKind::CatalogOperationApplied` / `EventKind::DmlRowObserved`
  event kinds. `sim-canary-catalog-cross-tenant` feature proves the
  wire fires (AUDIT-2026-04 C-2).
- **`RbacFilter::rewrite_query` recursive descent** into CTEs,
  UNION/INTERSECT/EXCEPT branches, derived-table subqueries in
  FROM, and subqueries in WHERE predicates (AUDIT-2026-04 M-7).
- **Seal / unseal tenant lifecycle** — single audit effect per
  command; tenant-scoped mutation rejection when sealed
  (AUDIT-2026-04 H-5).

**Testing & verification**

- **Nightly fuzzing** (`fuzz.yml`) — 20 targets with hard-fail
  crash + minimised repro archival.
- **VOPR nightly hard-fail** (`vopr-nightly.yml`) — removed
  `continue-on-error`; `.kmb` reproduction bundles archived on
  failure.
- **TLAPS PR-gating** (`formal-verification.yml::tla-tlaps-pr`) —
  5 core theorems PR-gated; the 92 compliance meta-theorems run
  nightly-full.
- **Fuzz-to-Types hardening campaign.** See
  `docs-internal/design-docs/active/fuzz-to-types-hardening-apr-2026.md`
  for the full narrative. Five bug classes made unrepresentable via
  new `kimberlite-types::domain` primitives (`NonEmptyVec`,
  `SqlIdentifier`, `BoundedSize<MAX>`, `ClearanceLevel`).
  Structure-aware fuzz targets (`fuzz_wire_typed`, `fuzz_vsr_typed`,
  `fuzz_sql_grammar`, `fuzz_sql_norec`, `fuzz_sql_pqs`). Persistent-
  mode fuzz infrastructure behind the `fuzz-reset` cargo feature
  (test-only).
- **UBSan nightly campaign** — 4h offset from ASan nightly on EPYC.
  Corpora shared so coverage benefits both.

### Changed

- **`AS OF TIMESTAMP` parser shipped** — `FOR SYSTEM_TIME AS OF '<iso>'`
  / `AS OF '<iso>'`. `QueryEngine::query_at_timestamp` accepts a
  caller-supplied resolver. (Default audit-log resolver: v0.6.0.)

### Fixed

- `pre_execute_subqueries` handles the `OFFSET`-never-read bug
  surfaced in the phase-1 regression tests.

### Migration

See [`docs/coding/migration-v0.5.md`](./docs/coding/migration-v0.5.md).
Short version: bump the SDK dep, update any `execute()` call sites
to read `.rowsAffected` / `.rows_affected` instead of treating the
return value as a number.

---

## [0.4.2] — 2026-04-20

**Theme:** release-readiness — truth-in-advertising patch + SDK
production launch. Corrects user-facing claims to match code; no
behaviour changes or feature deferrals.

### Breaking changes

- **Wire protocol v1 → v2.** `PROTOCOL_VERSION` bumped from 1 to 2.
  v0.4.0 clients cannot talk to v0.4.2 servers and vice versa — the
  handshake rejects. Payload is now a `Message` enum
  (Request | Response | Push) instead of a flat Request / Response
  pair, which cleanly supports the new Subscribe push primitive.
  No protocol shim; upgrade both sides in lockstep.

### Added

**SDK production launch (9 phases)**

- **Production-grade SDKs for Rust, TypeScript, and Python.** Every
  server-side primitive is reachable from all three SDKs with
  idiomatic ergonomics, structured errors, typed rows, connection
  pooling, real-time subscriptions, admin operations, and GDPR
  compliance flows.
- **Structured errors** replace the prior `number` return from
  `execute()` — all three SDKs now return `ExecuteResult` with
  `.rowsAffected` / `.rows_affected` / `.returning`.
- **Real-time subscriptions** over the new Push message kind.
- **Admin operations** — tenant CRUD, stream CRUD, API-key
  lifecycle.
- **GDPR compliance flows** — consent grant/revoke, breach
  notification, subject export, audit query.
- **Platform binaries** — Linux x86_64/aarch64, macOS x86_64/aarch64,
  Windows x64. `.sha256` sidecars; `install.sh` verifies against
  `SHA256SUMS` manifest.

### Changed

- **Truth-in-advertising scrub across `docs/`, `crates/`,
  `README.md`, `CLAUDE.md`, `SECURITY.md`** — "compliant" /
  "certified" language → "-ready" + framework-specific qualifiers.
  Formal-verification claims (`"world's first/only"`, "136+ proofs")
  replaced with honest decomposition. Assertion-count claims
  removed; replaced with pointer to
  `docs/internals/testing/assertions-inventory.md`. VOPR scenario
  count corrected (74 enum variants, ~50 substantive, ~24
  scaffolded). "90-95% Antithesis-grade" replaced with
  "Antithesis-inspired deterministic simulation". Framework
  readiness percentages → "designed for / ready / no audit
  completed" framing. See
  `docs-internal/audit/2026-Q2-release-readiness.md` for the full
  18-item rubric.

### Fixed

- `install.sh` POSIX-sh local-variable collision inside
  `verify_checksum()` that caused `unzip` to look for the wrong
  filename. Helper-function locals renamed throughout. Mirror
  re-synced to `website/public/install.sh`.
- **Release publish workflow** — `cargo publish | tee` masking
  cargo failures fixed with `set -euo pipefail` + `PIPESTATUS[0]`;
  publish order expanded from 6 crates to full 24-crate topological
  order in 6 tiers with 30s settle delays; `kimberlite-doc-tests`
  marked `publish = false`.

---

## [0.4.1] — 2026-02-04

**Theme:** VOPR infrastructure hardening + formal verification
additions.

### Added

- **FCIS pattern adapters** in `crates/kimberlite-sim/src/adapters/`
  for Clock, RNG, Network, Storage, Crash. Trait-based abstraction
  for swapping sim ↔ production implementations.
- **19 specialised invariant checkers** — offset monotonicity,
  event-count bounds, consensus safety. O(1) checkers replace the
  O(n!) linearizability checker (which was a naive implementation
  that couldn't scale).
- **5 canary mutations** with 100% detection rate — proves VOPR
  catches real bugs.
- **17 fault-injection test scenarios.**
- **Docker release hotfix** — v0.4.1 bumps the Docker image tag
  without a workspace version bump.

### Performance

- Maintained >70k sims/sec throughput with the new invariant
  checkers (vs the prior O(n!) checker that was the bottleneck).

---

## [0.4.0] — 2026-02-03

**Theme:** VOPR advanced debugging — production-grade DST platform.

### Added

- **Timeline visualisation** — ASCII Gantt chart renderer for
  understanding simulation execution flow. 11 event kinds
  (client ops, storage, network, protocol, crashes, invariants).
  `vopr timeline failure.kmb --width 120`.
- **Bisect to first bad event.** `BisectEngine` performs O(log n)
  binary search through the event sequence.
  `SimulationCheckpoint` + `CheckpointManager` with deterministic
  RNG fast-forward to any event. 10-100× faster than full replay;
  typical convergence <10 iterations for 100k events. Generates
  minimal reproduction bundles.
- **Delta debugging** — Zeller's ddmin algorithm for automatic
  test-case minimisation. Dependency-aware (network, storage,
  causality). 80-95% test-case reduction achieved.
- **Real kernel state hash** — BLAKE3 hashing of actual kernel
  state replaces the prior placeholder. Enables true determinism
  validation and checkpoint integrity.
- **Coverage dashboard** (`vopr dashboard`). Axum + Askama +
  Datastar web UI. 4 coverage dimensions (state points, message
  sequences, fault combinations, event sequences). Real-time SSE
  updates; top seeds table; progress bars.
- **Interactive TUI** (`vopr tui`). Ratatui-based terminal UI with
  3 tabs (Overview, Logs, Configuration), real-time progress
  gauge, scrollable logs.

### Changed

- CLI gains 5 new commands: `timeline`, `bisect`, `minimize`,
  `dashboard`, `tui`.
- Feature flags: `dashboard` (axum + askama + tokio-stream), `tui`
  (ratatui + crossterm).

---

## [0.3.1] — 2026-02-03

**Theme:** VOPR VSR mode — protocol-level Byzantine testing.

### Added

- Complete VSR protocol integration into VOPR. Fundamental
  architecture shift from state-based simulation to testing actual
  VSR replicas processing real protocol messages with Byzantine
  mutation.
- **10 protocol-level attack patterns** in `protocol_attacks.rs`
  (SplitBrain, MaliciousLeader, PrepareEquivocation, ForgedMessage,
  ReplayOldPrepare, …) with 3 pre-configured suites (Standard,
  Aggressive, Subtle). 100% attack detection rate.
- **Event logging** with compact binary format (~100 bytes/event).
  `.kmb` reproduction bundles (bincode + zstd). Perfect
  reproduction from seed + event log.

---

## [0.3.0] — 2026-02-03

**Theme:** VSR hardening and Byzantine resistance.

### Added

- **Storage realism** — 4 I/O scheduler policies (FIFO, Random,
  Elevator, Deadline); concurrent out-of-order I/O (up to 32
  operations/device); 5 crash scenarios (DuringWrite, DuringFsync,
  PowerLoss, etc.); block-level granularity (4KB); torn-write
  simulation.
- **Byzantine-resistant VSR consensus** with production-enforced
  assertions in cryptography, consensus, and state-machine paths.
- **Realistic workload generators** — 6 patterns (Uniform, Hotspot,
  Sequential, MultiTenant, Bursty, ReadModifyWrite).
- **Coverage-guided fuzzing** with multi-dimensional coverage
  tracking and 3 selection strategies.

### Performance

- 85k-167k sims/sec throughput with full fault injection.
- Storage realism overhead: <5%. Event logging overhead: <10%.

---

## [0.2.0] — 2026-02-02

**Theme:** advanced testing infrastructure + documentation.

### Added

- **VOPR framework** (Viewstamped Replication Operational Property
  testing) — deterministic simulation framework inspired by
  TigerBeetle + Antithesis.
- **Invariant checking** with production-ready documentation.
- **Comprehensive docs layout** — user-facing in `docs/`, internal
  in `docs-internal/`.

---

## [0.1.10] — 2026-01-31

**Theme:** protocol layer, SDK integration, secure data sharing.

### Added

- Complete **wire protocol** implementation (TCP + length-prefixed
  frames, JWT + API-key auth, TLS).
- **Multi-language SDKs** (Python, TypeScript, Rust, Go) with
  tests and CI.
- **SQL query engine** — SELECT, INSERT, UPDATE, DELETE, CREATE
  TABLE, aggregates, GROUP BY, HAVING, DISTINCT, INNER / LEFT
  JOIN, parameterised queries (`$1`, `$2`, ...).
- **Secure data-sharing layer** with consent ledger.
- **MCP server** for LLM integration (4 tools).

---

## [0.1.5] — 2026-01-25

**Theme:** RBAC, ABAC, masking, consent, erasure.

### Added

- **RBAC** with 4 roles, SQL rewriting, column filtering, row-level
  security.
- **ABAC** with 12 condition types, 3 pre-built compliance policies
  (HIPAA, FedRAMP, PCI DSS).
- **Field-level data masking** with 5 strategies (Redact, Hash,
  Tokenize, Truncate, Null).
- **Consent management** with 8 purposes, kernel-level enforcement.
- **Right to erasure** (GDPR Article 17) with 30-day deadlines and
  exemptions.
- **Breach detection** with 6 indicators and 72-hour notification
  deadlines.
- **Data portability export** (GDPR Article 20) with HMAC-SHA256
  signing.
- **Audit logging** with 13 action types across all compliance
  modules.

---

## [0.1.0] — 2025-12-20

**Theme:** core foundation — crypto, storage, consensus, projections.

### Added

- **Cryptographic primitives.** SHA-256 + BLAKE3 dual-hash
  (`HashPurpose` enum enforces compliance vs hot-path split).
  AES-256-GCM content encryption. Ed25519 signatures.
- **Append-only log storage** with CRC32 checksums, segment
  rotation (256MB), index WAL.
- **Pure functional kernel** (Functional Core / Imperative Shell).
  Commands → State + Effects; deterministic by construction.
- **VSR consensus** (Viewstamped Replication) adapted from
  TigerBeetle's architecture, extended with multi-tenant routing,
  clock synchronisation, and repair-budget policies.
- **B+tree projection store** with MVCC and SIEVE cache for
  derived-view queries.
- **Multi-tenant isolation** at placement and kernel level.

### Design principles

- **All data is an immutable ordered log; all state is a derived
  view.**
- **Functional Core / Imperative Shell** — kernel is pure, IO at
  the edges.
- **Make illegal states unrepresentable** — enums over booleans,
  newtypes over primitives.
- **Parse, don't validate** — validate at boundaries once, then
  use typed representations throughout.
