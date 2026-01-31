//! Query plan intermediate representation.
//!
//! Defines the execution plan produced by the query planner.

use std::ops::Bound;

use kmb_store::{Key, TableId};

use crate::parser::AggregateFunction;
use crate::schema::ColumnName;
use crate::value::Value;

/// A query execution plan.
#[derive(Debug, Clone)]
pub enum QueryPlan {
    /// Point lookup: WHERE pk = value
    PointLookup {
        /// Table to query.
        table_id: TableId,
        /// Table name (for error messages).
        table_name: String,
        /// Encoded primary key.
        key: Key,
        /// Column indices to project (empty = all columns).
        columns: Vec<usize>,
        /// Column names to return.
        column_names: Vec<ColumnName>,
    },

    /// Range scan on primary key.
    RangeScan {
        /// Table to query.
        table_id: TableId,
        /// Table name (for error messages).
        table_name: String,
        /// Start bound (inclusive/exclusive/unbounded).
        start: Bound<Key>,
        /// End bound (inclusive/exclusive/unbounded).
        end: Bound<Key>,
        /// Additional filter to apply after fetching.
        filter: Option<Filter>,
        /// Maximum rows to return.
        limit: Option<usize>,
        /// Sort order.
        order: ScanOrder,
        /// Column indices to project (empty = all columns).
        columns: Vec<usize>,
        /// Column names to return.
        column_names: Vec<ColumnName>,
    },

    /// Index scan on a secondary index.
    IndexScan {
        /// Table to query.
        table_id: TableId,
        /// Table name (for error messages).
        table_name: String,
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
        /// Sort order.
        order: ScanOrder,
        /// Column indices to project (empty = all columns).
        columns: Vec<usize>,
        /// Column names to return.
        column_names: Vec<ColumnName>,
    },

    /// Full table scan with optional filter.
    TableScan {
        /// Table to query.
        table_id: TableId,
        /// Table name (for error messages).
        table_name: String,
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
        /// Table to query.
        table_id: TableId,
        /// Table name (for error messages).
        table_name: String,
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
    },
}

impl QueryPlan {
    /// Returns the column names this plan will return.
    pub fn column_names(&self) -> &[ColumnName] {
        match self {
            QueryPlan::PointLookup { column_names, .. }
            | QueryPlan::RangeScan { column_names, .. }
            | QueryPlan::IndexScan { column_names, .. }
            | QueryPlan::TableScan { column_names, .. }
            | QueryPlan::Aggregate { column_names, .. } => column_names,
        }
    }

    /// Returns the column indices to project.
    #[allow(dead_code)]
    pub fn column_indices(&self) -> &[usize] {
        match self {
            QueryPlan::PointLookup { columns, .. }
            | QueryPlan::RangeScan { columns, .. }
            | QueryPlan::IndexScan { columns, .. }
            | QueryPlan::TableScan { columns, .. } => columns,
            QueryPlan::Aggregate { group_by_cols, .. } => group_by_cols,
        }
    }

    /// Returns the table name.
    pub fn table_name(&self) -> &str {
        match self {
            QueryPlan::PointLookup { table_name, .. }
            | QueryPlan::RangeScan { table_name, .. }
            | QueryPlan::IndexScan { table_name, .. }
            | QueryPlan::TableScan { table_name, .. }
            | QueryPlan::Aggregate { table_name, .. } => table_name,
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
            FilterOp::In(values) => values.contains(cell),
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
fn matches_like_pattern(text: &str, pattern: &str) -> bool {
    debug_assert!(!pattern.is_empty(), "LIKE pattern must not be empty");
    let text_chars: Vec<char> = text.chars().collect();
    let pattern_chars: Vec<char> = pattern.chars().collect();

    matches_like_impl(&text_chars, &pattern_chars, 0, 0)
}

fn matches_like_impl(text: &[char], pattern: &[char], t_idx: usize, p_idx: usize) -> bool {
    // End of pattern
    if p_idx >= pattern.len() {
        return t_idx >= text.len();
    }

    let p_char = pattern[p_idx];

    // Handle escape sequences
    if p_char == '\\' && p_idx + 1 < pattern.len() {
        let next_char = pattern[p_idx + 1];
        if next_char == '%' || next_char == '_' {
            // Escaped special character - treat as literal
            if t_idx < text.len() && text[t_idx] == next_char {
                return matches_like_impl(text, pattern, t_idx + 1, p_idx + 2);
            }
            return false;
        }
    }

    // Handle wildcards
    match p_char {
        '%' => {
            // % matches zero or more characters
            // Try matching zero characters first, then one, two, etc.
            for i in t_idx..=text.len() {
                if matches_like_impl(text, pattern, i, p_idx + 1) {
                    return true;
                }
            }
            false
        }
        '_' => {
            // _ matches exactly one character
            if t_idx < text.len() {
                matches_like_impl(text, pattern, t_idx + 1, p_idx + 1)
            } else {
                false
            }
        }
        c => {
            // Literal character match
            if t_idx < text.len() && text[t_idx] == c {
                matches_like_impl(text, pattern, t_idx + 1, p_idx + 1)
            } else {
                false
            }
        }
    }
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
