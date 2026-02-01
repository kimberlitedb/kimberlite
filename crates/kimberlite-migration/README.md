# kimberlite-migration

SQL migration system for Kimberlite database with file-based tracking, checksums, and tamper detection.

## Features

- **SQL-based migrations** - Write migrations in standard SQL
- **Auto-numbering** - Sequential migration IDs (0001, 0002, etc.)
- **Checksum validation** - SHA-256 checksums prevent tampering
- **Lock file** - Detects modifications to applied migrations
- **File-based tracking** - Simple TOML files for migration state
- **No custom DSL** - Just SQL, no new language to learn

## Quick Start

```bash
# Create a new migration
kmb migration create add_users_table

# Check migration status
kmb migration status

# Validate migrations (checksums, sequence)
kmb migration validate

# Apply pending migrations
kmb migration apply
```

## Migration File Format

Migration files use a simple comment-based metadata format:

```sql
-- Migration: Add users table
-- Created: 2026-02-01T10:00:00Z
-- Author: alice@example.com

-- Up Migration
CREATE TABLE users (
    id BIGINT NOT NULL,
    name TEXT NOT NULL,
    email TEXT,
    PRIMARY KEY (id)
);

CREATE INDEX users_email ON users(email);

-- Down Migration (optional, for rollback)
-- DROP INDEX users_email;
-- DROP TABLE users;
```

### Filename Convention

Migrations use zero-padded sequential IDs:
- `0001_add_users.sql`
- `0002_add_posts.sql`
- `0003_add_comments.sql`

## Directory Structure

```
project/
├── migrations/          # SQL migration files
│   ├── 0001_initial.sql
│   ├── 0002_add_users.sql
│   └── 0003_add_posts.sql
└── .kimberlite/
    └── migrations/
        ├── .lock        # Lock file with checksums
        └── applied.toml # Tracker state
```

## API Usage

```rust
use kimberlite_migration::{MigrationConfig, MigrationManager};
use std::path::PathBuf;

// Initialize manager
let config = MigrationConfig {
    migrations_dir: PathBuf::from("migrations"),
    state_dir: PathBuf::from(".kimberlite/migrations"),
    auto_timestamp: true,
};

let manager = MigrationManager::new(config)?;

// Create a migration
let file = manager.create("add_users_table")?;
println!("Created: {}", file.path.display());

// List pending migrations
let pending = manager.list_pending()?;
for migration in pending {
    println!("Pending: {} - {}", migration.migration.id, migration.migration.name);
}

// Validate checksums and sequence
manager.validate()?;
```

## Migration Workflow

### 1. Create Migration

```bash
$ kmb migration create add_patients_table

Creating migration add_patients_table in project .
Migration ID: 1
File: ./migrations/0001_add_patients_table.sql

Edit the file to add your SQL migration, then run:
  kmb migration apply
```

### 2. Edit Migration File

Edit `migrations/0001_add_patients_table.sql`:

```sql
-- Migration: Add patients table
-- Created: 2026-02-01T10:00:00Z

CREATE TABLE patients (
    id BIGINT NOT NULL,
    name TEXT NOT NULL,
    dob DATE,
    PRIMARY KEY (id)
);
```

### 3. Check Status

```bash
$ kmb migration status

Migration Status
┌────┬─────────────────────┬─────────┬──────────┐
│ ID ┆ Name                ┆ Status  ┆ Checksum │
╞════╪═════════════════════╪═════════╪══════════╡
│ 1  ┆ add_patients_table  ┆ Pending ┆ a1b2c3d4 │
└────┴─────────────────────┴─────────┴──────────┘

Applied: 0 | Pending: 1
```

### 4. Apply Migrations

```bash
$ kmb migration apply

Applying pending migrations in .

Pending migrations:
  1 add_patients_table

✓ Applied 1 add_patients_table

✓ Applied 1 migration(s)
```

### 5. Validate Integrity

```bash
$ kmb migration validate

Validating migrations in .

Validation checks:
  ✓ File checksums match lock file
  ✓ Migration sequence is continuous
  ✓ No gaps in migration IDs
```

## Configuration

### Project Config (`kimberlite.toml`)

```toml
[migrations]
directory = "migrations"
auto_timestamp = true
```

### Migration Manager Config

```rust
use kimberlite_migration::MigrationConfig;

let config = MigrationConfig {
    migrations_dir: PathBuf::from("migrations"),
    state_dir: PathBuf::from(".kimberlite/migrations"),
    auto_timestamp: true, // Add timestamps to filenames
};
```

## Lock File

The lock file (`.kimberlite/migrations/.lock`) stores SHA-256 checksums to detect tampering:

```toml
version = 1

[[migration]]
id = 1
name = "add_users_table"
checksum = "a1b2c3d4e5f6..."

[[migration]]
id = 2
name = "add_posts_table"
checksum = "b2c3d4e5f6a1..."
```

### Checksum Validation

If a migration file is modified after being applied, validation will fail:

```bash
$ kmb migration validate

Error: Checksum mismatch for migration 1: expected a1b2c3d4, found b2c3d4e5
```

## Tracker State

Applied migrations are tracked in `applied.toml`:

```toml
[[migrations]]
id = 1
name = "add_users_table"
checksum = "a1b2c3d4e5f6..."
applied_at = "2026-02-01T10:30:00Z"
applied_by = "alice"
```

## Error Handling

### Invalid Migration Name

```rust
let result = manager.create("invalid/name");
// Error: Invalid migration name: invalid/name
```

### Sequence Gap

```rust
// Missing 0002_*.sql
manager.validate()?;
// Error: Invalid migration sequence: expected 2, found 3
```

### Tampered Migration

```rust
// File modified after application
manager.validate()?;
// Error: Checksum mismatch for migration 1
```

## Testing

```bash
# Run migration system tests
cargo test -p kimberlite-migration

# Run specific test
cargo test -p kimberlite-migration test_create_migration
```

## Future Enhancements

- [ ] SQL execution integration with kimberlite_client
- [ ] Rollback support (DOWN migrations)
- [ ] Migration templates (CRUD, indexes, etc.)
- [ ] Dry-run mode
- [ ] Migration dependencies
- [ ] Data migrations (not just schema)
- [ ] Migration squashing
- [ ] Cloud sync for migrations

## Contributing

Follow the Kimberlite contribution guidelines in `CLAUDE.md`.

## License

Apache-2.0
