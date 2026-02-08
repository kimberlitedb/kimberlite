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

Kimberlite provides **formal verification for 23 compliance frameworks at 100%**. Each framework has a complete TLA+ specification with mechanized TLAPS proofs, demonstrating that the core architecture satisfies all requirements.

**Status:** All 23 frameworks formally verified and complete (as of v0.4.3, Feb 2026).

### USA Frameworks (12)

| Framework | Vertical | Key Requirements | TLA+ Spec | Proofs |
|-----------|----------|------------------|-----------|--------|
| **HIPAA** | Healthcare | PHI protection, breach notification, access controls | `HIPAA.tla` | 9 complete |
| **HITECH** | Healthcare | Minimum necessary access, business associate liability, 60-day breach notification | `HITECH.tla` | 4 complete |
| **21 CFR Part 11** | Pharma/Medical | Electronic signatures, operational sequencing, closed system controls | `CFR_Part_11.tla` | 6 complete |
| **SOC 2** | Technology | Trust Services Criteria, backup/recovery, SLA metrics | `SOC2.tla` | 5 complete |
| **PCI DSS** | Finance/Retail | Cardholder data protection, tokenization, transmission encryption | `PCI_DSS.tla` | 5 complete |
| **GLBA** | Finance | Safeguards Rule, privacy notice, 30-day FTC breach notification | `GLBA.tla` | 4 complete |
| **SOX** | Finance | 7-year retention, audit integrity, sections 302/404 | `SOX.tla` | 3 complete |
| **CCPA/CPRA** | California | Right to know/delete/correct/opt-out/limit, data correction | `CCPA.tla` | 5 complete |
| **FERPA** | Education | Student record privacy, consent for disclosure, access rights | `FERPA.tla` | 3 complete |
| **FedRAMP** | Government/Cloud | NIST 800-53 controls, CM-2/CM-6, 72h incident notification | `FedRAMP.tla` | 7 complete |
| **NIST 800-53** | Government | Control families AC/AU/CM/SC/SI, extends FedRAMP | `NIST_800_53.tla` | 2 complete |
| **CMMC** | Defense | NIST 800-171 derivative, 3-level maturity model | `CMMC.tla` | 3 complete |

### EU Frameworks (4)

| Framework | Vertical | Key Requirements | TLA+ Spec | Proofs |
|-----------|----------|------------------|-----------|--------|
| **GDPR** | All | Right to erasure, data portability, consent, 72h breach notification | `GDPR.tla` | 6 complete |
| **eIDAS** | Digital Identity | Qualified timestamps, electronic signatures/seals (RFC 3161) | `eIDAS.tla` | 4 complete |
| **NIS2** | Critical Infrastructure | Article 21 security, 24h early warning, 72h incident reporting | `NIS2.tla` | 3 complete |
| **DORA** | Finance | ICT risk management, resilience testing (VOPR), incident reporting | `DORA.tla` | 3 complete |

### Australia Frameworks (5)

| Framework | Vertical | Key Requirements | TLA+ Spec | Proofs |
|-----------|----------|------------------|-----------|--------|
| **Privacy Act/APPs** | All | 13 Australian Privacy Principles, security, access, correction | `AustralianPrivacyAct.tla` | 3 complete |
| **APRA CPS 234** | Finance/Insurance | Information security capability, 72h incident notification | `APRA_CPS_234.tla` | 2 complete |
| **Essential Eight** | Government | Restrict admin privileges, regular backups (ASD maturity model) | `EssentialEight.tla` | 2 complete |
| **NDB Scheme** | All | 30-day assessment, notification to individuals and OAIC | `NDB_Scheme.tla` | 2 complete |
| **IRAP** | Government | ISM controls (encryption, access, audit, classification) | `IRAP.tla` | 4 complete |

### International & Cross-Region (2)

| Framework | Vertical | Key Requirements | TLA+ Spec | Proofs |
|-----------|----------|------------------|-----------|--------|
| **ISO 27001** | All | Annex A controls, access control, logging, cryptography, continuity | `ISO27001.tla` | 8 complete |
| **Legal Compliance** | Legal | Legal hold, chain of custody, eDiscovery, attorney-client privilege | `LegalCompliance.tla` | 4 complete |

**Total Coverage:** **23 frameworks** at **100%** formal verification — **92 TLAPS proofs** total

**Core Properties (9):** TenantIsolation, AuditCompleteness, EncryptionAtRest, AccessControlEnforcement, AuditLogImmutability, HashChainIntegrity, ConsentManagement, ElectronicSignatureBinding, QualifiedTimestamping

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

## Right to Erasure (GDPR Article 17)

Kimberlite provides a **complete erasure engine** implementing GDPR Article 17 with full audit trail preservation:

- **Automated erasure workflow** — request, execute, verify, audit
- **30-day deadline enforcement** — overdue detection with automated alerts
- **Cascade deletion** — erasure across all streams containing subject data
- **Exemption mechanism** — legal holds and public interest exceptions (Article 17(3))
- **Tombstone design** — records marked inaccessible while preserving log integrity
- **Cryptographic erasure proof** — SHA-256 hash of erased record IDs

```rust
use kimberlite_compliance::erasure::ErasureEngine;

let mut engine = ErasureEngine::new();

// Request erasure (30-day deadline set automatically)
let request = engine.request_erasure("patient@hospital.com")?;

// Execute across affected streams
engine.mark_in_progress(request.request_id, vec![stream_1, stream_5])?;
engine.mark_stream_erased(request.request_id, stream_1, 42)?;
engine.mark_stream_erased(request.request_id, stream_5, 18)?;

// Complete with cryptographic proof
engine.complete_erasure(request.request_id, erasure_proof)?;

// Or exempt from erasure (legal hold)
engine.exempt_from_erasure(request.request_id, ExemptionBasis::LegalClaims)?;
```

**Consent withdrawal integration:** When `withdraw_consent()` is called and no remaining valid consents exist, an erasure request is automatically triggered.

**See:** [Right to Erasure](right-to-erasure.md) for complete documentation.

## Data Portability (GDPR Article 20)

Kimberlite provides **GDPR-compliant data portability exports** with cryptographic integrity:

- **Machine-readable formats** — JSON and CSV (Article 20(1))
- **SHA-256 content hashing** — integrity verification for every export
- **HMAC-SHA256 signing** — authenticity proof with constant-time verification
- **Immutable audit trail** — every export operation logged
- **Cross-stream aggregation** — collect subject data from all streams automatically

```rust
use kimberlite_compliance::export::{ExportEngine, ExportFormat};

let mut engine = ExportEngine::new();

// Export subject's data as JSON
let export = engine.export_subject_data(
    "patient@hospital.com",
    &records,
    ExportFormat::Json,
)?;

// Sign for authenticity
engine.sign_export(export.export_id, signing_key)?;

// Verify signature (constant-time comparison)
let valid = ExportEngine::verify_export_signature(&export, &data, signing_key)?;
```

**See:** [Data Portability](data-portability.md) for complete documentation.

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

## Role-Based Access Control (RBAC)

Kimberlite provides **fine-grained RBAC** with formal verification guarantees:

### 4 Roles with Escalating Privileges

| Role | Read | Write | Delete | Export | Cross-Tenant | Audit Logs |
|------|------|-------|--------|--------|--------------|------------|
| **Auditor** | ✓ | ✗ | ✗ | ✗ | ✗ | ✓ |
| **User** | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ |
| **Analyst** | ✓ | ✗ | ✗ | ✓ | ✓ | ✗ |
| **Admin** | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

### Field-Level Security (Column Filtering)

Hide sensitive columns from unauthorized users:

```rust
use kimberlite_rbac::policy::AccessPolicy;
use kimberlite_rbac::roles::Role;

let policy = AccessPolicy::new(Role::Analyst)
    .allow_stream("users")
    .allow_column("*")        // Allow all columns
    .deny_column("ssn")       // Except SSN
    .deny_column("password"); // And password

// Query: SELECT name, email, ssn FROM users
// Rewritten: SELECT name, email FROM users
```

### Row-Level Security (RLS)

Automatic tenant isolation for User role:

```rust
use kimberlite_rbac::policy::StandardPolicies;
use kimberlite_types::TenantId;

let policy = StandardPolicies::user(TenantId::new(42));

// Query: SELECT * FROM users
// Rewritten: SELECT * FROM users WHERE tenant_id = 42
```

### Formal Verification

All RBAC properties are **formally verified**:
- **TLA+ Specification** (`specs/tla/compliance/RBAC.tla`) - 3 theorems proven
- **Kani Bounded Model Checking** - 8 proofs (role separation, column filtering, etc.)
- **VOPR Simulation Testing** - 4 scenarios with 50K+ iterations

### Compliance Mappings

RBAC supports multi-framework compliance:
- **HIPAA § 164.312(a)(1)**: Technical access controls
- **GDPR Article 32(1)(b)**: Access controls and confidentiality
- **SOC 2 CC6.1**: Logical access controls
- **PCI DSS Requirement 7**: Restrict access to cardholder data
- **ISO 27001 A.5.15**: Access control policy
- **FedRAMP AC-3**: Access enforcement

**All access attempts logged** (even denials).

**See:** [RBAC Concepts](rbac.md) for complete documentation.

## Consent and Purpose Tracking (GDPR Articles 6 & 7)

Kimberlite provides **automatic consent tracking** for GDPR compliance:

### GDPR Requirements

**Article 6**: Processing must have lawful basis (consent, contract, legal obligation, etc.)
**Article 7**: Consent must be freely given, specific, informed, unambiguous, and withdrawable

### 8 Purposes with Automatic Validation

| Purpose | Lawful Basis | Requires Consent | Valid for PHI | Valid for PCI |
|---------|--------------|------------------|---------------|---------------|
| **Marketing** | Article 6(1)(a) | ✅ Yes | ❌ No | ❌ No |
| **Analytics** | Article 6(1)(f) | ❌ No | ❌ No | ❌ No |
| **Contractual** | Article 6(1)(b) | ❌ No | ✅ Yes | ✅ Yes |
| **LegalObligation** | Article 6(1)(c) | ❌ No | ✅ Yes | ✅ Yes |
| **VitalInterests** | Article 6(1)(d) | ❌ No | ✅ Yes | ✅ Yes |
| **PublicTask** | Article 6(1)(e) | ❌ No | ✅ Yes | ❌ No |
| **Research** | Article 9(2)(j) | ✅ Yes | ✅ Yes | ❌ No |
| **Security** | Article 6(1)(f) | ❌ No | ✅ Yes | ✅ Yes |

### Consent Lifecycle

```rust
use kimberlite_compliance::validator::ConsentValidator;
use kimberlite_compliance::purpose::Purpose;
use kimberlite_compliance::classification::DataClass;

let mut validator = ConsentValidator::new();

// Grant consent
let consent_id = validator.grant_consent("user@example.com", Purpose::Marketing).unwrap();

// Validate before processing
validator.validate_query(
    "user@example.com",
    Purpose::Marketing,
    DataClass::PII,
).unwrap();

// Withdraw consent (Article 7(3) - as easy as granting)
validator.withdraw_consent(consent_id).unwrap();
```

### Purpose Limitation (Article 5(1)(b))

Automatic validation prevents invalid purpose/data class combinations:

```rust
// ✓ Valid: Marketing with consent for PII
validate_purpose(DataClass::PII, Purpose::Marketing)?;

// ✗ Invalid: Marketing not allowed for PHI (HIPAA violation)
validate_purpose(DataClass::PHI, Purpose::Marketing)?; // Error

// ✗ Invalid: Analytics not allowed for PCI (PCI DSS violation)
validate_purpose(DataClass::PCI, Purpose::Analytics)?; // Error
```

### Formal Verification

- **TLA+ Specification** (`specs/tla/compliance/GDPR.tla`) - Updated with Article 6 & 7 properties
- **Kani Proofs** - 5 proofs (#41-45) verifying consent correctness
  - Proof #41: Consent grant/withdraw correctness
  - Proof #42: Purpose validation for data classes
  - Proof #43: Consent validator enforcement
  - Proof #44: Consent expiry handling
  - Proof #45: Multiple consents per subject

### Compliance Impact

- **GDPR Article 6**: ✅ Full support for lawful basis
- **GDPR Article 7**: ✅ Full support for consent conditions
- **GDPR Article 5(1)(b)**: ✅ Purpose limitation enforced
- **GDPR Article 5(1)(c)**: ✅ Data minimization validated

**See:** [Consent Management](consent-management.md) for complete documentation.

## Field-Level Data Masking (HIPAA § 164.312(a)(1))

Kimberlite provides **field-level data masking** to enforce the "minimum necessary" principle:

- **5 masking strategies** — Redact, Hash, Tokenize, Truncate, Null
- **Role-based application** — different roles see different views of the same data
- **Admin exemption** — privileged users see raw data when necessary
- **Deterministic output** — Hash and Tokenize preserve referential integrity for JOINs

| Strategy | Example Input | Example Output | Use Case |
|----------|---------------|----------------|----------|
| **Redact** (SSN) | `123-45-6789` | `***-**-6789` | Partial verification |
| **Hash** (SHA-256) | `alice@example.com` | `2c740c48e7f0...` | JOIN preservation |
| **Tokenize** (BLAKE3) | `4111-1111-1111-1234` | `tok_a1b2c3d4e5f6` | Reversible by Admin |
| **Truncate** | `John Smith` | `John...` | Partial visibility |
| **Null** | *(any)* | *(empty)* | Complete hiding |

Masking is applied as a **post-processing step** after RBAC column filtering and query execution, providing defense in depth.

**See:** [Field-Level Masking](field-masking.md) for complete documentation.

## Breach Detection and Notification (HIPAA § 164.404, GDPR Article 33)

Kimberlite provides **automated breach detection** with 72-hour notification deadline tracking:

- **6 breach indicators** — mass export, unauthorized access, privilege escalation, anomalous volume, unusual time, data exfiltration
- **Severity classification** — Low, Medium, High, Critical based on data classes affected
- **72-hour notification deadline** — per HIPAA § 164.404 and GDPR Article 33
- **Breach lifecycle management** — Detected → Under Investigation → Confirmed → Resolved (or False Positive)
- **Configurable thresholds** — per deployment environment

```rust
use kimberlite_compliance::breach::{BreachDetector, BreachThresholds};

let mut detector = BreachDetector::new();

// Check for mass data export breach
if let Some(event) = detector.check_mass_export(5000, &[DataClass::PHI]) {
    // event.severity = Critical (PHI data)
    // event.notification_deadline = now + 72h
    detector.escalate(event.event_id)?;
}

// Check for overdue notification deadlines
let overdue = detector.check_notification_deadlines(Utc::now());
```

**See:** [Breach Notification](breach-notification.md) for complete documentation.

## Enhanced Audit Logging (SOC 2 CC7.2, ISO 27001 A.12.4.1)

Kimberlite provides **comprehensive audit logging** with 13 action types across all compliance modules:

- **Immutable append-only log** — audit records cannot be modified after creation
- **15 action types** — covering consent, erasure, breach, export, access, masking, ABAC, tokenization, signatures
- **Filterable query API** — search by subject, action type, time range, severity
- **Location-aware auditing** — optional `source_country` field for FedRAMP location-based audit trails
- **Auditor export** — structured reports for compliance verification

| Action Type | Module | Description |
|-------------|--------|-------------|
| `ConsentGranted` | Consent | Subject granted consent for purpose |
| `ConsentWithdrawn` | Consent | Subject withdrew consent |
| `ErasureRequested` | Erasure | Erasure request filed |
| `ErasureCompleted` | Erasure | Erasure executed with proof |
| `BreachDetected` | Breach | Breach indicator triggered |
| `BreachNotified` | Breach | Notification sent within deadline |
| `DataExported` | Export | Subject data exported |
| `AccessGranted` | RBAC | Access decision: allowed |
| `AccessDenied` | RBAC | Access decision: denied |
| `FieldMasked` | Masking | Field masked for role |
| `PolicyEvaluated` | ABAC | ABAC policy decision |
| `RoleAssigned` | RBAC | Role assigned to user |
| `PolicyChanged` | ABAC | Policy configuration changed |
| `TokenizationApplied` | PCI DSS | Column tokenized for PAN protection |
| `RecordSigned` | 21 CFR Part 11 | Electronic record signed (Ed25519) |

## Attribute-Based Access Control (ABAC)

Kimberlite provides **context-aware access control** that extends RBAC with dynamic, attribute-based decisions:

- **18 condition types** — user, resource, environment, and compliance-specific attributes
- **19 pre-built compliance policies** — one per framework covering USA, EU, and Australia
- **Two-layer enforcement** — RBAC (coarse-grained) then ABAC (fine-grained)
- **Priority-based evaluation** — highest priority rule wins, deterministic decisions

| Policy | Key Rule | Compliance Driver |
|--------|----------|-------------------|
| **HIPAA** | PHI access only during business hours with clearance >= 2 | § 164.312(a)(1) |
| **FedRAMP** | Deny all access from outside the US | AC-3 |
| **PCI DSS** | PCI data only from Server devices with clearance >= 2 | Requirement 7 |
| **SOX** | Financial data + 7-year retention enforcement | Sections 302/404 |
| **CCPA** | Data correction allowed, PII clearance >= 1 | § 1798.106 |
| **21 CFR Part 11** | Operational sequencing: Author → Review → Approve | § 11.10(e) |
| **Legal** | Legal hold prevents deletion during litigation | FRCP 37(e) |
| **NIS2** | EU-only access, 24h incident reporting deadline | Article 23 |

```rust
use kimberlite_abac::evaluator;
use kimberlite_abac::policy::AbacPolicy;

let policy = AbacPolicy::hipaa_policy();
let decision = evaluator::evaluate(&policy, &user, &resource, &env);
// decision.effect = Allow or Deny
// decision.matched_rule = Some("hipaa-phi-access")
```

**See:** [Attribute-Based Access Control](abac.md) for complete documentation.

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

## Security Audit Status

**Current Status:** Ready for third-party compliance certification (v0.9.2, Feb 2026)

Kimberlite has undergone two independent security audits:

- **AUDIT-2026-01** (January 2026, v0.9.0) — 14 findings remediated in v0.9.1
- **AUDIT-2026-02** (February 2026, v0.9.1) — 4 findings remediated in v0.9.2

**v0.9.2 Remediation Summary:**

| Finding | Severity | Status | Impact |
|---------|----------|--------|--------|
| N-1: Ed25519 signature malleability | HIGH | ✅ Resolved | Strict verification per RFC 8032 prevents authentication bypass |
| N-2: 7 debug assertions stripped | MEDIUM | ✅ Resolved | Crypto invariants now enforced in all build modes |
| N-3: Consent defaults to Disabled | MEDIUM | ✅ Resolved | Privacy-by-default per GDPR Article 25 |
| N-4: No property test coverage | LOW | ✅ Resolved | 18 property tests (4,608 generated test cases) |

**Updated Compliance Status:**

| Regulation | Article/Section | Before v0.9.2 | After v0.9.2 |
|------------|-----------------|---------------|--------------|
| **GDPR** | Article 6 (Lawful basis) | PARTIAL | **GOOD** ✅ |
| **GDPR** | Article 25 (Privacy by default) | PARTIAL | **GOOD** ✅ |
| **HIPAA** | §164.312(d) (Authentication) | PARTIAL | **GOOD** ✅ |

**Overall Risk:** LOW → **VERY LOW** (0 High, 3 Medium, 3 Low remaining)

**Audit Reports:**
- Full audit reports: `docs-internal/audit/AUDIT-2026-01.md`, `AUDIT-2026-02.md`
- Remediation details: `docs-internal/audit/REMEDIATION-2026-02.md`

---

## Related Documentation

- **[Data Model](data-model.md)** - How immutability works
- **[Multi-tenancy](multitenancy.md)** - Tenant isolation and encryption
- **[RBAC](rbac.md)** - Role-based access control with SQL rewriting
- **[ABAC](abac.md)** - Attribute-based access control (context-aware)
- **[Field-Level Masking](field-masking.md)** - 5 masking strategies for data minimization
- **[Consent Management](consent-management.md)** - GDPR Articles 6 & 7 consent tracking
- **[Right to Erasure](right-to-erasure.md)** - GDPR Article 17 data deletion
- **[Breach Notification](breach-notification.md)** - Automated breach detection and 72h deadlines
- **[Data Portability](data-portability.md)** - GDPR Article 20 data export
- **[Data Classification](data-classification.md)** - 8-level classification system
- **[Compliance Implementation](../internals/compliance-implementation.md)** - Full technical details
- **[First Application](../start/first-app.md)** - Build a compliant healthcare app

---

**Key Takeaway:** Kimberlite's compliance isn't a feature you enable—it's the foundation. Immutability, audit trails, and tamper evidence are consequences of the append-only log architecture.
