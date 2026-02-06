# Right to Erasure (GDPR Article 17)

Kimberlite provides **GDPR-compliant data erasure** with full audit trail preservation:

- **Automated erasure workflow** — request, execute, verify, audit
- **30-day deadline enforcement** — GDPR Article 17 compliance
- **Cascade deletion** — erasure across all streams containing subject data
- **Exemption mechanism** — legal holds and public interest exceptions (Article 17(3))
- **Immutable audit trail** — cryptographic proof of erasure for compliance

---

## Why Right to Erasure Matters

GDPR Article 17 gives data subjects the right to request deletion of their personal data. Failure to comply can result in fines of **€20M or 4% of global revenue**.

**Challenges with immutable logs:**

Kimberlite uses an append-only log — data is never physically overwritten. Erasure is implemented via **tombstoning**: records are marked as erased and become inaccessible, while the log structure remains intact for audit purposes.

**Key requirements:**

| Requirement | GDPR Article | Kimberlite Support |
|-------------|-------------|-------------------|
| Delete on request | Article 17(1) | ✅ `ErasureEngine::request_erasure()` |
| Complete within 30 days | Article 17(1) | ✅ Deadline tracking with overdue alerts |
| Cascade to all copies | Article 17(2) | ✅ Cross-stream cascade deletion |
| Exempt for legal holds | Article 17(3)(e) | ✅ `ExemptionBasis::LegalClaims` |
| Exempt for public health | Article 17(3)(c) | ✅ `ExemptionBasis::PublicHealth` |
| Prove deletion occurred | Accountability (Art 5(2)) | ✅ Cryptographic erasure proof |

---

## Erasure Workflow

```
┌──────────────┐    ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│   Request     │───►│  In Progress  │───►│   Complete    │───►│  Audit Record │
│   (Pending)   │    │  (Executing)  │    │  (Verified)   │    │  (Immutable)  │
└──────────────┘    └──────────────┘    └──────────────┘    └──────────────┘
       │                                                            ▲
       │                                                            │
       ▼                                                    Cryptographic
┌──────────────┐                                            erasure proof
│   Exempt      │                                           (SHA-256 hash
│   (Art 17(3)) │                                            of erased IDs)
└──────────────┘
```

### Step 1: Request Erasure

A data subject requests deletion. A 30-day deadline is automatically set.

```rust
use kimberlite_compliance::erasure::ErasureEngine;

let mut engine = ErasureEngine::new();
let request = engine.request_erasure("patient@hospital.com")?;

// request.deadline = now + 30 days (GDPR requirement)
// request.status = ErasureStatus::Pending
```

### Step 2: Identify Affected Streams

Mark which streams contain the subject's data.

```rust
use kimberlite_types::StreamId;

engine.mark_in_progress(
    request.request_id,
    vec![StreamId::new(1), StreamId::new(5), StreamId::new(12)],
)?;
```

### Step 3: Execute Erasure Per Stream

Delete (tombstone) the subject's records in each stream.

```rust
// Erase from stream 1: 42 records
engine.mark_stream_erased(request.request_id, StreamId::new(1), 42)?;

// Erase from stream 5: 18 records
engine.mark_stream_erased(request.request_id, StreamId::new(5), 18)?;

// Erase from stream 12: 7 records
engine.mark_stream_erased(request.request_id, StreamId::new(12), 7)?;
```

### Step 4: Complete with Cryptographic Proof

Finalize the erasure with a SHA-256 hash of erased record IDs.

```rust
use kimberlite_crypto::Hash;

let erasure_proof = Hash::from_bytes(&sha256_of_erased_record_ids);
let audit_record = engine.complete_erasure(request.request_id, erasure_proof)?;

// audit_record.records_erased = 67 (42 + 18 + 7)
// audit_record.erasure_proof = SHA-256 hash
// audit_record.completed_at = now
```

---

## Exemptions (Article 17(3))

Not all erasure requests must be fulfilled. GDPR Article 17(3) provides four exemption bases:

| Exemption | Article | Example |
|-----------|---------|---------|
| `LegalObligation` | 17(3)(b) | Tax records must be retained for 7 years |
| `PublicHealth` | 17(3)(c) | Pandemic contact tracing data |
| `Archiving` | 17(3)(d) | Historical research in the public interest |
| `LegalClaims` | 17(3)(e) | Data needed for ongoing litigation |

```rust
use kimberlite_compliance::erasure::ExemptionBasis;

// Active litigation — cannot erase
engine.exempt_from_erasure(
    request.request_id,
    ExemptionBasis::LegalClaims,
)?;
// request.status = ErasureStatus::Exempt
```

---

## Deadline Enforcement

The engine tracks 30-day deadlines and reports overdue requests:

```rust
use chrono::Utc;

// Check for overdue erasure requests
let overdue = engine.check_deadlines(Utc::now());

for request in overdue {
    // Alert: erasure request for {subject_id} is past 30-day deadline
    tracing::warn!(
        subject_id = %request.subject_id,
        deadline = %request.deadline,
        "Erasure request overdue — GDPR compliance risk"
    );
}
```

---

## Audit Trail

Every erasure operation creates an **immutable audit record**:

```rust
pub struct ErasureAuditRecord {
    pub request_id: Uuid,
    pub subject_id: String,
    pub requested_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub records_erased: u64,
    pub streams_affected: Vec<StreamId>,
    pub erasure_proof: Hash,     // SHA-256 of erased record IDs
}
```

**Critical design decision:** The audit trail itself is *never erased*. GDPR requires proof that deletion occurred — you must be able to demonstrate compliance. The audit record contains no personal data (only subject ID and counts), satisfying both the deletion and accountability requirements.

---

## Tombstone vs Physical Deletion

Kimberlite uses **tombstoning** rather than physical deletion:

| Approach | Pros | Cons |
|----------|------|------|
| **Tombstone** (Kimberlite) | Log integrity preserved, audit trail intact, recoverable if exemption granted | Storage not reclaimed immediately |
| **Physical delete** | Storage reclaimed | Breaks hash chain, destroys audit evidence |

Tombstoned records are:
- Excluded from all query results
- Excluded from data exports
- Visible only in the raw audit log (for compliance proof)

---

## Integration with Consent Withdrawal

When consent is withdrawn via `TenantHandle::withdraw_consent()` and no remaining valid consents exist for the subject, an erasure request can be automatically triggered:

```rust
// Withdraw consent
tenant.withdraw_consent(consent_id)?;

// If no remaining consents → trigger erasure
let mut engine = ErasureEngine::new();
let request = engine.request_erasure("user@example.com")?;
```

This implements the **consent withdrawal → erasure pipeline** required by GDPR.

---

## Formal Verification

### Kani Bounded Model Checking

**File:** `crates/kimberlite-compliance/src/kani_proofs.rs`

| Proof | Property |
|-------|----------|
| `verify_breach_detection` | Erasure-related state transitions are valid |

### TLA+ Specification

**File:** `specs/tla/compliance/GDPR.tla`

**Article 17 Properties:**
- `ErasureCompleteness` — all subject data erased across all streams
- `DeadlineEnforced` — 30-day deadline tracked and reported
- `AuditPreserved` — erasure audit records are immutable

---

## Best Practices

### 1. Process Erasure Requests Promptly

Don't wait until day 29. Process requests within 7 days to provide margin for complications.

### 2. Document Exemptions Thoroughly

When exempting a request, record the specific legal basis, approving authority, and expected duration. This will be scrutinized in audits.

### 3. Test the Erasure Pipeline

Regularly test the full erasure workflow (request → identify → execute → verify) in a staging environment. Ensure the audit trail is correct.

### 4. Monitor Deadline Compliance

Set up automated alerts for erasure requests approaching their 30-day deadline. Treat overdue requests as compliance incidents.

---

## See Also

- [Consent Management](consent-management.md) — GDPR consent tracking
- [Data Classification](data-classification.md) — 8-level classification system
- [Compliance Overview](compliance.md) — Multi-framework compliance architecture
- [Breach Notification](breach-notification.md) — Incident detection and response

---

**Key Takeaway:** Right to erasure isn't just deleting rows — it's a complete workflow with deadlines, exemptions, cascade deletion, and cryptographic proof. Kimberlite handles this complexity so you don't have to.
