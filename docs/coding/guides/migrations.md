---
title: "Schema Migrations"
section: "coding/guides"
slug: "migrations"
order: 1
---

# Schema Migrations

How to evolve your database schema over time in Kimberlite.

## Overview

Kimberlite uses SQL-based migrations to version and apply schema changes. Migrations are:

- **Sequential:** Applied in order (001, 002, 003, ...)
- **Immutable:** Once applied, cannot be changed
- **Tracked:** System records which migrations have been applied
- **Reversible:** Can include rollback logic (optional)

## Creating Migrations

### Using the CLI (Future)

Once the `kmb` CLI is available (v0.6.0):

```bash
# Create a new migration
kmb migration create add_appointments_table

# This creates: migrations/0001_add_appointments_table.sql
```

### Manual Creation (Current)

For now, create migration files manually:

```bash
# Create migrations directory
mkdir -p migrations

# Create first migration
touch migrations/0001_initial_schema.sql
```

## Migration File Format

Migration files are SQL scripts:

```sql
-- migrations/0001_initial_schema.sql

-- Create patients table
CREATE TABLE patients (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    date_of_birth DATE,
    medical_record_number TEXT UNIQUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Create index for faster lookups
CREATE INDEX patients_dob_idx ON patients(date_of_birth);

-- Create appointments table
CREATE TABLE appointments (
    id BIGINT PRIMARY KEY,
    patient_id BIGINT NOT NULL,
    appointment_date TIMESTAMP NOT NULL,
    provider TEXT,
    status TEXT DEFAULT 'scheduled',
    notes TEXT
);

CREATE INDEX appointments_patient_id_idx ON appointments(patient_id);
CREATE INDEX appointments_date_idx ON appointments(appointment_date);
```

## Applying Migrations

### Automatic (Development)

The development server applies migrations automatically:

```bash
# Future: kmb dev
# Automatically applies pending migrations on startup
```

### Manual (Current)

For now, apply migrations manually:

```rust,ignore
use kimberlite::Client;

fn apply_migration(client: &Client, migration_sql: &str) -> Result<()> {
    client.execute(migration_sql)?;
    Ok(())
}
```

## Migration Naming Convention

Use descriptive names with sequence numbers:

```text
migrations/
├── 0001_initial_schema.sql
├── 0002_add_billing_tables.sql
├── 0003_add_audit_triggers.sql
├── 0004_add_patient_consents.sql
└── 0005_add_encryption_keys.sql
```

**Pattern:** `{sequence}_{description}.sql`

- **Sequence:** 4-digit number (0001, 0002, ...)
- **Description:** snake_case, descriptive
- **Extension:** `.sql`

## Best Practices

### 1. One Migration Per Logical Change

```bash
# Good: One migration for related changes
0001_add_patients_table.sql

# Bad: Multiple unrelated changes
0001_add_everything.sql
```

### 2. Always Add, Never Modify

```sql
-- Good: New migration to add column
-- migrations/0002_add_email_to_patients.sql
ALTER TABLE patients ADD COLUMN email TEXT;

-- Bad: Editing 0001 to add email (breaks existing deployments)
```

### 3. Test Migrations on Copy of Production Data

```bash
# 1. Export production data
kmb export --tenant 1 --output prod_data.sql

# 2. Import to test environment
kmb import --tenant 1 --input prod_data.sql

# 3. Apply migration
kmb migration apply 0002_add_email_to_patients.sql

# 4. Verify
kmb query "SELECT COUNT(*) FROM patients WHERE email IS NOT NULL"
```

### 4. Include Rollback Logic (Optional)

```sql
-- migrations/0002_add_email_to_patients.sql

-- UP: Apply changes
ALTER TABLE patients ADD COLUMN email TEXT;

-- DOWN: Rollback changes (commented out, for reference)
-- ALTER TABLE patients DROP COLUMN email;
```

### 5. Use Transactions (Future)

Once supported:

```sql
BEGIN;

-- Migration changes
ALTER TABLE patients ADD COLUMN email TEXT;
CREATE INDEX patients_email_idx ON patients(email);

COMMIT;
```

## Migration State Tracking

Kimberlite tracks applied migrations in a system table:

```sql
-- System table (automatically created)
CREATE TABLE __migrations (
    id BIGINT PRIMARY KEY,
    version TEXT NOT NULL,
    name TEXT NOT NULL,
    applied_at TIMESTAMP NOT NULL,
    checksum TEXT NOT NULL
);
```

**Query migration status:**

```sql
-- See which migrations have been applied
SELECT * FROM __migrations ORDER BY id;

-- Check if specific migration applied
SELECT * FROM __migrations WHERE name = '0001_initial_schema';
```

## Common Patterns

### Adding a Table

```sql
-- migrations/0003_add_prescriptions.sql
CREATE TABLE prescriptions (
    id BIGINT PRIMARY KEY,
    patient_id BIGINT NOT NULL,
    medication TEXT NOT NULL,
    dosage TEXT,
    prescribed_date TIMESTAMP NOT NULL,
    prescriber TEXT NOT NULL
);

CREATE INDEX prescriptions_patient_id_idx ON prescriptions(patient_id);
```

### Adding a Column

```sql
-- migrations/0004_add_phone_to_patients.sql
ALTER TABLE patients ADD COLUMN phone TEXT;
```

### Adding an Index

```sql
-- migrations/0005_add_name_index.sql
CREATE INDEX patients_name_idx ON patients(name);
```

### Adding Constraints

```sql
-- migrations/0006_add_patient_constraints.sql
ALTER TABLE appointments
ADD CONSTRAINT appointments_patient_fk
FOREIGN KEY (patient_id) REFERENCES patients(id);
```

## Data Migrations

For data transformations:

```sql
-- migrations/0007_normalize_phone_numbers.sql

-- Update existing phone numbers to normalized format
UPDATE patients
SET phone = REPLACE(REPLACE(REPLACE(phone, '-', ''), '(', ''), ')', '')
WHERE phone IS NOT NULL;
```

**Warning:** Data migrations can be slow on large tables. Consider:
- Running during off-peak hours
- Batching updates
- Using background jobs

## Multi-Tenant Migrations

Migrations apply to all tenants:

```sql
-- This migration applies to ALL tenants
CREATE TABLE prescriptions (
    id BIGINT PRIMARY KEY,
    patient_id BIGINT NOT NULL,
    medication TEXT NOT NULL
);
```

**Per-tenant differences** are handled via tenant configuration, not separate schemas.

## Handling Failures

If a migration fails:

1. **Check the error:** `kmb migration status`
2. **Fix the issue:** Correct the SQL
3. **Mark as failed:** `kmb migration mark-failed 0003`
4. **Reapply:** `kmb migration apply 0003_fixed.sql`

**Never modify a migration after it's been applied to production.**

## Version Control

Commit migrations to version control:

```bash
git add migrations/0003_add_prescriptions.sql
git commit -m "feat: Add prescriptions table"
git push
```

**All environments** (dev, staging, production) should have identical migration history.

## Example: Full Migration Workflow

```bash
# 1. Create migration
touch migrations/0003_add_prescriptions.sql

# 2. Write SQL
cat > migrations/0003_add_prescriptions.sql << 'EOF'
CREATE TABLE prescriptions (
    id BIGINT PRIMARY KEY,
    patient_id BIGINT NOT NULL,
    medication TEXT NOT NULL,
    dosage TEXT,
    prescribed_date TIMESTAMP NOT NULL
);

CREATE INDEX prescriptions_patient_id_idx ON prescriptions(patient_id);
EOF

# 3. Test locally
kmb migration apply 0003_add_prescriptions.sql

# 4. Verify
kmb query "SELECT COUNT(*) FROM prescriptions"

# 5. Commit
git add migrations/0003_add_prescriptions.sql
git commit -m "feat: Add prescriptions table"

# 6. Deploy to staging
git push staging main
# (CI/CD applies migrations automatically)

# 7. Deploy to production
git push production main
# (CI/CD applies migrations automatically)
```

## Troubleshooting

### "Migration already applied"

**Cause:** Trying to apply a migration that's already in `__migrations`.

**Solution:** Check status with `SELECT * FROM __migrations`.

### "Checksum mismatch"

**Cause:** Migration file changed after being applied.

**Solution:** Never modify applied migrations. Create a new migration instead.

### "Migration failed: syntax error"

**Cause:** Invalid SQL syntax.

**Solution:** Test migration in development first.

### "Table already exists"

**Cause:** Schema already has the table (possibly from manual changes).

**Solution:** Use `CREATE TABLE IF NOT EXISTS` or reconcile schema manually.

## Related Documentation

- **[First Application](..//docs/start)** - Building your first app
- **[Testing Guide](testing.md)** - Testing migrations
- **[Connection Pooling](connection-pooling.md)** - Database connections

---

**Status:** Current implementation is manual. Full migration tooling coming in v0.6.0.
