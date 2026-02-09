///! # SQL Metamorphic Testing Oracles
///!
///! This module implements SQLancer-inspired testing oracles for verifying SQL query
///! correctness through metamorphic testing.
///!
///! ## Testing Approaches
///!
///! 1. **TLP (Ternary Logic Partitioning)**: Split WHERE clause into partitions,
///!    verify union of results equals original query
///!
///! 2. **NoREC (Non-optimizing Reference Engine)**: Compare optimized query plans
///!    against unoptimized reference execution
///!
///! 3. **Query Plan Coverage**: Track unique query plans to guide state mutations
///!
///! 4. **Database State Mutators**: Generate actions (INSERT, UPDATE, DELETE) to
///!    increase coverage when stuck
///!
///! ## References
///!
///! - SQLancer: "Detecting Logic Bugs in DBMS" (Rigger & Su, 2020)
///! - "Testing Database Engines via Pivoted Query Synthesis" (Chen et al.)
///! - TigerBeetle: Deterministic query testing
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::instrumentation::invariant_tracker;
use crate::invariant::InvariantResult;
use kimberlite_oracle::{compare_results, OracleError, OracleRunner};

// ============================================================================
// TLP (Ternary Logic Partitioning) Oracle
// ============================================================================

/// Ternary Logic Partitioning oracle for detecting SQL logic bugs.
///
/// **Principle**: For a query `SELECT * FROM T WHERE C`, partition the condition
/// C into three disjoint sets:
/// - Rows where C is TRUE
/// - Rows where C is FALSE
/// - Rows where C is NULL
///
/// **Invariant**: `COUNT(TRUE) + COUNT(FALSE) + COUNT(NULL) = COUNT(ALL)`
///
/// **Violation Example**:
/// - Original query: `SELECT * FROM users WHERE age > 30` returns 100 rows
/// - Partitioned queries:
///   - `WHERE age > 30` returns 80 rows (TRUE partition)
///   - `WHERE NOT (age > 30)` returns 15 rows (FALSE partition)
///   - `WHERE (age > 30) IS NULL` returns 3 rows (NULL partition)
/// - Total: 80 + 15 + 3 = 98 ≠ 100 (VIOLATION!)
///
/// **Why it matters**: Catches incorrect WHERE clause evaluation, NULL handling
/// bugs, and boolean logic errors.
///
/// **Expected to catch**:
/// - Incorrect NULL handling in comparisons
/// - Boolean logic bugs (AND/OR precedence)
/// - Type coercion errors
#[derive(Debug)]
pub struct TlpOracle {
    /// Total queries verified
    queries_checked: u64,

    /// Violations detected
    violations_detected: u64,
}

impl TlpOracle {
    /// Creates a new TLP oracle.
    pub fn new() -> Self {
        Self {
            queries_checked: 0,
            violations_detected: 0,
        }
    }

    /// Verifies a query using Ternary Logic Partitioning.
    ///
    /// # Arguments
    /// - `original_count`: Result count from original query
    /// - `true_partition_count`: Result count from `WHERE condition`
    /// - `false_partition_count`: Result count from `WHERE NOT (condition)`
    /// - `null_partition_count`: Result count from `WHERE (condition) IS NULL`
    ///
    /// Returns violation if partition counts don't sum to original count.
    pub fn verify_partitioning(
        &mut self,
        query_id: &str,
        original_count: usize,
        true_partition_count: usize,
        false_partition_count: usize,
        null_partition_count: usize,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("sql_tlp_partitioning");
        self.queries_checked += 1;

        let partition_sum = true_partition_count + false_partition_count + null_partition_count;

        if partition_sum != original_count {
            self.violations_detected += 1;

            return InvariantResult::Violated {
                invariant: "sql_tlp_partitioning".to_string(),
                message: format!(
                    "TLP violation for query '{}': partitions sum to {} but original count is {}",
                    query_id, partition_sum, original_count
                ),
                context: vec![
                    ("query_id".to_string(), query_id.to_string()),
                    ("original_count".to_string(), original_count.to_string()),
                    (
                        "true_partition".to_string(),
                        true_partition_count.to_string(),
                    ),
                    (
                        "false_partition".to_string(),
                        false_partition_count.to_string(),
                    ),
                    (
                        "null_partition".to_string(),
                        null_partition_count.to_string(),
                    ),
                    ("partition_sum".to_string(), partition_sum.to_string()),
                    (
                        "discrepancy".to_string(),
                        (partition_sum as i64 - original_count as i64).to_string(),
                    ),
                ],
            };
        }

        InvariantResult::Ok
    }

    /// Returns the number of queries checked.
    pub fn queries_checked(&self) -> u64 {
        self.queries_checked
    }

    /// Returns the number of violations detected.
    pub fn violations_detected(&self) -> u64 {
        self.violations_detected
    }

    /// Resets the oracle state (for testing).
    pub fn reset(&mut self) {
        self.queries_checked = 0;
        self.violations_detected = 0;
    }
}

impl Default for TlpOracle {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// NoREC (Non-optimizing Reference Engine Comparison) Oracle
// ============================================================================

/// Non-optimizing Reference Engine Comparison oracle.
///
/// **Principle**: Execute the same query twice:
/// 1. With optimizations enabled (fast path)
/// 2. With optimizations disabled (slow but correct reference)
///
/// **Invariant**: Both executions must return identical results.
///
/// **Violation Example**:
/// - Optimized execution: Uses index scan, returns [row1, row2, row3]
/// - Unoptimized execution: Uses table scan, returns [row1, row2, row3, row4]
/// - Result mismatch → optimizer bug!
///
/// **Why it matters**: Catches optimizer bugs where optimizations change query
/// semantics (e.g., incorrect index usage, wrong join order).
///
/// **Expected to catch**:
/// - Index scan producing wrong results
/// - Predicate pushdown bugs
/// - Join reordering changing semantics
#[derive(Debug)]
pub struct NoRecOracle {
    /// Total comparisons performed
    comparisons_performed: u64,

    /// Violations detected
    violations_detected: u64,
}

impl NoRecOracle {
    /// Creates a new NoREC oracle.
    pub fn new() -> Self {
        Self {
            comparisons_performed: 0,
            violations_detected: 0,
        }
    }

    /// Compares optimized vs unoptimized query execution.
    ///
    /// # Arguments
    /// - `optimized_result_hash`: Hash of optimized query results (deterministic)
    /// - `unoptimized_result_hash`: Hash of unoptimized query results
    ///
    /// Returns violation if hashes don't match (results differ).
    pub fn verify_optimization(
        &mut self,
        query_id: &str,
        optimized_result_hash: u64,
        unoptimized_result_hash: u64,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("sql_norec_consistency");
        self.comparisons_performed += 1;

        if optimized_result_hash != unoptimized_result_hash {
            self.violations_detected += 1;

            return InvariantResult::Violated {
                invariant: "sql_norec_consistency".to_string(),
                message: format!(
                    "NoREC violation for query '{}': optimized and unoptimized results differ",
                    query_id
                ),
                context: vec![
                    ("query_id".to_string(), query_id.to_string()),
                    (
                        "optimized_hash".to_string(),
                        format!("{:x}", optimized_result_hash),
                    ),
                    (
                        "unoptimized_hash".to_string(),
                        format!("{:x}", unoptimized_result_hash),
                    ),
                ],
            };
        }

        InvariantResult::Ok
    }

    /// Returns the number of comparisons performed.
    pub fn comparisons_performed(&self) -> u64 {
        self.comparisons_performed
    }

    /// Returns the number of violations detected.
    pub fn violations_detected(&self) -> u64 {
        self.violations_detected
    }

    /// Resets the oracle state (for testing).
    pub fn reset(&mut self) {
        self.comparisons_performed = 0;
        self.violations_detected = 0;
    }
}

impl Default for NoRecOracle {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Query Plan Coverage Tracker
// ============================================================================

/// Tracks unique query plans to guide database state mutations.
///
/// **Principle**: Track which query plans have been executed. When coverage
/// plateaus (no new plans), mutate database state to trigger different plans.
///
/// **Example**:
/// - Initial state: 1 row → only PointLookup plans
/// - After INSERT: 100 rows → now see RangeScan plans
/// - After CREATE INDEX: → now see IndexScan plans
///
/// **Why it matters**: Ensures testing exercises diverse execution paths,
/// not just one query plan shape.
///
/// **Guides**:
/// - INSERT/DELETE to change row counts (affects plan selection)
/// - CREATE INDEX to enable index scans
/// - UPDATE to create NULL values, trigger different predicates
#[derive(Debug)]
pub struct QueryPlanCoverageTracker {
    /// Set of unique plan signatures seen
    /// Signature = hash of (plan_type, table, index, has_filter, has_limit, ...)
    unique_plans: HashSet<u64>,

    /// Total queries executed
    queries_executed: u64,

    /// Steps since last new plan discovered
    steps_since_new_plan: u64,

    /// Threshold for "coverage plateau" (trigger mutation)
    plateau_threshold: u64,
}

impl QueryPlanCoverageTracker {
    /// Creates a new query plan coverage tracker.
    ///
    /// # Arguments
    /// - `plateau_threshold`: Number of queries without new plan before triggering mutation
    pub fn new(plateau_threshold: u64) -> Self {
        Self {
            unique_plans: HashSet::new(),
            queries_executed: 0,
            steps_since_new_plan: 0,
            plateau_threshold,
        }
    }

    /// Records a query plan execution.
    ///
    /// Returns true if this is a new plan (coverage increased).
    pub fn record_plan(&mut self, plan_signature: u64) -> bool {
        self.queries_executed += 1;

        if self.unique_plans.insert(plan_signature) {
            // New plan discovered
            self.steps_since_new_plan = 0;
            true
        } else {
            // Duplicate plan
            self.steps_since_new_plan += 1;
            false
        }
    }

    /// Checks if coverage has plateaued (no new plans in a while).
    ///
    /// When true, caller should mutate database state to trigger new plans.
    pub fn is_coverage_plateaued(&self) -> bool {
        self.steps_since_new_plan >= self.plateau_threshold
    }

    /// Returns the number of unique plans seen.
    pub fn unique_plan_count(&self) -> usize {
        self.unique_plans.len()
    }

    /// Returns the total number of queries executed.
    pub fn queries_executed(&self) -> u64 {
        self.queries_executed
    }

    /// Resets coverage tracking (after database mutation).
    pub fn reset_plateau_counter(&mut self) {
        self.steps_since_new_plan = 0;
    }

    /// Resets the tracker state (for testing).
    pub fn reset(&mut self) {
        self.unique_plans.clear();
        self.queries_executed = 0;
        self.steps_since_new_plan = 0;
    }
}

impl Default for QueryPlanCoverageTracker {
    fn default() -> Self {
        Self::new(100) // Default: plateau after 100 queries without new plan
    }
}

// ============================================================================
// Query Plan Signature Computation
// ============================================================================

/// Computes a signature (hash) for a query plan to track coverage.
///
/// **Signature includes**:
/// - Plan type (PointLookup, RangeScan, IndexScan, TableScan, Aggregate)
/// - Table accessed
/// - Index used (if any)
/// - Has filter? Has limit? Has ORDER BY?
/// - Aggregate function (if any)
///
/// **Example**:
/// - `SELECT * FROM users WHERE id = 5` → PointLookup signature
/// - `SELECT * FROM users WHERE age > 30 LIMIT 10` → RangeScan + filter + limit signature
/// - `SELECT COUNT(*) FROM users` → Aggregate signature
pub fn compute_plan_signature(
    plan_type: &str,
    table_name: &str,
    index_name: Option<&str>,
    has_filter: bool,
    has_limit: bool,
    has_order_by: bool,
    aggregate_function: Option<&str>,
) -> u64 {
    let mut hasher = DefaultHasher::new();

    plan_type.hash(&mut hasher);
    table_name.hash(&mut hasher);
    index_name.hash(&mut hasher);
    has_filter.hash(&mut hasher);
    has_limit.hash(&mut hasher);
    has_order_by.hash(&mut hasher);
    aggregate_function.hash(&mut hasher);

    hasher.finish()
}

// ============================================================================
// Database State Mutators
// ============================================================================

/// Actions that can be performed to mutate database state.
///
/// Inspired by SQLancer's `Action` enum, these mutations increase coverage
/// by changing row counts, creating indexes, introducing NULLs, etc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseAction {
    /// Insert a new row with random values.
    InsertRandomRow { table: String },

    /// Insert a row with specific values (e.g., NULL, extremes).
    InsertSpecialRow {
        table: String,
        value_type: SpecialValueType,
    },

    /// Update random rows to introduce variety.
    UpdateRandomRows { table: String, count: usize },

    /// Delete random rows to change cardinality.
    DeleteRandomRows { table: String, count: usize },

    /// Create a secondary index to enable IndexScan plans.
    CreateIndex { table: String, column: String },

    /// Drop an index to force TableScan plans.
    DropIndex { table: String, index: String },

    /// Analyze table statistics (affects planner).
    AnalyzeTable { table: String },
}

/// Special value types for targeted testing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecialValueType {
    /// All NULLs (test NULL handling).
    AllNulls,

    /// Minimum values for each type.
    MinValues,

    /// Maximum values for each type.
    MaxValues,

    /// Empty strings / zero values.
    Empty,

    /// Duplicate of an existing row (test uniqueness).
    Duplicate,
}

/// Selects the next database action based on coverage plateau.
///
/// **Strategy**:
/// 1. If coverage is not plateaued, continue querying (no action).
/// 2. If plateaued with low row count, INSERT rows.
/// 3. If plateaued with many rows, CREATE INDEX or UPDATE to create variety.
/// 4. If plateaued with indexes, try DELETE or DROP INDEX.
///
/// This is a simple heuristic; more sophisticated strategies could use
/// reinforcement learning or genetic algorithms.
pub fn select_next_action(
    unique_plan_count: usize,
    row_count: usize,
    index_count: usize,
    queries_without_new_plan: u64,
) -> Option<DatabaseAction> {
    // Not plateaued - keep querying
    if queries_without_new_plan < 100 {
        return None;
    }

    // Plateaued - suggest a mutation
    if row_count < 10 {
        // Low row count - insert more
        Some(DatabaseAction::InsertRandomRow {
            table: "test_table".to_string(),
        })
    } else if index_count == 0 && unique_plan_count < 5 {
        // No indexes - create one to enable IndexScan
        Some(DatabaseAction::CreateIndex {
            table: "test_table".to_string(),
            column: "col1".to_string(),
        })
    } else if row_count > 100 {
        // Many rows - delete some to test different cardinalities
        Some(DatabaseAction::DeleteRandomRows {
            table: "test_table".to_string(),
            count: row_count / 10,
        })
    } else {
        // Default - update to create variety
        Some(DatabaseAction::UpdateRandomRows {
            table: "test_table".to_string(),
            count: 5,
        })
    }
}

// ============================================================================
// Differential Testing Oracle
// ============================================================================

/// Differential testing oracle that compares Kimberlite vs DuckDB results.
///
/// **Principle**: Execute the same query in both engines and compare results
/// byte-by-byte. Any discrepancy indicates a bug in Kimberlite's SQL engine.
///
/// **Violation Example**:
/// - Query: `SELECT COUNT(*) FROM users WHERE age > 30`
/// - DuckDB returns: 100
/// - Kimberlite returns: 98
/// - Result mismatch → Kimberlite SQL bug!
///
/// **Why it matters**: SQLancer found **154+ bugs** in production databases
/// using this technique. It catches optimizer bugs, incorrect NULL handling,
/// and semantic errors that internal oracles (TLP/NoREC) can't detect.
///
/// **Expected to catch**:
/// - Optimizer producing wrong results
/// - Aggregate function bugs (COUNT, SUM, AVG)
/// - JOIN semantic errors
/// - NULL handling inconsistencies
/// - Type coercion bugs
#[derive(Debug)]
pub struct DifferentialTester<R: OracleRunner, S: OracleRunner> {
    /// Reference oracle (ground truth)
    reference: R,

    /// System under test
    sut: S,

    /// Total queries checked
    queries_checked: u64,

    /// Violations detected
    violations_detected: u64,

    /// Last violation (for debugging)
    last_violation: Option<String>,
}

impl<R: OracleRunner, S: OracleRunner> DifferentialTester<R, S> {
    /// Creates a new differential tester.
    ///
    /// # Arguments
    /// - `reference`: The reference oracle (e.g., DuckDB)
    /// - `sut`: The system under test (e.g., Kimberlite)
    pub fn new(reference: R, sut: S) -> Self {
        Self {
            reference,
            sut,
            queries_checked: 0,
            violations_detected: 0,
            last_violation: None,
        }
    }

    /// Executes a query in both oracles and compares results.
    ///
    /// # Arguments
    /// - `query_id`: Identifier for this query (for debugging)
    /// - `sql`: The SQL query to execute
    ///
    /// Returns violation if results differ.
    pub fn verify_query(&mut self, query_id: &str, sql: &str) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("sql_differential_consistency");
        self.queries_checked += 1;

        // Execute in reference oracle
        let reference_result = match self.reference.execute(sql) {
            Ok(result) => result,
            Err(OracleError::SyntaxError(_)) | Err(OracleError::SemanticError(_)) => {
                // Both oracles should reject invalid queries
                // Verify that SUT also rejects it
                match self.sut.execute(sql) {
                    Err(OracleError::SyntaxError(_)) | Err(OracleError::SemanticError(_)) => {
                        // Both rejected - OK
                        return InvariantResult::Ok;
                    }
                    Ok(_) => {
                        // Reference rejected but SUT accepted - VIOLATION!
                        self.violations_detected += 1;
                        let msg = format!(
                            "Differential violation for query '{}': reference rejected but SUT accepted",
                            query_id
                        );
                        self.last_violation = Some(msg.clone());
                        return InvariantResult::Violated {
                            invariant: "sql_differential_consistency".to_string(),
                            message: msg,
                            context: vec![
                                ("query_id".to_string(), query_id.to_string()),
                                ("sql".to_string(), sql.to_string()),
                                ("reference".to_string(), self.reference.name().to_string()),
                                ("sut".to_string(), self.sut.name().to_string()),
                            ],
                        };
                    }
                    Err(_e) => {
                        // Both rejected but with different error types - OK (error message differences are acceptable)
                        return InvariantResult::Ok;
                    }
                }
            }
            Err(OracleError::Unsupported(_)) => {
                // Reference doesn't support this query - skip
                return InvariantResult::Ok;
            }
            Err(e) => {
                // Unexpected error in reference oracle
                self.violations_detected += 1;
                let msg = format!(
                    "Reference oracle error for query '{}': {}",
                    query_id, e
                );
                self.last_violation = Some(msg.clone());
                return InvariantResult::Violated {
                    invariant: "sql_differential_consistency".to_string(),
                    message: msg,
                    context: vec![
                        ("query_id".to_string(), query_id.to_string()),
                        ("sql".to_string(), sql.to_string()),
                        ("error".to_string(), e.to_string()),
                    ],
                };
            }
        };

        // Execute in SUT
        let sut_result = match self.sut.execute(sql) {
            Ok(result) => result,
            Err(OracleError::Unsupported(_)) => {
                // SUT doesn't support this query - skip
                return InvariantResult::Ok;
            }
            Err(e) => {
                // SUT error but reference succeeded - VIOLATION!
                self.violations_detected += 1;
                let msg = format!(
                    "SUT error for query '{}': {} (reference succeeded)",
                    query_id, e
                );
                self.last_violation = Some(msg.clone());
                return InvariantResult::Violated {
                    invariant: "sql_differential_consistency".to_string(),
                    message: msg,
                    context: vec![
                        ("query_id".to_string(), query_id.to_string()),
                        ("sql".to_string(), sql.to_string()),
                        ("error".to_string(), e.to_string()),
                        ("reference_rows".to_string(), reference_result.len().to_string()),
                    ],
                };
            }
        };

        // Compare results
        match compare_results(
            &reference_result,
            &sut_result,
            self.reference.name(),
            self.sut.name(),
        ) {
            Ok(()) => InvariantResult::Ok,
            Err(mismatch) => {
                self.violations_detected += 1;
                let msg = format!("Differential violation for query '{}': {}", query_id, mismatch);
                self.last_violation = Some(msg.clone());

                InvariantResult::Violated {
                    invariant: "sql_differential_consistency".to_string(),
                    message: msg,
                    context: vec![
                        ("query_id".to_string(), query_id.to_string()),
                        ("sql".to_string(), sql.to_string()),
                        ("reference".to_string(), self.reference.name().to_string()),
                        ("sut".to_string(), self.sut.name().to_string()),
                        (
                            "reference_rows".to_string(),
                            reference_result.len().to_string(),
                        ),
                        ("sut_rows".to_string(), sut_result.len().to_string()),
                        ("mismatch".to_string(), mismatch.to_string()),
                    ],
                }
            }
        }
    }

    /// Returns the number of queries checked.
    pub fn queries_checked(&self) -> u64 {
        self.queries_checked
    }

    /// Returns the number of violations detected.
    pub fn violations_detected(&self) -> u64 {
        self.violations_detected
    }

    /// Returns the last violation message (for debugging).
    pub fn last_violation(&self) -> Option<&str> {
        self.last_violation.as_deref()
    }

    /// Resets the tester state (for testing).
    pub fn reset(&mut self) {
        self.queries_checked = 0;
        self.violations_detected = 0;
        self.last_violation = None;
    }

    /// Resets both oracles to initial state.
    pub fn reset_oracles(&mut self) -> Result<(), String> {
        self.reference
            .reset()
            .map_err(|e| format!("Failed to reset reference oracle: {}", e))?;
        self.sut
            .reset()
            .map_err(|e| format!("Failed to reset SUT oracle: {}", e))?;
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tlp_oracle_ok() {
        let mut oracle = TlpOracle::new();

        // Original query returns 100 rows
        // Partitions: 60 TRUE, 35 FALSE, 5 NULL = 100 total
        let result = oracle.verify_partitioning("query1", 100, 60, 35, 5);
        assert!(matches!(result, InvariantResult::Ok));

        assert_eq!(oracle.queries_checked(), 1);
        assert_eq!(oracle.violations_detected(), 0);
    }

    #[test]
    fn test_tlp_oracle_violation() {
        let mut oracle = TlpOracle::new();

        // Original query returns 100 rows
        // Partitions: 60 TRUE, 35 FALSE, 3 NULL = 98 total (VIOLATION!)
        let result = oracle.verify_partitioning("query1", 100, 60, 35, 3);
        assert!(matches!(result, InvariantResult::Violated { .. }));

        assert_eq!(oracle.queries_checked(), 1);
        assert_eq!(oracle.violations_detected(), 1);
    }

    #[test]
    fn test_norec_oracle_ok() {
        let mut oracle = NoRecOracle::new();

        // Optimized and unoptimized produce same results
        let hash = 0x1234_5678_90ab_cdef;
        let result = oracle.verify_optimization("query1", hash, hash);
        assert!(matches!(result, InvariantResult::Ok));

        assert_eq!(oracle.comparisons_performed(), 1);
        assert_eq!(oracle.violations_detected(), 0);
    }

    #[test]
    fn test_norec_oracle_violation() {
        let mut oracle = NoRecOracle::new();

        // Optimized and unoptimized produce different results
        let optimized_hash = 0x1234567890abcdef;
        let unoptimized_hash = 0xfedcba0987654321;
        let result = oracle.verify_optimization("query1", optimized_hash, unoptimized_hash);
        assert!(matches!(result, InvariantResult::Violated { .. }));

        assert_eq!(oracle.comparisons_performed(), 1);
        assert_eq!(oracle.violations_detected(), 1);
    }

    #[test]
    fn test_query_plan_coverage_tracker() {
        let mut tracker = QueryPlanCoverageTracker::new(10);

        // First plan - new
        assert!(tracker.record_plan(1));
        assert_eq!(tracker.unique_plan_count(), 1);
        assert!(!tracker.is_coverage_plateaued());

        // Second plan - new
        assert!(tracker.record_plan(2));
        assert_eq!(tracker.unique_plan_count(), 2);

        // Duplicate plan - not new
        assert!(!tracker.record_plan(1));
        assert_eq!(tracker.unique_plan_count(), 2);

        // Keep executing duplicate plans (need 9 more to reach threshold of 10)
        for _ in 0..9 {
            assert!(!tracker.record_plan(1));
        }

        // After 10 steps without new plan, should be plateaued
        assert!(tracker.is_coverage_plateaued());
    }

    #[test]
    fn test_compute_plan_signature() {
        // Different plan types produce different signatures
        let sig1 = compute_plan_signature("PointLookup", "users", None, false, false, false, None);
        let sig2 = compute_plan_signature("RangeScan", "users", None, false, false, false, None);
        assert_ne!(sig1, sig2);

        // Same plan produces same signature
        let sig3 = compute_plan_signature("PointLookup", "users", None, false, false, false, None);
        assert_eq!(sig1, sig3);

        // Different tables produce different signatures
        let sig4 = compute_plan_signature("PointLookup", "posts", None, false, false, false, None);
        assert_ne!(sig1, sig4);

        // Index scan produces different signature
        let sig5 = compute_plan_signature(
            "IndexScan",
            "users",
            Some("age_idx"),
            false,
            false,
            false,
            None,
        );
        assert_ne!(sig1, sig5);
    }

    #[test]
    fn test_select_next_action_low_rows() {
        // Low row count → insert rows
        let action = select_next_action(2, 5, 0, 100);
        assert!(matches!(
            action,
            Some(DatabaseAction::InsertRandomRow { .. })
        ));
    }

    #[test]
    fn test_select_next_action_no_index() {
        // Many rows, no index → create index
        let action = select_next_action(3, 50, 0, 100);
        assert!(matches!(action, Some(DatabaseAction::CreateIndex { .. })));
    }

    #[test]
    fn test_select_next_action_many_rows() {
        // Many rows, has index → delete rows
        let action = select_next_action(5, 200, 1, 100);
        assert!(matches!(
            action,
            Some(DatabaseAction::DeleteRandomRows { .. })
        ));
    }

    #[test]
    fn test_select_next_action_not_plateaued() {
        // Not plateaued → no action
        let action = select_next_action(5, 50, 1, 50);
        assert!(action.is_none());
    }

    #[test]
    fn test_all_sql_oracles_track_execution() {
        use crate::instrumentation::invariant_tracker;

        invariant_tracker::reset_invariant_tracker();

        let mut tlp = TlpOracle::new();
        let mut norec = NoRecOracle::new();

        // Perform checks
        let _ = tlp.verify_partitioning("q1", 100, 60, 35, 5);
        let _ = norec.verify_optimization("q2", 123, 123);

        // Verify tracking
        let tracker = invariant_tracker::get_invariant_tracker();
        assert_eq!(tracker.get_run_count("sql_tlp_partitioning"), 1);
        assert_eq!(tracker.get_run_count("sql_norec_consistency"), 1);
    }

    // ========================================================================
    // Differential Testing Oracle Tests
    // ========================================================================

    // Mock oracle that always returns a pre-configured result
    struct MockOracle {
        name: &'static str,
        // Store the result as a serializable representation
        rows: Vec<Vec<Value>>,
        columns: Vec<ColumnName>,
        should_error: Option<OracleError>,
    }

    impl MockOracle {
        fn with_result(
            name: &'static str,
            result: Result<kimberlite_query::QueryResult, OracleError>,
        ) -> Self {
            match result {
                Ok(qr) => Self {
                    name,
                    rows: qr.rows,
                    columns: qr.columns,
                    should_error: None,
                },
                Err(e) => Self {
                    name,
                    rows: vec![],
                    columns: vec![],
                    should_error: Some(e),
                },
            }
        }
    }

    impl OracleRunner for MockOracle {
        fn execute(
            &mut self,
            _sql: &str,
        ) -> Result<kimberlite_query::QueryResult, OracleError> {
            if let Some(ref err) = self.should_error {
                return Err(match err {
                    OracleError::SyntaxError(msg) => OracleError::SyntaxError(msg.clone()),
                    OracleError::SemanticError(msg) => OracleError::SemanticError(msg.clone()),
                    OracleError::RuntimeError(msg) => OracleError::RuntimeError(msg.clone()),
                    OracleError::Timeout(t) => OracleError::Timeout(*t),
                    OracleError::Unsupported(msg) => OracleError::Unsupported(msg.clone()),
                    OracleError::Internal(msg) => OracleError::Internal(msg.clone()),
                });
            }

            Ok(KmbQueryResult {
                columns: self.columns.clone(),
                rows: self.rows.clone(),
            })
        }

        fn reset(&mut self) -> Result<(), OracleError> {
            Ok(())
        }

        fn name(&self) -> &'static str {
            self.name
        }
    }

    use kimberlite_query::{ColumnName, QueryResult as KmbQueryResult, Value};

    #[test]
    fn test_differential_tester_identical_results() {
        let result = KmbQueryResult {
            columns: vec![ColumnName::from("count")],
            rows: vec![vec![Value::BigInt(100)]],
        };

        let reference = MockOracle::with_result("Reference", Ok(result.clone()));
        let sut = MockOracle::with_result("SUT", Ok(result));

        let mut tester = DifferentialTester::new(reference, sut);
        let check_result = tester.verify_query("q1", "SELECT COUNT(*) FROM users");

        assert!(matches!(check_result, InvariantResult::Ok));
        assert_eq!(tester.queries_checked(), 1);
        assert_eq!(tester.violations_detected(), 0);
    }

    #[test]
    fn test_differential_tester_row_count_mismatch() {
        let reference_result = KmbQueryResult {
            columns: vec![ColumnName::from("count")],
            rows: vec![vec![Value::BigInt(100)]],
        };

        let sut_result = KmbQueryResult {
            columns: vec![ColumnName::from("count")],
            rows: vec![vec![Value::BigInt(98)]],
        };

        let reference = MockOracle::with_result("Reference", Ok(reference_result));
        let sut = MockOracle::with_result("SUT", Ok(sut_result));

        let mut tester = DifferentialTester::new(reference, sut);
        let check_result = tester.verify_query("q1", "SELECT COUNT(*) FROM users");

        assert!(matches!(check_result, InvariantResult::Violated { .. }));
        assert_eq!(tester.queries_checked(), 1);
        assert_eq!(tester.violations_detected(), 1);
        assert!(tester.last_violation().is_some());
    }

    #[test]
    fn test_differential_tester_sut_error() {
        let reference_result = KmbQueryResult {
            columns: vec![ColumnName::from("count")],
            rows: vec![vec![Value::BigInt(100)]],
        };

        let reference = MockOracle::with_result("Reference", Ok(reference_result));
        let sut = MockOracle::with_result(
            "SUT",
            Err(OracleError::RuntimeError("table not found".to_string())),
        );

        let mut tester = DifferentialTester::new(reference, sut);
        let check_result = tester.verify_query("q1", "SELECT COUNT(*) FROM users");

        assert!(matches!(check_result, InvariantResult::Violated { .. }));
        assert_eq!(tester.violations_detected(), 1);
    }

    #[test]
    fn test_differential_tester_both_reject() {
        let reference =
            MockOracle::with_result("Reference", Err(OracleError::SyntaxError("bad SQL".to_string())));
        let sut = MockOracle::with_result("SUT", Err(OracleError::SyntaxError("bad SQL".to_string())));

        let mut tester = DifferentialTester::new(reference, sut);
        let check_result = tester.verify_query("q1", "SELECT INVALID SYNTAX");

        // Both rejected - should be OK
        assert!(matches!(check_result, InvariantResult::Ok));
        assert_eq!(tester.violations_detected(), 0);
    }

    #[test]
    fn test_differential_tester_reset() {
        let result = KmbQueryResult {
            columns: vec![ColumnName::from("count")],
            rows: vec![vec![Value::BigInt(100)]],
        };

        let reference = MockOracle::with_result("Reference", Ok(result.clone()));
        let sut = MockOracle::with_result("SUT", Ok(result));

        let mut tester = DifferentialTester::new(reference, sut);
        let _ = tester.verify_query("q1", "SELECT COUNT(*) FROM users");

        assert_eq!(tester.queries_checked(), 1);

        tester.reset();

        assert_eq!(tester.queries_checked(), 0);
        assert_eq!(tester.violations_detected(), 0);
        assert!(tester.last_violation().is_none());
    }
}
