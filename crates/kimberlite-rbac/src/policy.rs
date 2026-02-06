//! Access control policies.
//!
//! Defines policies that govern who can access what data and how.

use crate::roles::Role;
use kimberlite_types::TenantId;
use serde::{Deserialize, Serialize};

/// Filter for stream-level access control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamFilter {
    /// Stream ID pattern (supports wildcards).
    ///
    /// Examples:
    /// - `"patient_*"` - All streams starting with "patient_"
    /// - `"*"` - All streams (no restriction)
    /// - `"audit_log"` - Exact stream name
    pub pattern: String,

    /// Whether this is an allow or deny rule.
    pub allow: bool,
}

impl StreamFilter {
    /// Creates a new stream filter.
    pub fn new(pattern: impl Into<String>, allow: bool) -> Self {
        Self {
            pattern: pattern.into(),
            allow,
        }
    }

    /// Returns whether this filter matches the given stream name.
    pub fn matches(&self, stream_name: &str) -> bool {
        // Simple wildcard matching (* matches any sequence)
        let pattern = &self.pattern;

        if pattern == "*" {
            return true;
        }

        if pattern.ends_with('*') {
            let prefix = &pattern[..pattern.len() - 1];
            return stream_name.starts_with(prefix);
        }

        if let Some(suffix) = pattern.strip_prefix('*') {
            return stream_name.ends_with(suffix);
        }

        // Exact match
        stream_name == pattern
    }
}

/// Filter for column-level access control (field-level security).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnFilter {
    /// Column name pattern (supports wildcards).
    ///
    /// Examples:
    /// - `"ssn"` - Exact column name
    /// - `"pii_*"` - All columns starting with "pii_"
    /// - `"*_secret"` - All columns ending with "_secret"
    pub pattern: String,

    /// Whether this is an allow or deny rule.
    pub allow: bool,
}

impl ColumnFilter {
    /// Creates a new column filter.
    pub fn new(pattern: impl Into<String>, allow: bool) -> Self {
        Self {
            pattern: pattern.into(),
            allow,
        }
    }

    /// Returns whether this filter matches the given column name.
    pub fn matches(&self, column_name: &str) -> bool {
        let pattern = &self.pattern;

        if pattern == "*" {
            return true;
        }

        if pattern.ends_with('*') {
            let prefix = &pattern[..pattern.len() - 1];
            return column_name.starts_with(prefix);
        }

        if let Some(suffix) = pattern.strip_prefix('*') {
            return column_name.ends_with(suffix);
        }

        // Exact match
        column_name == pattern
    }
}

/// Filter for row-level security (RLS).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowFilter {
    /// Column name to filter on.
    pub column: String,

    /// Operator for comparison.
    pub operator: RowFilterOperator,

    /// Value to compare against.
    pub value: String,
}

impl RowFilter {
    /// Creates a new row filter.
    pub fn new(
        column: impl Into<String>,
        operator: RowFilterOperator,
        value: impl Into<String>,
    ) -> Self {
        Self {
            column: column.into(),
            operator,
            value: value.into(),
        }
    }
}

/// Operator for row-level security filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RowFilterOperator {
    /// Equal (=)
    Eq,

    /// Not equal (!=)
    Ne,

    /// Less than (<)
    Lt,

    /// Less than or equal (<=)
    Le,

    /// Greater than (>)
    Gt,

    /// Greater than or equal (>=)
    Ge,

    /// IN list
    In,

    /// NOT IN list
    NotIn,
}

impl RowFilterOperator {
    /// Returns the SQL representation of this operator.
    pub fn to_sql(&self) -> &'static str {
        match self {
            RowFilterOperator::Eq => "=",
            RowFilterOperator::Ne => "!=",
            RowFilterOperator::Lt => "<",
            RowFilterOperator::Le => "<=",
            RowFilterOperator::Gt => ">",
            RowFilterOperator::Ge => ">=",
            RowFilterOperator::In => "IN",
            RowFilterOperator::NotIn => "NOT IN",
        }
    }
}

/// Access control policy.
///
/// Defines what a user with a given role can access and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessPolicy {
    /// Role this policy applies to.
    pub role: Role,

    /// Tenant ID (for tenant isolation).
    ///
    /// If None, cross-tenant access is allowed (Analyst, Admin only).
    pub tenant_id: Option<TenantId>,

    /// Stream-level filters.
    ///
    /// **Evaluation Order:**
    /// 1. Deny rules are evaluated first
    /// 2. If any deny rule matches, access is denied
    /// 3. Allow rules are evaluated next
    /// 4. If no allow rule matches, access is denied
    pub stream_filters: Vec<StreamFilter>,

    /// Column-level filters (field-level security).
    ///
    /// Unauthorized columns are removed from query results.
    pub column_filters: Vec<ColumnFilter>,

    /// Row-level security filters.
    ///
    /// Injected as WHERE clauses in queries.
    pub row_filters: Vec<RowFilter>,

    /// Field masking policy.
    ///
    /// Applied to query results after column filtering.
    /// Masks sensitive fields based on role (e.g., SSN → `***-**-1234`).
    pub masking_policy: Option<crate::masking::MaskingPolicy>,
}

impl AccessPolicy {
    /// Creates a new access policy for the given role.
    pub fn new(role: Role) -> Self {
        Self {
            role,
            tenant_id: None,
            stream_filters: Vec::new(),
            column_filters: Vec::new(),
            row_filters: Vec::new(),
            masking_policy: None,
        }
    }

    /// Sets the tenant ID for this policy (tenant isolation).
    pub fn with_tenant(mut self, tenant_id: TenantId) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }

    /// Sets the field masking policy.
    ///
    /// Masking is applied to query results after column filtering.
    pub fn with_masking(mut self, policy: crate::masking::MaskingPolicy) -> Self {
        self.masking_policy = Some(policy);
        self
    }

    /// Adds a stream filter.
    pub fn allow_stream(mut self, pattern: impl Into<String>) -> Self {
        self.stream_filters.push(StreamFilter::new(pattern, true));
        self
    }

    /// Adds a stream deny rule.
    pub fn deny_stream(mut self, pattern: impl Into<String>) -> Self {
        self.stream_filters.push(StreamFilter::new(pattern, false));
        self
    }

    /// Adds a column filter.
    pub fn allow_column(mut self, pattern: impl Into<String>) -> Self {
        self.column_filters.push(ColumnFilter::new(pattern, true));
        self
    }

    /// Adds a column deny rule.
    pub fn deny_column(mut self, pattern: impl Into<String>) -> Self {
        self.column_filters.push(ColumnFilter::new(pattern, false));
        self
    }

    /// Adds a row-level security filter.
    pub fn with_row_filter(mut self, filter: RowFilter) -> Self {
        self.row_filters.push(filter);
        self
    }

    /// Returns whether access to the given stream is allowed.
    pub fn allows_stream(&self, stream_name: &str) -> bool {
        // 1. Check deny rules first
        for filter in &self.stream_filters {
            if !filter.allow && filter.matches(stream_name) {
                return false; // Explicit deny
            }
        }

        // 2. Check allow rules
        for filter in &self.stream_filters {
            if filter.allow && filter.matches(stream_name) {
                return true; // Explicit allow
            }
        }

        // 3. Default deny (if no allow rule matches)
        false
    }

    /// Returns whether access to the given column is allowed.
    pub fn allows_column(&self, column_name: &str) -> bool {
        // 1. Check deny rules first
        for filter in &self.column_filters {
            if !filter.allow && filter.matches(column_name) {
                return false; // Explicit deny
            }
        }

        // 2. Check allow rules
        for filter in &self.column_filters {
            if filter.allow && filter.matches(column_name) {
                return true; // Explicit allow
            }
        }

        // 3. Default deny
        false
    }

    /// Returns the row-level security filters for this policy.
    pub fn row_filters(&self) -> &[RowFilter] {
        &self.row_filters
    }
}

/// Standard policies for each role.
pub struct StandardPolicies;

impl StandardPolicies {
    /// Creates the standard policy for an Admin role.
    ///
    /// **Access:**
    /// - All streams (no restrictions)
    /// - All columns (no restrictions)
    /// - No row filters (sees all data)
    pub fn admin() -> AccessPolicy {
        AccessPolicy::new(Role::Admin)
            .allow_stream("*") // All streams
            .allow_column("*") // All columns
    }

    /// Creates the standard policy for an Analyst role.
    ///
    /// **Access:**
    /// - All non-audit streams
    /// - All columns except PII (depends on data class)
    /// - No row filters (cross-tenant read)
    pub fn analyst() -> AccessPolicy {
        AccessPolicy::new(Role::Analyst)
            .allow_stream("*")
            .deny_stream("audit_*") // No audit log access
            .allow_column("*")
    }

    /// Creates the standard policy for a User role.
    ///
    /// **Access:**
    /// - Streams matching tenant ID
    /// - All columns (within tenant)
    /// - Row filter: `tenant_id` = <user's tenant>
    pub fn user(tenant_id: TenantId) -> AccessPolicy {
        AccessPolicy::new(Role::User)
            .with_tenant(tenant_id)
            .allow_stream("*")
            .allow_column("*")
            .with_row_filter(RowFilter::new(
                "tenant_id",
                RowFilterOperator::Eq,
                u64::from(tenant_id).to_string(),
            ))
    }

    /// Creates the standard policy for an Auditor role.
    ///
    /// **Access:**
    /// - Audit logs only
    /// - All audit log columns
    /// - No row filters (sees all audit entries)
    pub fn auditor() -> AccessPolicy {
        AccessPolicy::new(Role::Auditor)
            .allow_stream("audit_*") // Audit logs only
            .allow_column("*")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_filter_wildcard() {
        let filter = StreamFilter::new("patient_*", true);

        assert!(filter.matches("patient_records"));
        assert!(filter.matches("patient_vitals"));
        assert!(!filter.matches("audit_log"));

        let all_filter = StreamFilter::new("*", true);
        assert!(all_filter.matches("any_stream"));
    }

    #[test]
    fn test_column_filter_wildcard() {
        let filter = ColumnFilter::new("pii_*", false); // Deny PII columns

        assert!(filter.matches("pii_ssn"));
        assert!(filter.matches("pii_address"));
        assert!(!filter.matches("public_name"));
    }

    #[test]
    fn test_policy_stream_access() {
        let policy = AccessPolicy::new(Role::User)
            .allow_stream("patient_*")
            .deny_stream("patient_confidential");

        // Deny takes precedence
        assert!(!policy.allows_stream("patient_confidential"));

        // Allow rule matches
        assert!(policy.allows_stream("patient_records"));

        // No rule matches
        assert!(!policy.allows_stream("audit_log"));
    }

    #[test]
    fn test_policy_column_access() {
        let policy = AccessPolicy::new(Role::Analyst)
            .allow_column("*")
            .deny_column("ssn");

        assert!(policy.allows_column("name"));
        assert!(policy.allows_column("email"));
        assert!(!policy.allows_column("ssn")); // Denied
    }

    #[test]
    fn test_standard_policies() {
        let admin = StandardPolicies::admin();
        assert!(admin.allows_stream("any_stream"));
        assert!(admin.allows_column("any_column"));

        let analyst = StandardPolicies::analyst();
        assert!(analyst.allows_stream("patient_records"));
        assert!(!analyst.allows_stream("audit_system_events"));

        let tenant_id = TenantId::new(42);
        let user = StandardPolicies::user(tenant_id);
        assert_eq!(user.tenant_id, Some(tenant_id));
        assert_eq!(user.row_filters.len(), 1);

        let auditor = StandardPolicies::auditor();
        assert!(auditor.allows_stream("audit_access_log"));
        assert!(!auditor.allows_stream("patient_records"));
    }

    #[test]
    fn test_row_filter_operator_sql() {
        assert_eq!(RowFilterOperator::Eq.to_sql(), "=");
        assert_eq!(RowFilterOperator::Ne.to_sql(), "!=");
        assert_eq!(RowFilterOperator::Lt.to_sql(), "<");
        assert_eq!(RowFilterOperator::Le.to_sql(), "<=");
        assert_eq!(RowFilterOperator::Gt.to_sql(), ">");
        assert_eq!(RowFilterOperator::Ge.to_sql(), ">=");
        assert_eq!(RowFilterOperator::In.to_sql(), "IN");
        assert_eq!(RowFilterOperator::NotIn.to_sql(), "NOT IN");
    }
}
