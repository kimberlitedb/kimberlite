# Kimberlite Roadmap

Kimberlite is an OSS-first, compliance-positioned verifiable database
for regulated industries (healthcare, finance, legal). All data is an
immutable ordered log; all state is a derived view.

This file lists what's shipped, what's next, and the gates v1.0 must
clear. Detail for each completed release lives in [`CHANGELOG.md`].
Detail for each planned feature lives in GitHub issues.

[`CHANGELOG.md`]: ./CHANGELOG.md

## Status

**Current release:** [`v0.6.0`] (2026-04-21) —
feature-complete SQL + SDK + compliance surface. See [`CHANGELOG.md`]
for the full list.

**Next release:** v0.7.0 — DX + SQL follow-ups. No breaking wire
changes expected.

**Target v1.0:** when the gates below close. No fixed date — we ship
when the third-party audits, SDK coverage, and production readiness
criteria are all green.

[`v0.6.0`]: https://github.com/kimberlitedb/kimberlite/releases/tag/v0.6.0

---

## v0.7.0 — in-flight

Scheduled for the first minor after v0.6.0. All items below were
deliberately deferred from v0.6.0 to keep that release focused on the
feature-complete compliance surface.

### SQL

- [ ] Additional scalar functions: `MOD`, `POWER`, `SQRT`, `SUBSTRING`,
      `EXTRACT`, `DATE_TRUNC`, `NOW()`, `CURRENT_TIMESTAMP`,
      `CURRENT_DATE`, plus interval arithmetic. Requires a clock
      threading decision for VOPR-sim-vs-wall-clock (separate design
      conversation).
- [ ] `DROP TABLE IF EXISTS` + `DELETE FROM t` (no WHERE) `rowsAffected`
      fix. Test-infrastructure impact only.
- [ ] Auto-generated traceability matrix from in-source `AUDIT-YYYY-NN`
      markers (currently manual).
- [ ] SQL planner — prevent inverted range output. `fuzz_sql_norec`
      currently triggers an `if range.start > range.end` path in
      `kimberlite-store::btree::scan` that the debug assert surfaces as
      a planner correctness warning. Release builds clamp to empty, so
      results are still correct; the v0.6.1 patch disables the assert
      under `cfg(fuzzing)` to unblock CI. Track down which predicate
      lowerings emit the inverted range and fix upstream.

### SDK & DX

- [ ] Go SDK — deferred post-v0.4 in README; brings Kimberlite to
      parity with the TS / Python / Rust trio.
- [ ] SDK connection-pool metrics + Prometheus exporter parity.
- [ ] Python SDK typing refresh (PEP 604, Self types, Protocol-based
      plugins).

### Testing & verification

- [ ] VOPR workload generators for the v0.6.0 command families —
      `Masking*`, `Upsert`, `AS OF TIMESTAMP` resolution,
      `eraseSubject` auto-discovery. Kernel-level correctness is
      covered by unit + integration tests; this closes the
      storage-realism + protocol-attack coverage gap documented at
      `docs-internal/audit/2026-Q2-release-readiness.md`.
- [ ] Formal-verification specs for scalar-expression purity.
- [ ] VOPR scenarios for SQL-surface semantics (beyond the Tier 1 / 2
      scenarios already landed in v0.6.0).

### Infrastructure

- [ ] Topological-order validation for `PUBLISH_CRATES` in `justfile`
      (compare against `cargo metadata`).
- [ ] Performance re-baseline against current hardware — I/O
      throughput, consensus latency, SQL query throughput.
- [ ] GPG-signed release tags by default.
- [ ] MIRI nightly-lite runtime exceeds the service's
      `TimeoutStartSec=5400` (90 min). Root cause is MIRI's
      interpretation overhead on crypto-roundtrip tests (e.g.
      `encryption::tests::large_plaintext_encryption`) — not a
      proptest/isolation issue. Consider annotating the heaviest
      tests with `#[cfg_attr(miri, ignore)]` or narrowing MIRI scope
      (MIRI's main value is UB via pointer/lifetime interpretation,
      not arithmetic correctness on AES-GCM). As a stopgap, bump
      `TimeoutStartSec` to 10800 and accept that the FV/fuzz overlap
      window will trip `Conflicts=kimberlite-fuzz-nightly.service` —
      or shift the FV timer earlier so both finish before fuzz fires
      at 02:00 UTC.

---

## Deferred

Items we're not working on now. Revisit at v0.8+ or v1.0 planning.

- **Transactions** (`BEGIN` / `COMMIT` / `ROLLBACK`) — single
  statements are atomic; event-sourcing + optimistic concurrency
  covers current consumers. Re-evaluate against v1.0 if scope is
  manageable.
- **Window functions beyond what shipped** — ROWS BETWEEN clauses,
  EXCLUDE, window-aggregate frame defaults. Current `ROW_NUMBER` /
  `RANK` / `LAG` / `LEAD` / `FIRST_VALUE` / `LAST_VALUE` with
  `PARTITION BY` / `ORDER BY` covers the common cases.
- **Linearizability chaos testing** — currently labelled a liveness
  proxy in code. Full linearizability testing deferred pending a
  design conversation about what "linearizable" means in the
  single-writer-per-tenant model.
- **Physical stream deletion** — soft-delete + retention only for
  now. Physical deletion conflicts with the "all data is an
  immutable ordered log" principle; needs a careful design for how
  retention horizons interact with `AS OF TIMESTAMP` and audit
  witnesses.
- **Unbounded audit-log query surface** — current queries are
  paginated + bounded. Unbounded retrieval deferred until we decide
  how it interacts with retention + compliance export formats.
- **Antithesis integration** — paid service. Worth evaluating
  post-v1.0 once revenue supports it; current VOPR + fuzz nightly
  on EPYC covers the cost-effective window.
- **Snapshots** — gated on real-usage benchmarks from the first
  v0.7.0 consumer (notebar). We need aggregate size distribution,
  replay cost per 1k events, read-vs-write frequency, and
  worst-case long-lived-aggregate profiles before designing the
  snapshot primitive. Re-evaluate after the consumer runs on
  v0.7.0 for 2–4 weeks and produces those benchmarks.
  Correctness primitive (bounded recovery, deterministic
  reconstruction under formal verification), not just a
  perf optimisation — the design needs those numbers to land
  correctly the first time.

---

## v1.0 — checklist-gated

v1.0 ships when **every** item below is green. No date. If an item
proves unnecessary, it's removed from this checklist via a pull
request with justification, not quietly dropped.

### Third-party attestations

- [ ] SOC 2 Type II audit completed with a clean report.
- [ ] HIPAA attestation + a BAA partner willing to use Kimberlite
      as a healthcare database of record.
- [ ] GDPR readiness review by an independent privacy counsel.
- [ ] At least one compliance-regulated production deployment
      (healthcare, finance, or legal) running Kimberlite as the
      system of record, not a secondary store.

### SDK coverage

- [ ] Rust, TypeScript, Python — all three at SDK parity with
      Kimberlite's server surface. Verified by the SDK parity matrix
      at `docs/reference/sdk/parity.md`.
- [ ] Go SDK at parity.
- [ ] Java SDK — at least Phase 1 (core client + auth + queries).
- [ ] C++ SDK — via the existing FFI header + thin idiomatic
      wrapper.

### Formal verification

- [ ] Coq → Rust extraction pipeline producing verified cryptographic
      primitives, not just hand-written impls with Coq specs.
- [ ] Ivy → Apalache migration complete — Ivy's UX is blocking and
      Apalache gives us a clearer path to bounded model-checking.
- [ ] All TLA+ theorems PR-gated (currently: core subset PR-gated,
      92 compliance meta-theorems nightly). Target: 100% PR-gated.
- [ ] Kani proof count sustained ≥90 with coverage growing
      per-release.

### Performance

- [ ] Throughput baseline published for standard workloads on the
      reference hardware tier (EPYC 7xx3 series, 64 GB RAM,
      NVMe SSD).
- [ ] Latency baseline for consensus commit, SQL point-read, SQL
      point-write, and audit-log append.
- [ ] Benchmark reproduction kit in `benches/` that third parties
      can run against their own hardware.

### Operational maturity

- [ ] Documented disaster-recovery procedure with tested runbooks.
- [ ] Documented upgrade path for every v0.x → v0.(x+1) migration
      (not just breaking ones) with end-to-end validated smoke test.
- [ ] On-call rotation playbook (for self-hosters, not a managed
      service).
- [ ] Observability dashboards — Prometheus + Grafana templates for
      VSR consensus health, storage I/O, SQL query latency,
      compliance-event rates.

### Documentation

- [ ] A published book or long-form tutorial covering "design a
      compliance-backed app on Kimberlite" end-to-end.
- [ ] Reference architectures for HIPAA, PCI DSS, and GDPR
      deployments.
- [ ] Every API in the SDK parity matrix has runnable, tested
      examples in Rust, TypeScript, and Python.

---

## Post-v1.0

### Managed cloud

Kimberlite Cloud — managed database service — is planned for after
v1.0. The OSS core stays OSS. Cloud adds ops, scaling, billing, UI,
and a compliance-ready shared-responsibility model. Similar model to
CockroachDB Serverless / MongoDB Atlas / Supabase Platform layered on
Postgres.

Exact pricing + infrastructure vendor + geographic availability are
not yet settled. This roadmap is the commitment that the OSS core
will remain independently usable with the same feature set as the
cloud service.

### Continuous improvement

These items ship opportunistically once v1.0 lands — not gated on
specific versions.

- Performance improvements as real workloads expose bottlenecks.
- Additional compliance-framework formal specs as regulation
  evolves (EU AI Act, DORA deepening, APRA CPS 230/234
  refreshes).
- Deeper VOPR scenario coverage as production incidents surface
  new bug classes.
- Protocol evolution — wire v5 when a new feature needs it.
- Additional language SDKs driven by demand.

---

## How to propose a roadmap change

- **Adding an item to v0.7.0:** open a GitHub issue with the label
  `target:v0.7.0`, link a design doc if the change is non-trivial,
  then submit a PR against this file.
- **Adding an item to the v1.0 checklist:** the bar is high.
  v1.0 gates should represent the minimum for a healthcare / finance
  production deployment. Open an issue with the label
  `v1.0-gate-proposal` and expect pushback.
- **Removing a v1.0 gate:** requires a design discussion. Open an
  issue with the label `v1.0-gate-remove` and a written
  justification.
- **Deferring a v0.7.0 item:** move it to the Deferred section with a
  one-line reason. Don't quietly drop.

## Related documents

- [`CHANGELOG.md`](./CHANGELOG.md) — what shipped in each release.
- [`VERSIONING.md`](./VERSIONING.md) — SemVer policy, breaking-change
  rules, deprecation windows.
- [`CONTRIBUTING.md`](./CONTRIBUTING.md) — how to submit changes.
- [`SECURITY.md`](./SECURITY.md) — vulnerability reporting.
- `docs-internal/audit/` — internal audit trail (compliance,
  release-readiness).
- `docs-internal/design-docs/active/` — active design discussions.
