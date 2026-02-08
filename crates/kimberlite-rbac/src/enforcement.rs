//! Policy enforcement logic.
//!
//! Enforces access control policies at query time.

use crate::policy::{AccessPolicy, RowFilter};
use thiserror::Error;
use tracing::{error, info, warn};

/// Error type for policy enforcement.
#[derive(Debug, Error)]
pub enum EnforcementError {
    /// Access denied by policy.
    #[error("Access denied: {reason}")]
    AccessDenied { reason: String },

    /// Insufficient permissions for operation.
    #[error("Insufficient permissions: {operation} requires {required_permission}")]
    InsufficientPermissions {
        operation: String,
        required_permission: String,
    },

    /// Policy evaluation error.
    #[error("Policy evaluation failed: {0}")]
    PolicyEvaluationFailed(String),
}

/// Result type for enforcement operations.
pub type Result<T> = std::result::Result<T, EnforcementError>;

/// Policy enforcement engine.
///
/// Enforces access control policies at query time:
/// - Stream-level access control
/// - Column filtering (field-level security)
/// - Row-level security (RLS)
/// - Audit logging
pub struct PolicyEnforcer {
    /// Current access policy.
    policy: AccessPolicy,

    /// Whether to log access attempts.
    audit_enabled: bool,
}

impl PolicyEnforcer {
    /// Creates a new policy enforcer.
    pub fn new(policy: AccessPolicy) -> Self {
        Self {
            policy,
            audit_enabled: true,
        }
    }

    /// Disables audit logging (for testing).
    pub fn without_audit(mut self) -> Self {
        self.audit_enabled = false;
        self
    }

    /// Enforces stream-level access control.
    ///
    /// Returns `Ok(())` if access is allowed, `Err` otherwise.
    ///
    /// **Audit:** Logs all access attempts.
    pub fn enforce_stream_access(&self, stream_name: &str) -> Result<()> {
        let allowed = self.policy.allows_stream(stream_name);

        if self.audit_enabled {
            if allowed {
                info!(
                    stream = %stream_name,
                    role = ?self.policy.role,
                    "Stream access granted"
                );
            } else {
                warn!(
                    stream = %stream_name,
                    role = ?self.policy.role,
                    "Stream access denied"
                );
            }
        }

        if allowed {
            Ok(())
        } else {
            Err(EnforcementError::AccessDenied {
                reason: format!("Access to stream '{stream_name}' denied by policy"),
            })
        }
    }

    /// Filters columns based on policy.
    ///
    /// Removes unauthorized columns from the query result.
    ///
    /// # Arguments
    ///
    /// * `columns` - List of column names requested by the query
    ///
    /// # Returns
    ///
    /// Filtered list of column names that the policy allows.
    ///
    /// **Audit:** Logs denied columns (if any).
    pub fn filter_columns(&self, columns: &[String]) -> Vec<String> {
        let allowed: Vec<String> = columns
            .iter()
            .filter(|col| self.policy.allows_column(col))
            .cloned()
            .collect();

        if self.audit_enabled {
            let denied: Vec<&String> = columns
                .iter()
                .filter(|col| !self.policy.allows_column(col))
                .collect();

            if !denied.is_empty() {
                warn!(
                    role = ?self.policy.role,
                    denied_columns = ?denied,
                    "Columns filtered by policy"
                );
            }
        }

        allowed
    }

    /// Returns row-level security filters to inject into the query.
    ///
    /// These filters are added as WHERE clauses to restrict rows
    /// visible to the user.
    ///
    /// # Examples
    ///
    /// For a User role with `tenant_id=42`:
    /// ```sql
    /// WHERE tenant_id = 42
    /// ```
    ///
    /// For multiple filters:
    /// ```sql
    /// WHERE tenant_id = 42 AND status = 'active'
    /// ```
    pub fn row_filters(&self) -> &[RowFilter] {
        self.policy.row_filters()
    }

    /// Generates SQL WHERE clause from row filters.
    ///
    /// # Returns
    ///
    /// SQL WHERE clause (without "WHERE" keyword), or empty string if no filters.
    ///
    /// # Errors
    ///
    /// Returns [`EnforcementError::PolicyEvaluationFailed`] if a filter value
    /// fails SQL literal validation (e.g., contains SQL injection attempts).
    ///
    /// # Examples
    ///
    /// ```
    /// use kimberlite_rbac::enforcement::PolicyEnforcer;
    /// use kimberlite_rbac::policy::{AccessPolicy, RowFilter, RowFilterOperator, StandardPolicies};
    /// use kimberlite_types::TenantId;
    ///
    /// let policy = StandardPolicies::user(TenantId::new(42));
    /// let enforcer = PolicyEnforcer::new(policy).without_audit();
    ///
    /// let where_clause = enforcer.generate_where_clause().unwrap();
    /// assert_eq!(where_clause, "tenant_id = 42");
    /// ```
    pub fn generate_where_clause(&self) -> Result<String> {
        let filters = self.row_filters();

        if filters.is_empty() {
            return Ok(String::new());
        }

        let mut parts = Vec::with_capacity(filters.len());
        for f in filters {
            validate_sql_literal(&f.value)?;
            let op = f.operator.to_sql();
            parts.push(format!("{} {op} {}", f.column, f.value));
        }

        Ok(parts.join(" AND "))
    }

    /// Enforces policy for a complete query.
    ///
    /// Validates:
    /// 1. Stream access is allowed
    /// 2. Columns are filtered
    /// 3. Row filters are applied
    ///
    /// Returns filtered columns and WHERE clause.
    pub fn enforce_query(
        &self,
        stream_name: &str,
        requested_columns: &[String],
    ) -> Result<(Vec<String>, String)> {
        // 1. Check stream access
        self.enforce_stream_access(stream_name)?;

        // 2. Filter columns
        let allowed_columns = self.filter_columns(requested_columns);

        if allowed_columns.is_empty() {
            return Err(EnforcementError::AccessDenied {
                reason: "No authorized columns in query".to_string(),
            });
        }

        // 3. Generate row filters (validates SQL literals)
        let where_clause = self.generate_where_clause()?;

        if self.audit_enabled {
            info!(
                stream = %stream_name,
                role = ?self.policy.role,
                columns = ?allowed_columns,
                where_clause = %where_clause,
                "Query access granted"
            );
        }

        Ok((allowed_columns, where_clause))
    }

    /// Returns the current policy.
    pub fn policy(&self) -> &AccessPolicy {
        &self.policy
    }
}

/// Validates that a value is a safe SQL literal.
///
/// Accepts: integers, booleans (`true`/`false`), `NULL`, and simple quoted
/// strings (single-quoted, no embedded quotes or backslashes).
///
/// Rejects everything else to prevent SQL injection via row filter values.
fn validate_sql_literal(value: &str) -> Result<()> {
    // Integer literals (including negative)
    if value.parse::<i64>().is_ok() {
        return Ok(());
    }

    // Boolean literals
    if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false") {
        return Ok(());
    }

    // NULL literal
    if value.eq_ignore_ascii_case("null") {
        return Ok(());
    }

    // Simple single-quoted string: 'content' with no embedded quotes or backslashes
    if value.len() >= 2
        && value.starts_with('\'')
        && value.ends_with('\'')
        && !value[1..value.len() - 1].contains('\'')
        && !value[1..value.len() - 1].contains('\\')
    {
        return Ok(());
    }

    Err(EnforcementError::PolicyEvaluationFailed(format!(
        "Invalid SQL literal in row filter: {value:?}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{RowFilter, RowFilterOperator, StandardPolicies};
    use crate::roles::Role;
    use kimberlite_types::TenantId;

    #[test]
    fn test_enforce_stream_access_allowed() {
        let policy = StandardPolicies::admin();
        let enforcer = PolicyEnforcer::new(policy).without_audit();

        assert!(enforcer.enforce_stream_access("any_stream").is_ok());
    }

    #[test]
    fn test_enforce_stream_access_denied() {
        let policy = StandardPolicies::auditor();
        let enforcer = PolicyEnforcer::new(policy).without_audit();

        // Auditor can only access audit_* streams
        assert!(enforcer.enforce_stream_access("audit_log").is_ok());
        assert!(enforcer.enforce_stream_access("patient_records").is_err());
    }

    #[test]
    fn test_filter_columns() {
        let policy = AccessPolicy::new(Role::Analyst)
            .allow_column("*")
            .deny_column("ssn");

        let enforcer = PolicyEnforcer::new(policy).without_audit();

        let requested = vec!["name".to_string(), "email".to_string(), "ssn".to_string()];

        let allowed = enforcer.filter_columns(&requested);

        assert_eq!(allowed.len(), 2);
        assert!(allowed.contains(&"name".to_string()));
        assert!(allowed.contains(&"email".to_string()));
        assert!(!allowed.contains(&"ssn".to_string()));
    }

    #[test]
    fn test_generate_where_clause_single_filter() {
        let tenant_id = TenantId::new(42);
        let policy = StandardPolicies::user(tenant_id);
        let enforcer = PolicyEnforcer::new(policy).without_audit();

        let where_clause = enforcer.generate_where_clause().unwrap();
        assert_eq!(where_clause, "tenant_id = 42");
    }

    #[test]
    fn test_generate_where_clause_multiple_filters() {
        let policy = AccessPolicy::new(Role::User)
            .allow_stream("*")
            .allow_column("*")
            .with_row_filter(RowFilter::new("tenant_id", RowFilterOperator::Eq, "42"))
            .with_row_filter(RowFilter::new("status", RowFilterOperator::Eq, "'active'"));

        let enforcer = PolicyEnforcer::new(policy).without_audit();

        let where_clause = enforcer.generate_where_clause().unwrap();
        assert_eq!(where_clause, "tenant_id = 42 AND status = 'active'");
    }

    #[test]
    fn test_generate_where_clause_no_filters() {
        let policy = StandardPolicies::admin();
        let enforcer = PolicyEnforcer::new(policy).without_audit();

        let where_clause = enforcer.generate_where_clause().unwrap();
        assert_eq!(where_clause, "");
    }

    #[test]
    fn test_generate_where_clause_rejects_injection() {
        let policy = AccessPolicy::new(Role::User)
            .allow_stream("*")
            .allow_column("*")
            .with_row_filter(RowFilter::new(
                "tenant_id",
                RowFilterOperator::Eq,
                "1; DROP TABLE users",
            ));

        let enforcer = PolicyEnforcer::new(policy).without_audit();
        let result = enforcer.generate_where_clause();
        assert!(result.is_err());
    }

    #[test]
    fn test_enforce_query_full_flow() {
        let policy = AccessPolicy::new(Role::User)
            .with_tenant(TenantId::new(42))
            .allow_stream("patient_*")
            .allow_column("*")
            .deny_column("ssn")
            .with_row_filter(RowFilter::new("tenant_id", RowFilterOperator::Eq, "42"));

        let enforcer = PolicyEnforcer::new(policy).without_audit();

        let requested_columns = vec!["name".to_string(), "email".to_string(), "ssn".to_string()];

        let (allowed_columns, where_clause) = enforcer
            .enforce_query("patient_records", &requested_columns)
            .unwrap();

        assert_eq!(allowed_columns.len(), 2);
        assert!(allowed_columns.contains(&"name".to_string()));
        assert!(allowed_columns.contains(&"email".to_string()));
        assert!(!allowed_columns.contains(&"ssn".to_string()));

        assert_eq!(where_clause, "tenant_id = 42");
    }

    #[test]
    fn test_enforce_query_stream_denied() {
        let policy = StandardPolicies::auditor();
        let enforcer = PolicyEnforcer::new(policy).without_audit();

        let columns = vec!["name".to_string()];
        let result = enforcer.enforce_query("patient_records", &columns);

        assert!(result.is_err());
        match result {
            Err(EnforcementError::AccessDenied { reason }) => {
                assert!(reason.contains("patient_records"));
            }
            _ => panic!("Expected AccessDenied error"),
        }
    }

    #[test]
    fn test_enforce_query_no_authorized_columns() {
        let policy = AccessPolicy::new(Role::User)
            .allow_stream("*")
            .allow_column("public_*"); // Only public columns allowed

        let enforcer = PolicyEnforcer::new(policy).without_audit();

        let requested = vec!["private_ssn".to_string(), "private_address".to_string()];

        let result = enforcer.enforce_query("patient_records", &requested);

        assert!(result.is_err());
        match result {
            Err(EnforcementError::AccessDenied { reason }) => {
                assert!(reason.contains("No authorized columns"));
            }
            _ => panic!("Expected AccessDenied error"),
        }
    }
}
