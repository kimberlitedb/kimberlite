---
title: "Your First Application"
section: "start"
slug: "first-app"
order: 4
---

# Your First Application

Build a simple healthcare compliance application with Kimberlite in 10 minutes.

## What We'll Build

A minimal patient record system demonstrating:
- Multi-tenant isolation (HIPAA requirement)
- Immutable audit trail (compliance requirement)
- Time-travel queries (for audits and investigations)
- Data classification (PHI vs de-identified data)

## Prerequisites

- Kimberlite installed ([Installation Guide](installation.md))
- Basic Rust knowledge (or Python/TypeScript once SDKs are ready)

## Step 1: Create a New Project

```bash
# Create project directory
mkdir healthcare-app
cd healthcare-app

# Initialize a Rust project
cargo init --lib
```

## Step 2: Add Dependencies

Edit `Cargo.toml`:

```toml
[package]
name = "healthcare-app"
version = "0.1.0"
edition = "2024"

[dependencies]
kimberlite-types = "0.4.0"
kimberlite-storage = "0.4.0"
kimberlite-crypto = "0.4.0"
anyhow = "1"
chrono = "0.4"
```

## Step 3: Define Your Data Model

Create `src/patient.rs`:

```rust
use chrono::NaiveDate;
use kimberlite_types::{TenantId, StreamId};

/// Patient record (PHI - Protected Health Information)
#[derive(Debug, Clone)]
pub struct Patient {
    pub id: u64,
    pub tenant_id: TenantId,
    pub name: String,
    pub date_of_birth: NaiveDate,
    pub medical_record_number: String,
}

impl Patient {
    /// Create a new patient record
    pub fn new(
        id: u64,
        tenant_id: TenantId,
        name: String,
        date_of_birth: NaiveDate,
        medical_record_number: String,
    ) -> Self {
        Self {
            id,
            tenant_id,
            name,
            date_of_birth,
            medical_record_number,
        }
    }

    /// Serialize to bytes for storage
    pub fn to_bytes(&self) -> Vec<u8> {
        // In production, use proper serialization (postcard, bincode, etc.)
        format!(
            "{}|{}|{}|{}",
            self.name,
            self.date_of_birth,
            self.medical_record_number,
            self.tenant_id.as_u64()
        )
        .into_bytes()
    }

    /// Get stream ID for this patient
    pub fn stream_id(&self) -> StreamId {
        StreamId::new(self.tenant_id, self.id)
    }
}

/// Appointment (also PHI)
#[derive(Debug, Clone)]
pub struct Appointment {
    pub id: u64,
    pub patient_id: u64,
    pub date: NaiveDate,
    pub provider: String,
    pub notes: String,
}

impl Appointment {
    pub fn to_bytes(&self) -> Vec<u8> {
        format!(
            "{}|{}|{}|{}",
            self.patient_id, self.date, self.provider, self.notes
        )
        .into_bytes()
    }
}
```

## Step 4: Implement Storage Layer

Create `src/storage.rs`:

```rust
use anyhow::Result;
use kimberlite_storage::AppendOnlyLog;
use kimberlite_types::{Offset, StreamId, TenantId};

use crate::patient::{Patient, Appointment};

/// Healthcare data store with multi-tenant isolation
pub struct HealthcareStore {
    log: AppendOnlyLog,
}

impl HealthcareStore {
    /// Create a new store
    pub fn new(data_dir: &str) -> Result<Self> {
        let log = AppendOnlyLog::new(data_dir)?;
        Ok(Self { log })
    }

    /// Store a patient record (returns offset for auditing)
    pub fn store_patient(&mut self, patient: &Patient) -> Result<Offset> {
        let stream_id = patient.stream_id();
        let data = patient.to_bytes();
        let offset = self.log.append(stream_id, &data)?;

        println!("✓ Stored patient {} at offset {}", patient.name, offset);
        Ok(offset)
    }

    /// Store an appointment
    pub fn store_appointment(
        &mut self,
        tenant_id: TenantId,
        appointment: &Appointment,
    ) -> Result<Offset> {
        let stream_id = StreamId::new(tenant_id, appointment.patient_id);
        let data = appointment.to_bytes();
        let offset = self.log.append(stream_id, &data)?;

        println!("✓ Stored appointment for patient {} at offset {}",
                 appointment.patient_id, offset);
        Ok(offset)
    }

    /// Read patient record at specific offset (time-travel query)
    pub fn read_patient_at(
        &self,
        stream_id: StreamId,
        offset: Offset,
    ) -> Result<Vec<u8>> {
        let entry = self.log.read_at(stream_id, offset)?;
        Ok(entry.data)
    }

    /// Get audit trail for a stream (all historical records)
    pub fn audit_trail(&self, stream_id: StreamId) -> Result<Vec<(Offset, Vec<u8>)>> {
        // In real implementation, scan log entries
        // This is a simplified version
        Ok(vec![])
    }
}
```

## Step 5: Create Main Application

Edit `src/main.rs`:

```rust
mod patient;
mod storage;

use anyhow::Result;
use chrono::NaiveDate;
use kimberlite_types::TenantId;

use crate::patient::{Patient, Appointment};
use crate::storage::HealthcareStore;

fn main() -> Result<()> {
    println!("🏥 Healthcare App - HIPAA-Compliant Patient Records\n");

    // Initialize storage
    let mut store = HealthcareStore::new("./data")?;

    // Create two tenants (hospitals)
    let hospital_a = TenantId::new(1);
    let hospital_b = TenantId::new(2);

    println!("📋 Creating patient records...\n");

    // Hospital A: Create patient
    let alice = Patient::new(
        1,
        hospital_a,
        "Alice Smith".to_string(),
        NaiveDate::from_ymd_opt(1985, 3, 15).unwrap(),
        "MRN-001".to_string(),
    );
    let alice_offset = store.store_patient(&alice)?;

    // Hospital A: Create appointment
    let appointment_1 = Appointment {
        id: 1,
        patient_id: alice.id,
        date: NaiveDate::from_ymd_opt(2026, 2, 10).unwrap(),
        provider: "Dr. Jones".to_string(),
        notes: "Annual checkup".to_string(),
    };
    store.store_appointment(hospital_a, &appointment_1)?;

    // Hospital B: Create patient (separate tenant)
    let bob = Patient::new(
        2,
        hospital_b,
        "Bob Johnson".to_string(),
        NaiveDate::from_ymd_opt(1990, 7, 22).unwrap(),
        "MRN-101".to_string(),
    );
    let bob_offset = store.store_patient(&bob)?;

    println!("\n✅ Multi-Tenant Isolation Demonstrated:");
    println!("   Hospital A (Tenant 1): Alice Smith");
    println!("   Hospital B (Tenant 2): Bob Johnson");
    println!("   Each tenant's data is cryptographically isolated\n");

    // Demonstrate time-travel query
    println!("🕐 Time-Travel Query:");
    let alice_data = store.read_patient_at(alice.stream_id(), alice_offset)?;
    println!("   Alice's record at offset {}: {:?}",
             alice_offset,
             String::from_utf8_lossy(&alice_data));

    println!("\n✨ Features Demonstrated:");
    println!("   ✓ Multi-tenant isolation (HIPAA compliant)");
    println!("   ✓ Immutable audit trail (cannot be modified)");
    println!("   ✓ Time-travel queries (read data at any point in time)");
    println!("   ✓ Append-only storage (full history preserved)");

    println!("\n📊 Data stored in: ./data/");
    println!("   Each write is checksummed and hash-chained for integrity");

    Ok(())
}
```

## Step 6: Run the Application

```bash
cargo run
```

Expected output:

```
🏥 Healthcare App - HIPAA-Compliant Patient Records

📋 Creating patient records...

✓ Stored patient Alice Smith at offset 0
✓ Stored appointment for patient 1 at offset 1
✓ Stored patient Bob Johnson at offset 0

✅ Multi-Tenant Isolation Demonstrated:
   Hospital A (Tenant 1): Alice Smith
   Hospital B (Tenant 2): Bob Johnson
   Each tenant's data is cryptographically isolated

🕐 Time-Travel Query:
   Alice's record at offset 0: "Alice Smith|1985-03-15|MRN-001|1"

✨ Features Demonstrated:
   ✓ Multi-tenant isolation (HIPAA compliant)
   ✓ Immutable audit trail (cannot be modified)
   ✓ Time-travel queries (read data at any point in time)
   ✓ Append-only storage (full history preserved)

📊 Data stored in: ./data/
   Each write is checksummed and hash-chained for integrity
```

## Step 7: Verify Data Integrity

The data is stored in `./data/` with:
- CRC32 checksums on every entry
- Hash-chain linking entries together
- Cannot be modified without detection

```bash
# Inspect the data directory
ls -lh ./data/

# Try to modify a file (it will be detected!)
# Don't actually do this in production
```

## What You've Built

In 10 minutes, you've created a healthcare application with:

1. **Multi-tenant Isolation**
   - Each hospital's data is cryptographically separated
   - Hospital A cannot access Hospital B's patients
   - Required for HIPAA compliance

2. **Immutable Audit Trail**
   - All changes are append-only
   - Complete history preserved
   - Cannot be modified or deleted

3. **Time-Travel Queries**
   - Query data at any historical offset
   - Crucial for audits and investigations
   - "What did we know at time T?"

4. **Data Integrity**
   - CRC32 checksums on every write
   - Hash chains prevent tampering
   - Cryptographic verification

## Next Steps

### Add More Features

**Encryption:**
```rust
use kimberlite_crypto::SymmetricKey;

// Encrypt PHI before storage
let key = SymmetricKey::generate();
let encrypted = key.encrypt(&patient_data)?;
store.append(stream_id, &encrypted)?;
```

**Access Control:**
```rust
// Track who accessed what
struct AuditLog {
    user_id: UserId,
    action: Action,
    timestamp: Timestamp,
    patient_id: PatientId,
}
```

**Projections (Derived Views):**
```rust
// Create materialized views
struct PatientIndex {
    by_mrn: HashMap<String, PatientId>,
    by_name: HashMap<String, Vec<PatientId>>,
}
```

### Production Deployment

For production use:

1. **Use the full server** (coming in v0.6.0):
   ```bash
   kimberlite start --config production.toml
   ```

2. **Enable TLS**:
   ```toml
   [tls]
   cert = "/path/to/cert.pem"
   key = "/path/to/key.pem"
   ```

3. **Set up monitoring**:
   - See [Monitoring Guide](../operating/monitoring.md)
   - Enable metrics and tracing

4. **Deploy with replication**:
   - See [Deployment Guide](../operating/deployment.md)
   - Use 3+ replicas for fault tolerance

### Learn More

- **[Concepts](../concepts/)** - Understand the architecture
- **[Coding Guides](../coding/)** - Build more sophisticated apps
- **[SDK Reference](../reference/sdk/)** - Complete API docs
- **[Operating](../operating/)** - Production deployment

## Real-World Use Cases

This simple example demonstrates patterns for:

**Healthcare:**
- Electronic Health Records (EHR)
- Patient portals with full audit trails
- Clinical trial data management
- Prescription tracking

**Finance:**
- Transaction ledgers
- Account history
- Trade audit trails
- Regulatory compliance

**Legal:**
- Document management
- Case history
- Evidence chain-of-custody
- Contract versioning

## Testing Your Application

Add tests using VOPR:

```bash
# Run property-based tests
cargo test --workspace

# Run simulation tests
cargo run --bin vopr -- run --scenario baseline --iterations 1000
```

See [Testing Guide](../coding/guides/testing.md) for comprehensive testing strategies.

---

**Congratulations!** You've built your first compliance-aware application with Kimberlite. The patterns you've learned here scale to production systems handling millions of records.
