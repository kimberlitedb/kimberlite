///! # Projection & MVCC Invariants
///!
///! This module implements correctness invariants for state machine projection and
///! Multi-Version Concurrency Control (MVCC).
///!
///! ## Safety Properties
///!
///! 1. **AppliedPosition Monotonic**: applied_position never regresses and always ≤ commit_index
///! 2. **MVCC Visibility**: AS OF POSITION queries see correct snapshot at that position
///! 3. **AppliedIndex Integrity**: AppliedIndex references a real log entry with correct hash
///! 4. **Projection Catchup**: Projection eventually catches up to commit within bounded steps
///!
///! ## References
///!
///! - TigerBeetle: Deterministic state machine projection
///! - FoundationDB: MVCC and point-in-time queries
///! - Postgres: MVCC visibility rules

use std::collections::{HashMap, BTreeMap};
use kimberlite_types::Offset;
use kimberlite_crypto::ChainHash;

use crate::invariant::InvariantResult;
use crate::instrumentation::invariant_tracker;

// ============================================================================
// AppliedPosition Monotonic Invariant Checker
// ============================================================================

/// Verifies that applied_position never regresses and stays ≤ commit_index.
///
/// **Invariant**:
/// 1. applied_position is monotonically non-decreasing
/// 2. applied_position ≤ commit_index (projection can't be ahead of log)
///
/// **Violation Examples**:
/// - applied_position goes from 100 → 50 (regression)
/// - applied_position = 200 but commit_index = 150 (ahead of log)
///
/// **Why it matters**: Projection state must be consistent with the log. Regression
/// means data loss, being ahead means applying uncommitted data.
///
/// **Expected to catch**:
/// - Unsafe recovery that discards applied state
/// - Bugs in batch application that skip backwards
/// - Projection applying from wrong log position
#[derive(Debug)]
pub struct AppliedPositionMonotonicChecker {
    /// Map from replica_id (or projection_id) -> last seen applied_position
    last_applied: HashMap<String, u64>,

    /// Map from replica_id -> last seen commit_index
    last_commit: HashMap<String, u64>,

    /// Total checks performed
    checks_performed: u64,
}

impl AppliedPositionMonotonicChecker {
    /// Creates a new applied position monotonic checker.
    pub fn new() -> Self {
        Self {
            last_applied: HashMap::new(),
            last_commit: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records a new applied_position for a projection.
    ///
    /// Returns violation if:
    /// - applied_position regressed (went backwards)
    /// - applied_position > commit_index (ahead of log)
    pub fn record_applied_position(
        &mut self,
        projection_id: &str,
        applied_position: Offset,
        commit_index: Offset,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("projection_applied_position_monotonic");
        self.checks_performed += 1;

        let applied_u64 = u64::from(applied_position);
        let commit_u64 = u64::from(commit_index);

        // Check 1: applied_position ≤ commit_index
        if applied_u64 > commit_u64 {
            return InvariantResult::Violated {
                invariant: "projection_applied_position_monotonic".to_string(),
                message: format!(
                    "Projection {} applied_position ({}) > commit_index ({})",
                    projection_id, applied_u64, commit_u64
                ),
                context: vec![
                    ("projection_id".to_string(), projection_id.to_string()),
                    ("applied_position".to_string(), applied_u64.to_string()),
                    ("commit_index".to_string(), commit_u64.to_string()),
                ],
            };
        }

        // Check 2: applied_position is monotonic
        if let Some(&last_applied) = self.last_applied.get(projection_id) {
            if applied_u64 < last_applied {
                return InvariantResult::Violated {
                    invariant: "projection_applied_position_monotonic".to_string(),
                    message: format!(
                        "Projection {} applied_position regressed from {} to {}",
                        projection_id, last_applied, applied_u64
                    ),
                    context: vec![
                        ("projection_id".to_string(), projection_id.to_string()),
                        ("previous_applied".to_string(), last_applied.to_string()),
                        ("current_applied".to_string(), applied_u64.to_string()),
                        ("regressed_by".to_string(), (last_applied - applied_u64).to_string()),
                    ],
                };
            }
        }

        // Update tracking
        self.last_applied.insert(projection_id.to_string(), applied_u64);
        self.last_commit.insert(projection_id.to_string(), commit_u64);

        InvariantResult::Ok
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }

    /// Resets the checker state (for testing).
    pub fn reset(&mut self) {
        self.last_applied.clear();
        self.last_commit.clear();
        self.checks_performed = 0;
    }
}

impl Default for AppliedPositionMonotonicChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// MVCC Visibility Invariant Checker
// ============================================================================

/// Verifies that AS OF POSITION queries see correct snapshots.
///
/// **Invariant**: A query `AS OF POSITION p` must see exactly the state
/// as it existed after applying position p (not before, not after).
///
/// **Violation Example**:
/// - At position 5: key "x" = "value5"
/// - At position 10: key "x" = "value10"
/// - Query `AS OF POSITION 5` returns "value10" (wrong!)
///
/// **Why it matters**: MVCC visibility is critical for point-in-time consistency.
/// Wrong visibility breaks compliance guarantees, audit trails, and time-travel queries.
///
/// **Expected to catch**:
/// - Off-by-one errors in MVCC logic
/// - Wrong visibility predicate (created_at vs deleted_at)
/// - Clock/timestamp bugs
#[derive(Debug)]
pub struct MvccVisibilityChecker {
    /// Map from (table_id, key) -> BTreeMap<position, value_hash>
    /// Tracks the expected value at each position for verification
    version_history: HashMap<(String, String), BTreeMap<u64, Option<ChainHash>>>,

    /// Total checks performed
    checks_performed: u64,
}

impl MvccVisibilityChecker {
    /// Creates a new MVCC visibility checker.
    pub fn new() -> Self {
        Self {
            version_history: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records a write operation at a specific position.
    ///
    /// # Arguments
    /// - `table_id`: Table identifier
    /// - `key`: Row key
    /// - `position`: Log position where write occurred
    /// - `value_hash`: Hash of the new value (None for deletion)
    pub fn record_write(
        &mut self,
        table_id: &str,
        key: &str,
        position: Offset,
        value_hash: Option<ChainHash>,
    ) {
        let key_tuple = (table_id.to_string(), key.to_string());
        let history = self.version_history.entry(key_tuple).or_insert_with(BTreeMap::new);
        history.insert(u64::from(position), value_hash);
    }

    /// Verifies that a query result matches the expected value at the given position.
    ///
    /// Returns violation if the observed value doesn't match what should be visible
    /// at that position according to recorded write history.
    pub fn check_read_at_position(
        &mut self,
        table_id: &str,
        key: &str,
        position: Offset,
        observed_value_hash: Option<&ChainHash>,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("projection_mvcc_visibility");
        self.checks_performed += 1;

        let key_tuple = (table_id.to_string(), key.to_string());
        let position_u64 = u64::from(position);

        // Get version history for this key
        if let Some(history) = self.version_history.get(&key_tuple) {
            // Find the most recent write at or before the query position
            let expected = history
                .range(..=position_u64)
                .next_back()
                .map(|(_, hash)| hash);

            // Compare expected vs observed
            match (expected, observed_value_hash) {
                (Some(Some(expected_hash)), Some(observed_hash)) => {
                    if expected_hash != observed_hash {
                        return InvariantResult::Violated {
                            invariant: "projection_mvcc_visibility".to_string(),
                            message: format!(
                                "MVCC visibility violation: table={}, key={}, position={}, expected hash mismatch",
                                table_id, key, position_u64
                            ),
                            context: vec![
                                ("table_id".to_string(), table_id.to_string()),
                                ("key".to_string(), key.to_string()),
                                ("position".to_string(), position_u64.to_string()),
                                ("expected_hash".to_string(), format!("{:?}", expected_hash)),
                                ("observed_hash".to_string(), format!("{:?}", observed_hash)),
                            ],
                        };
                    }
                }
                (Some(None), None) => {
                    // Expected deleted, observed deleted - OK
                }
                (Some(None), Some(_)) => {
                    return InvariantResult::Violated {
                        invariant: "projection_mvcc_visibility".to_string(),
                        message: format!(
                            "MVCC visibility violation: key should be deleted at position {}",
                            position_u64
                        ),
                        context: vec![
                            ("table_id".to_string(), table_id.to_string()),
                            ("key".to_string(), key.to_string()),
                            ("position".to_string(), position_u64.to_string()),
                            ("expected".to_string(), "deleted".to_string()),
                            ("observed".to_string(), "exists".to_string()),
                        ],
                    };
                }
                (Some(Some(_)), None) => {
                    return InvariantResult::Violated {
                        invariant: "projection_mvcc_visibility".to_string(),
                        message: format!(
                            "MVCC visibility violation: key should exist at position {}",
                            position_u64
                        ),
                        context: vec![
                            ("table_id".to_string(), table_id.to_string()),
                            ("key".to_string(), key.to_string()),
                            ("position".to_string(), position_u64.to_string()),
                            ("expected".to_string(), "exists".to_string()),
                            ("observed".to_string(), "deleted".to_string()),
                        ],
                    };
                }
                (None, _) => {
                    // No history for this key - can't verify (not a violation)
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
        self.version_history.clear();
        self.checks_performed = 0;
    }
}

impl Default for MvccVisibilityChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// AppliedIndex Integrity Invariant Checker
// ============================================================================

/// Verifies that AppliedIndex references real log entries with correct hashes.
///
/// **Invariant**: When projection records "applied up to position P with hash H",
/// the log must actually have an entry at position P with hash H.
///
/// **Violation Example**:
/// - Projection claims applied_position=100, hash=ABC
/// - Log entry at position 100 has hash XYZ (mismatch!)
/// - Or: no log entry at position 100 (dangling reference)
///
/// **Why it matters**: AppliedIndex is used for crash recovery. If it references
/// a non-existent or wrong log entry, recovery will fail or produce corrupt state.
///
/// **Expected to catch**:
/// - Hash calculation bugs
/// - Superblock corruption
/// - Log/projection desynchronization
#[derive(Debug)]
pub struct AppliedIndexIntegrityChecker {
    /// Map from position -> expected log entry hash
    /// Populated as log entries are written
    log_entries: BTreeMap<u64, ChainHash>,

    /// Total checks performed
    checks_performed: u64,
}

impl AppliedIndexIntegrityChecker {
    /// Creates a new applied index integrity checker.
    pub fn new() -> Self {
        Self {
            log_entries: BTreeMap::new(),
            checks_performed: 0,
        }
    }

    /// Records a log entry at a specific position.
    pub fn record_log_entry(&mut self, position: Offset, entry_hash: &ChainHash) {
        self.log_entries.insert(u64::from(position), *entry_hash);
    }

    /// Verifies that the applied index reference is valid.
    ///
    /// Returns violation if:
    /// - No log entry exists at the claimed position
    /// - Log entry hash doesn't match the claimed hash
    pub fn check_applied_index(
        &mut self,
        applied_position: Offset,
        claimed_hash: &ChainHash,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("projection_applied_index_integrity");
        self.checks_performed += 1;

        let position_u64 = u64::from(applied_position);

        // Check if log entry exists
        match self.log_entries.get(&position_u64) {
            Some(actual_hash) => {
                if actual_hash != claimed_hash {
                    return InvariantResult::Violated {
                        invariant: "projection_applied_index_integrity".to_string(),
                        message: format!(
                            "AppliedIndex hash mismatch at position {}: expected {:?}, claimed {:?}",
                            position_u64, actual_hash, claimed_hash
                        ),
                        context: vec![
                            ("position".to_string(), position_u64.to_string()),
                            ("expected_hash".to_string(), format!("{:?}", actual_hash)),
                            ("claimed_hash".to_string(), format!("{:?}", claimed_hash)),
                        ],
                    };
                }
            }
            None => {
                return InvariantResult::Violated {
                    invariant: "projection_applied_index_integrity".to_string(),
                    message: format!(
                        "AppliedIndex references non-existent log entry at position {}",
                        position_u64
                    ),
                    context: vec![
                        ("position".to_string(), position_u64.to_string()),
                        ("claimed_hash".to_string(), format!("{:?}", claimed_hash)),
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
        self.log_entries.clear();
        self.checks_performed = 0;
    }
}

impl Default for AppliedIndexIntegrityChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Projection Catchup Invariant Checker
// ============================================================================

/// Verifies that projection catches up to commit_index within bounded steps.
///
/// **Invariant**: If projection is lagging (applied_position < commit_index),
/// it must catch up within a bounded number of simulation steps.
///
/// **Violation Example**:
/// - At step 1000: applied=50, commit=100 (lag of 50)
/// - At step 12000: applied=60, commit=150 (still lagging, 11k steps later!)
///
/// **Why it matters**: Unbounded lag means queries see stale data. Compliance
/// regulations often require "current" data, not arbitrarily old snapshots.
///
/// **Expected to catch**:
/// - Projection application bugs (infinite retry loop)
/// - Performance regressions
/// - Backpressure issues
///
/// **Note**: This uses deferred assertions via `assert_within_steps!` macro
/// to check catchup within a time budget.
#[derive(Debug)]
pub struct ProjectionCatchupChecker {
    /// Map from projection_id -> (lag_started_step, initial_applied, initial_commit)
    /// Tracks when lag was first detected
    lag_started: HashMap<String, (u64, u64, u64)>,

    /// Maximum allowed steps for catchup
    max_catchup_steps: u64,

    /// Total checks performed
    checks_performed: u64,
}

impl ProjectionCatchupChecker {
    /// Creates a new projection catchup checker.
    ///
    /// # Arguments
    /// - `max_catchup_steps`: Maximum simulation steps allowed for catchup (default: 10,000)
    pub fn new(max_catchup_steps: u64) -> Self {
        Self {
            lag_started: HashMap::new(),
            max_catchup_steps,
            checks_performed: 0,
        }
    }

    /// Records the current projection state at a simulation step.
    ///
    /// If projection is lagging, tracks when lag started. If lag persists beyond
    /// max_catchup_steps, returns a violation.
    pub fn check_catchup(
        &mut self,
        projection_id: &str,
        current_step: u64,
        applied_position: Offset,
        commit_index: Offset,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("projection_catchup");
        self.checks_performed += 1;

        let applied_u64 = u64::from(applied_position);
        let commit_u64 = u64::from(commit_index);

        let is_lagging = applied_u64 < commit_u64;

        if is_lagging {
            // Check if we're already tracking this lag
            if let Some(&(lag_start_step, initial_applied, initial_commit)) =
                self.lag_started.get(projection_id)
            {
                let steps_since_lag = current_step.saturating_sub(lag_start_step);

                if steps_since_lag > self.max_catchup_steps {
                    return InvariantResult::Violated {
                        invariant: "projection_catchup".to_string(),
                        message: format!(
                            "Projection {} failed to catch up within {} steps (lag: {} → {})",
                            projection_id,
                            self.max_catchup_steps,
                            initial_commit - initial_applied,
                            commit_u64 - applied_u64
                        ),
                        context: vec![
                            ("projection_id".to_string(), projection_id.to_string()),
                            ("lag_started_step".to_string(), lag_start_step.to_string()),
                            ("current_step".to_string(), current_step.to_string()),
                            ("steps_since_lag".to_string(), steps_since_lag.to_string()),
                            ("max_allowed_steps".to_string(), self.max_catchup_steps.to_string()),
                            ("initial_lag".to_string(), (initial_commit - initial_applied).to_string()),
                            ("current_lag".to_string(), (commit_u64 - applied_u64).to_string()),
                        ],
                    };
                }
            } else {
                // First time we've seen this projection lagging - start tracking
                self.lag_started
                    .insert(projection_id.to_string(), (current_step, applied_u64, commit_u64));
            }
        } else {
            // Caught up - clear any lag tracking
            self.lag_started.remove(projection_id);
        }

        InvariantResult::Ok
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }

    /// Resets the checker state (for testing).
    pub fn reset(&mut self) {
        self.lag_started.clear();
        self.checks_performed = 0;
    }
}

impl Default for ProjectionCatchupChecker {
    fn default() -> Self {
        Self::new(10_000) // Default: 10k steps
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a dummy hash
    fn dummy_hash(value: u8) -> ChainHash {
        let mut bytes = [0u8; 32];
        bytes[0] = value;
        ChainHash::from_bytes(&bytes)
    }

    #[test]
    fn test_applied_position_monotonic_ok() {
        let mut checker = AppliedPositionMonotonicChecker::new();

        // Normal progression
        assert!(matches!(
            checker.record_applied_position("proj1", Offset::new(0), Offset::new(10)),
            InvariantResult::Ok
        ));
        assert!(matches!(
            checker.record_applied_position("proj1", Offset::new(5), Offset::new(10)),
            InvariantResult::Ok
        ));
        assert!(matches!(
            checker.record_applied_position("proj1", Offset::new(10), Offset::new(10)),
            InvariantResult::Ok
        ));

        assert_eq!(checker.checks_performed(), 3);
    }

    #[test]
    fn test_applied_position_regression() {
        let mut checker = AppliedPositionMonotonicChecker::new();

        assert!(matches!(
            checker.record_applied_position("proj1", Offset::new(10), Offset::new(20)),
            InvariantResult::Ok
        ));

        // Regression - applied goes backwards
        let result = checker.record_applied_position("proj1", Offset::new(5), Offset::new(20));
        assert!(matches!(result, InvariantResult::Violated { .. }));
    }

    #[test]
    fn test_applied_position_ahead_of_commit() {
        let mut checker = AppliedPositionMonotonicChecker::new();

        // applied > commit - VIOLATION
        let result = checker.record_applied_position("proj1", Offset::new(20), Offset::new(10));
        assert!(matches!(result, InvariantResult::Violated { .. }));
    }

    #[test]
    fn test_mvcc_visibility_ok() {
        let mut checker = MvccVisibilityChecker::new();

        // Write at position 5
        let hash5 = dummy_hash(5);
        checker.record_write("table1", "key1", Offset::new(5), Some(hash5));

        // Write at position 10
        let hash10 = dummy_hash(10);
        checker.record_write("table1", "key1", Offset::new(10), Some(hash10));

        // Query at position 5 - should see hash5
        assert!(matches!(
            checker.check_read_at_position("table1", "key1", Offset::new(5), Some(&hash5)),
            InvariantResult::Ok
        ));

        // Query at position 10 - should see hash10
        assert!(matches!(
            checker.check_read_at_position("table1", "key1", Offset::new(10), Some(&hash10)),
            InvariantResult::Ok
        ));
    }

    #[test]
    fn test_mvcc_visibility_violation() {
        let mut checker = MvccVisibilityChecker::new();

        let hash5 = dummy_hash(5);
        let hash10 = dummy_hash(10);

        checker.record_write("table1", "key1", Offset::new(5), Some(hash5));
        checker.record_write("table1", "key1", Offset::new(10), Some(hash10));

        // Query at position 5 but see hash10 - VIOLATION
        let result = checker.check_read_at_position("table1", "key1", Offset::new(5), Some(&hash10));
        assert!(matches!(result, InvariantResult::Violated { .. }));
    }

    #[test]
    fn test_applied_index_integrity_ok() {
        let mut checker = AppliedIndexIntegrityChecker::new();

        let hash = dummy_hash(42);
        checker.record_log_entry(Offset::new(100), &hash);

        // Verify applied index points to correct entry
        assert!(matches!(
            checker.check_applied_index(Offset::new(100), &hash),
            InvariantResult::Ok
        ));
    }

    #[test]
    fn test_applied_index_integrity_hash_mismatch() {
        let mut checker = AppliedIndexIntegrityChecker::new();

        let actual_hash = dummy_hash(42);
        let wrong_hash = dummy_hash(99);

        checker.record_log_entry(Offset::new(100), &actual_hash);

        // Verify with wrong hash - VIOLATION
        let result = checker.check_applied_index(Offset::new(100), &wrong_hash);
        assert!(matches!(result, InvariantResult::Violated { .. }));
    }

    #[test]
    fn test_applied_index_integrity_missing_entry() {
        let mut checker = AppliedIndexIntegrityChecker::new();

        let hash = dummy_hash(42);

        // Try to verify entry that doesn't exist - VIOLATION
        let result = checker.check_applied_index(Offset::new(100), &hash);
        assert!(matches!(result, InvariantResult::Violated { .. }));
    }

    #[test]
    fn test_projection_catchup_ok() {
        let mut checker = ProjectionCatchupChecker::new(100);

        // Projection is lagging
        assert!(matches!(
            checker.check_catchup("proj1", 1000, Offset::new(10), Offset::new(20)),
            InvariantResult::Ok
        ));

        // Still lagging after 50 steps - OK (within limit)
        assert!(matches!(
            checker.check_catchup("proj1", 1050, Offset::new(15), Offset::new(25)),
            InvariantResult::Ok
        ));

        // Caught up - OK
        assert!(matches!(
            checker.check_catchup("proj1", 1060, Offset::new(25), Offset::new(25)),
            InvariantResult::Ok
        ));
    }

    #[test]
    fn test_projection_catchup_violation() {
        let mut checker = ProjectionCatchupChecker::new(100);

        // Projection starts lagging at step 1000
        assert!(matches!(
            checker.check_catchup("proj1", 1000, Offset::new(10), Offset::new(20)),
            InvariantResult::Ok
        ));

        // Still lagging after 101 steps - VIOLATION
        let result = checker.check_catchup("proj1", 1101, Offset::new(15), Offset::new(30));
        assert!(matches!(result, InvariantResult::Violated { .. }));
    }

    #[test]
    fn test_all_projection_checkers_track_execution() {
        use crate::instrumentation::invariant_tracker;

        invariant_tracker::reset_invariant_tracker();

        let mut monotonic = AppliedPositionMonotonicChecker::new();
        let mut mvcc = MvccVisibilityChecker::new();
        let mut applied_index = AppliedIndexIntegrityChecker::new();
        let mut catchup = ProjectionCatchupChecker::new(100);

        // Perform checks
        let _ = monotonic.record_applied_position("p1", Offset::new(5), Offset::new(10));
        mvcc.record_write("t1", "k1", Offset::new(5), Some(dummy_hash(5)));
        let _ = mvcc.check_read_at_position("t1", "k1", Offset::new(5), Some(&dummy_hash(5)));
        applied_index.record_log_entry(Offset::new(5), &dummy_hash(5));
        let _ = applied_index.check_applied_index(Offset::new(5), &dummy_hash(5));
        let _ = catchup.check_catchup("p1", 1000, Offset::new(5), Offset::new(10));

        // Verify tracking
        let tracker = invariant_tracker::get_invariant_tracker();
        assert_eq!(tracker.get_run_count("projection_applied_position_monotonic"), 1);
        assert_eq!(tracker.get_run_count("projection_mvcc_visibility"), 1);
        assert_eq!(tracker.get_run_count("projection_applied_index_integrity"), 1);
        assert_eq!(tracker.get_run_count("projection_catchup"), 1);
    }
}
