# Data Portability (GDPR Article 20)

Kimberlite provides **GDPR-compliant data portability exports** with cryptographic integrity guarantees:

- **Machine-readable formats** — JSON and CSV (Article 20(1))
- **SHA-256 content hashing** — integrity verification for every export
- **HMAC-SHA256 signing** — authenticity proof with constant-time verification
- **Immutable audit trail** — every export operation logged for compliance
- **Cross-stream aggregation** — collect subject data from all streams automatically
- **Formal verification** — Kani proofs for export correctness

---

## Why Data Portability Matters

GDPR Article 20 gives data subjects the right to receive their personal data in a **structured, commonly used, and machine-readable format**, and to transmit that data to another controller.

**Key requirements:**

| GDPR Article | Requirement | Kimberlite Support |
|-------------|-------------|-------------------|
| **Article 20(1)** | Receive data in structured, machine-readable format | JSON and CSV export |
| **Article 20(2)** | Transmit data to another controller | Signed, verifiable export packages |
| **Article 20(3)** | Direct transmission between controllers where technically feasible | Signed export with integrity proof |
| **Article 5(1)(f)** | Integrity and confidentiality of processing | SHA-256 content hash + HMAC signature |
| **Article 30** | Record of processing activities | Immutable audit trail for every export |

**Non-compliance risk:** Fines up to **EUR 20M or 4% of global revenue** under GDPR Article 83(5)(b).

---

## Export Formats

### JSON

**Structured, commonly used, machine-readable** — the gold standard for GDPR Article 20 compliance.

```json
[
  {
    "stream_id": 1,
    "stream_name": "patient_records",
    "offset": 0,
    "data": {"name": "Jane Doe", "dob": "1985-03-15"},
    "timestamp": "2025-01-15T10:30:00Z"
  },
  {
    "stream_id": 3,
    "stream_name": "billing_records",
    "offset": 42,
    "data": {"invoice": "INV-2025-001", "amount": 150.00},
    "timestamp": "2025-02-01T14:00:00Z"
  }
]
```

**Use when:** Transmitting data to another system, archiving, or when the recipient needs structured access to individual fields.

### CSV

**Tabular, commonly used, machine-readable** — widely supported by spreadsheets and legacy systems.

```
stream_id,stream_name,offset,data,timestamp
1,patient_records,0,"{""name"":""Jane Doe"",""dob"":""1985-03-15""}",2025-01-15T10:30:00Z
3,billing_records,42,"{""invoice"":""INV-2025-001"",""amount"":150.00}",2025-02-01T14:00:00Z
```

**Use when:** The data subject requests a spreadsheet-compatible format or the receiving controller has limited import capabilities.

---

## Architecture

Exports are generated from the **committed state** of all streams, ensuring consistency:

```
┌─────────────────────────────────────┐
│  Export Request                       │
│  Subject: jane@example.com            │
│  Format: JSON                         │
└───────────────┬─────────────────────┘
                │
                ▼
┌─────────────────────────────────────┐
│  1. Collect Records                  │
│  Scan all streams for subject data   │
│  patient_records: 12 records         │
│  billing_records: 5 records          │
│  prescriptions: 3 records            │
└───────────────┬─────────────────────┘
                │
                ▼
┌─────────────────────────────────────┐
│  2. Serialize                        │
│  Format records as JSON or CSV       │
│  Compute SHA-256 content hash        │
└───────────────┬─────────────────────┘
                │
                ▼
┌─────────────────────────────────────┐
│  3. Sign (Optional)                  │
│  HMAC-SHA256(key, content_hash)      │
│  Constant-time verification          │
└───────────────┬─────────────────────┘
                │
                ▼
┌─────────────────────────────────────┐
│  4. Audit Record                     │
│  export_id, subject_id, timestamp    │
│  record_count, content_hash          │
│  (Immutable, append-only)            │
└─────────────────────────────────────┘
```

**Dual-hash convention:** SHA-256 is used for content hashing (compliance-critical path), consistent with Kimberlite's cryptographic architecture (SHA-256 for compliance, BLAKE3 for internal hot paths).

---

## Usage

### Export Subject Data

```rust
use kimberlite_compliance::export::{ExportEngine, ExportFormat, ExportRecord};
use kimberlite_types::StreamId;
use chrono::Utc;

let mut engine = ExportEngine::new();

// Collect all records for the subject across streams
let records = vec![
    ExportRecord {
        stream_id: StreamId::new(1),
        stream_name: "patient_records".to_string(),
        offset: 0,
        data: serde_json::json!({"name": "Jane Doe", "dob": "1985-03-15"}),
        timestamp: Utc::now(),
    },
    ExportRecord {
        stream_id: StreamId::new(3),
        stream_name: "billing_records".to_string(),
        offset: 42,
        data: serde_json::json!({"invoice": "INV-2025-001", "amount": 150.00}),
        timestamp: Utc::now(),
    },
];

// Export as JSON
let export = engine.export_subject_data(
    "jane@example.com",
    &records,
    ExportFormat::Json,
)?;

// export.record_count = 2
// export.streams_included = [StreamId(1), StreamId(3)]
// export.content_hash = SHA-256 of serialized data
```

### Sign an Export

Signing provides **authenticity proof** — the recipient can verify the export was produced by Kimberlite and has not been tampered with.

```rust
// Sign with HMAC-SHA256
let signing_key = b"your-signing-key-32-bytes-long!!";
engine.sign_export(export.export_id, signing_key)?;

// Retrieve the signed export
let signed = engine.get_export(export.export_id).unwrap();
assert!(signed.signature.is_some());
```

### Verify an Export

```rust
use kimberlite_compliance::export::ExportEngine;

// Recipient verifies the export
let data = ExportEngine::format_as_json(&records)?;
let valid = ExportEngine::verify_export_signature(
    &signed_export,
    &data,
    signing_key,
)?;

assert!(valid); // Signature matches — export is authentic
```

**Constant-time comparison:** Signature verification uses constant-time byte comparison to prevent timing side-channel attacks.

### Verify with Wrong Key Fails

```rust
let wrong_key = b"attacker-key-should-fail-verify!";
let valid = ExportEngine::verify_export_signature(
    &signed_export,
    &data,
    wrong_key,
)?;

assert!(!valid); // Signature does not match
```

---

## Export Metadata

Every export produces a `PortabilityExport` with complete metadata:

```rust
pub struct PortabilityExport {
    pub export_id: Uuid,              // Unique identifier
    pub subject_id: String,           // Data subject
    pub requested_at: DateTime<Utc>,  // When requested
    pub completed_at: DateTime<Utc>,  // When completed
    pub format: ExportFormat,         // JSON or CSV
    pub streams_included: Vec<StreamId>, // Streams with subject data
    pub record_count: u64,            // Total records exported
    pub content_hash: Hash,           // SHA-256 integrity proof
    pub signature: Option<Vec<u8>>,   // HMAC-SHA256 authenticity proof
}
```

---

## Audit Trail

Every export operation creates an **immutable audit record**:

```rust
pub struct ExportAuditRecord {
    pub export_id: Uuid,              // Links to the export
    pub subject_id: String,           // Data subject
    pub requested_at: DateTime<Utc>,  // When requested
    pub completed_at: DateTime<Utc>,  // When completed
    pub format: ExportFormat,         // Format used
    pub record_count: u64,            // Records exported
    pub content_hash: Hash,           // SHA-256 of export data
}
```

**Why audit exports?** GDPR Article 30 requires records of processing activities. Every export is a processing activity that must be logged. The audit trail proves:

1. **What** was exported (record count, content hash)
2. **When** it was exported (timestamps)
3. **For whom** (subject ID)
4. **From where** (streams included)

```rust
// Query the audit trail
let trail = engine.get_audit_trail();
for record in trail {
    println!(
        "Export {} for {}: {} records, hash={}",
        record.export_id,
        record.subject_id,
        record.record_count,
        record.content_hash,
    );
}
```

---

## Cryptographic Integrity

### Content Hash (SHA-256)

Every export includes a SHA-256 hash of the serialized data bytes. This allows the recipient to independently verify that the data has not been corrupted or tampered with in transit.

```rust
use kimberlite_compliance::export::ExportEngine;

// Compute hash independently
let data = ExportEngine::format_as_json(&records)?;
let hash = ExportEngine::compute_content_hash(&data);

// Verify it matches the export metadata
assert_eq!(hash, export.content_hash);
```

### HMAC-SHA256 Signature

The optional signature proves **who** produced the export. Only entities with the signing key can produce a valid signature.

```
Signature = SHA256(signing_key || content_hash_bytes)
```

**Verification flow:**

1. Recipient receives `(export_data, content_hash, signature)`
2. Recipient computes `SHA256(export_data)` and verifies it matches `content_hash`
3. Recipient computes `SHA256(shared_key || content_hash)` and verifies it matches `signature`
4. If both match: data is authentic and unmodified

---

## CLI Integration

Export subject data using the compliance CLI:

```bash
# Generate a compliance report (includes export capabilities)
kimberlite-compliance verify --framework GDPR --detailed

# List all supported frameworks
kimberlite-compliance frameworks
```

---

## Formal Verification

### Kani Bounded Model Checking

**File:** `crates/kimberlite-compliance/src/kani_proofs.rs`

| Proof | Property |
|-------|----------|
| `verify_export_completeness` | All subject records included in export |
| `verify_export_signature` | HMAC-SHA256 signature verification succeeds for correct key |

### Production Assertions

Every export operation enforces:

- `assert!(!content_hash.is_genesis())` — export content hash must not be all zeros
- `assert!(!records.is_empty())` — no empty exports allowed (returns error instead)
- `assert!(!signing_key.is_empty())` — signing key must not be empty

### TLA+ Specification

**File:** `specs/tla/compliance/MetaFramework.tla`

**Properties:**
- `ExportCompleteness` — all subject data exported across all streams
- `AuditComplete` — every export produces an immutable audit record

---

## Integration with Other Compliance Features

### Erasure After Export

A common GDPR workflow: export data, then erase it.

```rust
use kimberlite_compliance::export::{ExportEngine, ExportFormat};
use kimberlite_compliance::erasure::ErasureEngine;

// Step 1: Export the subject's data
let mut export_engine = ExportEngine::new();
let export = export_engine.export_subject_data(
    "user@example.com",
    &records,
    ExportFormat::Json,
)?;

// Step 2: Erase the subject's data
let mut erasure_engine = ErasureEngine::new();
let request = erasure_engine.request_erasure("user@example.com")?;
```

### Consent Validation

Exports should verify that the requesting subject has valid consent or that the export is legally required:

```rust
// Validate consent before export
tenant.validate_consent(
    "user@example.com",
    kimberlite_compliance::purpose::Purpose::DataPortability,
)?;

// Consent valid — proceed with export
let export = export_engine.export_subject_data(
    "user@example.com",
    &records,
    ExportFormat::Json,
)?;
```

### Breach Detection

Mass exports trigger breach detection. The breach detector monitors for unusual export volumes:

```rust
use kimberlite_compliance::breach::BreachDetector;

// If export exceeds threshold → breach indicator
// detector.check_mass_export(record_count, &data_classes)
```

---

## Best Practices

### 1. Always Sign Exports

Unsigned exports cannot prove authenticity. Always sign exports with HMAC-SHA256, especially when transmitting data to other controllers (Article 20(2)).

### 2. Verify Before Delivering

After generating an export, verify the content hash and signature before delivering to the data subject. This catches corruption before it reaches the recipient.

### 3. Audit Every Export

The audit trail is mandatory under GDPR Article 30. Never bypass the audit logging — regulators will ask for proof of every data processing activity.

### 4. Use JSON for Interoperability

JSON is the most widely supported machine-readable format. Use CSV only when the recipient specifically requests it or has limited import capabilities.

### 5. Combine with Erasure When Appropriate

When a data subject exercises both portability (Article 20) and erasure (Article 17), export first, then erase. This ensures the subject receives their data before it is deleted.

---

## See Also

- [Right to Erasure](right-to-erasure.md) — GDPR Article 17 data deletion
- [Consent Management](consent-management.md) — GDPR consent tracking
- [Breach Notification](breach-notification.md) — Mass export triggers breach detection
- [Data Classification](data-classification.md) — Classification determines export scope
- [Compliance Overview](compliance.md) — Multi-framework compliance architecture

---

**Key Takeaway:** Data portability isn't just exporting rows — it's a complete pipeline with integrity proofs (SHA-256), authenticity guarantees (HMAC-SHA256), immutable audit trails, and cross-stream aggregation. Kimberlite handles this complexity while meeting GDPR Article 20 requirements.
