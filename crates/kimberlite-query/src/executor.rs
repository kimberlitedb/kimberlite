//! Query executor: executes query plans against a projection store.

#![allow(clippy::ref_option)]
#![allow(clippy::trivially_copy_pass_by_ref)]
#![allow(clippy::items_after_statements)]

use std::cmp::Ordering;
use std::ops::Bound;

// ============================================================================
// Query Execution Constants
// ============================================================================

/// Scan buffer multiplier when ORDER BY is present (needs extra buffer for sorting).
///
/// **Rationale**: Client-side sorting requires loading all candidate rows before
/// applying LIMIT. We over-fetch by 10x to handle common cases where the ORDER BY
/// columns have high cardinality, while still bounding memory usage.
const SCAN_LIMIT_MULTIPLIER_WITH_SORT: usize = 10;

/// Scan buffer multiplier without ORDER BY (minimal buffering).
///
/// **Rationale**: Without sorting, we can stream results and apply LIMIT incrementally.
/// We fetch 2x the limit to handle edge cases with deleted rows or MVCC conflicts.
const SCAN_LIMIT_MULTIPLIER_NO_SORT: usize = 2;

/// Default scan limit when no LIMIT clause is specified.
///
/// **Rationale**: Prevents unbounded memory allocation for large tables.
/// Set to 10K based on:
/// - Avg row size ~1KB → ~10MB memory footprint
/// - p99 query latency < 50ms for 10K row scan
/// - Sufficient for most analytical queries
const DEFAULT_SCAN_LIMIT: usize = 10_000;

/// Maximum number of aggregates per query.
///
/// **Rationale**: Prevents `DoS` via memory exhaustion.
/// Each aggregate maintains state (sum, count, min, max) ≈ 64 bytes per group.
/// 100 aggregates × 1000 groups = ~6.4MB state, which is reasonable.
const MAX_AGGREGATES_PER_QUERY: usize = 100;

use bytes::Bytes;
use kimberlite_store::{Key, ProjectionStore, TableId};
use kimberlite_types::Offset;

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

/// Executes an index scan query.
#[allow(clippy::too_many_arguments)]
fn execute_index_scan<S: ProjectionStore>(
    store: &mut S,
    metadata: &crate::plan::TableMetadata,
    index_id: u64,
    start: &Bound<Key>,
    end: &Bound<Key>,
    filter: &Option<crate::plan::Filter>,
    limit: &Option<usize>,
    order: &ScanOrder,
    order_by: &Option<crate::plan::SortSpec>,
    columns: &[usize],
    column_names: &[ColumnName],
    position: Option<Offset>,
) -> Result<QueryResult> {
    let (start_key, end_key) = bounds_to_range(start, end);

    // Calculate scan limit based on whether client-side sorting is needed
    let scan_limit = if order_by.is_some() {
        limit
            .map(|l| l.saturating_mul(SCAN_LIMIT_MULTIPLIER_WITH_SORT))
            .unwrap_or(DEFAULT_SCAN_LIMIT)
    } else {
        limit
            .map(|l| l.saturating_mul(SCAN_LIMIT_MULTIPLIER_NO_SORT))
            .unwrap_or(DEFAULT_SCAN_LIMIT)
    };

    // Postcondition: scan limit must be positive
    debug_assert!(scan_limit > 0, "scan_limit must be positive");

    // Calculate index table ID using hash to avoid overflow
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    metadata.table_id.as_u64().hash(&mut hasher);
    index_id.hash(&mut hasher);
    let index_table_id = TableId::new(hasher.finish());

    // Scan the index table to get composite keys
    let index_pairs = match position {
        Some(pos) => store.scan_at(index_table_id, start_key..end_key, scan_limit, pos)?,
        None => store.scan(index_table_id, start_key..end_key, scan_limit)?,
    };

    let mut full_rows = Vec::new();
    let index_iter: Box<dyn Iterator<Item = &(Key, Bytes)>> = match order {
        ScanOrder::Ascending => Box::new(index_pairs.iter()),
        ScanOrder::Descending => Box::new(index_pairs.iter().rev()),
    };

    for (index_key, _) in index_iter {
        // Extract primary key from the composite index key
        let pk_key = extract_pk_from_index_key(index_key, metadata);

        // Fetch the actual row from the base table
        let bytes_opt = match position {
            Some(pos) => store.get_at(metadata.table_id, &pk_key, pos)?,
            None => store.get(metadata.table_id, &pk_key)?,
        };
        if let Some(bytes) = bytes_opt {
            let full_row = decode_row(&bytes, metadata)?;

            // Apply filter
            if let Some(f) = filter {
                if !f.matches(&full_row) {
                    continue;
                }
            }

            full_rows.push(full_row);

            // When client-side sorting is needed, don't apply limit during scan
            if order_by.is_none() {
                if let Some(lim) = limit {
                    if full_rows.len() >= *lim {
                        break;
                    }
                }
            }
        }
    }

    // Apply client-side sorting if needed (on full rows before projection)
    if let Some(sort_spec) = order_by {
        sort_rows(&mut full_rows, sort_spec);
    }

    // Apply limit after sorting
    if let Some(lim) = limit {
        full_rows.truncate(*lim);
    }

    // Project columns after sorting and limiting
    let rows: Vec<Row> = full_rows
        .iter()
        .map(|full_row| project_row(full_row, columns))
        .collect();

    Ok(QueryResult {
        columns: column_names.to_vec(),
        rows,
    })
}

/// Executes a table scan query.
#[allow(clippy::too_many_arguments)]
fn execute_table_scan<S: ProjectionStore>(
    store: &mut S,
    metadata: &crate::plan::TableMetadata,
    filter: &Option<crate::plan::Filter>,
    limit: &Option<usize>,
    order: &Option<SortSpec>,
    columns: &[usize],
    column_names: &[ColumnName],
    position: Option<Offset>,
) -> Result<QueryResult> {
    // Scan entire table
    let scan_limit = limit.map(|l| l * 10).unwrap_or(100_000);
    let pairs = match position {
        Some(pos) => store.scan_at(metadata.table_id, Key::min()..Key::max(), scan_limit, pos)?,
        None => store.scan(metadata.table_id, Key::min()..Key::max(), scan_limit)?,
    };

    let mut full_rows = Vec::new();

    for (_, bytes) in &pairs {
        let full_row = decode_row(bytes, metadata)?;

        // Apply filter
        if let Some(f) = filter {
            if !f.matches(&full_row) {
                continue;
            }
        }

        full_rows.push(full_row);
    }

    // Apply sort on full rows (before projection)
    if let Some(sort_spec) = order {
        sort_rows(&mut full_rows, sort_spec);
    }

    // Apply limit
    if let Some(lim) = limit {
        full_rows.truncate(*lim);
    }

    // Project columns after sorting and limiting
    let rows: Vec<Row> = full_rows
        .iter()
        .map(|full_row| project_row(full_row, columns))
        .collect();

    Ok(QueryResult {
        columns: column_names.to_vec(),
        rows,
    })
}

/// Executes a range scan query.
#[allow(clippy::too_many_arguments)]
fn execute_range_scan<S: ProjectionStore>(
    store: &mut S,
    metadata: &crate::plan::TableMetadata,
    start: &Bound<Key>,
    end: &Bound<Key>,
    filter: &Option<crate::plan::Filter>,
    limit: &Option<usize>,
    order: &ScanOrder,
    order_by: &Option<crate::plan::SortSpec>,
    columns: &[usize],
    column_names: &[ColumnName],
    position: Option<Offset>,
) -> Result<QueryResult> {
    let (start_key, end_key) = bounds_to_range(start, end);

    // Calculate scan limit based on whether client-side sorting is needed
    let scan_limit = if order_by.is_some() {
        limit
            .map(|l| l.saturating_mul(SCAN_LIMIT_MULTIPLIER_WITH_SORT))
            .unwrap_or(DEFAULT_SCAN_LIMIT)
    } else {
        limit
            .map(|l| l.saturating_mul(SCAN_LIMIT_MULTIPLIER_NO_SORT))
            .unwrap_or(DEFAULT_SCAN_LIMIT)
    };

    // Postcondition: scan limit must be positive
    debug_assert!(scan_limit > 0, "scan_limit must be positive");

    let pairs = match position {
        Some(pos) => store.scan_at(metadata.table_id, start_key..end_key, scan_limit, pos)?,
        None => store.scan(metadata.table_id, start_key..end_key, scan_limit)?,
    };

    let mut full_rows = Vec::new();
    let row_iter: Box<dyn Iterator<Item = &(Key, Bytes)>> = match order {
        ScanOrder::Ascending => Box::new(pairs.iter()),
        ScanOrder::Descending => Box::new(pairs.iter().rev()),
    };

    for (_, bytes) in row_iter {
        let full_row = decode_row(bytes, metadata)?;

        // Apply filter
        if let Some(f) = filter {
            if !f.matches(&full_row) {
                continue;
            }
        }

        full_rows.push(full_row);

        // When client-side sorting is needed, don't apply limit during scan
        if order_by.is_none() {
            if let Some(lim) = limit {
                if full_rows.len() >= *lim {
                    break;
                }
            }
        }
    }

    // Apply client-side sorting if needed (on full rows before projection)
    if let Some(sort_spec) = order_by {
        sort_rows(&mut full_rows, sort_spec);
    }

    // Apply limit after sorting
    if let Some(lim) = limit {
        full_rows.truncate(*lim);
    }

    // Project columns after sorting and limiting
    let rows: Vec<Row> = full_rows
        .iter()
        .map(|full_row| project_row(full_row, columns))
        .collect();

    Ok(QueryResult {
        columns: column_names.to_vec(),
        rows,
    })
}

/// Executes a point lookup query.
fn execute_point_lookup<S: ProjectionStore>(
    store: &mut S,
    metadata: &crate::plan::TableMetadata,
    key: &Key,
    columns: &[usize],
    column_names: &[ColumnName],
    position: Option<Offset>,
) -> Result<QueryResult> {
    let result = match position {
        Some(pos) => store.get_at(metadata.table_id, key, pos)?,
        None => store.get(metadata.table_id, key)?,
    };
    match result {
        Some(bytes) => {
            let row = decode_and_project(&bytes, columns, metadata)?;
            Ok(QueryResult {
                columns: column_names.to_vec(),
                rows: vec![row],
            })
        }
        None => Ok(QueryResult::empty(column_names.to_vec())),
    }
}

/// Internal execution function that handles both current and point-in-time queries.
#[allow(clippy::too_many_lines)]
fn execute_internal<S: ProjectionStore>(
    store: &mut S,
    plan: &QueryPlan,
    _table_def: &TableDef, // Kept for API compatibility, but metadata is now in plans
    position: Option<Offset>,
) -> Result<QueryResult> {
    match plan {
        QueryPlan::PointLookup {
            metadata,
            key,
            columns,
            column_names,
        } => execute_point_lookup(store, metadata, key, columns, column_names, position),

        QueryPlan::RangeScan {
            metadata,
            start,
            end,
            filter,
            limit,
            order,
            order_by,
            columns,
            column_names,
        } => execute_range_scan(
            store,
            metadata,
            start,
            end,
            filter,
            limit,
            order,
            order_by,
            columns,
            column_names,
            position,
        ),

        QueryPlan::IndexScan {
            metadata,
            index_id,
            start,
            end,
            filter,
            limit,
            order,
            order_by,
            columns,
            column_names,
            ..
        } => execute_index_scan(
            store,
            metadata,
            *index_id,
            start,
            end,
            filter,
            limit,
            order,
            order_by,
            columns,
            column_names,
            position,
        ),

        QueryPlan::TableScan {
            metadata,
            filter,
            limit,
            order,
            columns,
            column_names,
        } => execute_table_scan(
            store,
            metadata,
            filter,
            limit,
            order,
            columns,
            column_names,
            position,
        ),

        QueryPlan::Aggregate {
            metadata,
            source,
            group_by_cols,
            group_by_names: _,
            aggregates,
            column_names,
            having,
        } => execute_aggregate(
            store,
            source,
            group_by_cols,
            aggregates,
            column_names,
            metadata,
            having,
            position,
        ),

        QueryPlan::Join {
            join_type,
            left,
            right,
            on_conditions,
            columns,
            column_names,
        } => execute_join(
            store,
            join_type,
            left,
            right,
            on_conditions,
            columns,
            column_names,
            position,
        ),

        QueryPlan::Materialize {
            source,
            filter,
            case_columns,
            order,
            limit,
            column_names,
        } => execute_materialize(
            store,
            source,
            filter,
            case_columns,
            order,
            limit,
            column_names,
            position,
        ),
    }
}

/// Executes a Materialize plan: filter, compute CASE columns, sort, and limit.
#[allow(clippy::too_many_arguments)]
fn execute_materialize<S: ProjectionStore>(
    store: &mut S,
    source: &QueryPlan,
    filter: &Option<crate::plan::Filter>,
    case_columns: &[crate::plan::CaseColumnDef],
    order: &Option<SortSpec>,
    limit: &Option<usize>,
    column_names: &[ColumnName],
    position: Option<Offset>,
) -> Result<QueryResult> {
    // Execute the source plan (e.g., the Join node)
    let dummy_def = TableDef {
        table_id: kimberlite_store::TableId::from(0u64),
        columns: vec![],
        primary_key: vec![],
        indexes: vec![],
    };
    let mut source_result = execute_internal(store, source, &dummy_def, position)?;

    // 1. Apply WHERE filter
    if let Some(f) = filter {
        source_result.rows.retain(|row| f.matches(row));
    }

    // 2. Evaluate CASE WHEN computed columns and append to each row
    if !case_columns.is_empty() {
        for row in &mut source_result.rows {
            for case_col in case_columns {
                let val = evaluate_case_column(case_col, row);
                row.push(val);
            }
        }
    }

    // 3. Apply ORDER BY (client-side sort)
    if let Some(spec) = order {
        sort_rows(&mut source_result.rows, spec);
    }

    // 4. Apply LIMIT
    if let Some(n) = limit {
        source_result.rows.truncate(*n);
    }

    // Return with the declared output column names
    Ok(QueryResult {
        columns: column_names.to_vec(),
        rows: source_result.rows,
    })
}

/// Evaluates a CASE WHEN computed column against a row, returning the result value.
fn evaluate_case_column(case_col: &crate::plan::CaseColumnDef, row: &[Value]) -> Value {
    for clause in &case_col.when_clauses {
        if clause.condition.matches(row) {
            return clause.result.clone();
        }
    }
    case_col.else_value.clone()
}

/// Executes a query plan against the current store state.
pub fn execute<S: ProjectionStore>(
    store: &mut S,
    plan: &QueryPlan,
    table_def: &TableDef,
) -> Result<QueryResult> {
    execute_internal(store, plan, table_def, None)
}

/// Executes a query plan at a specific log position (point-in-time query).
pub fn execute_at<S: ProjectionStore>(
    store: &mut S,
    plan: &QueryPlan,
    table_def: &TableDef,
    position: Offset,
) -> Result<QueryResult> {
    execute_internal(store, plan, table_def, Some(position))
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

/// Extracts the primary key from a composite index key.
///
/// Index keys are structured as: [`index_column_values`...][primary_key_values...]
/// This function strips the index column values and returns only the primary key portion.
///
/// # Assertions
/// - Index key must be longer than the number of index columns
/// - Primary key columns must be non-empty
fn extract_pk_from_index_key(index_key: &Key, metadata: &crate::plan::TableMetadata) -> Key {
    use crate::key_encoder::{decode_key, encode_key};

    // Decode the full composite key to get all values
    let all_values = decode_key(index_key);

    // Get the number of primary key columns
    let pk_count = metadata.primary_key.len();

    // Assertions
    debug_assert!(pk_count > 0, "primary key columns must be non-empty");
    debug_assert!(
        all_values.len() >= pk_count,
        "index key must contain at least the primary key values"
    );

    // Extract the last pk_count values (the primary key)
    // Index key format: [index_col1, index_col2, ..., pk_col1, pk_col2, ...]
    let pk_values: Vec<Value> = all_values
        .iter()
        .skip(all_values.len() - pk_count)
        .cloned()
        .collect();

    debug_assert_eq!(
        pk_values.len(),
        pk_count,
        "extracted primary key must have correct number of columns"
    );

    // Re-encode as a key
    encode_key(&pk_values)
}

/// Decodes a JSON row to values using embedded table metadata.
fn decode_row(bytes: &Bytes, metadata: &crate::plan::TableMetadata) -> Result<Row> {
    let json: serde_json::Value = serde_json::from_slice(bytes)?;

    let obj = json.as_object().ok_or_else(|| QueryError::TypeMismatch {
        expected: "object".to_string(),
        actual: format!("{json:?}"),
    })?;

    let mut row = Vec::with_capacity(metadata.columns.len());

    for col_def in &metadata.columns {
        let col_name = col_def.name.as_str();
        let json_val = obj.get(col_name).unwrap_or(&serde_json::Value::Null);
        let value = Value::from_json(json_val, col_def.data_type)?;
        row.push(value);
    }

    Ok(row)
}

/// Decodes a JSON row and projects columns (deprecated - use decode_row + project_row).
fn decode_and_project(
    bytes: &Bytes,
    columns: &[usize],
    metadata: &crate::plan::TableMetadata,
) -> Result<Row> {
    let full_row = decode_row(bytes, metadata)?;
    Ok(project_row(&full_row, columns))
}

/// Projects a row to selected columns.
fn project_row(full_row: &[Value], columns: &[usize]) -> Row {
    // Precondition: column indices must be valid
    debug_assert!(
        columns.iter().all(|&idx| idx < full_row.len()),
        "column index out of bounds: columns={:?}, row_len={}",
        columns,
        full_row.len()
    );

    if columns.is_empty() {
        // Empty columns means all columns
        return full_row.to_vec();
    }

    let projected: Vec<Value> = columns
        .iter()
        .map(|&idx| {
            full_row.get(idx).cloned().unwrap_or_else(|| {
                // This should never happen due to precondition
                panic!(
                    "column index {} out of bounds (row len {})",
                    idx,
                    full_row.len()
                );
            })
        })
        .collect();

    // Postcondition: result has correct length
    debug_assert_eq!(
        projected.len(),
        columns.len(),
        "projected row length mismatch"
    );

    projected
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

/// Executes a nested loop join between two tables.
#[allow(clippy::too_many_arguments)]
/// Evaluates all join conditions on a concatenated row.
///
/// All conditions must be true for the join to match (AND semantics).
fn evaluate_join_conditions(row: &[Value], conditions: &[crate::plan::JoinCondition]) -> bool {
    use crate::plan::JoinOp;

    conditions.iter().all(|cond| {
        // Get values at left and right column indices
        let left_val = row.get(cond.left_col_idx);
        let right_val = row.get(cond.right_col_idx);

        // Both values must exist
        if left_val.is_none() || right_val.is_none() {
            return false;
        }

        let left_val = left_val.unwrap();
        let right_val = right_val.unwrap();

        // Apply comparison operator
        match cond.op {
            JoinOp::Eq => left_val == right_val,
            JoinOp::Lt => left_val.compare(right_val) == Some(std::cmp::Ordering::Less),
            JoinOp::Le => matches!(
                left_val.compare(right_val),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            ),
            JoinOp::Gt => left_val.compare(right_val) == Some(std::cmp::Ordering::Greater),
            JoinOp::Ge => matches!(
                left_val.compare(right_val),
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            ),
        }
    })
}

fn execute_join<S: ProjectionStore>(
    store: &mut S,
    join_type: &crate::parser::JoinType,
    left: &QueryPlan,
    right: &QueryPlan,
    on_conditions: &[crate::plan::JoinCondition],
    _columns: &[usize],
    column_names: &[ColumnName],
    position: Option<Offset>,
) -> Result<QueryResult> {
    // Get metadata from child plans for dummy TableDef (each child has its own metadata embedded)
    let left_metadata = left.metadata().ok_or_else(|| {
        QueryError::UnsupportedFeature("JOIN child plan missing metadata".to_string())
    })?;
    let right_metadata = right.metadata().ok_or_else(|| {
        QueryError::UnsupportedFeature("JOIN child plan missing metadata".to_string())
    })?;

    // Create dummy table defs for execute_internal (metadata in plans will be used)
    let left_table_def = TableDef {
        table_id: left_metadata.table_id,
        columns: left_metadata.columns.clone(),
        primary_key: left_metadata.primary_key.clone(),
        indexes: vec![], // Not needed for JOIN execution
    };
    let right_table_def = TableDef {
        table_id: right_metadata.table_id,
        columns: right_metadata.columns.clone(),
        primary_key: right_metadata.primary_key.clone(),
        indexes: vec![], // Not needed for JOIN execution
    };

    // Execute left and right subqueries
    let left_result = execute_internal(store, left, &left_table_def, position)?;
    let right_result = execute_internal(store, right, &right_table_def, position)?;

    let mut output_rows = Vec::new();

    match join_type {
        crate::parser::JoinType::Inner => {
            // INNER JOIN: only rows that match the join conditions
            for left_row in &left_result.rows {
                for right_row in &right_result.rows {
                    // Build concatenated row: [left_cols..., right_cols...]
                    let combined_row: Vec<Value> =
                        left_row.iter().chain(right_row.iter()).cloned().collect();

                    // Evaluate all join conditions
                    if evaluate_join_conditions(&combined_row, on_conditions) {
                        output_rows.push(combined_row);
                    }
                }
            }
        }
        crate::parser::JoinType::Left => {
            // LEFT JOIN: include left row with NULLs if no match
            for left_row in &left_result.rows {
                let mut matched = false;
                for right_row in &right_result.rows {
                    // Build concatenated row
                    let combined_row: Vec<Value> =
                        left_row.iter().chain(right_row.iter()).cloned().collect();

                    // Evaluate all join conditions
                    if evaluate_join_conditions(&combined_row, on_conditions) {
                        output_rows.push(combined_row);
                        matched = true;
                    }
                }

                // LEFT JOIN: include left row with NULLs if no match
                if !matched {
                    let right_nulls = vec![Value::Null; right_result.columns.len()];
                    let combined_row: Vec<Value> = left_row
                        .iter()
                        .cloned()
                        .chain(right_nulls.into_iter())
                        .collect();
                    output_rows.push(combined_row);
                }
            }
        }
    }

    Ok(QueryResult {
        columns: column_names.to_vec(),
        rows: output_rows,
    })
}

/// Executes an aggregate query with optional grouping.
fn execute_aggregate<S: ProjectionStore>(
    store: &mut S,
    source: &QueryPlan,
    group_by_cols: &[usize],
    aggregates: &[crate::parser::AggregateFunction],
    column_names: &[ColumnName],
    metadata: &crate::plan::TableMetadata,
    having: &[crate::parser::HavingCondition],
    position: Option<Offset>,
) -> Result<QueryResult> {
    use std::collections::HashMap;

    // Execute source plan to get all rows
    // Pass metadata as TableDef for API compatibility (it will be ignored in child plans)
    let dummy_table_def = TableDef {
        table_id: metadata.table_id,
        columns: metadata.columns.clone(),
        primary_key: metadata.primary_key.clone(),
        indexes: vec![],
    };
    let source_result = execute_internal(store, source, &dummy_table_def, position)?;

    // Build aggregate state grouped by key
    let mut groups: HashMap<Vec<Value>, AggregateState> = HashMap::new();

    for row in source_result.rows {
        // Extract group key (values from GROUP BY columns)
        let group_key: Vec<Value> = if group_by_cols.is_empty() {
            // No GROUP BY - all rows in one group
            vec![]
        } else {
            group_by_cols
                .iter()
                .map(|&idx| row.get(idx).cloned().unwrap_or(Value::Null))
                .collect()
        };

        // Update aggregates for this group
        let state = groups.entry(group_key).or_insert_with(AggregateState::new);
        state.update(&row, aggregates, metadata)?;
    }

    // Convert groups to result rows
    let group_by_count = group_by_cols.len();
    let mut result_rows = Vec::new();
    for (group_key, state) in groups {
        let agg_values = state.finalize(aggregates);

        // Apply HAVING filter: check each condition against aggregate results
        if !having.is_empty() && !evaluate_having(having, aggregates, &agg_values, group_by_count) {
            continue;
        }

        let mut result_row = group_key; // Start with GROUP BY columns
        result_row.extend(agg_values); // Add aggregate results
        result_rows.push(result_row);
    }

    // If no groups and no GROUP BY, return one row with global aggregates
    if result_rows.is_empty() && group_by_cols.is_empty() && having.is_empty() {
        let state = AggregateState::new();
        let agg_values = state.finalize(aggregates);
        result_rows.push(agg_values);
    }

    Ok(QueryResult {
        columns: column_names.to_vec(),
        rows: result_rows,
    })
}

/// Evaluates HAVING conditions against aggregate results for a group.
///
/// Returns true if the group passes all HAVING conditions.
fn evaluate_having(
    having: &[crate::parser::HavingCondition],
    aggregates: &[crate::parser::AggregateFunction],
    agg_values: &[Value],
    _group_by_count: usize,
) -> bool {
    having.iter().all(|condition| match condition {
        crate::parser::HavingCondition::AggregateComparison {
            aggregate,
            op,
            value,
        } => {
            // Find the index of this aggregate in the aggregates list
            let agg_idx = aggregates.iter().position(|a| a == aggregate);
            let Some(idx) = agg_idx else {
                return false;
            };
            let Some(agg_value) = agg_values.get(idx) else {
                return false;
            };

            // Compare using the specified operator
            match op {
                crate::parser::HavingOp::Eq => agg_value == value,
                crate::parser::HavingOp::Lt => agg_value.compare(value) == Some(Ordering::Less),
                crate::parser::HavingOp::Le => matches!(
                    agg_value.compare(value),
                    Some(Ordering::Less | Ordering::Equal)
                ),
                crate::parser::HavingOp::Gt => agg_value.compare(value) == Some(Ordering::Greater),
                crate::parser::HavingOp::Ge => matches!(
                    agg_value.compare(value),
                    Some(Ordering::Greater | Ordering::Equal)
                ),
            }
        }
    })
}

/// State for computing aggregates over a group of rows.
#[derive(Debug, Clone)]
struct AggregateState {
    count: i64,
    non_null_counts: Vec<i64>, // For COUNT(col) - tracks non-NULL values per aggregate
    sums: Vec<Option<Value>>,
    mins: Vec<Option<Value>>,
    maxs: Vec<Option<Value>>,
}

impl AggregateState {
    fn new() -> Self {
        Self {
            count: 0,
            non_null_counts: Vec::new(),
            sums: Vec::new(),
            mins: Vec::new(),
            maxs: Vec::new(),
        }
    }

    fn update(
        &mut self,
        row: &[Value],
        aggregates: &[crate::parser::AggregateFunction],
        metadata: &crate::plan::TableMetadata,
    ) -> Result<()> {
        // Precondition: row must have at least one column
        debug_assert!(!row.is_empty(), "row must have at least one column");

        // Precondition: enforce maximum aggregates limit to prevent DoS
        // Note: aggregates can be empty for DISTINCT queries (deduplication only)
        assert!(
            aggregates.len() <= MAX_AGGREGATES_PER_QUERY,
            "too many aggregates ({} > {})",
            aggregates.len(),
            MAX_AGGREGATES_PER_QUERY
        );

        self.count += 1;

        // Ensure vectors are sized
        while self.sums.len() < aggregates.len() {
            self.non_null_counts.push(0);
            self.sums.push(None);
            self.mins.push(None);
            self.maxs.push(None);
        }

        // Invariant: all vectors must be same length after sizing
        debug_assert_eq!(
            self.sums.len(),
            self.non_null_counts.len(),
            "aggregate state vectors out of sync"
        );
        debug_assert_eq!(self.sums.len(), self.mins.len());
        debug_assert_eq!(self.sums.len(), self.maxs.len());

        // Helper to find column index
        let find_col_idx = |col: &ColumnName| -> usize {
            metadata
                .columns
                .iter()
                .position(|c| &c.name == col)
                .unwrap_or(0)
        };

        for (i, agg) in aggregates.iter().enumerate() {
            match agg {
                crate::parser::AggregateFunction::CountStar => {
                    // Already counted above
                }
                crate::parser::AggregateFunction::Count(col) => {
                    // COUNT(col) counts non-NULL values
                    let col_idx = find_col_idx(col);
                    if let Some(val) = row.get(col_idx) {
                        if !val.is_null() {
                            self.non_null_counts[i] += 1;
                        }
                    }
                }
                crate::parser::AggregateFunction::Sum(col) => {
                    let col_idx = find_col_idx(col);
                    if let Some(val) = row.get(col_idx) {
                        if !val.is_null() {
                            self.sums[i] = Some(add_values(&self.sums[i], val)?);
                        }
                    }
                }
                crate::parser::AggregateFunction::Avg(col) => {
                    // AVG = SUM / COUNT - compute sum here
                    let col_idx = find_col_idx(col);
                    if let Some(val) = row.get(col_idx) {
                        if !val.is_null() {
                            self.sums[i] = Some(add_values(&self.sums[i], val)?);
                        }
                    }
                }
                crate::parser::AggregateFunction::Min(col) => {
                    let col_idx = find_col_idx(col);
                    if let Some(val) = row.get(col_idx) {
                        if !val.is_null() {
                            self.mins[i] = Some(min_value(&self.mins[i], val));
                        }
                    }
                }
                crate::parser::AggregateFunction::Max(col) => {
                    let col_idx = find_col_idx(col);
                    if let Some(val) = row.get(col_idx) {
                        if !val.is_null() {
                            self.maxs[i] = Some(max_value(&self.maxs[i], val));
                        }
                    }
                }
            }
        }

        // Postcondition: state must match aggregate count after update
        debug_assert_eq!(
            self.sums.len(),
            aggregates.len(),
            "aggregate state must match aggregate count after update"
        );

        Ok(())
    }

    fn finalize(&self, aggregates: &[crate::parser::AggregateFunction]) -> Vec<Value> {
        let mut result = Vec::new();

        for (i, agg) in aggregates.iter().enumerate() {
            let value = match agg {
                crate::parser::AggregateFunction::CountStar => Value::BigInt(self.count),
                crate::parser::AggregateFunction::Count(_) => {
                    // Use non-NULL count for COUNT(col)
                    Value::BigInt(self.non_null_counts.get(i).copied().unwrap_or(0))
                }
                crate::parser::AggregateFunction::Sum(_) => self
                    .sums
                    .get(i)
                    .and_then(std::clone::Clone::clone)
                    .unwrap_or(Value::Null),
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
                crate::parser::AggregateFunction::Min(_) => self
                    .mins
                    .get(i)
                    .and_then(std::clone::Clone::clone)
                    .unwrap_or(Value::Null),
                crate::parser::AggregateFunction::Max(_) => self
                    .maxs
                    .get(i)
                    .and_then(std::clone::Clone::clone)
                    .unwrap_or(Value::Null),
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
                expected: format!("{a_val:?}"),
                actual: format!("{b:?}"),
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
#[allow(clippy::cast_precision_loss)]
fn divide_value(val: &Value, count: i64) -> Option<Value> {
    match val {
        Value::BigInt(x) => Some(Value::Real(*x as f64 / count as f64)),
        Value::Integer(x) => Some(Value::Real(f64::from(*x) / count as f64)),
        Value::SmallInt(x) => Some(Value::Real(f64::from(*x) / count as f64)),
        Value::TinyInt(x) => Some(Value::Real(f64::from(*x) / count as f64)),
        Value::Real(x) => Some(Value::Real(x / count as f64)),
        Value::Decimal(x, scale) => {
            // Convert to float for division
            let divisor = 10_i128.pow(u32::from(*scale));
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
