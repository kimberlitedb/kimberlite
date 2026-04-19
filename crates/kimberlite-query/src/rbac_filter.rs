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
    ///
    /// **AUDIT-2026-04 M-7 — recursive traversal.** Prior to this
    /// change, only the top-level `SetExpr::Select` was rewritten,
    /// so a predicate like
    /// `SELECT id FROM t WHERE x IN (SELECT ssn FROM patients)`
    /// would bypass column filtering on `ssn`. The recursive walk
    /// below ensures every nested `Query` (CTE / UNION / subquery
    /// in FROM / subquery in WHERE) is rewritten under the same
    /// policy before the outer select is processed.
    fn rewrite_query(&self, query: &mut Query) -> Result<Vec<(String, String)>> {
        // 1. Rewrite CTEs first — later referenced by name in the
        //    main set-expression, so their filtering must land
        //    before the outer select reads them.
        if let Some(with) = query.with.as_mut() {
            for cte in with.cte_tables.iter_mut() {
                // CTEs themselves cannot leak if the outer select
                // never references the denied columns — but we
                // rewrite defensively so that any CTE reference
                // through `SELECT * FROM cte_name` (once wildcard
                // support lands) does not expose masked sources.
                let _ = self.rewrite_query(cte.query.as_mut())?;
            }
        }

        // 2. Dispatch on set-expression shape.
        self.rewrite_set_expr(query.body.as_mut())
    }

    /// Recursively rewrites a `SetExpr`, returning the column
    /// lineage for the *representative* select (the left-most
    /// branch of a UNION, or the inner select of a parenthesised
    /// query).
    ///
    /// UNION branches must all satisfy the policy independently —
    /// if any branch references a denied column, the whole query
    /// is rejected.
    fn rewrite_set_expr(&self, set_expr: &mut SetExpr) -> Result<Vec<(String, String)>> {
        match set_expr {
            SetExpr::Select(select) => self.rewrite_select(select),
            // Parenthesised query — recurse.
            SetExpr::Query(inner) => self.rewrite_query(inner.as_mut()),
            // UNION / INTERSECT / EXCEPT — every branch must pass
            // RBAC independently. The outer lineage comes from the
            // left branch (all branches must have compatible
            // column counts, so either branch's lineage is a valid
            // descriptor; we use left for determinism).
            SetExpr::SetOperation { left, right, .. } => {
                let left_lineage = self.rewrite_set_expr(left.as_mut())?;
                let _right_lineage = self.rewrite_set_expr(right.as_mut())?;
                Ok(left_lineage)
            }
            _ => Err(RbacError::UnsupportedQuery(format!(
                "unsupported set-expression: {set_expr:?}"
            ))),
        }
    }

    /// Rewrites a SELECT statement. Returns the `(output, source)`
    /// column pairs for the surviving projection items.
    fn rewrite_select(&self, select: &mut Select) -> Result<Vec<(String, String)>> {
        // AUDIT-2026-04 M-7 — subquery / nested-SELECT recursion.
        //
        // Step 0a: rewrite any `TableFactor::Derived { subquery }`
        // in the FROM clause. A predicate that reads
        // `SELECT outer.x FROM (SELECT ssn AS x FROM patients) outer`
        // was previously accepted because `extract_stream_name` only
        // saw the outer derived-table reference — the inner SELECT
        // was never filtered against the `patients.ssn` deny policy.
        // Now the inner SELECT is rewritten first; if it references
        // a denied column it errors out here, before any outer
        // lineage is reported.
        for table_with_joins in select.from.iter_mut() {
            self.rewrite_table_factor(&mut table_with_joins.relation)?;
            for join in table_with_joins.joins.iter_mut() {
                self.rewrite_table_factor(&mut join.relation)?;
            }
        }

        // Step 0b: rewrite subqueries inside the WHERE clause.
        // Handles `IN (SELECT ...)`, `EXISTS (SELECT ...)`, and
        // scalar-subquery forms. The traversal is read-mutable
        // because the inner rewrite replaces column projections.
        if let Some(ref mut selection) = select.selection {
            self.rewrite_expr_subqueries(selection)?;
        }

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

    /// AUDIT-2026-04 M-7 helper — recurse into nested queries
    /// carried by a `TableFactor`.
    ///
    /// `TableFactor::Derived { subquery }` is the AST node for
    /// `FROM (SELECT ...)`. `TableFactor::NestedJoin` wraps a
    /// `TableWithJoins` that may itself contain derived tables.
    /// Anything else is a terminal table reference handled by
    /// `extract_stream_name` downstream.
    fn rewrite_table_factor(&self, factor: &mut TableFactor) -> Result<()> {
        match factor {
            TableFactor::Derived { subquery, .. } => {
                self.rewrite_query(subquery.as_mut())?;
                Ok(())
            }
            TableFactor::NestedJoin {
                table_with_joins, ..
            } => {
                self.rewrite_table_factor(&mut table_with_joins.relation)?;
                for join in table_with_joins.joins.iter_mut() {
                    self.rewrite_table_factor(&mut join.relation)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// AUDIT-2026-04 M-7 helper — recurse into subqueries embedded
    /// in a WHERE-clause `Expr`.
    ///
    /// Walks `Expr::Subquery`, `Expr::InSubquery`, `Expr::Exists`,
    /// and combinators (`BinaryOp`, `UnaryOp`, `Nested`) that can
    /// transport a subquery in their children. Non-subquery leaves
    /// (identifiers, literals) are terminal.
    ///
    /// A bounded-depth guard would belong here if the recursive
    /// kernel principle forbade it; the query parser already
    /// rejects SQL with unbounded expression depth before reaching
    /// this point, so we rely on the sqlparser limit.
    fn rewrite_expr_subqueries(&self, expr: &mut Expr) -> Result<()> {
        match expr {
            Expr::Subquery(q) | Expr::Exists { subquery: q, .. } => {
                self.rewrite_query(q.as_mut())?;
                Ok(())
            }
            Expr::InSubquery { subquery, expr: inner, .. } => {
                self.rewrite_expr_subqueries(inner.as_mut())?;
                self.rewrite_query(subquery.as_mut())?;
                Ok(())
            }
            Expr::BinaryOp { left, right, .. } => {
                self.rewrite_expr_subqueries(left.as_mut())?;
                self.rewrite_expr_subqueries(right.as_mut())
            }
            Expr::UnaryOp { expr: inner, .. } => self.rewrite_expr_subqueries(inner.as_mut()),
            Expr::Nested(inner) => self.rewrite_expr_subqueries(inner.as_mut()),
            Expr::InList { expr: inner, list, .. } => {
                self.rewrite_expr_subqueries(inner.as_mut())?;
                for item in list.iter_mut() {
                    self.rewrite_expr_subqueries(item)?;
                }
                Ok(())
            }
            Expr::Between {
                expr: inner,
                low,
                high,
                ..
            } => {
                self.rewrite_expr_subqueries(inner.as_mut())?;
                self.rewrite_expr_subqueries(low.as_mut())?;
                self.rewrite_expr_subqueries(high.as_mut())
            }
            Expr::Case {
                conditions,
                results,
                else_result,
                ..
            } => {
                for c in conditions.iter_mut() {
                    self.rewrite_expr_subqueries(c)?;
                }
                for r in results.iter_mut() {
                    self.rewrite_expr_subqueries(r)?;
                }
                if let Some(else_r) = else_result.as_mut() {
                    self.rewrite_expr_subqueries(else_r.as_mut())?;
                }
                Ok(())
            }
            // Identifiers, literals, function calls without subquery
            // arguments, etc. — nothing to rewrite.
            _ => Ok(()),
        }
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

    // -----------------------------------------------------------------
    // AUDIT-2026-04 M-7 — subquery / nested-SELECT RBAC enforcement.
    //
    // Before this fix, `rewrite_statement` only processed the
    // top-level SELECT. A predicate like
    //   SELECT id FROM orders WHERE customer IN (SELECT ssn FROM users)
    // passed through untouched because the inner SELECT was never
    // visited; `ssn` was exposed despite the user's `deny_column`.
    //
    // These tests pin that every nested Query (WHERE IN, EXISTS,
    // derived table in FROM, UNION branch) is rewritten under the
    // same policy.
    // -----------------------------------------------------------------

    fn user_denies_ssn_policy() -> kimberlite_rbac::policy::AccessPolicy {
        // `users` stream is fully accessible on the allow-list, but
        // `ssn` is explicitly denied. Any nested reference to
        // `ssn` must be rejected by the recursive walk.
        kimberlite_rbac::policy::AccessPolicy::new(kimberlite_rbac::roles::Role::User)
            .allow_stream("users")
            .allow_stream("orders")
            .allow_column("name")
            .allow_column("email")
            .allow_column("customer")
            .allow_column("id")
            .deny_column("ssn")
    }

    #[test]
    fn subquery_rbac_in_where_clause_enforces_inner_grants() {
        // AUDIT-2026-04 M-7 regression test. Prior to the fix, this
        // returned `Ok(_)` — `ssn` was never seen by the enforcer.
        // After the fix, the inner SELECT is rewritten, and since
        // `ssn` is denied + the inner projection has no other
        // allowed columns, the whole query is rejected.
        let filter = RbacFilter::new(user_denies_ssn_policy());
        let sql =
            "SELECT id FROM orders WHERE customer IN (SELECT ssn FROM users)";
        let stmt = parse_sql(sql);
        let result = filter.rewrite_statement(stmt);
        assert!(
            result.is_err(),
            "nested subquery referencing denied column must be rejected"
        );
    }

    #[test]
    fn subquery_rbac_exists_clause_recurses() {
        // EXISTS subqueries are rewritten too.
        let filter = RbacFilter::new(user_denies_ssn_policy());
        let sql =
            "SELECT id FROM orders WHERE EXISTS (SELECT ssn FROM users)";
        let stmt = parse_sql(sql);
        let result = filter.rewrite_statement(stmt);
        assert!(
            result.is_err(),
            "EXISTS-subquery referencing denied column must be rejected"
        );
    }

    #[test]
    fn subquery_rbac_derived_table_in_from_recurses() {
        // Derived-table subquery in FROM clause.
        let filter = RbacFilter::new(user_denies_ssn_policy());
        let sql =
            "SELECT t.email FROM (SELECT ssn FROM users) t";
        let stmt = parse_sql(sql);
        let result = filter.rewrite_statement(stmt);
        assert!(
            result.is_err(),
            "derived-table SELECT referencing denied column must be rejected"
        );
    }

    #[test]
    fn subquery_rbac_union_both_branches_checked() {
        // UNION — both branches must pass RBAC. The left branch
        // asks for `ssn` (denied) → whole query rejected.
        let filter = RbacFilter::new(user_denies_ssn_policy());
        let sql =
            "SELECT ssn FROM users UNION SELECT name FROM users";
        let stmt = parse_sql(sql);
        let result = filter.rewrite_statement(stmt);
        assert!(
            result.is_err(),
            "UNION branch referencing denied column must be rejected"
        );
    }

    #[test]
    fn subquery_rbac_allowed_subquery_still_succeeds() {
        // Sanity check: a subquery that references only allowed
        // columns is unaffected — the M-7 fix must not introduce
        // false-positive rejections.
        let filter = RbacFilter::new(user_denies_ssn_policy());
        let sql =
            "SELECT id FROM orders WHERE customer IN (SELECT name FROM users)";
        let stmt = parse_sql(sql);
        let result = filter.rewrite_statement(stmt);
        assert!(
            result.is_ok(),
            "all-allowed subquery must pass, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn subquery_rbac_cte_with_denied_column_rejected() {
        // CTEs are rewritten before the outer select reads them.
        let filter = RbacFilter::new(user_denies_ssn_policy());
        let sql = "WITH u AS (SELECT ssn FROM users) SELECT id FROM orders";
        let stmt = parse_sql(sql);
        let result = filter.rewrite_statement(stmt);
        assert!(
            result.is_err(),
            "CTE referencing denied column must be rejected"
        );
    }

    #[test]
    fn subquery_rbac_deeply_nested_three_levels() {
        // Three levels of nesting — inner-most references denied
        // column. Recursive walk must reach it.
        let filter = RbacFilter::new(user_denies_ssn_policy());
        let sql = "SELECT id FROM orders \
                   WHERE customer IN ( \
                     SELECT name FROM users \
                     WHERE email IN (SELECT ssn FROM users) \
                   )";
        let stmt = parse_sql(sql);
        let result = filter.rewrite_statement(stmt);
        assert!(
            result.is_err(),
            "deeply nested subquery referencing denied column must be rejected"
        );
    }

    #[test]
    fn subquery_rbac_in_list_does_not_recurse_into_values() {
        // `IN (literal_list)` is NOT a subquery — no recursion
        // needed. The fix must not trip on regular in-list
        // predicates.
        let filter = RbacFilter::new(user_denies_ssn_policy());
        let sql =
            "SELECT id FROM orders WHERE customer IN ('alice', 'bob')";
        let stmt = parse_sql(sql);
        let result = filter.rewrite_statement(stmt);
        assert!(
            result.is_ok(),
            "in-list with literal values must pass: {:?}",
            result.err()
        );
    }
}
