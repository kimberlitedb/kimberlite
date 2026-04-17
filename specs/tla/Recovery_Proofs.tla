------------------------ MODULE Recovery_Proofs ------------------------
(*
 * TLAPS Proof Stubs for Protocol-Aware Recovery (PAR)
 *
 * This module states the safety theorems about Kimberlite's recovery
 * protocol and tracks their verification status. The protocol itself
 * is fully specified in `Recovery.tla`; the invariants here are
 * checked by TLC (bounded model checking, via `Recovery.cfg`) and on
 * the EPYC runner through `just fv-epyc-tla-full`.
 *
 * Current TLAPS discharge status: the five theorems land as
 * `PROOF OMITTED` with specific unproven obligations named in the
 * preceding comments. A prior iteration of this file carried
 * "PROOF SKETCH ... <...>" blocks and bare "OMITTED" markers that were
 * not valid TLAPS syntax; those were replaced with honest
 * `PROOF OMITTED` per the project's epistemic-honesty policy for
 * formal verification
 * (`docs/internals/formal-verification/traceability-matrix.md`).
 *
 * Theorems stated:
 *   - CrashedLogBoundTheorem
 *   - RecoveryMonotonicityTheorem
 *   - RecoveryPreservesCommitsTheorem
 *   - RecoveryLivenessTheorem
 *
 * Action names referenced throughout are the ones defined in
 * Recovery.tla: Crash, StartRecovery, OnRecovery, OnRecoveryResponse.
 *
 * Based on: Protocol-Aware Recovery from VR Revisited
 * (Liskov & Cowling, 2012).
 *)

EXTENDS Recovery, TLAPS

--------------------------------------------------------------------------------
(* Main Theorems — stated; discharge status is PROOF OMITTED *)

\* THEOREM 1: Crashed Log Bound (structural invariant).
\* Outstanding obligation: the `Crash(r)` action assigns
\*   log'[r] = SubSeq(log[r], 1, commitNumber[r])
\* which gives `Len(log'[r]) = commitNumber[r] = commitNumber'[r]`. The
\* inductive proof for this case requires the `SeqTheorems`
\* sub-module for the `Len(SubSeq(...))` identity. The other three
\* actions (StartRecovery, OnRecovery, OnRecoveryResponse) don't
\* produce new "Crashed" replicas, so the invariant is preserved
\* vacuously. Discharging this theorem is the most tractable of the
\* four below and is the top priority for a future iteration.
THEOREM CrashedLogBoundTheorem ==
    Spec => []CrashedLogBound
PROOF OMITTED

\* THEOREM 2: Recovery Monotonicity (commit number never decreases
\* during recovery).
\* Outstanding obligation: the `OnRecoveryResponse` action updates
\*   commitNumber'[r] = Max(commitNumber[r], msg.commitNum)
\* when quorum is reached, which preserves commitNumber'[r] >=
\* commitNumber[r]. Each action-case in the inductive step requires
\* separate `BY DEF <ActionName>, ...` unfolds.
THEOREM RecoveryMonotonicityTheorem ==
    Spec => [][\A r \in Replicas :
                  (status[r] = "Recovering") =>
                      commitNumber'[r] >= commitNumber[r]]_vars
PROOF OMITTED

\* THEOREM 3: Recovery Preserves Commits.
\* Outstanding obligation: requires a helper lemma showing that any
\* quorum of `RecoveryResponse` messages contains at least one
\* response from a replica whose commitNumber >= the recovering
\* replica's pre-crash commitNumber (quorum intersection). This is
\* provable structurally but requires importing QuorumIntersection
\* from VSR_Proofs, which is blocked by the same module-structure
\* issue described in ViewChange_Proofs.tla.
THEOREM RecoveryPreservesCommitsTheorem ==
    Spec => [](\A r \in Replicas, op \in OpNumber :
                  (status[r] = "Crashed" /\ op <= commitNumber[r]) =>
                      op <= Len(log[r]))
PROOF OMITTED

\* THEOREM 4: Recovery Eventually Completes (liveness).
\* Outstanding obligation: requires weak-fairness temporal reasoning
\* (WF_vars(Next)). Liveness proofs need ENABLED and `<>[]`
\* operators over the fairness condition; we do not currently have a
\* TLAPS proof skeleton in the codebase for liveness properties.
\* VOPR simulation covers this via timeout scenarios in
\* `kimberlite-sim` but we do not claim a mechanized proof.
THEOREM RecoveryLivenessTheorem ==
    Spec => [](\A r \in Replicas :
                  (status[r] = "Recovering") =>
                      <>(status[r] = "Normal"))
PROOF OMITTED

================================================================================
