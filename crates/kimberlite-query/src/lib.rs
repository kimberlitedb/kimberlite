//! # kmb-query: SQL query layer for `Kimberlite` projections
//!
//! This crate provides a minimal SQL query engine for compliance lookups
//! against the projection store.
//!
//! ## SQL Subset
//!
//! Supported SQL features:
//! - `SELECT` with column list or `*`
//! - `FROM` single table or `JOIN` (`INNER`, `LEFT`, `RIGHT`, `FULL OUTER`,
//!   `CROSS`, with `ON` or `USING(...)` clauses)
//! - `WHERE` with comparison predicates (`=`, `<`, `>`, `<=`, `>=`, `IN`)
//! - `IN (SELECT ...)`, `EXISTS`, `NOT EXISTS` (uncorrelated subqueries)
//! - `ORDER BY` (ascending/descending)
//! - `LIMIT` and `OFFSET` — literal or `$N` parameter
//! - `GROUP BY` with aggregates (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`)
//! - Aggregate `FILTER (WHERE ...)` clauses, independent per aggregate
//! - `HAVING` with aggregate filtering
//! - `UNION` / `UNION ALL` / `INTERSECT` / `INTERSECT ALL` /
//!   `EXCEPT` / `EXCEPT ALL`
//! - `DISTINCT`
//! - JSON operators `->`, `->>`, `@>` in WHERE clauses
//! - `CASE` (searched and simple form)
//! - `WITH` (Common Table Expressions / CTEs), including `WITH RECURSIVE`
//!   via iterative fixed-point evaluation (depth cap 1000)
//! - Subqueries in FROM and JOIN (`SELECT * FROM (SELECT ...) AS t`)
//! - Window functions (`OVER`, `PARTITION BY`, `ROW_NUMBER`, `RANK`,
//!   `DENSE_RANK`, `LAG`, `LEAD`, `FIRST_VALUE`, `LAST_VALUE`)
//! - `ALTER TABLE` (ADD COLUMN, DROP COLUMN) — parser only; kernel
//!   execution pending
//! - Parameterized queries (`$1`, `$2`, ...) in WHERE, LIMIT, OFFSET, and
//!   DML values
//!
//! - Scalar functions in SELECT projection and WHERE predicates:
//!   `UPPER`, `LOWER`, `LENGTH`, `TRIM`, `CONCAT`, `||`, `ABS`,
//!   `ROUND`, `CEIL`/`CEILING`, `FLOOR`, `COALESCE`, `NULLIF`, `CAST`
//! - `ILIKE`, `NOT LIKE`, `NOT ILIKE` pattern matching
//! - `NOT IN (list)`, `NOT BETWEEN low AND high`
//!
//! Not yet supported:
//! - Correlated subqueries (uncorrelated only above)
//! - Clock-dependent functions (`NOW()`, `CURRENT_DATE`, `EXTRACT`,
//!   `DATE_TRUNC`) — deferred pending a clock-threading decision
//! - `MOD`, `POWER`, `SQRT`, `SUBSTRING` — deferred
//!
//! ## Usage
//!
//! ```ignore
//! use kimberlite_query::{QueryEngine, Schema, SchemaBuilder, ColumnDef, DataType, Value};
//! use kimberlite_store::{BTreeStore, TableId};
//!
//! // Define schema
//! let schema = SchemaBuilder::new()
//!     .table(
//!         "users",
//!         TableId::new(1),
//!         vec![
//!             ColumnDef::new("id", DataType::BigInt).not_null(),
//!             ColumnDef::new("name", DataType::Text).not_null(),
//!         ],
//!         vec!["id".into()],
//!     )
//!     .build();
//!
//! // Create engine
//! let engine = QueryEngine::new(schema);
//!
//! // Execute query
//! let mut store = BTreeStore::open("data/projections")?;
//! let result = engine.query(&mut store, "SELECT * FROM users WHERE id = $1", &[Value::BigInt(42)])?;
//! ```
//!
//! ## Point-in-Time Queries
//!
//! For compliance, you can query at a specific log position:
//!
//! ```ignore
//! let result = engine.query_at(
//!     &mut store,
//!     "SELECT * FROM users WHERE id = 1",
//!     &[],
//!     Offset::new(1000),  // Query state as of log position 1000
//! )?;
//! ```
//!
//! ## Scalar expressions (v0.5.1)
//!
//! The parser accepts scalar functions in SELECT projection and WHERE
//! predicates. Each of these queries parses cleanly and produces a
//! `ParsedStatement::Select` with either a `ScalarCmp` predicate or
//! entries in `scalar_projections`:
//!
//! ```
//! use kimberlite_query::{parse_statement, ParsedStatement, Predicate};
//!
//! // WHERE col NOT IN (list) — mirror of IN, v0.5.1.
//! let s = parse_statement("SELECT id FROM t WHERE x NOT IN (1, 2, 3)").unwrap();
//! let ParsedStatement::Select(sel) = s else { panic!() };
//! assert!(matches!(sel.predicates[0], Predicate::NotIn(_, _)));
//!
//! // WHERE UPPER(name) = 'ALICE' — scalar LHS routes to ScalarCmp.
//! let s = parse_statement("SELECT id FROM t WHERE UPPER(name) = 'ALICE'").unwrap();
//! let ParsedStatement::Select(sel) = s else { panic!() };
//! assert!(matches!(sel.predicates[0], Predicate::ScalarCmp { .. }));
//!
//! // SELECT col AS alias — alias preserved end-to-end.
//! let s = parse_statement("SELECT name AS display FROM t").unwrap();
//! let ParsedStatement::Select(sel) = s else { panic!() };
//! let aliases = sel.column_aliases.as_ref().unwrap();
//! assert_eq!(aliases[0].as_deref(), Some("display"));
//!
//! // SELECT CAST(x AS INTEGER) — lands in scalar_projections with a
//! // synthesised output column name.
//! let s = parse_statement("SELECT CAST(x AS INTEGER) FROM t").unwrap();
//! let ParsedStatement::Select(sel) = s else { panic!() };
//! assert_eq!(sel.scalar_projections.len(), 1);
//! assert_eq!(sel.scalar_projections[0].output_name.as_str(), "cast");
//! ```

pub mod dml_planner;
mod error;
mod executor;
pub mod explain;
pub mod expression;
pub mod information_schema;
pub mod key_encoder;
mod parse_cache;
mod parser;
mod plan;
mod planner;
pub mod rbac_filter;
mod schema;
mod value;
pub mod window;

#[cfg(test)]
mod tests;

// Re-export public types
pub use error::{QueryError, Result};
pub use executor::{QueryResult, Row, execute};
pub use expression::{EvalContext, ScalarExpr, evaluate};
pub use parser::{
    AlterTableOperation, HavingCondition, HavingOp, ParsedAlterTable, ParsedColumn,
    ParsedCreateIndex, ParsedCreateMask, ParsedCreateTable, ParsedCreateUser, ParsedCte,
    ParsedDelete, ParsedGrant, ParsedInsert, ParsedSelect, ParsedSetClassification,
    ParsedStatement, ParsedUnion, ParsedUpdate, Predicate, PredicateValue, ScalarCmpOp, TimeTravel,
    expr_to_scalar_expr, extract_at_offset, extract_time_travel, parse_statement,
    try_parse_custom_statement,
};
pub use planner::plan_query;
pub use schema::{
    ColumnDef, ColumnName, DataType, IndexDef, Schema, SchemaBuilder, TableDef, TableName,
};
pub use value::Value;

use kimberlite_store::ProjectionStore;
use kimberlite_types::Offset;

/// Query engine for executing SQL against a projection store.
///
/// Holds the schema plus an optional parse cache (AUDIT-2026-04
/// S3.4). The engine is `Clone` — the parse cache is shared via
/// `Arc` so cloned handles hit the same memoised entries.
#[derive(Debug, Clone)]
pub struct QueryEngine {
    schema: Schema,
    parse_cache: Option<std::sync::Arc<parse_cache::ParseCache>>,
}

impl QueryEngine {
    /// Creates a new query engine with the given schema. No
    /// parse cache is attached by default — use
    /// [`Self::with_parse_cache`] to opt in.
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            parse_cache: None,
        }
    }

    /// Attach an LRU parse cache of the given size. `0` disables
    /// caching (every call re-parses).
    #[must_use]
    pub fn with_parse_cache(mut self, max_size: usize) -> Self {
        self.parse_cache = Some(std::sync::Arc::new(parse_cache::ParseCache::new(max_size)));
        self
    }

    /// Returns a snapshot of parse-cache stats, or `None` if no
    /// cache is attached.
    pub fn parse_cache_stats(&self) -> Option<parse_cache::ParseCacheStats> {
        self.parse_cache
            .as_deref()
            .map(parse_cache::ParseCache::stats)
    }

    /// Clear the parse cache, if any.
    pub fn clear_parse_cache(&self) {
        if let Some(c) = &self.parse_cache {
            c.clear();
        }
    }

    /// Returns a reference to the schema.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Parses a SQL string and extracts the SELECT or UNION statement.
    ///
    /// Static variant — bypasses the parse cache. Used by call
    /// sites that predate the cache and by internal recursive
    /// parsers.
    fn parse_query_statement(sql: &str) -> Result<parser::ParsedStatement> {
        let stmt = parser::parse_statement(sql)?;
        match &stmt {
            parser::ParsedStatement::Select(_) | parser::ParsedStatement::Union(_) => Ok(stmt),
            _ => Err(QueryError::UnsupportedFeature(
                "only SELECT and UNION queries are supported".to_string(),
            )),
        }
    }

    /// Cache-aware parse wrapper.
    ///
    /// Looks the SQL up in the parse cache (if attached). On
    /// miss, parses via [`Self::parse_query_statement`] and
    /// inserts into the cache. Non-SELECT/UNION errors are
    /// returned directly without populating the cache — they're
    /// errors for every subsequent call anyway and we don't want
    /// to memoise them.
    fn parse_query_statement_cached(&self, sql: &str) -> Result<parser::ParsedStatement> {
        if let Some(cache) = &self.parse_cache {
            if let Some(stmt) = cache.get(sql) {
                return Ok(stmt);
            }
        }
        let stmt = Self::parse_query_statement(sql)?;
        if let Some(cache) = &self.parse_cache {
            cache.insert(sql.to_string(), stmt.clone());
        }
        Ok(stmt)
    }

    /// Executes a SQL query against the current store state.
    ///
    /// Supports SELECT and UNION/UNION ALL queries.
    ///
    /// # Arguments
    ///
    /// * `store` - The projection store to query
    /// * `sql` - SQL query string
    /// * `params` - Query parameters (for `$1`, `$2`, etc.)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = engine.query(
    ///     &mut store,
    ///     "SELECT name FROM users WHERE id = $1",
    ///     &[Value::BigInt(42)],
    /// )?;
    /// ```
    pub fn query<S: ProjectionStore>(
        &self,
        store: &mut S,
        sql: &str,
        params: &[Value],
    ) -> Result<QueryResult> {
        // AUDIT-2026-04 S3.5 — healthcare BREAK_GLASS prefix.
        // `WITH BREAK_GLASS REASON='...' SELECT ...` strips the
        // prefix, emits a warn-level structured log for the
        // caller's audit pipeline to pick up, and falls through
        // to the normal query path. The reason is the attribution
        // value — enforcement (RBAC + masking) is still applied.
        let (after_break_glass, break_glass_reason) = explain::extract_break_glass(sql);
        if let Some(ref reason) = break_glass_reason {
            tracing::warn!(
                break_glass_reason = %reason,
                "BREAK_GLASS query — regulator-visible audit signal",
            );
        }
        let sql = after_break_glass;

        // AUDIT-2026-04 S3.4 — `information_schema.*` virtual-table
        // interception. Synthesises results from the live schema
        // without going through the planner/executor. Callers
        // that point FROM at `information_schema.tables` or
        // `.columns` get back schema introspection rows without
        // the table needing to be registered in the store.
        if let Some(result) = information_schema::maybe_answer(sql, &self.schema) {
            return Ok(result);
        }

        // AUDIT-2026-04 S3.3 — EXPLAIN prefix dispatch. A caller
        // issuing `EXPLAIN SELECT ...` gets a single-row result
        // whose only value is the rendered plan tree, rather
        // than executing the statement.
        let (after_explain, is_explain) = explain::extract_explain(sql);
        if is_explain {
            let plan_text = self.explain(after_explain, params)?;
            return Ok(executor::QueryResult {
                columns: vec!["plan".into()],
                rows: vec![vec![Value::Text(plan_text)]],
            });
        }
        let sql = after_explain; // equivalent to original `sql` when EXPLAIN absent

        // Extract time-travel clause (AT OFFSET / FOR SYSTEM_TIME AS OF / AS OF)
        // before passing SQL to sqlparser. Offset syntax dispatches
        // directly; timestamp syntax without a resolver errors out
        // with a clear message pointing to
        // `query_at_timestamp(..., resolver)`.
        let (cleaned_sql, time_travel) = parser::extract_time_travel(sql);
        match time_travel {
            Some(parser::TimeTravel::Offset(o)) => {
                return self.query_at(store, &cleaned_sql, params, Offset::new(o));
            }
            Some(parser::TimeTravel::TimestampNs(_)) => {
                return Err(QueryError::UnsupportedFeature(
                    "FOR SYSTEM_TIME AS OF '<iso>' / AS OF '<iso>' \
                     requires a timestamp→offset resolver — use \
                     QueryEngine::query_at_timestamp(..., resolver)"
                        .to_string(),
                ));
            }
            None => {}
        }
        let sql = sql; // unmodified; suppress unused-shadow lint

        let stmt = self.parse_query_statement_cached(sql)?;

        match stmt {
            parser::ParsedStatement::Select(mut parsed) => {
                // Pre-execute uncorrelated subqueries (IN/EXISTS/NOT EXISTS)
                // and substitute their results into the predicates so the
                // planner sees a flat predicate tree.
                self.pre_execute_subqueries(store, &mut parsed.predicates, params)?;

                let window_fns = parsed.window_fns.clone();
                let result = if parsed.ctes.is_empty() {
                    let plan = planner::plan_query(&self.schema, &parsed, params)?;
                    let table_def = self
                        .schema
                        .get_table(&plan.table_name().into())
                        .ok_or_else(|| QueryError::TableNotFound(plan.table_name().to_string()))?;
                    executor::execute(store, &plan, table_def)?
                } else {
                    self.execute_with_ctes(store, &parsed, params)?
                };
                // AUDIT-2026-04 S3.2 — window functions are a
                // post-pass over the base SELECT result; the base
                // plan already projected the columns the window
                // fn references.
                window::apply_window_fns(result, &window_fns)
            }
            parser::ParsedStatement::Union(union_stmt) => {
                self.execute_union(store, &union_stmt, params)
            }
            _ => unreachable!("parse_query_statement only returns Select or Union"),
        }
    }

    /// Executes a SELECT with CTEs by materializing each CTE and building
    /// a temporary schema that includes the CTE result sets as tables.
    fn execute_with_ctes<S: ProjectionStore>(
        &self,
        store: &mut S,
        parsed: &parser::ParsedSelect,
        params: &[Value],
    ) -> Result<QueryResult> {
        // Build an extended schema that includes CTE-derived tables
        let mut extended_schema = self.schema.clone();

        // Materialize each CTE
        for cte in &parsed.ctes {
            // Execute the CTE's anchor (non-recursive) query
            let cte_plan = planner::plan_query(&extended_schema, &cte.query, params)?;
            let cte_table_def = extended_schema
                .get_table(&cte_plan.table_name().into())
                .ok_or_else(|| QueryError::TableNotFound(cte_plan.table_name().to_string()))?;
            let mut cte_result = executor::execute(store, &cte_plan, cte_table_def)?;

            // Register CTE result as a virtual table in the extended schema
            // Use a synthetic table ID based on the CTE name hash
            let cte_table_id = {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                cte.name.hash(&mut hasher);
                kimberlite_store::TableId::new(hasher.finish())
            };

            // Build column defs from the CTE result
            let cte_columns: Vec<schema::ColumnDef> = cte_result
                .columns
                .iter()
                .map(|col| schema::ColumnDef::new(col.as_str(), schema::DataType::Text))
                .collect();

            let pk_cols = if cte_columns.is_empty() {
                vec![]
            } else {
                vec![cte_result.columns[0].clone()]
            };

            let cte_table = schema::TableDef::new(cte_table_id, cte_columns, pk_cols);
            extended_schema.add_table(cte.name.as_str(), cte_table);

            // Write anchor rows into the store as the initial working set.
            let mut total_rows = cte_result.rows.len();
            for (row_idx, row) in cte_result.rows.iter().enumerate() {
                Self::write_cte_row(store, cte_table_id, row_idx, &cte_result.columns, row)?;
            }

            // Recursive iteration: keep evaluating the recursive arm against
            // the growing CTE table until no new rows are produced or the
            // depth cap is hit. Iterative fixed-point — honours the
            // workspace "no recursion" constraint and prevents runaway loops.
            if let Some(recursive_select) = &cte.recursive_arm {
                const MAX_RECURSIVE_DEPTH: usize = 1000;
                let mut seen: std::collections::HashSet<String> =
                    cte_result.rows.iter().map(|r| format!("{r:?}")).collect();

                for depth in 0..MAX_RECURSIVE_DEPTH {
                    let recursive_plan =
                        planner::plan_query(&extended_schema, recursive_select, params)?;
                    let recursive_table_def = extended_schema
                        .get_table(&recursive_plan.table_name().into())
                        .ok_or_else(|| {
                            QueryError::TableNotFound(recursive_plan.table_name().to_string())
                        })?;
                    let iteration_result =
                        executor::execute(store, &recursive_plan, recursive_table_def)?;

                    let mut new_rows = 0usize;
                    for row in iteration_result.rows {
                        let key = format!("{row:?}");
                        if seen.insert(key) {
                            Self::write_cte_row(
                                store,
                                cte_table_id,
                                total_rows,
                                &cte_result.columns,
                                &row,
                            )?;
                            cte_result.rows.push(row);
                            total_rows += 1;
                            new_rows += 1;
                        }
                    }
                    if new_rows == 0 {
                        break;
                    }
                    if depth + 1 == MAX_RECURSIVE_DEPTH {
                        return Err(QueryError::UnsupportedFeature(format!(
                            "recursive CTE `{}` exceeded maximum depth of {MAX_RECURSIVE_DEPTH} iterations",
                            cte.name
                        )));
                    }
                }
            }
        }

        // Execute the main query against the extended schema
        let main_query = parser::ParsedSelect {
            ctes: vec![], // CTEs already materialized
            ..parsed.clone()
        };

        let plan = planner::plan_query(&extended_schema, &main_query, params)?;
        let table_def = extended_schema
            .get_table(&plan.table_name().into())
            .ok_or_else(|| QueryError::TableNotFound(plan.table_name().to_string()))?;
        executor::execute(store, &plan, table_def)
    }

    /// Writes a single CTE row into the store under the synthetic table id.
    /// Helper extracted so the recursive-CTE iteration loop and the initial
    /// anchor materialisation share the same path.
    fn write_cte_row<S: ProjectionStore>(
        store: &mut S,
        cte_table_id: kimberlite_store::TableId,
        row_idx: usize,
        columns: &[crate::schema::ColumnName],
        row: &[Value],
    ) -> Result<()> {
        let mut row_map = serde_json::Map::new();
        for (col, val) in columns.iter().zip(row.iter()) {
            row_map.insert(col.as_str().to_string(), value_to_json(val));
        }
        let json_val = serde_json::to_vec(&serde_json::Value::Object(row_map)).map_err(|e| {
            QueryError::UnsupportedFeature(format!("CTE serialization failed: {e}"))
        })?;
        let pk_key = crate::key_encoder::encode_key(&[Value::BigInt(row_idx as i64)]);
        let batch = kimberlite_store::WriteBatch::new(kimberlite_types::Offset::new(
            store.applied_position().as_u64() + 1,
        ))
        .put(cte_table_id, pk_key, bytes::Bytes::from(json_val));
        store.apply(batch)?;
        Ok(())
    }

    /// Walks the predicate tree, pre-executes any uncorrelated subqueries
    /// (`IN (SELECT ...)`, `EXISTS (...)`, `NOT EXISTS (...)`), and rewrites
    /// the predicate tree in-place so the planner sees only flat predicates.
    ///
    /// `IN (SELECT col FROM t WHERE ...)` becomes `IN (val1, val2, ...)`.
    /// `EXISTS (...)` becomes a tautology when the subquery has rows, or a
    /// contradiction when it doesn't. `NOT EXISTS` is the inverse.
    fn pre_execute_subqueries<S: ProjectionStore>(
        &self,
        store: &mut S,
        predicates: &mut Vec<parser::Predicate>,
        params: &[Value],
    ) -> Result<()> {
        for pred in predicates.iter_mut() {
            match pred {
                parser::Predicate::InSubquery { column, subquery } => {
                    let inner_plan = planner::plan_query(&self.schema, subquery, params)?;
                    let inner_table_def = self
                        .schema
                        .get_table(&inner_plan.table_name().into())
                        .ok_or_else(|| {
                            QueryError::TableNotFound(inner_plan.table_name().to_string())
                        })?;
                    let inner_result = executor::execute(store, &inner_plan, inner_table_def)?;
                    if inner_result.columns.len() != 1 {
                        return Err(QueryError::UnsupportedFeature(format!(
                            "IN (SELECT ...) subquery must project exactly 1 column, got {}",
                            inner_result.columns.len()
                        )));
                    }
                    let values: Vec<parser::PredicateValue> = inner_result
                        .rows
                        .into_iter()
                        .filter_map(|row| row.into_iter().next())
                        .map(parser::PredicateValue::Literal)
                        .collect();
                    *pred = parser::Predicate::In(column.clone(), values);
                }
                parser::Predicate::Exists { subquery, negated } => {
                    let inner_plan = planner::plan_query(&self.schema, subquery, params)?;
                    let inner_table_def = self
                        .schema
                        .get_table(&inner_plan.table_name().into())
                        .ok_or_else(|| {
                            QueryError::TableNotFound(inner_plan.table_name().to_string())
                        })?;
                    let inner_result = executor::execute(store, &inner_plan, inner_table_def)?;
                    let exists = !inner_result.rows.is_empty();
                    let predicate_holds = if *negated { !exists } else { exists };
                    *pred = parser::Predicate::Always(predicate_holds);
                }
                parser::Predicate::Or(left, right) => {
                    self.pre_execute_subqueries(store, left, params)?;
                    self.pre_execute_subqueries(store, right, params)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Executes a `UNION`, `INTERSECT`, or `EXCEPT` query (with or without `ALL`).
    ///
    /// Implementation: materialise both sides, then combine according to the
    /// operator. `ALL` keeps multiset semantics; the bare form (no `ALL`)
    /// deduplicates by row content. `Value` doesn't implement `Hash`, so the
    /// dedup/intersect/except keys use the debug format of each row — same
    /// trick already used by the prior UNION implementation.
    fn execute_union<S: ProjectionStore>(
        &self,
        store: &mut S,
        union_stmt: &parser::ParsedUnion,
        params: &[Value],
    ) -> Result<QueryResult> {
        // Plan and execute left side
        let left_plan = planner::plan_query(&self.schema, &union_stmt.left, params)?;
        let left_table_def = self
            .schema
            .get_table(&left_plan.table_name().into())
            .ok_or_else(|| QueryError::TableNotFound(left_plan.table_name().to_string()))?;
        let left_result = executor::execute(store, &left_plan, left_table_def)?;

        // Plan and execute right side
        let right_plan = planner::plan_query(&self.schema, &union_stmt.right, params)?;
        let right_table_def = self
            .schema
            .get_table(&right_plan.table_name().into())
            .ok_or_else(|| QueryError::TableNotFound(right_plan.table_name().to_string()))?;
        let right_result = executor::execute(store, &right_plan, right_table_def)?;

        // Use left side column names for the result
        let column_names = left_result.columns;

        let row_key = |row: &Vec<Value>| format!("{row:?}");

        let combined_rows: Vec<Vec<Value>> = match (union_stmt.op, union_stmt.all) {
            // UNION ALL: concatenate, keep duplicates
            (parser::SetOp::Union, true) => {
                let mut all_rows = left_result.rows;
                all_rows.extend(right_result.rows);
                all_rows
            }
            // UNION: concatenate then dedup
            (parser::SetOp::Union, false) => {
                let mut all_rows = left_result.rows;
                all_rows.extend(right_result.rows);
                let mut seen = std::collections::HashSet::new();
                all_rows.retain(|row| seen.insert(row_key(row)));
                all_rows
            }
            // INTERSECT: rows present in both sides (set semantics)
            (parser::SetOp::Intersect, false) => {
                let right_keys: std::collections::HashSet<String> =
                    right_result.rows.iter().map(&row_key).collect();
                let mut seen = std::collections::HashSet::new();
                left_result
                    .rows
                    .into_iter()
                    .filter(|row| {
                        let key = row_key(row);
                        right_keys.contains(&key) && seen.insert(key)
                    })
                    .collect()
            }
            // INTERSECT ALL: keep multiplicities — for each row appearing
            // min(left_count, right_count) times, emit that many copies.
            (parser::SetOp::Intersect, true) => {
                let mut right_counts: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                for row in &right_result.rows {
                    *right_counts.entry(row_key(row)).or_insert(0) += 1;
                }
                let mut out = Vec::new();
                for row in left_result.rows {
                    let key = row_key(&row);
                    if let Some(count) = right_counts.get_mut(&key) {
                        if *count > 0 {
                            *count -= 1;
                            out.push(row);
                        }
                    }
                }
                out
            }
            // EXCEPT: rows in left side not in right side (set semantics)
            (parser::SetOp::Except, false) => {
                let right_keys: std::collections::HashSet<String> =
                    right_result.rows.iter().map(&row_key).collect();
                let mut seen = std::collections::HashSet::new();
                left_result
                    .rows
                    .into_iter()
                    .filter(|row| {
                        let key = row_key(row);
                        !right_keys.contains(&key) && seen.insert(key)
                    })
                    .collect()
            }
            // EXCEPT ALL: subtract multiplicities — left_count - right_count copies
            (parser::SetOp::Except, true) => {
                let mut right_counts: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                for row in &right_result.rows {
                    *right_counts.entry(row_key(row)).or_insert(0) += 1;
                }
                let mut out = Vec::new();
                for row in left_result.rows {
                    let key = row_key(&row);
                    let count = right_counts.entry(key).or_insert(0);
                    if *count > 0 {
                        *count -= 1;
                    } else {
                        out.push(row);
                    }
                }
                out
            }
        };

        Ok(QueryResult {
            columns: column_names,
            rows: combined_rows,
        })
    }

    /// Executes a SQL query at a specific log position (point-in-time query).
    ///
    /// This enables compliance queries that show the state as it was
    /// at a specific point in the log.
    ///
    /// # Arguments
    ///
    /// * `store` - The projection store to query
    /// * `sql` - SQL query string
    /// * `params` - Query parameters
    /// * `position` - Log position to query at
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Get user state as of log position 1000
    /// let result = engine.query_at(
    ///     &mut store,
    ///     "SELECT * FROM users WHERE id = 1",
    ///     &[],
    ///     Offset::new(1000),
    /// )?;
    /// ```
    pub fn query_at<S: ProjectionStore>(
        &self,
        store: &mut S,
        sql: &str,
        params: &[Value],
        position: Offset,
    ) -> Result<QueryResult> {
        let stmt = self.parse_query_statement_cached(sql)?;

        match stmt {
            parser::ParsedStatement::Select(parsed) => {
                let plan = planner::plan_query(&self.schema, &parsed, params)?;
                let table_def = self
                    .schema
                    .get_table(&plan.table_name().into())
                    .ok_or_else(|| QueryError::TableNotFound(plan.table_name().to_string()))?;
                executor::execute_at(store, &plan, table_def, position)
            }
            parser::ParsedStatement::Union(_) => Err(QueryError::UnsupportedFeature(
                "UNION is not supported in point-in-time queries".to_string(),
            )),
            _ => unreachable!("parse_query_statement only returns Select or Union"),
        }
    }

    /// Executes a query against a historical snapshot selected by
    /// wall-clock timestamp (AUDIT-2026-04 L-4).
    ///
    /// This is the user-facing ergonomic form of
    /// [`Self::query_at`] — healthcare auditors ask "what did the
    /// chart look like on 2026-01-15?", not "what was log offset
    /// 948,274?". The caller supplies a `resolver` callback that
    /// translates a Unix-nanosecond timestamp into the log offset
    /// whose commit timestamp is the greatest value ≤ the target.
    ///
    /// The resolver is a callback rather than a hard dependency
    /// so the query crate does not take a direct dep on
    /// `kimberlite-compliance::audit` or the kernel's audit log.
    /// A typical impl performs a binary search on the in-memory
    /// audit index.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnsupportedFeature`] if the resolver
    ///   returns `None` (no offset exists at or before the target
    ///   — typically because the log is empty or the timestamp
    ///   predates genesis).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let resolver = |ts_ns: i64| -> Option<Offset> {
    ///     audit_log.offset_at_or_before(ts_ns)
    /// };
    /// let result = engine.query_at_timestamp(
    ///     &mut store,
    ///     "SELECT * FROM charts WHERE patient_id = $1",
    ///     &[Value::BigInt(42)],
    ///     1_760_000_000_000_000_000, // 2025-10-09T07:06:40Z in ns
    ///     resolver,
    /// )?;
    /// ```
    pub fn query_at_timestamp<S, R>(
        &self,
        store: &mut S,
        sql: &str,
        params: &[Value],
        target_ns: i64,
        resolver: R,
    ) -> Result<QueryResult>
    where
        S: ProjectionStore,
        R: FnOnce(i64) -> Option<Offset>,
    {
        let offset = resolver(target_ns).ok_or_else(|| {
            QueryError::UnsupportedFeature(format!(
                "no log offset at or before timestamp {target_ns} ns \
                 (empty log or predates genesis)"
            ))
        })?;
        self.query_at(store, sql, params, offset)
    }

    /// AUDIT-2026-04 S3.3 — render a SQL query's access plan
    /// without executing it.
    ///
    /// Returns a deterministic multi-line tree string — same
    /// query always produces the same bytes, which lets apps
    /// diff plans across schema versions and catch unexpected
    /// regressions.
    ///
    /// The rendered plan **never reveals row data** — only table
    /// names, column counts, filter presence/absence, and LIMIT
    /// bounds. Masked column names render as their source name
    /// (masks are applied post-projection and are not a plan
    /// concern).
    ///
    /// # Errors
    ///
    /// Any error from parsing or planning (unsupported statement,
    /// missing table, etc.) propagates verbatim.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tree = engine.explain("SELECT * FROM patients WHERE id = $1", &[Value::BigInt(42)])?;
    /// println!("{tree}");
    /// // -> PointLookup [patients, cols=3]
    /// ```
    pub fn explain(&self, sql: &str, params: &[Value]) -> Result<String> {
        let stmt = self.parse_query_statement_cached(sql)?;
        match stmt {
            parser::ParsedStatement::Select(parsed) => {
                let plan = planner::plan_query(&self.schema, &parsed, params)?;
                Ok(explain::explain_plan(&plan))
            }
            parser::ParsedStatement::Union(_) => Err(QueryError::UnsupportedFeature(
                "EXPLAIN does not yet render UNION plans".to_string(),
            )),
            _ => unreachable!("parse_query_statement only returns Select or Union"),
        }
    }

    /// Parses a SQL query without executing it.
    ///
    /// Useful for validation or query plan inspection.
    pub fn prepare(&self, sql: &str, params: &[Value]) -> Result<PreparedQuery> {
        let stmt = self.parse_query_statement_cached(sql)?;
        let parsed = match stmt {
            parser::ParsedStatement::Select(s) => s,
            _ => {
                return Err(QueryError::UnsupportedFeature(
                    "only SELECT queries can be prepared".to_string(),
                ));
            }
        };
        let plan = planner::plan_query(&self.schema, &parsed, params)?;

        Ok(PreparedQuery {
            plan,
            schema: self.schema.clone(),
        })
    }
}

/// A prepared (planned) query ready for execution.
#[derive(Debug, Clone)]
pub struct PreparedQuery {
    plan: plan::QueryPlan,
    schema: Schema,
}

impl PreparedQuery {
    /// Executes this prepared query against the current store state.
    pub fn execute<S: ProjectionStore>(&self, store: &mut S) -> Result<QueryResult> {
        let table_def = self
            .schema
            .get_table(&self.plan.table_name().into())
            .ok_or_else(|| QueryError::TableNotFound(self.plan.table_name().to_string()))?;

        executor::execute(store, &self.plan, table_def)
    }

    /// Executes this prepared query at a specific log position.
    pub fn execute_at<S: ProjectionStore>(
        &self,
        store: &mut S,
        position: Offset,
    ) -> Result<QueryResult> {
        let table_def = self
            .schema
            .get_table(&self.plan.table_name().into())
            .ok_or_else(|| QueryError::TableNotFound(self.plan.table_name().to_string()))?;

        executor::execute_at(store, &self.plan, table_def, position)
    }

    /// Returns the column names this query will return.
    pub fn columns(&self) -> &[ColumnName] {
        self.plan.column_names()
    }

    /// Returns the table name being queried.
    pub fn table_name(&self) -> &str {
        self.plan.table_name()
    }
}

/// Converts a Value to a serde_json::Value for CTE materialization.
fn value_to_json(val: &Value) -> serde_json::Value {
    // NEVER: Placeholder values must be bound before reaching the CTE /
    // query-result JSON boundary. An unbound Placeholder here indicates a
    // parameter-binding bug (AUDIT fix: placeholders must be resolved upstream).
    kimberlite_properties::never!(
        matches!(val, Value::Placeholder(_)),
        "query.placeholder_reaches_result_boundary",
        "Value::Placeholder must never reach query-result / JSON serialization boundary"
    );
    match val {
        Value::Null | Value::Placeholder(_) => serde_json::Value::Null,
        Value::BigInt(i) => serde_json::json!(i),
        Value::TinyInt(i) => serde_json::json!(i),
        Value::SmallInt(i) => serde_json::json!(i),
        Value::Integer(i) => serde_json::json!(i),
        Value::Real(f) => serde_json::json!(f),
        Value::Decimal(v, scale) => {
            // Format decimal: store the raw value and scale as a string
            if *scale == 0 {
                serde_json::json!(v.to_string())
            } else {
                let divisor = 10i128.pow(u32::from(*scale));
                let whole = v / divisor;
                let frac = (v % divisor).unsigned_abs();
                serde_json::json!(format!("{whole}.{frac:0>width$}", width = *scale as usize))
            }
        }
        Value::Text(s) => serde_json::json!(s),
        Value::Boolean(b) => serde_json::json!(b),
        Value::Date(d) => serde_json::json!(d),
        Value::Time(t) => serde_json::json!(t),
        Value::Timestamp(ts) => serde_json::json!(ts.as_nanos()),
        Value::Uuid(u) => {
            // Format UUID bytes as hex string
            let hex: String = u.iter().map(|b| format!("{b:02x}")).collect();
            serde_json::json!(hex)
        }
        Value::Json(j) => j.clone(),
        Value::Bytes(b) => {
            use base64::Engine;
            serde_json::json!(base64::engine::general_purpose::STANDARD.encode(b))
        }
    }
}
