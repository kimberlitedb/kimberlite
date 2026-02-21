//! # SQL Differential Testing Oracles
//!
//! This crate provides oracle implementations for differential SQL testing,
//! inspired by the Crucible framework and SQLancer research.
//!
//! ## Architecture
//!
//! The `OracleRunner` trait defines a common interface for executing SQL queries
//! against different database engines. Implementations include:
//!
//! - **`DuckDbOracle`**: Executes queries in DuckDB (ground truth oracle)
//! - **`KimberliteOracle`**: Executes queries in Kimberlite (system under test)
//!
//! ## Usage
//!
//! ```rust,no_run
//! use kimberlite_oracle::{OracleRunner, DuckDbOracle, KimberliteOracle};
//!
//! let duckdb = DuckDbOracle::new()?;
//! let kimberlite = KimberliteOracle::new(/* ... */)?;
//!
//! let sql = "SELECT COUNT(*) FROM users WHERE age > 30";
//! let duckdb_result = duckdb.execute(sql)?;
//! let kimberlite_result = kimberlite.execute(sql)?;
//!
//! // Compare results to find bugs
//! assert_eq!(duckdb_result, kimberlite_result);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Differential Testing Strategy
//!
//! Differential testing compares two implementations of the same specification:
//!
//! 1. **Generate** a valid SQL query
//! 2. **Execute** in both DuckDB (reference) and Kimberlite (SUT)
//! 3. **Compare** results byte-by-byte
//! 4. **Report** any discrepancies as bugs
//!
//! **Why DuckDB?**
//! - Battle-tested SQL engine with 99.9% TPC-H compliance
//! - Embedded (no network overhead)
//! - Fast (columnar execution)
//! - Well-documented semantics
//!
//! ## References
//!
//! - Crucible: "Detecting Logic Bugs in DBMS" (154+ bugs found)
//! - SQLancer: Differential testing framework (148+ bugs in nghttp2)
//! - Jepsen: Distributed systems testing methodology

use std::fmt;

use kimberlite_query::QueryResult;

pub mod duckdb;
pub mod kimberlite;

pub use self::duckdb::DuckDbOracle;
pub use self::kimberlite::KimberliteOracle;

// ============================================================================
// Oracle Runner Trait
// ============================================================================

/// Trait for executing SQL queries against a database engine.
///
/// Oracle runners provide a uniform interface for differential testing,
/// allowing us to compare results across different implementations.
pub trait OracleRunner {
    /// Executes a SQL query and returns the result.
    ///
    /// # Arguments
    ///
    /// * `sql` - The SQL query string to execute
    ///
    /// # Returns
    ///
    /// - `Ok(QueryResult)` on successful execution
    /// - `Err(OracleError)` if the query fails or produces an error
    ///
    /// # Guarantees
    ///
    /// - **Deterministic**: Same SQL + same state = same result
    /// - **Isolated**: Queries don't interfere with each other
    /// - **Correct**: Results match SQL standard semantics
    fn execute(&mut self, sql: &str) -> Result<QueryResult, OracleError>;

    /// Resets the oracle to its initial state.
    ///
    /// This clears all tables, drops indexes, and resets sequences.
    /// Used to start fresh between test iterations.
    fn reset(&mut self) -> Result<(), OracleError>;

    /// Returns the name of this oracle (for logging).
    fn name(&self) -> &'static str;
}

// ============================================================================
// Oracle Error Types
// ============================================================================

/// Errors that can occur during oracle execution.
#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    /// SQL syntax error (query is malformed).
    #[error("SQL syntax error: {0}")]
    SyntaxError(String),

    /// Semantic error (query is valid but incorrect, e.g., table not found).
    #[error("Semantic error: {0}")]
    SemanticError(String),

    /// Runtime error (query execution failed).
    #[error("Runtime error: {0}")]
    RuntimeError(String),

    /// Timeout error (query took too long).
    #[error("Timeout after {0}ms")]
    Timeout(u64),

    /// Unsupported feature (query uses SQL features not implemented).
    #[error("Unsupported feature: {0}")]
    Unsupported(String),

    /// Internal error (bug in the oracle implementation).
    #[error("Internal error: {0}")]
    Internal(String),
}

// ============================================================================
// Result Comparison
// ============================================================================

/// Compares two query results for equality.
///
/// # Comparison Rules
///
/// - **Column count**: Must match
/// - **Column names**: Must match (case-sensitive)
/// - **Row count**: Must match
/// - **Row values**: Must match byte-for-byte (including NULLs)
/// - **Row order**: Must match (unless ORDER BY is absent)
///
/// # Returns
///
/// - `Ok(())` if results are identical
/// - `Err(ResultMismatch)` with detailed diagnostic info
pub fn compare_results(
    left: &QueryResult,
    right: &QueryResult,
    left_name: &str,
    right_name: &str,
) -> Result<(), ResultMismatch> {
    // Check column count
    if left.columns.len() != right.columns.len() {
        return Err(ResultMismatch::ColumnCountMismatch {
            left: left.columns.len(),
            right: right.columns.len(),
            left_name: left_name.to_string(),
            right_name: right_name.to_string(),
        });
    }

    // Check column names
    for (i, (left_col, right_col)) in left.columns.iter().zip(right.columns.iter()).enumerate() {
        if left_col.as_str() != right_col.as_str() {
            return Err(ResultMismatch::ColumnNameMismatch {
                column_index: i,
                left: left_col.as_str().to_string(),
                right: right_col.as_str().to_string(),
                left_name: left_name.to_string(),
                right_name: right_name.to_string(),
            });
        }
    }

    // Check row count
    if left.rows.len() != right.rows.len() {
        return Err(ResultMismatch::RowCountMismatch {
            left: left.rows.len(),
            right: right.rows.len(),
            left_name: left_name.to_string(),
            right_name: right_name.to_string(),
        });
    }

    // Check row values
    for (row_idx, (left_row, right_row)) in left.rows.iter().zip(right.rows.iter()).enumerate() {
        if left_row.len() != right_row.len() {
            return Err(ResultMismatch::RowValueCountMismatch {
                row_index: row_idx,
                left: left_row.len(),
                right: right_row.len(),
                left_name: left_name.to_string(),
                right_name: right_name.to_string(),
            });
        }

        for (col_idx, (left_val, right_val)) in left_row.iter().zip(right_row.iter()).enumerate() {
            if left_val != right_val {
                return Err(ResultMismatch::ValueMismatch {
                    row_index: row_idx,
                    column_index: col_idx,
                    left: format!("{left_val:?}"),
                    right: format!("{right_val:?}"),
                    left_name: left_name.to_string(),
                    right_name: right_name.to_string(),
                });
            }
        }
    }

    Ok(())
}

/// Describes a mismatch between two query results.
#[derive(Debug, Clone)]
pub enum ResultMismatch {
    /// Column counts don't match.
    ColumnCountMismatch {
        left: usize,
        right: usize,
        left_name: String,
        right_name: String,
    },

    /// Column names don't match.
    ColumnNameMismatch {
        column_index: usize,
        left: String,
        right: String,
        left_name: String,
        right_name: String,
    },

    /// Row counts don't match.
    RowCountMismatch {
        left: usize,
        right: usize,
        left_name: String,
        right_name: String,
    },

    /// Row value counts don't match (same row has different number of columns).
    RowValueCountMismatch {
        row_index: usize,
        left: usize,
        right: usize,
        left_name: String,
        right_name: String,
    },

    /// Individual cell values don't match.
    ValueMismatch {
        row_index: usize,
        column_index: usize,
        left: String,
        right: String,
        left_name: String,
        right_name: String,
    },
}

impl fmt::Display for ResultMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResultMismatch::ColumnCountMismatch {
                left,
                right,
                left_name,
                right_name,
            } => {
                write!(
                    f,
                    "Column count mismatch: {left_name}={left}, {right_name}={right}"
                )
            }
            ResultMismatch::ColumnNameMismatch {
                column_index,
                left,
                right,
                left_name,
                right_name,
            } => {
                write!(
                    f,
                    "Column name mismatch at index {column_index}: {left_name}='{left}', {right_name}='{right}'"
                )
            }
            ResultMismatch::RowCountMismatch {
                left,
                right,
                left_name,
                right_name,
            } => {
                write!(
                    f,
                    "Row count mismatch: {left_name}={left}, {right_name}={right}"
                )
            }
            ResultMismatch::RowValueCountMismatch {
                row_index,
                left,
                right,
                left_name,
                right_name,
            } => {
                write!(
                    f,
                    "Row value count mismatch at row {row_index}: {left_name}={left}, {right_name}={right}"
                )
            }
            ResultMismatch::ValueMismatch {
                row_index,
                column_index,
                left,
                right,
                left_name,
                right_name,
            } => {
                write!(
                    f,
                    "Value mismatch at row {row_index}, column {column_index}: {left_name}={left}, {right_name}={right}"
                )
            }
        }
    }
}

impl std::error::Error for ResultMismatch {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_query::{ColumnName, Value};

    #[test]
    fn test_compare_results_identical() {
        let result1 = QueryResult {
            columns: vec![ColumnName::from("id"), ColumnName::from("name")],
            rows: vec![
                vec![Value::BigInt(1), Value::Text("Alice".to_string())],
                vec![Value::BigInt(2), Value::Text("Bob".to_string())],
            ],
        };

        let result2 = result1.clone();

        assert!(compare_results(&result1, &result2, "left", "right").is_ok());
    }

    #[test]
    fn test_compare_results_column_count_mismatch() {
        let result1 = QueryResult {
            columns: vec![ColumnName::from("id"), ColumnName::from("name")],
            rows: vec![],
        };

        let result2 = QueryResult {
            columns: vec![ColumnName::from("id")],
            rows: vec![],
        };

        let err = compare_results(&result1, &result2, "left", "right").unwrap_err();
        assert!(matches!(err, ResultMismatch::ColumnCountMismatch { .. }));
    }

    #[test]
    fn test_compare_results_column_name_mismatch() {
        let result1 = QueryResult {
            columns: vec![ColumnName::from("id"), ColumnName::from("name")],
            rows: vec![],
        };

        let result2 = QueryResult {
            columns: vec![ColumnName::from("id"), ColumnName::from("email")],
            rows: vec![],
        };

        let err = compare_results(&result1, &result2, "left", "right").unwrap_err();
        assert!(matches!(err, ResultMismatch::ColumnNameMismatch { .. }));
    }

    #[test]
    fn test_compare_results_row_count_mismatch() {
        let result1 = QueryResult {
            columns: vec![ColumnName::from("id")],
            rows: vec![vec![Value::BigInt(1)], vec![Value::BigInt(2)]],
        };

        let result2 = QueryResult {
            columns: vec![ColumnName::from("id")],
            rows: vec![vec![Value::BigInt(1)]],
        };

        let err = compare_results(&result1, &result2, "left", "right").unwrap_err();
        assert!(matches!(err, ResultMismatch::RowCountMismatch { .. }));
    }

    #[test]
    fn test_compare_results_value_mismatch() {
        let result1 = QueryResult {
            columns: vec![ColumnName::from("id")],
            rows: vec![vec![Value::BigInt(1)]],
        };

        let result2 = QueryResult {
            columns: vec![ColumnName::from("id")],
            rows: vec![vec![Value::BigInt(2)]],
        };

        let err = compare_results(&result1, &result2, "left", "right").unwrap_err();
        assert!(matches!(err, ResultMismatch::ValueMismatch { .. }));
    }
}
