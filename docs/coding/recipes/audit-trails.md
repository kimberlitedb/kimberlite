---
title: "Audit Trails"
section: "coding/recipes"
slug: "audit-trails"
order: 2
---

# Audit Trails

Implement comprehensive audit logging in Kimberlite applications.

## Overview

Kimberlite provides audit trails by default—every write is logged. This recipe shows how to:

- Query audit logs
- Track who did what and when
- Generate compliance reports
- Implement custom audit logic

## Built-In Audit Log

Every operation is automatically logged:

```sql
-- Query the system audit log
SELECT * FROM __audit
WHERE entity_type = 'Patient'
  AND entity_id = 123
ORDER BY timestamp DESC;
```

**Result:**

```text
| timestamp           | user_id | operation | entity_type | entity_id | details         |
|---------------------|---------|-----------|-------------|-----------|-----------------|
| 2024-01-15 10:30:00 | 456     | UPDATE    | Patient     | 123       | name changed    |
| 2024-01-14 09:15:00 | 789     | READ      | Patient     | 123       | viewed record   |
| 2024-01-10 14:20:00 | 456     | CREATE    | Patient     | 123       | initial insert  |
```

## Audit Log Schema

The system audit log includes:

```rust,ignore
struct AuditEntry {
    // What happened
    operation: Operation,      // CREATE, READ, UPDATE, DELETE
    entity_type: String,       // "Patient", "Appointment", etc.
    entity_id: u64,

    // When it happened
    timestamp: Timestamp,
    log_position: Position,

    // Who did it
    user_id: UserId,
    client_id: ClientId,
    session_id: SessionId,
    ip_address: Option<IpAddr>,

    // Why it happened
    reason: Option<String>,    // Optional justification

    // What changed
    before: Option<Value>,     // Previous state
    after: Option<Value>,      // New state

    // Integrity
    previous_hash: Hash,       // Tamper-evident chain
}
```

## Query Patterns

### Who Accessed Patient Data?

```sql
SELECT
  user_id,
  operation,
  timestamp,
  ip_address
FROM __audit
WHERE entity_type = 'Patient'
  AND entity_id = 123
  AND timestamp > NOW() - INTERVAL '30 days'
ORDER BY timestamp DESC;
```

### What Changed Today?

```sql
SELECT
  entity_type,
  entity_id,
  operation,
  user_id,
  timestamp
FROM __audit
WHERE timestamp > CURRENT_DATE
  AND operation IN ('CREATE', 'UPDATE', 'DELETE')
ORDER BY timestamp DESC;
```

### Failed Access Attempts

```sql
SELECT
  user_id,
  entity_type,
  entity_id,
  timestamp,
  error_code
FROM __audit
WHERE operation = 'READ'
  AND success = false
ORDER BY timestamp DESC
LIMIT 100;
```

### Activity by User

```sql
SELECT
  user_id,
  COUNT(*) as operation_count,
  MIN(timestamp) as first_activity,
  MAX(timestamp) as last_activity
FROM __audit
WHERE timestamp > NOW() - INTERVAL '24 hours'
GROUP BY user_id
ORDER BY operation_count DESC;
```

## Programmatic Access

### Rust

```rust,ignore
use kimberlite::Client;

struct AuditLogger {
    client: Client,
    user_id: UserId,
}

impl AuditLogger {
    /// Log an operation with context
    pub fn log_access(
        &self,
        operation: Operation,
        entity_type: &str,
        entity_id: u64,
        reason: Option<&str>,
    ) -> Result<()> {
        self.client.execute(
            "INSERT INTO __audit (operation, entity_type, entity_id, user_id, reason, timestamp)
             VALUES (?, ?, ?, ?, ?, ?)",
            &[
                &operation.to_string(),
                &entity_type,
                &entity_id,
                &self.user_id,
                &reason,
                &Utc::now(),
            ],
        )?;
        Ok(())
    }

    /// Query audit log
    pub fn get_audit_trail(
        &self,
        entity_type: &str,
        entity_id: u64,
    ) -> Result<Vec<AuditEntry>> {
        self.client
            .query(
                "SELECT * FROM __audit
                 WHERE entity_type = ? AND entity_id = ?
                 ORDER BY timestamp DESC",
                &[&entity_type, &entity_id],
            )?
            .map(|row| AuditEntry::from_row(row))
            .collect()
    }
}
```

### Python

```python
from kimberlite import Client
from datetime import datetime
from enum import Enum

class Operation(Enum):
    CREATE = "CREATE"
    READ = "READ"
    UPDATE = "UPDATE"
    DELETE = "DELETE"

class AuditLogger:
    def __init__(self, client: Client, user_id: int):
        self.client = client
        self.user_id = user_id

    def log_access(
        self,
        operation: Operation,
        entity_type: str,
        entity_id: int,
        reason: str = None
    ):
        self.client.execute(
            """INSERT INTO __audit
               (operation, entity_type, entity_id, user_id, reason, timestamp)
               VALUES (?, ?, ?, ?, ?, ?)""",
            [operation.value, entity_type, entity_id, self.user_id, reason, datetime.utcnow()]
        )

    def get_audit_trail(self, entity_type: str, entity_id: int):
        return self.client.query(
            """SELECT * FROM __audit
               WHERE entity_type = ? AND entity_id = ?
               ORDER BY timestamp DESC""",
            [entity_type, entity_id]
        )
```

## Custom Audit Tables

For application-specific auditing:

```sql
-- Custom audit table for sensitive operations
CREATE TABLE hipaa_audit_log (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    action TEXT NOT NULL,
    patient_id BIGINT,
    phi_accessed BOOLEAN,  -- Was PHI accessed?
    justification TEXT,    -- Required for PHI access
    timestamp TIMESTAMP NOT NULL,
    ip_address TEXT,
    user_agent TEXT
);

CREATE INDEX hipaa_audit_patient_idx ON hipaa_audit_log(patient_id);
CREATE INDEX hipaa_audit_timestamp_idx ON hipaa_audit_log(timestamp);
```

**Log PHI access:**

```rust,ignore
fn log_phi_access(
    client: &Client,
    user_id: UserId,
    patient_id: PatientId,
    justification: &str,
) -> Result<()> {
    client.execute(
        "INSERT INTO hipaa_audit_log
         (user_id, action, patient_id, phi_accessed, justification, timestamp, ip_address)
         VALUES (?, 'VIEW_RECORD', ?, true, ?, ?, ?)",
        &[
            &user_id,
            &patient_id,
            &justification,
            &Utc::now(),
            &get_client_ip(),
        ],
    )?;
    Ok(())
}
```

## Compliance Reports

### HIPAA Audit Report

```sql
-- All PHI access in the last year
SELECT
    user_id,
    action,
    patient_id,
    justification,
    timestamp,
    ip_address
FROM hipaa_audit_log
WHERE phi_accessed = true
  AND timestamp > NOW() - INTERVAL '1 year'
ORDER BY timestamp DESC;
```

### Suspicious Activity Report

```sql
-- Users who accessed >100 patient records in 1 hour
SELECT
    user_id,
    COUNT(DISTINCT patient_id) as patients_accessed,
    MIN(timestamp) as window_start,
    MAX(timestamp) as window_end
FROM hipaa_audit_log
WHERE phi_accessed = true
  AND timestamp > NOW() - INTERVAL '1 hour'
GROUP BY user_id
HAVING COUNT(DISTINCT patient_id) > 100;
```

### Access Without Justification

```sql
-- PHI access without proper justification
SELECT * FROM hipaa_audit_log
WHERE phi_accessed = true
  AND (justification IS NULL OR justification = '')
  AND timestamp > NOW() - INTERVAL '30 days';
```

## Alerting on Suspicious Activity

```rust,ignore
use kimberlite::Client;

struct AuditMonitor {
    client: Client,
}

impl AuditMonitor {
    /// Check for suspicious patterns
    pub fn check_suspicious_activity(&self) -> Result<Vec<Alert>> {
        let mut alerts = vec![];

        // Check: User accessed >50 records in 10 minutes
        let mass_access = self.client.query(
            "SELECT user_id, COUNT(*) as count
             FROM __audit
             WHERE operation = 'READ'
               AND timestamp > NOW() - INTERVAL '10 minutes'
             GROUP BY user_id
             HAVING COUNT(*) > 50",
            &[],
        )?;

        for row in mass_access {
            alerts.push(Alert {
                severity: Severity::High,
                message: format!("User {} accessed {} records in 10 minutes", row.user_id, row.count),
            });
        }

        // Check: Access from unusual location
        let unusual_ip = self.client.query(
            "SELECT user_id, ip_address
             FROM __audit
             WHERE ip_address NOT IN (SELECT ip_address FROM user_known_ips WHERE user_id = __audit.user_id)",
            &[],
        )?;

        for row in unusual_ip {
            alerts.push(Alert {
                severity: Severity::Medium,
                message: format!("User {} accessed from unusual IP: {}", row.user_id, row.ip_address),
            });
        }

        Ok(alerts)
    }
}
```

## Tamper Detection

Audit logs are tamper-evident via hash chaining:

```rust,ignore
fn verify_audit_log_integrity(client: &Client) -> Result<bool> {
    let entries = client.query(
        "SELECT * FROM __audit ORDER BY timestamp ASC",
        &[],
    )?;

    let mut prev_hash = Hash::zero();

    for entry in entries {
        // Verify hash chain
        if entry.previous_hash != prev_hash {
            return Ok(false);  // Tampering detected!
        }

        prev_hash = entry.compute_hash();
    }

    Ok(true)
}
```

## Retention Policy

Configure audit log retention:

```rust,ignore
// Keep audit logs for 7 years (HIPAA requirement)
let retention = Duration::from_secs(86400 * 365 * 7);

db.set_audit_retention(retention)?;
```

**Legal hold:** Audit logs under legal hold are never deleted.

## Best Practices

### 1. Always Provide Justification for PHI Access

```rust,ignore
// Bad: No justification
audit.log_access(Operation::Read, "Patient", 123, None)?;

// Good: Clear justification
audit.log_access(
    Operation::Read,
    "Patient",
    123,
    Some("Preparing for scheduled appointment on 2024-01-20"),
)?;
```

### 2. Log All Access, Not Just Modifications

```sql
-- Log reads too (required for HIPAA)
INSERT INTO __audit (operation, entity_type, entity_id, user_id)
VALUES ('READ', 'Patient', 123, current_user_id());
```

### 3. Capture Client Context

```rust,ignore
struct AuditContext {
    user_id: UserId,
    session_id: SessionId,
    ip_address: IpAddr,
    user_agent: String,
    request_id: RequestId,
}
```

### 4. Review Audit Logs Regularly

```bash
# Weekly audit review
kmb audit report --since "7 days ago" --output report.pdf

# Monthly compliance check
kmb audit suspicious --since "30 days ago"
```

### 5. Test Audit Trail Completeness

```rust,ignore
#[test]
fn test_all_operations_logged() {
    let client = setup_test_client();

    // Perform operation
    client.update_patient(123, "Alice Johnson")?;

    // Verify logged
    let audit = client.query(
        "SELECT * FROM __audit WHERE entity_id = 123",
        &[],
    )?;

    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].operation, Operation::Update);
}
```

## Export Audit Logs

Generate compliance reports:

```rust,ignore
// Export audit log for regulators
db.export_audit_log(
    ExportConfig {
        start_date: "2024-01-01",
        end_date: "2024-12-31",
        format: ExportFormat::Pdf,  // or JSON, CSV
        include_hash_chain: true,   // For verification
        digital_signature: true,    // Ed25519 signature
    },
    "audit-2024.pdf",
)?;
```

## Related Documentation

- **[Compliance](/docs/concepts/compliance)** - Compliance architecture
- **[Time-Travel Queries](/docs/coding/recipes/time-travel-queries)** - Historical data access
- **[Multi-Tenant Queries](/docs/coding/recipes/multi-tenant-queries)** - Tenant isolation

---

**Key Takeaway:** Kimberlite provides audit trails by default. Every operation is logged, tamper-evident, and queryable. Build on this foundation for compliance-specific requirements.
