# 2026 Q2 Release-Readiness Campaign — v0.4.2 through v0.6.0

**Status:** Closed (v0.6.0 shipped 2026-04-21).
**Scope:** Narrative record of the three-release remediation arc that took
Kimberlite from "v0.4.0 claims ≠ code" to "v0.6.0 Notebar-ready — every
compliance feature live and kernel-side."
**Classification:** Internal — audit trail.

## Why this file exists

Between April 18 and April 22, 2026 the project ran two overlapping
audit streams against the v0.4.0 baseline:

- **AUDIT-2026-04** — compliance-claim gaps (25 findings: 4 Critical,
  5 High, 9 Medium, 7 Low). Source doc:
  `docs-internal/audit/AUDIT-2026-04.md`.
- **Release-readiness audit** — truth-in-advertising / OSS publish gates
  (18 numbered fixes across features, docs, website, install script,
  formal-verification claims, crates.io publish workflow). Source doc:
  `docs-internal/audit/AUDIT_RELEASE_READINESS_APRIL_2026.md`.

The CHANGELOG originally carried full per-PR write-ups of both streams
across three `[Unreleased]` blocks (≈3,000 lines). That was removed in
the v0.6.0 pre-publish cleanup — git history holds the full content;
this file captures the narrative and the rubric for closing out similar
campaigns in the future.

## Campaign shape

| Stage   | Release | Tag        | Date       | What landed                                                                 |
|---------|---------|------------|------------|-----------------------------------------------------------------------------|
| Phase 0 | v0.4.0  | (baseline) | 2026-02-03 | VOPR advanced debugging; pre-audit baseline.                                |
| Phase 1 | v0.4.1  | v0.4.1     | 2026-02-04 | Docker-only hotfix; no workspace bump.                                      |
| Phase 2 | v0.4.2  | v0.4.2     | 2026-04-20 | Truth-in-advertising patch + 9-phase SDK production launch + 7-PR compliance remediation. No behaviour changes; all feature work scheduled forward. |
| Phase 3 | v0.5.0  | v0.5.0     | 2026-04-21 | SQL coverage uplift (9 phases), Phase 6 compliance server handlers, ALTER TABLE end-to-end, TLAPS PR-gating, VOPR nightly hard-fail, fuzz nightly (20 targets), fuzz-to-types hardening campaign, AUDIT-2026-04 remediation (all 4 C + 5 H closed). |
| Phase 4 | v0.5.1  | v0.5.1     | 2026-04-21 | DX point release — scalar expressions, NOT IN / ILIKE family, SELECT alias preservation, test-harness phase 1, SDK package rename (`@kimberlite/client` → `@kimberlitedb/client`). |
| Phase 5 | v0.6.0  | v0.6.0     | 2026-04-21 | Final feature-complete push — ON CONFLICT, correlated subqueries, AS OF TIMESTAMP resolver, masking policy CRUD, audit query SDK, consent basis, eraseSubject auto-discovery, StorageBackend trait + MemoryStorage, wire protocol v4, sqlparser 0.54→0.61, printpdf 0.7→0.9. Plus the pre-publish audit fix-ups documented below. |

## AUDIT-2026-04 findings — closure index

All 25 findings closed by v0.5.0 tag. One-line summary per finding;
full detail in `AUDIT-2026-04.md` and git log.

**Critical:**
- `C-1` — `ErasureExecutor` trait + signed `ErasureCompleted` witness
  emitted as a single kernel effect. SDK surface `client.compliance.eraseSubject()`
  in all three languages. Auto-discovery of affected streams deferred to
  v0.6.0 (closed commit `055a468`).
- `C-2` — `TenantIsolationChecker::verify_catalog_isolation` wired into
  the VOPR main loop. `sim-canary-catalog-cross-tenant` feature proves
  the wire fires.
- `C-3` — Phase 6 compliance endpoints (`audit_query`, `export_subject`,
  `verify_export`, `breach_*`) implemented server-side; SDK wrappers
  follow.
- `C-4` — Hash-chained tamper-evident `ComplianceAuditLog` backed by
  `trait AuditStore` + durable storage.

**High:**
- `H-1` / `M-3` — Trace validator replaces tautological
  `trace_alignment::calculate_coverage`. `syn`-backed AST check against
  real source tree; four stale `(200, 350)` line drifts corrected.
- `H-2` — (closed under C-4 umbrella)
- `H-3` — Linearizability "proof" demoted to liveness proxy in code +
  docs. Roadmap captures the chaos-test follow-up.
- `H-4` — 92 TLAPS compliance meta-theorems moved from
  nightly-aspirational to PR-gated (subset) + nightly (rest).
- `H-5` — Seal/unseal tenant lifecycle gains a single audit effect per
  command; tenant-scoped mutation rejection when sealed.

**Medium and Low:** 16 findings closed across the v0.5.0 / v0.5.1 /
v0.6.0 cycle; see PR numbers in commit messages prefixed `audit-2026-04`.

## Release-readiness audit — 18 fixes

Originally shipped as the v0.4.2 truth-in-advertising patch. Categorical
summary (no behaviour changes — all claim/doc/script fixes):

1. **Compliance certification language** — scrubbed "-compliant" / "-certified"
   across `docs/**`, `crates/**`, `README.md`, `CLAUDE.md`, `SECURITY.md`.
   Replaced with "-ready" + framework-specific qualifiers.
2. **Formal-verification claims** — "world's first / only database with
   complete 6-layer formal verification" and "136+ proofs" replaced with
   an honest decomposition (~91 Kani proofs PR-gated, ~25 core TLA+ theorems,
   Coq specs, Alloy models, Ivy invariants, MIRI).
3. **Assertion-count claim** — "38 production assertions" removed everywhere;
   replaced with pointer to `docs/internals/testing/assertions-inventory.md`
   (which uses `grep` as source of truth).
4. **VOPR scenario count** — "46 scenarios" corrected to 74 enum variants,
   ~50 substantive + ~24 scaffolded.
5. **"90-95% Antithesis-grade"** framing removed; replaced with
   "Antithesis-inspired deterministic simulation."
6. **Performance claim** — removed "sacrifices 10-50% write performance"
   number (sourced from v0.2.0 baseline); pointer to v0.5.0 re-baseline.
7. **Time-travel consistency** — `README.md` Quick Start now uses the
   working `AT OFFSET` example.
8. **Framework readiness percentages** ("HIPAA 98%, GDPR 95%, SOC 2 90%")
   replaced with "designed for / ready / no audit completed" framing.
9. **TigerBeetle attribution** clarified — VSR is "adapted from TigerBeetle's
   architecture, extended with multi-tenant routing, clock synchronization,
   and repair-budget policies" — does not inherit their battle-testing.
10-12. **Website fixes** — home.html "Latest Release" bumped from v0.1.0,
    download.html fictional version pin fixed, blog posts 006 + 008
    retitled with post-hoc corrections.
13-17. **install.sh** — SHA-256 checksum verification, `kmb` alias symlink,
    `KIMBERLITE_SKIP_CHECKSUM` escape hatch, `verify_checksum()` POSIX
    local-variable bug fix, mirror re-sync.
18. **Release publish workflow** — `cargo publish … | tee` masking cargo
    failures fixed with `set -euo pipefail` + `PIPESTATUS[0]`; publish
    order expanded from 6 crates to full 24-crate topological order in
    6 tiers with 30s settle delays; `kimberlite-doc-tests` marked
    `publish = false`.

## Fuzz-to-Types hardening campaign (Apr 2026)

Separate but contemporaneous campaign that converted 5 real bugs the
first EPYC nightly fuzz run found from patched-conditionals into
type-system guarantees. Full design doc:
`docs-internal/design-docs/fuzz-to-types-hardening-apr-2026.md`.

Headline deliverables (all shipped with v0.5.0):

- `kimberlite-types::domain` module — `NonEmptyVec<T>`,
  `SqlIdentifier`, `BoundedSize<const MAX: usize>`, `ClearanceLevel`.
- Structure-aware `fuzz_wire_typed` + `fuzz_vsr_typed` targets —
  coverage reaches handlers immediately instead of being
  framing-rejected at ~99% rate.
- UBSan nightly campaign on the EPYC box, 4h offset from ASan.
- Constructor audit eliminating ~40 sites where invariants lived
  outside the type boundary.

## v0.6.0 pre-publish audit (2026-04-22)

Final gate before remote writes. Findings + fixes folded into
`v0.6.0 → 9b5658f`:

- **Blocker B1** — 4 masking CRUD postcondition `assert!`s lacked
  paired `#[should_panic]` tests. Fixed by adding mirror-assertion
  tests per `docs/internals/testing/assertions-inventory.md` policy.
- **Blocker B2** — 10 RustSec advisories unallowed by `deny.toml`.
  Fixed by `cargo update -p aws-lc-rs -p rustls-webpki -p tar` +
  justified ignores for the 4 remaining unmaintained transitives
  (kuchiki, bincode, fxhash, proc-macro-error — all printpdf
  transitives) + new 10th gate in `just release-dry-run`.
- **Blocker B3** — `docs/reference/sql/queries.md` + `overview.md`
  said AS OF TIMESTAMP was "not yet implemented" despite shipping.
  Rewritten to document shipped semantics.
- **C1** — Sub-workspace Cargo.locks (`fuzz/`, `examples/rust/`,
  `website/`) not regenerated by `bump-version`. Extended the
  `bump-version` recipe; fixed `fuzz_kernel_command.rs` missing
  `tenant_id` fields and bumped fuzz sqlparser pin 0.54→0.61.
- **C2** — Added `### Breaking Changes` subsection at top of
  v0.6.0 CHANGELOG block covering wire v3→v4 and ABAC rule
  uniqueness.
- **C3** — New `docs/coding/migration-v0.6.md`.
- **C4** — Logged VOPR workload-generator coverage gap (Masking*,
  Upsert, AS OF, eraseSubject) to ROADMAP v0.7.0 deferred.
- **Drive-bys** — pre-existing clippy + ban failures at the original
  v0.6.0 tag: `items_after_statements` in upsert test,
  identical-match-arm in Query/QueryAt client dispatch,
  wildcard-for-single-variant in ON CONFLICT parser tests,
  `rand_core 0.5.1` ban failure (rand_pcg + rand_hc transitives via
  printpdf).

## Rubric for future release-readiness audits

Use this file as the template shape. Each campaign should capture:

1. **Why the audit ran** — what prompted it, what the scope boundary was.
2. **Finding list with closure index** — one line per finding, severity,
   closure commit/PR.
3. **Category summary** — not per-PR write-ups. Link to the design doc
   or git log for detail.
4. **Narrative across releases** — which release closed what. A table
   of releases + themes, not per-release bullet dumps.

The CHANGELOG itself is for user-facing release notes, not audit
trails. This file is where audit trails live.
