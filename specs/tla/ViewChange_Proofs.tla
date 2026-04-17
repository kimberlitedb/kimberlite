------------------------ MODULE ViewChange_Proofs ------------------------
(*
 * TLAPS Proof Stubs for View Change Protocol
 *
 * This module states the safety theorems about Kimberlite's view-change
 * protocol and tracks their verification status. The protocol itself is
 * fully specified in `ViewChange.tla`; the invariants here are checked
 * by TLC (bounded model checking, via `ViewChange.cfg` and
 * `ViewChange_Small.cfg`) and on the EPYC runner through
 * `just fv-epyc-tla-full`.
 *
 * Current TLAPS discharge status: all three main theorems land as
 * `PROOF OMITTED` with a specific unproven obligation named in the
 * preceding comment. A prior iteration of this file carried
 * "PROOF SKETCH ... <...>" blocks that were not valid TLAPS syntax;
 * those were replaced with honest `PROOF OMITTED` markers per the
 * project's epistemic-honesty policy for formal verification
 * (`docs/internals/formal-verification/traceability-matrix.md`).
 *
 * Theorems stated (all via PROOF OMITTED at this time):
 *   - ViewChangePreservesCommitsTheorem
 *   - ViewChangeAgreementTheorem
 *   - ViewChangeMonotonicityTheorem
 *
 * Action names referenced throughout are the ones defined in
 * ViewChange.tla: StartViewChange, OnStartViewChange, OnDoViewChange,
 * OnStartView (NOT the LeaderPrepare/... family which lives in VSR.tla).
 *)

EXTENDS ViewChange, TLAPS

--------------------------------------------------------------------------------
(* Main Theorems — stated; discharge status is PROOF OMITTED *)

\* THEOREM 1: View Change Preserves Commits.
\* Outstanding obligation: the OnDoViewChange case (view-change
\* completion) must show that among a quorum of DoViewChange messages,
\* at least one replica has the committed op in its log, and that the
\* leader's CHOOSE selects a log whose prefix includes the committed
\* op. This reduces to quorum intersection combined with temporal
\* Eventually reasoning, which tlapm's SMT backend has not discharged
\* in prior attempts. TLC in PR CI verifies this invariant at depth 10
\* via ViewChange_Small.cfg, and on EPYC at depth 20 via ViewChange.cfg.
THEOREM ViewChangePreservesCommitsTheorem ==
    Spec => [](\A r \in Replicas, op \in OpNumber :
                  (status[r] = "ViewChange" /\ op <= commitNumber[r]) =>
                      (op <= Len(log[r])))
PROOF OMITTED

\* THEOREM 2: View Change Preserves Agreement.
\* Outstanding obligation: cross-view agreement reduces to the VSR-
\* level Agreement property (proved as AgreementTheorem in
\* VSR_Proofs.tla), but importing that theorem across modules requires
\* `EXTENDS VSR_Proofs` or a `USE` statement, which introduces a
\* circular include because VSR_Proofs re-defines its own spec inline
\* rather than EXTENDING VSR. Resolving this requires refactoring
\* VSR_Proofs to EXTEND VSR (and share the CONSTANTS/VARIABLES).
THEOREM ViewChangeAgreementTheorem ==
    Spec => [](\A r1, r2 \in Replicas, op \in OpNumber :
                  (op <= commitNumber[r1] /\ op <= commitNumber[r2] /\
                   op > 0 /\ op <= Len(log[r1]) /\ op <= Len(log[r2])) =>
                      log[r1][op] = log[r2][op])
PROOF OMITTED

\* THEOREM 3: View Change Monotonicity (views never decrease).
\* Outstanding obligation: each of the four actions
\* (StartViewChange, OnStartViewChange, OnDoViewChange, OnStartView)
\* preserves view'[r] >= view[r]:
\*   - StartViewChange: view' = view + 1 > view (trivial)
\*   - OnStartViewChange: view' = msg.view when transitioning, and
\*     precondition msg.view > view[r] (trivial)
\*   - OnDoViewChange: UNCHANGED view (trivial)
\*   - OnStartView: view' = msg.view when msg.view >= view[r]
\*     (trivial)
\* Each case is one-line by definition, but the inductive proof needs
\* a case-split on Next with explicit DEF unfolds that we have not yet
\* written out in a form tlapm accepts.
THEOREM ViewChangeMonotonicityTheorem ==
    Spec => [][\A r \in Replicas : view'[r] >= view[r]]_vars
PROOF OMITTED

================================================================================
