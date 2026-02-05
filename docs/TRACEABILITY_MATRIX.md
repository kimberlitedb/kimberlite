# Traceability Matrix

**Coverage:** 19/19 theorems fully traced (100.0%)

| TLA+ Theorem | TLA+ File | Rust Implementation | VOPR Scenario | VOPR Invariant |
|--------------|-----------|---------------------|---------------|----------------|
| `AgreementTheorem` | `specs/tla/VSR.tla` | `crates/kimberlite-vsr/src/replica.rs::on_prepare_ok_quorum`| `protocol_attacks::byzantine_attacks` | `check_agreement` |
| `ViewMonotonicityTheorem` | `specs/tla/VSR.tla` | `crates/kimberlite-vsr/src/types.rs::ViewNumber::new`| `baseline` | `check_view_monotonic` |
| `PrefixConsistencyTheorem` | `specs/tla/VSR.tla` | `crates/kimberlite-kernel/src/kernel.rs::apply_committed`| `baseline` | `check_committed_prefix_consistency` |
| `ViewChangePreservesCommitsTheorem` | `specs/tla/ViewChange_Proofs.tla` | `crates/kimberlite-vsr/src/view_change.rs::on_start_view_change`| `view_change_recovery` | `check_view_change_safety` |
| `RecoveryPreservesCommitsTheorem` | `specs/tla/Recovery_Proofs.tla` | `crates/kimberlite-vsr/src/recovery.rs::recover_from_crash`| `crash_recovery` | `check_recovery_safety` |
| `TenantIsolationTheorem` | `specs/tla/Compliance_Proofs.tla` | `crates/kimberlite-kernel/src/kernel.rs::apply_committed`| `multi_tenant_isolation` | `check_tenant_isolation` |
| `AuditCompletenessTheorem` | `specs/tla/Compliance_Proofs.tla` | `crates/kimberlite-kernel/src/kernel.rs::apply_committed`| `baseline` | `check_audit_completeness` |
| `HashChainIntegrityTheorem` | `specs/tla/Compliance_Proofs.tla` | `crates/kimberlite-storage/src/storage.rs::append_record`| `storage_corruption` | `check_hash_chain_integrity` |
| `EncryptionAtRestTheorem` | `specs/tla/Compliance_Proofs.tla` | `crates/kimberlite-crypto/src/encryption.rs::encrypt_data`| `baseline` | `check_encryption_at_rest` |
| `OffsetMonotonicityProperty` | `specs/tla/Kernel.tla` | `crates/kimberlite-kernel/src/state.rs::with_updated_offset`| `baseline` | `check_offset_monotonic` |
| `StreamUniquenessProperty` | `specs/tla/Kernel.tla` | `crates/kimberlite-kernel/src/kernel.rs::apply_committed (CreateStream)`| `baseline` | `check_stream_uniqueness` |
| `SHA256DeterministicTheorem` | `specs/coq/SHA256.v` | `crates/kimberlite-crypto/src/hash.rs::hash_sha256`| `baseline` | `check_hash_determinism` |
| `ChainHashIntegrityTheorem` | `specs/coq/SHA256.v` | `crates/kimberlite-crypto/src/hash.rs::chain_hash`| `storage_corruption` | `check_chain_hash_integrity` |
| `ByzantineAgreementInvariant` | `specs/ivy/VSR_Byzantine.ivy` | `crates/kimberlite-vsr/src/replica.rs::on_prepare_ok_quorum`| `protocol_attacks::byzantine_attacks` | `check_agreement` |
| `QuorumIntersectionProperty` | `specs/ivy/VSR_Byzantine.ivy` | `crates/kimberlite-vsr/src/quorum.rs::is_quorum`| `protocol_attacks::byzantine_attacks` | `check_quorum_intersection` |
| `HIPAA_164_312_a_1_TechnicalAccessControl` | `specs/tla/compliance/HIPAA.tla` | `crates/kimberlite-kernel/src/kernel.rs::apply_committed`| `multi_tenant_isolation` | `check_tenant_isolation` |
| `HIPAA_164_312_a_2_iv_Encryption` | `specs/tla/compliance/HIPAA.tla` | `crates/kimberlite-crypto/src/encryption.rs::encrypt_data`| `baseline` | `check_encryption_at_rest` |
| `GDPR_Article_25_DataProtectionByDesign` | `specs/tla/compliance/GDPR.tla` | `crates/kimberlite-kernel/src/kernel.rs::apply_committed`| `multi_tenant_isolation` | `check_tenant_isolation` |

## Coverage Summary

- Total TLA+ Theorems: 19
- Theorems Implemented in Rust: 19/19
- Theorems Tested by VOPR: 19/19
- Fully Traced (TLA+ → Rust → VOPR): 19/19
