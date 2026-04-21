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

    /// SQL input exceeds a complexity limit (depth or token budget).
    /// Pre-parse guard that rejects pathological inputs which would
    /// trigger super-linear behavior in the upstream SQL parser.
    #[error("sql too complex: {kind} = {value} exceeds limit {limit}")]
    SqlTooComplex {
        /// Which budget was exceeded (e.g., `paren_depth`, `not_tokens`).
        kind: &'static str,
        /// Observed value.
        value: usize,
        /// Configured limit.
        limit: usize,
    },

    /// Constraint violation (e.g., duplicate primary key, NOT NULL violation).
    #[error("constraint violation: {0}")]
    ConstraintViolation(String),

    /// Underlying store error.
    #[error("store error: {0}")]
    Store(#[from] StoreError),

    /// JSON serialization/deserialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type for query operations.
pub type Result<T> = std::result::Result<T, QueryError>;
