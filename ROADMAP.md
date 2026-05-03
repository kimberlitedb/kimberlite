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

**Next release:** v0.6.2 — TS SDK pagination + buffer-default patch
(found integrating notebar). Then v0.7.0 — DX + SQL follow-ups. No
breaking wire changes expected in either.

**Target v1.0:** when the gates below close. No fixed date — we ship
when the third-party audits, SDK coverage, and production readiness
criteria are all green.

[`v0.6.0`]: https://github.com/kimberlitedb/kimberlite/releases/tag/v0.6.0

---

## v0.6.2 — patch

Found while integrating Kimberlite into the first downstream consumer
(notebar). Event-sourcing replays via the TS SDK trip
`connection error: response too large` once a stream grows past the
~1 MiB read budget, and the partial response wedges the channel —
the next request fails with `ResponseMismatch`. App-level workaround
already shipped in notebar (offset-walking pagination in `repo-kit.ts`
+ `bufferSizeBytes: 64 MiB` in `adapter.ts`); this section tracks the
upstream fixes so the next consumer doesn't rediscover the same trap.

### TS SDK (`@kimberlitedb/client`)

- [ ] **Add `readAll(stream)` (or pagination iterator) to `Client`.**
      Highest-leverage fix: makes silent-truncation bugs impossible by
      construction. Every consumer who needs a full-stream replay
      currently reinvents offset-walking pagination — or, worse,
      doesn't, and silently truncates at the 1 MiB `maxBytes` default,
      which is a much scarier failure mode than the visible error
      notebar hit. Two acceptable shapes:
      - `client.readAll(stream, { batchSize })` returning an
        `AsyncIterable<Event>` that walks offsets internally until
        end-of-stream.
      - `client.read(...)` returns `{ events, hasMore, nextOffset }`
        instead of a bare array, so partial reads are explicit at the
        type level and impossible to ignore.
      Prefer the iterator — it's harder to misuse and matches the
      streaming nature of an immutable log.
- [ ] **Align `bufferSizeBytes` default with `read({ maxBytes })`.**
      The connection buffer default trips below the 1 MiB `maxBytes`
      default because of framing overhead, so the SDK fails on its own
      defaults for any reasonably sized event. Set the connection
      buffer to 2–4× the read budget by default, or derive it from
      `maxBytes` at connect time so they can't drift apart.
- [ ] **Recover the channel after an oversized response.**
      Once `response too large` fires, the unread bytes stay in the
      framing buffer and the next request hits `ResponseMismatch`.
      Connection should either drain the offending frame and continue,
      or mark itself poisoned and force a reconnect — the framing
      error must not leak into unrelated subsequent requests.
- [ ] **Improve the error message.**
      `connection error: response too large` reads like a transport
      bug. It should hint at the actual remediation: "response exceeds
      `bufferSizeBytes` — increase the buffer, lower `maxBytes`, or
      use `readAll` for full-stream replay."

Found by: notebar `repo-kit.ts::replayFromStream` against
`appointment_events` / `invoice_events`.

### Compliance SDK

- [ ] **Add `termsVersion` + `accepted` to `consent.grant(...)`.**
      Notebar's Phase 1 consent-capture flow needs to record which
      terms version a subject accepted (and explicitly capture
      `accepted: false` when a subject declines), but today
      `grant_consent[_with_basis]` in
      `crates/kimberlite-compliance/src/consent.rs:234-258`
      stores neither. Add optional `terms_version: Option<String>`
      and `accepted: bool` (default `true`) fields to `ConsentRecord`
      and thread them through `grant_consent_with_basis`, the TS SDK
      (`sdks/typescript/src/compliance.ts::consent.grant(...)`), and
      the Python SDK (`sdks/python/kimberlite/compliance.py`).
      Backwards-compatible — existing callers keep working unchanged
      because both fields are optional with sensible defaults. No
      new kernel command; the existing event-sourced
      `ConsentRecord` is the audit trail. Blocks notebar Phase 1.

Found by: notebar Phase 1 consent-capture flow.

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
- [ ] **GROUP BY scale ceiling.** `MAX_GROUP_COUNT = 100_000` in
      `crates/kimberlite-query/src/executor.rs:54` is a hard error
      rather than a degradation, and aggregation is fully in-memory
      (`HashMap<Vec<Value>, AggregateState>`). Replace the const with
      a configurable `aggregate_memory_budget_bytes` (default 256 MiB,
      ≈ 1M groups), and replace the panic-style error with a
      structured `AggregateMemoryExceeded { budget, observed }` whose
      message names the knob. Pushes the ceiling out ~10× without a
      planner overhaul. Proper spill-to-disk hash aggregate is
      tracked under Deferred (v0.8.0). Found by: notebar GST report
      drill-down on `ar_ledger`.
- [ ] **Expression-index note (no v0.7.0 work).** `CreateIndex.columns`
      in `crates/kimberlite-kernel/src/command.rs` carries bare
      column names (`Vec<String>`); expressions like
      `DATE_TRUNC('month', created_at)` cannot be indexed today.
      Surfacing this requires an index-definition AST plus an
      evaluator on the write path — too large for v0.7.0. Tracked
      under Deferred for v0.8.0 so consumers know not to rely on
      it. (Equality / range indexes on plain columns already work end
      to end via `find_usable_indexes` → `IndexScan` in
      `crates/kimberlite-query/src/planner.rs:1373-1383`.)

### SDK & DX

- [ ] Go SDK — deferred post-v0.4 in README; brings Kimberlite to
      parity with the TS / Python / Rust trio.
- [ ] SDK connection-pool metrics + Prometheus exporter parity.
- [ ] Python SDK typing refresh (PEP 604, Self types, Protocol-based
      plugins).
- [ ] **Cookbook examples for already-shipped primitives that
      downstream consumers keep missing.** Notebar filed gap reports
      for two features that are already implemented end-to-end —
      because nothing in `examples/` or the SDK README pointed at
      them. Ship one runnable TS example per primitive, linked from
      the SDK README:
      - **Real-time subscriptions.** `client.subscribe(streamId, {
        startOffset })` from `sdks/typescript/src/subscription.ts` is
        an `AsyncIterable<SubscriptionEvent>` with credit-based flow
        control; wire frame + server handler shipped in earlier v0.x.
        Notebar still believes the client is pull-only.
      - **Secondary-index lookup by non-PK column.**
        `CREATE INDEX ON projection(provider, providerMessageId)`
        followed by a `SELECT … WHERE provider = ? AND
        providerMessageId = ?` — the planner already emits
        `IndexScan` via `find_usable_indexes` /
        `select_best_index` (`crates/kimberlite-query/src/planner.rs`).
      - **`recordConsent` round-trip.** Once the v0.6.2 fields
        (`termsVersion`, `accepted`) land, ensure an example
        demonstrates the full grant + audit-query flow.
      Goal: the next consumer integrating Kimberlite finds the
      answer in `examples/` instead of filing a gap report.

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
- [ ] Investigate fuzz-target slow inputs. `fuzz_kernel_command` and
      `fuzz_abac_evaluator` have corpus entries that take 20+ minutes
      per iteration on the 2-vCPU GitHub runner. v0.6.1 raised the
      libFuzzer per-input timeout above the per-target wall-clock
      budget so these don't false-positive as crashes, but the
      underlying perf signal is real and worth chasing — either the
      corpus has accumulated pathological inputs that should be
      pruned, or there's an O(n^k) blowup in the kernel command
      apply-path on certain shapes.

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

- **Transactions** (`BEGIN` / `COMMIT` / `ROLLBACK`, including
  multi-stream atomic appends) — single statements are atomic;
  event-sourcing + optimistic concurrency covers current consumers.
  Notebar's Phase 4 POS flow (issue invoice + decrement stock across
  two streams) is the first concrete v1.0 motivator; outbox pattern
  remains the documented workaround for v0.7.0. The single-writer-
  per-tenant VSR model makes cross-stream atomicity a non-trivial
  design tension — `AppendBatch` in
  `crates/kimberlite-kernel/src/command.rs:89-94` is single-stream
  by construction. If notebar Phase 8 claim reconciliation hits a
  half-success the outbox can't tolerate, escalate to a v1.0 design
  doc before v1.0 freeze. Re-evaluate against v1.0 if scope is
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
- **User-defined materialised projections** — notebar wants
  `practitioner_hours_by_day` (currently compute-on-read in their
  `repos/practitioner-hours.ts`) and a typed `communications`
  projection registered as kernel-managed views. Today
  `ProjectionStore` in `crates/kimberlite-store/src/lib.rs` is
  system-internal; surfacing a `Cmd::CreateProjection` plus a SQL
  `CREATE MATERIALIZED VIEW` plus a refresh scheduler is ~1000+ LOC
  across kernel, query, and store, and needs a design doc that
  reconciles refresh semantics with the immutable-log model.
  Re-evaluate at v0.8 — this is a kernel primitive, not a patch.
- **Spill-to-disk hash aggregate** — proper fix for the GROUP BY
  ceiling that v0.7.0 only widens with a memory-budget knob (see
  v0.7.0 SQL section). When notebar's GST drill-down or any future
  consumer's aggregate workload approaches the budget, this becomes
  the real fix. Re-evaluate at v0.8.0.
- **Expression indexes** — `CreateIndex.columns` carries bare
  identifier strings today, so `DATE_TRUNC('month', created_at)`
  cannot be indexed. Needs an index-definition AST that survives
  parse → kernel → executor, plus an evaluator on the write path so
  index entries reflect the expression result. Re-evaluate at v0.8.0
  alongside the materialised-projection work — they share planner
  infrastructure.
- **Blob storage adapter abstraction** — Kimberlite's storage layer
  is the event log by design; `docs/reference/sql/ddl.md` already
  directs consumers to keep blobs out of the log. Notebar's
  `document-store.ts` hardcoding S3 is the intended pattern, not a
  workaround. Defer to v1.0+; revisit only if a compliance use case
  (signed-blob retention with audit witnesses, GDPR-erasure
  integration spanning blob lifecycle) requires kernel-level blob
  primitives. A backend-adapter trait alone (S3 / GCS / Azure /
  MinIO) is a community-extension shape, not a core primitive.
- **Document content search / full-text index** — `LIKE` / `ILIKE`
  in `crates/kimberlite-query/src/plan.rs::matches_like_pattern`
  cover pattern matching; there is no tokenizer, inverted index,
  `MATCH` operator, or ranking. From-scratch subsystem
  (tokenization + stemming pipeline, inverted-index storage,
  `MATCH` syntax, real-time index maintenance on append). Notebar's
  Phase 9 explicitly cuts content search. Defer to v1.0+ pending
  consumer demand and a clear story for how FT interacts with
  retention + erasure.

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
