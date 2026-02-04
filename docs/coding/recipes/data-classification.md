# Data Classification

Tag and manage data by sensitivity level in Kimberlite.

## Overview

Data classification helps you:
- Identify sensitive data (PHI, PII, PCI, etc.)
- Apply appropriate security controls
- Generate compliance reports
- Enforce access policies

## Classification Levels

Common classification schemes:

### Healthcare (HIPAA)

| Level | Description | Examples |
|-------|-------------|----------|
| **PHI** | Protected Health Information | Name + DOB, Medical records, SSN |
| **De-identified** | HIPAA Safe Harbor compliant | Age range, Zip code (3 digits) |
| **Public** | No PHI | Facility hours, Public health stats |

### General Purpose

| Level | Description | Examples |
|-------|-------------|----------|
| **Restricted** | Highly sensitive | SSN, Credit cards, Passwords |
| **Confidential** | Sensitive business data | Financial records, Contracts |
| **Internal** | Internal use only | Employee directory, Policies |
| **Public** | Public information | Marketing materials, Website |

## Schema Design

Add a `classification` column to track sensitivity:

```sql
CREATE TABLE patients (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    date_of_birth DATE,
    ssn_encrypted TEXT,
    classification TEXT DEFAULT 'PHI',  -- Data classification
    created_at TIMESTAMP
);

-- Track classification at the column level
CREATE TABLE data_classifications (
    table_name TEXT NOT NULL,
    column_name TEXT NOT NULL,
    classification TEXT NOT NULL,
    reason TEXT,
    PRIMARY KEY (table_name, column_name)
);

-- Insert classification metadata
INSERT INTO data_classifications VALUES
    ('patients', 'name', 'PHI', 'Directly identifies patient'),
    ('patients', 'date_of_birth', 'PHI', 'Part of HIPAA identifiers'),
    ('patients', 'ssn_encrypted', 'RESTRICTED', 'Encrypted SSN');
```

## Classification Types

Define an enum for type safety:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataClassification {
    /// Highly sensitive (SSN, credit cards, passwords)
    Restricted,

    /// Protected Health Information (HIPAA)
    PHI,

    /// Personally Identifiable Information (GDPR)
    PII,

    /// Payment Card Industry data (PCI DSS)
    PCI,

    /// Confidential business data
    Confidential,

    /// Internal use only
    Internal,

    /// De-identified data (HIPAA Safe Harbor)
    DeIdentified,

    /// Public information
    Public,
}

impl DataClassification {
    /// Returns required access controls for this classification
    pub fn required_controls(&self) -> Vec<AccessControl> {
        match self {
            Self::Restricted | Self::PHI | Self::PCI => vec![
                AccessControl::Encryption,
                AccessControl::AuditLog,
                AccessControl::TwoFactor,
                AccessControl::JustificationRequired,
            ],
            Self::PII | Self::Confidential => vec![
                AccessControl::Encryption,
                AccessControl::AuditLog,
            ],
            Self::Internal => vec![
                AccessControl::Authentication,
            ],
            Self::DeIdentified | Self::Public => vec![],
        }
    }

    /// Returns maximum retention period
    pub fn max_retention(&self) -> Option<Duration> {
        match self {
            Self::PCI => Some(Duration::from_secs(86400 * 365)),  // 1 year
            Self::PHI => Some(Duration::from_secs(86400 * 365 * 7)),  // 7 years
            _ => None,  // No limit
        }
    }
}
```

## Tagging Data

### At Insert Time

```rust
fn insert_patient(
    client: &Client,
    name: &str,
    dob: NaiveDate,
    classification: DataClassification,
) -> Result<()> {
    client.execute(
        "INSERT INTO patients (name, date_of_birth, classification)
         VALUES (?, ?, ?)",
        &[&name, &dob, &classification.to_string()],
    )?;
    Ok(())
}
```

### Bulk Classification

```sql
-- Classify all existing patients as PHI
UPDATE patients SET classification = 'PHI';

-- Classify based on content
UPDATE patients
SET classification = 'DE_IDENTIFIED'
WHERE name IS NULL OR name = 'REDACTED';
```

## Querying by Classification

```sql
-- Find all PHI records
SELECT * FROM patients WHERE classification = 'PHI';

-- Find all restricted data
SELECT table_name, column_name
FROM data_classifications
WHERE classification = 'RESTRICTED';

-- Count records by classification
SELECT classification, COUNT(*)
FROM patients
GROUP BY classification;
```

## Access Control by Classification

Enforce policies based on classification:

```rust
struct AccessPolicy {
    user_role: Role,
    allowed_classifications: Vec<DataClassification>,
}

impl AccessPolicy {
    /// Check if user can access data with this classification
    pub fn can_access(&self, classification: DataClassification) -> bool {
        self.allowed_classifications.contains(&classification)
    }
}

fn query_patients_with_access_control(
    client: &Client,
    user: &User,
    policy: &AccessPolicy,
) -> Result<Vec<Patient>> {
    // Get user's allowed classifications
    let allowed: Vec<String> = policy.allowed_classifications
        .iter()
        .map(|c| c.to_string())
        .collect();

    // Query only data user can access
    let query = format!(
        "SELECT * FROM patients WHERE classification IN ({})",
        allowed.iter().map(|_| "?").collect::<Vec<_>>().join(",")
    );

    client.query(&query, &allowed.iter().map(|s| s.as_str()).collect::<Vec<_>>())
}
```

## De-identification

Convert PHI to de-identified data (HIPAA Safe Harbor):

```rust
use chrono::Datelike;

fn de_identify_patient(patient: &Patient) -> DeIdentifiedPatient {
    DeIdentifiedPatient {
        // Remove direct identifiers
        name: None,  // Remove name
        ssn: None,   // Remove SSN

        // Generalize dates
        year_of_birth: patient.date_of_birth.year(),  // Keep year, remove month/day
        age_range: calculate_age_range(patient.date_of_birth),  // "40-50"

        // Generalize location
        zip_prefix: patient.zip_code[..3].to_string(),  // First 3 digits only

        // Keep relevant clinical data
        diagnosis: patient.diagnosis.clone(),
        treatment: patient.treatment.clone(),

        // Mark as de-identified
        classification: DataClassification::DeIdentified,
    }
}

fn calculate_age_range(dob: NaiveDate) -> String {
    let age = (Utc::now().naive_utc().date() - dob).num_days() / 365;
    let range_start = (age / 10) * 10;
    let range_end = range_start + 9;
    format!("{}-{}", range_start, range_end)
}
```

## Compliance Reports

### HIPAA Data Inventory

```sql
-- All PHI fields in database
SELECT
    table_name,
    column_name,
    classification,
    COUNT(*) as record_count
FROM data_classifications dc
JOIN information_schema.tables t ON t.table_name = dc.table_name
WHERE classification = 'PHI'
GROUP BY table_name, column_name, classification;
```

### PCI DSS Data Report

```sql
-- All PCI data (should be minimal)
SELECT * FROM data_classifications
WHERE classification = 'PCI';
```

### Data Minimization Report

```sql
-- Check for unnecessary sensitive data
SELECT
    table_name,
    column_name,
    classification,
    last_accessed
FROM data_classifications
WHERE classification IN ('RESTRICTED', 'PHI', 'PCI')
  AND last_accessed < NOW() - INTERVAL '1 year';
```

## Automatic Classification

Use rules to classify data automatically:

```rust
pub fn auto_classify(column_name: &str, value: &str) -> DataClassification {
    // Check column name
    if column_name.contains("ssn") || column_name.contains("social_security") {
        return DataClassification::Restricted;
    }

    if column_name.contains("credit_card") || column_name.contains("card_number") {
        return DataClassification::PCI;
    }

    // Check content patterns
    if is_ssn_pattern(value) {
        return DataClassification::Restricted;
    }

    if is_credit_card_pattern(value) {
        return DataClassification::PCI;
    }

    if is_email(value) || is_phone(value) {
        return DataClassification::PII;
    }

    // Default to internal
    DataClassification::Internal
}

fn is_ssn_pattern(value: &str) -> bool {
    // Match XXX-XX-XXXX pattern
    regex::Regex::new(r"^\d{3}-\d{2}-\d{4}$")
        .unwrap()
        .is_match(value)
}
```

## Audit Classification Changes

```sql
-- Track classification changes
CREATE TABLE classification_audit (
    id BIGINT PRIMARY KEY,
    table_name TEXT NOT NULL,
    record_id BIGINT NOT NULL,
    old_classification TEXT,
    new_classification TEXT NOT NULL,
    changed_by BIGINT NOT NULL,
    changed_at TIMESTAMP NOT NULL,
    reason TEXT
);

-- Trigger on classification change
CREATE TRIGGER audit_classification_change
AFTER UPDATE OF classification ON patients
FOR EACH ROW
BEGIN
    INSERT INTO classification_audit (
        table_name, record_id, old_classification, new_classification,
        changed_by, changed_at, reason
    ) VALUES (
        'patients', NEW.id, OLD.classification, NEW.classification,
        current_user_id(), CURRENT_TIMESTAMP, 'Classification updated'
    );
END;
```

## Labeling and Watermarking

Add visual indicators for sensitive data:

```rust
pub struct ClassifiedData {
    pub data: String,
    pub classification: DataClassification,
}

impl ClassifiedData {
    /// Display with classification label
    pub fn to_labeled_string(&self) -> String {
        let label = match self.classification {
            DataClassification::Restricted => "[RESTRICTED]",
            DataClassification::PHI => "[PHI]",
            DataClassification::PCI => "[PCI DSS]",
            DataClassification::Confidential => "[CONFIDENTIAL]",
            _ => "",
        };

        format!("{} {}", label, self.data)
    }
}
```

## Best Practices

### 1. Classify Early

```rust
// Good: Classify at insert time
INSERT INTO patients (name, classification) VALUES ('Alice', 'PHI');

// Bad: Classify later (easy to forget)
INSERT INTO patients (name) VALUES ('Alice');
-- (classification is NULL)
```

### 2. Document Classification Decisions

```sql
INSERT INTO data_classifications (table_name, column_name, classification, reason)
VALUES ('patients', 'ssn_encrypted', 'RESTRICTED', 'Contains encrypted SSN per HIPAA requirements');
```

### 3. Review Classifications Regularly

```bash
# Quarterly data classification review
kmb classify review --since "90 days ago"
```

### 4. Enforce Access Controls

```rust
// Check before allowing access
if !policy.can_access(data.classification) {
    return Err(Error::Unauthorized {
        message: "User cannot access data with this classification",
    });
}
```

### 5. Minimize Sensitive Data

```sql
-- Bad: Store full SSN when last 4 digits suffice
ssn TEXT

-- Good: Store only what's needed
ssn_last_4 TEXT
```

## Related Documentation

- **[Compliance](../../concepts/compliance.md)** - Compliance architecture
- **[Encryption](encryption.md)** - Encrypting sensitive data
- **[Multi-tenancy](../../concepts/multitenancy.md)** - Tenant isolation

---

**Key Takeaway:** Data classification helps you understand what data you have, how sensitive it is, and what controls are needed. Start classifying from day one.
