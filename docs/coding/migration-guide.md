# Migration Guide: Old CLI → Unified CLI

This guide helps you migrate from the old Kimberlite CLI structure to the new unified `kmb` command.

## What Changed?

The new unified CLI consolidates all Kimberlite tools into a single `kmb` command with hierarchical subcommands. This provides:

- **Single entry point**: One command instead of many binaries
- **Consistent UX**: Unified flag conventions and output styling
- **Better discovery**: `kmb help` shows all available commands
- **Shell completions**: Tab completion for all commands

## Command Mapping

### Core Commands

| Old CLI | New CLI | Notes |
|---------|---------|-------|
| `kimberlite init` | `kmb init` | ✅ Compatible |
| `kimberlite start <path>` | `kmb start <path>` | ✅ Compatible |
| N/A | `kmb dev` | 🆕 **New**: All-in-one dev server |
| `kimberlite-repl` | `kmb repl --tenant <ID>` | ⚠️ Requires `--tenant` flag |
| N/A | `kmb query "SQL" --tenant <ID>` | 🆕 **New**: One-shot queries |

### Tenant Management

| Old CLI | New CLI | Notes |
|---------|---------|-------|
| N/A | `kmb tenant create --id <ID> --name <NAME>` | 🆕 **New** |
| N/A | `kmb tenant list` | 🆕 **New** |
| N/A | `kmb tenant delete --id <ID>` | 🆕 **New** |
| N/A | `kmb tenant info --id <ID>` | 🆕 **New** |

### Cluster Management

| Old CLI | New CLI | Notes |
|---------|---------|-------|
| N/A | `kmb cluster init --nodes <N>` | 🆕 **New** |
| N/A | `kmb cluster start` | 🆕 **New** |
| N/A | `kmb cluster stop` | 🆕 **New** |
| N/A | `kmb cluster status` | 🆕 **New** |

### Migration Workflow

| Old CLI | New CLI | Notes |
|---------|---------|-------|
| N/A | `kmb migration create <name>` | 🆕 **New** |
| N/A | `kmb migration apply` | 🆕 **New** |
| N/A | `kmb migration rollback` | 🆕 **New** |
| N/A | `kmb migration status` | 🆕 **New** |

### Simulation Testing

| Old CLI | New CLI | Notes |
|---------|---------|-------|
| `vopr --seed <SEED>` | `kmb sim verify --seed <SEED>` | ✅ Integration |
| `vopr -n <N>` | `kmb sim run --iterations <N>` | ✅ Integration |
| N/A | `kmb sim report --output <FILE>` | 🆕 **New** |
| `vopr <advanced-flags>` | `vopr <advanced-flags>` | ⚠️ Standalone binary still available |

### Configuration

| Old CLI | New CLI | Notes |
|---------|---------|-------|
| Manual TOML editing | `kmb config show` | 🆕 **New**: View config |
| Manual TOML editing | `kmb config set <key> <value>` | 🆕 **New**: Update config |
| N/A | `kmb config validate` | 🆕 **New**: Validate config |

### Studio UI

| Old CLI | New CLI | Notes |
|---------|---------|-------|
| N/A | `kmb studio` | 🆕 **New**: Standalone Studio |
| N/A | `kmb dev` | 🆕 **New**: Auto-launch with dev server |

### Shell Completions

| Old CLI | New CLI | Notes |
|---------|---------|-------|
| N/A | `kmb completion bash` | 🆕 **New** |
| N/A | `kmb completion zsh` | 🆕 **New** |
| N/A | `kmb completion fish` | 🆕 **New** |

## Breaking Changes

### 1. REPL Requires `--tenant` Flag

**Old**:
```bash
kimberlite-repl  # Implicitly used tenant 1
```

**New**:
```bash
kmb repl --tenant 1  # Explicit tenant required
```

**Rationale**: Prevents accidental cross-tenant data access (compliance-first design).

**Workaround**: Add alias to your shell config:
```bash
alias repl='kmb repl --tenant 1'
```

### 2. Crate Names Changed

**Old**: `kmb-*` (e.g., `kmb-sim`, `kmb-client`)
**New**: `kimberlite-*` (e.g., `kimberlite-sim`, `kimberlite-client`)

**Impact**: If you depend on Kimberlite crates in your `Cargo.toml`, update package names:

```toml
# Old
[dependencies]
kmb-client = { path = "../kmb-client" }

# New
[dependencies]
kimberlite-client = { path = "../kimberlite-client" }
```

**CLI binary name unchanged**: `kmb` command still works.

### 3. Studio Port Flag

**Old**: `kmb studio -p 8080` (if it existed)
**New**: `kmb studio --port 8080`

**Impact**: Short flag `-p` removed to avoid conflict with `--project` flag.

### 4. Dev Server Replaces Multiple Terminals

**Old workflow**:
```bash
# Terminal 1
kimberlite start

# Terminal 2
kimberlite-studio

# Terminal 3
kimberlite-repl
```

**New workflow**:
```bash
# Single terminal
kmb dev

# Opens browser to Studio automatically
# Connect with: kmb repl --tenant 1 (in new terminal)
```

## New Features

### 1. Unified Dev Command (`kmb dev`)

The star feature of the new CLI:

```bash
kmb dev
```

This single command:
- Starts database server
- Launches Studio UI
- Applies pending migrations
- Sets up auto-reload (future)
- Provides aggregated logging

**Benefits**:
- No more juggling multiple terminals
- Zero-config development
- Integrated migration workflow

### 2. Tenant Safety

All commands require explicit `--tenant` flag:

```bash
# ERROR: No tenant specified
kmb query "SELECT * FROM patients"

# CORRECT: Explicit tenant
kmb query "SELECT * FROM patients" --tenant 1
```

**Benefits**:
- Prevents accidental cross-tenant queries
- Clear audit trail (tenant always logged)
- HIPAA/GDPR compliance

### 3. Migration Management

File-based SQL migrations:

```bash
# Create migration
kmb migration create add_users_table

# Edit migrations/0001_add_users_table.sql
# CREATE TABLE users (...);

# Apply
kmb migration apply

# Check status
kmb migration status
```

**Benefits**:
- Version-controlled schema changes
- Checksum validation (tamper detection)
- Rollback support

### 4. Local Cluster Testing

Test multi-node scenarios without complex setup:

```bash
kmb cluster init --nodes 3
kmb cluster start
kmb cluster status
```

**Benefits**:
- Test failover locally
- Verify replication
- No Docker/Kubernetes required

### 5. Config Management

Structured configuration with validation:

```bash
# View current config
kmb config show

# Update setting
kmb config set database.bind_address "0.0.0.0:5432"

# Validate config files
kmb config validate
```

**Benefits**:
- No manual TOML editing
- Validation on write
- Environment-specific overrides

### 6. Integrated Simulation Testing

VOPR simulations now accessible via CLI:

```bash
kmb sim run --iterations 1000
kmb sim verify --seed 12345
```

**Benefits**:
- Discover bugs before production
- Reproduce failures exactly
- Automated failure diagnosis

## Migration Steps

### Step 1: Update Dependencies

If you're using Kimberlite crates:

```toml
# Update Cargo.toml
[dependencies]
kimberlite = "0.2"  # or latest version
kimberlite-client = "0.2"
kimberlite-types = "0.2"
```

### Step 2: Update Scripts

Update any scripts or CI/CD pipelines:

```bash
# Old
./kimberlite start /data

# New
./kmb start /data --address 0.0.0.0:5432
```

### Step 3: Create Config File

Initialize config in existing projects:

```bash
cd my-existing-project
kmb init  # Creates kimberlite.toml
```

### Step 4: Migrate Environment Variables

Old environment variables still work:

```bash
# These still work
export KMB_DATA_DIR=/var/lib/kimberlite
export KMB_BIND_ADDRESS=0.0.0.0:5432
```

New config file approach (recommended):

```toml
# kimberlite.toml
[database]
data_dir = "/var/lib/kimberlite"
bind_address = "0.0.0.0:5432"
```

### Step 5: Update Aliases

Add helpful aliases to your shell config:

```bash
# ~/.bashrc or ~/.zshrc
alias kmb-dev='kmb dev'
alias repl='kmb repl --tenant 1'
alias kmb-migrate='kmb migration apply'
```

### Step 6: Install Shell Completions

```bash
# Bash
kmb completion bash > ~/.local/share/bash-completion/completions/kmb

# Zsh
kmb completion zsh > ~/.zsh/completions/_kmb

# Fish
kmb completion fish > ~/.config/fish/completions/kmb.fish
```

## Compatibility

### Backward Compatibility

- ✅ **Config files**: Old `kimberlite.toml` format still works
- ✅ **Environment variables**: `KMB_*` variables still work
- ✅ **Data format**: Existing data files compatible
- ✅ **VOPR binary**: Standalone `vopr` still available

### Forward Compatibility

New features are opt-in:
- Migrations: Only used if `migrations/` directory exists
- Cluster: Only used if explicitly initialized
- Studio: Can be disabled with `--no-studio`

## Troubleshooting

### "Command not found: kmb"

**Problem**: Old CLI binaries still in PATH

**Solution**: Rebuild and ensure new binary is in PATH:

```bash
cargo build --release
export PATH="$PWD/target/release:$PATH"
kmb --version  # Verify
```

### "Tenant is required"

**Problem**: Old scripts don't specify `--tenant`

**Solution**: Add `--tenant` flag to all commands:

```bash
# Old
kmb-client query "SELECT * FROM patients"

# New
kmb query "SELECT * FROM patients" --tenant 1
```

### "Migration checksum mismatch"

**Problem**: Migration files were edited after being applied

**Solution**: Don't edit applied migrations. Create a new one:

```bash
kmb migration create fix_previous_change
```

### "Port already in use"

**Problem**: Old server still running

**Solution**: Stop old server or use different port:

```bash
# Stop old server
pkill kimberlite

# Or use custom port
kmb dev --port 5433
```

## FAQ

### Q: Can I use both old and new CLI?

**A**: Yes, but not recommended. The new CLI is the supported version. Migrate as soon as possible.

### Q: Will the old CLI be maintained?

**A**: No. All future development is on the unified CLI. The old CLI is deprecated.

### Q: Do I need to migrate my data?

**A**: No. The data format is unchanged. Just update the CLI.

### Q: What about the standalone `vopr` binary?

**A**: It's still available for advanced use cases. Use `kmb sim` for common scenarios.

### Q: Can I still use environment variables?

**A**: Yes. Environment variables have highest precedence over config files.

### Q: How do I migrate CI/CD pipelines?

**A**: Update scripts to use new commands. Example:

```yaml
# Old
- run: kimberlite start /data &
- run: kimberlite-repl < test.sql

# New
- run: kmb start /data &
- run: kmb query "$(cat test.sql)" --tenant 1
```

## Getting Help

If you encounter issues during migration:

1. **Check this guide**: Most common scenarios are covered
2. **CLI help**: Run `kmb help <command>` for detailed info
3. **GitHub Issues**: https://github.com/kimberlite/kimberlite/issues
4. **Discussions**: Ask in GitHub Discussions

## Summary

The unified CLI provides:
- ✅ Better developer experience
- ✅ Consistent command structure
- ✅ New features (dev server, migrations, cluster)
- ✅ Improved safety (tenant requirements)
- ✅ Backward compatibility (data, config)

**Recommended migration timeline**: Within 1-2 weeks

**Deprecation timeline**: Old CLI will be removed in version 0.3

## Next Steps

- **[Getting Started](../start/quick-start.md)**: Learn the new CLI
- **[Shell Completions](guides/shell-completions.md)**: Set up tab completion
- **[Configuration Guide](../operating/configuration.md)**: Understanding config files

Happy migrating! 🚀
