/// # VSR Invariants
///!
///! This module implements consensus correctness invariants for Viewstamped Replication (VSR).
///! These invariants verify the safety properties of the VSR protocol.
///!
///! ## Safety Properties
///!
///! 1. **Agreement**: No two replicas commit different operations at the same (view, op) position
///! 2. **Prefix Property**: If a replica has committed operation o, all replicas agree on operations [0..o]
///! 3. **View-Change Safety**: New primary after view change has all committed operations from previous view
///! 4. **Recovery Safety**: Recovery never discards committed offsets
///!
///! ## References
///!
///! - "Viewstamped Replication Revisited" (Liskov & Cowling, 2012)
///! - TigerBeetle VOPR implementation
///! - FoundationDB simulation testing

use std::collections::{HashMap, BTreeMap};
use kimberlite_vsr::{ViewNumber, OpNumber, ReplicaId};
use kimberlite_crypto::ChainHash;

use crate::invariant::InvariantResult;
use crate::instrumentation::invariant_tracker;

// ============================================================================
// Agreement Invariant Checker
// ============================================================================

/// Verifies that no two replicas commit different operations at the same position.
///
/// **Invariant**: For any (view, op) tuple, at most one operation is committed.
///
/// **Violation**: Two replicas commit different operations at the same (view, op).
///
/// **Why it matters**: Agreement is fundamental to consensus - if replicas disagree
/// on what was committed, the system loses consistency.
///
/// **Expected to catch**:
/// - `canary-commit-quorum`: Committing with f instead of f+1 replicas
/// - Network partition bugs where replicas diverge
/// - View-change bugs that allow multiple primaries
#[derive(Debug)]
pub struct AgreementChecker {
    /// Map from (view, op) -> (replica_id, operation_hash)
    /// Tracks what each replica committed at each position
    committed: HashMap<(u64, u64), HashMap<u8, ChainHash>>,

    /// Total checks performed
    checks_performed: u64,
}

impl AgreementChecker {
    /// Creates a new agreement checker.
    pub fn new() -> Self {
        Self {
            committed: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records that a replica committed an operation at a given position.
    ///
    /// Returns an invariant violation if a different operation was already
    /// committed at this position by another replica.
    pub fn record_commit(
        &mut self,
        replica_id: ReplicaId,
        view: ViewNumber,
        op: OpNumber,
        operation_hash: &ChainHash,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("vsr_agreement");
        self.checks_performed += 1;

        let view_u64 = view.as_u64();
        let op_u64 = op.as_u64();
        let replica_u8 = replica_id.as_u8();

        let key = (view_u64, op_u64);

        // Get or create the entry for this (view, op)
        let replicas_at_position = self.committed.entry(key).or_insert_with(HashMap::new);

        // Check if another replica already committed a different operation here
        for (existing_replica, existing_hash) in replicas_at_position.iter() {
            if existing_replica != &replica_u8 && existing_hash != operation_hash {
                return InvariantResult::Violated {
                    invariant: "vsr_agreement".to_string(),
                    message: format!(
                        "Replicas {} and {} committed different operations at (view={}, op={})",
                        ReplicaId::new(*existing_replica),
                        replica_id,
                        view_u64,
                        op_u64
                    ),
                    context: vec![
                        ("view".to_string(), view_u64.to_string()),
                        ("op".to_string(), op_u64.to_string()),
                        ("replica_1".to_string(), existing_replica.to_string()),
                        ("replica_2".to_string(), replica_u8.to_string()),
                        ("hash_1".to_string(), format!("{:?}", existing_hash)),
                        ("hash_2".to_string(), format!("{:?}", operation_hash)),
                    ],
                };
            }
        }

        // Record this commit
        replicas_at_position.insert(replica_u8, *operation_hash);

        InvariantResult::Ok
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }

    /// Resets the checker state (for testing).
    pub fn reset(&mut self) {
        self.committed.clear();
        self.checks_performed = 0;
    }
}

impl Default for AgreementChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Prefix Property Invariant Checker
// ============================================================================

/// Verifies that all replicas agree on the prefix of committed operations.
///
/// **Invariant**: If replica R has committed operation o, then for all operations
/// i < o, all replicas that have committed i committed the same operation.
///
/// **Violation**: Replica R1 has committed ops [A, B, C] and R2 has committed [A, X, C]
/// where X ≠ B.
///
/// **Why it matters**: The log prefix must be consistent across replicas. Divergence
/// in the prefix means replicas have incompatible histories.
///
/// **Expected to catch**:
/// - Log truncation bugs during recovery
/// - View-change bugs that allow divergent histories
/// - Unsafe repair operations
#[derive(Debug)]
pub struct PrefixPropertyChecker {
    /// Map from replica_id -> BTreeMap<op_number, operation_hash>
    /// BTreeMap maintains sorted order for efficient prefix checks
    replica_logs: HashMap<u8, BTreeMap<u64, ChainHash>>,

    /// Total checks performed
    checks_performed: u64,
}

impl PrefixPropertyChecker {
    /// Creates a new prefix property checker.
    pub fn new() -> Self {
        Self {
            replica_logs: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records a committed operation for a replica.
    pub fn record_committed_op(
        &mut self,
        replica_id: ReplicaId,
        op: OpNumber,
        operation_hash: &ChainHash,
    ) {
        let replica_u8 = replica_id.as_u8();
        let op_u64 = op.as_u64();

        let log = self.replica_logs.entry(replica_u8).or_insert_with(BTreeMap::new);
        log.insert(op_u64, *operation_hash);
    }

    /// Checks that all replicas agree on the prefix up to the given op number.
    ///
    /// Returns an invariant violation if any two replicas disagree on the
    /// committed operation at any position <= op_number.
    pub fn check_prefix_agreement(&mut self, up_to_op: OpNumber) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("vsr_prefix_property");
        self.checks_performed += 1;

        let up_to = up_to_op.as_u64();

        // Compare all pairs of replicas
        let replica_ids: Vec<u8> = self.replica_logs.keys().copied().collect();

        for i in 0..replica_ids.len() {
            for j in (i + 1)..replica_ids.len() {
                let replica1 = replica_ids[i];
                let replica2 = replica_ids[j];

                let log1 = &self.replica_logs[&replica1];
                let log2 = &self.replica_logs[&replica2];

                // Check each position from 0 to up_to
                for op_num in 0..=up_to {
                    let hash1 = log1.get(&op_num);
                    let hash2 = log2.get(&op_num);

                    // Both have this op - must match
                    if let (Some(h1), Some(h2)) = (hash1, hash2) {
                        if h1 != h2 {
                            return InvariantResult::Violated {
                                invariant: "vsr_prefix_property".to_string(),
                                message: format!(
                                    "Replicas {} and {} disagree on operation {} (prefix divergence)",
                                    ReplicaId::new(replica1),
                                    ReplicaId::new(replica2),
                                    op_num
                                ),
                                context: vec![
                                    ("replica_1".to_string(), replica1.to_string()),
                                    ("replica_2".to_string(), replica2.to_string()),
                                    ("op_number".to_string(), op_num.to_string()),
                                    ("hash_1".to_string(), format!("{:?}", h1)),
                                    ("hash_2".to_string(), format!("{:?}", h2)),
                                ],
                            };
                        }
                    }
                    // One has it, other doesn't - not a violation yet
                    // (one replica might be ahead)
                }
            }
        }

        InvariantResult::Ok
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }

    /// Resets the checker state (for testing).
    pub fn reset(&mut self) {
        self.replica_logs.clear();
        self.checks_performed = 0;
    }
}

impl Default for PrefixPropertyChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// View-Change Safety Invariant Checker
// ============================================================================

/// Verifies that view changes preserve all committed operations.
///
/// **Invariant**: After a view change completes, the new primary has all
/// operations that were committed in the previous view.
///
/// **Violation**: View change from view V1 to V2, where V1 had committed ops [A, B, C]
/// but V2's new primary only has [A, B].
///
/// **Why it matters**: View changes must preserve committed operations - losing
/// committed data violates durability guarantees.
///
/// **Expected to catch**:
/// - Premature view-change completion (before gathering all committed ops)
/// - Unsafe log truncation during view-change
/// - Quorum calculation bugs in DoViewChange
#[derive(Debug)]
pub struct ViewChangeSafetyChecker {
    /// Map from view_number -> highest committed op_number in that view
    committed_in_view: HashMap<u64, u64>,

    /// Map from view_number -> primary's log after view change
    primary_after_view_change: HashMap<u64, BTreeMap<u64, ChainHash>>,

    /// Total checks performed
    checks_performed: u64,
}

impl ViewChangeSafetyChecker {
    /// Creates a new view-change safety checker.
    pub fn new() -> Self {
        Self {
            committed_in_view: HashMap::new(),
            primary_after_view_change: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records the highest committed operation in a view.
    pub fn record_committed_in_view(&mut self, view: ViewNumber, highest_op: OpNumber) {
        let view_u64 = view.as_u64();
        let op_u64 = highest_op.as_u64();

        self.committed_in_view
            .entry(view_u64)
            .and_modify(|current| *current = (*current).max(op_u64))
            .or_insert(op_u64);
    }

    /// Records the new primary's log after a view change.
    ///
    /// Returns a violation if the new primary is missing operations that
    /// were committed in the previous view.
    pub fn record_view_change_complete(
        &mut self,
        new_view: ViewNumber,
        primary_log: &BTreeMap<u64, ChainHash>,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("vsr_view_change_safety");
        self.checks_performed += 1;

        let new_view_u64 = new_view.as_u64();

        // Check if we have any committed ops from the previous view
        if new_view_u64 > 0 {
            let prev_view = new_view_u64 - 1;

            if let Some(&highest_committed) = self.committed_in_view.get(&prev_view) {
                // Verify new primary has all committed ops from previous view
                for op_num in 0..=highest_committed {
                    if !primary_log.contains_key(&op_num) {
                        return InvariantResult::Violated {
                            invariant: "vsr_view_change_safety".to_string(),
                            message: format!(
                                "View change from {} to {} lost committed operation {}",
                                prev_view, new_view_u64, op_num
                            ),
                            context: vec![
                                ("previous_view".to_string(), prev_view.to_string()),
                                ("new_view".to_string(), new_view_u64.to_string()),
                                ("missing_op".to_string(), op_num.to_string()),
                                (
                                    "highest_committed_in_prev".to_string(),
                                    highest_committed.to_string(),
                                ),
                            ],
                        };
                    }
                }
            }
        }

        // Store the new primary's log
        self.primary_after_view_change.insert(new_view_u64, primary_log.clone());

        InvariantResult::Ok
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }

    /// Resets the checker state (for testing).
    pub fn reset(&mut self) {
        self.committed_in_view.clear();
        self.primary_after_view_change.clear();
        self.checks_performed = 0;
    }
}

impl Default for ViewChangeSafetyChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Recovery Safety Invariant Checker
// ============================================================================

/// Verifies that recovery never discards committed offsets.
///
/// **Invariant**: During recovery, a replica never truncates its log to a
/// point below the highest committed operation.
///
/// **Violation**: Replica had committed operations [0, 1, 2, 3] before crash,
/// but after recovery only has [0, 1].
///
/// **Why it matters**: Recovery must preserve durability - discarding committed
/// operations breaks the "acknowledged writes persist" guarantee.
///
/// **Expected to catch**:
/// - Unsafe log truncation during recovery
/// - Missing NACK quorum checks (Protocol-Aware Recovery)
/// - Superblock corruption leading to incorrect recovery state
#[derive(Debug)]
pub struct RecoverySafetyChecker {
    /// Map from replica_id -> highest committed op before crash
    pre_crash_commit: HashMap<u8, u64>,

    /// Total checks performed
    checks_performed: u64,
}

impl RecoverySafetyChecker {
    /// Creates a new recovery safety checker.
    pub fn new() -> Self {
        Self {
            pre_crash_commit: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records the highest committed operation for a replica before a crash.
    pub fn record_pre_crash_state(&mut self, replica_id: ReplicaId, highest_commit: OpNumber) {
        let replica_u8 = replica_id.as_u8();
        let commit_u64 = highest_commit.as_u64();

        self.pre_crash_commit.insert(replica_u8, commit_u64);
    }

    /// Checks that recovery didn't discard committed operations.
    ///
    /// Returns a violation if the replica's log after recovery has a lower
    /// commit point than before the crash.
    pub fn check_post_recovery_state(
        &mut self,
        replica_id: ReplicaId,
        post_recovery_commit: OpNumber,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("vsr_recovery_safety");
        self.checks_performed += 1;

        let replica_u8 = replica_id.as_u8();
        let post_commit_u64 = post_recovery_commit.as_u64();

        if let Some(&pre_commit) = self.pre_crash_commit.get(&replica_u8) {
            if post_commit_u64 < pre_commit {
                return InvariantResult::Violated {
                    invariant: "vsr_recovery_safety".to_string(),
                    message: format!(
                        "Replica {} discarded committed operations during recovery (had {}, now has {})",
                        replica_id, pre_commit, post_commit_u64
                    ),
                    context: vec![
                        ("replica_id".to_string(), replica_u8.to_string()),
                        ("pre_crash_commit".to_string(), pre_commit.to_string()),
                        ("post_recovery_commit".to_string(), post_commit_u64.to_string()),
                        (
                            "discarded_ops".to_string(),
                            (pre_commit - post_commit_u64).to_string(),
                        ),
                    ],
                };
            }
        }

        InvariantResult::Ok
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }

    /// Resets the checker state (for testing).
    pub fn reset(&mut self) {
        self.pre_crash_commit.clear();
        self.checks_performed = 0;
    }
}

impl Default for RecoverySafetyChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_vsr::{ViewNumber, OpNumber, ReplicaId};

    // Helper to create a dummy hash
    fn dummy_hash(value: u8) -> ChainHash {
        let mut bytes = [0u8; 32];
        bytes[0] = value;
        ChainHash::from_bytes(&bytes)
    }

    #[test]
    fn test_agreement_checker_ok() {
        let mut checker = AgreementChecker::new();

        let replica1 = ReplicaId::new(0);
        let replica2 = ReplicaId::new(1);
        let view = ViewNumber::from(1);
        let op = OpNumber::from(5);
        let hash = dummy_hash(42);

        // Both replicas commit the same operation - OK
        assert!(matches!(
            checker.record_commit(replica1, view, op, &hash),
            InvariantResult::Ok
        ));
        assert!(matches!(
            checker.record_commit(replica2, view, op, &hash),
            InvariantResult::Ok
        ));

        assert_eq!(checker.checks_performed(), 2);
    }

    #[test]
    fn test_agreement_checker_violation() {
        let mut checker = AgreementChecker::new();

        let replica1 = ReplicaId::new(0);
        let replica2 = ReplicaId::new(1);
        let view = ViewNumber::from(1);
        let op = OpNumber::from(5);
        let hash1 = dummy_hash(42);
        let hash2 = dummy_hash(99);

        // First replica commits
        assert!(matches!(
            checker.record_commit(replica1, view, op, &hash1),
            InvariantResult::Ok
        ));

        // Second replica commits different operation at same position - VIOLATION
        let result = checker.record_commit(replica2, view, op, &hash2);
        assert!(matches!(result, InvariantResult::Violated { .. }));

        if let InvariantResult::Violated { invariant, message, .. } = result {
            assert_eq!(invariant, "vsr_agreement");
            assert!(message.contains("different operations"));
        }
    }

    #[test]
    fn test_prefix_property_checker_ok() {
        let mut checker = PrefixPropertyChecker::new();

        let replica1 = ReplicaId::new(0);
        let replica2 = ReplicaId::new(1);

        // Both replicas have same prefix
        checker.record_committed_op(replica1, OpNumber::from(0), &dummy_hash(1));
        checker.record_committed_op(replica1, OpNumber::from(1), &dummy_hash(2));
        checker.record_committed_op(replica1, OpNumber::from(2), &dummy_hash(3));

        checker.record_committed_op(replica2, OpNumber::from(0), &dummy_hash(1));
        checker.record_committed_op(replica2, OpNumber::from(1), &dummy_hash(2));

        // Check prefix up to op 1 - both match
        assert!(matches!(
            checker.check_prefix_agreement(OpNumber::from(1)),
            InvariantResult::Ok
        ));
    }

    #[test]
    fn test_prefix_property_checker_violation() {
        let mut checker = PrefixPropertyChecker::new();

        let replica1 = ReplicaId::new(0);
        let replica2 = ReplicaId::new(1);

        // Divergent histories
        checker.record_committed_op(replica1, OpNumber::from(0), &dummy_hash(1));
        checker.record_committed_op(replica1, OpNumber::from(1), &dummy_hash(2));

        checker.record_committed_op(replica2, OpNumber::from(0), &dummy_hash(1));
        checker.record_committed_op(replica2, OpNumber::from(1), &dummy_hash(99)); // Different!

        // Check prefix - should detect divergence
        let result = checker.check_prefix_agreement(OpNumber::from(1));
        assert!(matches!(result, InvariantResult::Violated { .. }));
    }

    #[test]
    fn test_view_change_safety_ok() {
        let mut checker = ViewChangeSafetyChecker::new();

        // View 0 committed ops 0-5
        checker.record_committed_in_view(ViewNumber::from(0), OpNumber::from(5));

        // View 1's new primary has all ops
        let mut primary_log = BTreeMap::new();
        for i in 0..=5 {
            primary_log.insert(i, dummy_hash(i as u8));
        }

        assert!(matches!(
            checker.record_view_change_complete(ViewNumber::from(1), &primary_log),
            InvariantResult::Ok
        ));
    }

    #[test]
    fn test_view_change_safety_violation() {
        let mut checker = ViewChangeSafetyChecker::new();

        // View 0 committed ops 0-5
        checker.record_committed_in_view(ViewNumber::from(0), OpNumber::from(5));

        // View 1's new primary only has ops 0-3 (missing 4, 5!)
        let mut primary_log = BTreeMap::new();
        for i in 0..=3 {
            primary_log.insert(i, dummy_hash(i as u8));
        }

        let result = checker.record_view_change_complete(ViewNumber::from(1), &primary_log);
        assert!(matches!(result, InvariantResult::Violated { .. }));
    }

    #[test]
    fn test_recovery_safety_ok() {
        let mut checker = RecoverySafetyChecker::new();

        let replica = ReplicaId::new(0);

        // Before crash: committed up to op 10
        checker.record_pre_crash_state(replica, OpNumber::from(10));

        // After recovery: still has all ops (or more)
        assert!(matches!(
            checker.check_post_recovery_state(replica, OpNumber::from(10)),
            InvariantResult::Ok
        ));
        assert!(matches!(
            checker.check_post_recovery_state(replica, OpNumber::from(15)),
            InvariantResult::Ok
        ));
    }

    #[test]
    fn test_recovery_safety_violation() {
        let mut checker = RecoverySafetyChecker::new();

        let replica = ReplicaId::new(0);

        // Before crash: committed up to op 10
        checker.record_pre_crash_state(replica, OpNumber::from(10));

        // After recovery: only has up to op 7 (lost ops 8, 9, 10!)
        let result = checker.check_post_recovery_state(replica, OpNumber::from(7));
        assert!(matches!(result, InvariantResult::Violated { .. }));
    }

    #[test]
    fn test_all_checkers_track_execution() {
        use crate::instrumentation::invariant_tracker;

        // Reset tracking
        invariant_tracker::reset_invariant_tracker();

        let mut agreement = AgreementChecker::new();
        let mut prefix = PrefixPropertyChecker::new();
        let mut view_change = ViewChangeSafetyChecker::new();
        let mut recovery = RecoverySafetyChecker::new();

        // Perform checks
        let _ = agreement.record_commit(
            ReplicaId::new(0),
            ViewNumber::from(1),
            OpNumber::from(1),
            &dummy_hash(1),
        );
        let _ = prefix.check_prefix_agreement(OpNumber::from(1));
        let _ = view_change.record_view_change_complete(ViewNumber::from(1), &BTreeMap::new());
        let _ = recovery.check_post_recovery_state(ReplicaId::new(0), OpNumber::from(1));

        // Verify tracking
        let tracker = invariant_tracker::get_invariant_tracker();
        assert_eq!(tracker.get_run_count("vsr_agreement"), 1);
        assert_eq!(tracker.get_run_count("vsr_prefix_property"), 1);
        assert_eq!(tracker.get_run_count("vsr_view_change_safety"), 1);
        assert_eq!(tracker.get_run_count("vsr_recovery_safety"), 1);
    }
}
