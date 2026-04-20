//! # kmb-query: SQL query layer for `Kimberlite` projections
//!
//! This crate provides a minimal SQL query engine for compliance lookups
//! against the projection store.
//!
//! ## SQL Subset
//!
//! Supported SQL features:
//! - `SELECT` with column list or `*`
//! - `FROM` single table or `JOIN` (INNER, LEFT)
//! - `WHERE` with comparison predicates (`=`, `<`, `>`, `<=`, `>=`, `IN`)
//! - `ORDER BY` (ascending/descending)
//! - `LIMIT`
//! - `GROUP BY` with aggregates (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`)
//! - `HAVING` with aggregate filtering
//! - `UNION` / `UNION ALL`
//! - `DISTINCT`
//! - `ALTER TABLE` (ADD COLUMN, DROP COLUMN)
//! - Parameterized queries (`$1`, `$2`, ...)
//!
//! - `WITH` (Common Table Expressions / CTEs)
//! - Subqueries in FROM and JOIN (`SELECT * FROM (SELECT ...) AS t`)
//!
//! Not yet supported:
//! - `WITH RECURSIVE`
//! - Window functions
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

pub mod dml_planner;
mod error;
mod executor;
pub mod explain;
pub mod information_schema;
pub mod key_encoder;
mod parse_cache;
mod parser;
mod plan;
mod planner;
pub mod rbac_filter;
mod schema;
mod value;

#[cfg(test)]
mod tests;

// Re-export public types
pub use error::{QueryError, Result};
pub use executor::{QueryResult, Row, execute};
pub use parser::{
    HavingCondition, HavingOp, ParsedAlterTable, ParsedColumn, ParsedCreateIndex,
    ParsedCreateMask, ParsedCreateTable, ParsedCreateUser, ParsedCte, ParsedDelete, ParsedGrant,
    ParsedInsert, ParsedSelect, ParsedSetClassification, ParsedStatement, ParsedUnion,
    ParsedUpdate, Predicate, PredicateValue, TimeTravel, extract_at_offset,
    extract_time_travel, parse_statement, try_parse_custom_statement,
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
        self.parse_cache.as_deref().map(parse_cache::ParseCache::stats)
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
    fn parse_query_statement_cached(
        &self,
        sql: &str,
    ) -> Result<parser::ParsedStatement> {
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
        let (after_break_glass, break_glass_reason) =
            explain::extract_break_glass(sql);
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
        let sql = sql;  // unmodified; suppress unused-shadow lint

        let stmt = self.parse_query_statement_cached(sql)?;

        match stmt {
            parser::ParsedStatement::Select(parsed) => {
                if parsed.ctes.is_empty() {
                    let plan = planner::plan_query(&self.schema, &parsed, params)?;
                    let table_def = self
                        .schema
                        .get_table(&plan.table_name().into())
                        .ok_or_else(|| QueryError::TableNotFound(plan.table_name().to_string()))?;
                    executor::execute(store, &plan, table_def)
                } else {
                    self.execute_with_ctes(store, &parsed, params)
                }
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
            // Execute the CTE's inner query
            let cte_plan = planner::plan_query(&extended_schema, &cte.query, params)?;
            let cte_table_def = extended_schema
                .get_table(&cte_plan.table_name().into())
                .ok_or_else(|| QueryError::TableNotFound(cte_plan.table_name().to_string()))?;
            let cte_result = executor::execute(store, &cte_plan, cte_table_def)?;

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

            // Write CTE rows into the store as a temporary table
            for (row_idx, row) in cte_result.rows.iter().enumerate() {
                let mut row_map = serde_json::Map::new();
                for (col, val) in cte_result.columns.iter().zip(row.iter()) {
                    row_map.insert(col.as_str().to_string(), value_to_json(val));
                }

                let json_val =
                    serde_json::to_vec(&serde_json::Value::Object(row_map)).map_err(|e| {
                        QueryError::UnsupportedFeature(format!("CTE serialization failed: {e}"))
                    })?;

                let pk_key = crate::key_encoder::encode_key(&[Value::BigInt(row_idx as i64)]);
                let batch = kimberlite_store::WriteBatch::new(kimberlite_types::Offset::new(
                    store.applied_position().as_u64() + 1,
                ))
                .put(cte_table_id, pk_key, bytes::Bytes::from(json_val));
                store.apply(batch)?;
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

    /// Executes a UNION / UNION ALL query.
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

        // Combine rows
        let mut all_rows = left_result.rows;
        all_rows.extend(right_result.rows);

        // UNION (not ALL) removes duplicates
        if !union_stmt.all {
            let mut seen = std::collections::HashSet::new();
            all_rows.retain(|row| {
                // Use debug format as hash key (Value doesn't impl Hash)
                let key = format!("{row:?}");
                seen.insert(key)
            });
        }

        Ok(QueryResult {
            columns: column_names,
            rows: all_rows,
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
