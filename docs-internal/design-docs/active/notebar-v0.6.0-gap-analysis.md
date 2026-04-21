# Kimberlite v0.6.0 — "Clinic management complete" gap analysis

## Goal

Ship a v0.6.0 of Kimberlite such that Notebar (a clinic-management app for Australian healthcare: AU Privacy Act + NDB, GDPR overlap) can build out its **complete** feature set — clinical workflow, scheduling, consent, erasure, audit, and reporting — with:

- **No in-memory workarounds** for things the kernel should handle.
- **No parallel infrastructure** (audit streams, migration runners, erasure orchestrators, regex SQL parsers) that exists because the kernel can't yet provide it.
- **No half-shipped compliance features** — if the SDK exports a type, the type must be live on the wire.
- **No tempdir-based tests** — CI must be able to run the real kernel in-process, fast.

Once v0.6.0 lands, Notebar can finish migration in one pass and start building the remaining clinic features against a complete surface.

---

## Baseline — what's already shipped

### v0.5.0 (2026-04-21)

- 9-phase SQL uplift: parameterised `LIMIT`/`OFFSET` (Notebar-motivated), simple CASE, uncorrelated `IN (SELECT)` / `EXISTS` / `NOT EXISTS`, `INTERSECT` / `EXCEPT` (+ `ALL`), `RIGHT` / `FULL OUTER` / `CROSS JOIN` / `USING`, aggregate `FILTER`, JSON operators `-> ->> @>`, `WITH RECURSIVE`, `INSERT/UPDATE/DELETE ... RETURNING`.
- `ALTER TABLE ADD/DROP COLUMN` **parser** (kernel execution still missing — see v0.6.0 scope).
- Compliance: phase-6 server handlers, signed erasure receipts, break-glass query logging, audit hash-chain.
- SDK: auto-reconnect, retry + `mapKimberliteError` error taxonomy.
- Release hygiene: fuzz CI, doc-tests in CI, TLAPS PR-gate, VOPR nightly hard-fail.

### v0.5.1 (2026-04-21 — Notebar papercuts response)

- `NOT IN (list)`, `NOT BETWEEN` parser paths.
- Scalar expressions in `WHERE` + `SELECT` (`UPPER`, `LOWER`, `LENGTH`, `TRIM`, `CONCAT`, `ABS`, `ROUND`, `CEIL`, `FLOOR`, `COALESCE`, `NULLIF`, `CAST`, `||` string concat).
- SELECT alias preservation regression test.
- `kimberlite-test-harness` **phase 1** — real-kernel-in-child-process via tempdir. `@kimberlitedb/client/testing` + `kimberlite.testing` Python + Rust `TestKimberlite::builder()`.
- SDK package rename: `@kimberlite/client` → `@kimberlitedb/client`.

### Already shipped elsewhere (per ROADMAP.md)

- v0.7.0 (complete): metrics HTTP endpoint, `/health`, `/ready`, OTel traces, backup/restore, tenant CLI.
- v0.8.0 (complete): io_uring, thread-per-core, log compaction, compression, zero-copy deser, ~200k events/sec target.
- v0.9.0 (complete): graceful shutdown / rolling upgrade, dynamic cluster reconfiguration, hot shard migration, tag-based rate limiting, `Subscribe` operation, **stream retention policies with legal holds**, ML data classification.

⚠ **Important implication:** retention / legal-hold / subscribe / break-glass / shard migration / rate-limiting are all already kernel-side. Notebar hasn't wired them up yet. These are **Notebar gaps, not Kimberlite gaps**, and should not be added to v0.6.0 scope.

---

## What Notebar still can't do cleanly on v0.5.1

Grouped by what's actually blocking vs what's cosmetic. File references are in Notebar unless prefixed with `kimberlite/`.

### Kernel-shaped features Notebar currently hand-rolls

1. **Schema evolution.** Every projection is declared in a giant `CREATE TABLE IF NOT EXISTS` and never changed. `packages/kimberlite-client/src/repos/clinical-note.ts:237-250` has a literal comment about this. `packages/auth/src/better-auth/schema.ts` declares 9 auth tables with no migration story. A real clinic needs to add columns (medicare number, additional demographics, clinical template fields) post-launch.
2. **Idempotent upsert.** Notebar's `upsertRow(table, cols, values)` is an SDK helper that does `UPDATE … WHERE pk = $1; if 0 rows affected then INSERT` (see `kimberlite/sdks/typescript/src/client.ts:405-445` — comment: "Kimberlite does not (yet) support `INSERT ... ON CONFLICT`, so"). Every repo's `afterApply` hook uses it. An `ON CONFLICT (pk) DO UPDATE SET …` kernel path collapses two round-trips to one and is atomic.
3. **`ConsentBasis` wire.** Type exported in SDK (`kimberlite/sdks/typescript/src/compliance.ts:55-60`), field present on `ConsentRecord` (line 76), but `grant()` doesn't accept it and `nativeConsentToRecord` hardcodes `basis: null` (line 472: "wire protocol v4 will carry basis"). GDPR Art 6(1) basis capture is **non-optional** for healthcare — the type being inert is worse than it not existing, because downstream code branches on a field that's always null.
4. **Erasure orchestration.** `packages/kimberlite-client/src/erasure.ts:15-17`: "The SDK doesn't yet ship a one-call `tenant.eraseSubject(subjectId)` so this orchestrator exists." 100-line walk over `SUBJECT_STREAMS`, manually calling `markProgress` + `markStreamErased` per stream. Only `patient_events` is wired today — adding `clinical_note_events`, `appointment_events`, `invoice_events`, `consent_events`, `form_events` is per-aggregate boilerplate.
5. **Audit query surface.** Notebar maintains a **parallel** `audit_events` stream (`packages/kimberlite-client/src/audit.ts`, `notebar-audit.ts`) because Kimberlite's server-side audit ledger is hash-chain-tamper-evident but query-opaque. Compliance teams need "show me every access to patient X by practitioner Y in the last 30 days" — today that's only answerable against the notebar stream.
6. **In-memory SQL filtering (identity surface).** Mostly solved in v0.5.1 (Better Auth adapter refactor is an in-flight notebar task), but the test infrastructure — `packages/testing/src/fakes/fake-kimberlite.ts` + `sql-parser.ts` — exists entirely because Notebar can't run the real kernel fast enough in unit tests. Phase-1 harness (tempdir, child-process) is too slow and too heavy for the ~hundreds of repo tests that run per suite.
7. **Multi-table write coordination.** `packages/auth/src/better-auth/txn-runner.ts` runs Better Auth's multi-table mutations under optimistic concurrency against an `identity_events` stream because there's no BEGIN/COMMIT. For identity (low contention, small surface) this is fine. For the rest of Notebar, the event-sourced pattern already gives atomicity per aggregate — **multi-aggregate transactions are genuinely not needed.** (Flagging this to remove it from v0.6.0 scope.)

### Capability gaps that block specific clinic features

8. **Correlated subqueries.** Reports pattern "patients who have consented to `HealthcareDelivery`" naturally expresses as `SELECT p.* FROM patient_current p WHERE EXISTS (SELECT 1 FROM consent_current c WHERE c.subject_id = p.id AND c.purpose = 'HealthcareDelivery' AND c.withdrawn_at IS NULL)`. Planner today surfaces column-not-found per ROADMAP.md:343-348. Without correlated subqueries Notebar either pulls all rows and filters in-app (bad at clinic-scale) or maintains a denormalised projection per report.
9. **`AS OF TIMESTAMP` time-travel.** Healthcare audit investigations routinely need "what did this record look like on date X?" The kernel already retains every event — exposing `SELECT … FROM patient_current AS OF TIMESTAMP '2026-01-01T00:00:00Z' WHERE id = $1` makes this a first-class SQL feature rather than a replay-by-hand exercise.
10. **Column-level masking.** Exports, reports, and break-glass queries all need PHI redaction. Without kernel-side masking policies, Notebar hand-rolls redaction in every export route (`apps/web/app/routes/*/reports*`) — fragile, inconsistent, and audit-fragile. A `CREATE MASKING POLICY` DDL plus per-role application resolves this.
11. **`DROP TABLE IF EXISTS` + bulk-delete semantics.** `docs/operating/kimberlite-upstream-queue.md:10-30` tracks these. Low-impact (test infrastructure only) but trivial to fix.

### Not gaps — already addressed or out of scope

- JSON ops, recursive CTEs, new JOIN types, set ops, aggregate `FILTER`, window functions — all shipped v0.5.0 and available when needed.
- `ILIKE` / `NOT ILIKE` / `NOT LIKE`, scalar fns in WHERE/SELECT — shipped v0.5.1; server-side filtering refactor in-flight on Notebar side.
- Retry / reconnect / error taxonomy — shipped in SDK; Notebar's `retry.ts` is a 3-line re-export.
- Subscribe / retention / rate limiting / break-glass / shard migration — shipped in v0.9.x, not yet wired up on Notebar side (Notebar's problem).
- Multi-statement BEGIN/COMMIT transactions — explicitly out of scope for Notebar (event-sourced pattern + optimistic concurrency covers it).

---

## Proposed v0.6.0 scope

Three tiers. **Tier 1 is blocking for Notebar to claim "clinic management complete."** Tier 2 removes healthcare-specific cliff edges. Tier 3 is polish.

---

### Tier 1 — blocking

#### 1. `ALTER TABLE ADD/DROP COLUMN` end-to-end execution

**Already on ROADMAP.md:332-337.** Parser works; kernel command path, replay handler, and projection rebuild still missing.

**Acceptance:**
- `ALTER TABLE patient_current ADD COLUMN medicare_number TEXT;` executes server-side.
- The new column appears in `_tables` catalog, survives server restart, and is returned by subsequent `SELECT *`.
- `ALTER TABLE … DROP COLUMN …` likewise, with the column removed from replay stream metadata (existing event payloads retain the field; the projection just no longer stores it).
- Idempotent: re-running an add on an existing column returns a clear `ColumnAlreadyExists` error, not silent success.
- VOPR scenario: add column + concurrent inserts + crash → recovery preserves schema.
- Doc-test in `crates/kimberlite-query/src/lib.rs` demonstrates the round-trip.

**Design note:** Notebar has no opinion on whether projection rebuild is eager or lazy. Either works, provided subsequent queries return the declared shape.

---

#### 2. Wire protocol v4 — end-to-end `ConsentBasis`

**Already on ROADMAP.md:354-371.** Bumping `PROTOCOL_VERSION = 3 → 4` in `crates/kimberlite-wire/src/frame.rs`, plumbing `Option<GdprArticle>` through `ConsentGrantRequest` + `ConsentRecord` wire messages → Node FFI `consent_grant` → TS/Python/Rust SDK `grant(basis)` → server handler.

**Acceptance:**
- TS SDK: `client.compliance.consent.grant(subjectId, purpose, basis?: ConsentBasis)` accepts basis. Python + Rust parity.
- Round-trip: `grant(subjectId, purpose, { article: 'Consent', justification: 'Patient self-service form submission' })` followed by `list(subjectId)` returns the basis intact.
- `check(subjectId, purpose)` can filter by basis if needed (not required, but shouldn't regress).
- **Back-compat:** v3 client against v4 server → basis encoded as `None` on the wire, decoder tolerates missing field. v4 client against v3 server → clear "server too old" error, not silent data loss. Test both directions.
- `nativeConsentToRecord` no longer hardcodes `null`; the TODO at `compliance.ts:469-472` is removed.
- Persists through backup/restore.

**Why blocking:** Notebar ships without GDPR Art 6 basis capture is a compliance gap. The current half-shipped state is worse than nothing because it misleads downstream code.

---

#### 3. `ON CONFLICT` / UPSERT

**Already on ROADMAP.md:338-342.**

**Acceptance:**
- `INSERT INTO patient_current (id, given_names, family_name, updated_at) VALUES ($1, $2, $3, $4) ON CONFLICT (id) DO UPDATE SET given_names = EXCLUDED.given_names, family_name = EXCLUDED.family_name, updated_at = EXCLUDED.updated_at;` executes atomically.
- `DO NOTHING` variant supported (`ON CONFLICT (pk) DO NOTHING`).
- Returns accurate `rowsAffected` and supports `RETURNING`.
- SDK `upsertRow` helper deprecated (keep it working, mark `@deprecated` pointing at `ON CONFLICT`).
- Doc-test covering both `DO UPDATE` and `DO NOTHING`.
- VOPR scenario: concurrent upserts on same pk converge to a deterministic final value.

**Why blocking:** Every single Notebar repo's `afterApply` hook uses `upsertRow`. Collapsing it to one SQL statement removes the two-round-trip pattern and makes projection updates atomic with their event append path (given v0.6.0's transactional semantics of a single statement).

---

#### 4. Test harness phase 2 — `StorageBackend` trait + `MemoryStorage` impl

**Already on ROADMAP.md:372-382.**

**Acceptance:**
- `trait StorageBackend` abstracts `kimberlite-storage::Storage`.
- `MemoryStorage` implementation with no disk I/O.
- `Kimberlite::in_memory()` constructor + `Backend::InMemory` in the test harness.
- TS: `createTestKimberlite({ backend: 'memory' })`. Python + Rust parity.
- Existing e2e tests pass against both `TempDir` and `InMemory` backends (harness smoke test proves this).
- Performance target: `InMemory` harness startup + 1k inserts + 1k selects completes in **< 200ms** on CI hardware (current `TempDir` budget is ~1s per test — far too slow for the ~300 Notebar repo tests).
- VOPR: in-memory backend exercised in short-horizon fuzz scenarios.

**Why blocking:** Notebar maintains `packages/testing/src/fakes/fake-kimberlite.ts` + `sql-parser.ts` (~600 lines of regex-SQL drift) *entirely* because the tempdir harness is too slow for per-test-case usage. An in-memory backend lets Notebar delete the whole fake and run every repo test against the real parser/executor. This is the single biggest drift-elimination lever.

---

#### 5. Correlated subqueries

**Already on ROADMAP.md:343-348.**

**Acceptance:**
- `SELECT a.* FROM t_a a WHERE EXISTS (SELECT 1 FROM t_b b WHERE b.a_id = a.id)` executes correctly.
- `WHERE col IN (SELECT c FROM t_b b WHERE b.a_id = a.id)` correlated form.
- Works for `EXISTS`, `NOT EXISTS`, `IN (SELECT)`, `NOT IN (SELECT)`.
- Decorrelation where the planner can statically prove semi-join equivalence; correlated-loop fallback otherwise.
- Cardinality guard: reject queries whose inner-loop cardinality × outer-loop cardinality exceeds a configurable cap (default 10M row-evaluations).
- Doc-test: the healthcare example above (`patients who have consented`) runs end-to-end.
- Design doc in `docs/reference/sql/correlated-subqueries.md` covers the semi-join semantics.

**Why blocking:** Several reporting and RBAC-filtering shapes in Notebar naturally want correlation. Without it we either denormalise projections per report (schema drift) or pull everything into the app tier (defeats having a DB).

---

### Tier 2 — strongly recommended for healthcare completeness

#### 6. `AS OF TIMESTAMP` time-travel

**On ROADMAP.md:327-328.** `docs/reference/sql/queries.md:28` already flags it.

**Acceptance:**
- `SELECT * FROM patient_current AS OF TIMESTAMP '2026-01-01T00:00:00Z' WHERE id = $1` returns the projection state as of that wall-clock instant.
- Supports TS `toISOString()` format, `Instant` bigint nanos, and `AT OFFSET n` (event-offset form; already partially shipped).
- Error clearly when asking for a timestamp before the stream's retention horizon.
- Works through SDK: `client.queryAt(sql, params, timestamp)`.
- Doc-test demonstrating a point-in-time patient lookup.

**Why strongly recommended:** Audit investigations, regulatory subpoenas, and "undo" workflows are first-class healthcare needs. The kernel already retains the events — making this SQL surface closes a loop the ops team will otherwise hand-roll.

---

#### 7. Column-level masking policy CRUD

**On ROADMAP.md:329** (deferred in `docs/reference/sdk/parity.md:83`).

**Acceptance:**
- `CREATE MASKING POLICY phi_redact AS CASE WHEN session_role = 'clinician' THEN @col ELSE '***' END` (or similar — final DDL shape up to Kimberlite).
- `ALTER TABLE patient_current ALTER COLUMN given_names SET MASKING POLICY phi_redact;`
- Queries hitting masked columns under non-privileged roles return the redacted form.
- Break-glass query path bypasses masking and emits an audit record (already shipped) — masking policy behaviour must compose cleanly with break-glass.
- SDK: `client.admin.maskingPolicy.{create,alter,drop,list}()`.
- Doc-test: report query under clinician role returns values; under reception role returns `'***'`.

**Why strongly recommended:** Every export path in Notebar (bookings CSV, AR summary, patient list) needs role-aware redaction. Today that's app-tier code, which is the exact pattern that leaks PHI in practice. Moving it to the kernel gives us one enforcement point, auditable.

---

#### 8. `tenant.eraseSubject(subjectId)` one-call helper

**Referenced in `packages/kimberlite-client/src/erasure.ts:15-17`** but not yet on ROADMAP.md. Proposing adding it.

**Acceptance:**
- `client.compliance.eraseSubject(subjectId)` walks every stream that carries records for that subject (inferring from stream metadata: streams tagged `PHI` or `PII` with a configurable `subject_id` column).
- Opens the request, marks progress per stream, marks erased, completes, and returns the signed receipt — one call.
- Custom stream coverage override: `eraseSubject(subjectId, { streams: [...] })` for cases where the auto-walk is wrong.
- Idempotent — calling again after completion returns the existing receipt.
- Doc-test demonstrating single-call erasure across multiple streams.

**Why strongly recommended:** Notebar's `erasure.ts` orchestrator is 100 lines of walk-loop-mark-complete boilerplate. A single kernel-level call collapses it and removes the per-aggregate coverage gap (today only `patient_events` is wired; the rest are incrementally-written).

---

#### 9. Audit log query surface

**Not on ROADMAP.md.** Proposing adding it.

**Observation:** Kimberlite's audit log is hash-chain tamper-evident. That guarantee is preserved by append-only semantics, not query-opaqueness — a read API that returns non-PHI structured entries does **not** break the chain. The current opacity forces every consumer to maintain a parallel audit stream for queries, which is ironic given the kernel already stores it.

**Acceptance:**
- `client.compliance.audit.query({ subjectId?, actor?, action?, fromTs?, toTs?, limit? })` returns structured rows `{ actor, action, subjectId, correlationId, requestId, occurredAt, reason, changedFieldNames }`.
- Never returns PHI values — only field *names* and opaque IDs.
- Filters on server-side (performant for multi-year logs).
- Streaming form: `client.compliance.audit.subscribe({...})` using the shipped Subscribe primitive.
- Doc-test: query "every access to subject X in the last 30 days" returns structured audit trail.

**Why strongly recommended:** Eliminates ~150 lines of Notebar's parallel audit layer (`audit.ts`, `notebar-audit.ts`) and removes the double-write cost on every mutation. This is the second-biggest drift-elimination lever after the in-memory test backend.

---

### Tier 3 — defer if time is tight

#### 10. `DROP TABLE IF EXISTS` + `DROP TABLE` semantics + `DELETE FROM t` (no WHERE) rowsAffected

Three small fixes from `docs/operating/kimberlite-upstream-queue.md:10-30`. Test-infrastructure-only impact. Useful for Notebar's `wipeIdentityTables` cleanup loop but not blocking.

#### 11. Auto-generated traceability matrix

`AUDIT-2026-04` / `AUDIT-2026-NN` in-source markers → generated matrix doc. Dev ergonomics. ROADMAP.md:352-353.

#### 12. Dep bumps: `sqlparser 0.54 → 0.61`, `printpdf 0.7 → 0.9`

Maintenance, internal. ROADMAP.md:383-398. Do these as their own commits to keep diffs reviewable.

---

## Explicitly out of scope for v0.6.0

Push these to v0.7.0+ unless otherwise motivated:

- **Multi-statement transactions (BEGIN/COMMIT/ROLLBACK).** Notebar's event-sourced + optimistic-concurrency pattern doesn't want them. ROADMAP.md:349-350 already flags "re-evaluate against v1.0." Skip.
- **Foreign keys, check constraints, generated columns.** Reducer-style events encode invariants in the app layer; notebar doesn't need kernel-enforced referential integrity.
- **Stored procedures, triggers.** Same reason.
- **Full-text search.** Not needed for current notebar scope.
- **MOD / POWER / SQRT / SUBSTRING / EXTRACT / DATE_TRUNC / NOW().** Listed as v0.5.1 non-goals (CHANGELOG:186-189). Clock-threading decision for VOPR-sim-vs-wall-clock is a separate design conversation. Skip unless a Notebar surface needs them.
- **VOPR scenarios for SQL-surface semantics.** Good to have; not blocking Notebar correctness.
- **Formal-verification specs for scalar-expression purity.** Same.
- **ML data classification wiring on Notebar side.** Kimberlite has shipped it (v0.9.0); Notebar hasn't adopted it. Notebar's problem.
- **Retention policy / legal hold wiring on Notebar side.** Same — kernel has it (v0.9.0, `RetentionEnforcer`), Notebar hasn't adopted it.
- **Subscribe operation wiring on Notebar side.** Same — kernel has it (v0.9.0). Would make reminder/booking-promoter workers reactive, but that's a Notebar build-out, not a Kimberlite gap.

---

## Suggested phasing

```
Week 1-2:  Tier 1 #4 (StorageBackend + MemoryStorage)
           ─ Unblocks faster feedback on everything else.
           ─ Harness phase-1 API already stable; this is the backend swap.

Week 2-4:  Tier 1 #1 (ALTER TABLE e2e)  ──┐ Can run in parallel — different
Week 2-4:  Tier 1 #3 (ON CONFLICT)       ─┤ layers of the kernel; share the
                                          │ new kernel-command machinery.

Week 4-5:  Tier 1 #2 (Wire protocol v4 + ConsentBasis)
           ─ Isolated to wire + FFI + SDK layers; doesn't block kernel work.

Week 5-7:  Tier 1 #5 (Correlated subqueries)
           ─ Needs design doc first. Planner-heavy; deserves its own slot.

Week 7-8:  Tier 2 #6 (AS OF TIMESTAMP), #8 (eraseSubject), #9 (audit query)
           ─ Each is a relatively thin wrapper over existing kernel facilities.

Week 8-9:  Tier 2 #7 (masking policy CRUD)
           ─ New DDL; touches parser, planner, executor. Bigger piece.

Week 9-10: Tier 3 cleanup + dep bumps + release engineering.
```

This is a **10-week v0.6.0**, consistent with the scope. If time runs short, drop Tier 2 #7 (masking) first — it's the largest single item and has the highest "can defer" argument given Notebar's exports can be manually redacted in the short term.

---

## Acceptance: "Notebar can claim clinic management complete"

When v0.6.0 ships with Tier 1 + Tier 2 (#6, #8, #9), Notebar can land a single migration commit that:

1. Bumps SDK to `@kimberlitedb/client@0.6.0`.
2. **Deletes** `packages/testing/src/fakes/fake-kimberlite.ts` + `sql-parser.ts` (~600 lines). Rewrite remaining repo tests against `createTestKimberlite({ backend: 'memory' })`.
3. **Deletes** `packages/kimberlite-client/src/erasure.ts` orchestrator (~120 lines). Replace call sites with `client.compliance.eraseSubject(subjectId)`.
4. **Deletes** `packages/kimberlite-client/src/audit.ts` + `notebar-audit.ts` parallel audit stream (~150 lines). Query the kernel audit log directly.
5. **Deletes** `packages/kimberlite-client/src/port.ts::upsertRow` + its adapter plumbing. Replace ~31 call sites with `INSERT … ON CONFLICT … DO UPDATE`.
6. Plumbs `ConsentBasis` through `domain/consent.ts` + `ConsentRepoScoped.recordGrant` + event payload.
7. Ships a real schema migration story (versioned `ALTER TABLE` scripts per package).
8. Optionally adopts masking policies (Tier 2 #7) on PHI columns across exports.
9. Starts using correlated subqueries for the `patients-with-consent`, `appointments-with-active-consent`, and `audit-of-patient-access` report shapes.
10. Uses `AS OF TIMESTAMP` for audit-investigation admin screens.

**Lines of Notebar code expected to delete:** ~1000+ (fakes + orchestrator + parallel audit + upsert plumbing). Line-for-line replacement with direct kernel calls.

**What Notebar will still need to build after v0.6.0 (not Kimberlite's concern):**

- Retention enforcement worker (uses kernel's `RetentionEnforcer`).
- Reactive reminder/booking-promoter workers (uses kernel's `Subscribe`).
- Admin UI for break-glass queries (uses kernel's `queryBreakGlass`).
- Legal-hold toggle in compliance dashboard.
- ML-classification adoption if needed (probably not for current scope).
- Feature-flag surface (entirely Notebar-side).
- Consent UI that captures basis.
- Schema migration scripts per projection.

---

## Notes on scope discipline

Two things to consciously **not** add to v0.6.0, despite temptation:

1. **"Just one more SQL feature."** The v0.5.0 + v0.5.1 SQL uplift was massive. Notebar has all the query shapes it needs. Resist scope creep on subqueries / JSON / window functions — the Tier 1 / Tier 2 list is enough.

2. **Cross-aggregate ACID transactions.** Event-sourced systems don't want them. Every time a consumer thinks they do, the right answer is usually "model the aggregate boundary differently." Leave BEGIN/COMMIT for v1.0 or skip entirely.

---

## Critical files (Kimberlite side) for reference

- `ROADMAP.md:325-399` — existing v0.6.0 scope definition (this plan aligns with and extends it).
- `CHANGELOG.md:184-197` — v0.5.1 non-goals (confirms Tier 1 items).
- `crates/kimberlite-query/src/parser.rs` — ALTER TABLE parser exists, kernel execution doesn't.
- `crates/kimberlite-wire/src/{frame.rs,message.rs}` — wire protocol v3, needs v4 bump for ConsentBasis.
- `sdks/typescript/src/compliance.ts:55-76,144-151,459-474` — ConsentBasis type/field exported but inert.
- `sdks/typescript/src/client.ts:405-445` — `upsertRow` helper with "does not support ON CONFLICT" comment.
- `crates/kimberlite-test-harness/` — phase-1 harness; phase-2 swaps backend.
- `crates/kimberlite-storage/` — where `StorageBackend` trait + `MemoryStorage` land.
