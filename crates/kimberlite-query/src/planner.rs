//! Query planner: transforms parsed SQL into execution plans.
//!
//! The planner analyzes predicates to select the optimal access path:
//! - `PointLookup`: When all primary key columns have equality predicates
//! - `RangeScan`: When primary key has range predicates
//! - `TableScan`: Fallback for non-indexed predicates

use std::ops::Bound;
use std::sync::Arc;

use crate::error::{QueryError, Result};
use crate::expression::ScalarExpr;
use crate::key_encoder::{encode_key, successor_key};
use crate::parser::{
    CaseWhenArm, ComputedColumn, LimitExpr, OrderByClause, ParsedSelect, Predicate, PredicateValue,
    ScalarCmpOp,
};
use crate::plan::{
    CaseColumnDef, CaseWhenClause, Filter, FilterCondition, FilterOp, QueryPlan, ScanOrder,
    SortSpec,
};
use crate::schema::{ColumnName, Schema, TableDef};
use crate::value::Value;

/// Creates table metadata from a table definition.
#[inline]
fn create_metadata(table_def: &TableDef, table_name: String) -> crate::plan::TableMetadata {
    crate::plan::TableMetadata {
        table_id: table_def.table_id,
        table_name,
        columns: table_def.columns.clone(),
        primary_key: table_def.primary_key.clone(),
    }
}

/// Builds a point lookup plan.
#[inline]
fn build_point_lookup_plan(
    table_def: &TableDef,
    table_name: String,
    key_values: &[Value],
    column_indices: Vec<usize>,
    column_names: Vec<ColumnName>,
) -> QueryPlan {
    let key = encode_key(key_values);
    QueryPlan::PointLookup {
        metadata: create_metadata(table_def, table_name),
        key,
        columns: column_indices,
        column_names,
    }
}

/// Builds a range scan plan.
#[inline]
#[allow(clippy::too_many_arguments)]
fn build_range_scan_plan(
    table_def: &TableDef,
    table_name: String,
    start_key: Bound<kimberlite_store::Key>,
    end_key: Bound<kimberlite_store::Key>,
    remaining_predicates: &[ResolvedPredicate],
    limit: Option<usize>,
    offset: Option<usize>,
    order_by: &[OrderByClause],
    column_indices: Vec<usize>,
    column_names: Vec<ColumnName>,
) -> Result<QueryPlan> {
    let filter = build_filter(table_def, remaining_predicates, &table_name)?;
    let order = determine_scan_order(order_by, table_def);

    // Build sort spec for client-side sorting when ORDER BY doesn't match scan order
    let needs_client_sort = !order_by.is_empty()
        && order_by
            .iter()
            .any(|clause| !table_def.is_primary_key(&clause.column));
    let order_by_spec = if needs_client_sort {
        build_sort_spec(order_by, table_def, &table_name)?
    } else {
        None
    };

    Ok(QueryPlan::RangeScan {
        metadata: create_metadata(table_def, table_name),
        start: start_key,
        end: end_key,
        filter,
        limit,
        offset,
        order,
        order_by: order_by_spec,
        columns: column_indices,
        column_names,
    })
}

/// Builds an index scan plan.
#[inline]
#[allow(clippy::too_many_arguments)]
fn build_index_scan_plan(
    table_def: &TableDef,
    table_name: String,
    index_id: u64,
    index_name: String,
    start_key: Bound<kimberlite_store::Key>,
    end_key: Bound<kimberlite_store::Key>,
    remaining_predicates: &[ResolvedPredicate],
    limit: Option<usize>,
    offset: Option<usize>,
    order_by: &[OrderByClause],
    column_indices: Vec<usize>,
    column_names: Vec<ColumnName>,
) -> Result<QueryPlan> {
    let filter = build_filter(table_def, remaining_predicates, &table_name)?;
    let order = determine_scan_order(order_by, table_def);

    // Build sort spec for client-side sorting when ORDER BY doesn't match scan order
    let needs_client_sort = !order_by.is_empty()
        && order_by
            .iter()
            .any(|clause| !table_def.is_primary_key(&clause.column));
    let order_by_spec = if needs_client_sort {
        build_sort_spec(order_by, table_def, &table_name)?
    } else {
        None
    };

    Ok(QueryPlan::IndexScan {
        metadata: create_metadata(table_def, table_name),
        index_id,
        index_name,
        start: start_key,
        end: end_key,
        filter,
        limit,
        offset,
        order,
        order_by: order_by_spec,
        columns: column_indices,
        column_names,
    })
}

/// Builds a table scan plan.
#[inline]
#[allow(clippy::too_many_arguments)]
fn build_table_scan_plan(
    table_def: &TableDef,
    table_name: String,
    all_predicates: &[ResolvedPredicate],
    limit: Option<usize>,
    offset: Option<usize>,
    order_by: &[OrderByClause],
    column_indices: Vec<usize>,
    column_names: Vec<ColumnName>,
) -> Result<QueryPlan> {
    let filter = build_filter(table_def, all_predicates, &table_name)?;
    let order = build_sort_spec(order_by, table_def, &table_name)?;

    Ok(QueryPlan::TableScan {
        metadata: create_metadata(table_def, table_name),
        filter,
        limit,
        offset,
        order,
        columns: column_indices,
        column_names,
    })
}

/// Resolves a `LimitExpr` against the bound parameter slice into a concrete
/// row count. Mirrors `resolve_value` for the LIMIT/OFFSET position.
///
/// Errors on negative or non-integer bound values so the caller surfaces a
/// clear message instead of letting a bad cast panic at the plan boundary.
fn resolve_limit(expr: Option<LimitExpr>, params: &[Value]) -> Result<Option<usize>> {
    match expr {
        None => Ok(None),
        Some(LimitExpr::Literal(v)) => Ok(Some(v)),
        Some(LimitExpr::Param(idx)) => {
            let zero_idx = idx.checked_sub(1).ok_or(QueryError::ParameterNotFound(0))?;
            let value = params
                .get(zero_idx)
                .cloned()
                .ok_or(QueryError::ParameterNotFound(idx))?;
            match value {
                Value::BigInt(n) if n >= 0 => Ok(Some(n as usize)),
                Value::Integer(n) if n >= 0 => Ok(Some(n as usize)),
                Value::SmallInt(n) if n >= 0 => Ok(Some(n as usize)),
                Value::TinyInt(n) if n >= 0 => Ok(Some(n as usize)),
                Value::BigInt(_) | Value::Integer(_) | Value::SmallInt(_) | Value::TinyInt(_) => {
                    Err(QueryError::ParseError(
                        "LIMIT/OFFSET parameter must be non-negative".to_string(),
                    ))
                }
                other => Err(QueryError::UnsupportedFeature(format!(
                    "LIMIT/OFFSET parameter must bind to an integer; got {other:?}"
                ))),
            }
        }
    }
}

/// Wraps a base plan with an aggregate plan if needed.
#[inline]
fn wrap_with_aggregate(
    base_plan: QueryPlan,
    table_def: &TableDef,
    table_name: String,
    parsed: &ParsedSelect,
    params: &[Value],
) -> Result<QueryPlan> {
    // For DISTINCT without explicit GROUP BY, group by all selected columns
    let group_by_columns = if parsed.distinct && parsed.group_by.is_empty() {
        parsed
            .columns
            .clone()
            .unwrap_or_else(|| table_def.columns.iter().map(|c| c.name.clone()).collect())
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
        let (idx, _) =
            table_def
                .find_column(col_name)
                .ok_or_else(|| QueryError::ColumnNotFound {
                    table: table_name.clone(),
                    column: col_name.to_string(),
                })?;
        group_by_indices.push(idx);
    }

    // Build result column names: GROUP BY columns + aggregate results
    let mut result_columns = group_by_columns.clone();
    for agg in &aggregates {
        let agg_name = match agg {
            crate::parser::AggregateFunction::CountStar => "COUNT(*)".to_string(),
            crate::parser::AggregateFunction::Count(col) => format!("COUNT({col})"),
            crate::parser::AggregateFunction::Sum(col) => format!("SUM({col})"),
            crate::parser::AggregateFunction::Avg(col) => format!("AVG({col})"),
            crate::parser::AggregateFunction::Min(col) => format!("MIN({col})"),
            crate::parser::AggregateFunction::Max(col) => format!("MAX({col})"),
        };
        result_columns.push(ColumnName::new(agg_name));
    }

    // Resolve per-aggregate FILTER (WHERE ...) predicates against the table.
    // The parser guarantees aggregate_filters is parallel with aggregates, but
    // older planner paths build aggregates without filters — fall back to None
    // when the parsed list is shorter than the resolved aggregates list.
    let mut aggregate_filters: Vec<Option<crate::plan::Filter>> = Vec::new();
    for (i, _agg) in aggregates.iter().enumerate() {
        let filter_predicates = parsed.aggregate_filters.get(i).and_then(|f| f.as_ref());
        let filter = match filter_predicates {
            Some(preds) => {
                let resolved = resolve_predicates(preds, params)?;
                build_filter(table_def, &resolved, &table_name)?
            }
            None => None,
        };
        aggregate_filters.push(filter);
    }

    Ok(QueryPlan::Aggregate {
        metadata: create_metadata(table_def, table_name),
        source: Box::new(base_plan),
        group_by_cols: group_by_indices,
        group_by_names: group_by_columns,
        aggregates,
        aggregate_filters,
        column_names: result_columns,
        having: parsed.having.clone(),
    })
}

/// Plans a parsed SELECT statement. Samples the system clock once and
/// folds `NOW()` / `CURRENT_TIMESTAMP` / `CURRENT_DATE` sentinels into
/// literals before returning, so the executor never sees a raw
/// sentinel and stays pure (PRESSURECRAFT §1 FCIS). Callers that need
/// a deterministic clock (VOPR, replay) should use
/// [`plan_query_with_clock`] and pass their own timestamp.
pub fn plan_query(schema: &Schema, parsed: &ParsedSelect, params: &[Value]) -> Result<QueryPlan> {
    plan_query_with_clock(schema, parsed, params, current_statement_timestamp_ns())
}

/// Plans a parsed SELECT statement with an explicit statement-stable
/// timestamp. Use this from VOPR / replay / any path that requires
/// determinism. AUDIT-2026-05 S3.7.
pub fn plan_query_with_clock(
    schema: &Schema,
    parsed: &ParsedSelect,
    params: &[Value],
    statement_ts_ns: i64,
) -> Result<QueryPlan> {
    let mut plan = if parsed.joins.is_empty() {
        // Single-table query - existing logic
        plan_single_table_query(schema, parsed, params)?
    } else {
        // Multi-table query - new JOIN logic
        plan_join_query(schema, parsed, params)?
    };
    fold_time_constants_in_plan(&mut plan, statement_ts_ns);
    Ok(plan)
}

/// Plans a single-table query (no JOINs).
fn plan_single_table_query(
    schema: &Schema,
    parsed: &ParsedSelect,
    params: &[Value],
) -> Result<QueryPlan> {
    // Look up table
    let table_name = parsed.table.clone();
    let table_def = schema
        .get_table(&table_name.clone().into())
        .ok_or_else(|| QueryError::TableNotFound(table_name.clone()))?;

    // Resolve predicate values (substitute parameters)
    let resolved_predicates = resolve_predicates(&parsed.predicates, params)?;

    // Resolve columns for the query (accounts for aggregate requirements)
    let (column_indices, column_names) = resolve_query_columns(table_def, parsed, &table_name)?;

    // Check if we need aggregates
    let needs_aggregate = !parsed.aggregates.is_empty()
        || !parsed.group_by.is_empty()
        || parsed.distinct
        || !parsed.having.is_empty();

    // Analyze predicates to determine access path
    let access_path = analyze_access_path(table_def, &resolved_predicates);

    // Build the base scan plan
    let base_plan = build_scan_plan(
        access_path,
        table_def,
        table_name.clone(),
        parsed,
        params,
        column_indices,
        column_names,
    )?;

    // Wrap in an aggregate plan if needed
    let plan_after_agg = if needs_aggregate {
        wrap_with_aggregate(base_plan, table_def, table_name.clone(), parsed, params)?
    } else {
        base_plan
    };

    // Wrap in a Materialize plan if CASE WHEN or scalar projections are present.
    if !parsed.case_columns.is_empty() || !parsed.scalar_projections.is_empty() {
        let source_columns = plan_after_agg.column_names().to_vec();
        let case_columns = resolve_case_columns_for_join(&parsed.case_columns, &source_columns)?;
        // Scalar columns see the *post-case* layout so a scalar projection
        // can reference a CASE alias too (rare but consistent with PG).
        let mut post_case_columns = source_columns.clone();
        post_case_columns.extend(case_columns.iter().map(|c| c.alias.clone()));
        let scalar_columns =
            resolve_scalar_columns(&parsed.scalar_projections, &post_case_columns, params)?;

        // Build the declared output column list in SELECT order:
        //   [user-selected bare columns (with aliases)...; CASE aliases...; scalar outputs...]
        // The executor prunes source rows down to this shape by name, so
        // source columns fetched only for scalar-expression evaluation
        // don't leak into the final result.
        let mut output_columns: Vec<ColumnName> = match (&parsed.columns, &parsed.column_aliases) {
            (None, _) => {
                // SELECT * — pass through every source column.
                source_columns.clone()
            }
            (Some(cols), aliases) => {
                let mut out = Vec::with_capacity(cols.len());
                for (i, col) in cols.iter().enumerate() {
                    let alias = aliases
                        .as_ref()
                        .and_then(|v| v.get(i))
                        .and_then(|a| a.as_ref());
                    out.push(match alias {
                        Some(a) => ColumnName::new(a.clone()),
                        None => col.clone(),
                    });
                }
                out
            }
        };
        output_columns.extend(case_columns.iter().map(|c| c.alias.clone()));
        output_columns.extend(scalar_columns.iter().map(|c| c.output_name.clone()));

        Ok(QueryPlan::Materialize {
            source: Box::new(plan_after_agg),
            filter: None,
            case_columns,
            scalar_columns,
            order: None,
            limit: None,
            offset: None,
            column_names: output_columns,
        })
    } else {
        Ok(plan_after_agg)
    }
}

/// Bake a list of parsed scalar projections against a known source
/// column layout. Substitutes bound parameter values into each
/// expression tree so the executor doesn't have to know about `params`.
fn resolve_scalar_columns(
    projections: &[crate::parser::ParsedScalarProjection],
    source_columns: &[ColumnName],
    params: &[Value],
) -> Result<Vec<crate::plan::ScalarColumnDef>> {
    if projections.is_empty() {
        return Ok(Vec::new());
    }
    let columns: Arc<[ColumnName]> = source_columns.to_vec().into();
    projections
        .iter()
        .map(|p| {
            Ok(crate::plan::ScalarColumnDef {
                output_name: p.output_name.clone(),
                expr: substitute_scalar_params(&p.expr, params)?,
                columns: columns.clone(),
            })
        })
        .collect()
}

/// Plans a multi-table query with JOINs.
fn plan_join_query(schema: &Schema, parsed: &ParsedSelect, params: &[Value]) -> Result<QueryPlan> {
    // Build left-deep join tree
    let mut current_plan = plan_table_access(schema, &parsed.table, params)?;

    for join in &parsed.joins {
        // Plan right table access
        let right_plan = plan_table_access(schema, &join.table, params)?;

        // Build join conditions from ON clause
        let on_conditions =
            build_join_conditions(&join.on_condition, schema, &parsed.table, &join.table)?;

        // Merge column names from both tables (left then right)
        let left_columns = current_plan.column_names().to_vec();
        let right_columns = right_plan.column_names().to_vec();
        let mut all_columns = left_columns.clone();
        all_columns.extend(right_columns);

        // Build join node
        current_plan = QueryPlan::Join {
            join_type: join.join_type.clone(),
            left: Box::new(current_plan),
            right: Box::new(right_plan),
            on_conditions,
            columns: vec![], // All columns from both tables
            column_names: all_columns,
        };
    }

    // Collect the full combined column list from the join tree
    let combined_columns = current_plan.column_names().to_vec();

    // Resolve WHERE predicates against the combined column list
    let resolved_predicates = resolve_predicates(&parsed.predicates, params)?;
    let filter = build_filter_for_join(&resolved_predicates, &combined_columns)?;

    // Resolve ORDER BY against the combined column list
    let order = build_sort_spec_for_join(&parsed.order_by, &combined_columns)?;

    // Resolve CASE WHEN computed columns
    let case_columns = resolve_case_columns_for_join(&parsed.case_columns, &combined_columns)?;

    // Compute the post-case column layout — scalar projections see this
    // shape so they can reference a CASE alias if needed.
    let mut post_case_columns = combined_columns.clone();
    post_case_columns.extend(case_columns.iter().map(|c| c.alias.clone()));
    let scalar_columns =
        resolve_scalar_columns(&parsed.scalar_projections, &post_case_columns, params)?;

    // Determine output column names: selected columns from combined list
    // + CASE aliases + scalar output names.
    let output_columns: Vec<ColumnName> = match &parsed.columns {
        None => {
            // SELECT * — all combined columns + computed aliases
            let mut out = combined_columns.clone();
            out.extend(case_columns.iter().map(|c| c.alias.clone()));
            out.extend(scalar_columns.iter().map(|c| c.output_name.clone()));
            out
        }
        Some(selected) => {
            // Validate that each selected column exists in the combined list
            for col in selected {
                if !combined_columns.iter().any(|c| c == col) {
                    return Err(QueryError::ColumnNotFound {
                        table: parsed.table.clone(),
                        column: col.to_string(),
                    });
                }
            }
            let mut out = selected.clone();
            out.extend(case_columns.iter().map(|c| c.alias.clone()));
            out.extend(scalar_columns.iter().map(|c| c.output_name.clone()));
            out
        }
    };

    let limit = resolve_limit(parsed.limit, params)?;
    let offset = resolve_limit(parsed.offset, params)?;
    let needs_materialize = filter.is_some()
        || order.is_some()
        || limit.is_some()
        || offset.is_some()
        || !case_columns.is_empty()
        || !scalar_columns.is_empty();

    if needs_materialize {
        Ok(QueryPlan::Materialize {
            source: Box::new(current_plan),
            filter,
            case_columns,
            scalar_columns,
            order,
            limit,
            offset,
            column_names: output_columns,
        })
    } else {
        Ok(current_plan)
    }
}

/// Builds a filter from resolved predicates against a named column list.
///
/// Used for JOIN queries where we don't have a single `TableDef` — instead we
/// resolve column names against the combined left+right column list.
fn build_filter_for_join(
    predicates: &[ResolvedPredicate],
    columns: &[ColumnName],
) -> Result<Option<Filter>> {
    if predicates.is_empty() {
        return Ok(None);
    }

    let filters: Result<Vec<_>> = predicates
        .iter()
        .map(|p| build_filter_for_join_predicate(p, columns))
        .collect();

    Ok(Some(Filter::and(filters?)))
}

fn build_filter_for_join_predicate(
    pred: &ResolvedPredicate,
    columns: &[ColumnName],
) -> Result<Filter> {
    if let ResolvedOp::Or(left_preds, right_preds) = &pred.op {
        let left_filter = build_filter_for_join(left_preds, columns)?.ok_or_else(|| {
            QueryError::UnsupportedFeature("OR left side has no predicates".to_string())
        })?;
        let right_filter = build_filter_for_join(right_preds, columns)?.ok_or_else(|| {
            QueryError::UnsupportedFeature("OR right side has no predicates".to_string())
        })?;
        Ok(Filter::or(vec![left_filter, right_filter]))
    } else {
        let condition = build_filter_condition_for_join(pred, columns)?;
        Ok(Filter::single(condition))
    }
}

fn build_filter_condition_for_join(
    pred: &ResolvedPredicate,
    columns: &[ColumnName],
) -> Result<FilterCondition> {
    if matches!(pred.op, ResolvedOp::AlwaysTrue) {
        return Ok(FilterCondition {
            column_idx: 0,
            op: FilterOp::AlwaysTrue,
            value: Value::Null,
        });
    }
    if matches!(pred.op, ResolvedOp::AlwaysFalse) {
        return Ok(FilterCondition {
            column_idx: 0,
            op: FilterOp::AlwaysFalse,
            value: Value::Null,
        });
    }
    // ScalarCmp resolves column names against the join-combined column
    // layout passed in. Short-circuit before the keyed column lookup.
    if let ResolvedOp::ScalarCmp { lhs, op, rhs } = &pred.op {
        let cols: Arc<[ColumnName]> = columns.to_vec().into();
        return Ok(FilterCondition {
            column_idx: 0,
            op: FilterOp::ScalarCmp {
                columns: cols,
                lhs: lhs.clone(),
                op: *op,
                rhs: rhs.clone(),
            },
            value: Value::Null,
        });
    }

    let col_idx = columns
        .iter()
        .position(|c| c == &pred.column)
        .ok_or_else(|| QueryError::ColumnNotFound {
            table: "(join)".to_string(),
            column: pred.column.to_string(),
        })?;

    let (op, value) = match &pred.op {
        ResolvedOp::Eq(v) => (FilterOp::Eq, v.clone()),
        ResolvedOp::Lt(v) => (FilterOp::Lt, v.clone()),
        ResolvedOp::Le(v) => (FilterOp::Le, v.clone()),
        ResolvedOp::Gt(v) => (FilterOp::Gt, v.clone()),
        ResolvedOp::Ge(v) => (FilterOp::Ge, v.clone()),
        ResolvedOp::In(vals) => (FilterOp::In(vals.clone()), Value::Null),
        ResolvedOp::NotIn(vals) => (FilterOp::NotIn(vals.clone()), Value::Null),
        ResolvedOp::NotBetween(low, high) => {
            (FilterOp::NotBetween(low.clone(), high.clone()), Value::Null)
        }
        ResolvedOp::Like(pattern) => (FilterOp::Like(pattern.clone()), Value::Null),
        ResolvedOp::NotLike(pattern) => (FilterOp::NotLike(pattern.clone()), Value::Null),
        ResolvedOp::ILike(pattern) => (FilterOp::ILike(pattern.clone()), Value::Null),
        ResolvedOp::NotILike(pattern) => (FilterOp::NotILike(pattern.clone()), Value::Null),
        ResolvedOp::IsNull => (FilterOp::IsNull, Value::Null),
        ResolvedOp::IsNotNull => (FilterOp::IsNotNull, Value::Null),
        ResolvedOp::JsonExtractEq {
            path,
            as_text,
            value: v,
        } => (
            FilterOp::JsonExtractEq {
                path: path.clone(),
                as_text: *as_text,
                value: v.clone(),
            },
            Value::Null,
        ),
        ResolvedOp::JsonContains(v) => (FilterOp::JsonContains(v.clone()), Value::Null),
        ResolvedOp::AlwaysTrue => (FilterOp::AlwaysTrue, Value::Null),
        ResolvedOp::AlwaysFalse => (FilterOp::AlwaysFalse, Value::Null),
        ResolvedOp::Or(_, _) => {
            return Err(QueryError::UnsupportedFeature(
                "OR predicates must be handled at filter level".to_string(),
            ));
        }
        ResolvedOp::ScalarCmp { .. } => {
            unreachable!("ScalarCmp handled by the short-circuit above");
        }
    };

    Ok(FilterCondition {
        column_idx: col_idx,
        op,
        value,
    })
}

/// Builds a sort specification from ORDER BY clauses against a named column list.
fn build_sort_spec_for_join(
    order_by: &[OrderByClause],
    columns: &[ColumnName],
) -> Result<Option<SortSpec>> {
    if order_by.is_empty() {
        return Ok(None);
    }

    let mut sort_cols = Vec::with_capacity(order_by.len());
    for clause in order_by {
        let idx = columns
            .iter()
            .position(|c| c == &clause.column)
            .ok_or_else(|| QueryError::ColumnNotFound {
                table: "(join)".to_string(),
                column: clause.column.to_string(),
            })?;
        let order = if clause.ascending {
            ScanOrder::Ascending
        } else {
            ScanOrder::Descending
        };
        sort_cols.push((idx, order));
    }

    Ok(Some(SortSpec { columns: sort_cols }))
}

/// Resolves CASE WHEN computed columns against a named column list.
fn resolve_case_columns_for_join(
    case_columns: &[ComputedColumn],
    columns: &[ColumnName],
) -> Result<Vec<CaseColumnDef>> {
    case_columns
        .iter()
        .map(|cc| resolve_single_case_column(cc, columns))
        .collect()
}

fn resolve_single_case_column(
    cc: &ComputedColumn,
    columns: &[ColumnName],
) -> Result<CaseColumnDef> {
    let when_clauses: Result<Vec<_>> = cc
        .when_clauses
        .iter()
        .map(|arm| resolve_case_when_arm(arm, columns))
        .collect();

    Ok(CaseColumnDef {
        alias: cc.alias.clone(),
        when_clauses: when_clauses?,
        else_value: cc.else_value.clone(),
    })
}

fn resolve_case_when_arm(arm: &CaseWhenArm, columns: &[ColumnName]) -> Result<CaseWhenClause> {
    // Resolve predicates (no params in CASE conditions)
    let resolved = resolve_predicates(&arm.condition, &[])?;
    let filter = build_filter_for_join(&resolved, columns)?.ok_or_else(|| {
        QueryError::UnsupportedFeature("CASE WHEN condition has no predicates".to_string())
    })?;

    Ok(CaseWhenClause {
        condition: filter,
        result: arm.result.clone(),
    })
}

/// Plans a table access for a single table (used in JOINs).
fn plan_table_access(schema: &Schema, table_name: &str, _params: &[Value]) -> Result<QueryPlan> {
    let table_def = schema
        .get_table(&table_name.into())
        .ok_or_else(|| QueryError::TableNotFound(table_name.to_string()))?;

    // For JOIN table access, just do a full table scan
    // (In the future, we could optimize this based on join predicates)
    let all_column_indices: Vec<usize> = (0..table_def.columns.len()).collect();
    let all_column_names: Vec<ColumnName> =
        table_def.columns.iter().map(|c| c.name.clone()).collect();

    Ok(QueryPlan::TableScan {
        metadata: create_metadata(table_def, table_name.to_string()),
        filter: None,
        limit: None,
        offset: None,
        order: None,
        columns: all_column_indices,
        column_names: all_column_names,
    })
}

/// Builds join conditions from ON clause predicates.
///
/// JOIN predicates can have column references on both sides (e.g., users.id = orders.user_id).
/// These need to be resolved to indices in the concatenated row [left_cols..., right_cols...].
fn build_join_conditions(
    predicates: &[Predicate],
    schema: &Schema,
    left_table: &str,
    right_table: &str,
) -> Result<Vec<crate::plan::JoinCondition>> {
    let left_table_def = schema
        .get_table(&left_table.into())
        .ok_or_else(|| QueryError::TableNotFound(left_table.to_string()))?;
    let right_table_def = schema
        .get_table(&right_table.into())
        .ok_or_else(|| QueryError::TableNotFound(right_table.to_string()))?;

    let left_col_count = left_table_def.columns.len();

    predicates
        .iter()
        .map(|pred| {
            build_single_join_condition(
                pred,
                left_table,
                left_table_def,
                right_table,
                right_table_def,
                left_col_count,
            )
        })
        .collect()
}

/// Builds a single join condition from a JOIN predicate.
///
/// Handles column-to-column comparisons by resolving qualified/unqualified column names
/// to indices in the concatenated row [left_cols..., right_cols...].
fn build_single_join_condition(
    pred: &Predicate,
    left_table: &str,
    left_table_def: &TableDef,
    right_table: &str,
    right_table_def: &TableDef,
    left_col_count: usize,
) -> Result<crate::plan::JoinCondition> {
    use crate::plan::JoinOp;

    // Extract column name, operator, and right side from predicate
    let (left_col_name, op, right_value) = match pred {
        Predicate::Eq(col, val) => (col, JoinOp::Eq, val),
        Predicate::Lt(col, val) => (col, JoinOp::Lt, val),
        Predicate::Le(col, val) => (col, JoinOp::Le, val),
        Predicate::Gt(col, val) => (col, JoinOp::Gt, val),
        Predicate::Ge(col, val) => (col, JoinOp::Ge, val),
        _ => {
            return Err(QueryError::UnsupportedFeature(
                "only equality and comparison operators supported in JOIN ON clause".to_string(),
            ));
        }
    };

    // Resolve left column to index in concatenated row
    let left_col_idx = resolve_join_column(left_col_name, left_table, left_table_def, 0)?;

    // Right side must be a column reference for JOIN conditions
    match right_value {
        PredicateValue::ColumnRef(ref_str) => {
            // Column-to-column comparison
            let right_col_idx = resolve_join_column_ref(
                ref_str,
                left_table,
                left_table_def,
                right_table,
                right_table_def,
                left_col_count,
            )?;

            Ok(crate::plan::JoinCondition {
                left_col_idx,
                right_col_idx,
                op,
            })
        }
        _ => Err(QueryError::UnsupportedFeature(
            "JOIN ON clause requires column-to-column comparisons (e.g., users.id = orders.user_id)".to_string(),
        )),
    }
}

/// Resolves a column name to an index in the concatenated row.
fn resolve_join_column(
    col_name: &ColumnName,
    table_name: &str,
    table_def: &TableDef,
    offset: usize,
) -> Result<usize> {
    let (idx, _) = table_def
        .find_column(col_name)
        .ok_or_else(|| QueryError::ColumnNotFound {
            table: table_name.to_string(),
            column: col_name.to_string(),
        })?;
    Ok(offset + idx)
}

/// Resolves a qualified/unqualified column reference to an index in the concatenated row.
fn resolve_join_column_ref(
    ref_str: &str,
    left_table: &str,
    left_table_def: &TableDef,
    right_table: &str,
    right_table_def: &TableDef,
    left_col_count: usize,
) -> Result<usize> {
    // Parse qualified reference: "table.column" or just "column"
    if let Some((table, column)) = ref_str.split_once('.') {
        // Qualified: table.column
        if table == left_table {
            resolve_join_column(&column.into(), left_table, left_table_def, 0)
        } else if table == right_table {
            resolve_join_column(&column.into(), right_table, right_table_def, left_col_count)
        } else {
            Err(QueryError::TableNotFound(table.to_string()))
        }
    } else {
        // Unqualified: just "column" - try right table first (common pattern)
        if let Ok(idx) = resolve_join_column(
            &ref_str.into(),
            right_table,
            right_table_def,
            left_col_count,
        ) {
            Ok(idx)
        } else {
            resolve_join_column(&ref_str.into(), left_table, left_table_def, 0)
        }
    }
}

/// Builds a scan plan from the analyzed access path.
#[inline]
fn build_scan_plan(
    access_path: AccessPath,
    table_def: &TableDef,
    table_name: String,
    parsed: &ParsedSelect,
    params: &[Value],
    column_indices: Vec<usize>,
    column_names: Vec<ColumnName>,
) -> Result<QueryPlan> {
    let limit = resolve_limit(parsed.limit, params)?;
    let offset = resolve_limit(parsed.offset, params)?;
    match access_path {
        AccessPath::PointLookup { key_values } => Ok(build_point_lookup_plan(
            table_def,
            table_name,
            &key_values,
            column_indices,
            column_names,
        )),
        AccessPath::RangeScan {
            start_key,
            end_key,
            remaining_predicates,
        } => build_range_scan_plan(
            table_def,
            table_name,
            start_key,
            end_key,
            &remaining_predicates,
            limit,
            offset,
            &parsed.order_by,
            column_indices,
            column_names,
        ),
        AccessPath::IndexScan {
            index_id,
            index_name,
            start_key,
            end_key,
            remaining_predicates,
        } => build_index_scan_plan(
            table_def,
            table_name,
            index_id,
            index_name,
            start_key,
            end_key,
            &remaining_predicates,
            limit,
            offset,
            &parsed.order_by,
            column_indices,
            column_names,
        ),
        AccessPath::TableScan {
            predicates: all_predicates,
        } => build_table_scan_plan(
            table_def,
            table_name,
            &all_predicates,
            limit,
            offset,
            &parsed.order_by,
            column_indices,
            column_names,
        ),
    }
}

/// Resolves column selection for queries, accounting for aggregate requirements.
#[inline]
fn resolve_query_columns(
    table_def: &TableDef,
    parsed: &ParsedSelect,
    table_name: &str,
) -> Result<(Vec<usize>, Vec<ColumnName>)> {
    let needs_aggregate = !parsed.aggregates.is_empty()
        || !parsed.group_by.is_empty()
        || parsed.distinct
        || !parsed.having.is_empty();

    // For aggregate or scalar-projection queries, the source plan must
    // fetch ALL columns so the Materialize / Aggregate wrapper can
    // resolve column references by name. Aliases don't apply to the
    // source scan in that case — the wrapper's `column_names` is built
    // separately.
    if needs_aggregate || !parsed.scalar_projections.is_empty() {
        resolve_columns(table_def, None, None, table_name)
    } else {
        resolve_columns(
            table_def,
            parsed.columns.as_ref(),
            parsed.column_aliases.as_ref(),
            table_name,
        )
    }
}

/// Resolves column selection to indices and names.
///
/// ROADMAP v0.5.0 item A — when `aliases` is `Some(vec)` and
/// `aliases[i]` is `Some(alias)`, the output `names[i]` is the alias
/// rather than the source column name. The source column still
/// drives `indices` for projection; the alias affects only what the
/// client sees in `QueryResult.columns`.
fn resolve_columns(
    table_def: &TableDef,
    columns: Option<&Vec<ColumnName>>,
    aliases: Option<&Vec<Option<String>>>,
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

            for (i, col) in cols.iter().enumerate() {
                let (idx, col_def) =
                    table_def
                        .find_column(col)
                        .ok_or_else(|| QueryError::ColumnNotFound {
                            table: table_name.to_string(),
                            column: col.to_string(),
                        })?;
                indices.push(idx);
                // If an alias was supplied for this position, stamp the
                // output name with the alias; otherwise fall back to the
                // column-def canonical name.
                let alias = aliases.and_then(|v| v.get(i)).and_then(|a| a.as_ref());
                let out_name = match alias {
                    Some(a) => ColumnName::new(a.clone()),
                    None => col_def.name.clone(),
                };
                names.push(out_name);
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
    NotIn(Vec<Value>),
    NotBetween(Value, Value),
    Like(String),
    NotLike(String),
    ILike(String),
    NotILike(String),
    IsNull,
    IsNotNull,
    /// JSON path extraction with equality comparison (`->`/`->>`).
    JsonExtractEq {
        path: String,
        as_text: bool,
        value: Value,
    },
    /// JSON containment (`@>`).
    JsonContains(Value),
    Or(Vec<ResolvedPredicate>, Vec<ResolvedPredicate>),
    /// Tautology — matches every row (from `EXISTS` whose subquery had rows).
    AlwaysTrue,
    /// Contradiction — matches no rows (from `EXISTS` whose subquery was empty).
    AlwaysFalse,
    /// Comparison between two scalar expressions, evaluated per row.
    ///
    /// The `column` on the outer `ResolvedPredicate` is empty for this
    /// variant; the filter path evaluates `lhs` and `rhs` via
    /// [`crate::expression::evaluate`] against the full row. Mirrors
    /// the shape of [`super::Predicate::ScalarCmp`].
    ScalarCmp {
        lhs: ScalarExpr,
        op: ScalarCmpOp,
        rhs: ScalarExpr,
    },
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
        Predicate::NotIn(col, vals) => {
            let resolved: Result<Vec<_>> = vals.iter().map(|v| resolve_value(v, params)).collect();
            Ok(ResolvedPredicate {
                column: col.clone(),
                op: ResolvedOp::NotIn(resolved?),
            })
        }
        Predicate::NotBetween(col, low, high) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::NotBetween(resolve_value(low, params)?, resolve_value(high, params)?),
        }),
        Predicate::ScalarCmp { lhs, op, rhs } => Ok(ResolvedPredicate {
            // ScalarCmp spans the whole row — no single column to key
            // off. Mirror Or/Always, which use the empty string and let
            // the filter path dispatch on the op.
            column: ColumnName::new(String::new()),
            op: ResolvedOp::ScalarCmp {
                lhs: substitute_scalar_params(lhs, params)?,
                op: *op,
                rhs: substitute_scalar_params(rhs, params)?,
            },
        }),
        Predicate::Like(col, pattern) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::Like(pattern.clone()),
        }),
        Predicate::NotLike(col, pattern) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::NotLike(pattern.clone()),
        }),
        Predicate::ILike(col, pattern) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::ILike(pattern.clone()),
        }),
        Predicate::NotILike(col, pattern) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::NotILike(pattern.clone()),
        }),
        Predicate::IsNull(col) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::IsNull,
        }),
        Predicate::IsNotNull(col) => Ok(ResolvedPredicate {
            column: col.clone(),
            op: ResolvedOp::IsNotNull,
        }),
        Predicate::JsonExtractEq {
            column,
            path,
            as_text,
            value,
        } => Ok(ResolvedPredicate {
            column: column.clone(),
            op: ResolvedOp::JsonExtractEq {
                path: path.clone(),
                as_text: *as_text,
                value: resolve_value(value, params)?,
            },
        }),
        Predicate::JsonContains { column, value } => Ok(ResolvedPredicate {
            column: column.clone(),
            op: ResolvedOp::JsonContains(resolve_value(value, params)?),
        }),
        // Subquery predicates are pre-executed and substituted in the
        // top-level query() entry point before reaching the planner.
        // If they reach here, that substitution failed.
        Predicate::InSubquery { .. } | Predicate::Exists { .. } => {
            Err(QueryError::UnsupportedFeature(
                "subquery predicate not pre-executed (likely a correlated subquery)".to_string(),
            ))
        }
        // Always(true)  → no-op; substitute with a tautological IS NOT NULL
        //                 against a primary-key column (which is never NULL).
        // Always(false) → impossible; substitute with IS NULL against a
        //                 primary-key column (which is never NULL → always
        //                 false).
        Predicate::Always(b) => {
            // We don't have access to the table here to pick a real column.
            // Use an empty column name; resolve_predicates handles this by
            // returning a non-error tautology/contradiction at filter level.
            // Tag the op so build_filter recognises it.
            Ok(ResolvedPredicate {
                column: ColumnName::new(String::new()),
                op: if *b {
                    ResolvedOp::AlwaysTrue
                } else {
                    ResolvedOp::AlwaysFalse
                },
            })
        }
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
        PredicateValue::ColumnRef(_) => Err(QueryError::UnsupportedFeature(
            "column references in WHERE clause not supported (use JOIN ON for column-to-column comparisons)".to_string(),
        )),
    }
}

/// Sample the system clock for a fresh statement-stable timestamp in
/// Unix nanoseconds. Lives at the planner boundary on purpose — the
/// evaluator stays pure (PRESSURECRAFT §1 FCIS), so the clock read is
/// done once per statement, threaded into the plan, and never sampled
/// inside row evaluation.
///
/// Returns `0` if the system clock is somehow before the Unix epoch;
/// callers can override with their own clock for deterministic VOPR
/// runs by calling [`fold_time_constants_in_plan`] directly.
pub fn current_statement_timestamp_ns() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// Convert a Unix-nanosecond timestamp to "days since Unix epoch"
/// (truncating toward negative infinity for pre-epoch values, though
/// the evaluator guards against negatives).
fn ns_to_days_since_epoch(ts_ns: i64) -> i32 {
    let days = ts_ns.div_euclid(86_400_000_000_000);
    let clamped = days.clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    // Clamp guarantees the value fits in i32; the cast is lossless by
    // construction.
    #[allow(clippy::cast_possible_truncation)]
    {
        clamped as i32
    }
}

/// Walk a `ScalarExpr` tree and replace `ScalarExpr::Now` /
/// `CurrentTimestamp` with a `Literal(Timestamp(statement_ts_ns))` and
/// `CurrentDate` with `Literal(Date(days_since_epoch))`. The same
/// `statement_ts_ns` value MUST be used for every fold call within a
/// single statement so that two `NOW()` references in the same query
/// return identical timestamps (SQL standard: statement-stable).
///
/// Pure function. Called from [`fold_time_constants_in_plan`] and any
/// site that needs to stabilise scalar-expression trees before
/// evaluation. The companion `#[should_panic]` tests in
/// `expression.rs` (`now_panics_at_evaluator_when_unfolded` etc.)
/// remain the contract that the evaluator never sees a raw sentinel.
///
/// AUDIT-2026-05 S3.7 (planner-side completion of the fold contract
/// promised in v0.7.0).
pub fn fold_time_constants(expr: &ScalarExpr, statement_ts_ns: i64) -> ScalarExpr {
    fn f(e: &ScalarExpr, ts: i64) -> ScalarExpr {
        match e {
            ScalarExpr::Now | ScalarExpr::CurrentTimestamp => ScalarExpr::Literal(
                Value::Timestamp(kimberlite_types::Timestamp::from_nanos(ts.max(0) as u64)),
            ),
            ScalarExpr::CurrentDate => ScalarExpr::Literal(Value::Date(ns_to_days_since_epoch(ts))),
            // Recurse into operands so nested time-now references
            // (e.g. `EXTRACT(YEAR FROM NOW())`) are also folded.
            ScalarExpr::Literal(v) => ScalarExpr::Literal(v.clone()),
            ScalarExpr::Column(c) => ScalarExpr::Column(c.clone()),
            ScalarExpr::Upper(x) => ScalarExpr::Upper(Box::new(f(x, ts))),
            ScalarExpr::Lower(x) => ScalarExpr::Lower(Box::new(f(x, ts))),
            ScalarExpr::Length(x) => ScalarExpr::Length(Box::new(f(x, ts))),
            ScalarExpr::Trim(x) => ScalarExpr::Trim(Box::new(f(x, ts))),
            ScalarExpr::Concat(xs) => ScalarExpr::Concat(xs.iter().map(|x| f(x, ts)).collect()),
            ScalarExpr::Abs(x) => ScalarExpr::Abs(Box::new(f(x, ts))),
            ScalarExpr::Round(x) => ScalarExpr::Round(Box::new(f(x, ts))),
            ScalarExpr::RoundScale(x, n) => ScalarExpr::RoundScale(Box::new(f(x, ts)), *n),
            ScalarExpr::Ceil(x) => ScalarExpr::Ceil(Box::new(f(x, ts))),
            ScalarExpr::Floor(x) => ScalarExpr::Floor(Box::new(f(x, ts))),
            ScalarExpr::Coalesce(xs) => ScalarExpr::Coalesce(xs.iter().map(|x| f(x, ts)).collect()),
            ScalarExpr::Nullif(a, b) => ScalarExpr::Nullif(Box::new(f(a, ts)), Box::new(f(b, ts))),
            ScalarExpr::Cast(x, t) => ScalarExpr::Cast(Box::new(f(x, ts)), *t),
            ScalarExpr::Mod(a, b) => ScalarExpr::Mod(Box::new(f(a, ts)), Box::new(f(b, ts))),
            ScalarExpr::Power(a, b) => ScalarExpr::Power(Box::new(f(a, ts)), Box::new(f(b, ts))),
            ScalarExpr::Sqrt(x) => ScalarExpr::Sqrt(Box::new(f(x, ts))),
            ScalarExpr::Substring(x, r) => ScalarExpr::Substring(Box::new(f(x, ts)), *r),
            ScalarExpr::Extract(field, x) => ScalarExpr::Extract(*field, Box::new(f(x, ts))),
            ScalarExpr::DateTrunc(field, x) => ScalarExpr::DateTrunc(*field, Box::new(f(x, ts))),
        }
    }
    f(expr, statement_ts_ns)
}

/// Walk a [`Filter`] tree and fold any embedded [`ScalarExpr::Now`] /
/// `CurrentTimestamp` / `CurrentDate` sentinels. Called from
/// [`fold_time_constants_in_plan`].
fn fold_time_constants_in_filter(filter: &mut crate::plan::Filter, statement_ts_ns: i64) {
    use crate::plan::{Filter, FilterOp};
    match filter {
        Filter::Condition(cond) => {
            if let FilterOp::ScalarCmp { lhs, rhs, .. } = &mut cond.op {
                *lhs = fold_time_constants(lhs, statement_ts_ns);
                *rhs = fold_time_constants(rhs, statement_ts_ns);
            }
        }
        Filter::And(parts) | Filter::Or(parts) => {
            for p in parts {
                fold_time_constants_in_filter(p, statement_ts_ns);
            }
        }
    }
}

/// Walk a [`QueryPlan`] tree and fold every embedded `NOW()` /
/// `CURRENT_TIMESTAMP` / `CURRENT_DATE` sentinel into a literal,
/// using a single statement-stable timestamp. Mutates the plan in
/// place. The evaluator can then run pure (no clock read), and two
/// `NOW()` references in the same statement return identical values
/// per the SQL standard.
///
/// This is the production wiring of the contract that v0.7.0
/// established: the AST carries `ScalarExpr::Now` / `CurrentTimestamp`
/// / `CurrentDate` sentinels, the evaluator panics if it sees them
/// raw, and this function is the planner-side pass that replaces
/// them. AUDIT-2026-05 S3.7.
pub fn fold_time_constants_in_plan(plan: &mut QueryPlan, statement_ts_ns: i64) {
    match plan {
        QueryPlan::PointLookup { .. } => {
            // No scalar expressions in this shape.
        }
        QueryPlan::RangeScan { filter, .. }
        | QueryPlan::IndexScan { filter, .. }
        | QueryPlan::TableScan { filter, .. } => {
            if let Some(f) = filter {
                fold_time_constants_in_filter(f, statement_ts_ns);
            }
        }
        QueryPlan::Aggregate {
            source,
            aggregate_filters,
            ..
        } => {
            fold_time_constants_in_plan(source, statement_ts_ns);
            for af in aggregate_filters.iter_mut().flatten() {
                fold_time_constants_in_filter(af, statement_ts_ns);
            }
        }
        QueryPlan::Join { left, right, .. } => {
            fold_time_constants_in_plan(left, statement_ts_ns);
            fold_time_constants_in_plan(right, statement_ts_ns);
        }
        QueryPlan::Materialize {
            source,
            filter,
            scalar_columns,
            ..
        } => {
            fold_time_constants_in_plan(source, statement_ts_ns);
            if let Some(f) = filter {
                fold_time_constants_in_filter(f, statement_ts_ns);
            }
            for sc in scalar_columns {
                sc.expr = fold_time_constants(&sc.expr, statement_ts_ns);
            }
        }
    }
}

/// Walk a `ScalarExpr` tree and replace any `Literal(Placeholder(n))`
/// with the bound parameter value. Leaves other `Literal` variants
/// and column references untouched. Mirrors `resolve_value` but for
/// scalar-expression trees used by [`Predicate::ScalarCmp`] and
/// `ParsedScalarProjection`.
pub(crate) fn substitute_scalar_params(expr: &ScalarExpr, params: &[Value]) -> Result<ScalarExpr> {
    fn s(e: &ScalarExpr, p: &[Value]) -> Result<ScalarExpr> {
        Ok(match e {
            ScalarExpr::Literal(Value::Placeholder(idx)) => {
                let zero = idx.checked_sub(1).ok_or(QueryError::ParameterNotFound(0))?;
                let v = p
                    .get(zero)
                    .cloned()
                    .ok_or(QueryError::ParameterNotFound(*idx))?;
                ScalarExpr::Literal(v)
            }
            ScalarExpr::Literal(v) => ScalarExpr::Literal(v.clone()),
            ScalarExpr::Column(c) => ScalarExpr::Column(c.clone()),
            ScalarExpr::Upper(x) => ScalarExpr::Upper(Box::new(s(x, p)?)),
            ScalarExpr::Lower(x) => ScalarExpr::Lower(Box::new(s(x, p)?)),
            ScalarExpr::Length(x) => ScalarExpr::Length(Box::new(s(x, p)?)),
            ScalarExpr::Trim(x) => ScalarExpr::Trim(Box::new(s(x, p)?)),
            ScalarExpr::Concat(xs) => {
                ScalarExpr::Concat(xs.iter().map(|x| s(x, p)).collect::<Result<Vec<_>>>()?)
            }
            ScalarExpr::Abs(x) => ScalarExpr::Abs(Box::new(s(x, p)?)),
            ScalarExpr::Round(x) => ScalarExpr::Round(Box::new(s(x, p)?)),
            ScalarExpr::RoundScale(x, n) => ScalarExpr::RoundScale(Box::new(s(x, p)?), *n),
            ScalarExpr::Ceil(x) => ScalarExpr::Ceil(Box::new(s(x, p)?)),
            ScalarExpr::Floor(x) => ScalarExpr::Floor(Box::new(s(x, p)?)),
            ScalarExpr::Coalesce(xs) => {
                ScalarExpr::Coalesce(xs.iter().map(|x| s(x, p)).collect::<Result<Vec<_>>>()?)
            }
            ScalarExpr::Nullif(a, b) => ScalarExpr::Nullif(Box::new(s(a, p)?), Box::new(s(b, p)?)),
            ScalarExpr::Cast(x, t) => ScalarExpr::Cast(Box::new(s(x, p)?), *t),
            // v0.7.0 scalar functions — recurse into operands.
            ScalarExpr::Mod(a, b) => ScalarExpr::Mod(Box::new(s(a, p)?), Box::new(s(b, p)?)),
            ScalarExpr::Power(a, b) => ScalarExpr::Power(Box::new(s(a, p)?), Box::new(s(b, p)?)),
            ScalarExpr::Sqrt(x) => ScalarExpr::Sqrt(Box::new(s(x, p)?)),
            ScalarExpr::Substring(x, r) => ScalarExpr::Substring(Box::new(s(x, p)?), *r),
            ScalarExpr::Extract(f, x) => ScalarExpr::Extract(*f, Box::new(s(x, p)?)),
            ScalarExpr::DateTrunc(f, x) => ScalarExpr::DateTrunc(*f, Box::new(s(x, p)?)),
            // Time-now sentinels carry no operands; identity.
            ScalarExpr::Now => ScalarExpr::Now,
            ScalarExpr::CurrentTimestamp => ScalarExpr::CurrentTimestamp,
            ScalarExpr::CurrentDate => ScalarExpr::CurrentDate,
        })
    }
    s(expr, params)
}

/// Access path determined by predicate analysis.
enum AccessPath {
    /// Point lookup on primary key.
    PointLookup { key_values: Vec<Value> },
    /// Range scan on primary key.
    RangeScan {
        start_key: Bound<kimberlite_store::Key>,
        end_key: Bound<kimberlite_store::Key>,
        remaining_predicates: Vec<ResolvedPredicate>,
    },
    /// Index scan on a secondary index.
    IndexScan {
        index_id: u64,
        index_name: String,
        start_key: Bound<kimberlite_store::Key>,
        end_key: Bound<kimberlite_store::Key>,
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

            // Unsatisfiable bound set (e.g., `x > 5 AND x < 3`) —
            // short-circuit before the store ever sees an inverted
            // range. AUDIT-2026-05 H-3.
            if bounds_result.is_empty {
                return AccessPath::TableScan {
                    predicates: vec![ResolvedPredicate {
                        column: ColumnName::new(String::new()),
                        op: ResolvedOp::AlwaysFalse,
                    }],
                };
            }

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
            // If no useful bounds (e.g., only IN predicates), fall through to index scan check
        }
    }

    // Check for index scan: find indexes that can be used
    let index_candidates = find_usable_indexes(table_def, predicates);
    if let Some((best_index, start, end, remaining)) = select_best_index(&index_candidates) {
        return AccessPath::IndexScan {
            index_id: best_index.index_id,
            index_name: best_index.name.clone(),
            start_key: start,
            end_key: end,
            remaining_predicates: remaining,
        };
    }

    // Fall back to table scan
    AccessPath::TableScan {
        predicates: predicates.to_vec(),
    }
}

/// Result of computing range bounds from predicates.
struct RangeBoundsResult {
    start: Bound<kimberlite_store::Key>,
    end: Bound<kimberlite_store::Key>,
    /// Predicates that couldn't be converted to bounds (e.g., IN).
    unconverted: Vec<ResolvedPredicate>,
    /// AUDIT-2026-05 H-3 — set when the lowering produced an
    /// unsatisfiable predicate set (e.g. `x > 5 AND x < 3`). The
    /// caller is responsible for short-circuiting to an empty
    /// result rather than passing inverted bounds down to the
    /// store, which would have to defensively clamp them. Closes
    /// the inverted-range planner output deferred from v0.6.1
    /// (the `cfg(not(fuzzing))` debug-assert escape hatch in
    /// `kimberlite-store::btree::scan`).
    is_empty: bool,
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
            | ResolvedOp::NotIn(_)
            | ResolvedOp::NotBetween(_, _)
            | ResolvedOp::Like(_)
            | ResolvedOp::NotLike(_)
            | ResolvedOp::ILike(_)
            | ResolvedOp::NotILike(_)
            | ResolvedOp::IsNull
            | ResolvedOp::IsNotNull
            | ResolvedOp::JsonExtractEq { .. }
            | ResolvedOp::JsonContains(_)
            | ResolvedOp::AlwaysTrue
            | ResolvedOp::AlwaysFalse
            | ResolvedOp::Or(_, _)
            | ResolvedOp::ScalarCmp { .. } => {
                // These can't be converted to range bounds - add to filter
                unconverted.push((*pred).clone());
            }
        }
    }

    let start = match &lower {
        Some((val, true)) => Bound::Included(encode_key(&[val.clone()])),
        Some((val, false)) => Bound::Excluded(encode_key(&[val.clone()])),
        None => Bound::Unbounded,
    };

    let end = match &upper {
        Some((val, true)) => {
            // For inclusive upper bound, we need the successor key
            Bound::Excluded(successor_key(&encode_key(&[val.clone()])))
        }
        Some((val, false)) => Bound::Excluded(encode_key(&[val.clone()])),
        None => Bound::Unbounded,
    };

    // AUDIT-2026-05 H-3 — detect unsatisfiable bound combinations
    // upstream of the storage scan. A prior version relied on the
    // store's defensive clamp (`if range.start >= range.end { return
    // empty }`) plus a `cfg(not(fuzzing))` debug-assert; the
    // assert had to be muted because the planner legitimately
    // emitted inverted ranges for inputs like `x > 5 AND x < 3`.
    // Detecting at the source means the store can keep its
    // assertion live in fuzz builds too — closing the loop with
    // PRESSURECRAFT §3 (parse-don't-validate: an inverted range
    // is now unrepresentable in `RangeBoundsResult` whose caller
    // honours `is_empty`).
    let is_empty = bounds_are_unsatisfiable(&start, &end);

    RangeBoundsResult {
        start,
        end,
        unconverted,
        is_empty,
    }
}

/// Returns `true` when the (start, end) pair describes the empty
/// range: encoded `start_bytes > end_bytes`, or `start_bytes ==
/// end_bytes` with at least one side excluded.
///
/// `Unbounded` on either side is never empty (one-sided ranges are
/// satisfiable). Bytes-level comparison is the source of truth:
/// post-encoding inversion (e.g. successor-key arithmetic on
/// inclusive-upper bounds) is what the downstream store sees, so
/// that's what we check.
fn bounds_are_unsatisfiable(
    start: &Bound<kimberlite_store::Key>,
    end: &Bound<kimberlite_store::Key>,
) -> bool {
    use std::cmp::Ordering;
    let (start_bytes, start_excluded) = match start {
        Bound::Included(b) => (b.as_ref(), false),
        Bound::Excluded(b) => (b.as_ref(), true),
        Bound::Unbounded => return false,
    };
    let (end_bytes, end_excluded) = match end {
        Bound::Included(b) => (b.as_ref(), false),
        Bound::Excluded(b) => (b.as_ref(), true),
        Bound::Unbounded => return false,
    };

    match start_bytes.cmp(end_bytes) {
        Ordering::Greater => true,
        Ordering::Equal => start_excluded || end_excluded,
        Ordering::Less => false,
    }
}

/// Candidate index with its bounds and remaining predicates.
struct IndexCandidate<'a> {
    index_def: &'a crate::schema::IndexDef,
    start: Bound<kimberlite_store::Key>,
    end: Bound<kimberlite_store::Key>,
    remaining: Vec<ResolvedPredicate>,
    score: usize,
}

/// Finds indexes that can be used for the given predicates.
///
/// Returns a list of index candidates with their computed bounds.
/// Only includes indexes where the first column has predicates.
fn find_usable_indexes<'a>(
    table_def: &'a TableDef,
    predicates: &[ResolvedPredicate],
) -> Vec<IndexCandidate<'a>> {
    let mut candidates = Vec::new();
    let max_iterations = 100; // Bounded iteration limit

    for (iter_count, index_def) in table_def.indexes().iter().enumerate() {
        // Bounded iteration check
        if iter_count >= max_iterations {
            break;
        }

        // Skip empty indexes
        if index_def.columns.is_empty() {
            continue;
        }

        // Check if first column has predicates
        let first_col = &index_def.columns[0];
        let first_col_predicates: Vec<_> = predicates
            .iter()
            .filter(|p| &p.column == first_col)
            .collect();

        if first_col_predicates.is_empty() {
            continue;
        }

        // Compute range bounds for this index
        let bounds_result = compute_range_bounds(&first_col_predicates);

        // Unsatisfiable bounds disqualify the index entirely; the
        // caller's PK-range short-circuit (above) already handles
        // the common case, but a non-PK index reaching here with
        // an inverted predicate set would otherwise emit an
        // inverted-range scan. Skip silently — the table-scan
        // fallback's `AlwaysFalse` predicate will report empty.
        // AUDIT-2026-05 H-3.
        if bounds_result.is_empty {
            continue;
        }

        // Skip if both bounds are unbounded (no useful range)
        if matches!(
            (&bounds_result.start, &bounds_result.end),
            (Bound::Unbounded, Bound::Unbounded)
        ) {
            continue;
        }

        // Collect remaining predicates (non-index predicates + unconverted)
        let mut remaining: Vec<_> = predicates
            .iter()
            .filter(|p| !index_def.columns.contains(&p.column))
            .cloned()
            .collect();
        remaining.extend(bounds_result.unconverted);

        // Score this index
        let score = score_index(index_def, predicates);

        candidates.push(IndexCandidate {
            index_def,
            start: bounds_result.start,
            end: bounds_result.end,
            remaining,
            score,
        });
    }

    candidates
}

/// Scores an index based on predicate coverage.
///
/// Returns higher scores for indexes that cover more predicates with better match types.
fn score_index(index_def: &crate::schema::IndexDef, predicates: &[ResolvedPredicate]) -> usize {
    let mut score = 0;
    let max_columns = 10; // Bounded iteration limit

    for (iter_count, index_col) in index_def.columns.iter().enumerate() {
        // Bounded iteration check
        if iter_count >= max_columns {
            break;
        }

        for pred in predicates {
            if &pred.column == index_col {
                match &pred.op {
                    ResolvedOp::Eq(_) => score += 10, // Equality predicates are best
                    ResolvedOp::Lt(_)
                    | ResolvedOp::Le(_)
                    | ResolvedOp::Gt(_)
                    | ResolvedOp::Ge(_) => {
                        score += 5; // Range predicates are good
                    }
                    _ => score += 1, // Other predicates have minor benefit
                }
            }
        }
    }

    score
}

/// Return type for index selection with index definition, key bounds, and remaining predicates.
type BestIndexResult<'a> = (
    &'a crate::schema::IndexDef,
    Bound<kimberlite_store::Key>,
    Bound<kimberlite_store::Key>,
    Vec<ResolvedPredicate>,
);

/// Selects the best index from candidates.
///
/// Returns the index with the highest score, breaking ties by fewest remaining predicates.
fn select_best_index<'a>(candidates: &'a [IndexCandidate<'a>]) -> Option<BestIndexResult<'a>> {
    if candidates.is_empty() {
        return None;
    }

    // Find the maximum score
    let max_score = candidates.iter().map(|c| c.score).max().unwrap_or(0);

    // Filter to candidates with max score
    let best_candidates: Vec<_> = candidates.iter().filter(|c| c.score == max_score).collect();

    // Among ties, select the one with fewest remaining predicates
    let best = best_candidates
        .iter()
        .min_by_key(|c| (c.remaining.len(), c.index_def.columns.len()))?;

    Some((
        best.index_def,
        best.start.clone(),
        best.end.clone(),
        best.remaining.clone(),
    ))
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
    if let ResolvedOp::Or(left_preds, right_preds) = &pred.op {
        // Recursively build filters for left and right sides
        let left_filter = build_filter(table_def, left_preds, table_name)?.ok_or_else(|| {
            QueryError::UnsupportedFeature("OR left side has no predicates".to_string())
        })?;
        let right_filter = build_filter(table_def, right_preds, table_name)?.ok_or_else(|| {
            QueryError::UnsupportedFeature("OR right side has no predicates".to_string())
        })?;

        Ok(Filter::or(vec![left_filter, right_filter]))
    } else {
        // For non-OR predicates, build a FilterCondition
        let condition = build_filter_condition(table_def, pred, table_name)?;
        Ok(Filter::single(condition))
    }
}

fn build_filter_condition(
    table_def: &TableDef,
    pred: &ResolvedPredicate,
    table_name: &str,
) -> Result<FilterCondition> {
    // AlwaysTrue / AlwaysFalse don't reference a column — short-circuit before
    // the column lookup so the empty column name doesn't surface as an error.
    if matches!(pred.op, ResolvedOp::AlwaysTrue) {
        return Ok(FilterCondition {
            column_idx: 0,
            op: FilterOp::AlwaysTrue,
            value: Value::Null,
        });
    }
    if matches!(pred.op, ResolvedOp::AlwaysFalse) {
        return Ok(FilterCondition {
            column_idx: 0,
            op: FilterOp::AlwaysFalse,
            value: Value::Null,
        });
    }
    // ScalarCmp spans the whole row — carry the table's column layout
    // alongside the expression trees so `ScalarExpr::Column(name)`
    // resolves positionally at evaluation time.
    if let ResolvedOp::ScalarCmp { lhs, op, rhs } = &pred.op {
        let columns: Arc<[ColumnName]> = table_def
            .columns
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>()
            .into();
        return Ok(FilterCondition {
            column_idx: 0,
            op: FilterOp::ScalarCmp {
                columns,
                lhs: lhs.clone(),
                op: *op,
                rhs: rhs.clone(),
            },
            value: Value::Null,
        });
    }

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
        ResolvedOp::NotIn(vals) => (FilterOp::NotIn(vals.clone()), Value::Null),
        ResolvedOp::NotBetween(low, high) => {
            (FilterOp::NotBetween(low.clone(), high.clone()), Value::Null)
        }
        ResolvedOp::Like(pattern) => (FilterOp::Like(pattern.clone()), Value::Null),
        ResolvedOp::NotLike(pattern) => (FilterOp::NotLike(pattern.clone()), Value::Null),
        ResolvedOp::ILike(pattern) => (FilterOp::ILike(pattern.clone()), Value::Null),
        ResolvedOp::NotILike(pattern) => (FilterOp::NotILike(pattern.clone()), Value::Null),
        ResolvedOp::IsNull => (FilterOp::IsNull, Value::Null),
        ResolvedOp::IsNotNull => (FilterOp::IsNotNull, Value::Null),
        ResolvedOp::JsonExtractEq {
            path,
            as_text,
            value: v,
        } => (
            FilterOp::JsonExtractEq {
                path: path.clone(),
                as_text: *as_text,
                value: v.clone(),
            },
            Value::Null,
        ),
        ResolvedOp::JsonContains(v) => (FilterOp::JsonContains(v.clone()), Value::Null),
        ResolvedOp::AlwaysTrue => (FilterOp::AlwaysTrue, Value::Null),
        ResolvedOp::AlwaysFalse => (FilterOp::AlwaysFalse, Value::Null),
        ResolvedOp::Or(_, _) => {
            // OR predicates need special handling - they can't be represented as a single FilterCondition
            return Err(QueryError::UnsupportedFeature(
                "OR predicates must be handled at filter level, not as individual conditions"
                    .to_string(),
            ));
        }
        ResolvedOp::ScalarCmp { .. } => {
            // Unreachable — handled by the short-circuit above.
            unreachable!("ScalarCmp must be handled by the short-circuit branch");
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
    use crate::parser::{ParsedStatement, parse_statement};
    use crate::schema::{ColumnDef, DataType, SchemaBuilder};
    use kimberlite_store::TableId;

    fn parse_test_select(sql: &str) -> ParsedSelect {
        match parse_statement(sql).unwrap() {
            ParsedStatement::Select(s) => s,
            _ => panic!("expected SELECT statement"),
        }
    }

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
        let parsed = parse_test_select("SELECT * FROM users WHERE id = 42");
        let plan = plan_query(&schema, &parsed, &[]).unwrap();

        assert!(matches!(plan, QueryPlan::PointLookup { .. }));
    }

    #[test]
    fn test_plan_range_scan() {
        let schema = test_schema();
        let parsed = parse_test_select("SELECT * FROM users WHERE id > 10");
        let plan = plan_query(&schema, &parsed, &[]).unwrap();

        assert!(matches!(plan, QueryPlan::RangeScan { .. }));
    }

    #[test]
    fn test_plan_table_scan() {
        let schema = test_schema();
        let parsed = parse_test_select("SELECT * FROM users WHERE name = 'alice'");
        let plan = plan_query(&schema, &parsed, &[]).unwrap();

        assert!(matches!(plan, QueryPlan::TableScan { .. }));
    }

    #[test]
    fn test_plan_with_params() {
        let schema = test_schema();
        let parsed = parse_test_select("SELECT * FROM users WHERE id = $1");
        let plan = plan_query(&schema, &parsed, &[Value::BigInt(42)]).unwrap();

        assert!(matches!(plan, QueryPlan::PointLookup { .. }));
    }

    #[test]
    fn test_plan_missing_param() {
        let schema = test_schema();
        let parsed = parse_test_select("SELECT * FROM users WHERE id = $1");
        let result = plan_query(&schema, &parsed, &[]);

        assert!(matches!(result, Err(QueryError::ParameterNotFound(1))));
    }

    #[test]
    fn test_plan_unknown_table() {
        let schema = test_schema();
        let parsed = parse_test_select("SELECT * FROM unknown");
        let result = plan_query(&schema, &parsed, &[]);

        assert!(matches!(result, Err(QueryError::TableNotFound(_))));
    }

    #[test]
    fn test_plan_unknown_column() {
        let schema = test_schema();
        let parsed = parse_test_select("SELECT unknown FROM users");
        let result = plan_query(&schema, &parsed, &[]);

        assert!(matches!(result, Err(QueryError::ColumnNotFound { .. })));
    }

    // AUDIT-2026-05 H-3 — inverted-range planner regression
    // coverage. Every shape that produces an unsatisfiable PK
    // predicate set must short-circuit to a TableScan with an
    // AlwaysFalse filter, never an inverted RangeScan that the
    // store has to defensively clamp.

    #[test]
    fn inverted_range_pk_short_circuits_to_alwaysfalse_table_scan() {
        let schema = test_schema();
        let parsed = parse_test_select("SELECT * FROM users WHERE id > 5 AND id < 3");
        let plan = plan_query(&schema, &parsed, &[]).unwrap();

        match plan {
            QueryPlan::TableScan { filter, .. } => {
                let filter_str = format!("{filter:?}");
                assert!(
                    filter_str.contains("AlwaysFalse"),
                    "expected AlwaysFalse filter for x > 5 AND x < 3, got: {filter_str}"
                );
            }
            other => panic!("expected TableScan with AlwaysFalse filter, got: {other:?}"),
        }
    }

    #[test]
    fn excluded_equal_bounds_pk_short_circuits() {
        // x > 5 AND x < 5 — bounds at the same value but both excluded.
        let schema = test_schema();
        let parsed = parse_test_select("SELECT * FROM users WHERE id > 5 AND id < 5");
        let plan = plan_query(&schema, &parsed, &[]).unwrap();
        assert!(matches!(plan, QueryPlan::TableScan { .. }));
    }

    #[test]
    fn one_sided_lower_bound_still_uses_range_scan() {
        // Sanity: x > 5 (no upper) is satisfiable, must remain a
        // RangeScan. Guards against an over-eager is_empty
        // detector that flags Unbounded-on-one-side as empty.
        let schema = test_schema();
        let parsed = parse_test_select("SELECT * FROM users WHERE id > 5");
        let plan = plan_query(&schema, &parsed, &[]).unwrap();
        assert!(matches!(plan, QueryPlan::RangeScan { .. }));
    }

    #[test]
    fn bounds_are_unsatisfiable_unit() {
        // Direct test of the helper — not via the planner so any
        // refactor of compute_range_bounds keeps the contract
        // intact.
        use std::ops::Bound;

        // Both unbounded → satisfiable.
        assert!(!bounds_are_unsatisfiable(
            &Bound::<kimberlite_store::Key>::Unbounded,
            &Bound::Unbounded,
        ));
        // Inclusive equal → satisfiable (point lookup).
        let k = kimberlite_store::Key::from(vec![1u8, 2, 3]);
        assert!(!bounds_are_unsatisfiable(
            &Bound::Included(k.clone()),
            &Bound::Included(k.clone()),
        ));
        // Excluded equal → empty.
        assert!(bounds_are_unsatisfiable(
            &Bound::Excluded(k.clone()),
            &Bound::Excluded(k.clone()),
        ));
        // start > end → empty.
        let k_high = kimberlite_store::Key::from(vec![9u8]);
        let k_low = kimberlite_store::Key::from(vec![1u8]);
        assert!(bounds_are_unsatisfiable(
            &Bound::Included(k_high),
            &Bound::Included(k_low),
        ));
    }

    // ============================================================================
    // AUDIT-2026-05 S3.7 — fold_time_constants production wiring tests.
    //
    // Companion to the `#[should_panic]` tests in `expression.rs` that
    // pin the contract "evaluator panics on raw NOW/CURRENT_TIMESTAMP/
    // CURRENT_DATE." These tests prove the planner-side fold pass
    // actually replaces the sentinels with literals so the evaluator
    // never sees them.
    // ============================================================================

    #[test]
    fn fold_time_constants_replaces_now_with_timestamp_literal() {
        let ts_ns = 1_746_316_800_i64 * 1_000_000_000; // 2025-05-04T00:00:00Z
        let folded = fold_time_constants(&ScalarExpr::Now, ts_ns);
        match folded {
            ScalarExpr::Literal(Value::Timestamp(t)) => {
                assert_eq!(t.as_nanos() as i64, ts_ns);
            }
            other => panic!("expected Literal(Timestamp), got {other:?}"),
        }
    }

    #[test]
    fn fold_time_constants_replaces_current_timestamp() {
        let ts_ns = 1_746_316_800_i64 * 1_000_000_000;
        let folded = fold_time_constants(&ScalarExpr::CurrentTimestamp, ts_ns);
        match folded {
            ScalarExpr::Literal(Value::Timestamp(t)) => {
                assert_eq!(t.as_nanos() as i64, ts_ns);
            }
            other => panic!("expected Literal(Timestamp), got {other:?}"),
        }
    }

    #[test]
    fn fold_time_constants_replaces_current_date_with_days_since_epoch() {
        // 2025-05-04T00:00:00Z = 20212 days since epoch.
        let ts_ns = 1_746_316_800_i64 * 1_000_000_000;
        let folded = fold_time_constants(&ScalarExpr::CurrentDate, ts_ns);
        match folded {
            ScalarExpr::Literal(Value::Date(days)) => {
                assert_eq!(days, 20_212);
            }
            other => panic!("expected Literal(Date), got {other:?}"),
        }
    }

    #[test]
    fn fold_time_constants_recurses_into_operands() {
        // EXTRACT(YEAR FROM NOW()) — fold must descend into the inner
        // operand so the EXTRACT evaluator sees a Timestamp literal.
        let ts_ns = 1_746_316_800_i64 * 1_000_000_000;
        let expr =
            ScalarExpr::Extract(kimberlite_types::DateField::Year, Box::new(ScalarExpr::Now));
        let folded = fold_time_constants(&expr, ts_ns);
        match folded {
            ScalarExpr::Extract(field, inner) => {
                assert_eq!(field, kimberlite_types::DateField::Year);
                match *inner {
                    ScalarExpr::Literal(Value::Timestamp(_)) => {}
                    other => panic!("expected Literal(Timestamp) inner, got {other:?}"),
                }
            }
            other => panic!("expected Extract, got {other:?}"),
        }
    }

    #[test]
    fn fold_time_constants_is_idempotent_on_non_sentinel_exprs() {
        // Folding a tree without Now/CurrentTimestamp/CurrentDate is
        // a structural no-op (trivially: the fold function is a deep
        // clone that only rewrites three variants).
        let ts_ns = 1_746_316_800_i64 * 1_000_000_000;
        let expr = ScalarExpr::Upper(Box::new(ScalarExpr::Literal(Value::Text("hi".into()))));
        let folded = fold_time_constants(&expr, ts_ns);
        match folded {
            ScalarExpr::Upper(inner) => match *inner {
                ScalarExpr::Literal(Value::Text(s)) => assert_eq!(s, "hi"),
                other => panic!("unexpected inner: {other:?}"),
            },
            other => panic!("expected Upper, got {other:?}"),
        }
    }

    #[test]
    fn fold_time_constants_is_deterministic_with_same_clock() {
        // Two NOW() folds with the same statement_ts_ns must produce
        // identical Timestamp literals — that's the SQL standard's
        // statement-stable contract for NOW().
        let ts_ns = 1_746_316_800_i64 * 1_000_000_000;
        let a = fold_time_constants(&ScalarExpr::Now, ts_ns);
        let b = fold_time_constants(&ScalarExpr::Now, ts_ns);
        match (a, b) {
            (
                ScalarExpr::Literal(Value::Timestamp(ta)),
                ScalarExpr::Literal(Value::Timestamp(tb)),
            ) => assert_eq!(ta.as_nanos(), tb.as_nanos()),
            other => panic!("expected matching Timestamp literals: {other:?}"),
        }
    }

    fn find_scalar_columns(p: &QueryPlan) -> Option<&[crate::plan::ScalarColumnDef]> {
        match p {
            QueryPlan::Materialize { scalar_columns, .. } => Some(scalar_columns),
            QueryPlan::Aggregate { source, .. } => find_scalar_columns(source),
            _ => None,
        }
    }

    fn assert_no_sentinel(e: &ScalarExpr) {
        match e {
            ScalarExpr::Now | ScalarExpr::CurrentTimestamp | ScalarExpr::CurrentDate => {
                panic!("planner left an unfolded time-now sentinel in the plan");
            }
            ScalarExpr::Upper(x)
            | ScalarExpr::Lower(x)
            | ScalarExpr::Length(x)
            | ScalarExpr::Trim(x)
            | ScalarExpr::Abs(x)
            | ScalarExpr::Round(x)
            | ScalarExpr::Ceil(x)
            | ScalarExpr::Floor(x)
            | ScalarExpr::RoundScale(x, _)
            | ScalarExpr::Sqrt(x)
            | ScalarExpr::Substring(x, _)
            | ScalarExpr::Extract(_, x)
            | ScalarExpr::DateTrunc(_, x)
            | ScalarExpr::Cast(x, _) => assert_no_sentinel(x),
            ScalarExpr::Concat(xs) | ScalarExpr::Coalesce(xs) => {
                for x in xs {
                    assert_no_sentinel(x);
                }
            }
            ScalarExpr::Nullif(a, b) | ScalarExpr::Mod(a, b) | ScalarExpr::Power(a, b) => {
                assert_no_sentinel(a);
                assert_no_sentinel(b);
            }
            ScalarExpr::Literal(_) | ScalarExpr::Column(_) => {}
        }
    }

    #[test]
    fn plan_query_with_clock_folds_select_now() {
        // End-to-end: parse `SELECT NOW()`, plan it, verify the
        // resulting Materialize plan's scalar_columns contains a
        // Literal(Timestamp), not a raw `Now` sentinel.
        let schema = test_schema();
        let parsed = parse_test_select("SELECT NOW() FROM users");
        let ts_ns = 1_746_316_800_i64 * 1_000_000_000;
        let plan = plan_query_with_clock(&schema, &parsed, &[], ts_ns).unwrap();

        let scalars = find_scalar_columns(&plan).expect("plan must have scalar projection");
        assert!(!scalars.is_empty(), "expected at least one scalar column");
        for sc in scalars {
            assert_no_sentinel(&sc.expr);
        }
    }
}
