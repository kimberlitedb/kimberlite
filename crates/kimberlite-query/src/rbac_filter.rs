//! RBAC query filtering and rewriting.
//!
//! This module provides query rewriting to enforce RBAC policies:
//! - **Column filtering**: Remove unauthorized columns from SELECT
//! - **Row-level security**: Inject WHERE clauses
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │  Original Query                      │
//! │  SELECT name, ssn FROM patients      │
//! └───────────────┬─────────────────────┘
//!                 │
//!                 ▼
//! ┌─────────────────────────────────────┐
//! │  RBAC Filter                         │
//! │  - Check stream access               │
//! │  - Filter columns (remove "ssn")     │
//! │  - Inject WHERE clause               │
//! └───────────────┬─────────────────────┘
//!                 │
//!                 ▼
//! ┌─────────────────────────────────────┐
//! │  Rewritten Query                     │
//! │  SELECT name FROM patients           │
//! │  WHERE tenant_id = 42                │
//! └─────────────────────────────────────┘
//! ```

use crate::error::QueryError;
use kimberlite_rbac::{AccessPolicy, enforcement::PolicyEnforcer};
use sqlparser::ast::{Expr, Query, Select, SelectItem, SetExpr, Statement, TableFactor};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Error type for RBAC filtering.
#[derive(Debug, Error)]
pub enum RbacError {
    /// Access denied by policy.
    #[error("Access denied: {0}")]
    AccessDenied(String),

    /// No authorized columns in query.
    #[error("No authorized columns in query")]
    NoAuthorizedColumns,

    /// Unsupported query type for RBAC.
    #[error("Unsupported query type: {0}")]
    UnsupportedQuery(String),

    /// Policy enforcement failed.
    #[error("Policy enforcement failed: {0}")]
    EnforcementFailed(String),
}

impl From<kimberlite_rbac::enforcement::EnforcementError> for RbacError {
    fn from(err: kimberlite_rbac::enforcement::EnforcementError) -> Self {
        match err {
            kimberlite_rbac::enforcement::EnforcementError::AccessDenied { reason } => {
                RbacError::AccessDenied(reason)
            }
            _ => RbacError::EnforcementFailed(err.to_string()),
        }
    }
}

impl From<RbacError> for QueryError {
    fn from(err: RbacError) -> Self {
        QueryError::UnsupportedFeature(err.to_string())
    }
}

/// Result type for RBAC operations.
pub type Result<T> = std::result::Result<T, RbacError>;

/// Output of [`RbacFilter::rewrite_statement`].
///
/// Carries the rewritten statement alongside the alias mapping derived
/// from the original projection. Downstream code (e.g. the masking pass
/// in `kimberlite`) uses the mapping to resolve output column names
/// back to their source columns so masks are applied to the underlying
/// sensitive attribute, not the user-visible alias.
#[derive(Debug)]
pub struct RewriteOutput {
    /// The rewritten SQL statement.
    pub statement: Statement,
    /// Pairs of `(output_column_name, source_column_name)` for each
    /// projection item that survived RBAC filtering.
    ///
    /// Bare identifiers produce pairs where both entries are equal.
    /// Aliased identifiers (`SELECT ssn AS id`) produce distinct
    /// output/source entries — the masking pass must key its lookup
    /// on the source entry (AUDIT-2026-04 M-7).
    pub column_aliases: Vec<(String, String)>,
}

/// RBAC query filter.
///
/// Rewrites SQL queries to enforce access control policies.
pub struct RbacFilter {
    enforcer: PolicyEnforcer,
}

impl RbacFilter {
    /// Creates a new RBAC filter with the given policy.
    pub fn new(policy: AccessPolicy) -> Self {
        Self {
            enforcer: PolicyEnforcer::new(policy),
        }
    }

    /// Rewrites a SQL statement to enforce RBAC policy.
    ///
    /// **Transformations:**
    /// 1. Check stream access (deny if unauthorized)
    /// 2. Filter SELECT columns (remove unauthorized columns)
    /// 3. Inject WHERE clause for row-level security
    ///
    /// # Arguments
    ///
    /// * `stmt` - SQL statement to rewrite
    ///
    /// # Returns
    ///
    /// Rewritten statement plus a map of `(output_column_name,
    /// source_column_name)` pairs — one entry per projection item that
    /// survived RBAC filtering. The masking pass uses this map to look
    /// up column masks by source column rather than by the
    /// potentially-aliased output name (AUDIT-2026-04 M-7).
    ///
    /// # Errors
    ///
    /// - `AccessDenied` if stream access is denied
    /// - `NoAuthorizedColumns` if all columns are unauthorized
    /// - `UnsupportedQuery` if query type is not supported
    pub fn rewrite_statement(&self, mut stmt: Statement) -> Result<RewriteOutput> {
        match &mut stmt {
            Statement::Query(query) => {
                let column_aliases = self.rewrite_query(query)?;
                Ok(RewriteOutput {
                    statement: stmt,
                    column_aliases,
                })
            }
            _ => Err(RbacError::UnsupportedQuery(
                "Only SELECT queries are currently supported".to_string(),
            )),
        }
    }

    /// Rewrites a query to enforce RBAC.
    fn rewrite_query(&self, query: &mut Query) -> Result<Vec<(String, String)>> {
        match query.body.as_mut() {
            SetExpr::Select(select) => self.rewrite_select(select),
            _ => Err(RbacError::UnsupportedQuery(
                "Only simple SELECT queries are supported".to_string(),
            )),
        }
    }

    /// Rewrites a SELECT statement. Returns the `(output, source)`
    /// column pairs for the surviving projection items.
    fn rewrite_select(&self, select: &mut Select) -> Result<Vec<(String, String)>> {
        // 1. Extract stream name from FROM clause
        let stream_name = Self::extract_stream_name(select)?;

        debug!(stream = %stream_name, "Extracting columns from SELECT");

        // 2. Extract requested columns (source names) and aliases
        let aliases = Self::extract_column_aliases(select)?;
        let requested_columns: Vec<String> =
            aliases.iter().map(|(_, src)| src.clone()).collect();

        info!(
            stream = %stream_name,
            columns = ?requested_columns,
            "Enforcing RBAC policy"
        );

        // 3. Enforce policy (checks stream access + filters columns)
        let (allowed_columns, where_clause_sql) = self
            .enforcer
            .enforce_query(&stream_name, &requested_columns)?;

        if allowed_columns.is_empty() {
            warn!(stream = %stream_name, "No authorized columns");
            return Err(RbacError::NoAuthorizedColumns);
        }

        // 4. Rewrite SELECT projection (filter columns)
        Self::rewrite_projection(select, &allowed_columns);

        // 5. Inject WHERE clause for row-level security
        if !where_clause_sql.is_empty() {
            Self::inject_where_clause(select, &where_clause_sql)?;
        }

        info!(
            stream = %stream_name,
            allowed_columns = ?allowed_columns,
            where_clause = %where_clause_sql,
            "Query rewritten successfully"
        );

        // 6. Trim the alias map to the surviving projection.
        let allowed: std::collections::HashSet<&str> =
            allowed_columns.iter().map(String::as_str).collect();
        let surviving_aliases = aliases
            .into_iter()
            .filter(|(_, src)| allowed.contains(src.as_str()))
            .collect();

        Ok(surviving_aliases)
    }

    /// Extracts the stream name from the FROM clause.
    fn extract_stream_name(select: &Select) -> Result<String> {
        if select.from.is_empty() {
            return Err(RbacError::UnsupportedQuery(
                "SELECT without FROM clause".to_string(),
            ));
        }

        let table = &select.from[0];
        match &table.relation {
            TableFactor::Table { name, .. } => {
                let stream_name = name
                    .0
                    .iter()
                    .map(|i| i.value.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                Ok(stream_name)
            }
            _ => Err(RbacError::UnsupportedQuery(
                "Only simple table references are supported".to_string(),
            )),
        }
    }

    /// Extracts `(output_column_name, source_column_name)` pairs for
    /// each item in the SELECT projection. See [`column_aliases`] for
    /// the free-function entry point used by the SQL-level mask pass.
    fn extract_column_aliases(select: &Select) -> Result<Vec<(String, String)>> {
        column_aliases_from_select(select)
    }
}

/// Extracts `(output_column_name, source_column_name)` pairs for each
/// item in the SELECT projection of `stmt`.
///
/// Returns an empty vector for non-`SELECT` statements or for set-expr
/// bodies that are not a plain `SELECT` (e.g. `UNION`) — the masking
/// pass treats an empty map as "no aliases known" and falls back to
/// output-name keying, matching pre-M-7 semantics for those shapes.
///
/// Semantics:
/// - `SELECT col` → `("col", "col")`
/// - `SELECT col AS alias` → `("alias", "col")`
/// - `SELECT UPPER(col) AS alias` → `("alias", "alias")` (non-identifier
///   expressions cannot be resolved to a source column — mask lookup
///   keys on the alias, mirroring the pre-M-7 behaviour).
///
/// AUDIT-2026-04 M-7: the masking pass uses the source half of the
/// pair to look up column masks. Without this, `SELECT ssn AS id FROM
/// patients` passed RBAC (source `ssn` is permitted) but
/// `mask_for_column("id")` returned `None`, leaking the masked
/// attribute under a rename.
pub fn column_aliases(stmt: &Statement) -> Vec<(String, String)> {
    let Statement::Query(query) = stmt else {
        return Vec::new();
    };
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Vec::new();
    };
    column_aliases_from_select(select).unwrap_or_default()
}

fn column_aliases_from_select(select: &Select) -> Result<Vec<(String, String)>> {
    let mut pairs = Vec::new();

    for item in &select.projection {
        match item {
            SelectItem::UnnamedExpr(Expr::Identifier(ident)) => {
                pairs.push((ident.value.clone(), ident.value.clone()));
            }
            SelectItem::ExprWithAlias { expr, alias } => {
                if let Expr::Identifier(ident) = expr {
                    pairs.push((alias.value.clone(), ident.value.clone()));
                } else {
                    pairs.push((alias.value.clone(), alias.value.clone()));
                }
            }
            SelectItem::Wildcard(_) => {
                return Err(RbacError::UnsupportedQuery(
                    "SELECT * is not yet supported with RBAC".to_string(),
                ));
            }
            _ => {
                return Err(RbacError::UnsupportedQuery(format!(
                    "Unsupported SELECT item: {item:?}"
                )));
            }
        }
    }

    Ok(pairs)
}

impl RbacFilter {

    /// Rewrites the SELECT projection to include only allowed columns.
    fn rewrite_projection(select: &mut Select, allowed_columns: &[String]) {
        let allowed_set: std::collections::HashSet<_> = allowed_columns.iter().collect();

        select.projection.retain(|item| match item {
            SelectItem::UnnamedExpr(Expr::Identifier(ident))
            | SelectItem::ExprWithAlias {
                expr: Expr::Identifier(ident),
                ..
            } => allowed_set.contains(&ident.value),
            _ => false,
        });
    }

    /// Injects a WHERE clause for row-level security.
    fn inject_where_clause(select: &mut Select, where_clause_sql: &str) -> Result<()> {
        // Parse the WHERE clause SQL into an Expr
        let where_expr = Self::parse_where_clause(where_clause_sql)?;

        // Combine with existing WHERE clause (if any)
        select.selection = match select.selection.take() {
            Some(existing) => Some(Expr::BinaryOp {
                left: Box::new(existing),
                op: sqlparser::ast::BinaryOperator::And,
                right: Box::new(where_expr),
            }),
            None => Some(where_expr),
        };

        Ok(())
    }

    /// Parses a WHERE clause SQL string into an Expr.
    ///
    /// # Security boundary
    ///
    /// This function is **only ever called with trusted `RowFilter` values** generated
    /// internally by the RBAC policy engine (see [`PolicyEnforcer::row_filter`]).
    /// It is **not** called with user-supplied SQL strings and is therefore not a
    /// SQL-injection vector.  If you ever call this with data derived from user input,
    /// you MUST validate/sanitize the input first.
    ///
    /// The parser handles `column = value` predicates joined by `AND`.  It produces
    /// AST nodes directly (not concatenated SQL), so the result is safe to pass to
    /// the query planner without further escaping.
    ///
    /// More complex predicates may require the full SQL parser.
    fn parse_where_clause(where_clause_sql: &str) -> Result<Expr> {
        // Simple parser for "column = value" and "column1 = value1 AND column2 = value2".
        // SAFETY: Only called with trusted, internally-generated RowFilter strings.
        let parts: Vec<&str> = where_clause_sql.split(" AND ").collect();

        let mut exprs = Vec::new();

        for part in parts {
            // Parse "column = value"
            let tokens: Vec<&str> = part.trim().split('=').collect();
            if tokens.len() != 2 {
                return Err(RbacError::UnsupportedQuery(format!(
                    "Invalid WHERE clause: {part}"
                )));
            }

            let column = tokens[0].trim();
            let value = tokens[1].trim();

            let expr = Expr::BinaryOp {
                left: Box::new(Expr::Identifier(sqlparser::ast::Ident::new(column))),
                op: sqlparser::ast::BinaryOperator::Eq,
                right: Box::new(Expr::Value(sqlparser::ast::Value::Number(
                    value.to_string(),
                    false,
                ))),
            };

            exprs.push(expr);
        }

        // Combine with AND
        let mut iter = exprs.into_iter();
        let mut result = iter
            .next()
            .ok_or_else(|| RbacError::UnsupportedQuery("Empty WHERE clause".to_string()))?;

        for expr in iter {
            result = Expr::BinaryOp {
                left: Box::new(result),
                op: sqlparser::ast::BinaryOperator::And,
                right: Box::new(expr),
            };
        }

        Ok(result)
    }

    /// Returns the underlying policy enforcer.
    pub fn enforcer(&self) -> &PolicyEnforcer {
        &self.enforcer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_rbac::policy::StandardPolicies;
    use kimberlite_types::TenantId;
    use sqlparser::dialect::GenericDialect;
    use sqlparser::parser::Parser;

    fn parse_sql(sql: &str) -> Statement {
        let dialect = GenericDialect {};
        let statements = Parser::parse_sql(&dialect, sql).expect("Failed to parse SQL");
        statements.into_iter().next().expect("No statement parsed")
    }

    #[test]
    fn test_rewrite_admin_policy() {
        let policy = StandardPolicies::admin();
        let filter = RbacFilter::new(policy);

        let sql = "SELECT name, email FROM users";
        let stmt = parse_sql(sql);

        let result = filter.rewrite_statement(stmt);
        assert!(result.is_ok());
    }

    #[test]
    fn test_rewrite_user_policy_column_filter() {
        let policy = kimberlite_rbac::policy::AccessPolicy::new(kimberlite_rbac::roles::Role::User)
            .allow_stream("users")
            .allow_column("name")
            .deny_column("ssn");

        let filter = RbacFilter::new(policy);

        let sql = "SELECT name, ssn FROM users";
        let stmt = parse_sql(sql);

        let result = filter.rewrite_statement(stmt);
        assert!(result.is_ok());

        // Check that ssn was filtered out
        if let Statement::Query(query) = result.unwrap().statement {
            if let SetExpr::Select(select) = query.body.as_ref() {
                assert_eq!(select.projection.len(), 1);
                // Should only have "name" column
            }
        }
    }

    #[test]
    fn test_rewrite_with_row_filter() {
        let tenant_id = TenantId::new(42);
        let policy = StandardPolicies::user(tenant_id);
        let filter = RbacFilter::new(policy);

        let sql = "SELECT name, email FROM users";
        let stmt = parse_sql(sql);

        let result = filter.rewrite_statement(stmt);
        assert!(result.is_ok());

        // Check that WHERE clause was injected
        if let Statement::Query(query) = result.unwrap().statement {
            if let SetExpr::Select(select) = query.body.as_ref() {
                assert!(select.selection.is_some());
                // Should have WHERE tenant_id = 42
            }
        }
    }

    #[test]
    fn test_rewrite_access_denied() {
        let policy = StandardPolicies::auditor();
        let filter = RbacFilter::new(policy);

        let sql = "SELECT name FROM users"; // Auditor cannot access users table
        let stmt = parse_sql(sql);

        let result = filter.rewrite_statement(stmt);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RbacError::AccessDenied(_)));
    }

    #[test]
    fn test_rewrite_no_authorized_columns() {
        let policy = kimberlite_rbac::policy::AccessPolicy::new(kimberlite_rbac::roles::Role::User)
            .allow_stream("users")
            .deny_column("*"); // Deny all columns

        let filter = RbacFilter::new(policy);

        let sql = "SELECT name, email FROM users";
        let stmt = parse_sql(sql);

        let result = filter.rewrite_statement(stmt);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, RbacError::AccessDenied(ref msg) if msg.contains("No authorized columns"))
        );
    }
}
