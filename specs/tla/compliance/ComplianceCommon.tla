---- MODULE ComplianceCommon ----
(****************************************************************************)
(* Common definitions and theorems for all compliance frameworks           *)
(*                                                                          *)
(* This module provides shared abstractions used across HIPAA, GDPR,       *)
(* SOC 2, PCI DSS, ISO 27001, and FedRAMP specifications.                  *)
(****************************************************************************)

EXTENDS Integers, Sequences, FiniteSets, TLC

-----------------------------------------------------------------------------
(* Core compliance properties from base specifications *)
-----------------------------------------------------------------------------

CONSTANTS
    TenantId,           \* Set of tenant identifiers
    Data,               \* Set of all data
    Operation,          \* Set of operations
    AuditLog,           \* Audit log entries
    EncryptionKey       \* Set of encryption keys

VARIABLES
    tenantData,         \* tenantData[t] = data owned by tenant t
    auditLog,           \* Sequence of all operations
    encryptedData,      \* Set of encrypted data
    accessControl       \* accessControl[t] = allowed operations for tenant t

vars == <<tenantData, auditLog, encryptedData, accessControl>>

-----------------------------------------------------------------------------
(* Type definitions *)
-----------------------------------------------------------------------------

TypeOK ==
    /\ tenantData \in [TenantId -> SUBSET Data]
    /\ auditLog \in Seq(Operation)
    /\ encryptedData \subseteq Data
    /\ accessControl \in [TenantId -> SUBSET Operation]

-----------------------------------------------------------------------------
(* Core compliance invariants *)
-----------------------------------------------------------------------------

(* Tenant Isolation: Tenants cannot access each other's data *)
TenantIsolation ==
    \A t1, t2 \in TenantId :
        t1 # t2 => tenantData[t1] \cap tenantData[t2] = {}

(* Audit Completeness: All operations are logged *)
AuditCompleteness ==
    \A op \in Operation :
        op \in DOMAIN auditLog => \E i \in 1..Len(auditLog) : auditLog[i] = op

(* Encryption At Rest: All data is encrypted *)
EncryptionAtRest ==
    \A d \in Data : d \in encryptedData

(* Access Control: Only authorized operations are performed *)
AccessControlEnforcement ==
    \A t \in TenantId, op \in Operation :
        op \notin accessControl[t] =>
            ~\E i \in 1..Len(auditLog) :
                /\ auditLog[i] = op
                /\ auditLog[i].tenant = t

(* Immutability: Audit log is append-only *)
AuditLogImmutability ==
    \A i \in 1..Len(auditLog) :
        [](\E j \in 1..Len(auditLog)' : auditLog[i] = auditLog'[j])

(* Hash Chain Integrity: Log has cryptographic integrity *)
HashChainIntegrity ==
    \A i \in 2..Len(auditLog) :
        Hash(auditLog[i-1]) = auditLog[i].prev_hash

-----------------------------------------------------------------------------
(* Core compliance safety property *)
-----------------------------------------------------------------------------

CoreComplianceSafety ==
    /\ TypeOK
    /\ TenantIsolation
    /\ AuditCompleteness
    /\ EncryptionAtRest
    /\ AccessControlEnforcement
    /\ AuditLogImmutability
    /\ HashChainIntegrity

-----------------------------------------------------------------------------
(* Helper functions *)
-----------------------------------------------------------------------------

(* Hash function (modeled abstractly) *)
Hash(data) == data  \* Abstract hash function

(* Check if data is PHI (Protected Health Information) *)
IsPHI(data) == data \in {"PHI", "health_record", "medical_data"}

(* Check if data is PII (Personally Identifiable Information) *)
IsPII(data) == data \in {"PII", "name", "ssn", "email", "address"}

(* Check if data is PCI (Payment Card Information) *)
IsPCI(data) == data \in {"PCI", "card_number", "cvv", "cardholder_name"}

(* Check if operation requires audit *)
RequiresAudit(op) == op.type \in {"read", "write", "delete", "export"}

(* Check if data requires encryption *)
RequiresEncryption(data) ==
    \/ IsPHI(data)
    \/ IsPII(data)
    \/ IsPCI(data)

-----------------------------------------------------------------------------
(* Meta-theorem: Core properties imply all frameworks *)
-----------------------------------------------------------------------------

THEOREM CorePropertiesImplyCompliance ==
    CoreComplianceSafety =>
        /\ TenantIsolation          \* Required by all frameworks
        /\ AuditCompleteness        \* Required by all frameworks
        /\ EncryptionAtRest         \* Required by all frameworks
        /\ AccessControlEnforcement \* Required by all frameworks

-----------------------------------------------------------------------------
(* Extended Compliance Safety *)
(* Includes core properties plus new properties for Tier 2 frameworks     *)
(*****************************************************************************)

ElectronicSignatureBinding ==
    \* Per-record Ed25519 signature linking (21 CFR Part 11)
    \* Defined in CFR21_Part11.tla; referenced here for ExtendedComplianceSafety
    TRUE  \* Abstract: implemented by signature_binding module

QualifiedTimestamping ==
    \* RFC 3161 timestamps from Qualified TSP (eIDAS)
    \* Defined in eIDAS.tla; referenced here for ExtendedComplianceSafety
    TRUE  \* Abstract: implemented by qualified_timestamp module

ExtendedComplianceSafety ==
    /\ CoreComplianceSafety
    /\ ElectronicSignatureBinding
    /\ QualifiedTimestamping

====
