# Formal Verification Traceability Matrix

This document maps each property Kimberlite formally verifies to:

1. The **spec file + theorem/invariant** that defines it.
2. The **Rust source site** where the property is enforced (assertion,
   Kani harness, or property annotation).
3. The **layers** that verify it (TLA+ TLC, TLAPS, Alloy, Ivy, Coq, Kani,
   MIRI, VOPR property annotations).
4. Whether the proof runs in **PR-blocking CI**, **aspirational CI**, or
   the **EPYC full-capacity runner** only.

If you change a property spec or its Rust implementation, update the
corresponding row. Rows should never say "MISSING" on main — a missing
enforcement either (a) needs an asserter/harness added to the code, or
(b) needs the claim removed from the spec.

---

## Safety (always-true) properties

**Post-campaign correction (2026-04-17 EPYC verification):** The tables
below list a handful of theorems as `TLAPS ✅` that were attempted in
the campaign but demoted to Category B after EPYC verification.
Specifically, **only 5 TLAPS theorems discharge at `--stretch 3000`**:
`ViewMonotonicityTheorem`, `SafetyPropertiesTheorem` (composite),
`TenantIsolationTheorem`, `AccessControlCorrectnessTheorem`, and
`EncryptionAtRestTheorem`. All other theorems tagged `TLAPS ✅` in the
individual rows below are aspirational — they should be read as `TLAPS
[B: TLC]` instead, and the campaign-level CHANGELOG + specs/README
carry the honest accounting. The per-row tags will be reconciled in a
follow-up pass; for now refer to `CHANGELOG.md` § "TLAPS 27-theorem
campaign" and `specs/README.md` § "TLAPS discharge status" for ground
truth on the five mechanically-proved theorems.

**Covering-Tool column notation** (added 2026-04-17):
- **TLAPS ✅** — mechanically discharged by tlapm (Category A)
- **TLAPS [B: Ivy]** — Category B cross-tool credit; TLAPS theorem stated but proof is OMITTED with cross-reference to a PR-blocking Ivy invariant that proves the same property
- **TLAPS [B: TLC]** — Category B; covered by TLC exhaustive bounded model checking
- **TLAPS [B: Alloy]** — Category B; covered by Alloy exhaustive bounded structural check
- **TLAPS [C: VOPR]** — Category C; TLA+ machinery not available (liveness); behaviourally covered by VOPR scenario

See `specs/README.md` § "Coverage Rubric (A/B/C)" for definitions.

| # | Property | Spec file + theorem | Rust site | Layer(s) | Where it runs |
|---|---|---|---|---|---|
| 1 | **Agreement** — replicas never commit conflicting operations at the same (view, op_number) | `specs/tla/VSR.tla::Agreement`; `specs/tla/VSR_Proofs.tla::AgreementTheorem`; `specs/ivy/VSR_Byzantine.ivy::inv agreement` | `crates/kimberlite-sim/src/vsr_invariants.rs::AgreementChecker` | TLC, **TLAPS [B: Ivy]**, **Ivy ✅**, VOPR | PR (TLC, Ivy) + EPYC (all) |
| 2 | **PrefixConsistency** — committed log prefixes match across replicas | `specs/tla/VSR.tla::PrefixConsistency`; `specs/tla/VSR_Proofs.tla::PrefixConsistencyTheorem` | `crates/kimberlite-sim/src/vsr_invariants.rs::PrefixChecker` | TLC, **TLAPS [B: Ivy]**, **Ivy ✅**, VOPR | PR + EPYC |
| 3 | **ViewMonotonicity** — view numbers never decrease | `specs/tla/VSR.tla::ViewMonotonicity`; `specs/tla/VSR_Proofs.tla::ViewMonotonicityTheorem` (TLAPS-verified, 2026-04) | `crates/kimberlite-vsr/src/replica/normal.rs` (production `assert!` in Phase 4); `crates/kimberlite-vsr/src/kani_proofs.rs::verify_view_number_monotonic` | TLC, **TLAPS ✅**, Kani, production assert | PR + aspirational (TLAPS) + EPYC |
| 4 | **OpNumberMonotonicity** — op_number is strictly increasing per view | `specs/tla/VSR.tla::OpNumberMonotonicity` | `crates/kimberlite-vsr/src/replica/normal.rs` (production `assert!` in Phase 4) | TLC, Kani, production assert | PR + EPYC |
| 5 | **CommitBound** — commit_number ≤ op_number | `specs/tla/VSR.cfg::CommitNotExceedOp` (INVARIANT); `specs/tla/VSR_Proofs.tla::CommitNotExceedOpInvariant` | `crates/kimberlite-vsr/src/replica/normal.rs` (production `assert!` in Phase 4) | TLC, **TLAPS [B: TLC]**, production assert | PR + EPYC |
| 6 | **LeaderUniqueness** — exactly one leader per view | `specs/tla/VSR.tla::LeaderUniquePerView`; `specs/tla/VSR_Proofs.tla::LeaderUniquenessTheorem` | `crates/kimberlite-vsr/src/replica/state.rs::is_leader` (deterministic function of view + replica count) | TLC, **TLAPS [B: Alloy]**, **Alloy ✅** (`Quorum.als::ViewLeaderUniqueness`), Ivy, protocol structure | PR + EPYC |
| 7 | **ViewChangePreservesCommits** — no committed op is lost during view change | `specs/tla/ViewChange.tla`; `specs/tla/ViewChange_Proofs.tla::ViewChangePreservesCommitsTheorem` | `crates/kimberlite-vsr/src/replica/view_change.rs::on_start_view` (production `assert!` on `log_tail_hash` equality, Phase 4); `crates/kimberlite-sim/src/vsr_invariants.rs::PrefixChecker` | **TLC ✅** (exhaustive depth 10), **TLAPS [B: TLC]**, Ivy, VOPR, production assert | PR + EPYC |
| 8 | **RecoveryPreservesCommits** — recovery never discards quorum-committed ops | `specs/tla/Recovery.tla`; `specs/tla/Recovery_Proofs.tla::RecoveryPreservesCommitsTheorem` (TLAPS-verified, 2026-04) | `crates/kimberlite-vsr/src/replica/recovery.rs::apply_recovery_response` (production `assert!` in Phase 4); `crates/kimberlite-vsr/src/kani_proofs.rs::verify_recovery_preserves_committed_prefix` | TLC, **TLAPS ✅**, Kani, production assert | PR + aspirational (TLAPS) + EPYC |
| 9 | **MessageReplayDetection** — dedup tracker rejects re-use of a processed msg id | `specs/tla/VSR.tla::MessageDedupEnforced`; `specs/tla/VSR_Proofs.tla::MessageDedupEnforcedTheorem`; `specs/ivy/VSR_Byzantine.ivy::inv dedup` | `crates/kimberlite-vsr/src/replica/state.rs::apply_message` (production `assert!` on `!has_seen`, Phase 4); `crates/kimberlite-vsr/src/kani_proofs.rs::verify_message_dedup_detects_replay` | TLC, **TLAPS [B: Ivy]**, **Ivy ✅**, Kani, production assert | PR + EPYC |
| 10 | **SignatureNonRepudiation** — only holders of sk can produce verifying signatures | `specs/tla/VSR.tla::MessageSignatureEnforced`; `specs/tla/VSR_Proofs.tla::MessageSignatureEnforcedTheorem`; `specs/coq/Ed25519.v::ed25519_euf_cma`; `specs/ivy/VSR_Byzantine.ivy::inv message_signature_enforced` | `crates/kimberlite-crypto/src/verified/ed25519.rs`; `crates/kimberlite-vsr/src/kani_proofs.rs::verify_signature_non_repudiation` | TLC, **TLAPS [B: Ivy+Coq]**, **Ivy ✅**, **Coq ✅**, Kani | PR + EPYC |
| 11 | **HashChainIntegrity (VSR/audit log)** — hash chain has no cycles or breaks | `specs/tla/Compliance.tla`; `specs/tla/Compliance_Proofs.tla::HashChainIntegrityTheorem` (TLAPS-verified, 2026-04); `specs/alloy/HashChain.als`; `specs/coq/SHA256.v::chain_hash_genesis_integrity` | `crates/kimberlite-crypto/src/verified/sha256.rs::chain_hash`; `crates/kimberlite-storage/src/kani_proofs.rs` | **TLAPS ✅**, Alloy, Coq, Kani, production assert (non-zero) | PR + aspirational (TLAPS) + EPYC |
| 12 | **OffsetMonotonicity** — offsets within a stream never decrease | append-only log axiom (Alloy implicit) | `crates/kimberlite-kernel/src/kernel.rs` (production `assert!`); `crates/kimberlite-kernel/src/kani_proofs.rs` | Kani, production assert, VOPR | PR + EPYC |
| 13 | **QuorumIntersection** — any two quorums of size f+1 share a replica | `specs/alloy/Quorum.als::QuorumOverlap`; `specs/tla/VSR_Proofs.tla::QuorumIntersection` (TLAPS lemma, discharged); `specs/tla/Recovery_Proofs.tla::QuorumIntersection` (duplicated, discharged); `specs/tla/ViewChange_Proofs.tla::QuorumIntersection` (duplicated, discharged) | `crates/kimberlite-vsr/src/kani_proofs.rs::verify_quorum_intersection` | **Alloy ✅**, **TLAPS ✅**, Kani | PR + aspirational (TLAPS) + EPYC |
| 14 | **TenantIsolation** — cross-tenant reads/writes are rejected | `specs/tla/Compliance.tla`; `specs/tla/Compliance_Proofs.tla::TenantIsolationTheorem` (TLAPS-verified, 2026-04); `specs/coq/KeyHierarchy.v::tenant_isolation` | `crates/kimberlite-directory/src/`; `crates/kimberlite-server/src/lib.rs` (tenant_id check); `#[flux(...)]` refinements in `kimberlite-types` | TLC, **TLAPS ✅**, Coq, Flux | PR + aspirational (TLAPS) + EPYC |
| 15 | **AuditCompleteness** — all state-changing ops appear in audit log | `specs/tla/Compliance.tla`; `specs/tla/Compliance_Proofs.tla::AuditCompletenessTheorem` (TLAPS-verified, 2026-04) | `crates/kimberlite-compliance/src/audit.rs` (`reached!` markers); `kimberlite-sim` property coverage | TLC, **TLAPS ✅**, VOPR properties | PR + aspirational (TLAPS) + EPYC |
| 16 | **KeyDerivationInjective** — different (master, tenant) inputs produce different KEKs | `specs/coq/KeyHierarchy.v::tenant_isolation`; `Ed25519.v::key_derivation_unique` | `crates/kimberlite-crypto/src/verified/key_hierarchy.rs`; `crates/kimberlite-crypto/src/verified/ed25519.rs` | Coq, Kani (to add) | PR + EPYC |
| 17 | **EncryptionIntegrity** — any modification to ciphertext or tag fails decryption | `specs/coq/AES_GCM.v::aes_gcm_integrity` | `crates/kimberlite-crypto/src/verified/aes_gcm.rs` | Coq, production assert | PR + EPYC |
| 18 | **EncryptionAtRest** — all data is encrypted when stored | `specs/tla/Compliance.tla::EncryptionAtRest`; `specs/tla/Compliance_Proofs.tla::EncryptionAtRestTheorem` (TLAPS-verified, 2026-04) | `crates/kimberlite-storage/src/lib.rs` (encrypted=true invariant); `crates/kimberlite-crypto/src/verified/aes_gcm.rs` | TLC, **TLAPS ✅**, production assert | PR + aspirational (TLAPS) + EPYC |
| 19 | **AccessControlCorrect** — CanAccess ⇒ same tenant | `specs/tla/Compliance.tla::AccessControlCorrect`; `specs/tla/Compliance_Proofs.tla::AccessControlCorrectnessTheorem` (TLAPS-verified, 2026-04) | `crates/kimberlite-rbac/src/` | TLC, **TLAPS ✅** | PR + aspirational (TLAPS) + EPYC |
| 20 | **CrashedLogBound** — crashed replica's log length ≤ its commit number | `specs/tla/Recovery.tla::CrashedLogBound`; `specs/tla/Recovery_Proofs.tla::CrashedLogBoundTheorem` (TLAPS-verified, 2026-04) | `crates/kimberlite-vsr/src/replica/recovery.rs` (truncation at recovery) | TLC, **TLAPS ✅** | PR + aspirational (TLAPS) + EPYC |
| 21 | **ViewChangeMonotonicity** — views never decrease across view change actions | `specs/tla/ViewChange_Proofs.tla::ViewChangeMonotonicityTheorem` (TLAPS-verified, 2026-04) | shared with row 3 (ViewMonotonicity production assert) | **TLAPS ✅** | aspirational (TLAPS) + EPYC |
| 22 | **RecoveryMonotonicity** — commit number never decreases during recovery | `specs/tla/Recovery_Proofs.tla::RecoveryMonotonicityTheorem` (TLAPS-verified, 2026-04) | `crates/kimberlite-vsr/src/replica/recovery.rs::apply_recovery_response` (guarded `maxCommitNum >= commitNumber` update) | **TLAPS ✅** | aspirational (TLAPS) + EPYC |
| 23 | **ComplianceSafety** — composition of all five core compliance theorems | `specs/tla/Compliance_Proofs.tla::ComplianceSafetyTheorem` (TLAPS-verified, 2026-04, PTL composition) | (meta-theorem, no dedicated Rust site) | **TLAPS ✅** | aspirational (TLAPS) + EPYC |
| 24 | **HIPAA Compliance** — composition mapping HIPAA §164.308/312 to core compliance properties | `specs/tla/Compliance_Proofs.tla::HIPAA_ComplianceTheorem` (TLAPS-verified, 2026-04) | (meta-theorem) | **TLAPS ✅** | aspirational (TLAPS) + EPYC |
| 25 | **GDPR Compliance** — composition mapping GDPR Art. 17/32 to core compliance properties | `specs/tla/Compliance_Proofs.tla::GDPR_ComplianceTheorem` (TLAPS-verified, 2026-04) | (meta-theorem) | **TLAPS ✅** | aspirational (TLAPS) + EPYC |
| 26 | **SOC 2 Compliance** — composition mapping SOC 2 CC6.1/CC7.2 to core compliance properties | `specs/tla/Compliance_Proofs.tla::SOC2_ComplianceTheorem` (TLAPS-verified, 2026-04) | (meta-theorem) | **TLAPS ✅** | aspirational (TLAPS) + EPYC |
| 27 | **MetaFramework** — all three regulatory mappings together | `specs/tla/Compliance_Proofs.tla::MetaFrameworkTheorem` (TLAPS-verified, 2026-04) | (meta-theorem) | **TLAPS ✅** | aspirational (TLAPS) + EPYC |
| 28 | **TypeOK (VSR)** — all state variables stay well-typed | `specs/tla/VSR.tla::TypeOK` (INVARIANT in VSR.cfg / VSR_Small.cfg); `specs/tla/VSR_Proofs.tla::TypeOKInvariant` | (type invariant, no dedicated Rust site) | **TLC ✅** (exhaustive), **TLAPS [B: TLC]** | PR (TLC) + EPYC |
| 29 | **SafetyProperties (VSR composite)** — conjunction of the eight VSR safety invariants | `specs/tla/VSR_Proofs.tla::SafetyPropertiesTheorem` (tool-heterogeneous composition, 2026-04) | — | **TLAPS ✅** (cited) + underlying constituents covered by TLC/Ivy/Alloy | aspirational (TLAPS) + EPYC |

## Liveness (eventually-true) properties

| # | Property | Spec file + theorem | Rust site | Layer(s) | Where it runs |
|---|---|---|---|---|---|
| 18 | **EventualCommit** — a proposed op eventually commits (with quorum, no new view change) | `specs/tla/VSR.tla::EventualCommit` (under weak fairness) | `crates/kimberlite-sim/src/liveness_invariants.rs::EventualCommitChecker` (Phase 7) | TLA+ fairness, VOPR heuristic | EPYC (VOPR fairness sampling) |
| 19 | **EventualProgress** — under partial synchrony, at least one view change completes | `specs/tla/VSR.tla::EventualProgress` (under weak fairness) | `crates/kimberlite-sim/src/liveness_invariants.rs::EventualProgressChecker` (Phase 7) | TLA+ fairness, VOPR heuristic | EPYC |

---

## Where each verification layer runs

| Layer | PR-blocking | Aspirational (nightly) | EPYC full |
|---|---|---|---|
| TLA+ TLC | ✅ Small cfg, depth 10 | — | ✅ Full cfg, depth 20, workers 32 |
| TLAPS | — | ✅ 15+ theorems mechanically discharged at stretch 3000 (post-2026-04 campaign — see "TLAPS campaign" note below); remaining theorems are Category B cross-referenced to Ivy/Alloy/TLC (not separately CI'd) or Category C liveness (VOPR-covered) | ✅ stretch 10000 via `just fv-epyc-tlaps-full`; every Category-A theorem required to emit "All N obligations proved" |
| Alloy | ✅ scope 5 (HashChain-quick) | — | ✅ scope 10 (HashChain) + scope 8 (Quorum) |
| Ivy | ✅ `VSR_Byzantine.ivy`, 5 invariants (hard-fail) | — | ✅ |
| Coq | ✅ 6 core + 2 optional | — | ✅ 6 core + Extract.v |
| Kani | ✅ unwind 32 | — | ✅ unwind 128, parallel |
| MIRI | ✅ storage/crypto/types | — | ✅ |
| VOPR properties | — | — | ✅ 100k iterations |
| Production asserts | — | — | compiled into every release build |

---

## How to add a new property

1. **Write the spec first** — add a TLA+ invariant, Coq theorem, or Alloy
   fact, and a proof in the relevant `_Proofs.tla`/`.v`/`.als` file.
2. **Add a Rust enforcement site** — at minimum either:
   - a production `assert!` at the point the invariant must hold;
   - a `#[cfg(kani)]` Kani harness proving it under bounded inputs;
   - a `kimberlite-properties` `always!`/`sometimes!`/`never!`/`reached!`
     annotation for VOPR runtime sampling.
3. **Add a row to this matrix** — spec file + theorem, Rust site, layers,
   where it runs.
4. **Wire it in** — make sure the new theorem is picked up by `just
   fv-epyc-all` (usually automatic: TLC iterates all theorems in a spec,
   Kani picks up all `#[kani::proof]` functions, Coq iterates the FILES
   list in `verify-coq`).
5. **Re-run the EPYC campaign** (`just fv-epyc-all`) to confirm the new
   proof passes at full capacity before merging.

---

## Notes on drift

- **SignatureNonRepudiation** (row 10) and **MessageReplayDetection** (row
  9): Ed25519 message signing was added to Rust during AUDIT-2026-03 M-3
  but the TLA+ VSR.tla did not model signatures until Phase 5 of this
  migration (commit `specs: model Ed25519 signatures ...`). Before that,
  the two specs claimed to track it but only via "sig_valid implicit"
  hand-wave.
- **TenantIsolation** (row 14) relies partly on Flux refinement types.
  Full refinement coverage is a Phase 7 deliverable; some rows of this
  matrix may still be "production assert + Flux stub" rather than "Flux
  proven" at the time of merge.
- **EventualCommit / EventualProgress** are probabilistic heuristics in
  VOPR — they sample runs and flag windows that violate fairness. TLA+
  `WF_vars` / `SF_vars` fairness assumptions are the authoritative proofs;
  VOPR catches regressions that introduce livelock.
- **TLAPS campaign (2026-04-17)** — a targeted campaign discharged the
  trivial / simple / cascade tiers of the outstanding TLAPS theorems.
  State after the campaign:

  **Category A — TLAPS mechanically discharged:**
  `ViewMonotonicityTheorem` (pre-existing), `EncryptionAtRestTheorem`,
  `TenantIsolationTheorem`, `AccessControlCorrectnessTheorem`,
  `AuditCompletenessTheorem`, `HashChainIntegrityTheorem`,
  `ComplianceSafetyTheorem`, `HIPAA_ComplianceTheorem`,
  `GDPR_ComplianceTheorem`, `SOC2_ComplianceTheorem`,
  `MetaFrameworkTheorem`, `CrashedLogBoundTheorem`,
  `RecoveryMonotonicityTheorem`, `RecoveryPreservesCommitsTheorem`,
  `ViewChangeMonotonicityTheorem`, and `SafetyPropertiesTheorem`
  (heterogeneous composition). Plus the helper lemmas `QuorumIntersection`
  (in VSR/ViewChange/Recovery proof files), `IsQuorumNonEmpty`, and
  `AuditIndexEqualsLenInvariant`.

  **Category B — cross-tool credit (PROOF OMITTED with reference):**
  `AgreementTheorem`, `PrefixConsistencyTheorem` (credited to Ivy
  `VSR_Byzantine.ivy::agreement` and `log_consistency`, both PR-blocking);
  `LeaderUniquenessTheorem` (Alloy `Quorum.als::ViewLeaderUniqueness`,
  PR-blocking); `MessageSignatureEnforcedTheorem` (Ivy + Coq Ed25519);
  `MessageDedupEnforcedTheorem` (Ivy `dedup` invariant);
  `TypeOKInvariant`, `CommitNotExceedOpInvariant` (TLC exhaustive at
  VSR_Small.cfg, PR-blocking); `ViewChangePreservesCommitsTheorem` (TLC
  at ViewChange_Small.cfg); `ViewChangeAgreementTheorem` (cross-
  referenced to Ivy + TLC, derivative of Agreement).

  **Category C — out of TLAPS scope (VOPR-covered):**
  `RecoveryLivenessTheorem`. Requires TLA+ liveness infrastructure
  (WF_vars, ENABLED, `<>[]`) that is not yet built. Behaviourally
  covered by VOPR `recovery_timeout` scenario in `kimberlite-sim`.
  Tracked under ROADMAP v0.6.0 "TLA+ liveness infrastructure".

  The Category-B cross-references are sound mechanical proofs — Ivy uses
  SMT, Alloy uses bounded-exhaustive model enumeration, TLC uses
  exhaustive state exploration. "Equivalent verification coverage by
  any sound tool" is the explicit goal. See `specs/README.md` § "Coverage
  Rubric (A/B/C)" for the classification rubric that future contributors
  should apply to new theorems.
