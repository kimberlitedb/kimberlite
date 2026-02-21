//! Query plan intermediate representation.
//!
//! Defines the execution plan produced by the query planner.

use std::ops::Bound;

use kimberlite_store::{Key, TableId};

use crate::parser::{AggregateFunction, HavingCondition};
use crate::schema::{ColumnDef, ColumnName};
use crate::value::Value;

/// Table metadata embedded in query plans.
///
/// Contains everything needed to decode rows without external schema access.
/// This ensures plans are self-contained and preserve schema version for MVCC.
#[derive(Debug, Clone)]
pub struct TableMetadata {
    /// Table ID for storage lookups.
    pub table_id: TableId,
    /// Table name (for error messages).
    pub table_name: String,
    /// Column definitions (for row decoding).
    pub columns: Vec<ColumnDef>,
    /// Primary key columns.
    pub primary_key: Vec<ColumnName>,
}

/// Join condition for column-to-column comparisons.
#[derive(Debug, Clone)]
pub struct JoinCondition {
    /// Left column index in concatenated row.
    pub left_col_idx: usize,
    /// Right column index in concatenated row.
    pub right_col_idx: usize,
    /// Comparison operator.
    pub op: JoinOp,
}

/// Join comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinOp {
    /// Equal (=)
    Eq,
    /// Less than (<)
    Lt,
    /// Less than or equal (<=)
    Le,
    /// Greater than (>)
    Gt,
    /// Greater than or equal (>=)
    Ge,
}

/// A query execution plan.
#[derive(Debug, Clone)]
pub enum QueryPlan {
    /// Point lookup: WHERE pk = value
    PointLookup {
        /// Table metadata (embedded for MVCC correctness).
        metadata: TableMetadata,
        /// Encoded primary key.
        key: Key,
        /// Column indices to project (empty = all columns).
        columns: Vec<usize>,
        /// Column names to return.
        column_names: Vec<ColumnName>,
    },

    /// Range scan on primary key.
    RangeScan {
        /// Table metadata (embedded for MVCC correctness).
        metadata: TableMetadata,
        /// Start bound (inclusive/exclusive/unbounded).
        start: Bound<Key>,
        /// End bound (inclusive/exclusive/unbounded).
        end: Bound<Key>,
        /// Additional filter to apply after fetching.
        filter: Option<Filter>,
        /// Maximum rows to return.
        limit: Option<usize>,
        /// Sort order for index scan.
        order: ScanOrder,
        /// Client-side sort specification (used when ORDER BY doesn't match scan order).
        order_by: Option<SortSpec>,
        /// Column indices to project (empty = all columns).
        columns: Vec<usize>,
        /// Column names to return.
        column_names: Vec<ColumnName>,
    },

    /// Index scan on a secondary index.
    IndexScan {
        /// Table metadata (embedded for MVCC correctness).
        metadata: TableMetadata,
        /// Index ID to scan.
        index_id: u64,
        /// Index name (for error messages).
        index_name: String,
        /// Start bound on index key.
        start: Bound<Key>,
        /// End bound on index key.
        end: Bound<Key>,
        /// Additional filter to apply after fetching.
        filter: Option<Filter>,
        /// Maximum rows to return.
        limit: Option<usize>,
        /// Sort order for index scan.
        order: ScanOrder,
        /// Client-side sort specification (used when ORDER BY doesn't match scan order).
        order_by: Option<SortSpec>,
        /// Column indices to project (empty = all columns).
        columns: Vec<usize>,
        /// Column names to return.
        column_names: Vec<ColumnName>,
    },

    /// Full table scan with optional filter.
    TableScan {
        /// Table metadata (embedded for MVCC correctness).
        metadata: TableMetadata,
        /// Filter to apply.
        filter: Option<Filter>,
        /// Maximum rows to return (after filtering).
        limit: Option<usize>,
        /// Sort order (client-side).
        order: Option<SortSpec>,
        /// Column indices to project (empty = all columns).
        columns: Vec<usize>,
        /// Column names to return.
        column_names: Vec<ColumnName>,
    },

    /// Aggregate query with optional grouping.
    Aggregate {
        /// Table metadata (embedded for MVCC correctness).
        metadata: TableMetadata,
        /// Underlying scan to get rows.
        source: Box<QueryPlan>,
        /// Columns to group by (column indices).
        group_by_cols: Vec<usize>,
        /// Column names for GROUP BY.
        group_by_names: Vec<ColumnName>,
        /// Aggregate functions to compute.
        aggregates: Vec<AggregateFunction>,
        /// Column names to return (`group_by` columns + aggregate results).
        column_names: Vec<ColumnName>,
        /// HAVING conditions to filter groups after aggregation.
        having: Vec<HavingCondition>,
    },

    /// Nested loop join between two tables.
    Join {
        /// Join type (Inner or Left).
        join_type: crate::parser::JoinType,
        /// Left table scan.
        left: Box<QueryPlan>,
        /// Right table scan.
        right: Box<QueryPlan>,
        /// Join conditions (ON clause) - column-to-column comparisons.
        on_conditions: Vec<JoinCondition>,
        /// Column indices to project (empty = all columns).
        columns: Vec<usize>,
        /// Column names to return.
        column_names: Vec<ColumnName>,
    },

    /// Post-processing: apply filter, computed columns, sort, and limit to source rows.
    ///
    /// Used to apply WHERE / ORDER BY / LIMIT / CASE WHEN on top of Join plans,
    /// and for CASE WHEN columns on single-table scans.
    Materialize {
        /// Source plan producing rows.
        source: Box<QueryPlan>,
        /// Optional filter to apply after materializing rows.
        filter: Option<Filter>,
        /// CASE WHEN computed columns to append to each row.
        case_columns: Vec<CaseColumnDef>,
        /// Optional client-side sort.
        order: Option<SortSpec>,
        /// Optional row limit (applied after filter and sort).
        limit: Option<usize>,
        /// Output column names.
        column_names: Vec<ColumnName>,
    },
}

/// A CASE WHEN computed column.
///
/// Evaluated per-row: the first matching WHEN clause determines the output value.
#[derive(Debug, Clone)]
pub struct CaseColumnDef {
    /// Alias name for this column in the output.
    pub alias: ColumnName,
    /// WHEN ... THEN ... arms evaluated in order.
    pub when_clauses: Vec<CaseWhenClause>,
    /// Value returned when no WHEN clause matches. Defaults to NULL.
    pub else_value: crate::value::Value,
}

/// A single WHEN condition → THEN result arm of a CASE expression.
#[derive(Debug, Clone)]
pub struct CaseWhenClause {
    /// Filter condition evaluated against the row.
    pub condition: Filter,
    /// Value returned when the condition matches.
    pub result: crate::value::Value,
}

impl QueryPlan {
    /// Returns the column names this plan will return.
    pub fn column_names(&self) -> &[ColumnName] {
        match self {
            QueryPlan::PointLookup { column_names, .. }
            | QueryPlan::RangeScan { column_names, .. }
            | QueryPlan::IndexScan { column_names, .. }
            | QueryPlan::TableScan { column_names, .. }
            | QueryPlan::Aggregate { column_names, .. }
            | QueryPlan::Join { column_names, .. }
            | QueryPlan::Materialize { column_names, .. } => column_names,
        }
    }

    /// Returns the column indices to project.
    #[allow(dead_code)]
    pub fn column_indices(&self) -> &[usize] {
        match self {
            QueryPlan::PointLookup { columns, .. }
            | QueryPlan::RangeScan { columns, .. }
            | QueryPlan::IndexScan { columns, .. }
            | QueryPlan::TableScan { columns, .. }
            | QueryPlan::Join { columns, .. } => columns,
            QueryPlan::Aggregate { group_by_cols, .. } => group_by_cols,
            QueryPlan::Materialize { .. } => &[],
        }
    }

    /// Returns the table name.
    pub fn table_name(&self) -> &str {
        match self {
            QueryPlan::PointLookup { metadata, .. }
            | QueryPlan::RangeScan { metadata, .. }
            | QueryPlan::IndexScan { metadata, .. }
            | QueryPlan::TableScan { metadata, .. }
            | QueryPlan::Aggregate { metadata, .. } => &metadata.table_name,
            QueryPlan::Join { left, .. } | QueryPlan::Materialize { source: left, .. } => {
                left.table_name()
            }
        }
    }

    /// Returns the table metadata (for single-table plans).
    pub fn metadata(&self) -> Option<&TableMetadata> {
        match self {
            QueryPlan::PointLookup { metadata, .. }
            | QueryPlan::RangeScan { metadata, .. }
            | QueryPlan::IndexScan { metadata, .. }
            | QueryPlan::TableScan { metadata, .. }
            | QueryPlan::Aggregate { metadata, .. } => Some(metadata),
            QueryPlan::Join { .. } | QueryPlan::Materialize { .. } => None,
        }
    }
}

/// Scan order for range scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanOrder {
    /// Ascending order (natural B+tree order).
    #[default]
    Ascending,
    /// Descending order (reverse iteration).
    Descending,
}

/// Sort specification for table scans.
#[derive(Debug, Clone)]
pub struct SortSpec {
    /// Columns to sort by.
    pub columns: Vec<(usize, ScanOrder)>,
}

/// Filter to apply to scanned rows.
///
/// Supports both AND and OR logical operations in a tree structure.
#[derive(Debug, Clone)]
pub enum Filter {
    /// Single condition.
    Condition(FilterCondition),
    /// All conditions must match (AND).
    And(Vec<Filter>),
    /// At least one condition must match (OR).
    Or(Vec<Filter>),
}

impl Filter {
    /// Creates a filter with a single condition.
    pub fn single(condition: FilterCondition) -> Self {
        Filter::Condition(condition)
    }

    /// Creates a filter with AND of multiple conditions.
    pub fn and(filters: Vec<Filter>) -> Self {
        assert!(
            !filters.is_empty(),
            "AND filter must have at least one condition"
        );
        if filters.len() == 1 {
            return filters
                .into_iter()
                .next()
                .expect("filter list verified to have exactly 1 element");
        }
        Filter::And(filters)
    }

    /// Creates a filter with OR of multiple conditions.
    pub fn or(filters: Vec<Filter>) -> Self {
        assert!(
            !filters.is_empty(),
            "OR filter must have at least one condition"
        );
        if filters.len() == 1 {
            return filters
                .into_iter()
                .next()
                .expect("filter list verified to have exactly 1 element");
        }
        Filter::Or(filters)
    }

    /// Evaluates the filter against a row.
    pub fn matches(&self, row: &[Value]) -> bool {
        match self {
            Filter::Condition(c) => c.matches(row),
            Filter::And(filters) => filters.iter().all(|f| f.matches(row)),
            Filter::Or(filters) => filters.iter().any(|f| f.matches(row)),
        }
    }
}

/// A single filter condition.
#[derive(Debug, Clone)]
pub struct FilterCondition {
    /// Column index to compare.
    pub column_idx: usize,
    /// Comparison operator.
    pub op: FilterOp,
    /// Value to compare against.
    pub value: Value,
}

impl FilterCondition {
    /// Evaluates this condition against a row.
    pub fn matches(&self, row: &[Value]) -> bool {
        debug_assert!(
            self.column_idx < row.len(),
            "column index {} must be within row bounds (len={})",
            self.column_idx,
            row.len()
        );
        let Some(cell) = row.get(self.column_idx) else {
            return false;
        };

        match &self.op {
            FilterOp::Eq => cell == &self.value,
            FilterOp::Lt => cell.compare(&self.value) == Some(std::cmp::Ordering::Less),
            FilterOp::Le => matches!(
                cell.compare(&self.value),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            ),
            FilterOp::Gt => cell.compare(&self.value) == Some(std::cmp::Ordering::Greater),
            FilterOp::Ge => matches!(
                cell.compare(&self.value),
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            ),
            FilterOp::In(values) => {
                // Try exact match first
                if values.contains(cell) {
                    return true;
                }
                // Try type-coerced comparison for numeric types
                values.iter().any(|v| match (cell, v) {
                    // Integer type coercions
                    (Value::TinyInt(a), Value::SmallInt(b)) => i16::from(*a) == *b,
                    (Value::TinyInt(a), Value::Integer(b)) => i32::from(*a) == *b,
                    (Value::TinyInt(a), Value::BigInt(b)) => i64::from(*a) == *b,
                    (Value::SmallInt(a), Value::TinyInt(b)) => *a == i16::from(*b),
                    (Value::SmallInt(a), Value::Integer(b)) => i32::from(*a) == *b,
                    (Value::SmallInt(a), Value::BigInt(b)) => i64::from(*a) == *b,
                    (Value::Integer(a), Value::TinyInt(b)) => *a == i32::from(*b),
                    (Value::Integer(a), Value::SmallInt(b)) => *a == i32::from(*b),
                    (Value::Integer(a), Value::BigInt(b)) => i64::from(*a) == *b,
                    (Value::BigInt(a), Value::TinyInt(b)) => *a == i64::from(*b),
                    (Value::BigInt(a), Value::SmallInt(b)) => *a == i64::from(*b),
                    (Value::BigInt(a), Value::Integer(b)) => *a == i64::from(*b),
                    _ => false,
                })
            }
            FilterOp::Like(pattern) => {
                debug_assert!(!pattern.is_empty(), "LIKE pattern must not be empty");
                match cell {
                    Value::Text(s) => matches_like_pattern(s, pattern),
                    _ => false,
                }
            }
            FilterOp::IsNull => cell.is_null(),
            FilterOp::IsNotNull => !cell.is_null(),
        }
    }
}

/// Pattern matching for LIKE operator.
///
/// Supports:
/// - `%` matches zero or more characters
/// - `_` matches exactly one character
/// - `\%` and `\_` match literal `%` and `_`
///
/// Uses iterative dynamic programming (O(m×n) time, O(m) space) to avoid
/// the exponential back-tracking that afflicts recursive implementations and
/// makes them vulnerable to ReDoS attacks with adversarial inputs.
pub(crate) fn matches_like_pattern(text: &str, pattern: &str) -> bool {
    debug_assert!(!pattern.is_empty(), "LIKE pattern must not be empty");

    let text_chars: Vec<char> = text.chars().collect();

    // Pre-process pattern to expand escape sequences into typed tokens.
    #[allow(clippy::items_after_statements)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Token {
        Literal(char),
        Any,  // % — zero or more
        One,  // _ — exactly one
    }

    let mut tokens: Vec<Token> = Vec::new();
    let pat_chars: Vec<char> = pattern.chars().collect();
    let mut pi = 0;
    while pi < pat_chars.len() {
        if pat_chars[pi] == '\\' && pi + 1 < pat_chars.len() {
            let next = pat_chars[pi + 1];
            if next == '%' || next == '_' {
                tokens.push(Token::Literal(next));
                pi += 2;
                continue;
            }
        }
        tokens.push(match pat_chars[pi] {
            '%' => Token::Any,
            '_' => Token::One,
            c   => Token::Literal(c),
        });
        pi += 1;
    }

    let t_len = text_chars.len();
    let p_len = tokens.len();

    // dp[j] = true iff pattern[0..j] matches text[0..i] (current text prefix).
    // We only need the previous row in memory.
    let mut dp = vec![false; p_len + 1];
    dp[0] = true;

    // Pattern prefixes consisting entirely of `%` wildcards match the empty text.
    for j in 1..=p_len {
        if tokens[j - 1] == Token::Any {
            dp[j] = dp[j - 1];
        } else {
            break;
        }
    }

    for i in 1..=t_len {
        let mut new_dp = vec![false; p_len + 1];
        // new_dp[0] is always false: non-empty text cannot match empty pattern.
        for j in 1..=p_len {
            new_dp[j] = match tokens[j - 1] {
                Token::Any => {
                    // `%` can match zero chars (dp[j-1] for text[0..i-1] with pattern[0..j])
                    // or one more char (new_dp[j-1] for text[0..i] with pattern[0..j-1]).
                    dp[j] || new_dp[j - 1]
                }
                Token::One => {
                    // `_` matches exactly one character.
                    dp[j - 1]
                }
                Token::Literal(c) => {
                    // Literal: text character must match pattern character.
                    dp[j - 1] && text_chars[i - 1] == c
                }
            };
        }
        dp = new_dp;
    }

    dp[p_len]
}

/// Filter comparison operator.
#[derive(Debug, Clone)]
pub enum FilterOp {
    /// Equal.
    Eq,
    /// Less than.
    Lt,
    /// Less than or equal.
    Le,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Ge,
    /// In list.
    In(Vec<Value>),
    /// Pattern matching with wildcards (% = any chars, _ = single char).
    Like(String),
    /// IS NULL check.
    IsNull,
    /// IS NOT NULL check.
    IsNotNull,
}
