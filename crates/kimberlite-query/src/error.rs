//! Error types for query operations.

use kimberlite_store::StoreError;

/// Errors that can occur during query parsing and execution.
#[derive(thiserror::Error, Debug)]
pub enum QueryError {
    /// SQL syntax or parsing error.
    #[error("parse error: {0}")]
    ParseError(String),

    /// Table not found in schema.
    #[error("table '{0}' not found")]
    TableNotFound(String),

    /// Column not found in table.
    #[error("column '{column}' not found in table '{table}'")]
    ColumnNotFound { table: String, column: String },

    /// Query parameter not provided.
    #[error("parameter ${0} not provided")]
    ParameterNotFound(usize),

    /// Type mismatch between expected and actual value.
    #[error("type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },

    /// SQL feature not supported.
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),

    /// Constraint violation (e.g., duplicate primary key, NOT NULL violation).
    #[error("constraint violation: {0}")]
    ConstraintViolation(String),

    /// Correlated subquery row-evaluation cap exceeded.
    ///
    /// Emitted before the correlated-loop executor runs when the estimated
    /// product of outer rows × inner rows per iteration exceeds the
    /// configured cap (default `10_000_000`; see
    /// `QueryEngine::with_correlated_cap`). Fails fast rather than
    /// consuming memory. See `docs/reference/sql/correlated-subqueries.md`.
    #[error(
        "correlated subquery cardinality exceeded: estimated {estimated} row \
         evaluations exceeds cap of {cap}"
    )]
    CorrelatedCardinalityExceeded { estimated: u64, cap: u64 },

    /// Underlying store error.
    #[error("store error: {0}")]
    Store(#[from] StoreError),

    /// JSON serialization/deserialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type for query operations.
pub type Result<T> = std::result::Result<T, QueryError>;
