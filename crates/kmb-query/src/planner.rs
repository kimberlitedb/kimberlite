//! Query planner: transforms parsed SQL into execution plans.
//!
//! The planner analyzes predicates to select the optimal access path:
//! - `PointLookup`: When all primary key columns have equality predicates
//! - `RangeScan`: When primary key has range predicates
//! - `TableScan`: Fallback for non-indexed predicates

use std::ops::Bound;

use crate::error::{QueryError, Result};
use crate::key_encoder::{encode_key, successor_key};
use crate::parser::{OrderByClause, ParsedSelect, Predicate, PredicateValue};
use crate::plan::{Filter, FilterCondition, FilterOp, QueryPlan, ScanOrder, SortSpec};
use crate::schema::{ColumnName, Schema, TableDef};
use crate::value::Value;

/// Plans a parsed SELECT statement.
pub fn plan_query(schema: &Schema, parsed: &ParsedSelect, params: &[Value]) -> Result<QueryPlan> {
    // Look up table
    let table_name = parsed.table.clone();
    let table_def = schema
        .get_table(&table_name.clone().into())
        .ok_or_else(|| QueryError::TableNotFound(table_name.clone()))?;

    // Resolve predicate values (substitute parameters)
    let resolved_predicates = resolve_predicates(&parsed.predicates, params)?;

    // Check if we need aggregates (before resolving columns)
    let needs_aggregate = !parsed.aggregates.is_empty() || !parsed.group_by.is_empty() || parsed.distinct;

    // For aggregate queries, the source plan must fetch ALL columns
    // so the executor can access columns by table-level indices
    let (column_indices, column_names) = if needs_aggregate {
        // SELECT * for aggregates
        resolve_columns(table_def, None, &table_name)?
    } else {
        // Use parsed columns for non-aggregate queries
        resolve_columns(table_def, parsed.columns.as_ref(), &table_name)?
    };

    // Analyze predicates to determine access path
    let access_path = analyze_access_path(table_def, &resolved_predicates);

    // Build the base scan plan
    let base_plan = match access_path {
        AccessPath::PointLookup { key_values } => {
            let key = encode_key(&key_values);
            QueryPlan::PointLookup {
                table_id: table_def.table_id,
                table_name: table_name.clone(),
                key,
                columns: column_indices.clone(),
                column_names: column_names.clone(),
            }
        }

        AccessPath::RangeScan {
            start_key,
            end_key,
            remaining_predicates,
        } => {
            let filter = build_filter(table_def, &remaining_predicates, &table_name)?;
            let order = determine_scan_order(&parsed.order_by, table_def);

            QueryPlan::RangeScan {
                table_id: table_def.table_id,
                table_name: table_name.clone(),
                start: start_key,
                end: end_key,
                filter,
                limit: parsed.limit,
                order,
                columns: column_indices.clone(),
                column_names: column_names.clone(),
            }
        }

        AccessPath::TableScan {
            predicates: all_predicates,
        } => {
            let filter = build_filter(table_def, &all_predicates, &table_name)?;
            let order = build_sort_spec(&parsed.order_by, table_def, &table_name)?;

            QueryPlan::TableScan {
                table_id: table_def.table_id,
                table_name: table_name.clone(),
                filter,
                limit: parsed.limit,
                order,
                columns: column_indices.clone(),
                column_names: column_names.clone(),
            }
        }
    };

    // Wrap in an aggregate plan if needed
    // DISTINCT is implemented as GROUP BY all selected columns
    if needs_aggregate {
        // For DISTINCT without explicit GROUP BY, group by all selected columns
        let group_by_columns = if parsed.distinct && parsed.group_by.is_empty() {
            // Use parsed columns (what user selected), not column_names (which is all columns for aggregates)
            parsed.columns.clone().unwrap_or_else(|| {
                // If SELECT *, use all columns
                table_def.columns.iter().map(|c| c.name.clone()).collect()
            })
        } else {
            parsed.group_by.clone()
        };

        // For DISTINCT, aggregates should be empty (just deduplication)
        let aggregates = if parsed.distinct && parsed.aggregates.is_empty() {
            vec![]
        } else {
            parsed.aggregates.clone()
        };
        // Resolve GROUP BY column indices
        let mut group_by_indices = Vec::new();
        for col_name in &group_by_columns {
            let (idx, _) = table_def.find_column(col_name).ok_or_else(|| {
                QueryError::ColumnNotFound {
                    table: table_name.clone(),
                    column: col_name.to_string(),
                }
            })?;
            group_by_indices.push(idx);
        }

        // Build result column names: GROUP BY columns + aggregate results
        let mut result_columns = group_by_columns.clone();
        for agg in &aggregates {
            let agg_name = match agg {
                crate::parser::AggregateFunction::CountStar => "COUNT(*)".to_string(),
                crate::parser::AggregateFunction::Count(col) => format!("COUNT({})", col),
                crate::parser::AggregateFunction::Sum(col) => format!("SUM({})", col),
                crate::parser::AggregateFunction::Avg(col) => format!("AVG({})", col),
                crate::parser::AggregateFunction::Min(col) => format!("MIN({})", col),
                crate::parser::AggregateFunction::Max(col) => format!("MAX({})", col),
            };
            result_columns.push(ColumnName::new(agg_name));
        }

        Ok(QueryPlan::Aggregate {
            table_id: table_def.table_id,
            table_name,
            source: Box::new(base_plan),
            group_by_cols: group_by_indices,
            group_by_names: group_by_columns,
            aggregates,
            column_names: result_columns,
        })
    } else {
        Ok(base_plan)
    }
}

/// Resolves column selection to indices and names.
fn resolve_columns(
    table_def: &TableDef,
    columns: Option<&Vec<ColumnName>>,
    table_name: &str,
) -> Result<(Vec<usize>, Vec<ColumnName>)> {
    match columns {
        None => {
            // SELECT * - return all columns
            let indices: Vec<usize> = (0..table_def.columns.len()).collect();
            let names: Vec<ColumnName> = table_def.columns.iter().map(|c| c.name.clone()).collect();
            Ok((indices, names))
        }
        Some(cols) => {
            let mut indices = Vec::with_capacity(cols.len());
            let mut names = Vec::with_capacity(cols.len());

            for col in cols {
                let (idx, col_def) =
                    table_def
                        .find_column(col)
                        .ok_or_else(|| QueryError::ColumnNotFound {
                            table: table_name.to_string(),
                            column: col.to_string(),
                        })?;
                indices.push(idx);
                names.push(col_def.name.clone());
            }

            Ok((indices, names))
        }
    }
}

/// Resolved predicate with concrete values (parameters substituted).
#[derive(Debug, Clone)]
struct ResolvedPredicate {
    column: ColumnName,
    op: ResolvedOp,
}

#[derive(Debug, Clone)]
enum ResolvedOp {
    Eq(Value),
    Lt(Value),
    Le(Value),
    Gt(Value),
    Ge(Value),
    In(Vec<Value>),
    Like(String),
    IsNull,
    IsNotNull,
    Or(Vec<ResolvedPredicate>, Vec<ResolvedPredicate>),
}

/// Resolves predicates by substituting parameter values.
fn resolve_predicates(
    predicates: &[Predicate],
    params: &[Value],
) -> Result<Vec<ResolvedPredicate>> {
    predicates
        .iter()
        .map(|p| resolve_predicate(p, params))
        .collect()
}

fn resolve_predicate(predicate: &Predicate, params: &[Value]) -> Result<ResolvedPredicate> {
    match predicate {
        Predicate::Eq(col, val) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::Eq(resolve_value(val, params)?),
        }),
        Predicate::Lt(col, val) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::Lt(resolve_value(val, params)?),
        }),
        Predicate::Le(col, val) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::Le(resolve_value(val, params)?),
        }),
        Predicate::Gt(col, val) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::Gt(resolve_value(val, params)?),
        }),
        Predicate::Ge(col, val) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::Ge(resolve_value(val, params)?),
        }),
        Predicate::In(col, vals) => {
            let resolved: Result<Vec<_>> = vals.iter().map(|v| resolve_value(v, params)).collect();
            Ok(ResolvedPredicate {
                column: col.clone(),
                op: ResolvedOp::In(resolved?),
            })
        }
        Predicate::Like(col, pattern) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::Like(pattern.clone()),
        }),
        Predicate::IsNull(col) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::IsNull,
        }),
        Predicate::IsNotNull(col) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::IsNotNull,
        }),
        Predicate::Or(left_preds, right_preds) => {
            // For OR, we use a dummy column (empty string) since OR can span multiple columns
            let left_resolved = resolve_predicates(left_preds, params)?;
            let right_resolved = resolve_predicates(right_preds, params)?;
            Ok(ResolvedPredicate {
                column: ColumnName::new(String::new()),
                op: ResolvedOp::Or(left_resolved, right_resolved),
            })
        }
    }
}

fn resolve_value(val: &PredicateValue, params: &[Value]) -> Result<Value> {
    match val {
        PredicateValue::Int(v) => Ok(Value::BigInt(*v)),
        PredicateValue::String(s) => Ok(Value::Text(s.clone())),
        PredicateValue::Bool(b) => Ok(Value::Boolean(*b)),
        PredicateValue::Null => Ok(Value::Null),
        PredicateValue::Literal(v) => Ok(v.clone()),
        PredicateValue::Param(idx) => {
            // Parameters are 1-indexed in SQL
            let zero_idx = idx.checked_sub(1).ok_or(QueryError::ParameterNotFound(0))?;
            params
                .get(zero_idx)
                .cloned()
                .ok_or(QueryError::ParameterNotFound(*idx))
        }
    }
}

/// Access path determined by predicate analysis.
enum AccessPath {
    /// Point lookup on primary key.
    PointLookup { key_values: Vec<Value> },
    /// Range scan on primary key.
    RangeScan {
        start_key: Bound<kmb_store::Key>,
        end_key: Bound<kmb_store::Key>,
        remaining_predicates: Vec<ResolvedPredicate>,
    },
    /// Full table scan.
    TableScan { predicates: Vec<ResolvedPredicate> },
}

/// Analyzes predicates to determine the optimal access path.
fn analyze_access_path(table_def: &TableDef, predicates: &[ResolvedPredicate]) -> AccessPath {
    let pk_columns = &table_def.primary_key;

    if pk_columns.is_empty() {
        // No primary key - must do table scan
        return AccessPath::TableScan {
            predicates: predicates.to_vec(),
        };
    }

    // Check for point lookup: all PK columns have equality predicates
    let mut pk_values: Vec<Option<Value>> = vec![None; pk_columns.len()];
    let mut non_pk_predicates = Vec::new();

    for pred in predicates {
        if let Some(pk_pos) = table_def.primary_key_position(&pred.column) {
            if let ResolvedOp::Eq(val) = &pred.op {
                pk_values[pk_pos] = Some(val.clone());
                continue;
            }
        }
        non_pk_predicates.push(pred.clone());
    }

    // Check if we have all PK columns with equality
    if pk_values.iter().all(Option::is_some) {
        let key_values: Vec<Value> = pk_values.into_iter().flatten().collect();
        return AccessPath::PointLookup { key_values };
    }

    // Check for range scan: first PK column(s) have predicates
    // For simplicity, only handle single-column PK range scans for now
    if pk_columns.len() == 1 {
        let pk_col = &pk_columns[0];
        let pk_predicates: Vec<_> = predicates.iter().filter(|p| &p.column == pk_col).collect();

        if !pk_predicates.is_empty() {
            let bounds_result = compute_range_bounds(&pk_predicates);

            // If we have useful bounds (not both unbounded), use range scan
            let has_bounds = !matches!(
                (&bounds_result.start, &bounds_result.end),
                (Bound::Unbounded, Bound::Unbounded)
            );

            if has_bounds {
                // Collect remaining predicates: non-PK predicates + unconverted PK predicates
                let mut remaining: Vec<_> = predicates
                    .iter()
                    .filter(|p| &p.column != pk_col)
                    .cloned()
                    .collect();
                remaining.extend(bounds_result.unconverted);

                return AccessPath::RangeScan {
                    start_key: bounds_result.start,
                    end_key: bounds_result.end,
                    remaining_predicates: remaining,
                };
            }
            // If no useful bounds (e.g., only IN predicates), fall through to table scan
        }
    }

    // Fall back to table scan
    AccessPath::TableScan {
        predicates: predicates.to_vec(),
    }
}

/// Result of computing range bounds from predicates.
struct RangeBoundsResult {
    start: Bound<kmb_store::Key>,
    end: Bound<kmb_store::Key>,
    /// Predicates that couldn't be converted to bounds (e.g., IN).
    unconverted: Vec<ResolvedPredicate>,
}

/// Computes range bounds from predicates on a single column.
fn compute_range_bounds(predicates: &[&ResolvedPredicate]) -> RangeBoundsResult {
    let mut lower: Option<(Value, bool)> = None; // (value, inclusive)
    let mut upper: Option<(Value, bool)> = None;
    let mut unconverted = Vec::new();

    for pred in predicates {
        match &pred.op {
            ResolvedOp::Eq(val) => {
                // Exact match - both bounds are this value
                lower = Some((val.clone(), true));
                upper = Some((val.clone(), true));
            }
            ResolvedOp::Gt(val) => {
                lower = Some((val.clone(), false));
            }
            ResolvedOp::Ge(val) => {
                lower = Some((val.clone(), true));
            }
            ResolvedOp::Lt(val) => {
                upper = Some((val.clone(), false));
            }
            ResolvedOp::Le(val) => {
                upper = Some((val.clone(), true));
            }
            ResolvedOp::In(_)
            | ResolvedOp::Like(_)
            | ResolvedOp::IsNull
            | ResolvedOp::IsNotNull
            | ResolvedOp::Or(_, _) => {
                // These can't be converted to range bounds - add to filter
                unconverted.push((*pred).clone());
            }
        }
    }

    let start = match lower {
        Some((val, true)) => Bound::Included(encode_key(&[val])),
        Some((val, false)) => Bound::Excluded(encode_key(&[val])),
        None => Bound::Unbounded,
    };

    let end = match upper {
        Some((val, true)) => {
            // For inclusive upper bound, we need the successor key
            Bound::Excluded(successor_key(&encode_key(&[val])))
        }
        Some((val, false)) => Bound::Excluded(encode_key(&[val])),
        None => Bound::Unbounded,
    };

    RangeBoundsResult {
        start,
        end,
        unconverted,
    }
}

/// Builds a filter from remaining predicates.
fn build_filter(
    table_def: &TableDef,
    predicates: &[ResolvedPredicate],
    table_name: &str,
) -> Result<Option<Filter>> {
    if predicates.is_empty() {
        return Ok(None);
    }

    let filters: Result<Vec<_>> = predicates
        .iter()
        .map(|p| build_filter_from_predicate(table_def, p, table_name))
        .collect();

    Ok(Some(Filter::and(filters?)))
}

/// Builds a filter from a single resolved predicate.
/// Handles OR predicates recursively.
fn build_filter_from_predicate(
    table_def: &TableDef,
    pred: &ResolvedPredicate,
    table_name: &str,
) -> Result<Filter> {
    match &pred.op {
        ResolvedOp::Or(left_preds, right_preds) => {
            // Recursively build filters for left and right sides
            let left_filter = build_filter(table_def, left_preds, table_name)?
                .ok_or_else(|| QueryError::UnsupportedFeature("OR left side has no predicates".to_string()))?;
            let right_filter = build_filter(table_def, right_preds, table_name)?
                .ok_or_else(|| QueryError::UnsupportedFeature("OR right side has no predicates".to_string()))?;

            Ok(Filter::or(vec![left_filter, right_filter]))
        }
        _ => {
            // For non-OR predicates, build a FilterCondition
            let condition = build_filter_condition(table_def, pred, table_name)?;
            Ok(Filter::single(condition))
        }
    }
}

fn build_filter_condition(
    table_def: &TableDef,
    pred: &ResolvedPredicate,
    table_name: &str,
) -> Result<FilterCondition> {
    let (col_idx, _) =
        table_def
            .find_column(&pred.column)
            .ok_or_else(|| QueryError::ColumnNotFound {
                table: table_name.to_string(),
                column: pred.column.to_string(),
            })?;

    let (op, value) = match &pred.op {
        ResolvedOp::Eq(v) => (FilterOp::Eq, v.clone()),
        ResolvedOp::Lt(v) => (FilterOp::Lt, v.clone()),
        ResolvedOp::Le(v) => (FilterOp::Le, v.clone()),
        ResolvedOp::Gt(v) => (FilterOp::Gt, v.clone()),
        ResolvedOp::Ge(v) => (FilterOp::Ge, v.clone()),
        ResolvedOp::In(vals) => (FilterOp::In(vals.clone()), Value::Null), // Value unused for In
        ResolvedOp::Like(pattern) => (FilterOp::Like(pattern.clone()), Value::Null),
        ResolvedOp::IsNull => (FilterOp::IsNull, Value::Null),
        ResolvedOp::IsNotNull => (FilterOp::IsNotNull, Value::Null),
        ResolvedOp::Or(_, _) => {
            // OR predicates need special handling - they can't be represented as a single FilterCondition
            return Err(QueryError::UnsupportedFeature(
                "OR predicates must be handled at filter level, not as individual conditions".to_string(),
            ));
        }
    };

    Ok(FilterCondition {
        column_idx: col_idx,
        op,
        value,
    })
}

/// Determines scan order from ORDER BY for range scans.
fn determine_scan_order(order_by: &[OrderByClause], table_def: &TableDef) -> ScanOrder {
    if order_by.is_empty() {
        return ScanOrder::Ascending;
    }

    // Check if first ORDER BY column is in the primary key
    let first = &order_by[0];
    if table_def.is_primary_key(&first.column) {
        if first.ascending {
            ScanOrder::Ascending
        } else {
            ScanOrder::Descending
        }
    } else {
        ScanOrder::Ascending
    }
}

/// Builds a sort specification for table scans.
fn build_sort_spec(
    order_by: &[OrderByClause],
    table_def: &TableDef,
    table_name: &str,
) -> Result<Option<SortSpec>> {
    if order_by.is_empty() {
        return Ok(None);
    }

    let mut columns = Vec::with_capacity(order_by.len());

    for clause in order_by {
        let (col_idx, _) =
            table_def
                .find_column(&clause.column)
                .ok_or_else(|| QueryError::ColumnNotFound {
                    table: table_name.to_string(),
                    column: clause.column.to_string(),
                })?;

        let order = if clause.ascending {
            ScanOrder::Ascending
        } else {
            ScanOrder::Descending
        };

        columns.push((col_idx, order));
    }

    Ok(Some(SortSpec { columns }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_query;
    use crate::schema::{ColumnDef, DataType, SchemaBuilder};
    use kmb_store::TableId;

    fn test_schema() -> Schema {
        SchemaBuilder::new()
            .table(
                "users",
                TableId::new(1),
                vec![
                    ColumnDef::new("id", DataType::BigInt).not_null(),
                    ColumnDef::new("name", DataType::Text).not_null(),
                    ColumnDef::new("age", DataType::BigInt),
                ],
                vec!["id".into()],
            )
            .build()
    }

    #[test]
    fn test_plan_point_lookup() {
        let schema = test_schema();
        let parsed = parse_query("SELECT * FROM users WHERE id = 42").unwrap();
        let plan = plan_query(&schema, &parsed, &[]).unwrap();

        assert!(matches!(plan, QueryPlan::PointLookup { .. }));
    }

    #[test]
    fn test_plan_range_scan() {
        let schema = test_schema();
        let parsed = parse_query("SELECT * FROM users WHERE id > 10").unwrap();
        let plan = plan_query(&schema, &parsed, &[]).unwrap();

        assert!(matches!(plan, QueryPlan::RangeScan { .. }));
    }

    #[test]
    fn test_plan_table_scan() {
        let schema = test_schema();
        let parsed = parse_query("SELECT * FROM users WHERE name = 'alice'").unwrap();
        let plan = plan_query(&schema, &parsed, &[]).unwrap();

        assert!(matches!(plan, QueryPlan::TableScan { .. }));
    }

    #[test]
    fn test_plan_with_params() {
        let schema = test_schema();
        let parsed = parse_query("SELECT * FROM users WHERE id = $1").unwrap();
        let plan = plan_query(&schema, &parsed, &[Value::BigInt(42)]).unwrap();

        assert!(matches!(plan, QueryPlan::PointLookup { .. }));
    }

    #[test]
    fn test_plan_missing_param() {
        let schema = test_schema();
        let parsed = parse_query("SELECT * FROM users WHERE id = $1").unwrap();
        let result = plan_query(&schema, &parsed, &[]);

        assert!(matches!(result, Err(QueryError::ParameterNotFound(1))));
    }

    #[test]
    fn test_plan_unknown_table() {
        let schema = test_schema();
        let parsed = parse_query("SELECT * FROM unknown").unwrap();
        let result = plan_query(&schema, &parsed, &[]);

        assert!(matches!(result, Err(QueryError::TableNotFound(_))));
    }

    #[test]
    fn test_plan_unknown_column() {
        let schema = test_schema();
        let parsed = parse_query("SELECT unknown FROM users").unwrap();
        let result = plan_query(&schema, &parsed, &[]);

        assert!(matches!(result, Err(QueryError::ColumnNotFound { .. })));
    }
}
