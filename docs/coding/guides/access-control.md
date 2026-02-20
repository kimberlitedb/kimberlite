---
title: "Access Control Implementation Guide"
section: "coding/guides"
slug: "access-control"
order: 4
---

# Access Control Implementation Guide

This guide shows you how to implement Role-Based Access Control (RBAC) in your Kimberlite applications.

---

## Quick Start

### 1. Add Dependency

```toml
[dependencies]
kimberlite-rbac = "0.4"
```

### 2. Create a Policy

```rust
use kimberlite_rbac::policy::StandardPolicies;
use kimberlite_types::TenantId;

// For a standard user
let tenant_id = TenantId::new(42);
let policy = StandardPolicies::user(tenant_id);

// For an admin
let admin_policy = StandardPolicies::admin();
```

### 3. Enforce the Policy

```rust
use kimberlite_rbac::enforcement::PolicyEnforcer;

let enforcer = PolicyEnforcer::new(policy);

// Check stream access
enforcer.enforce_stream_access("users")?;

// Filter columns
let requested = vec!["name".to_string(), "email".to_string(), "ssn".to_string()];
let allowed = enforcer.filter_columns(&requested);

// Generate WHERE clause
let where_clause = enforcer.generate_where_clause();
// Result: "tenant_id = 42"
```

---

## Common Patterns

### Pattern 1: Multi-Tenant SaaS Application

**Use Case:** Each customer (tenant) should only access their own data.

```rust
use kimberlite_rbac::policy::StandardPolicies;
use kimberlite_types::TenantId;

// Extract tenant ID from authenticated user
let user = get_authenticated_user();
let tenant_id = user.tenant_id;

// Create tenant-isolated policy
let policy = StandardPolicies::user(tenant_id);
let enforcer = PolicyEnforcer::new(policy);

// All queries automatically filtered:
// SELECT * FROM orders WHERE tenant_id = 42
```

---

### Pattern 2: Hide Sensitive Columns

**Use Case:** Business analysts can see aggregate data but not PII.

```rust
use kimberlite_rbac::policy::AccessPolicy;
use kimberlite_rbac::roles::Role;

let policy = AccessPolicy::new(Role::Analyst)
    .allow_stream("*")
    .allow_column("*")
    .deny_column("ssn")
    .deny_column("password")
    .deny_column("credit_card");

let enforcer = PolicyEnforcer::new(policy);

// Query: SELECT name, email, ssn FROM users
// Rewritten: SELECT name, email FROM users
```

---

### Pattern 3: Custom Row Filters

**Use Case:** Show only active records to certain users.

```rust
use kimberlite_rbac::policy::{AccessPolicy, RowFilter, RowFilterOperator};
use kimberlite_rbac::roles::Role;

let policy = AccessPolicy::new(Role::User)
    .allow_stream("*")
    .allow_column("*")
    .with_row_filter(RowFilter::new(
        "status",
        RowFilterOperator::Eq,
        "active",
    ));

let enforcer = PolicyEnforcer::new(policy);

// Original: SELECT * FROM users
// Rewritten: SELECT * FROM users WHERE status = 'active'
```

---

### Pattern 4: Wildcard Column Filtering

**Use Case:** Deny all columns with a specific prefix.

```rust
use kimberlite_rbac::policy::AccessPolicy;
use kimberlite_rbac::roles::Role;

let policy = AccessPolicy::new(Role::Analyst)
    .allow_stream("*")
    .allow_column("*")
    .deny_column("pii_*")        // Deny all PII columns
    .deny_column("internal_*");  // Deny all internal columns

// Denies: pii_ssn, pii_address, internal_notes
// Allows: public_name, public_email
```

---

### Pattern 5: Stream-Level Access Control

**Use Case:** Auditors can only access audit logs.

```rust
use kimberlite_rbac::policy::StandardPolicies;

let policy = StandardPolicies::auditor();

let enforcer = PolicyEnforcer::new(policy);

// Allows: audit_log, audit_access, audit_system
enforcer.enforce_stream_access("audit_log")?; // ✓ OK

// Denies: users, orders, payments
let result = enforcer.enforce_stream_access("users");
assert!(result.is_err()); // ✗ Access denied
```

---

## Advanced Usage

### Custom Policy with Multiple Filters

```rust
use kimberlite_rbac::policy::{AccessPolicy, RowFilter, RowFilterOperator};
use kimberlite_rbac::roles::Role;
use kimberlite_types::TenantId;

let policy = AccessPolicy::new(Role::User)
    .with_tenant(TenantId::new(42))
    // Stream filters
    .allow_stream("users")
    .allow_stream("orders")
    .deny_stream("admin_*")
    // Column filters
    .allow_column("*")
    .deny_column("ssn")
    .deny_column("password")
    // Row filters (combined with AND)
    .with_row_filter(RowFilter::new(
        "tenant_id",
        RowFilterOperator::Eq,
        "42",
    ))
    .with_row_filter(RowFilter::new(
        "status",
        RowFilterOperator::Eq,
        "active",
    ));

// WHERE tenant_id = 42 AND status = 'active'
```

---

### Query Rewriting with kimberlite-query

**Integration Example:**

```rust
use kimberlite_query::rbac_filter::RbacFilter;
use kimberlite_rbac::policy::StandardPolicies;
use kimberlite_types::TenantId;
use sqlparser::parser::Parser;
use sqlparser::dialect::GenericDialect;

// 1. Create policy
let policy = StandardPolicies::user(TenantId::new(42));

// 2. Create RBAC filter
let filter = RbacFilter::new(policy);

// 3. Parse SQL
let dialect = GenericDialect {};
let sql = "SELECT name, ssn FROM users";
let mut stmt = Parser::parse_sql(&dialect, sql)?
    .into_iter()
    .next()
    .unwrap();

// 4. Rewrite query
let rewritten = filter.rewrite_statement(stmt)?;

// Result: SELECT name FROM users WHERE tenant_id = 42
// (ssn column removed, WHERE clause injected)
```

---

## Testing Access Control

### Unit Testing Policies

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_rbac::enforcement::PolicyEnforcer;
    use kimberlite_rbac::policy::StandardPolicies;
    use kimberlite_types::TenantId;

    #[test]
    fn test_user_policy_tenant_isolation() {
        let tenant_id = TenantId::new(42);
        let policy = StandardPolicies::user(tenant_id);
        let enforcer = PolicyEnforcer::new(policy).without_audit();

        // Verify WHERE clause includes tenant filter
        let where_clause = enforcer.generate_where_clause();
        assert!(where_clause.contains("tenant_id"));
        assert!(where_clause.contains("42"));
    }

    #[test]
    fn test_column_filtering() {
        let policy = AccessPolicy::new(Role::Analyst)
            .allow_stream("*")
            .allow_column("*")
            .deny_column("ssn");

        let enforcer = PolicyEnforcer::new(policy).without_audit();

        let columns = vec!["name".to_string(), "ssn".to_string()];
        let allowed = enforcer.filter_columns(&columns);

        assert!(allowed.contains(&"name".to_string()));
        assert!(!allowed.contains(&"ssn".to_string()));
    }

    #[test]
    fn test_access_denied() {
        let policy = StandardPolicies::auditor();
        let enforcer = PolicyEnforcer::new(policy).without_audit();

        // Auditor cannot access user data
        let result = enforcer.enforce_stream_access("users");
        assert!(result.is_err());
    }
}
```

---

## Troubleshooting

### Error: "Access denied"

**Cause:** Policy does not allow access to the requested stream or columns.

**Solution:** Check policy configuration:

```rust
let policy = enforcer.policy();
println!("Role: {:?}", policy.role);
println!("Tenant: {:?}", policy.tenant_id);
println!("Stream filters: {:?}", policy.stream_filters);
println!("Column filters: {:?}", policy.column_filters);
```

---

### Error: "No authorized columns in query"

**Cause:** All requested columns are denied by the policy.

**Solution:** Add allow rules for required columns:

```rust
let policy = AccessPolicy::new(Role::User)
    .allow_stream("users")
    .allow_column("name")    // ✓ Explicitly allow
    .allow_column("email");  // ✓ Explicitly allow
```

---

### Query returns empty results

**Cause:** Row-level security filter excludes all rows.

**Solution:** Verify row filter logic:

```rust
let filters = enforcer.row_filters();
for filter in filters {
    println!("Column: {}, Operator: {:?}, Value: {}",
             filter.column, filter.operator, filter.value);
}
```

---

## Best Practices

### 1. Use Standard Policies First

Start with `StandardPolicies` before creating custom policies:

```rust
// ✓ Good
let policy = StandardPolicies::user(tenant_id);

// ✗ Avoid (unless you have specific requirements)
let policy = AccessPolicy::new(Role::User)
    .with_tenant(tenant_id)
    .allow_stream("*")
    .allow_column("*");
```

---

### 2. Test Policy Changes

Always test RBAC policies in a staging environment before production:

```rust
#[test]
fn test_production_policy() {
    let policy = create_production_policy();
    let enforcer = PolicyEnforcer::new(policy);

    // Test all expected access patterns
    assert!(enforcer.enforce_stream_access("users").is_ok());
    assert!(enforcer.enforce_stream_access("admin_panel").is_err());
}
```

---

### 3. Enable Audit Logging in Production

```rust
// Production: Audit enabled (default)
let enforcer = PolicyEnforcer::new(policy);

// Testing only: Audit disabled
let enforcer = PolicyEnforcer::new(policy).without_audit();
```

---

### 4. Monitor Access Denials

Set up alerts for excessive access denials:

```sql
SELECT role, stream, COUNT(*) as denials
FROM audit_access_log
WHERE decision = 'DENY'
  AND timestamp > NOW() - INTERVAL '1 hour'
GROUP BY role, stream
HAVING COUNT(*) > 100;
```

---

## See Also

- [RBAC Concepts](..//docs/concepts/rbac) - Roles, permissions, and compliance
- [Data Classification](..//docs/concepts/data-classification) - Classification levels
- [Compliance Overview](..//docs/concepts/compliance) - Multi-framework compliance
- [Audit Logging](..//docs/operating/audit-logging) - Audit trail configuration

---

**Next Steps:**
1. Define your application's roles and permissions
2. Create access policies for each role
3. Test policies with unit tests
4. Deploy with audit logging enabled
5. Monitor access patterns in production
