//! Query engine invariant checkers for simulation testing.
//!
//! These checkers verify correctness properties specific to the query engine,
//! such as determinism, type safety, and semantic correctness.

use std::collections::HashMap;

use crate::invariant::{InvariantChecker, InvariantResult};

// ============================================================================
// Query-Specific Types
// ============================================================================

/// A query execution with parameters and result.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueryExecution {
    /// The SQL query string.
    pub sql: String,
    /// Query parameters (serialized to string for hashing).
    pub params: Vec<String>,
    /// The result (serialized to string for comparison).
    pub result: String,
}

/// A data modification operation (INSERT/UPDATE/DELETE).
#[derive(Debug, Clone)]
pub struct DataModification {
    /// Table affected.
    pub table_name: String,
    /// Key affected (serialized).
    pub key: String,
    /// Value written (serialized), None for DELETE.
    pub value: Option<String>,
    /// Timestamp of modification.
    pub time_ns: u64,
}

// ============================================================================
// Query Determinism Checker
// ============================================================================

/// Verifies that query results are deterministic.
///
/// Same query + same parameters + same database state should always
/// produce identical results. This catches non-deterministic bugs in:
/// - Random number generation in queries
/// - Uninitialized memory reads
/// - Timestamp/clock dependencies
/// - Ordering issues without ORDER BY
#[derive(Debug)]
pub struct QueryDeterminismChecker {
    /// Cache of query executions: (sql, params) -> result
    query_cache: HashMap<(String, Vec<String>), String>,
    /// Number of queries checked.
    queries_checked: u64,
}

impl QueryDeterminismChecker {
    /// Creates a new query determinism checker.
    pub fn new() -> Self {
        Self {
            query_cache: HashMap::new(),
            queries_checked: 0,
        }
    }

    /// Records a query execution and checks for determinism violation.
    pub fn check_query(&mut self, sql: &str, params: &[String], result: &str) -> InvariantResult {
        let key = (sql.to_string(), params.to_vec());

        if let Some(cached_result) = self.query_cache.get(&key) {
            // Same query+params executed before - result must match
            if cached_result != result {
                return InvariantResult::Violated {
                    invariant: "query_determinism".to_string(),
                    message: format!("query returned different results for same inputs: {}", sql),
                    context: vec![
                        ("sql".to_string(), sql.to_string()),
                        ("params".to_string(), format!("{:?}", params)),
                        ("first_result".to_string(), cached_result.clone()),
                        ("second_result".to_string(), result.to_string()),
                    ],
                };
            }
        } else {
            // First time seeing this query+params
            self.query_cache.insert(key, result.to_string());
        }

        self.queries_checked += 1;
        InvariantResult::Ok
    }

    /// Returns the number of queries checked.
    pub fn queries_checked(&self) -> u64 {
        self.queries_checked
    }

    /// Returns the number of unique query+param combinations seen.
    pub fn unique_queries(&self) -> usize {
        self.query_cache.len()
    }
}

impl Default for QueryDeterminismChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for QueryDeterminismChecker {
    fn name(&self) -> &'static str {
        "QueryDeterminismChecker"
    }

    fn reset(&mut self) {
        self.query_cache.clear();
        self.queries_checked = 0;
    }
}

// ============================================================================
// Read-Your-Writes Checker
// ============================================================================

/// Verifies that writes are visible to subsequent reads.
///
/// This ensures the fundamental consistency property:
/// - If a client writes a value
/// - Then reads the same key
/// - The read must see the written value
///
/// Violations indicate:
/// - Incorrect transaction isolation
/// - Stale read bugs
/// - Cache coherency issues
#[derive(Debug)]
pub struct ReadYourWritesChecker {
    /// Pending writes: table -> key -> value
    pending_writes: HashMap<String, HashMap<String, Option<String>>>,
    /// Number of writes tracked.
    writes_tracked: u64,
    /// Number of reads verified.
    reads_verified: u64,
}

impl ReadYourWritesChecker {
    /// Creates a new read-your-writes checker.
    pub fn new() -> Self {
        Self {
            pending_writes: HashMap::new(),
            writes_tracked: 0,
            reads_verified: 0,
        }
    }

    /// Records a write operation.
    pub fn record_write(&mut self, table: &str, key: &str, value: Option<&str>) {
        self.pending_writes
            .entry(table.to_string())
            .or_default()
            .insert(key.to_string(), value.map(String::from));
        self.writes_tracked += 1;
    }

    /// Verifies a read operation sees all pending writes.
    ///
    /// Returns a violation if the read doesn't see a pending write.
    pub fn verify_read(
        &mut self,
        table: &str,
        key: &str,
        observed_value: Option<&str>,
    ) -> InvariantResult {
        if let Some(table_writes) = self.pending_writes.get(table) {
            if let Some(expected_value) = table_writes.get(key) {
                // There's a pending write for this key
                let expected = expected_value.as_deref();
                if expected != observed_value {
                    return InvariantResult::Violated {
                        invariant: "read_your_writes".to_string(),
                        message: format!(
                            "read did not see pending write: table={}, key={}",
                            table, key
                        ),
                        context: vec![
                            ("table".to_string(), table.to_string()),
                            ("key".to_string(), key.to_string()),
                            ("expected_value".to_string(), format!("{:?}", expected)),
                            (
                                "observed_value".to_string(),
                                format!("{:?}", observed_value),
                            ),
                        ],
                    };
                }
            }
        }

        self.reads_verified += 1;
        InvariantResult::Ok
    }

    /// Clears pending writes (e.g., after a transaction commits).
    pub fn clear_pending_writes(&mut self) {
        self.pending_writes.clear();
    }

    /// Returns the number of writes tracked.
    pub fn writes_tracked(&self) -> u64 {
        self.writes_tracked
    }

    /// Returns the number of reads verified.
    pub fn reads_verified(&self) -> u64 {
        self.reads_verified
    }
}

impl Default for ReadYourWritesChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for ReadYourWritesChecker {
    fn name(&self) -> &'static str {
        "ReadYourWritesChecker"
    }

    fn reset(&mut self) {
        self.pending_writes.clear();
        self.writes_tracked = 0;
        self.reads_verified = 0;
    }
}

// ============================================================================
// Type Safety Checker
// ============================================================================

/// Verifies type safety across query operations.
///
/// This ensures that:
/// - Column types match schema definitions
/// - Type coercions are safe and well-defined
/// - No type confusion between operations
///
/// Violations indicate:
/// - Schema validation bugs
/// - Type coercion errors
/// - Serialization/deserialization issues
#[derive(Debug)]
pub struct TypeSafetyChecker {
    /// Schema tracking: table -> column -> expected type
    schema: HashMap<String, HashMap<String, String>>,
    /// Number of type checks performed.
    checks_performed: u64,
}

impl TypeSafetyChecker {
    /// Creates a new type safety checker.
    pub fn new() -> Self {
        Self {
            schema: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Registers a table's schema.
    pub fn register_table(&mut self, table: &str, columns: &[(String, String)]) {
        let mut col_types = HashMap::new();
        for (col_name, col_type) in columns {
            col_types.insert(col_name.clone(), col_type.clone());
        }
        self.schema.insert(table.to_string(), col_types);
    }

    /// Verifies that a value matches the expected type for a column.
    pub fn verify_type(&mut self, table: &str, column: &str, value_type: &str) -> InvariantResult {
        if let Some(table_schema) = self.schema.get(table) {
            if let Some(expected_type) = table_schema.get(column) {
                // Check if types match or are compatible
                if !self.types_compatible(expected_type, value_type) {
                    return InvariantResult::Violated {
                        invariant: "type_safety".to_string(),
                        message: format!(
                            "type mismatch: table={}, column={}, expected={}, got={}",
                            table, column, expected_type, value_type
                        ),
                        context: vec![
                            ("table".to_string(), table.to_string()),
                            ("column".to_string(), column.to_string()),
                            ("expected_type".to_string(), expected_type.clone()),
                            ("actual_type".to_string(), value_type.to_string()),
                        ],
                    };
                }
            }
        }

        self.checks_performed += 1;
        InvariantResult::Ok
    }

    /// Checks if two types are compatible (exact match or valid coercion).
    fn types_compatible(&self, expected: &str, actual: &str) -> bool {
        if expected == actual {
            return true;
        }

        // Allow numeric type coercions
        let numeric_types = [
            "TINYINT", "SMALLINT", "INTEGER", "BIGINT", "REAL", "DECIMAL",
        ];
        if numeric_types.contains(&expected) && numeric_types.contains(&actual) {
            return true;
        }

        false
    }

    /// Returns the number of type checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }
}

impl Default for TypeSafetyChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for TypeSafetyChecker {
    fn name(&self) -> &'static str {
        "TypeSafetyChecker"
    }

    fn reset(&mut self) {
        self.schema.clear();
        self.checks_performed = 0;
    }
}

// ============================================================================
// ORDER BY + LIMIT Correctness Checker
// ============================================================================

/// Verifies that ORDER BY + LIMIT returns the correct top-N results.
///
/// This ensures that:
/// - LIMIT is applied AFTER sorting
/// - The correct rows are returned (not arbitrary rows)
/// - Ordering is stable and deterministic
///
/// Violations indicate bugs in query execution order.
#[derive(Debug)]
pub struct OrderByLimitChecker {
    /// Known full result sets for queries (for verification).
    /// Maps query -> full sorted result
    full_results: HashMap<String, Vec<String>>,
    /// Number of checks performed.
    checks_performed: u64,
}

impl OrderByLimitChecker {
    /// Creates a new ORDER BY + LIMIT checker.
    pub fn new() -> Self {
        Self {
            full_results: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records a full result set for a query (without LIMIT).
    pub fn record_full_result(&mut self, query_key: &str, rows: Vec<String>) {
        self.full_results.insert(query_key.to_string(), rows);
    }

    /// Verifies that a limited result is a correct prefix of the full result.
    pub fn verify_limited_result(
        &mut self,
        query_key: &str,
        limited_rows: &[String],
        limit: usize,
    ) -> InvariantResult {
        if let Some(full_result) = self.full_results.get(query_key) {
            // Limited result should be the first N rows of the full result
            let expected = &full_result[..limit.min(full_result.len())];

            if limited_rows != expected {
                return InvariantResult::Violated {
                    invariant: "order_by_limit_correctness".to_string(),
                    message: format!("ORDER BY + LIMIT returned wrong rows: query={}", query_key),
                    context: vec![
                        ("query_key".to_string(), query_key.to_string()),
                        ("limit".to_string(), limit.to_string()),
                        ("expected_rows".to_string(), format!("{:?}", expected)),
                        ("actual_rows".to_string(), format!("{:?}", limited_rows)),
                    ],
                };
            }
        }

        self.checks_performed += 1;
        InvariantResult::Ok
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }
}

impl Default for OrderByLimitChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for OrderByLimitChecker {
    fn name(&self) -> &'static str {
        "OrderByLimitChecker"
    }

    fn reset(&mut self) {
        self.full_results.clear();
        self.checks_performed = 0;
    }
}

// ============================================================================
// Aggregate Correctness Checker
// ============================================================================

/// Verifies that aggregate functions produce correct results.
///
/// This maintains a "ground truth" by manually computing aggregates
/// and comparing to query results.
///
/// Violations indicate bugs in aggregate computation.
#[derive(Debug)]
pub struct AggregateCorrectnessChecker {
    /// Tracked data for verification: table -> rows (serialized)
    table_data: HashMap<String, Vec<String>>,
    /// Number of checks performed.
    checks_performed: u64,
}

impl AggregateCorrectnessChecker {
    /// Creates a new aggregate correctness checker.
    pub fn new() -> Self {
        Self {
            table_data: HashMap::new(),
            checks_performed: 0,
        }
    }

    /// Records table data for aggregate verification.
    pub fn record_table_data(&mut self, table: &str, rows: Vec<String>) {
        self.table_data.insert(table.to_string(), rows);
    }

    /// Verifies a COUNT(*) result.
    pub fn verify_count(&mut self, table: &str, observed_count: i64) -> InvariantResult {
        if let Some(rows) = self.table_data.get(table) {
            let expected_count = rows.len() as i64;

            if observed_count != expected_count {
                return InvariantResult::Violated {
                    invariant: "aggregate_count_correctness".to_string(),
                    message: format!("COUNT(*) returned wrong value for table {}", table),
                    context: vec![
                        ("table".to_string(), table.to_string()),
                        ("expected_count".to_string(), expected_count.to_string()),
                        ("observed_count".to_string(), observed_count.to_string()),
                    ],
                };
            }
        }

        self.checks_performed += 1;
        InvariantResult::Ok
    }

    /// Verifies a SUM result (simplified - expects integer sums).
    pub fn verify_sum(
        &mut self,
        query_key: &str,
        observed_sum: i64,
        expected_sum: i64,
    ) -> InvariantResult {
        if observed_sum != expected_sum {
            return InvariantResult::Violated {
                invariant: "aggregate_sum_correctness".to_string(),
                message: format!("SUM() returned wrong value: query={}", query_key),
                context: vec![
                    ("query_key".to_string(), query_key.to_string()),
                    ("expected_sum".to_string(), expected_sum.to_string()),
                    ("observed_sum".to_string(), observed_sum.to_string()),
                ],
            };
        }

        self.checks_performed += 1;
        InvariantResult::Ok
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }
}

impl Default for AggregateCorrectnessChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for AggregateCorrectnessChecker {
    fn name(&self) -> &'static str {
        "AggregateCorrectnessChecker"
    }

    fn reset(&mut self) {
        self.table_data.clear();
        self.checks_performed = 0;
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_determinism_checker_detects_nondeterminism() {
        let mut checker = QueryDeterminismChecker::new();

        // First execution
        let result1 = checker.check_query("SELECT * FROM users", &[], "result1");
        assert!(result1.is_ok());

        // Same query, same result - OK
        let result2 = checker.check_query("SELECT * FROM users", &[], "result1");
        assert!(result2.is_ok());

        // Same query, different result - VIOLATION
        let result3 = checker.check_query("SELECT * FROM users", &[], "result2");
        assert!(!result3.is_ok());
    }

    #[test]
    fn query_determinism_checker_tracks_params() {
        let mut checker = QueryDeterminismChecker::new();

        // Different params = different query
        checker.check_query(
            "SELECT * FROM users WHERE id = $1",
            &["1".to_string()],
            "result1",
        );
        checker.check_query(
            "SELECT * FROM users WHERE id = $1",
            &["2".to_string()],
            "result2",
        );

        assert_eq!(checker.unique_queries(), 2);
    }

    #[test]
    fn read_your_writes_checker_basic() {
        let mut checker = ReadYourWritesChecker::new();

        // Write a value
        checker.record_write("users", "1", Some("Alice"));

        // Read should see the written value
        let result = checker.verify_read("users", "1", Some("Alice"));
        assert!(result.is_ok());

        // Read with wrong value - VIOLATION
        let result = checker.verify_read("users", "1", Some("Bob"));
        assert!(!result.is_ok());
    }

    #[test]
    fn read_your_writes_checker_delete() {
        let mut checker = ReadYourWritesChecker::new();

        // Delete a row (write None)
        checker.record_write("users", "1", None);

        // Read should see None
        let result = checker.verify_read("users", "1", None);
        assert!(result.is_ok());

        // Read with value - VIOLATION
        let result = checker.verify_read("users", "1", Some("Alice"));
        assert!(!result.is_ok());
    }

    #[test]
    fn type_safety_checker_basic() {
        let mut checker = TypeSafetyChecker::new();

        checker.register_table(
            "users",
            &[
                ("id".to_string(), "BIGINT".to_string()),
                ("name".to_string(), "TEXT".to_string()),
            ],
        );

        // Correct type - OK
        let result = checker.verify_type("users", "id", "BIGINT");
        assert!(result.is_ok());

        // Type mismatch - VIOLATION
        let result = checker.verify_type("users", "id", "TEXT");
        assert!(!result.is_ok());
    }

    #[test]
    fn type_safety_checker_numeric_coercion() {
        let mut checker = TypeSafetyChecker::new();

        checker.register_table("users", &[("id".to_string(), "INTEGER".to_string())]);

        // INTEGER can coerce to BIGINT
        let result = checker.verify_type("users", "id", "BIGINT");
        assert!(result.is_ok());

        // But not to TEXT
        let result = checker.verify_type("users", "id", "TEXT");
        assert!(!result.is_ok());
    }

    #[test]
    fn order_by_limit_checker_basic() {
        let mut checker = OrderByLimitChecker::new();

        // Record full result
        let full_result = vec!["row1".to_string(), "row2".to_string(), "row3".to_string()];
        checker.record_full_result("query1", full_result);

        // Verify limited result (first 2 rows)
        let limited = vec!["row1".to_string(), "row2".to_string()];
        let result = checker.verify_limited_result("query1", &limited, 2);
        assert!(result.is_ok());

        // Wrong limited result - VIOLATION
        let wrong_limited = vec!["row2".to_string(), "row3".to_string()];
        let result = checker.verify_limited_result("query1", &wrong_limited, 2);
        assert!(!result.is_ok());
    }

    #[test]
    fn aggregate_correctness_checker_count() {
        let mut checker = AggregateCorrectnessChecker::new();

        checker.record_table_data(
            "users",
            vec!["row1".to_string(), "row2".to_string(), "row3".to_string()],
        );

        // Correct count - OK
        let result = checker.verify_count("users", 3);
        assert!(result.is_ok());

        // Wrong count - VIOLATION
        let result = checker.verify_count("users", 5);
        assert!(!result.is_ok());
    }

    #[test]
    fn aggregate_correctness_checker_sum() {
        let mut checker = AggregateCorrectnessChecker::new();

        // Expected sum: 1 + 2 + 3 = 6
        let result = checker.verify_sum("sum_query", 6, 6);
        assert!(result.is_ok());

        // Wrong sum - VIOLATION
        let result = checker.verify_sum("sum_query", 10, 6);
        assert!(!result.is_ok());
    }

    #[test]
    fn all_checkers_implement_trait() {
        let mut determinism: Box<dyn InvariantChecker> = Box::new(QueryDeterminismChecker::new());
        let mut ryw: Box<dyn InvariantChecker> = Box::new(ReadYourWritesChecker::new());
        let mut type_safety: Box<dyn InvariantChecker> = Box::new(TypeSafetyChecker::new());
        let mut order_limit: Box<dyn InvariantChecker> = Box::new(OrderByLimitChecker::new());
        let mut aggregate: Box<dyn InvariantChecker> = Box::new(AggregateCorrectnessChecker::new());

        assert_eq!(determinism.name(), "QueryDeterminismChecker");
        assert_eq!(ryw.name(), "ReadYourWritesChecker");
        assert_eq!(type_safety.name(), "TypeSafetyChecker");
        assert_eq!(order_limit.name(), "OrderByLimitChecker");
        assert_eq!(aggregate.name(), "AggregateCorrectnessChecker");

        determinism.reset();
        ryw.reset();
        type_safety.reset();
        order_limit.reset();
        aggregate.reset();
    }
}
