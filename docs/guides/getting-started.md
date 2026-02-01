# Getting Started with Kimberlite

Welcome to Kimberlite - the compliance-first database for regulated industries! This guide will have you up and running in less than 5 minutes.

## What is Kimberlite?

Kimberlite is a database built specifically for healthcare, finance, and legal industries. It provides:

- **Immutability**: All data is append-only, creating a complete audit trail
- **Multi-tenancy**: Strong isolation between tenants for HIPAA/GDPR compliance
- **Time-travel queries**: Query data at any point in history
- **Built-in compliance**: Audit logs, encryption, and access controls

## Prerequisites

- Rust 1.88+ (install from [rustup.rs](https://rustup.rs))
- Basic familiarity with SQL
- A terminal/command line

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/kimberlite/kimberlite.git
cd kimberlite

# Build the CLI
cargo build --release

# Add to PATH
export PATH="$PWD/target/release:$PATH"
```

### Verify Installation

```bash
kmb --version
# Output: kmb 0.1.0
```

## Quick Start

### 1. Initialize a Project

```bash
# Create a new directory for your project
mkdir my-healthcare-app
cd my-healthcare-app

# Initialize Kimberlite
kmb init
```

This creates:
- `kimberlite.toml` - Project configuration
- `migrations/` - SQL migration files
- `.kimberlite/` - Local state directory (gitignored)

### 2. Start the Development Server

```bash
kmb dev
```

This single command:
- ✅ Starts the database server
- ✅ Launches Studio UI at http://127.0.0.1:5555
- ✅ Applies pending migrations
- ✅ Sets up auto-reload

You should see:

```
┌─────────────────────────────────────────────────────┐
│ Kimberlite Development Server                       │
└─────────────────────────────────────────────────────┘

✓ Config loaded from kimberlite.toml
✓ Database started on 127.0.0.1:5432
✓ Studio started on http://127.0.0.1:5555

Ready! Press Ctrl+C to stop all services.
```

### 3. Create Your First Tenant

Kimberlite is multi-tenant by default. Let's create a tenant:

```bash
# In a new terminal
kmb tenant create --id 1 --name "Development"
```

### 4. Connect with the REPL

```bash
kmb repl --tenant 1
```

You're now in an interactive SQL session:

```sql
kimberlite[tenant:1]> CREATE TABLE patients (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    dob DATE,
    mrn TEXT UNIQUE
);

kimberlite[tenant:1]> INSERT INTO patients VALUES
    (1, 'Alice Smith', '1985-03-15', 'MRN-001');

kimberlite[tenant:1]> SELECT * FROM patients;
┌────┬─────────────┬────────────┬─────────┐
│ id │ name        │ dob        │ mrn     │
├────┼─────────────┼────────────┼─────────┤
│ 1  │ Alice Smith │ 1985-03-15 │ MRN-001 │
└────┴─────────────┴────────────┴─────────┘
```

### 5. Use Studio UI

Open http://127.0.0.1:5555 in your browser. You'll see:

- **Query Editor**: Write and execute SQL queries
- **Schema Browser**: Explore tables and columns
- **Time-Travel**: Query data at any historical offset
- **Tenant Selector**: Switch between tenants

Try running a query in Studio:

```sql
SELECT * FROM patients WHERE dob > '1980-01-01';
```

## Common Workflows

### Creating Migrations

Migrations are SQL files that define schema changes:

```bash
# Create a new migration
kmb migration create add_appointments_table

# Edit the generated file
vim migrations/0001_add_appointments_table.sql
```

```sql
-- Migration: Add appointments table
CREATE TABLE appointments (
    id BIGINT PRIMARY KEY,
    patient_id BIGINT NOT NULL,
    appointment_date TIMESTAMP NOT NULL,
    status TEXT
);

CREATE INDEX appointments_patient_id ON appointments(patient_id);
```

```bash
# Apply the migration
kmb migration apply
```

### Time-Travel Queries

One of Kimberlite's unique features is querying historical data:

```bash
# Query at specific offset
kmb query "SELECT * FROM patients" --tenant 1 --at 0
```

In Studio, use the time-travel slider to explore data at different points in time.

### Multi-Tenant Queries

Each tenant's data is completely isolated:

```bash
# Create another tenant
kmb tenant create --id 2 --name "Tenant Two"

# Insert data in tenant 2
kmb query "INSERT INTO patients VALUES (2, 'Bob Jones', '1990-05-20', 'MRN-002')" --tenant 2

# Tenant 1 cannot see Tenant 2's data
kmb query "SELECT * FROM patients" --tenant 1
# Returns only Alice Smith

kmb query "SELECT * FROM patients" --tenant 2
# Returns only Bob Jones
```

### Production Deployment

For production, use `kmb start` instead of `kmb dev`:

```bash
# Start in production mode
kmb start /var/lib/kimberlite --address 0.0.0.0:5432
```

## Configuration

Edit `kimberlite.toml` to customize your setup:

```toml
[project]
name = "my-healthcare-app"

[database]
bind_address = "127.0.0.1:5432"

[development]
studio = true
auto_migrate = true

[replication]
mode = "single-node"  # or "cluster"
```

See [Configuration Guide](./configuration.md) for all options.

## Testing

### Local Multi-Node Cluster

Test distributed scenarios locally:

```bash
# Initialize 3-node cluster
kmb cluster init --nodes 3

# Start all nodes
kmb cluster start

# Check status
kmb cluster status

# Cleanup
kmb cluster destroy
```

### Simulation Testing (VOPR)

Run deterministic simulations to find bugs:

```bash
# Run 1000 simulations
kmb sim run --iterations 1000

# Verify specific seed
kmb sim verify --seed 12345

# Generate report
kmb sim report --output results.html
```

## Next Steps

- **[Migration Guide](./migration-guide.md)**: Migrating from the old CLI
- **[Shell Completions](./shell-completions.md)**: Set up tab completion
- **[Architecture Overview](../ARCHITECTURE.md)**: Understanding Kimberlite internals
- **[CLAUDE.md](../CLAUDE.md)**: Building and testing from source

## Common Issues

### Port Already in Use

**Error**: `Address already in use (os error 48)`

**Solution**: Another process is using port 5432. Either stop that process or use a custom port:

```bash
kmb dev --port 5433
```

### Tenant Not Found

**Error**: `Tenant 1 not found`

**Solution**: Create the tenant first:

```bash
kmb tenant create --id 1 --name "Development"
```

### Migration Checksum Mismatch

**Error**: `Migration checksum mismatch`

**Solution**: Don't edit applied migrations. Create a new migration instead:

```bash
kmb migration create fix_previous_migration
```

### Studio Not Loading

**Problem**: Studio shows "Connection refused"

**Solution**: Ensure `kmb dev` is running and check the Studio URL in the terminal output.

## Getting Help

- **Documentation**: This guide and others in `docs/`
- **CLI Help**: Run `kmb help <command>` for detailed command info
- **Issues**: Report bugs at https://github.com/kimberlite/kimberlite/issues
- **Discussions**: Ask questions in GitHub Discussions

## Cheat Sheet

```bash
# Project setup
kmb init                    # Initialize project
kmb dev                     # Start dev environment

# Tenants
kmb tenant create --id 1 --name "Dev"
kmb tenant list
kmb tenant info --id 1

# Queries
kmb repl --tenant 1
kmb query "SELECT * FROM patients" --tenant 1
kmb query "..." --tenant 1 --at 100  # Time-travel

# Migrations
kmb migration create <name>
kmb migration apply
kmb migration status

# Cluster testing
kmb cluster init --nodes 3
kmb cluster start
kmb cluster status

# Simulations
kmb sim run --iterations 1000
kmb sim verify --seed 12345

# Config
kmb config show
kmb config set database.bind_address "0.0.0.0:5432"

# Completions
kmb completion bash > ~/.local/share/bash-completion/completions/kmb
```

## What's Next?

You're now ready to build compliance-first applications with Kimberlite! Here are some ideas:

1. **Healthcare**: Build a HIPAA-compliant patient record system
2. **Finance**: Create an audit-trail for financial transactions
3. **Legal**: Store tamper-proof legal documents with full history

Explore the documentation to learn more about Kimberlite's unique features like:
- Cryptographic hash chains for data integrity
- Built-in audit logging
- Flexible data classification (PHI, PII, de-identified)
- Time-travel queries for compliance reporting

Happy building! 🚀
