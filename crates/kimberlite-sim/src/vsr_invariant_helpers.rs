//! Helper functions for checking VSR invariants on replica snapshots.
//!
//! This module provides convenience functions that work with `VsrReplicaSnapshot`
//! to check VSR invariants. These helpers bridge the gap between the simulation's
//! snapshot-based state and the invariant checkers' operational interfaces.

use kimberlite_crypto::{ChainHash, internal_hash};

use crate::{
    AgreementChecker, CommitNumberConsistencyChecker, InvariantResult, PrefixPropertyChecker,
    VsrReplicaSnapshot,
};

// ============================================================================
// Snapshot-Based Invariant Checking
// ============================================================================

/// Checks commit number consistency for all replicas in the provided snapshots.
///
/// Verifies that `commit_number <= op_number` for each replica.
///
/// # Parameters
///
/// - `checker`: The commit number consistency checker
/// - `snapshots`: Array of replica snapshots to check
///
/// # Returns
///
/// `InvariantResult::Ok` if all replicas pass, or `InvariantResult::Violated` if any fail.
pub fn check_commit_number_consistency_snapshots(
    checker: &mut CommitNumberConsistencyChecker,
    snapshots: &[VsrReplicaSnapshot],
) -> InvariantResult {
    for snapshot in snapshots {
        let result = checker.check_consistency(
            snapshot.replica_id,
            snapshot.op_number,
            snapshot.commit_number,
        );

        if !result.is_ok() {
            return result;
        }
    }

    InvariantResult::Ok
}

/// Checks agreement property across all replicas in the provided snapshots.
///
/// Verifies that no two replicas have different operations at the same (view, op) position.
///
/// # Parameters
///
/// - `checker`: The agreement checker
/// - `snapshots`: Array of replica snapshots to check
///
/// # Returns
///
/// `InvariantResult::Ok` if agreement holds, or `InvariantResult::Violated` if violated.
pub fn check_agreement_snapshots(
    checker: &mut AgreementChecker,
    snapshots: &[VsrReplicaSnapshot],
) -> InvariantResult {
    // Record all committed operations from all replicas
    for snapshot in snapshots {
        // Only check committed operations (up to commit_number)
        let commit_op = snapshot.commit_number.as_u64();

        for entry in &snapshot.log {
            let op_num = entry.op_number.as_u64();

            // Only consider committed operations
            if op_num <= commit_op {
                // Compute hash of the entry for comparison
                let entry_hash = compute_log_entry_hash(entry);

                let result = checker.record_commit(
                    snapshot.replica_id,
                    entry.view,
                    entry.op_number,
                    &entry_hash,
                );

                if !result.is_ok() {
                    return result;
                }
            }
        }
    }

    InvariantResult::Ok
}

/// Checks prefix property across all replicas in the provided snapshots.
///
/// Verifies that all replicas agree on the committed prefix of the log.
///
/// # Parameters
///
/// - `checker`: The prefix property checker
/// - `snapshots`: Array of replica snapshots to check
///
/// # Returns
///
/// `InvariantResult::Ok` if prefix property holds, or `InvariantResult::Violated` if violated.
pub fn check_prefix_property_snapshots(
    checker: &mut PrefixPropertyChecker,
    snapshots: &[VsrReplicaSnapshot],
) -> InvariantResult {
    // Find the minimum commit number across all replicas
    let min_commit = snapshots
        .iter()
        .map(|s| s.commit_number.as_u64())
        .min()
        .unwrap_or(0);

    if min_commit == 0 {
        // No operations committed yet, prefix property trivially holds
        return InvariantResult::Ok;
    }

    // Record all committed operations up to min_commit for each replica
    for snapshot in snapshots {
        for entry in &snapshot.log {
            let op_num = entry.op_number.as_u64();

            // Only consider operations up to the minimum commit point
            if op_num <= min_commit {
                let entry_hash = compute_log_entry_hash(entry);
                checker.record_committed_op(snapshot.replica_id, entry.op_number, &entry_hash);
            }
        }
    }

    // Check that all replicas agree on the prefix up to min_commit
    checker.check_prefix_agreement(kimberlite_vsr::OpNumber::new(min_commit))
}

/// Checks all core VSR invariants on the provided snapshots.
///
/// This is a convenience function that runs all VSR invariant checks in sequence.
///
/// # Parameters
///
/// - `commit_checker`: Commit number consistency checker
/// - `agreement_checker`: Agreement checker
/// - `prefix_checker`: Prefix property checker
/// - `snapshots`: Array of replica snapshots to check
///
/// # Returns
///
/// `InvariantResult::Ok` if all invariants pass, or the first violation encountered.
pub fn check_all_vsr_invariants(
    commit_checker: &mut CommitNumberConsistencyChecker,
    agreement_checker: &mut AgreementChecker,
    prefix_checker: &mut PrefixPropertyChecker,
    snapshots: &[VsrReplicaSnapshot],
) -> InvariantResult {
    // Check commit number consistency
    let result = check_commit_number_consistency_snapshots(commit_checker, snapshots);
    if !result.is_ok() {
        return result;
    }

    // Check agreement
    let result = check_agreement_snapshots(agreement_checker, snapshots);
    if !result.is_ok() {
        return result;
    }

    // Check prefix property
    check_prefix_property_snapshots(prefix_checker, snapshots)
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Computes a hash of a log entry for comparison.
///
/// This uses BLAKE3 (internal_hash) to compute a content hash of the log entry,
/// which is used for agreement and prefix property checking.
fn compute_log_entry_hash(entry: &kimberlite_vsr::LogEntry) -> ChainHash {
    // Serialize the entry for hashing
    // For Phase 2, we use a simple serialization approach
    let mut data = Vec::new();
    data.extend_from_slice(&entry.op_number.as_u64().to_le_bytes());
    data.extend_from_slice(&entry.view.as_u64().to_le_bytes());
    data.extend_from_slice(&entry.checksum.to_le_bytes());

    // Serialize command (simplified - real implementation would use bincode)
    let command_str = format!("{:?}", entry.command);
    data.extend_from_slice(command_str.as_bytes());

    let hash = internal_hash(&data);
    ChainHash::from_bytes(hash.as_bytes())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_kernel::Command;
    use kimberlite_types::{DataClass, Placement, Region, StreamId, StreamName, TenantId};
    use kimberlite_vsr::{CommitNumber, LogEntry, OpNumber, ReplicaId, ReplicaStatus, ViewNumber};

    fn make_test_entry(op: u64) -> LogEntry {
        LogEntry {
            op_number: OpNumber::new(op),
            view: ViewNumber::ZERO,
            command: Command::CreateStream {
                stream_id: StreamId::from_tenant_and_local(TenantId::new(1), op as u32),
                stream_name: StreamName::from(format!("stream_{}", op)),
                data_class: DataClass::PHI,
                placement: Placement::Region(Region::USEast1),
            },
            idempotency_id: None,
            checksum: 0,
        }
    }

    fn make_snapshot(replica_id: u8, op: u64, commit: u64) -> VsrReplicaSnapshot {
        let mut log = Vec::new();
        for i in 1..=op {
            log.push(make_test_entry(i));
        }

        VsrReplicaSnapshot {
            replica_id: ReplicaId::new(replica_id),
            view: ViewNumber::ZERO,
            op_number: OpNumber::new(op),
            commit_number: CommitNumber::new(OpNumber::new(commit)),
            log,
            status: ReplicaStatus::Normal,
        }
    }

    #[test]
    fn test_commit_number_consistency_pass() {
        let mut checker = CommitNumberConsistencyChecker::new();

        let snapshots = vec![
            make_snapshot(0, 10, 10), // commit == op
            make_snapshot(1, 10, 5),  // commit < op
            make_snapshot(2, 0, 0),   // no ops yet
        ];

        let result = check_commit_number_consistency_snapshots(&mut checker, &snapshots);
        assert!(result.is_ok());
    }

    #[test]
    fn test_commit_number_consistency_fail() {
        let mut checker = CommitNumberConsistencyChecker::new();

        let mut snapshot = make_snapshot(0, 5, 10); // commit > op - VIOLATION
        snapshot.commit_number = CommitNumber::new(OpNumber::new(10));

        let result = check_commit_number_consistency_snapshots(&mut checker, &[snapshot]);
        assert!(!result.is_ok());
    }

    #[test]
    fn test_agreement_same_ops() {
        let mut checker = AgreementChecker::new();

        // All replicas have the same operations
        let snapshots = vec![
            make_snapshot(0, 3, 3),
            make_snapshot(1, 3, 3),
            make_snapshot(2, 3, 3),
        ];

        let result = check_agreement_snapshots(&mut checker, &snapshots);
        assert!(result.is_ok());
    }

    #[test]
    fn test_prefix_property_pass() {
        let mut checker = PrefixPropertyChecker::new();

        // All replicas agree on committed prefix
        let snapshots = vec![
            make_snapshot(0, 5, 5),
            make_snapshot(1, 5, 5),
            make_snapshot(2, 3, 3), // Behind but agrees on prefix
        ];

        let result = check_prefix_property_snapshots(&mut checker, &snapshots);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_all_vsr_invariants() {
        let mut commit_checker = CommitNumberConsistencyChecker::new();
        let mut agreement_checker = AgreementChecker::new();
        let mut prefix_checker = PrefixPropertyChecker::new();

        let snapshots = vec![
            make_snapshot(0, 3, 3),
            make_snapshot(1, 3, 2),
            make_snapshot(2, 2, 2),
        ];

        let result = check_all_vsr_invariants(
            &mut commit_checker,
            &mut agreement_checker,
            &mut prefix_checker,
            &snapshots,
        );

        assert!(result.is_ok());
    }
}
