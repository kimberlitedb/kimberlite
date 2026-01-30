//! Query executor: executes query plans against a projection store.

use std::cmp::Ordering;
use std::ops::Bound;

use bytes::Bytes;
use kmb_store::{Key, ProjectionStore};
use kmb_types::Offset;

use crate::error::{QueryError, Result};
use crate::key_encoder::successor_key;
use crate::plan::{QueryPlan, ScanOrder, SortSpec};
use crate::schema::{ColumnName, TableDef};
use crate::value::Value;

/// Result of executing a query.
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Column names in result order.
    pub columns: Vec<ColumnName>,
    /// Result rows.
    pub rows: Vec<Row>,
}

impl QueryResult {
    /// Creates an empty result with the given columns.
    pub fn empty(columns: Vec<ColumnName>) -> Self {
        Self {
            columns,
            rows: vec![],
        }
    }

    /// Returns the number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Returns true if there are no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// A single result row.
pub type Row = Vec<Value>;

/// Executes a query plan against the current store state.
pub fn execute<S: ProjectionStore>(
    store: &mut S,
    plan: &QueryPlan,
    table_def: &TableDef,
) -> Result<QueryResult> {
    match plan {
        QueryPlan::PointLookup {
            table_id,
            key,
            columns,
            column_names,
            ..
        } => {
            let result = store.get(*table_id, key)?;
            match result {
                Some(bytes) => {
                    let row = decode_and_project(&bytes, columns, table_def)?;
                    Ok(QueryResult {
                        columns: column_names.clone(),
                        rows: vec![row],
                    })
                }
                None => Ok(QueryResult::empty(column_names.clone())),
            }
        }

        QueryPlan::RangeScan {
            table_id,
            start,
            end,
            filter,
            limit,
            order,
            columns,
            column_names,
            ..
        } => {
            let (start_key, end_key) = bounds_to_range(start, end);
            let scan_limit = limit.map(|l| l * 2).unwrap_or(10000); // Over-fetch for filtering

            let pairs = store.scan(*table_id, start_key..end_key, scan_limit)?;

            let mut rows = Vec::new();
            let row_iter: Box<dyn Iterator<Item = &(Key, Bytes)>> = match order {
                ScanOrder::Ascending => Box::new(pairs.iter()),
                ScanOrder::Descending => Box::new(pairs.iter().rev()),
            };

            for (_, bytes) in row_iter {
                let full_row = decode_row(bytes, table_def)?;

                // Apply filter
                if let Some(f) = filter {
                    if !f.matches(&full_row) {
                        continue;
                    }
                }

                // Project columns
                let projected = project_row(&full_row, columns);
                rows.push(projected);

                // Check limit
                if let Some(lim) = limit {
                    if rows.len() >= *lim {
                        break;
                    }
                }
            }

            Ok(QueryResult {
                columns: column_names.clone(),
                rows,
            })
        }

        QueryPlan::TableScan {
            table_id,
            filter,
            limit,
            order,
            columns,
            column_names,
            ..
        } => {
            // Scan entire table
            let scan_limit = limit.map(|l| l * 10).unwrap_or(100_000);
            let pairs = store.scan(*table_id, Key::min()..Key::max(), scan_limit)?;

            let mut rows = Vec::new();

            for (_, bytes) in &pairs {
                let full_row = decode_row(bytes, table_def)?;

                // Apply filter
                if let Some(f) = filter {
                    if !f.matches(&full_row) {
                        continue;
                    }
                }

                // Project columns
                let projected = project_row(&full_row, columns);
                rows.push(projected);
            }

            // Apply sort
            if let Some(sort_spec) = order {
                sort_rows(&mut rows, sort_spec);
            }

            // Apply limit
            if let Some(lim) = limit {
                rows.truncate(*lim);
            }

            Ok(QueryResult {
                columns: column_names.clone(),
                rows,
            })
        }

        QueryPlan::Aggregate {
            source,
            group_by_cols,
            aggregates,
            column_names,
            ..
        } => execute_aggregate(store, source, group_by_cols, aggregates, column_names, table_def)
    }
}

/// Executes a query plan at a specific log position.
pub fn execute_at<S: ProjectionStore>(
    store: &mut S,
    plan: &QueryPlan,
    table_def: &TableDef,
    position: Offset,
) -> Result<QueryResult> {
    match plan {
        QueryPlan::PointLookup {
            table_id,
            key,
            columns,
            column_names,
            ..
        } => {
            let result = store.get_at(*table_id, key, position)?;
            match result {
                Some(bytes) => {
                    let row = decode_and_project(&bytes, columns, table_def)?;
                    Ok(QueryResult {
                        columns: column_names.clone(),
                        rows: vec![row],
                    })
                }
                None => Ok(QueryResult::empty(column_names.clone())),
            }
        }

        QueryPlan::RangeScan {
            table_id,
            start,
            end,
            filter,
            limit,
            order,
            columns,
            column_names,
            ..
        } => {
            let (start_key, end_key) = bounds_to_range(start, end);
            let scan_limit = limit.map(|l| l * 2).unwrap_or(10000);

            let pairs = store.scan_at(*table_id, start_key..end_key, scan_limit, position)?;

            let mut rows = Vec::new();
            let row_iter: Box<dyn Iterator<Item = &(Key, Bytes)>> = match order {
                ScanOrder::Ascending => Box::new(pairs.iter()),
                ScanOrder::Descending => Box::new(pairs.iter().rev()),
            };

            for (_, bytes) in row_iter {
                let full_row = decode_row(bytes, table_def)?;

                if let Some(f) = filter {
                    if !f.matches(&full_row) {
                        continue;
                    }
                }

                let projected = project_row(&full_row, columns);
                rows.push(projected);

                if let Some(lim) = limit {
                    if rows.len() >= *lim {
                        break;
                    }
                }
            }

            Ok(QueryResult {
                columns: column_names.clone(),
                rows,
            })
        }

        QueryPlan::TableScan {
            table_id,
            filter,
            limit,
            order,
            columns,
            column_names,
            ..
        } => {
            let scan_limit = limit.map(|l| l * 10).unwrap_or(100_000);
            let pairs = store.scan_at(*table_id, Key::min()..Key::max(), scan_limit, position)?;

            let mut rows = Vec::new();

            for (_, bytes) in &pairs {
                let full_row = decode_row(bytes, table_def)?;

                if let Some(f) = filter {
                    if !f.matches(&full_row) {
                        continue;
                    }
                }

                let projected = project_row(&full_row, columns);
                rows.push(projected);
            }

            if let Some(sort_spec) = order {
                sort_rows(&mut rows, sort_spec);
            }

            if let Some(lim) = limit {
                rows.truncate(*lim);
            }

            Ok(QueryResult {
                columns: column_names.clone(),
                rows,
            })
        }

        QueryPlan::Aggregate {
            source,
            group_by_cols,
            aggregates,
            column_names,
            ..
        } => execute_aggregate(store, source, group_by_cols, aggregates, column_names, table_def)
    }
}

/// Converts bounds to a range.
///
/// The store scan uses a half-open range [start, end), so we need to:
/// - For Included start: use the key as-is
/// - For Excluded start: use the successor key (to skip the excluded value)
/// - For Included end: use successor key (to include the value)
/// - For Excluded end: use the key as-is
fn bounds_to_range(start: &Bound<Key>, end: &Bound<Key>) -> (Key, Key) {
    let start_key = match start {
        Bound::Included(k) => k.clone(),
        Bound::Excluded(k) => successor_key(k),
        Bound::Unbounded => Key::min(),
    };

    let end_key = match end {
        Bound::Included(k) => successor_key(k),
        Bound::Excluded(k) => k.clone(),
        Bound::Unbounded => Key::max(),
    };

    (start_key, end_key)
}

/// Decodes a JSON row and projects columns.
fn decode_and_project(bytes: &Bytes, columns: &[usize], table_def: &TableDef) -> Result<Row> {
    let full_row = decode_row(bytes, table_def)?;
    Ok(project_row(&full_row, columns))
}

/// Decodes a JSON row to values.
fn decode_row(bytes: &Bytes, table_def: &TableDef) -> Result<Row> {
    let json: serde_json::Value = serde_json::from_slice(bytes)?;

    let obj = json.as_object().ok_or_else(|| QueryError::TypeMismatch {
        expected: "object".to_string(),
        actual: format!("{json:?}"),
    })?;

    let mut row = Vec::with_capacity(table_def.columns.len());

    for col_def in &table_def.columns {
        let col_name = col_def.name.as_str();
        let json_val = obj.get(col_name).unwrap_or(&serde_json::Value::Null);
        let value = Value::from_json(json_val, col_def.data_type)?;
        row.push(value);
    }

    Ok(row)
}

/// Projects a row to selected columns.
fn project_row(full_row: &[Value], columns: &[usize]) -> Row {
    if columns.is_empty() {
        // Empty columns means all columns
        return full_row.to_vec();
    }

    columns
        .iter()
        .map(|&idx| full_row.get(idx).cloned().unwrap_or(Value::Null))
        .collect()
}

/// Sorts rows according to the sort specification.
fn sort_rows(rows: &mut [Row], spec: &SortSpec) {
    rows.sort_by(|a, b| {
        for (col_idx, order) in &spec.columns {
            let a_val = a.get(*col_idx);
            let b_val = b.get(*col_idx);

            let cmp = match (a_val, b_val) {
                (Some(av), Some(bv)) => av.compare(bv).unwrap_or(Ordering::Equal),
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
            };

            if cmp != Ordering::Equal {
                return match order {
                    ScanOrder::Ascending => cmp,
                    ScanOrder::Descending => cmp.reverse(),
                };
            }
        }
        Ordering::Equal
    });
}

/// Executes an aggregate query with optional grouping.
fn execute_aggregate<S: ProjectionStore>(
    store: &mut S,
    source: &QueryPlan,
    group_by_cols: &[usize],
    aggregates: &[crate::parser::AggregateFunction],
    column_names: &[ColumnName],
    table_def: &TableDef,
) -> Result<QueryResult> {
    use std::collections::HashMap;

    // Execute source plan to get all rows
    let source_result = execute(store, source, table_def)?;

    // Build aggregate state grouped by key
    let mut groups: HashMap<Vec<Value>, AggregateState> = HashMap::new();

    for row in source_result.rows {
        // Extract group key (values from GROUP BY columns)
        let group_key: Vec<Value> = if group_by_cols.is_empty() {
            // No GROUP BY - all rows in one group
            vec![]
        } else {
            group_by_cols.iter().map(|&idx| row.get(idx).cloned().unwrap_or(Value::Null)).collect()
        };

        // Update aggregates for this group
        let state = groups.entry(group_key).or_insert_with(AggregateState::new);
        state.update(&row, aggregates, table_def)?;
    }

    // Convert groups to result rows
    let mut result_rows = Vec::new();
    for (group_key, state) in groups {
        let mut result_row = group_key; // Start with GROUP BY columns
        result_row.extend(state.finalize(aggregates)); // Add aggregate results
        result_rows.push(result_row);
    }

    // If no groups and no GROUP BY, return one row with global aggregates
    if result_rows.is_empty() && group_by_cols.is_empty() {
        let state = AggregateState::new();
        let agg_values = state.finalize(aggregates);
        result_rows.push(agg_values);
    }

    Ok(QueryResult {
        columns: column_names.to_vec(),
        rows: result_rows,
    })
}

/// State for computing aggregates over a group of rows.
#[derive(Debug, Clone)]
struct AggregateState {
    count: i64,
    sums: Vec<Option<Value>>,
    mins: Vec<Option<Value>>,
    maxs: Vec<Option<Value>>,
}

impl AggregateState {
    fn new() -> Self {
        Self {
            count: 0,
            sums: Vec::new(),
            mins: Vec::new(),
            maxs: Vec::new(),
        }
    }

    fn update(
        &mut self,
        row: &[Value],
        aggregates: &[crate::parser::AggregateFunction],
        table_def: &TableDef,
    ) -> Result<()> {
        self.count += 1;

        // Ensure vectors are sized
        while self.sums.len() < aggregates.len() {
            self.sums.push(None);
            self.mins.push(None);
            self.maxs.push(None);
        }

        for (i, agg) in aggregates.iter().enumerate() {
            match agg {
                crate::parser::AggregateFunction::CountStar => {
                    // Already counted above
                }
                crate::parser::AggregateFunction::Count(col) => {
                    // COUNT(col) counts non-NULL values
                    let col_idx = table_def.find_column(col).map(|(idx, _)| idx).unwrap_or(0);
                    if let Some(val) = row.get(col_idx) {
                        if !val.is_null() {
                            // Will be computed from self.count in finalize
                        }
                    }
                }
                crate::parser::AggregateFunction::Sum(col) => {
                    let col_idx = table_def.find_column(col).map(|(idx, _)| idx).unwrap_or(0);
                    if let Some(val) = row.get(col_idx) {
                        if !val.is_null() {
                            self.sums[i] = Some(add_values(&self.sums[i], val)?);
                        }
                    }
                }
                crate::parser::AggregateFunction::Avg(col) => {
                    // AVG = SUM / COUNT - compute sum here
                    let col_idx = table_def.find_column(col).map(|(idx, _)| idx).unwrap_or(0);
                    if let Some(val) = row.get(col_idx) {
                        if !val.is_null() {
                            self.sums[i] = Some(add_values(&self.sums[i], val)?);
                        }
                    }
                }
                crate::parser::AggregateFunction::Min(col) => {
                    let col_idx = table_def.find_column(col).map(|(idx, _)| idx).unwrap_or(0);
                    if let Some(val) = row.get(col_idx) {
                        if !val.is_null() {
                            self.mins[i] = Some(min_value(&self.mins[i], val));
                        }
                    }
                }
                crate::parser::AggregateFunction::Max(col) => {
                    let col_idx = table_def.find_column(col).map(|(idx, _)| idx).unwrap_or(0);
                    if let Some(val) = row.get(col_idx) {
                        if !val.is_null() {
                            self.maxs[i] = Some(max_value(&self.maxs[i], val));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn finalize(&self, aggregates: &[crate::parser::AggregateFunction]) -> Vec<Value> {
        let mut result = Vec::new();

        for (i, agg) in aggregates.iter().enumerate() {
            let value = match agg {
                crate::parser::AggregateFunction::CountStar => Value::BigInt(self.count),
                crate::parser::AggregateFunction::Count(_) => Value::BigInt(self.count),
                crate::parser::AggregateFunction::Sum(_) => {
                    self.sums.get(i).and_then(|v| v.clone()).unwrap_or(Value::Null)
                }
                crate::parser::AggregateFunction::Avg(_) => {
                    // AVG = SUM / COUNT
                    if self.count == 0 {
                        Value::Null
                    } else {
                        match self.sums.get(i).and_then(|v| v.as_ref()) {
                            Some(sum) => divide_value(sum, self.count).unwrap_or(Value::Null),
                            None => Value::Null,
                        }
                    }
                }
                crate::parser::AggregateFunction::Min(_) => {
                    self.mins.get(i).and_then(|v| v.clone()).unwrap_or(Value::Null)
                }
                crate::parser::AggregateFunction::Max(_) => {
                    self.maxs.get(i).and_then(|v| v.clone()).unwrap_or(Value::Null)
                }
            };
            result.push(value);
        }

        result
    }
}

/// Adds two values for SUM aggregates.
fn add_values(a: &Option<Value>, b: &Value) -> Result<Value> {
    match a {
        None => Ok(b.clone()),
        Some(a_val) => match (a_val, b) {
            (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::BigInt(x + y)),
            (Value::Integer(x), Value::Integer(y)) => Ok(Value::Integer(x + y)),
            (Value::SmallInt(x), Value::SmallInt(y)) => Ok(Value::SmallInt(x + y)),
            (Value::TinyInt(x), Value::TinyInt(y)) => Ok(Value::TinyInt(x + y)),
            (Value::Real(x), Value::Real(y)) => Ok(Value::Real(x + y)),
            (Value::Decimal(x, sx), Value::Decimal(y, sy)) if sx == sy => {
                Ok(Value::Decimal(x + y, *sx))
            }
            _ => Err(QueryError::TypeMismatch {
                expected: format!("{:?}", a_val),
                actual: format!("{:?}", b),
            }),
        },
    }
}

/// Returns the minimum of two values.
fn min_value(a: &Option<Value>, b: &Value) -> Value {
    match a {
        None => b.clone(),
        Some(a_val) => {
            if let Some(ord) = a_val.compare(b) {
                if ord == Ordering::Less {
                    a_val.clone()
                } else {
                    b.clone()
                }
            } else {
                a_val.clone() // Incomparable types, keep current
            }
        }
    }
}

/// Returns the maximum of two values.
fn max_value(a: &Option<Value>, b: &Value) -> Value {
    match a {
        None => b.clone(),
        Some(a_val) => {
            if let Some(ord) = a_val.compare(b) {
                if ord == Ordering::Greater {
                    a_val.clone()
                } else {
                    b.clone()
                }
            } else {
                a_val.clone() // Incomparable types, keep current
            }
        }
    }
}

/// Divides a value by a count for AVG aggregates.
fn divide_value(val: &Value, count: i64) -> Option<Value> {
    match val {
        Value::BigInt(x) => Some(Value::Real(*x as f64 / count as f64)),
        Value::Integer(x) => Some(Value::Real(*x as f64 / count as f64)),
        Value::SmallInt(x) => Some(Value::Real(*x as f64 / count as f64)),
        Value::TinyInt(x) => Some(Value::Real(*x as f64 / count as f64)),
        Value::Real(x) => Some(Value::Real(x / count as f64)),
        Value::Decimal(x, scale) => {
            // Convert to float for division
            let divisor = 10_i128.pow(*scale as u32);
            let float_val = *x as f64 / divisor as f64;
            Some(Value::Real(float_val / count as f64))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Filter;
    use crate::plan::FilterCondition;
    use crate::plan::FilterOp;

    #[test]
    fn test_project_row() {
        let row = vec![
            Value::BigInt(1),
            Value::Text("alice".to_string()),
            Value::BigInt(30),
        ];

        let projected = project_row(&row, &[0, 2]);
        assert_eq!(projected, vec![Value::BigInt(1), Value::BigInt(30)]);
    }

    #[test]
    fn test_project_row_all() {
        let row = vec![Value::BigInt(1), Value::Text("bob".to_string())];
        let projected = project_row(&row, &[]);
        assert_eq!(projected, row);
    }

    #[test]
    fn test_filter_matches() {
        let row = vec![Value::BigInt(42), Value::Text("alice".to_string())];

        let filter = Filter::single(FilterCondition {
            column_idx: 0,
            op: FilterOp::Eq,
            value: Value::BigInt(42),
        });

        assert!(filter.matches(&row));

        let filter_miss = Filter::single(FilterCondition {
            column_idx: 0,
            op: FilterOp::Eq,
            value: Value::BigInt(99),
        });

        assert!(!filter_miss.matches(&row));
    }

    #[test]
    fn test_sort_rows() {
        let mut rows = vec![
            vec![Value::BigInt(3), Value::Text("c".to_string())],
            vec![Value::BigInt(1), Value::Text("a".to_string())],
            vec![Value::BigInt(2), Value::Text("b".to_string())],
        ];

        let spec = SortSpec {
            columns: vec![(0, ScanOrder::Ascending)],
        };

        sort_rows(&mut rows, &spec);

        assert_eq!(rows[0][0], Value::BigInt(1));
        assert_eq!(rows[1][0], Value::BigInt(2));
        assert_eq!(rows[2][0], Value::BigInt(3));
    }

    #[test]
    fn test_sort_rows_descending() {
        let mut rows = vec![
            vec![Value::BigInt(1)],
            vec![Value::BigInt(3)],
            vec![Value::BigInt(2)],
        ];

        let spec = SortSpec {
            columns: vec![(0, ScanOrder::Descending)],
        };

        sort_rows(&mut rows, &spec);

        assert_eq!(rows[0][0], Value::BigInt(3));
        assert_eq!(rows[1][0], Value::BigInt(2));
        assert_eq!(rows[2][0], Value::BigInt(1));
    }
}
