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

| # | Property | Spec file + theorem | Rust site | Layer(s) | Where it runs |
|---|---|---|---|---|---|
| 1 | **Agreement** — replicas never commit conflicting operations at the same (view, op_number) | `specs/tla/VSR.tla::Agreement`; `specs/tla/VSR_Proofs.tla::AgreementTheorem`; `specs/ivy/VSR_Byzantine.ivy::inv agreement` | `crates/kimberlite-sim/src/vsr_invariants.rs::AgreementChecker` | TLC, TLAPS, Ivy, VOPR | PR (TLC, Alloy) + aspirational (TLAPS, Ivy) + EPYC (all) |
| 2 | **PrefixConsistency** — committed log prefixes match across replicas | `specs/tla/VSR.tla::PrefixConsistency`; `specs/tla/VSR_Proofs.tla::PrefixConsistencyTheorem` | `crates/kimberlite-sim/src/vsr_invariants.rs::PrefixChecker` | TLC, TLAPS, VOPR | PR + aspirational + EPYC |
| 3 | **ViewMonotonicity** — view numbers never decrease | `specs/tla/VSR.tla::ViewMonotonicity`; `specs/tla/VSR_Proofs.tla::ViewMonotonicityTheorem` | `crates/kimberlite-vsr/src/replica/normal.rs` (production `assert!` in Phase 4); `crates/kimberlite-vsr/src/kani_proofs.rs::verify_view_number_monotonic` | TLC, TLAPS, Kani, production assert | PR + aspirational + EPYC |
| 4 | **OpNumberMonotonicity** — op_number is strictly increasing per view | `specs/tla/VSR.tla::OpNumberMonotonicity` | `crates/kimberlite-vsr/src/replica/normal.rs` (production `assert!` in Phase 4) | TLC, Kani, production assert | PR + EPYC |
| 5 | **CommitBound** — commit_number ≤ op_number | `specs/tla/VSR.cfg::CommitNotExceedOp` (INVARIANT) | `crates/kimberlite-vsr/src/replica/normal.rs` (production `assert!` in Phase 4) | TLC, production assert | PR + EPYC |
| 6 | **LeaderUniqueness** — exactly one leader per view | `specs/tla/VSR.tla::LeaderUniquePerView`; `specs/tla/VSR_Proofs.tla::LeaderUniquenessTheorem` | `crates/kimberlite-vsr/src/replica/state.rs::is_leader` (deterministic function of view + replica count) | TLC, TLAPS, Ivy, protocol structure | PR + aspirational + EPYC |
| 7 | **ViewChangePreservesCommits** — no committed op is lost during view change | `specs/tla/ViewChange.tla`; `specs/tla/ViewChange_Proofs.tla::ViewChangePreservesCommitsTheorem` | `crates/kimberlite-vsr/src/replica/view_change.rs::on_start_view` (production `assert!` on `log_tail_hash` equality, Phase 4); `crates/kimberlite-sim/src/vsr_invariants.rs::PrefixChecker` | TLC, TLAPS, Ivy, VOPR, production assert | PR + aspirational + EPYC |
| 8 | **RecoveryPreservesCommits** — recovery never discards quorum-committed ops | `specs/tla/Recovery.tla`; `specs/tla/Recovery_Proofs.tla::RecoveryPreservesCommitsTheorem` | `crates/kimberlite-vsr/src/replica/recovery.rs::apply_recovery_response` (production `assert!` in Phase 4); `crates/kimberlite-vsr/src/kani_proofs.rs::verify_recovery_preserves_committed_prefix` | TLC, TLAPS, Kani, production assert | PR + aspirational + EPYC |
| 9 | **MessageReplayDetection** — dedup tracker rejects re-use of a processed msg id | `specs/tla/VSR.tla::MessageDedupEnforced` (Phase 5) | `crates/kimberlite-vsr/src/replica/state.rs::apply_message` (production `assert!` on `!has_seen`, Phase 4); `crates/kimberlite-vsr/src/kani_proofs.rs::verify_message_dedup_detects_replay` | TLC (Phase 5), Kani, production assert | PR + EPYC |
| 10 | **SignatureNonRepudiation** — only holders of sk can produce verifying signatures | `specs/tla/VSR.tla::MessageSignatureEnforced` (Phase 5); `specs/coq/Ed25519.v::ed25519_euf_cma` | `crates/kimberlite-crypto/src/verified/ed25519.rs`; `crates/kimberlite-vsr/src/kani_proofs.rs::verify_signature_non_repudiation` | TLC (Phase 5), Coq, Kani | PR + EPYC |
| 11 | **HashChainIntegrity** — hash chain has no cycles or breaks | `specs/alloy/HashChain.als`; `specs/coq/SHA256.v::chain_hash_genesis_integrity` | `crates/kimberlite-crypto/src/verified/sha256.rs::chain_hash`; `crates/kimberlite-storage/src/kani_proofs.rs` | Alloy, Coq, Kani, production assert (non-zero) | PR + EPYC |
| 12 | **OffsetMonotonicity** — offsets within a stream never decrease | append-only log axiom (Alloy implicit) | `crates/kimberlite-kernel/src/kernel.rs` (production `assert!`); `crates/kimberlite-kernel/src/kani_proofs.rs` | Kani, production assert, VOPR | PR + EPYC |
| 13 | **QuorumIntersection** — any two quorums of size f+1 share a replica | `specs/alloy/Quorum.als::QuorumOverlap`; `specs/tla/VSR.tla::Quorum` helper | `crates/kimberlite-vsr/src/kani_proofs.rs::verify_quorum_intersection` | Alloy, Kani | PR + EPYC |
| 14 | **TenantIsolation** — cross-tenant reads/writes are rejected | `specs/tla/Compliance.tla`; `specs/tla/Compliance_Proofs.tla::TenantIsolationTheorem`; `specs/coq/KeyHierarchy.v::tenant_isolation` | `crates/kimberlite-directory/src/`; `crates/kimberlite-server/src/lib.rs` (tenant_id check); `#[flux(...)]` refinements in `kimberlite-types` | TLC, TLAPS, Coq, Flux | PR + aspirational + EPYC |
| 15 | **AuditCompleteness** — all state-changing ops appear in audit log | `specs/tla/Compliance.tla`; `specs/tla/Compliance_Proofs.tla::AuditCompletenessTheorem` | `crates/kimberlite-compliance/src/audit.rs` (`reached!` markers); `kimberlite-sim` property coverage | TLC, TLAPS, VOPR properties | PR + aspirational + EPYC |
| 16 | **KeyDerivationInjective** — different (master, tenant) inputs produce different KEKs | `specs/coq/KeyHierarchy.v::tenant_isolation`; `Ed25519.v::key_derivation_unique` | `crates/kimberlite-crypto/src/verified/key_hierarchy.rs`; `crates/kimberlite-crypto/src/verified/ed25519.rs` | Coq, Kani (to add) | PR + EPYC |
| 17 | **EncryptionIntegrity** — any modification to ciphertext or tag fails decryption | `specs/coq/AES_GCM.v::aes_gcm_integrity` | `crates/kimberlite-crypto/src/verified/aes_gcm.rs` | Coq, production assert | PR + EPYC |

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
| TLAPS | — | ✅ stretch 3000–5000 | ✅ stretch 10000 |
| Alloy | ✅ scope 5 (HashChain-quick) | — | ✅ scope 10 (HashChain) + scope 8 (Quorum) |
| Ivy | — | ✅ (continue-on-error) | ✅ |
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
