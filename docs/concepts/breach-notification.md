# Breach Detection and Notification

Kimberlite provides **automated breach detection** with 72-hour notification deadline tracking:

- **6 breach indicators** with configurable thresholds
- **Severity classification** (Low, Medium, High, Critical) based on data classes affected
- **72-hour notification deadline** per HIPAA § 164.404 and GDPR Article 33
- **Structured breach reports** with timeline, remediation, and notification status
- **Formal verification** — TLA+ and Kani proofs for detection correctness

---

## Why Breach Detection Matters

**Regulations require rapid response to data breaches:**

| Framework | Requirement | Deadline |
|-----------|-------------|----------|
| **HIPAA** | § 164.404 | Notify affected individuals within 60 days |
| **GDPR** | Article 33 | Notify supervisory authority within **72 hours** |
| **GDPR** | Article 34 | Notify data subjects "without undue delay" |
| **PCI DSS** | Requirement 12.10 | Incident response plan with defined timelines |
| **SOC 2** | CC7.3 | Evaluate and communicate security events |

**Kimberlite's approach:** Detection happens inline with the audit pipeline. Every access decision (allow or deny) is checked against breach indicators with O(1) overhead per event.

---

## Breach Indicators

Kimberlite monitors 6 indicators that may signal a data breach:

### 1. Mass Data Export

**Trigger:** A single user exports more than `threshold` records in one operation.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `mass_export_records` | 1,000 | Records exported in a single operation |

**Example:** An employee downloads 5,000 patient records to a USB drive.

### 2. Unauthorized Access Pattern

**Trigger:** More than `threshold` access denials from the same source within a time window.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `denied_attempts_window` | 10 in 60s | Denied attempts before triggering |

**Example:** An attacker brute-forces API endpoints, generating repeated 403 responses.

### 3. Privilege Escalation

**Trigger:** Any attempt to access resources above the user's role level. **Always triggers** — there is no threshold.

**Example:** A User role attempts to query audit logs (Auditor-only), or an Analyst attempts a DELETE operation (Admin-only).

### 4. Anomalous Query Volume

**Trigger:** Query rate exceeds `multiplier` times the baseline for that user/role.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `query_volume_multiplier` | 5.0x | Multiple of baseline query rate |

**Example:** An analyst account typically runs 50 queries/hour but suddenly runs 500.

### 5. Unusual Access Time

**Trigger:** Data access outside business hours (09:00–17:00 UTC, weekdays).

**Example:** A production database query at 3:00 AM on a Sunday.

### 6. Data Exfiltration Pattern

**Trigger:** Total bytes exported exceed `threshold` in a session.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `export_bytes_threshold` | 100 MB | Total bytes exported in a session |

**Example:** Automated scraping exports 500 MB of customer data.

---

## Severity Classification

Severity is determined by the **data classes affected** and the **indicator type**:

| Data Class Affected | Severity |
|---------------------|----------|
| PHI or PCI | **Critical** |
| PII or Sensitive | **High** |
| Confidential or Financial | **Medium** |
| Public or Deidentified | **Low** |

**Special cases:**
- Privilege escalation is always **High** or above (regardless of data class)
- Mixed data classes use the **highest** severity

---

## Breach Lifecycle

```
┌──────────┐    ┌───────────────────┐    ┌──────────┐    ┌──────────┐
│ Detected  │───►│ Under             │───►│ Confirmed │───►│ Resolved  │
│           │    │ Investigation     │    │           │    │           │
└──────────┘    └───────────────────┘    └──────────┘    └──────────┘
     │                │
     │                │
     ▼                ▼
┌──────────────────────┐
│ False Positive        │
│ (Dismissed with       │
│  reason + approver)   │
└──────────────────────┘
```

### Status Transitions

| From | To | Method | Requirements |
|------|----|--------|-------------|
| Detected | Under Investigation | `escalate()` | — |
| Detected | False Positive | `dismiss()` | Reason + approver |
| Under Investigation | Confirmed | `confirm()` | — |
| Under Investigation | False Positive | `dismiss()` | Reason + approver |
| Confirmed | Resolved | `resolve()` | Remediation description |

---

## Usage

### Create a Breach Detector

```rust
use kimberlite_compliance::breach::{BreachDetector, BreachThresholds};

// Default thresholds
let mut detector = BreachDetector::new();

// Or custom thresholds for stricter environments
let mut detector = BreachDetector::with_thresholds(BreachThresholds {
    mass_export_records: 500,       // Stricter than default 1000
    denied_attempts_window: 5,      // Stricter than default 10
    query_volume_multiplier: 3.0,   // Stricter than default 5.0
    export_bytes_threshold: 50_000_000, // 50 MB instead of 100 MB
});
```

### Check for Breaches

```rust
// Check mass data export
if let Some(event) = detector.check_mass_export(5000, &[DataClass::PHI]) {
    // event.severity = Critical (PHI data)
    // event.notification_deadline = now + 72h
    handle_breach(event);
}

// Check denied access pattern
if let Some(event) = detector.check_denied_access(&[DataClass::PII]) {
    handle_breach(event);
}

// Check privilege escalation
if let Some(event) = detector.check_privilege_escalation(
    "User", "Admin", &[DataClass::Sensitive]
) {
    handle_breach(event);
}
```

### Manage Breach Lifecycle

```rust
// Escalate to investigation
detector.escalate(event.event_id)?;

// After investigation: confirm or dismiss
detector.confirm(event.event_id)?;
// OR
detector.dismiss(event.event_id, "False positive: scheduled batch job")?;

// After remediation: resolve
detector.resolve(event.event_id, "Revoked compromised API key, rotated tokens")?;
```

### Check 72-Hour Deadlines

```rust
use chrono::Utc;

let overdue = detector.check_notification_deadlines(Utc::now());

for event in overdue {
    tracing::error!(
        event_id = %event.event_id,
        severity = ?event.severity,
        deadline = %event.notification_deadline,
        "Breach notification OVERDUE — regulatory violation risk"
    );
}
```

### Generate Breach Report

```rust
let report = detector.generate_report(event.event_id)?;

// report.timeline — chronological event history
// report.affected_data_classes — which data types involved
// report.remediation_steps — actions taken
// report.notification_status — whether 72h deadline was met
```

---

## Custom Thresholds

Configure thresholds per deployment environment:

```rust
use kimberlite_compliance::breach::BreachThresholds;

// Production: strict thresholds
let prod = BreachThresholds {
    mass_export_records: 500,
    denied_attempts_window: 5,
    query_volume_multiplier: 3.0,
    export_bytes_threshold: 50_000_000,
};

// Staging: relaxed for testing
let staging = BreachThresholds {
    mass_export_records: 10_000,
    denied_attempts_window: 50,
    query_volume_multiplier: 20.0,
    export_bytes_threshold: 1_000_000_000,
};
```

---

## Formal Verification

### Kani Bounded Model Checking

**File:** `crates/kimberlite-compliance/src/kani_proofs.rs`

| Proof | Property |
|-------|----------|
| `verify_breach_detection` | Threshold comparison correctness |

### TLA+ Specification

**File:** `specs/tla/compliance/MetaFramework.tla`

**Properties:**
- `BreachDetected` — all indicators trigger events when thresholds exceeded
- `DeadlineEnforced` — 72-hour notification deadline tracked
- `AuditComplete` — all breach events logged immutably

---

## Best Practices

### 1. Set Conservative Thresholds

Start with strict thresholds and relax only when you have data on false positive rates. It's better to investigate a false alarm than miss a real breach.

### 2. Automate Escalation

Wire breach events into your incident response pipeline (PagerDuty, Opsgenie, etc.). Don't rely on humans checking dashboards.

### 3. Document Every Dismissal

When dismissing a breach event as a false positive, record the reason, the investigating analyst, and the evidence. Regulators will ask.

### 4. Test Breach Response Quarterly

Run tabletop exercises: simulate a breach event and verify your team can respond within 72 hours. Document the exercise results.

---

## See Also

- [Compliance Overview](compliance.md) — Multi-framework compliance architecture
- [Right to Erasure](right-to-erasure.md) — GDPR Article 17 data deletion
- [Data Classification](data-classification.md) — Severity classification depends on data class
- [RBAC](rbac.md) — Access controls that generate audit events for breach detection

---

**Key Takeaway:** Breach detection isn't optional — GDPR and HIPAA mandate it. Kimberlite detects breaches inline with zero additional I/O, classifies severity by data sensitivity, and enforces 72-hour notification deadlines.
