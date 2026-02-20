---
title: "Start"
section: "start"
slug: "start"
order: 1
---

# Start

Get Kimberlite running in 2 minutes. This guide shows you how to install Kimberlite, start a cluster, and try compliance features.

## Install

<details open>
<summary>Linux</summary>

```bash
curl -Lo kimberlite.zip https://linux.kimberlite.com
unzip kimberlite.zip
./kimberlite version
```
</details>

<details>
<summary>macOS</summary>

```bash
curl -Lo kimberlite.zip https://macos.kimberlite.com
unzip kimberlite.zip
./kimberlite version
```
</details>

<details>
<summary>Windows</summary>

```powershell
Invoke-WebRequest -Uri https://windows.kimberlite.com -OutFile kimberlite.zip
Expand-Archive kimberlite.zip
.\kimberlite\kimberlite.exe version
```
</details>

<details>
<summary>Build from Source</summary>

Requires Rust 1.88+:

```bash
git clone https://github.com/kimberlitedb/kimberlite.git
cd kimberlite
cargo build --release --bin kimberlite
./target/release/kimberlite version
```
</details>

## Run a Cluster

Create a data file and start a single-node cluster:

```bash
# Format a new data file
./kimberlite format --cluster=0 --replica=0 --replica-count=1 ./0_0.kimber

# Start the cluster
./kimberlite start --addresses=3000 ./0_0.kimber
```

Your cluster is now running on port 3000.

## Create Your First Table

Open the interactive SQL shell:

```bash
./kimberlite repl --addresses=3000
```

Create a table and insert data:

```sql
-- Create a patients table
CREATE TABLE patients (
    id INT PRIMARY KEY,
    name TEXT NOT NULL,
    ssn TEXT NOT NULL,
    diagnosis TEXT
);

-- Insert sample data
INSERT INTO patients VALUES (1, 'Alice Johnson', '123-45-6789', 'Hypertension');
INSERT INTO patients VALUES (2, 'Bob Smith', '987-65-4321', 'Diabetes');

-- Query it
SELECT * FROM patients;
```

Output:
```
 id | name          | ssn         | diagnosis
----+---------------+-------------+--------------
  1 | Alice Johnson | 123-45-6789 | Hypertension
  2 | Bob Smith     | 987-65-4321 | Diabetes
```

## Try Compliance Features

Kimberlite is built for regulated industries. Here are some compliance features you can try right now.

### Data Masking (HIPAA-Compliant)

Mask sensitive fields like SSNs:

```sql
-- Create a masking rule
CREATE MASK ssn_mask ON patients.ssn USING REDACT;

-- Query again - SSN is now masked
SELECT * FROM patients;
```

Output:
```
 id | name          | ssn  | diagnosis
----+---------------+------+--------------
  1 | Alice Johnson | **** | Hypertension
  2 | Bob Smith     | **** | Diabetes
```

### Data Classification

Tag fields with compliance categories:

```sql
-- Classify SSN as PHI (Protected Health Information)
ALTER TABLE patients MODIFY COLUMN ssn SET CLASSIFICATION 'PHI';

-- Classify diagnosis as MEDICAL
ALTER TABLE patients MODIFY COLUMN diagnosis SET CLASSIFICATION 'MEDICAL';

-- View classification metadata
SHOW CLASSIFICATIONS FOR patients;
```

Output:
```
 column    | classification
-----------+----------------
 ssn       | PHI
 diagnosis | MEDICAL
```

### Role-Based Access Control

Create roles with specific permissions:

```sql
-- Create a billing clerk role (can see names, not diagnoses)
CREATE ROLE billing_clerk;
GRANT SELECT (id, name, ssn) ON patients TO billing_clerk;

-- Create a doctor role (full access)
CREATE ROLE doctor;
GRANT SELECT ON patients TO doctor;

-- Assign role to a user
CREATE USER clerk1 WITH ROLE billing_clerk;
```

### Audit Trail (Automatic)

Every change is recorded automatically. View the audit log:

```sql
-- See who did what and when
SELECT * FROM _kimberlite_audit
WHERE table_name = 'patients'
ORDER BY timestamp DESC
LIMIT 5;
```

Output:
```
 timestamp           | user    | operation | table_name | record_id
---------------------+---------+-----------+------------+-----------
 2026-02-11 10:15:32 | admin   | INSERT    | patients   | 2
 2026-02-11 10:15:28 | admin   | INSERT    | patients   | 1
 2026-02-11 10:15:12 | admin   | CREATE    | patients   | NULL
```

### Time-Travel Queries

Query historical state (everything is append-only):

```sql
-- See what the table looked like 5 minutes ago
SELECT * FROM patients AS OF TIMESTAMP '2026-02-11 10:10:00';

-- See all versions of a record
SELECT * FROM patients FOR SYSTEM_TIME ALL WHERE id = 1;
```

### Consent Management

Track user consent for data processing:

```sql
-- Record patient consent
INSERT INTO _kimberlite_consent (subject_id, purpose, granted)
VALUES ('patient:1', 'marketing', true);

-- Query patients who consented to marketing
SELECT p.* FROM patients p
JOIN _kimberlite_consent c ON c.subject_id = CONCAT('patient:', p.id)
WHERE c.purpose = 'marketing' AND c.granted = true;
```

### Right to Erasure (GDPR)

Mark data for deletion (while preserving audit trail):

```sql
-- Anonymize a patient's data (GDPR right to be forgotten)
UPDATE patients SET name = 'REDACTED', ssn = 'REDACTED' WHERE id = 1;

-- The change is logged, so compliance officers can prove deletion
SELECT * FROM _kimberlite_audit WHERE table_name = 'patients' AND record_id = 1;
```

## What Makes Kimberlite Different?

Traditional databases bolt on compliance features as afterthoughts. Kimberlite is built from the ground up for regulated industries:

- **23 compliance frameworks** - HIPAA, SOX, GDPR, PCI-DSS, and 19 more
- **Append-only architecture** - Nothing is ever deleted, perfect audit trails
- **Built-in data classification** - PHI, PII, PCI automatically tagged
- **Time-travel queries** - Query any historical state
- **Field-level masking** - Selective redaction of sensitive data
- **Formal verification** - Safety properties proven with TLA+, Coq, and Alloy
- **Multi-tenant isolation** - Complete data separation per tenant

## Next Steps

### Build an Application

Try our SDK quickstarts:

- [Python](/docs/coding/python) - Build apps with Python
- [TypeScript](/docs/coding/typescript) - Node.js applications
- [Rust](/docs/coding/rust) - Native Rust integration
- [Go](/docs/coding/go) - Go applications

### Explore Features

Learn about Kimberlite's unique capabilities:

- [Compliance](/docs/concepts/compliance) - 23 frameworks explained
- [Data Classification](/docs/concepts/data-classification) - PHI, PII, PCI tags
- [RBAC](/docs/concepts/rbac) - Role-based access control
- [ABAC](/docs/concepts/abac) - Attribute-based policies
- [Multitenancy](/docs/concepts/multitenancy) - Tenant isolation
- [Architecture](/docs/concepts/architecture) - How it works

### Deploy to Production

Run Kimberlite in production:

- [Deployment](/docs/operating/deployment) - Single-node and clusters
- [Monitoring](/docs/operating/monitoring) - Metrics and alerts
- [Security](/docs/operating/security) - Hardening and best practices
- [Performance](/docs/operating/performance) - Tuning for scale

### Command Reference

Learn all available commands:

- [CLI Reference](/docs/reference/cli) - All commands and options
- [SQL Reference](/docs/reference/sql/overview) - SQL dialect and extensions
- [Protocol](/docs/reference/protocol) - Wire protocol specification

## Getting Help

- **Documentation** - Browse [all docs](/docs)
- **Examples** - See the `examples/` directory in the repository
- **Issues** - Report bugs on [GitHub](https://github.com/kimberlitedb/kimberlite/issues)
- **Discussions** - Ask questions on [GitHub Discussions](https://github.com/kimberlitedb/kimberlite/discussions)

---

**Ready to build?** Pick a [language quickstart](/docs/coding) and start coding.
