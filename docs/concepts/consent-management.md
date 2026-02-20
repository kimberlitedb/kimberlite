---
title: "Consent Management"
section: "concepts"
slug: "consent-management"
order: 10
---

# Consent Management

Kimberlite provides **GDPR-compliant consent tracking** for personal data processing.

## Core Principle

Processing personal data requires **lawful basis** under GDPR Article 6. Consent is one of the valid bases, requiring:

1. **Freely given** - Subject has genuine choice
2. **Specific** - Consent for particular purpose
3. **Informed** - Subject understands what they're consenting to
4. **Unambiguous** - Clear affirmative action
5. **Withdrawable** - As easy to withdraw as to give

**Result:** Consent isn't a checkbox—it's a legally binding contract with the data subject.

## GDPR Articles Covered

| Article | Requirement | Kimberlite Support |
|---------|-------------|-------------------|
| **Article 6** | Lawful basis for processing | ✅ Purpose validation |
| **Article 7** | Conditions for consent | ✅ Consent tracking |
| **Article 5(1)(b)** | Purpose limitation | ✅ Purpose enforcement |
| **Article 5(1)(c)** | Data minimization | ✅ Scope validation |

## Architecture

```rust
ConsentRecord = {
    consent_id: Uuid,           // Unique identifier
    subject_id: String,         // Data subject (email, user ID)
    purpose: Purpose,           // Why data is processed
    granted_at: Timestamp,      // When consent was given
    withdrawn_at: Option<Timestamp>,
    scope: ConsentScope,        // What data is covered
}
```

## Purposes (GDPR Article 6)

Kimberlite supports 8 purposes with automatic validation:

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

## Consent Scopes

Control what data is covered by consent:

```rust
pub enum ConsentScope {
    AllData,               // All personal data
    ContactInfo,           // Email, phone only
    AnalyticsOnly,         // Anonymized usage data
    ContractualNecessity,  // Only for contract performance
}
```

## Usage Examples

### Grant Consent

```rust
use kimberlite_compliance::consent::ConsentTracker;
use kimberlite_compliance::purpose::Purpose;

let mut tracker = ConsentTracker::new();

// Grant consent for marketing
let consent_id = tracker.grant_consent(
    "user@example.com",
    Purpose::Marketing,
).unwrap();

// Grant consent with specific scope
let consent_id = tracker.grant_consent_with_scope(
    "user@example.com",
    Purpose::Analytics,
    ConsentScope::AnalyticsOnly,
).unwrap();
```

### Check Consent

```rust
// Check if subject has valid consent
if tracker.check_consent("user@example.com", Purpose::Marketing) {
    // Process marketing data
} else {
    // Reject: No consent
}
```

### Withdraw Consent

```rust
// Withdraw consent (GDPR Article 7(3))
tracker.withdraw_consent(consent_id).unwrap();

// Consent is now invalid
assert!(!tracker.check_consent("user@example.com", Purpose::Marketing));
```

### Validate Query Before Execution

```rust
use kimberlite_compliance::validator::ConsentValidator;
use kimberlite_compliance::classification::DataClass;

let mut validator = ConsentValidator::new();
validator.grant_consent("user@example.com", Purpose::Analytics).unwrap();

// Validate before query execution
let result = validator.validate_query(
    "user@example.com",
    Purpose::Analytics,
    DataClass::PII,
);

match result {
    Ok(()) => {
        // Execute query
    }
    Err(e) => {
        // Reject query: {e}
    }
}
```

## Purpose Limitation

GDPR Article 5(1)(b) requires data be collected for "specified, explicit and legitimate purposes."

### Automatic Validation

```rust
use kimberlite_compliance::purpose::{Purpose, validate_purpose};
use kimberlite_compliance::classification::DataClass;

// Valid: Marketing with consent for PII
assert!(validate_purpose(DataClass::PII, Purpose::Marketing).is_ok());

// Invalid: Marketing not allowed for PHI (HIPAA violation)
assert!(validate_purpose(DataClass::PHI, Purpose::Marketing).is_err());

// Invalid: Analytics not allowed for PCI (PCI DSS violation)
assert!(validate_purpose(DataClass::PCI, Purpose::Analytics).is_err());

// Valid: Contractual processing for any data class
assert!(validate_purpose(DataClass::PHI, Purpose::Contractual).is_ok());
```

### Data Minimization

GDPR Article 5(1)(c) requires data be "adequate, relevant and limited to what is necessary."

```rust
// Check if purpose satisfies data minimization
assert!(!Purpose::Marketing.is_data_minimization_compliant(DataClass::PHI));
assert!(Purpose::Contractual.is_data_minimization_compliant(DataClass::PII));
```

## Consent Expiry

Set expiry dates for time-limited consent:

```rust
use chrono::{Utc, Duration};

let consent_id = tracker.grant_consent("user@example.com", Purpose::Marketing).unwrap();

// Get mutable record and set expiry
let record = tracker.consents.get_mut(&consent_id).unwrap();
record.expires_at = Some(Utc::now() + Duration::days(365)); // 1 year

// After expiry, consent automatically becomes invalid
```

## Audit Trail

All consent operations are logged:

- Consent granted → Timestamp recorded
- Consent withdrawn → Timestamp recorded
- Query rejected → Audit log entry

```sql
-- Query consent audit trail
SELECT subject_id, purpose, granted_at, withdrawn_at
FROM __consent_audit
WHERE subject_id = 'user@example.com'
ORDER BY granted_at DESC;
```

## Integration with Server

### API Layer

```rust
// In kimberlite-server request handler
fn handle_query(req: QueryRequest) -> Result<QueryResponse> {
    let mut validator = get_consent_validator();

    // Extract subject and purpose from request
    let subject_id = req.authenticated_user.subject_id;
    let purpose = req.purpose; // Client specifies purpose
    let data_class = infer_data_class(&req.query); // From query analysis

    // Validate before execution
    validator.validate_query(&subject_id, purpose, data_class)?;

    // Execute query...
}
```

### Consent Management API

```rust
// POST /consent/grant
fn grant_consent_endpoint(req: GrantConsentRequest) -> Result<GrantConsentResponse> {
    let mut validator = get_consent_validator();

    let consent_id = validator.grant_consent(
        &req.subject_id,
        req.purpose,
    )?;

    Ok(GrantConsentResponse { consent_id })
}

// POST /consent/withdraw
fn withdraw_consent_endpoint(req: WithdrawConsentRequest) -> Result<()> {
    let mut validator = get_consent_validator();
    validator.withdraw_consent(req.consent_id)?;
    Ok(())
}

// GET /consent/list
fn list_consents_endpoint(req: ListConsentsRequest) -> Result<ListConsentsResponse> {
    let validator = get_consent_validator();
    let consents = validator.tracker().get_consents_for_subject(&req.subject_id);
    Ok(ListConsentsResponse { consents: consents.into_iter().cloned().collect() })
}
```

## Formal Verification

### TLA+ Specification

`specs/tla/compliance/GDPR.tla` proves consent properties:

**Article 6 (Lawful Basis)**:
```tla
GDPR_Article_6_LawfulBasis ==
    \A ds \in DataSubject :
        \A op \in Operation :
            op.purpose \in {"Contractual", "LegalObligation"}
            \/ HasValidConsent(ds, op.purpose)
```

**Article 7 (Consent Conditions)**:
```tla
GDPR_Article_7_ConsentConditions ==
    \A ds \in DataSubject :
        \A consent \in consentRecords[ds] :
            /\ consent.granted_at # NULL
            /\ consent.withdrawn_at # NULL => consent.valid = FALSE
```

### Kani Proofs

5 bounded model checking proofs (#41-45):

1. **Proof #41**: Consent grant/withdraw correctness
   - Withdrawn consent is never valid
   - `is_withdrawn()` returns true after withdrawal

2. **Proof #42**: Purpose validation for data classes
   - Marketing not allowed for PHI/PCI
   - Contractual allowed for all classes

3. **Proof #43**: Consent validator enforcement
   - Queries without required consent are rejected
   - Withdrawn consent is treated as no consent

4. **Proof #44**: Consent expiry handling
   - Expired consent is treated as invalid
   - `is_expired()` returns true past expiry

5. **Proof #45**: Multiple consents per subject
   - Subject can have multiple valid consents
   - Withdrawing one doesn't affect others

Run proofs:
```bash
cargo kani --harness verify_consent_withdraw_correctness
cargo kani --harness verify_purpose_validation
cargo kani --harness verify_consent_validator_enforcement
```

## Best Practices

### 1. Always Specify Purpose

```rust
// ✓ Good: Explicit purpose
validator.validate_query(subject_id, Purpose::Analytics, data_class)?;

// ✗ Bad: Implicit purpose (none)
// No way to validate consent
```

### 2. Request Minimal Scope

```rust
// ✓ Good: Request only contact info for email campaign
tracker.grant_consent_with_scope(
    user_id,
    Purpose::Marketing,
    ConsentScope::ContactInfo,
)?;

// ✗ Avoid: Requesting AllData when ContactInfo suffices
tracker.grant_consent_with_scope(
    user_id,
    Purpose::Marketing,
    ConsentScope::AllData,  // Over-broad
)?;
```

### 3. Set Expiry for Time-Limited Processing

```rust
// ✓ Good: Campaign-specific consent with expiry
let consent_id = tracker.grant_consent(user_id, Purpose::Marketing)?;
let record = tracker.consents.get_mut(&consent_id).unwrap();
record.expires_at = Some(Utc::now() + Duration::days(90)); // Campaign duration
```

### 4. Provide Withdrawal UI

Make withdrawal as easy as granting consent (GDPR Article 7(3)):

```rust
// UI: "Manage Consents" page
fn render_consents_page(user_id: &str) -> Html {
    let consents = tracker.get_valid_consents(user_id);

    html! {
        <div>
            <h2>"Your Consents"</h2>
            {for consents.iter().map(|c| html! {
                <div>
                    <p>{format!("Purpose: {}", c.purpose)}</p>
                    <button onclick={withdraw_consent(c.consent_id)}>
                        "Withdraw Consent"
                    </button>
                </div>
            })}
        </div>
    }
}
```

## Kernel Integration

Consent is enforced at the **kernel level** — queries and writes for PII/PHI/Sensitive data are rejected unless valid consent exists:

```rust
// Validate consent via TenantHandle
tenant.validate_consent("user@example.com", Purpose::Marketing)?;
// Returns Ok(()) if valid consent exists
// Returns Err(ComplianceError) if no consent

// Grant consent (returns consent_id for withdrawal)
let consent_id = tenant.grant_consent("user@example.com", Purpose::Marketing)?;

// Withdraw consent
tenant.withdraw_consent(consent_id)?;
```

**Automatic behavior:**
- Queries for **PII, PHI, Sensitive** data require valid consent for the stated purpose
- Queries for **Public, Deidentified** data do not require consent
- Purposes that don't require consent (e.g., `LegalObligation`, `VitalInterests`) bypass consent checking

## Consent Withdrawal and Erasure

When consent is withdrawn and no remaining valid consents exist for the subject, an **erasure request** can be automatically triggered per GDPR Article 17:

```rust
// Withdraw consent
tenant.withdraw_consent(consent_id)?;

// If no remaining consents → trigger erasure
let mut engine = ErasureEngine::new();
let request = engine.request_erasure("user@example.com")?;
// request.deadline = now + 30 days
```

**See:** [Right to Erasure](right-to-erasure.md) for the complete erasure workflow.

## Related Documentation

- **[Right to Erasure](right-to-erasure.md)** - GDPR Article 17 (triggered by consent withdrawal)
- **[Data Portability](data-portability.md)** - GDPR Article 20 data export
- **[Data Classification](data-classification.md)** - 8-level classification system
- **[RBAC](rbac.md)** - Role-based access control
- **[Field-Level Masking](field-masking.md)** - Data minimization for masked fields
- **[Compliance Overview](compliance.md)** - Multi-framework compliance

---

**Key Takeaway:** Consent management isn't a feature you bolt on—it's a legal requirement. Kimberlite makes it impossible to process personal data without valid consent.
