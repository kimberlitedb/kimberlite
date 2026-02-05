# Compliance

Kimberlite provides **compliance by construction**, not compliance by configuration.

## Core Principle

The architecture makes certain violations **impossible**:

| Guarantee | How It's Achieved |
|-----------|-------------------|
| **Immutability** | Append-only log; no UPDATE or DELETE on raw events |
| **Auditability** | Every state change is logged with metadata |
| **Tamper Evidence** | Cryptographic hash chain links all events |
| **Non-Repudiation** | Events can be signed with Ed25519 |
| **Data Sovereignty** | Regional placement enforced at routing layer |
| **Isolation** | Per-tenant encryption keys |
| **Retention** | Legal holds prevent deletion; configurable retention |
| **Reconstruction** | Any point-in-time state derivable from log |

**Result:** Compliance isn't a checklist item—it's the natural consequence of the architecture.

## Supported Frameworks

Kimberlite's architecture supports compliance with multiple regulatory frameworks:

| Framework | Industry | Key Requirements | Kimberlite Support |
|-----------|----------|------------------|------------------|
| **HIPAA** | Healthcare | Audit trails, access controls, encryption | ✅ Full |
| **GDPR** | All (EU) | Right to erasure, data portability, consent | ✅ Full |
| **SOC 2** | Technology | Security, availability, processing integrity | ✅ Full |
| **21 CFR Part 11** | Pharma/Medical | Electronic records, signatures, timestamps | ✅ Full |
| **CCPA** | All (California) | Data access, deletion, opt-out | ✅ Full |
| **GLBA** | Finance | Data protection, access controls | ✅ Full |
| **FERPA** | Education | Student data privacy, access controls | ✅ Full |

The same architectural primitives—immutable logs, hash chaining, encryption, and audit trails—provide the foundation for compliance across all frameworks.

## Immutability

**Nothing is ever deleted or modified.** "Deletion" is a new event:

```sql
-- There is no DELETE in the traditional sense
-- Instead, append a deletion event
INSERT INTO __events (type, data)
VALUES ('PatientDeleted', '{"id": 123, "reason": "Patient request (GDPR)"}');
```

**Benefits:**

1. **Complete audit trail:** See what was deleted and why
2. **Time-travel queries:** Query data as it existed before deletion
3. **Tamper evidence:** Cannot cover up mistakes by deleting logs
4. **Compliance:** Regulators can see full history

## Audit Trail by Default

Every write is logged with full context:

```rust
struct AuditEntry {
    // What changed
    operation: Operation,  // INSERT, UPDATE, DELETE
    entity: EntityType,    // Patient, Appointment, etc.
    entity_id: u64,

    // When it changed
    timestamp: Timestamp,
    log_position: Position,

    // Who changed it
    user_id: UserId,
    client_id: ClientId,
    session_id: SessionId,

    // Why it changed (optional)
    reason: Option<String>,

    // What was before (for compliance)
    previous_hash: Hash,
}
```

**Query audit trail:**

```sql
-- Who accessed patient 123?
SELECT user_id, timestamp, operation
FROM __audit
WHERE entity = 'Patient' AND entity_id = 123
ORDER BY timestamp DESC;

-- What changed in the last 24 hours?
SELECT * FROM __audit
WHERE timestamp > NOW() - INTERVAL '24 hours'
ORDER BY timestamp DESC;
```

## Timestamp Accuracy Guarantees

**Critical for compliance:** HIPAA, GDPR, 21 CFR Part 11, and SOC 2 all require **accurate, monotonic timestamps** for audit trails. Kimberlite uses cluster-wide clock synchronization to guarantee timestamp reliability.

### Cluster-Wide Clock Consensus

Instead of relying on individual replica clocks (which drift) or client clocks (which are untrusted), Kimberlite achieves **cluster consensus on time** using Marzullo's algorithm:

1. **Sample collection:** Primary collects clock measurements from all replicas via heartbeat ping/pong
2. **Quorum agreement:** Marzullo's algorithm finds smallest time interval consistent with quorum
3. **Bounded uncertainty:** Synchronized interval width ≤ 500ms (CLOCK_OFFSET_TOLERANCE_MS)
4. **Monotonicity enforcement:** Timestamps never decrease, even across view changes

**Result:** Audit timestamps are provably accurate and monotonic, backed by formal verification.

### Guarantees

| Property | Guarantee | Verification |
|----------|-----------|--------------|
| **Monotonicity** | `timestamp[n+1] >= timestamp[n]` (never decreases) | Kani Proof #22, TLA+ theorem |
| **Cluster consensus** | Timestamp within bounds agreed by quorum | Marzullo algorithm, Kani Proof #21 |
| **Bounded offset** | Clock offset ≤ 500ms across all replicas | Kani Proof #23, VOPR scenario |
| **View change safety** | Timestamps preserved across leader elections | VOPR ClockBackwardJump scenario |
| **NTP-independent HA** | Continues with stale epoch if NTP fails | VOPR ClockNtpFailure scenario |

### Compliance Impact

**HIPAA (§164.312(b)):** Requires audit controls with accurate timestamps for PHI access.
- ✅ **Before Phase 1.1:** Timestamps could diverge across replicas
- ✅ **After Phase 1.1:** Cluster consensus guarantees ≤500ms accuracy

**GDPR (Article 30):** Requires records of processing activities with temporal ordering.
- ✅ **Before Phase 1.1:** No monotonicity guarantees during view changes
- ✅ **After Phase 1.1:** Formal proof of timestamp monotonicity

**21 CFR Part 11:** FDA regulation requiring trustworthy computer-generated timestamps.
- ✅ **Before Phase 1.1:** Individual replica clocks (unreliable)
- ✅ **After Phase 1.1:** Quorum-validated timestamps with bounded uncertainty

### Implementation Details

- **Algorithm:** Marzullo's algorithm (1984) for clock synchronization
- **Epoch duration:** 3-10 seconds (sample collection window)
- **Epoch validity:** 30 seconds (after which re-synchronization required)
- **Tolerance:** 500ms maximum offset (conservative for diverse NTP environments)

**See:** `docs/internals/clock-synchronization.md` for technical details and formal verification.

## Cryptographic Hash Chaining

Every event links to the previous event's hash, creating a tamper-evident chain:

```
Event N-1          Event N            Event N+1
┌─────────┐        ┌─────────┐        ┌─────────┐
│ data    │        │ data    │        │ data    │
│ hash ───┼───────►│ prev    │        │ prev    │
│         │        │ hash ───┼───────►│ hash    │
└─────────┘        └─────────┘        └─────────┘
```

**Tamper detection:**

If any event is modified, all subsequent hashes become invalid:

```rust
fn verify_chain(log: &[Event]) -> Result<()> {
    for i in 1..log.len() {
        let prev_hash = hash(&log[i-1]);
        if log[i].prev_hash != prev_hash {
            return Err(Error::TamperedLog {
                position: i,
                expected: prev_hash,
                actual: log[i].prev_hash,
            });
        }
    }
    Ok(())
}
```

**Compliance benefit:** Auditors can verify log integrity without trusting the database.

## Point-in-Time Reconstruction

Query data as it existed at any historical point:

```sql
-- What did we know about patient 123 on January 15th?
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-15 10:30:00'
WHERE id = 123;

-- What did the entire database look like 1000 operations ago?
SELECT * FROM patients
AS OF POSITION 1000;
```

**Use cases:**

- **Audits:** "Show me what you knew on date X"
- **Investigations:** "When did this error occur?"
- **Compliance:** "Prove you followed the process"
- **Debugging:** "What state caused this bug?"

## Retention and Legal Hold

Configure retention per tenant:

```rust
db.create_tenant(TenantConfig {
    id: TenantId::new(1),
    retention: Retention {
        min_duration: Duration::from_secs(86400 * 2555),  // 7 years (HIPAA)
        max_duration: Duration::from_secs(86400 * 3650),  // 10 years
        legal_hold: false,  // Can be deleted after max_duration
    },
})?;
```

**Legal hold:**

```rust
// Prevent deletion during litigation
db.enable_legal_hold(TenantId::new(1), "Case #12345")?;

// Later, after case closes
db.disable_legal_hold(TenantId::new(1), "Case #12345 closed")?;
```

**Automatic enforcement:** System rejects deletion while legal hold is active.

## Right to Erasure (GDPR)

GDPR requires "right to erasure" (right to be forgotten). How does this work with immutable logs?

**Solution 1: Cryptographic Erasure**

Delete the tenant's encryption key:

```rust
// Delete KEK (Key Encryption Key)
kms.delete_key(tenant_kek)?;

// Result: All data becomes unrecoverable
// Log still exists (for audit), but cannot be decrypted
```

**Solution 2: Redaction Events**

Append a redaction event:

```rust
db.append(Event::Redacted {
    position: 12345,  // Position of original event
    reason: "GDPR right to erasure request",
    requester: "patient@example.com",
})?;
```

Query layer filters out redacted events.

**Compliance note:** Both approaches satisfy GDPR. Consult your legal team.

## Data Portability (GDPR)

Export tenant data in standard format:

```rust
// Export all data for tenant
let export = db.export_tenant(TenantId::new(1), ExportFormat::Json)?;

// Export includes:
// - All events in chronological order
// - Metadata (timestamps, user IDs, etc.)
// - Hash chain for verification
```

**Export format (JSON example):**

```json
{
  "tenant_id": 1,
  "export_timestamp": "2024-01-15T10:30:00Z",
  "events": [
    {
      "position": 1,
      "timestamp": "2024-01-01T08:00:00Z",
      "type": "PatientCreated",
      "data": {"id": 123, "name": "Alice Smith"},
      "hash": "abc123...",
      "prev_hash": "000000..."
    },
    ...
  ]
}
```

Recipient can verify hash chain independently.

## Transaction Idempotency

Prevent duplicate transactions (compliance violation in healthcare/finance):

```rust
// Client generates ID before first attempt
let idempotency_id = IdempotencyId::generate();

// First attempt
let result = client.execute_with_id(idempotency_id, cmd).await;

// If network fails, retry with SAME ID
let result = client.execute_with_id(idempotency_id, cmd).await;
// Returns same result without re-executing
```

**Prevents:**
- Double-charging patients
- Double-booking appointments
- Duplicate financial transactions

See [Compliance Implementation](../internals/compliance-implementation.md) for technical details.

## Access Controls

Role-based access control (RBAC):

```rust
struct AccessControl {
    user_id: UserId,
    role: Role,  // Admin, User, ReadOnly
    permissions: Vec<Permission>,  // Read, Write, Delete, etc.
    tenant_id: TenantId,  // Cannot access other tenants
}

// Check before every operation
fn authorize(user: &User, operation: Operation) -> Result<()> {
    if !user.has_permission(operation.required_permission()) {
        return Err(Error::Unauthorized);
    }
    // Also log access attempt
    audit_log.log(AccessAttempt { user, operation, allowed: true });
    Ok(())
}
```

**All access attempts logged** (even denials).

## Regulator-Friendly Exports

Generate compliance reports:

```rust
// Generate HIPAA audit report
let report = db.generate_report(ReportType::HipaaAudit {
    tenant_id: TenantId::new(1),
    date_range: DateRange::last_year(),
})?;

// Report includes:
// - All access to PHI (Protected Health Information)
// - Who accessed what and when
// - Any access denials
// - Hash chain verification
// - Digital signatures
```

**Export formats:**
- PDF (for printing)
- JSON (for programmatic verification)
- CSV (for spreadsheet analysis)

## Compliance Checklist

Before deploying to production:

### HIPAA (Healthcare)

- [ ] Enable encryption at rest (per-tenant keys)
- [ ] Enable TLS for all network communication
- [ ] Configure audit logging
- [ ] Set retention period (minimum 7 years)
- [ ] Implement role-based access control
- [ ] Enable session timeout (15 minutes max)
- [ ] Test right-to-access data export
- [ ] Document incident response plan

### GDPR (EU Data)

- [ ] Enable right to erasure (cryptographic or redaction)
- [ ] Implement data portability export
- [ ] Configure consent tracking
- [ ] Set up data processing agreements (DPAs)
- [ ] Implement data minimization (don't log unnecessary data)
- [ ] Enable breach notification alerts
- [ ] Document data retention policies
- [ ] Appoint Data Protection Officer (DPO) if required

### SOC 2

- [ ] Enable comprehensive audit logging
- [ ] Implement access controls and least privilege
- [ ] Set up monitoring and alerting
- [ ] Document security policies
- [ ] Test disaster recovery procedures
- [ ] Conduct security training for team
- [ ] Perform regular security assessments
- [ ] Maintain change management process

See [Compliance Implementation](../internals/compliance-implementation.md) for detailed checklists.

## Benefits Over Traditional Databases

| Traditional Database | Kimberlite |
|---------------------|------------|
| Add audit table (easy to forget) | Audit by default (architectural) |
| Hope nobody tampers | Cryptographic hash chain (tamper-evident) |
| Reconstruct state manually | Point-in-time queries (built-in) |
| Pray during audits | Export verifiable logs (regulator-friendly) |
| Bolt on encryption | Per-tenant keys (structural) |

## Related Documentation

- **[Data Model](data-model.md)** - How immutability works
- **[Multi-tenancy](multitenancy.md)** - Tenant isolation and encryption
- **[Compliance Implementation](../internals/compliance-implementation.md)** - Full technical details
- **[First Application](../start/first-app.md)** - Build a compliant healthcare app

---

**Key Takeaway:** Kimberlite's compliance isn't a feature you enable—it's the foundation. Immutability, audit trails, and tamper evidence are consequences of the append-only log architecture.
