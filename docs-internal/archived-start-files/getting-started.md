---
title: "Getting Started with Kimberlite"
section: "start"
slug: "getting-started"
order: 1
---

# Getting Started with Kimberlite

A 5-minute quickstart to explore Kimberlite's core concepts through hands-on experience.

## Prerequisites

- Rust 1.88 or later
- 10 MB disk space

## Installation

```bash
# Install from source (crates.io coming soon)
git clone https://github.com/kimberlitedb/kimberlite.git
cd kimberlite
cargo build --release

# Binary will be at: target/release/kimberlite
```

## Initialize a Development Database

```bash
# Create a new database directory
./target/release/kimberlite init ./data

# Output shows:
# ✓ Created data directory: ./data
# ✓ Initialized append-only log
# ✓ Generated hash chain seed
# ✓ Database ready
```

## Start the Server

```bash
./target/release/kimberlite start ./data

# Server starts on 127.0.0.1:5432
# Press Ctrl+C to stop
```

## Your First Query (REPL)

In a new terminal, start the REPL:

```bash
./target/release/kimberlite repl --address 127.0.0.1:5432 --tenant 1
```

Now try these commands:

```sql
-- Create a table
CREATE TABLE patients (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Insert data
INSERT INTO patients (id, name) VALUES (1, 'Alice');
INSERT INTO patients (id, name) VALUES (2, 'Bob');

-- Query current state
SELECT * FROM patients;
-- Returns:
-- | id | name  | created_at          |
-- |----|-------|---------------------|
-- | 1  | Alice | 2026-02-03 10:30:15 |
-- | 2  | Bob   | 2026-02-03 10:30:18 |

-- Time-travel query (view state 10 seconds ago)
SELECT * FROM patients AS OF TIMESTAMP '2026-02-03 10:30:16';
-- Returns only Alice (Bob wasn't inserted yet)
-- | id | name  | created_at          |
-- |----|-------|---------------------|
-- | 1  | Alice | 2026-02-03 10:30:15 |
```

## What Just Happened?

### 1. Immutable Append-Only Log

Every write went to an append-only log file at `./data/log.kmb`:

```
[Offset 0] CREATE TABLE patients...
[Offset 1] INSERT INTO patients (1, 'Alice')...
[Offset 2] INSERT INTO patients (2, 'Bob')...
```

**No data is ever modified or deleted** - only new entries are appended.

### 2. Hash-Chained Integrity

Each log entry contains a SHA-256 hash of the previous entry:

```
Entry N: {data, hash(Entry N-1)}
```

This creates a tamper-evident chain - modifying any past entry breaks the chain.

### 3. MVCC (Multi-Version Concurrency Control)

The `AS OF TIMESTAMP` query worked because Kimberlite tracks:
- **When** each row was created (commit timestamp)
- **When** each row was deleted (if deleted)

Queries at time T only see rows where: `created_at <= T AND (deleted_at > T OR deleted_at IS NULL)`

### 4. CRC32 Checksums

Every log entry has a CRC32 checksum. On startup, Kimberlite verifies:
```
✓ Verified 3 log entries (0 corruption detected)
✓ Hash chain valid from offset 0 to 2
```

Corrupted entries are detected immediately.

## Next Steps

### Learn the Architecture

Read [Architecture](../concepts/architecture.md) to understand:
- Functional Core / Imperative Shell (FCIS) pattern
- Pure state machine design
- VSR consensus protocol (for clustering)

### Explore Pressurecraft

Kimberlite is built with a **teaching-first** philosophy:

- [Pressurecraft](../concepts/pressurecraft.md) - Our code quality standards
- [Assertions](../internals/testing/assertions.md) - Why we have 2+ assertions per function
- [Testing Overview](../internals/testing/overview.md) - Deterministic simulation testing with VOPR

### Try More SQL

```sql
-- Create audit trail view
CREATE VIEW patient_audit AS
SELECT * FROM patients FOR SYSTEM_TIME ALL;

-- See all versions (including deleted rows)
SELECT * FROM patient_audit;

-- Update a row (creates new version)
UPDATE patients SET name = 'Alice Smith' WHERE id = 1;

-- Query shows both versions
SELECT * FROM patient_audit WHERE id = 1;
```

### Deploy in Production

- [Deployment](../operating/deployment.md) - Production configuration
- [VSR Consensus](../internals/vsr.md) - Multi-node clustering with VSR
- [Compliance](../concepts/compliance.md) - HIPAA, SOC 2, GDPR compliance features

## Get Help

- **Discord:** https://discord.gg/QPChWYjD - Community support and discussions
- **Issues:** https://github.com/kimberlitedb/kimberlite/issues - Bug reports
- **Discussions:** https://github.com/kimberlitedb/kimberlite/discussions - Questions and ideas

## What Makes Kimberlite Different?

Traditional databases optimize for:
- **Fast writes** (in-place updates)
- **Low storage** (overwrite old data)
- **High throughput** (eventual consistency)

Kimberlite optimizes for:
- **Auditability** (immutable history)
- **Correctness** (strong consistency, deterministic)
- **Compliance** (tamper-evidence, time-travel queries)

Trade-off: Storage grows with history (use retention policies to prune old data).

---

**Ready to explore?** Join our Discord to share what you're building!
