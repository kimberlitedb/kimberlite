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
//! - `IN (SELECT)`, `NOT IN (SELECT)`, `EXISTS`, `NOT EXISTS` — both
//!   uncorrelated (pre-execute fast path) and correlated (semi-join
//!   decorrelation or nested-loop fallback; see
//!   `docs/reference/sql/correlated-subqueries.md`)
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
//! - Scalar subquery `WHERE col = (SELECT ...)`, `ANY`, `ALL`, `SOME`
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

pub mod correlated;
pub mod depth_check;
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

/// Outcome returned by a timestamp→offset resolver.
///
/// v0.6.0 Tier 2 #6 — a resolver that owns its own index (the
/// runtime's in-memory timestamp index, an audit-log-backed index,
/// etc.) can distinguish three cases that `Option<Offset>` cannot:
///
/// - We found an offset at or before the requested timestamp.
/// - The log has entries, but the earliest is *after* the requested
///   timestamp — i.e. the request predates the retention horizon.
///   Surfacing this as a distinct variant lets the query layer emit
///   [`QueryError::AsOfBeforeRetentionHorizon`] with the horizon
///   attached, which is far more actionable to the caller.
/// - The log is empty — no timestamps recorded yet.
///
/// See [`QueryEngine::query_at_timestamp_resolved`] for the
/// consumer of this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampResolution {
    /// Resolver found a projection offset whose commit timestamp is
    /// the greatest value ≤ the target.
    Offset(Offset),
    /// Resolver has entries, but the earliest commit timestamp is
    /// strictly greater than the target. `horizon_ns` is that
    /// earliest timestamp, so callers can tell users "the oldest
    /// retained data is <horizon>, try a later instant".
    BeforeRetentionHorizon { horizon_ns: i64 },
    /// Resolver has no entries at all (fresh DB or the index hasn't
    /// been seeded yet). Indistinguishable from "predates genesis"
    /// in observable behaviour — surfaced via `UnsupportedFeature`.
    LogEmpty,
}

/// Query engine for executing SQL against a projection store.
///
/// Holds the schema plus an optional parse cache (AUDIT-2026-04
/// S3.4). The engine is `Clone` — the parse cache is shared via
/// `Arc` so cloned handles hit the same memoised entries.
#[derive(Debug, Clone)]
pub struct QueryEngine {
    schema: Schema,
    parse_cache: Option<std::sync::Arc<parse_cache::ParseCache>>,
    /// Cap on `outer_rows × inner_rows_per_iter` for correlated
    /// subquery loops. See `docs/reference/sql/correlated-subqueries.md`.
    /// Defaults to [`correlated::DEFAULT_CORRELATED_CAP`] (10 million).
    correlated_cap: u64,
}

impl QueryEngine {
    /// Creates a new query engine with the given schema. No
    /// parse cache is attached by default — use
    /// [`Self::with_parse_cache`] to opt in.
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            parse_cache: None,
            correlated_cap: correlated::DEFAULT_CORRELATED_CAP,
        }
    }

    /// Attach an LRU parse cache of the given size. `0` disables
    /// caching (every call re-parses).
    #[must_use]
    pub fn with_parse_cache(mut self, max_size: usize) -> Self {
        self.parse_cache = Some(std::sync::Arc::new(parse_cache::ParseCache::new(max_size)));
        self
    }

    /// Override the correlated-subquery row-evaluation cap. Defaults
    /// to 10,000,000. Queries whose estimated
    /// `outer_rows × inner_rows_per_iter` exceeds this cap fail with
    /// [`QueryError::CorrelatedCardinalityExceeded`] before the
    /// correlated loop runs. Set to `u64::MAX` to effectively disable.
    #[must_use]
    pub fn with_correlated_cap(mut self, cap: u64) -> Self {
        self.correlated_cap = cap;
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

        let stmt = self.parse_query_statement_cached(sql)?;

        match stmt {
            parser::ParsedStatement::Select(mut parsed) => {
                // Pre-execute uncorrelated subqueries (IN/EXISTS/NOT EXISTS),
                // attempt semi-join decorrelation of correlated ones, and
                // leave remaining correlated predicates in place for the
                // correlated-loop executor.
                //
                // `outer_scope` is the enclosing scope stack visible to the
                // subquery; at the top level we seed it with the outer
                // SELECT's FROM table so the walker can detect
                // correlation.
                self.pre_execute_subqueries(store, &mut parsed, params)?;

                let window_fns = parsed.window_fns.clone();
                let result = if parsed.ctes.is_empty() {
                    if has_correlated_predicate(&parsed.predicates) {
                        self.execute_correlated_query(store, &parsed, params)?
                    } else {
                        let plan = planner::plan_query(&self.schema, &parsed, params)?;
                        let table_def =
                            self.schema.get_table(&plan.table_name().into()).ok_or_else(
                                || QueryError::TableNotFound(plan.table_name().to_string()),
                            )?;
                        executor::execute(store, &plan, table_def)?
                    }
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

    /// Walks the outer SELECT's predicate tree, classifies each
    /// subquery as uncorrelated or correlated, and rewrites as follows:
    ///
    /// - **Uncorrelated** `IN (SELECT)` / `EXISTS` / `NOT EXISTS` →
    ///   pre-executed once; result substituted into `Predicate::In` /
    ///   `Predicate::NotIn` / `Predicate::Always(bool)`. Matches the
    ///   v0.5.0 fast path.
    /// - **Correlated** `EXISTS` / `NOT EXISTS` with a single equijoin
    ///   to the outer scope: rewritten by
    ///   [`correlated::try_semi_join_rewrite`] into
    ///   `Predicate::InSubquery { negated }` against the outer column,
    ///   then the uncorrelated path pre-executes it.
    /// - **Correlated** everything else: left in place for
    ///   [`Self::execute_correlated_query`] to handle per-outer-row.
    ///
    /// Assertion: on return, every `InSubquery` / `Exists` still in
    /// `parsed.predicates` is correlated. The caller's
    /// `has_correlated_predicate` check uses that invariant to
    /// dispatch.
    fn pre_execute_subqueries<S: ProjectionStore>(
        &self,
        store: &mut S,
        parsed: &mut parser::ParsedSelect,
        params: &[Value],
    ) -> Result<()> {
        // Build the outer scope — the outer SELECT's FROM table (plus
        // any JOIN tables) as the enclosing scope for each inner
        // subquery.
        let outer_scope = self.build_outer_scope(parsed);

        let preds = std::mem::take(&mut parsed.predicates);
        let mut out = Vec::with_capacity(preds.len());
        for pred in preds {
            out.push(self.classify_and_rewrite_predicate(store, pred, &outer_scope, params)?);
        }
        parsed.predicates = out;
        Ok(())
    }

    /// Construct the outer `PlannerScope` for `parsed` — the visible
    /// tables in the outer SELECT. FROM first, then any JOIN tables.
    fn build_outer_scope<'s>(&'s self, parsed: &parser::ParsedSelect) -> correlated::PlannerScope<'s> {
        let mut bindings: Vec<(String, &schema::TableDef)> = Vec::new();
        if let Some(t) = self.schema.get_table(&parsed.table.clone().into()) {
            bindings.push((parsed.table.clone(), t));
        }
        for join in &parsed.joins {
            if let Some(t) = self.schema.get_table(&join.table.clone().into()) {
                bindings.push((join.table.clone(), t));
            }
        }
        correlated::PlannerScope::empty().push(bindings)
    }

    /// Classifier for a single predicate — drives pre-execute vs.
    /// semi-join rewrite vs. leave-as-correlated. Recursive on `Or`.
    fn classify_and_rewrite_predicate<S: ProjectionStore>(
        &self,
        store: &mut S,
        pred: parser::Predicate,
        outer_scope: &correlated::PlannerScope<'_>,
        params: &[Value],
    ) -> Result<parser::Predicate> {
        match pred {
            parser::Predicate::InSubquery {
                column,
                subquery,
                negated,
            } => {
                let outer_refs = correlated::collect_outer_refs(&subquery, outer_scope, &self.schema);
                if outer_refs.is_empty() {
                    // Uncorrelated — pre-execute and substitute.
                    self.pre_execute_uncorrelated_in(
                        store, &column, &subquery, negated, params,
                    )
                } else {
                    // Correlated IN/NOT IN — keep the predicate
                    // in place; the correlated-loop executor handles it.
                    Ok(parser::Predicate::InSubquery {
                        column,
                        subquery,
                        negated,
                    })
                }
            }
            parser::Predicate::Exists { subquery, negated } => {
                let outer_refs = correlated::collect_outer_refs(&subquery, outer_scope, &self.schema);
                if outer_refs.is_empty() {
                    // Uncorrelated.
                    self.pre_execute_uncorrelated_exists(store, &subquery, negated, params)
                } else if let Some((outer_col, rewritten)) =
                    correlated::try_semi_join_rewrite(&subquery, negated, &outer_refs)
                {
                    // Decorrelated: rewrite as IN (SELECT) / NOT IN (SELECT)
                    // against the outer column, then pre-execute.
                    self.pre_execute_uncorrelated_in(
                        store, &outer_col, &rewritten, negated, params,
                    )
                } else {
                    // Correlated loop fallback.
                    Ok(parser::Predicate::Exists { subquery, negated })
                }
            }
            parser::Predicate::Or(left, right) => {
                let mut new_left = Vec::with_capacity(left.len());
                for p in left {
                    new_left.push(self.classify_and_rewrite_predicate(
                        store,
                        p,
                        outer_scope,
                        params,
                    )?);
                }
                let mut new_right = Vec::with_capacity(right.len());
                for p in right {
                    new_right.push(self.classify_and_rewrite_predicate(
                        store,
                        p,
                        outer_scope,
                        params,
                    )?);
                }
                Ok(parser::Predicate::Or(new_left, new_right))
            }
            other => Ok(other),
        }
    }

    /// Pre-execute an uncorrelated `IN (SELECT)` / `NOT IN (SELECT)`
    /// and return the substituted `Predicate::In` / `Predicate::NotIn`.
    fn pre_execute_uncorrelated_in<S: ProjectionStore>(
        &self,
        store: &mut S,
        column: &schema::ColumnName,
        subquery: &parser::ParsedSelect,
        negated: bool,
        params: &[Value],
    ) -> Result<parser::Predicate> {
        let inner_plan = planner::plan_query(&self.schema, subquery, params)?;
        let inner_table_def = self
            .schema
            .get_table(&inner_plan.table_name().into())
            .ok_or_else(|| QueryError::TableNotFound(inner_plan.table_name().to_string()))?;
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
        Ok(if negated {
            parser::Predicate::NotIn(column.clone(), values)
        } else {
            parser::Predicate::In(column.clone(), values)
        })
    }

    /// Pre-execute an uncorrelated `EXISTS` / `NOT EXISTS` and return
    /// the collapsed `Predicate::Always(bool)`.
    fn pre_execute_uncorrelated_exists<S: ProjectionStore>(
        &self,
        store: &mut S,
        subquery: &parser::ParsedSelect,
        negated: bool,
        params: &[Value],
    ) -> Result<parser::Predicate> {
        let inner_plan = planner::plan_query(&self.schema, subquery, params)?;
        let inner_table_def = self
            .schema
            .get_table(&inner_plan.table_name().into())
            .ok_or_else(|| QueryError::TableNotFound(inner_plan.table_name().to_string()))?;
        let inner_result = executor::execute(store, &inner_plan, inner_table_def)?;
        let exists = !inner_result.rows.is_empty();
        let predicate_holds = if negated { !exists } else { exists };
        Ok(parser::Predicate::Always(predicate_holds))
    }

    /// Execute a SELECT whose predicate list still contains at least
    /// one correlated subquery (InSubquery or Exists that survived
    /// `pre_execute_subqueries`).
    ///
    /// Strategy: split the predicate list into "simple" (non-correlated)
    /// predicates and "correlated" ones. Plan the outer query using
    /// only the simple predicates. For each returned row, substitute
    /// outer column values into each correlated inner subquery and
    /// execute it; keep the row iff all correlated predicates pass.
    fn execute_correlated_query<S: ProjectionStore>(
        &self,
        store: &mut S,
        parsed: &parser::ParsedSelect,
        params: &[Value],
    ) -> Result<QueryResult> {
        // Split simple vs. correlated predicates.
        let mut simple_preds: Vec<parser::Predicate> = Vec::new();
        let mut correlated_preds: Vec<parser::Predicate> = Vec::new();
        for pred in &parsed.predicates {
            match pred {
                parser::Predicate::InSubquery { .. } | parser::Predicate::Exists { .. } => {
                    correlated_preds.push(pred.clone());
                }
                other => simple_preds.push(other.clone()),
            }
        }

        // Build the outer query stripped of correlated predicates — we
        // need the FULL outer row (all columns) so we can substitute
        // outer column values into each inner subquery, regardless of
        // which columns the user SELECTed. We'll project to the
        // requested columns after the row-by-row filter.
        let outer_table_def = self
            .schema
            .get_table(&parsed.table.clone().into())
            .ok_or_else(|| QueryError::TableNotFound(parsed.table.clone()))?;
        let outer_scan = parser::ParsedSelect {
            predicates: simple_preds,
            columns: None, // force SELECT * so we have every column available
            column_aliases: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
            aggregates: Vec::new(),
            aggregate_filters: Vec::new(),
            group_by: Vec::new(),
            distinct: false,
            having: Vec::new(),
            ctes: Vec::new(),
            window_fns: Vec::new(),
            scalar_projections: Vec::new(),
            case_columns: Vec::new(),
            joins: Vec::new(),
            ..parsed.clone()
        };

        let outer_plan = planner::plan_query(&self.schema, &outer_scan, params)?;
        let outer_rows = executor::execute(store, &outer_plan, outer_table_def)?;

        // Estimate correlated row-evaluation cost for cardinality guard.
        //
        // Inner cost per outer row is bounded by the total inner-table
        // row count; we use the store's current table size as an upper
        // bound. If multiple correlated predicates reference the same
        // or different inner tables, sum the per-predicate cost.
        let outer_count = outer_rows.rows.len() as u64;
        let mut inner_cost_per_row: u64 = 0;
        for pred in &correlated_preds {
            let inner_table = match pred {
                parser::Predicate::InSubquery { subquery, .. }
                | parser::Predicate::Exists { subquery, .. } => &subquery.table,
                _ => continue,
            };
            let inner_def = self
                .schema
                .get_table(&inner_table.clone().into())
                .ok_or_else(|| QueryError::TableNotFound(inner_table.clone()))?;
            // Upper-bound the inner cost by scanning the table once — we
            // issue a bounded scan so this is cheap.
            let pairs =
                store.scan(inner_def.table_id, kimberlite_store::Key::min()..kimberlite_store::Key::max(), 1_000_000)?;
            inner_cost_per_row = inner_cost_per_row.saturating_add(pairs.len() as u64);
        }
        // When inner tables are empty, bound by 1 to keep estimation
        // monotonic (so a 0 × N query doesn't look free).
        let inner_cost_per_row = inner_cost_per_row.max(1);
        let estimated = outer_count.saturating_mul(inner_cost_per_row);
        if estimated > self.correlated_cap {
            return Err(QueryError::CorrelatedCardinalityExceeded {
                estimated,
                cap: self.correlated_cap,
            });
        }

        // For each outer row, evaluate the correlated predicates.
        let outer_columns = outer_rows.columns.clone();
        let outer_alias = parsed.table.clone();
        let mut kept: Vec<Vec<Value>> = Vec::new();
        for row in outer_rows.rows {
            // Build the `"alias.column"` → Value binding map. We bind
            // the FROM alias as it appears in the ParsedSelect (the
            // parser already resolved user aliases into the `table`
            // field when the alias shadows the table name).
            let mut bindings = std::collections::HashMap::new();
            for (col, val) in outer_columns.iter().zip(row.iter()) {
                bindings.insert(format!("{outer_alias}.{col}"), val.clone());
                // Also bind under every possible alias seen in the
                // correlated predicates — the user may have written
                // `p.id` while the parser stored the table name
                // `patient_current`. We cover the common case by
                // also binding the bare column name and any alias
                // we can extract from the inner refs themselves.
                bindings.insert(col.as_str().to_string(), val.clone());
            }
            // Extend bindings with each correlated-ref alias. Walking
            // the correlated predicate trees once up-front is fine.
            for pred in &correlated_preds {
                let refs = correlated_predicate_outer_refs(pred);
                for r in refs {
                    let col_idx = outer_columns
                        .iter()
                        .position(|c| c.as_str() == r.column.as_str());
                    if let Some(idx) = col_idx {
                        if let Some(v) = row.get(idx) {
                            bindings.insert(r.as_column_ref(), v.clone());
                        }
                    }
                }
            }

            let mut all_pass = true;
            for pred in &correlated_preds {
                if !self.evaluate_correlated_predicate(store, pred, &bindings, params)? {
                    all_pass = false;
                    break;
                }
            }
            if all_pass {
                kept.push(row);
            }
        }

        // Apply ORDER BY, LIMIT, OFFSET on the full rows before
        // projecting to the user's requested column list.
        // (Simple implementation: leverage the fact that we kept full
        // rows. We construct a second plan that projects + orders +
        // limits using a temporary store isn't worth it; do it inline.)
        Self::post_process_correlated_result(parsed, params, outer_columns, kept)
    }

    /// Apply the outer query's projection / ORDER BY / LIMIT / OFFSET
    /// to the rows surviving the correlated-predicate filter.
    fn post_process_correlated_result(
        parsed: &parser::ParsedSelect,
        params: &[Value],
        outer_columns: Vec<schema::ColumnName>,
        mut rows: Vec<Vec<Value>>,
    ) -> Result<QueryResult> {
        // ORDER BY — bare-column only, resolved against outer_columns.
        if !parsed.order_by.is_empty() {
            let indices: Vec<(usize, bool)> = parsed
                .order_by
                .iter()
                .map(|ob| {
                    let idx = outer_columns
                        .iter()
                        .position(|c| c == &ob.column)
                        .ok_or_else(|| QueryError::ColumnNotFound {
                            table: parsed.table.clone(),
                            column: ob.column.to_string(),
                        })?;
                    Ok::<_, QueryError>((idx, ob.ascending))
                })
                .collect::<Result<Vec<_>>>()?;
            rows.sort_by(|a, b| {
                for (idx, asc) in &indices {
                    let ord = a
                        .get(*idx)
                        .and_then(|av| b.get(*idx).and_then(|bv| av.compare(bv)))
                        .unwrap_or(std::cmp::Ordering::Equal);
                    let ord = if *asc { ord } else { ord.reverse() };
                    if ord != std::cmp::Ordering::Equal {
                        return ord;
                    }
                }
                std::cmp::Ordering::Equal
            });
        }

        // OFFSET / LIMIT.
        let offset = match parsed.offset {
            Some(parser::LimitExpr::Literal(n)) => n,
            Some(parser::LimitExpr::Param(idx)) => params
                .get(idx.saturating_sub(1))
                .and_then(|v| match v {
                    Value::BigInt(n) if *n >= 0 => Some(*n as usize),
                    Value::Integer(n) if *n >= 0 => Some(*n as usize),
                    _ => None,
                })
                .unwrap_or(0),
            None => 0,
        };
        let limit = match parsed.limit {
            Some(parser::LimitExpr::Literal(n)) => Some(n),
            Some(parser::LimitExpr::Param(idx)) => params
                .get(idx.saturating_sub(1))
                .and_then(|v| match v {
                    Value::BigInt(n) if *n >= 0 => Some(*n as usize),
                    Value::Integer(n) if *n >= 0 => Some(*n as usize),
                    _ => None,
                }),
            None => None,
        };
        if offset > 0 {
            rows.drain(0..offset.min(rows.len()));
        }
        if let Some(l) = limit {
            rows.truncate(l);
        }

        // Project to the requested column list.
        let (out_columns, projected_rows) = match (&parsed.columns, &parsed.column_aliases) {
            (None, _) => (outer_columns.clone(), rows),
            (Some(cols), aliases) => {
                let mut indices = Vec::with_capacity(cols.len());
                let mut out_names: Vec<schema::ColumnName> = Vec::with_capacity(cols.len());
                for (i, col) in cols.iter().enumerate() {
                    let idx = outer_columns
                        .iter()
                        .position(|c| c == col)
                        .ok_or_else(|| QueryError::ColumnNotFound {
                            table: parsed.table.clone(),
                            column: col.to_string(),
                        })?;
                    indices.push(idx);
                    let alias =
                        aliases.as_ref().and_then(|a| a.get(i)).and_then(|a| a.as_ref());
                    out_names.push(match alias {
                        Some(a) => schema::ColumnName::new(a.clone()),
                        None => col.clone(),
                    });
                }
                let projected: Vec<Vec<Value>> = rows
                    .into_iter()
                    .map(|r| indices.iter().map(|i| r[*i].clone()).collect())
                    .collect();
                (out_names, projected)
            }
        };

        Ok(QueryResult {
            columns: out_columns,
            rows: projected_rows,
        })
    }

    /// Evaluate a correlated `InSubquery` / `Exists` against one
    /// outer row (already baked into `bindings`). Returns true iff
    /// the predicate holds.
    fn evaluate_correlated_predicate<S: ProjectionStore>(
        &self,
        store: &mut S,
        pred: &parser::Predicate,
        bindings: &std::collections::HashMap<String, Value>,
        params: &[Value],
    ) -> Result<bool> {
        match pred {
            parser::Predicate::Exists { subquery, negated } => {
                let substituted = correlated::substitute_outer_refs(subquery, bindings);
                // The inner subquery may itself have nested subqueries;
                // run it through the full query engine path so nested
                // correlations (if any) are handled.
                let inner_result =
                    self.execute_inner_subquery(store, &substituted, params)?;
                let exists = !inner_result.rows.is_empty();
                Ok(if *negated { !exists } else { exists })
            }
            parser::Predicate::InSubquery {
                column,
                subquery,
                negated,
            } => {
                let substituted = correlated::substitute_outer_refs(subquery, bindings);
                let inner_result =
                    self.execute_inner_subquery(store, &substituted, params)?;
                if inner_result.columns.len() != 1 {
                    return Err(QueryError::UnsupportedFeature(format!(
                        "IN (SELECT ...) subquery must project exactly 1 column, got {}",
                        inner_result.columns.len()
                    )));
                }
                let outer_val = bindings
                    .get(column.as_str())
                    .or_else(|| bindings.values().next()) // defensive
                    .cloned();
                let Some(outer_val) = outer_val else {
                    return Ok(false);
                };
                let any_match = inner_result
                    .rows
                    .iter()
                    .any(|row| row.first().is_some_and(|v| v == &outer_val));
                Ok(if *negated { !any_match } else { any_match })
            }
            _ => Err(QueryError::UnsupportedFeature(
                "evaluate_correlated_predicate called on non-subquery predicate".to_string(),
            )),
        }
    }

    /// Execute an inner subquery (with all outer refs already
    /// substituted). Delegates to `plan_query` + `executor::execute`.
    fn execute_inner_subquery<S: ProjectionStore>(
        &self,
        store: &mut S,
        inner: &parser::ParsedSelect,
        params: &[Value],
    ) -> Result<QueryResult> {
        // An inner subquery post-substitution might itself contain
        // nested correlations or uncorrelated subqueries. Run the
        // pre-execute pass once more to handle those cases.
        let mut inner_clone = inner.clone();
        self.pre_execute_subqueries(store, &mut inner_clone, params)?;
        if has_correlated_predicate(&inner_clone.predicates) {
            // v0.6.0 caps nesting at one correlated level — the
            // outer loop is already one nesting.
            return Err(QueryError::UnsupportedFeature(
                "nested correlated subqueries (depth > 1) are not supported in v0.6.0"
                    .to_string(),
            ));
        }
        let plan = planner::plan_query(&self.schema, &inner_clone, params)?;
        let table_def = self
            .schema
            .get_table(&plan.table_name().into())
            .ok_or_else(|| QueryError::TableNotFound(plan.table_name().to_string()))?;
        executor::execute(store, &plan, table_def)
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

    /// Executes a query against a historical snapshot selected by
    /// wall-clock timestamp, with a resolver that can distinguish
    /// the "timestamp predates retention horizon" case from a plain
    /// "no offset found".
    ///
    /// v0.6.0 Tier 2 #6: this is the runtime-layer variant of
    /// [`Self::query_at_timestamp`] used by `TenantHandle::query`
    /// when the resolver has a concrete notion of a retention
    /// horizon (e.g. an in-memory timestamp index maintained at
    /// append time). The existing `query_at_timestamp` stays as-is
    /// so callers with an `Option<Offset>` resolver (e.g. ad-hoc
    /// binary search over an external index) keep working.
    ///
    /// # Resolution semantics
    ///
    /// - `TimestampResolution::Offset(o)` → execute at `o`.
    /// - `TimestampResolution::BeforeRetentionHorizon { horizon_ns }` →
    ///   [`QueryError::AsOfBeforeRetentionHorizon`] with both the
    ///   requested and horizon timestamps.
    /// - `TimestampResolution::LogEmpty` →
    ///   [`QueryError::UnsupportedFeature`] (same message the
    ///   ergonomic form emits for an empty log).
    pub fn query_at_timestamp_resolved<S, R>(
        &self,
        store: &mut S,
        sql: &str,
        params: &[Value],
        target_ns: i64,
        resolver: R,
    ) -> Result<QueryResult>
    where
        S: ProjectionStore,
        R: FnOnce(i64) -> TimestampResolution,
    {
        match resolver(target_ns) {
            TimestampResolution::Offset(offset) => self.query_at(store, sql, params, offset),
            TimestampResolution::BeforeRetentionHorizon { horizon_ns } => {
                Err(QueryError::AsOfBeforeRetentionHorizon {
                    requested_ns: target_ns,
                    horizon_ns,
                })
            }
            TimestampResolution::LogEmpty => Err(QueryError::UnsupportedFeature(format!(
                "no log offset at or before timestamp {target_ns} ns \
                 (empty log or predates genesis)"
            ))),
        }
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

/// True iff any top-level predicate is a surviving correlated
/// subquery (`InSubquery` / `Exists`). Uncorrelated subqueries were
/// rewritten by `pre_execute_subqueries` before this is called.
fn has_correlated_predicate(predicates: &[parser::Predicate]) -> bool {
    predicates.iter().any(|p| {
        matches!(
            p,
            parser::Predicate::InSubquery { .. } | parser::Predicate::Exists { .. }
        )
    })
}

/// Extract outer references from a surviving correlated predicate.
/// Used by the correlated-loop executor to populate the
/// `alias.column → Value` binding map for each outer row.
fn correlated_predicate_outer_refs(pred: &parser::Predicate) -> Vec<correlated::OuterRef> {
    let subquery = match pred {
        parser::Predicate::InSubquery { subquery, .. }
        | parser::Predicate::Exists { subquery, .. } => subquery,
        _ => return Vec::new(),
    };
    // Walk inner predicates gathering any qualified ColumnRef —
    // post-pre-execute, anything that remains as a ColumnRef with a
    // qualifier is an outer reference (the inner FROM has its own
    // bare-column refs).
    let mut out = Vec::new();
    for pred in &subquery.predicates {
        collect_refs_in_pred(pred, &mut out);
    }
    out
}

fn collect_refs_in_pred(pred: &parser::Predicate, out: &mut Vec<correlated::OuterRef>) {
    let push_if_colref = |pv: &parser::PredicateValue, out: &mut Vec<correlated::OuterRef>| {
        if let parser::PredicateValue::ColumnRef(raw) = pv {
            if let Some((q, c)) = raw.split_once('.') {
                out.push(correlated::OuterRef {
                    qualifier: q.to_string(),
                    column: schema::ColumnName::new(c.to_string()),
                    scope_depth: 1,
                });
            }
        }
    };
    match pred {
        parser::Predicate::Eq(_, v)
        | parser::Predicate::Lt(_, v)
        | parser::Predicate::Le(_, v)
        | parser::Predicate::Gt(_, v)
        | parser::Predicate::Ge(_, v) => push_if_colref(v, out),
        parser::Predicate::In(_, vs) | parser::Predicate::NotIn(_, vs) => {
            for v in vs {
                push_if_colref(v, out);
            }
        }
        parser::Predicate::NotBetween(_, lo, hi) => {
            push_if_colref(lo, out);
            push_if_colref(hi, out);
        }
        parser::Predicate::Or(l, r) => {
            for p in l {
                collect_refs_in_pred(p, out);
            }
            for p in r {
                collect_refs_in_pred(p, out);
            }
        }
        _ => {}
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
