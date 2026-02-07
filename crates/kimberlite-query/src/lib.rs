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
//! Not yet supported:
//! - Subqueries
//! - Common Table Expressions (`WITH`)
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

mod error;
mod executor;
pub mod key_encoder;
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
    ParsedCreateTable, ParsedDelete, ParsedInsert, ParsedSelect, ParsedStatement, ParsedUnion,
    ParsedUpdate, Predicate, PredicateValue, parse_statement,
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
/// The engine is stateless and can be shared across threads.
/// It holds only the schema definition.
#[derive(Debug, Clone)]
pub struct QueryEngine {
    schema: Schema,
}

impl QueryEngine {
    /// Creates a new query engine with the given schema.
    pub fn new(schema: Schema) -> Self {
        Self { schema }
    }

    /// Returns a reference to the schema.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Parses a SQL string and extracts the SELECT or UNION statement.
    fn parse_query_statement(sql: &str) -> Result<parser::ParsedStatement> {
        let stmt = parser::parse_statement(sql)?;
        match &stmt {
            parser::ParsedStatement::Select(_) | parser::ParsedStatement::Union(_) => Ok(stmt),
            _ => Err(QueryError::UnsupportedFeature(
                "only SELECT and UNION queries are supported".to_string(),
            )),
        }
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
        let stmt = Self::parse_query_statement(sql)?;

        match stmt {
            parser::ParsedStatement::Select(parsed) => {
                let plan = planner::plan_query(&self.schema, &parsed, params)?;
                let table_def = self
                    .schema
                    .get_table(&plan.table_name().into())
                    .ok_or_else(|| QueryError::TableNotFound(plan.table_name().to_string()))?;
                executor::execute(store, &plan, table_def)
            }
            parser::ParsedStatement::Union(union_stmt) => {
                self.execute_union(store, &union_stmt, params)
            }
            _ => unreachable!("parse_query_statement only returns Select or Union"),
        }
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
        let stmt = Self::parse_query_statement(sql)?;

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

    /// Parses a SQL query without executing it.
    ///
    /// Useful for validation or query plan inspection.
    pub fn prepare(&self, sql: &str, params: &[Value]) -> Result<PreparedQuery> {
        let stmt = Self::parse_query_statement(sql)?;
        let parsed = match stmt {
            parser::ParsedStatement::Select(s) => s,
            _ => {
                return Err(QueryError::UnsupportedFeature(
                    "only SELECT queries can be prepared".to_string(),
                ))
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
