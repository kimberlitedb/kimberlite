use kimberlite_crypto::ChainHash;
use kimberlite_vsr::{CommitNumber, OpNumber, ReplicaId, ViewNumber};
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
use std::collections::{BTreeMap, HashMap};

use crate::instrumentation::invariant_tracker;
use crate::invariant::InvariantResult;

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

        let log = self
            .replica_logs
            .entry(replica_u8)
            .or_insert_with(BTreeMap::new);
        log.insert(op_u64, *operation_hash);
    }

    /// Checks that all replicas agree on the prefix up to the given op number.
    ///
    /// Returns an invariant violation if any two replicas disagree on the
    /// committed operation at any position <= op_number.
    ///
    /// # Performance
    ///
    /// Uses sparse iteration - only checks positions where operations actually exist
    /// instead of checking all positions 0..up_to. This reduces O(n³) to O(actual_ops).
    pub fn check_prefix_agreement(&mut self, up_to_op: OpNumber) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("vsr_prefix_property");
        self.checks_performed += 1;

        let up_to = up_to_op.as_u64();

        // Compare all pairs of replicas
        let replica_ids: Vec<u8> = self.replica_logs.keys().copied().collect();

        if replica_ids.len() < 2 {
            return InvariantResult::Ok;
        }

        for i in 0..replica_ids.len() {
            for j in (i + 1)..replica_ids.len() {
                let replica1 = replica_ids[i];
                let replica2 = replica_ids[j];

                let log1 = &self.replica_logs[&replica1];
                let log2 = &self.replica_logs[&replica2];

                // Sparse iteration: only check positions where at least one replica has an op
                // Collect all op numbers present in either log (up to up_to)
                let mut op_numbers = std::collections::BTreeSet::new();
                for &op_num in log1.keys() {
                    if op_num <= up_to {
                        op_numbers.insert(op_num);
                    }
                }
                for &op_num in log2.keys() {
                    if op_num <= up_to {
                        op_numbers.insert(op_num);
                    }
                }

                // Check only the positions where operations exist
                for op_num in op_numbers {
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
        self.primary_after_view_change
            .insert(new_view_u64, primary_log.clone());

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
                        (
                            "post_recovery_commit".to_string(),
                            post_commit_u64.to_string(),
                        ),
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
// Enhanced Invariant Checkers for Byzantine Testing
// ============================================================================

/// Verifies that commit_number never exceeds op_number.
///
/// **Invariant**: `commit_number <= op_number` at all times.
///
/// **Violation**: A replica claims to have committed more operations than it has in its log.
///
/// **Why it matters**: Commit number inflation can lead to state machine corruption when
/// replicas try to apply non-existent operations.
///
/// **Expected to catch**: Bug #2 (commit desync) and Bug #3 (inflated commit)
#[derive(Debug)]
pub struct CommitNumberConsistencyChecker {
    /// Map from replica_id -> (op_number, commit_number)
    replica_state: HashMap<u8, (u64, u64)>,
    /// Total checks performed
    checks_performed: u64,
}

impl CommitNumberConsistencyChecker {
    /// Creates a new commit number consistency checker.
    pub fn new() -> Self {
        Self {
            replica_state: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records replica state and checks consistency.
    pub fn check_consistency(
        &mut self,
        replica_id: ReplicaId,
        op_number: OpNumber,
        commit_number: CommitNumber,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("commit_number_consistency");
        self.checks_performed += 1;

        let replica_u8 = replica_id.as_u8();
        let op_u64 = op_number.as_u64();
        let commit_u64 = commit_number.as_u64();

        // Check invariant: commit_number <= op_number
        if commit_u64 > op_u64 {
            return InvariantResult::Violated {
                invariant: "commit_number_consistency".to_string(),
                message: format!(
                    "Replica {} has commit_number ({}) > op_number ({})",
                    replica_id, commit_u64, op_u64
                ),
                context: vec![
                    ("replica_id".to_string(), replica_u8.to_string()),
                    ("op_number".to_string(), op_u64.to_string()),
                    ("commit_number".to_string(), commit_u64.to_string()),
                    ("inflation".to_string(), (commit_u64 - op_u64).to_string()),
                ],
            };
        }

        // Store current state
        self.replica_state.insert(replica_u8, (op_u64, commit_u64));

        InvariantResult::Ok
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }

    /// Resets the checker state.
    pub fn reset(&mut self) {
        self.replica_state.clear();
        self.checks_performed = 0;
    }
}

impl Default for CommitNumberConsistencyChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Verifies that merge_log_tail never overwrites committed entries.
///
/// **Invariant**: Log entries below commit_number are immutable.
///
/// **Violation**: A StartView message causes merge_log_tail to replace a committed entry.
///
/// **Why it matters**: Overwriting committed entries violates durability and agreement.
///
/// **Expected to catch**: Bug #1 (view change merge overwrites)
#[derive(Debug)]
pub struct MergeLogSafetyChecker {
    /// Map from replica_id -> Map<op_number, (operation_hash, is_committed)>
    replica_logs: HashMap<u8, BTreeMap<u64, (ChainHash, bool)>>,
    /// Total checks performed
    checks_performed: u64,
}

impl MergeLogSafetyChecker {
    /// Creates a new merge log safety checker.
    pub fn new() -> Self {
        Self {
            replica_logs: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records an entry in a replica's log.
    pub fn record_entry(
        &mut self,
        replica_id: ReplicaId,
        op_number: OpNumber,
        operation_hash: &ChainHash,
        is_committed: bool,
    ) {
        let replica_u8 = replica_id.as_u8();
        let op_u64 = op_number.as_u64();

        let log = self
            .replica_logs
            .entry(replica_u8)
            .or_insert_with(BTreeMap::new);
        log.insert(op_u64, (*operation_hash, is_committed));
    }

    /// Checks that a merge operation doesn't overwrite committed entries.
    pub fn check_merge(
        &mut self,
        replica_id: ReplicaId,
        op_number: OpNumber,
        new_hash: &ChainHash,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("merge_log_safety");
        self.checks_performed += 1;

        let replica_u8 = replica_id.as_u8();
        let op_u64 = op_number.as_u64();

        if let Some(log) = self.replica_logs.get(&replica_u8) {
            if let Some((existing_hash, is_committed)) = log.get(&op_u64) {
                if *is_committed && existing_hash != new_hash {
                    return InvariantResult::Violated {
                        invariant: "merge_log_safety".to_string(),
                        message: format!(
                            "Replica {} attempted to overwrite committed entry at op {}",
                            replica_id, op_u64
                        ),
                        context: vec![
                            ("replica_id".to_string(), replica_u8.to_string()),
                            ("op_number".to_string(), op_u64.to_string()),
                            ("existing_hash".to_string(), format!("{:?}", existing_hash)),
                            ("new_hash".to_string(), format!("{:?}", new_hash)),
                        ],
                    };
                }
            }
        }

        InvariantResult::Ok
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }

    /// Resets the checker state.
    pub fn reset(&mut self) {
        self.replica_logs.clear();
        self.checks_performed = 0;
    }
}

impl Default for MergeLogSafetyChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Commit Monotonicity Checker
// ============================================================================

/// Verifies that commit_number never regresses across views.
///
/// **Invariant**: For any replica, commit_number must be monotonically increasing.
///
/// **Violation**: A replica's commit_number decreases after a view change.
///
/// **Why it matters**: Committed operations are immutable. If commit_number regresses,
/// we might "uncommit" operations, breaking the fundamental guarantee of consensus.
///
/// **Expected to catch**:
/// - View change bugs that reset commit_number incorrectly
/// - State transfer bugs that overwrite committed state
/// - Byzantine leaders claiming lower commit numbers
#[derive(Debug)]
pub struct CommitMonotonicityChecker {
    /// Map from replica_id -> highest commit_number seen
    highest_commit: HashMap<u8, u64>,
    checks_performed: u64,
}

impl CommitMonotonicityChecker {
    pub fn new() -> Self {
        Self {
            highest_commit: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records a replica's commit_number and checks for regression.
    pub fn record_commit_number(
        &mut self,
        replica_id: ReplicaId,
        commit: CommitNumber,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_commit_monotonicity");
        self.checks_performed += 1;

        let replica_u8 = replica_id.as_u8();
        let commit_u64 = commit.as_u64();

        if let Some(&prev_commit) = self.highest_commit.get(&replica_u8) {
            if commit_u64 < prev_commit {
                return InvariantResult::Violated {
                    invariant: "vsr_commit_monotonicity".to_string(),
                    message: format!(
                        "Replica {} commit_number regressed from {} to {}",
                        replica_id, prev_commit, commit_u64
                    ),
                    context: vec![
                        ("replica".to_string(), replica_u8.to_string()),
                        ("previous_commit".to_string(), prev_commit.to_string()),
                        ("new_commit".to_string(), commit_u64.to_string()),
                    ],
                };
            }
        }

        self.highest_commit.insert(
            replica_u8,
            commit_u64.max(*self.highest_commit.get(&replica_u8).unwrap_or(&0)),
        );
        InvariantResult::Ok
    }
}

impl Default for CommitMonotonicityChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// View Number Monotonicity Checker
// ============================================================================

/// Verifies that view numbers only increase, never decrease.
///
/// **Invariant**: For any replica, view_number must be monotonically increasing.
///
/// **Violation**: A replica's view_number decreases.
///
/// **Why it matters**: View numbers provide a total order over leader elections.
/// Regression would break the fundamental VSR assumption of monotonic view progression.
#[derive(Debug)]
pub struct ViewNumberMonotonicityChecker {
    /// Map from replica_id -> highest view_number seen
    highest_view: HashMap<u8, u64>,
    checks_performed: u64,
}

impl ViewNumberMonotonicityChecker {
    pub fn new() -> Self {
        Self {
            highest_view: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records a replica's view_number and checks for regression.
    pub fn record_view_number(
        &mut self,
        replica_id: ReplicaId,
        view: ViewNumber,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_view_monotonicity");
        self.checks_performed += 1;

        let replica_u8 = replica_id.as_u8();
        let view_u64 = view.as_u64();

        if let Some(&prev_view) = self.highest_view.get(&replica_u8) {
            if view_u64 < prev_view {
                return InvariantResult::Violated {
                    invariant: "vsr_view_monotonicity".to_string(),
                    message: format!(
                        "Replica {} view_number regressed from {} to {}",
                        replica_id, prev_view, view_u64
                    ),
                    context: vec![
                        ("replica".to_string(), replica_u8.to_string()),
                        ("previous_view".to_string(), prev_view.to_string()),
                        ("new_view".to_string(), view_u64.to_string()),
                    ],
                };
            }
        }

        self.highest_view.insert(
            replica_u8,
            view_u64.max(*self.highest_view.get(&replica_u8).unwrap_or(&0)),
        );
        InvariantResult::Ok
    }
}

impl Default for ViewNumberMonotonicityChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Idempotency Checker
// ============================================================================

/// Verifies that operations are not double-applied.
///
/// **Invariant**: Each operation is applied exactly once per replica.
///
/// **Violation**: An operation is applied multiple times with different results.
///
/// **Why it matters**: Double-application breaks state machine semantics and
/// can cause data corruption, account balance errors, etc.
#[derive(Debug)]
pub struct IdempotencyChecker {
    /// Map from (replica_id, op_number) -> operation_hash
    /// Tracks which operations have been applied
    applied_ops: HashMap<(u8, u64), ChainHash>,
    checks_performed: u64,
}

impl IdempotencyChecker {
    pub fn new() -> Self {
        Self {
            applied_ops: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records that a replica applied an operation and checks for double-application.
    pub fn record_apply(
        &mut self,
        replica_id: ReplicaId,
        op: OpNumber,
        operation_hash: &ChainHash,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_idempotency");
        self.checks_performed += 1;

        let key = (replica_id.as_u8(), op.as_u64());

        if let Some(existing_hash) = self.applied_ops.get(&key) {
            if existing_hash != operation_hash {
                return InvariantResult::Violated {
                    invariant: "vsr_idempotency".to_string(),
                    message: format!(
                        "Replica {} applied op {} multiple times with different hashes",
                        replica_id,
                        op.as_u64()
                    ),
                    context: vec![
                        ("replica".to_string(), replica_id.as_u8().to_string()),
                        ("op".to_string(), op.as_u64().to_string()),
                        ("first_hash".to_string(), format!("{:?}", existing_hash)),
                        ("second_hash".to_string(), format!("{:?}", operation_hash)),
                    ],
                };
            }
        }

        self.applied_ops.insert(key, operation_hash.clone());
        InvariantResult::Ok
    }
}

impl Default for IdempotencyChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Log Checksum Chain Checker
// ============================================================================

/// Verifies continuous hash chain integrity in the log.
///
/// **Invariant**: Each log entry's checksum must match when recomputed.
///
/// **Violation**: A log entry has an incorrect checksum.
///
/// **Why it matters**: Checksums detect corruption. A broken checksum chain
/// indicates either corruption or a Byzantine attack.
#[derive(Debug)]
pub struct LogChecksumChainChecker {
    checks_performed: u64,
}

impl LogChecksumChainChecker {
    pub fn new() -> Self {
        Self {
            checks_performed: 0,
        }
    }

    /// Verifies that a log entry's checksum is correct.
    pub fn verify_entry_checksum(
        &mut self,
        op: OpNumber,
        claimed_checksum: u32,
        computed_checksum: u32,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_checksum_chain");
        self.checks_performed += 1;

        if claimed_checksum != computed_checksum {
            return InvariantResult::Violated {
                invariant: "vsr_checksum_chain".to_string(),
                message: format!(
                    "Log entry at op {} has checksum mismatch: claimed {}, computed {}",
                    op.as_u64(),
                    claimed_checksum,
                    computed_checksum
                ),
                context: vec![
                    ("op".to_string(), op.as_u64().to_string()),
                    ("claimed_checksum".to_string(), claimed_checksum.to_string()),
                    (
                        "computed_checksum".to_string(),
                        computed_checksum.to_string(),
                    ),
                ],
            };
        }

        InvariantResult::Ok
    }
}

impl Default for LogChecksumChainChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// State Transfer Safety Checker
// ============================================================================

/// Verifies that state transfer preserves all committed operations.
///
/// **Invariant**: After state transfer, all previously committed operations
/// are still present.
///
/// **Violation**: State transfer causes loss of committed operations.
///
/// **Why it matters**: State transfer must preserve committed state to maintain
/// durability guarantees.
#[derive(Debug)]
pub struct StateTransferSafetyChecker {
    /// Map from replica_id -> commit_number before state transfer
    pre_transfer_commits: HashMap<u8, u64>,
    checks_performed: u64,
}

impl StateTransferSafetyChecker {
    pub fn new() -> Self {
        Self {
            pre_transfer_commits: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records a replica's commit_number before state transfer.
    pub fn record_pre_transfer(&mut self, replica_id: ReplicaId, commit: CommitNumber) {
        self.pre_transfer_commits
            .insert(replica_id.as_u8(), commit.as_u64());
    }

    /// Checks that post-transfer commit_number hasn't regressed.
    pub fn check_post_transfer(
        &mut self,
        replica_id: ReplicaId,
        post_commit: CommitNumber,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_state_transfer_safety");
        self.checks_performed += 1;

        if let Some(&pre_commit) = self.pre_transfer_commits.get(&replica_id.as_u8()) {
            if post_commit.as_u64() < pre_commit {
                return InvariantResult::Violated {
                    invariant: "vsr_state_transfer_safety".to_string(),
                    message: format!(
                        "Replica {} lost committed operations during state transfer: {} -> {}",
                        replica_id,
                        pre_commit,
                        post_commit.as_u64()
                    ),
                    context: vec![
                        ("replica".to_string(), replica_id.as_u8().to_string()),
                        ("pre_transfer_commit".to_string(), pre_commit.to_string()),
                        (
                            "post_transfer_commit".to_string(),
                            post_commit.as_u64().to_string(),
                        ),
                    ],
                };
            }
        }

        InvariantResult::Ok
    }
}

impl Default for StateTransferSafetyChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Repair Completion Checker
// ============================================================================

/// Verifies that repair requests eventually complete or timeout.
///
/// **Invariant**: Repair requests don't remain pending indefinitely.
///
/// **Violation**: A repair request is pending for too long without progress.
///
/// **Why it matters**: Stuck repairs can stall replica progress and prevent
/// the system from making forward progress.
#[derive(Debug)]
pub struct RepairCompletionChecker {
    /// Map from (replica_id, start_op, end_op) -> timestamp when repair started
    pending_repairs: HashMap<(u8, u64, u64), u64>,
    /// Maximum allowed repair duration (in simulation ticks)
    max_repair_duration: u64,
    checks_performed: u64,
}

impl RepairCompletionChecker {
    pub fn new(max_repair_duration: u64) -> Self {
        Self {
            pending_repairs: HashMap::new(),
            max_repair_duration,
            checks_performed: 0,
        }
    }

    /// Records the start of a repair request.
    pub fn record_repair_start(
        &mut self,
        replica_id: ReplicaId,
        start_op: OpNumber,
        end_op: OpNumber,
        timestamp: u64,
    ) {
        let key = (replica_id.as_u8(), start_op.as_u64(), end_op.as_u64());
        self.pending_repairs.insert(key, timestamp);
    }

    /// Records the completion of a repair request.
    pub fn record_repair_complete(
        &mut self,
        replica_id: ReplicaId,
        start_op: OpNumber,
        end_op: OpNumber,
    ) {
        let key = (replica_id.as_u8(), start_op.as_u64(), end_op.as_u64());
        self.pending_repairs.remove(&key);
    }

    /// Checks that all pending repairs are within the time limit.
    pub fn check_repair_timeouts(&mut self, current_timestamp: u64) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_repair_completion");
        self.checks_performed += 1;

        for ((replica_u8, start_op, end_op), &start_time) in &self.pending_repairs {
            let duration = current_timestamp.saturating_sub(start_time);
            if duration > self.max_repair_duration {
                return InvariantResult::Violated {
                    invariant: "vsr_repair_completion".to_string(),
                    message: format!(
                        "Replica {} repair [{}, {}) stuck for {} ticks (max: {})",
                        ReplicaId::new(*replica_u8),
                        start_op,
                        end_op,
                        duration,
                        self.max_repair_duration
                    ),
                    context: vec![
                        ("replica".to_string(), replica_u8.to_string()),
                        ("start_op".to_string(), start_op.to_string()),
                        ("end_op".to_string(), end_op.to_string()),
                        ("duration".to_string(), duration.to_string()),
                        (
                            "max_duration".to_string(),
                            self.max_repair_duration.to_string(),
                        ),
                    ],
                };
            }
        }

        InvariantResult::Ok
    }
}

impl Default for RepairCompletionChecker {
    fn default() -> Self {
        Self::new(10_000) // Default: 10k ticks
    }
}

// ============================================================================
// Leader Election Race Checker
// ============================================================================

/// Detects multiple leaders in the same view (split-brain).
///
/// **Invariant**: At most one leader per view.
///
/// **Violation**: Two replicas both act as leader in the same view.
///
/// **Why it matters**: Multiple leaders can commit conflicting operations,
/// breaking consensus safety.
#[derive(Debug)]
pub struct LeaderElectionRaceChecker {
    /// Map from view_number -> replica_id of the leader
    leaders_by_view: HashMap<u64, u8>,
    checks_performed: u64,
}

impl LeaderElectionRaceChecker {
    pub fn new() -> Self {
        Self {
            leaders_by_view: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records that a replica is acting as leader in a view.
    pub fn record_leader_action(
        &mut self,
        replica_id: ReplicaId,
        view: ViewNumber,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_leader_election_race");
        self.checks_performed += 1;

        let view_u64 = view.as_u64();
        let replica_u8 = replica_id.as_u8();

        if let Some(&existing_leader) = self.leaders_by_view.get(&view_u64) {
            if existing_leader != replica_u8 {
                return InvariantResult::Violated {
                    invariant: "vsr_leader_election_race".to_string(),
                    message: format!(
                        "Split-brain detected: both {} and {} are leaders in view {}",
                        ReplicaId::new(existing_leader),
                        replica_id,
                        view_u64
                    ),
                    context: vec![
                        ("view".to_string(), view_u64.to_string()),
                        ("first_leader".to_string(), existing_leader.to_string()),
                        ("second_leader".to_string(), replica_u8.to_string()),
                    ],
                };
            }
        }

        self.leaders_by_view.insert(view_u64, replica_u8);
        InvariantResult::Ok
    }
}

impl Default for LeaderElectionRaceChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Heartbeat Liveness Checker
// ============================================================================

/// Verifies that leaders send heartbeats to maintain liveness.
///
/// **Invariant**: Leaders must send heartbeats within the heartbeat interval.
///
/// **Violation**: A leader is silent for too long.
///
/// **Why it matters**: Missing heartbeats can cause unnecessary view changes
/// and reduce system availability.
#[derive(Debug)]
pub struct HeartbeatLivenessChecker {
    /// Map from (view, leader_id) -> timestamp of last heartbeat
    last_heartbeat: HashMap<(u64, u8), u64>,
    /// Maximum allowed heartbeat interval (in simulation ticks)
    max_heartbeat_interval: u64,
    checks_performed: u64,
}

impl HeartbeatLivenessChecker {
    pub fn new(max_heartbeat_interval: u64) -> Self {
        Self {
            last_heartbeat: HashMap::new(),
            max_heartbeat_interval,
            checks_performed: 0,
        }
    }

    /// Records a heartbeat from a leader.
    pub fn record_heartbeat(&mut self, view: ViewNumber, leader: ReplicaId, timestamp: u64) {
        let key = (view.as_u64(), leader.as_u8());
        self.last_heartbeat.insert(key, timestamp);
    }

    /// Checks that the leader is sending heartbeats.
    pub fn check_heartbeat_liveness(
        &mut self,
        view: ViewNumber,
        leader: ReplicaId,
        current_timestamp: u64,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_heartbeat_liveness");
        self.checks_performed += 1;

        let key = (view.as_u64(), leader.as_u8());

        if let Some(&last_time) = self.last_heartbeat.get(&key) {
            let elapsed = current_timestamp.saturating_sub(last_time);
            if elapsed > self.max_heartbeat_interval {
                return InvariantResult::Violated {
                    invariant: "vsr_heartbeat_liveness".to_string(),
                    message: format!(
                        "Leader {} in view {} hasn't sent heartbeat for {} ticks (max: {})",
                        leader,
                        view.as_u64(),
                        elapsed,
                        self.max_heartbeat_interval
                    ),
                    context: vec![
                        ("view".to_string(), view.as_u64().to_string()),
                        ("leader".to_string(), leader.as_u8().to_string()),
                        ("elapsed".to_string(), elapsed.to_string()),
                        (
                            "max_interval".to_string(),
                            self.max_heartbeat_interval.to_string(),
                        ),
                    ],
                };
            }
        }

        InvariantResult::Ok
    }
}

impl Default for HeartbeatLivenessChecker {
    fn default() -> Self {
        Self::new(5_000) // Default: 5k ticks
    }
}

// ============================================================================
// Tenant Isolation Checker
// ============================================================================

/// Verifies that there is no cross-tenant data leakage.
///
/// **Invariant**: Operations from one tenant must never appear in another tenant's results.
///
/// **Violation**: A tenant receives data belonging to a different tenant.
///
/// **Why it matters**: Tenant isolation is CRITICAL for compliance (HIPAA, GDPR, SOC 2).
/// Leakage could expose PII, PHI, or confidential business data.
#[derive(Debug)]
pub struct TenantIsolationChecker {
    /// Map from (tenant_id, stream_id) -> owner_tenant_id
    /// Tracks which tenant owns each stream
    stream_ownership: HashMap<(u64, u64), u64>,
    checks_performed: u64,
}

impl TenantIsolationChecker {
    pub fn new() -> Self {
        Self {
            stream_ownership: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records stream ownership.
    pub fn record_stream_creation(&mut self, tenant_id: u64, stream_id: u64) {
        self.stream_ownership
            .insert((tenant_id, stream_id), tenant_id);
    }

    /// Checks that an operation accesses only streams owned by the same tenant.
    pub fn check_access(&mut self, accessing_tenant: u64, stream_id: u64) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_tenant_isolation");
        self.checks_performed += 1;

        // Find if this stream exists under any tenant
        for ((_, sid), &owner_tenant) in &self.stream_ownership {
            if *sid == stream_id {
                if owner_tenant != accessing_tenant {
                    return InvariantResult::Violated {
                        invariant: "vsr_tenant_isolation".to_string(),
                        message: format!(
                            "Tenant {} accessed stream {} owned by tenant {}",
                            accessing_tenant, stream_id, owner_tenant
                        ),
                        context: vec![
                            ("accessing_tenant".to_string(), accessing_tenant.to_string()),
                            ("stream_id".to_string(), stream_id.to_string()),
                            ("owner_tenant".to_string(), owner_tenant.to_string()),
                        ],
                    };
                }
                break;
            }
        }

        InvariantResult::Ok
    }
}

impl Default for TenantIsolationChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Corruption Detection Checker
// ============================================================================

/// Verifies that corruption is detected by checksums before it propagates.
///
/// **Invariant**: Corrupted data must be detected and rejected.
///
/// **Violation**: Corrupted data is accepted and applied.
///
/// **Why it matters**: Silent corruption can lead to data loss, incorrect results,
/// and cascading failures.
#[derive(Debug)]
pub struct CorruptionDetectionChecker {
    /// Map from op_number -> (original_checksum, corruption_injected)
    /// Tracks which ops have had corruption injected
    corruption_injections: HashMap<u64, (u32, bool)>,
    checks_performed: u64,
}

impl CorruptionDetectionChecker {
    pub fn new() -> Self {
        Self {
            corruption_injections: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records that corruption was injected into an operation.
    pub fn record_corruption_injection(&mut self, op: OpNumber, original_checksum: u32) {
        self.corruption_injections
            .insert(op.as_u64(), (original_checksum, true));
    }

    /// Checks that corrupted data was detected and rejected.
    pub fn check_corruption_detected(
        &mut self,
        op: OpNumber,
        was_rejected: bool,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_corruption_detection");
        self.checks_performed += 1;

        if let Some(&(original_checksum, corruption_injected)) =
            self.corruption_injections.get(&op.as_u64())
        {
            if corruption_injected && !was_rejected {
                return InvariantResult::Violated {
                    invariant: "vsr_corruption_detection".to_string(),
                    message: format!(
                        "Corrupted operation {} (checksum: {}) was accepted instead of rejected",
                        op.as_u64(),
                        original_checksum
                    ),
                    context: vec![
                        ("op".to_string(), op.as_u64().to_string()),
                        (
                            "original_checksum".to_string(),
                            original_checksum.to_string(),
                        ),
                    ],
                };
            }
        }

        InvariantResult::Ok
    }
}

impl Default for CorruptionDetectionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Quorum Validation Checker
// ============================================================================

/// Verifies that all quorum-based decisions have sufficient responses.
///
/// **Invariant**: Decisions requiring quorum must have at least f+1 matching responses.
///
/// **Violation**: A decision is made with fewer than quorum responses.
///
/// **Why it matters**: Quorum is the fundamental mechanism for Byzantine fault tolerance.
/// Violating quorum requirements can lead to unsafe decisions.
#[derive(Debug)]
pub struct QuorumValidationChecker {
    /// Cluster size (to calculate required quorum)
    cluster_size: usize,
    checks_performed: u64,
}

impl QuorumValidationChecker {
    pub fn new(cluster_size: usize) -> Self {
        Self {
            cluster_size,
            checks_performed: 0,
        }
    }

    /// Checks that a quorum decision has sufficient responses.
    pub fn check_quorum_decision(
        &mut self,
        decision_type: &str,
        response_count: usize,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_quorum_validation");
        self.checks_performed += 1;

        let required_quorum = (self.cluster_size / 2) + 1;

        if response_count < required_quorum {
            return InvariantResult::Violated {
                invariant: "vsr_quorum_validation".to_string(),
                message: format!(
                    "{} decision made with {} responses (required: {})",
                    decision_type, response_count, required_quorum
                ),
                context: vec![
                    ("decision_type".to_string(), decision_type.to_string()),
                    ("response_count".to_string(), response_count.to_string()),
                    ("required_quorum".to_string(), required_quorum.to_string()),
                    ("cluster_size".to_string(), self.cluster_size.to_string()),
                ],
            };
        }

        InvariantResult::Ok
    }
}

impl Default for QuorumValidationChecker {
    fn default() -> Self {
        Self::new(3) // Default: 3-node cluster
    }
}

// ============================================================================
// Message Ordering Checker
// ============================================================================

/// Detects protocol violations in message ordering.
///
/// **Invariant**: VSR messages must follow protocol ordering rules.
///
/// **Violation**: A replica sends messages in an invalid order.
///
/// **Why it matters**: Protocol violations can lead to undefined behavior,
/// deadlocks, or consensus failures.
#[derive(Debug)]
pub struct MessageOrderingChecker {
    /// Map from replica_id -> last message type sent
    last_message_by_replica: HashMap<u8, String>,
    checks_performed: u64,
}

impl MessageOrderingChecker {
    pub fn new() -> Self {
        Self {
            last_message_by_replica: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records a message send and checks for protocol violations.
    ///
    /// Example violations:
    /// - Sending PrepareOK before receiving Prepare
    /// - Sending StartView before collecting quorum of `DoViewChange`
    pub fn record_message(
        &mut self,
        replica_id: ReplicaId,
        message_type: &str,
        is_valid_transition: bool,
    ) -> InvariantResult {
        invariant_tracker::record_invariant_execution("vsr_message_ordering");
        self.checks_performed += 1;

        if !is_valid_transition {
            let prev_message = self
                .last_message_by_replica
                .get(&replica_id.as_u8())
                .map_or("(none)", std::string::String::as_str);

            return InvariantResult::Violated {
                invariant: "vsr_message_ordering".to_string(),
                message: format!(
                    "Replica {replica_id} sent invalid message sequence: {prev_message} -> {message_type}"
                ),
                context: vec![
                    ("replica".to_string(), replica_id.as_u8().to_string()),
                    ("previous_message".to_string(), prev_message.to_string()),
                    ("current_message".to_string(), message_type.to_string()),
                ],
            };
        }

        self.last_message_by_replica
            .insert(replica_id.as_u8(), message_type.to_string());
        InvariantResult::Ok
    }
}

impl Default for MessageOrderingChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Offset Monotonicity Checker
// ============================================================================

/// Verifies that offsets are monotonically increasing per stream.
///
/// This provides linearizability guarantees through natural log ordering,
/// following FoundationDB's version-based approach.
///
/// **Invariant**: For any stream, offsets must be monotonically increasing.
///
/// **Violation**: A stream's offset regresses to a lower value.
///
/// **Why it matters**: Offset monotonicity is the foundation of linearizability
/// in log-structured systems. The append-only log with monotonic offsets provides
/// a total ordering of operations, which is the basis for consensus correctness.
///
/// **Complexity**: O(1) per operation (HashMap lookup/insert)
///
/// ## Industry Approach
///
/// This checker follows the pattern used by FoundationDB, TigerBeetle, and Turso:
/// - **Natural ordering from log structure** (offsets provide total ordering)
/// - **Trust consensus algorithm** to provide linearizability internally
/// - **Verify monotonicity** rather than reconstructing linearizability post-hoc
/// - **O(1) complexity** rather than O(n!) history checking
#[derive(Debug)]
pub struct OffsetMonotonicityChecker {
    /// Map from stream_id -> highest_offset_seen
    stream_offsets: HashMap<u64, u64>,
    /// Total checks performed
    checks_performed: u64,
}

impl OffsetMonotonicityChecker {
    /// Creates a new offset monotonicity checker.
    pub fn new() -> Self {
        Self {
            stream_offsets: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records an operation at a given offset for a stream.
    ///
    /// Returns a violation if the offset regresses from a previously seen value.
    pub fn record_offset(&mut self, stream_id: u64, offset: u64) -> InvariantResult {
        invariant_tracker::record_invariant_execution("offset_monotonicity");
        self.checks_performed += 1;

        if let Some(&prev_offset) = self.stream_offsets.get(&stream_id) {
            if offset < prev_offset {
                return InvariantResult::Violated {
                    invariant: "offset_monotonicity".to_string(),
                    message: format!(
                        "Stream {} offset regressed from {} to {}",
                        stream_id, prev_offset, offset
                    ),
                    context: vec![
                        ("stream_id".to_string(), stream_id.to_string()),
                        ("previous_offset".to_string(), prev_offset.to_string()),
                        ("new_offset".to_string(), offset.to_string()),
                        (
                            "regression_amount".to_string(),
                            (prev_offset - offset).to_string(),
                        ),
                    ],
                };
            }
        }

        // Update to max of current and new offset (allows idempotent operations)
        self.stream_offsets.insert(
            stream_id,
            offset.max(*self.stream_offsets.get(&stream_id).unwrap_or(&0)),
        );

        InvariantResult::Ok
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }

    /// Returns the number of streams being tracked.
    pub fn stream_count(&self) -> usize {
        self.stream_offsets.len()
    }

    /// Returns the highest offset seen for a stream.
    pub fn get_offset(&self, stream_id: u64) -> Option<u64> {
        self.stream_offsets.get(&stream_id).copied()
    }

    /// Resets the checker state (for testing).
    pub fn reset(&mut self) {
        self.stream_offsets.clear();
        self.checks_performed = 0;
    }
}

impl Default for OffsetMonotonicityChecker {
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
    use kimberlite_vsr::{OpNumber, ReplicaId, ViewNumber};

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

        if let InvariantResult::Violated {
            invariant, message, ..
        } = result
        {
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

    // ========================================================================
    // Offset Monotonicity Checker Tests
    // ========================================================================

    #[test]
    fn offset_monotonicity_checker_basic() {
        let mut checker = OffsetMonotonicityChecker::new();

        // First offset for stream 1
        assert!(matches!(checker.record_offset(1, 0), InvariantResult::Ok));

        // Monotonically increasing offsets should succeed
        assert!(matches!(checker.record_offset(1, 1), InvariantResult::Ok));
        assert!(matches!(checker.record_offset(1, 2), InvariantResult::Ok));

        assert_eq!(checker.checks_performed(), 3);
        assert_eq!(checker.get_offset(1), Some(2));
    }

    #[test]
    fn offset_monotonicity_checker_detects_regression() {
        let mut checker = OffsetMonotonicityChecker::new();

        // Record offset 10
        checker.record_offset(1, 10);

        // Try to record offset 5 (regression!)
        let result = checker.record_offset(1, 5);
        assert!(matches!(result, InvariantResult::Violated { .. }));

        if let InvariantResult::Violated {
            invariant, message, ..
        } = result
        {
            assert_eq!(invariant, "offset_monotonicity");
            assert!(message.contains("regressed"));
            assert!(message.contains("10"));
            assert!(message.contains("5"));
        }
    }

    #[test]
    fn offset_monotonicity_checker_allows_same_offset() {
        let mut checker = OffsetMonotonicityChecker::new();

        // Record offset 5
        checker.record_offset(1, 5);

        // Recording same offset should be OK (idempotent)
        assert!(matches!(checker.record_offset(1, 5), InvariantResult::Ok));
    }

    #[test]
    fn offset_monotonicity_checker_multiple_streams() {
        let mut checker = OffsetMonotonicityChecker::new();

        // Different streams are independent
        assert!(checker.record_offset(1, 10).is_ok());
        assert!(checker.record_offset(2, 5).is_ok());
        assert!(checker.record_offset(3, 20).is_ok());

        assert_eq!(checker.stream_count(), 3);
        assert_eq!(checker.get_offset(1), Some(10));
        assert_eq!(checker.get_offset(2), Some(5));
        assert_eq!(checker.get_offset(3), Some(20));

        // Each stream can progress independently
        assert!(checker.record_offset(2, 6).is_ok());
        assert!(checker.record_offset(1, 11).is_ok());

        // But regression within a stream is still detected
        let result = checker.record_offset(3, 15);
        assert!(matches!(result, InvariantResult::Violated { .. }));
    }

    #[test]
    fn offset_monotonicity_checker_reset() {
        let mut checker = OffsetMonotonicityChecker::new();

        checker.record_offset(1, 10);
        assert_eq!(checker.stream_count(), 1);

        checker.reset();

        assert_eq!(checker.stream_count(), 0);
        assert_eq!(checker.checks_performed(), 0);
        assert_eq!(checker.get_offset(1), None);
    }

    #[test]
    fn offset_monotonicity_checker_tracks_execution() {
        use crate::instrumentation::invariant_tracker;

        invariant_tracker::reset_invariant_tracker();

        let mut checker = OffsetMonotonicityChecker::new();
        checker.record_offset(1, 0);
        checker.record_offset(1, 1);

        let tracker = invariant_tracker::get_invariant_tracker();
        assert_eq!(tracker.get_run_count("offset_monotonicity"), 2);
    }
}
