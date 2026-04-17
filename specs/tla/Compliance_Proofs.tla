----------------------- MODULE Compliance_Proofs -----------------------
(*
 * TLAPS Proof Stubs for Compliance Meta-Framework
 *
 * This module states the safety theorems that map Kimberlite's
 * abstract compliance properties (tenant isolation, audit
 * completeness, hash chain integrity, encryption at rest, access
 * control) onto specific regulatory frameworks (HIPAA, GDPR, SOC 2,
 * etc.). The base Compliance.tla spec is checked by TLC (bounded
 * model checking, via `Compliance.cfg` at depth 8) in PR CI and at
 * full capacity on EPYC via `just fv-epyc-tla-full`.
 *
 * Current TLAPS discharge status: all theorems land as
 * `PROOF OMITTED` with specific unproven obligations named in the
 * preceding comments. A prior iteration of this file carried proof
 * structures where lemma step bodies were written as English prose
 * rather than TLA+ formulas (e.g., `<1>1. GrantAccess preserves
 * userTenant and dataOwner`), which tlapm rejected with
 * "Unexpected end of (sub)proof". Those were replaced with honest
 * `PROOF OMITTED` markers per the project's epistemic-honesty policy
 * for formal verification
 * (`docs/internals/formal-verification/traceability-matrix.md`).
 *
 * Compliance Mappings:
 *   - HIPAA: §164.308(a)(4), §164.312(a)(1), §164.312(b),
 *            §164.312(a)(2)(iv), §164.312(c)(1)
 *   - GDPR: Article 17, Article 32
 *   - SOC 2: CC6.1, CC7.2
 *   - PCI DSS, ISO 27001, FedRAMP (via meta-framework)
 *
 * Theorems stated:
 *   - TenantIsolationTheorem
 *   - AuditCompletenessTheorem
 *   - HashChainIntegrityTheorem
 *   - EncryptionAtRestTheorem
 *   - AccessControlCorrectnessTheorem
 *   - ComplianceSafetyTheorem  (composition)
 *   - HIPAA_ComplianceTheorem
 *   - GDPR_ComplianceTheorem
 *   - SOC2_ComplianceTheorem
 *   - MetaFrameworkTheorem     (composition over the three above)
 *
 * Action names referenced are the ones defined in Compliance.tla:
 * AccessData, GrantAccess (RequestErasure / ExecuteErasure are
 * excluded from Next for model-checking reasons).
 *)

EXTENDS Compliance, TLAPS

--------------------------------------------------------------------------------
(* Core Safety Theorems *)

\* THEOREM 1: Tenant Isolation (HIPAA §164.308, GDPR Art. 32, SOC 2 CC6.1)
\* Outstanding obligation: the GrantAccess case must show that
\* granting a permission cannot cross tenant boundaries, i.e.
\*   userTenant[user] = dataOwner[d]
\* is a GrantAccess precondition (encoded at line 141-142 of
\* Compliance.tla). The inductive step needs this precondition fact
\* propagated through `accessPermissions'[user][d]`, which requires
\* an explicit case-split on whether `(u, d)` is the pair being
\* granted. TLC covers the invariant at depth 8 in PR CI.
THEOREM TenantIsolationTheorem ==
    Spec => []TenantIsolation
PROOF OMITTED

\* THEOREM 2: Audit Completeness (HIPAA §164.312(b), SOC 2 CC7.2)
\* Outstanding obligation: every `Append(auditLog, entry)` in
\* AccessData and GrantAccess sets `entry.immutable = TRUE`. The
\* inductive step needs to show that for every index i in
\* 1..Len(auditLog'): auditLog'[i].immutable = TRUE. Split into
\* i <= Len(auditLog) (by IH) vs. i = Len(auditLog) + 1 (by the
\* appended entry's `immutable |-> TRUE` field).
THEOREM AuditCompletenessTheorem ==
    Spec => []AuditCompleteness
PROOF OMITTED

\* THEOREM 3: Hash Chain Integrity
\* (HIPAA §164.312(c)(1), tamper-evident audit requirement)
\* Outstanding obligation: the hashChain EXCEPT update in AccessData
\* (line 133-134) and GrantAccess (line 156-157) of Compliance.tla
\* assigns hashChain'[auditIndex'] = HashOf(hashChain[auditIndex],
\* entry), where entry = auditLog'[auditIndex']. The inductive step
\* requires showing that auditIndex' = auditIndex + 1 in both cases
\* (true by the `auditIndex' = auditIndex + 1` assignment) and that
\* previous chain entries are preserved (which they are because the
\* EXCEPT only touches index auditIndex').
THEOREM HashChainIntegrityTheorem ==
    Spec => []HashChainIntegrity
PROOF OMITTED

\* THEOREM 4: Encryption At Rest
\* (HIPAA §164.312(a)(2)(iv), GDPR Article 32)
\* Outstanding obligation: none of AccessData / GrantAccess modify
\* the `encrypted` variable (both have it in their UNCHANGED list,
\* lines 135-136 and 158-159 of Compliance.tla). The inductive step
\* is one line per action: `BY DEF AccessData` and `BY DEF
\* GrantAccess`. This is the simplest of the five core theorems and
\* is the top priority for a future iteration.
THEOREM EncryptionAtRestTheorem ==
    Spec => []EncryptionAtRest
PROOF OMITTED

\* THEOREM 5: Access Control Correctness
\* (HIPAA §164.308(a)(4), SOC 2 CC6.1)
\* Outstanding obligation: AccessControlCorrect follows directly
\* from TenantIsolation via CanAccess's tenant-check clause
\* (line 105 of Compliance.tla: `userTenant[u] = dataOwner[d]`).
\* Discharge: `BY TenantIsolationTheorem DEF TenantIsolation,
\* AccessControlCorrect, CanAccess`. Blocked on TenantIsolation.
THEOREM AccessControlCorrectnessTheorem ==
    Spec => []AccessControlCorrect
PROOF OMITTED

--------------------------------------------------------------------------------
(* Combined Safety Theorem *)

\* Composition of the five core theorems above.
\* Outstanding obligation: PTL combination once the five are green.
THEOREM ComplianceSafetyTheorem ==
    Spec => [](TenantIsolation /\
               AuditCompleteness /\
               HashChainIntegrity /\
               EncryptionAtRest /\
               AccessControlCorrect)
PROOF OMITTED

--------------------------------------------------------------------------------
(* Framework-Specific Mappings *)

\* HIPAA Compliance — logical mapping onto core properties.
\* Outstanding obligation: once ComplianceSafetyTheorem is green,
\* discharge with `BY ComplianceSafetyTheorem DEF AccessControlCorrect,
\* TenantIsolation, AuditCompleteness, EncryptionAtRest,
\* HashChainIntegrity`.
THEOREM HIPAA_ComplianceTheorem ==
    Spec => [](AccessControlCorrect /\
               TenantIsolation /\
               AuditCompleteness /\
               EncryptionAtRest /\
               HashChainIntegrity)
PROOF OMITTED

\* GDPR Compliance — logical mapping onto core properties.
\* Outstanding obligation: once ComplianceSafetyTheorem is green,
\* discharge with `BY ComplianceSafetyTheorem`.
THEOREM GDPR_ComplianceTheorem ==
    Spec => [](EncryptionAtRest /\
               HashChainIntegrity /\
               AuditCompleteness)
PROOF OMITTED

\* SOC 2 Compliance — logical mapping onto core properties.
\* Outstanding obligation: once ComplianceSafetyTheorem is green,
\* discharge with `BY ComplianceSafetyTheorem`.
THEOREM SOC2_ComplianceTheorem ==
    Spec => [](AccessControlCorrect /\
               TenantIsolation /\
               AuditCompleteness)
PROOF OMITTED

\* Meta-Framework Theorem — all three framework-specific theorems
\* together. Outstanding obligation: PTL composition.
THEOREM MetaFrameworkTheorem ==
    Spec => [](AccessControlCorrect /\
               TenantIsolation /\
               AuditCompleteness /\
               EncryptionAtRest /\
               HashChainIntegrity)
PROOF OMITTED

================================================================================
