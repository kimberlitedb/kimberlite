---
title: "First Application"
section: "start"
slug: "first-app"
order: 3
---

# First Application

Build a simple healthcare compliance application using the Kimberlite Rust SDK.

## What You'll Build

A patient record system that:
- Stores patient records with HIPAA data classification
- Creates an immutable audit trail for every access
- Queries historical records via time travel

## Prerequisites

- `kmb` installed — see [Installation](installation.md)
- Rust 1.88+ installed

## Step 1: Start the Database

```bash
kmb dev
```

## Step 2: Create Your Project

```bash
cargo new patient-records
cd patient-records
```

Add Kimberlite to `Cargo.toml`:

```toml
[dependencies]
kimberlite-client = "0.4"
anyhow = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## Step 3: Write the Application

Replace `src/main.rs`:

```rust
use anyhow::Result;
use kimberlite_client::{Client, ClientConfig};
use kimberlite_types::TenantId;

fn main() -> Result<()> {
    // Connect to the local dev server
    let config = ClientConfig::default();
    let mut client = Client::connect("127.0.0.1:5432", TenantId::new(1), config)?;

    // Create the patients table
    client.query(
        "CREATE TABLE IF NOT EXISTS patients (
            id BIGINT,
            name TEXT,
            dob TEXT,
            diagnosis TEXT,
            PRIMARY KEY (id)
        )",
        &[],
    )?;

    println!("✓ Table created");

    // Insert patient records
    client.query(
        "INSERT INTO patients VALUES (1, 'Jane Doe', '1980-01-15', 'Hypertension')",
        &[],
    )?;
    client.query(
        "INSERT INTO patients VALUES (2, 'John Smith', '1992-07-22', 'Diabetes Type 2')",
        &[],
    )?;

    println!("✓ Patient records inserted");

    // Query all patients
    let result = client.query("SELECT id, name, dob FROM patients ORDER BY id", &[])?;

    println!("\nPatient Records:");
    println!("{:-<40}", "");
    for row in &result.rows {
        let id = &row[0];
        let name = &row[1];
        let dob = &row[2];
        println!("{:<5} {:<20} {}", id, name, dob);
    }
    println!("{} patients found", result.rows.len());

    // Query a single patient by ID (point lookup — O(1))
    let single = client.query("SELECT * FROM patients WHERE id = 1", &[])?;
    println!("\nRecord for patient 1:");
    if let Some(row) = single.rows.first() {
        for (col, val) in result.columns.iter().zip(row) {
            println!("  {}: {}", col, val);
        }
    }

    println!("\n✓ Application complete");
    Ok(())
}
```

## Step 4: Run the Application

```bash
cargo run
```

Expected output:

```
✓ Table created
✓ Patient records inserted

Patient Records:
----------------------------------------
1     Jane Doe             1980-01-15
2     John Smith           1992-07-22
2 patients found

Record for patient 1:
  id: 1
  name: Jane Doe
  ...

✓ Application complete
```

## Step 5: View the Audit Trail in Studio

Open [http://localhost:5555](http://localhost:5555) and click the Studio tab.

Every insert and query is recorded in the immutable log. You can:
- Browse all data in the schema explorer
- Execute SQL queries interactively
- Use the time-travel slider to see the database at any past state

## Next Steps

- **[Compliance Guide](../coding/compliance-quickstart.md)** — Add HIPAA data classification
- **[RBAC Guide](../concepts/rbac.md)** — Role-based access control
- **[Data Masking](../concepts/masking.md)** — Mask PHI fields automatically
- **[Python SDK](../reference/sdk-python.md)** — Use Kimberlite from Python
