//! Error types for query operations.

use kimberlite_store::StoreError;

use crate::value::Value;

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

    /// Constraint violation (e.g., NOT NULL violation, type constraint).
    ///
    /// Generic catch-all for non-uniqueness constraint failures.
    /// Duplicate primary keys raise [`Self::DuplicatePrimaryKey`] instead
    /// so SDK callers can pattern-match without parsing message strings.
    #[error("constraint violation: {0}")]
    ConstraintViolation(String),

    /// Duplicate primary-key value detected on INSERT.
    ///
    /// Carries the table name and the rejected key tuple so callers can
    /// short-circuit retry/upsert flows without parsing the error string.
    /// Notebar's webhook-dedup loop (try-INSERT-then-SELECT) is the
    /// canonical consumer.
    #[error("duplicate primary key in table '{table}': {key:?}")]
    DuplicatePrimaryKey {
        /// Name of the table whose primary key was violated.
        table: String,
        /// Rejected key tuple (one element per primary-key column).
        key: Vec<Value>,
    },

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

    /// Requested `AS OF TIMESTAMP` precedes the earliest retained event.
    ///
    /// Emitted when a `FOR SYSTEM_TIME AS OF '<iso>'` / `AS OF TIMESTAMP`
    /// query asks for a wall-clock instant older than the oldest entry
    /// in the timestamp-to-offset index (typically a freshly-opened
    /// database, or a timestamp predating any write). Distinguished
    /// from a general "no offset found" error so callers can surface
    /// the retention horizon to the user.
    ///
    /// `requested_ns` is the caller-supplied Unix-nanosecond timestamp;
    /// `horizon_ns` is the earliest wall-clock instant the index can
    /// answer for (or `0` when the log is empty).
    ///
    /// Shipped with v0.6.0 Tier 2 #6 (AS OF TIMESTAMP time-travel).
    #[error(
        "AS OF TIMESTAMP {requested_ns} ns precedes the earliest retained \
         event (retention horizon: {horizon_ns} ns)"
    )]
    AsOfBeforeRetentionHorizon { requested_ns: i64, horizon_ns: i64 },

    /// Underlying store error.
    #[error("store error: {0}")]
    Store(#[from] StoreError),

    /// JSON serialization/deserialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type for query operations.
pub type Result<T> = std::result::Result<T, QueryError>;
