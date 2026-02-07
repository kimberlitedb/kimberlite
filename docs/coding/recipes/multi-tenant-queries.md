# Multi-Tenant Queries

Safely query data across tenants in Kimberlite.

## Default: Strict Tenant Isolation

By default, tenants **cannot** query each other's data:

```sql
-- Client authenticated as Tenant 1
SELECT * FROM patients WHERE id = 123;
-- Returns: Tenant 1's patient 123 (if exists)

-- Client authenticated as Tenant 2
SELECT * FROM patients WHERE id = 123;
-- Returns: Tenant 2's patient 123 (if exists)
-- CANNOT see Tenant 1's data
```

This is enforced at the protocol level—no way to bypass it with SQL.

## When You Need Cross-Tenant Access

Sometimes cross-tenant queries are legitimate:

| Use Case | Example |
|----------|---------|
| **Multi-hospital referrals** | Hospital A sends patient to Hospital B |
| **Shared research data** | Multiple institutions collaborate on study |
| **Parent organization** | Corporate entity oversees subsidiaries |
| **Data sharing agreements** | Explicit consent to share specific records |

## Data Sharing API

Use the explicit data sharing API (not SQL):

```rust,ignore
use kimberlite::Client;

// Tenant A grants read access to Tenant B
client.grant_access(GrantConfig {
    from_tenant: TenantId::new(1),  // Tenant A
    to_tenant: TenantId::new(2),    // Tenant B
    stream: StreamId::new(1, 100),  // Specific stream
    permissions: Permissions::Read,
    expiration: Some(Duration::from_secs(3600)),  // 1 hour
})?;

// Now Tenant B can query that stream
let data = client_b.query_shared_stream(
    TenantId::new(1),  // From Tenant A
    StreamId::new(1, 100),
)?;
```

**Key properties:**
- **Explicit:** Requires grant, not implicit JOIN
- **Audited:** All cross-tenant access logged
- **Revocable:** Grants can be revoked at any time
- **Time-limited:** Grants expire automatically
- **Fine-grained:** Grant access to specific streams, not entire tenant

## Grant Patterns

### Pattern 1: One-Time Access

```rust,ignore
// Grant access for 1 hour
client.grant_access(GrantConfig {
    from_tenant: tenant_a,
    to_tenant: tenant_b,
    stream: patient_stream,
    permissions: Permissions::Read,
    expiration: Some(Duration::from_secs(3600)),
})?;

// Tenant B reads data
let data = client_b.query_shared_stream(tenant_a, patient_stream)?;

// Grant automatically expires after 1 hour
```

### Pattern 2: Standing Access

```rust,ignore
// Grant long-term access (1 year)
client.grant_access(GrantConfig {
    from_tenant: hospital_system,
    to_tenant: research_org,
    stream: de_identified_data_stream,
    permissions: Permissions::Read,
    expiration: Some(Duration::from_secs(86400 * 365)),
})?;
```

### Pattern 3: Conditional Access

```rust,ignore
// Grant access only if patient consented
if patient_consented_to_sharing(patient_id)? {
    client.grant_access(GrantConfig {
        from_tenant: tenant_a,
        to_tenant: tenant_b,
        stream: patient_stream,
        permissions: Permissions::Read,
        expiration: Some(Duration::from_secs(86400 * 30)),  // 30 days
    })?;
} else {
    return Err(Error::ConsentRequired);
}
```

## Querying Shared Data

### From Rust

```rust,ignore
use kimberlite::Client;

// Tenant B queries Tenant A's shared data
let shared_data = client_b.query_shared(QuerySharedConfig {
    owner_tenant: TenantId::new(1),  // Tenant A
    stream: StreamId::new(1, 100),
    query: "SELECT * FROM patients WHERE id = ?",
    params: &[&patient_id],
})?;

// Check if we have access
match shared_data {
    Ok(data) => println!("Data: {:?}", data),
    Err(Error::AccessDenied) => println!("No grant exists"),
    Err(e) => return Err(e),
}
```

### From Python

```python
from kimberlite import Client, TenantId, StreamId

client_b = Client("localhost:5432", tenant_id=2)

# Query Tenant A's shared data
try:
    shared_data = client_b.query_shared(
        owner_tenant=TenantId(1),
        stream=StreamId(1, 100),
        query="SELECT * FROM patients WHERE id = ?",
        params=[patient_id]
    )
    print(f"Data: {shared_data}")
except AccessDenied:
    print("No grant exists")
```

## Audit Trail

All cross-tenant access is logged:

```sql
-- Query cross-tenant access log
SELECT
    from_tenant,
    to_tenant,
    stream,
    accessed_at,
    accessed_by
FROM __cross_tenant_access_log
WHERE from_tenant = 1
  AND accessed_at > NOW() - INTERVAL '30 days'
ORDER BY accessed_at DESC;
```

**Logged information:**
- Which tenant accessed data
- Which tenant's data was accessed
- What stream was accessed
- When the access occurred
- Which user performed the access
- Whether access was granted or denied

## Revoking Access

```rust,ignore
// Revoke Tenant B's access to Tenant A's data
client_a.revoke_access(RevokeConfig {
    from_tenant: TenantId::new(1),
    to_tenant: TenantId::new(2),
    stream: StreamId::new(1, 100),
})?;

// Tenant B's subsequent queries will fail
let result = client_b.query_shared_stream(
    TenantId::new(1),
    StreamId::new(1, 100),
)?;
// Error: AccessDenied
```

## Parent-Child Tenants

For parent organizations that need to see all child data:

```rust,ignore
// Create parent-child relationship
client.create_tenant_hierarchy(HierarchyConfig {
    parent: TenantId::new(100),  // Corporate HQ
    children: vec![
        TenantId::new(1),  // Hospital A
        TenantId::new(2),  // Hospital B
        TenantId::new(3),  // Hospital C
    ],
})?;

// Parent automatically has read access to all children
let all_patients = client_parent.query_descendants(
    "SELECT * FROM patients",
    &[],
)?;
```

**Use case:** Corporate entity needs to generate reports across all subsidiaries.

## Data Sharing Agreements

Formalize sharing with contracts:

```rust,ignore
struct DataSharingAgreement {
    from_tenant: TenantId,
    to_tenant: TenantId,
    purpose: String,  // "Patient referral", "Research collaboration", etc.
    legal_basis: String,  // HIPAA, GDPR, contract reference
    effective_date: Date,
    expiration_date: Date,
    signed_by: UserId,
}

// Create agreement
let agreement = client.create_data_sharing_agreement(DataSharingAgreement {
    from_tenant: TenantId::new(1),
    to_tenant: TenantId::new(2),
    purpose: "Multi-hospital patient care coordination".to_string(),
    legal_basis: "HIPAA 164.506(c)(1) - Treatment".to_string(),
    effective_date: Utc::today(),
    expiration_date: Utc::today() + Duration::days(365),
    signed_by: UserId::new(456),
})?;

// Grant access based on agreement
client.grant_access_with_agreement(
    agreement.id,
    StreamId::new(1, patient_stream),
)?;
```

## Query Aggregates Across Tenants

For analytics across tenants (no PHI):

```rust,ignore
// Get aggregate stats (no individual records)
let stats = client_admin.query_aggregate(
    "SELECT COUNT(*), AVG(age) FROM patients",
    tenants: vec![TenantId::new(1), TenantId::new(2), TenantId::new(3)],
)?;

// Result: Aggregate statistics only, no individual records
// { count: 5000, avg_age: 42.5 }
```

**Use case:** Generate reports without exposing individual patient data.

## Best Practices

### 1. Always Require Justification

```rust,ignore
struct GrantConfig {
    from_tenant: TenantId,
    to_tenant: TenantId,
    stream: StreamId,
    justification: String,  // REQUIRED
    // ...
}

// Good
grant_access(GrantConfig {
    justification: "Patient referral for cardiology consult - authorization #12345".to_string(),
    // ...
})?;

// Bad: No justification
grant_access(GrantConfig {
    justification: "".to_string(),  // Will be rejected
    // ...
})?;
```

### 2. Use Shortest Possible Expiration

```rust,ignore
// Good: 1 hour for one-time access
expiration: Some(Duration::from_secs(3600))

// Bad: Indefinite access
expiration: None
```

### 3. Grant Minimal Scope

```rust,ignore
// Good: Specific stream (single patient)
stream: StreamId::new(tenant, patient_id)

// Bad: All streams (entire tenant)
stream: StreamId::wildcard()  // DON'T DO THIS
```

### 4. Review Grants Regularly

```sql
-- Find grants expiring soon
SELECT * FROM __data_sharing_grants
WHERE expiration < NOW() + INTERVAL '7 days';

-- Find unused grants
SELECT * FROM __data_sharing_grants g
LEFT JOIN __cross_tenant_access_log a ON g.grant_id = a.grant_id
WHERE a.grant_id IS NULL;
```

### 5. Test Revocation

```rust,ignore
#[test]
fn test_revoke_access() {
    // Grant access
    client_a.grant_access(config)?;

    // Verify Tenant B can access
    let data = client_b.query_shared_stream(tenant_a, stream)?;
    assert!(data.is_ok());

    // Revoke access
    client_a.revoke_access(config)?;

    // Verify Tenant B cannot access
    let result = client_b.query_shared_stream(tenant_a, stream);
    assert!(matches!(result, Err(Error::AccessDenied)));
}
```

## Limitations

### No Direct SQL Joins

```sql
-- This is NOT possible
SELECT t1.name, t2.appointments
FROM tenant_1.patients t1
JOIN tenant_2.appointments t2 ON t1.id = t2.patient_id;
-- Error: Cross-tenant SQL joins not supported
```

**Workaround:** Use the data sharing API, not SQL.

### No Wildcard Grants

```rust,ignore
// This is NOT possible
grant_access(GrantConfig {
    stream: StreamId::wildcard(),  // All streams
    // ...
})?;
// Error: Must specify exact stream
```

**Why:** Too broad, violates principle of least privilege.

## Related Documentation

- **[Multi-tenancy](../../concepts/multitenancy.md)** - Tenant isolation architecture
- **[Data Sharing Design](../../internals/design/data-sharing.md)** - Implementation details
- **[Compliance](../../concepts/compliance.md)** - Audit requirements

---

**Key Takeaway:** Kimberlite enforces strict tenant isolation by default. Cross-tenant access requires explicit grants through the data sharing API, not SQL. All cross-tenant access is audited.
