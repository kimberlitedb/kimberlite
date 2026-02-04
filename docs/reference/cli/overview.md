# CLI Tools Overview

Kimberlite provides command-line tools for development, testing, and operations.

## Available Tools

### VOPR - Simulation Testing

**Purpose:** Deterministic simulation testing for finding consensus and safety bugs.

**Command:** `vopr`

**Use cases:**
- Pre-commit testing (CI integration)
- Debugging consensus issues
- Reproducing production failures
- Coverage-guided testing

**Documentation:** [VOPR CLI Reference](vopr.md)

**Quick start:**
```bash
# Run baseline scenario
vopr run --scenario baseline --iterations 1000

# List all 46 scenarios
vopr scenarios

# Reproduce a failure
vopr repro failure.kmb
```

---

## Future CLI Tools

The following tools are planned but not yet implemented:

### kmbctl - Cluster Management

**Status:** Planned for v0.5.0

**Purpose:** Manage Kimberlite clusters (deploy, scale, reconfigure).

**Planned commands:**
```bash
kmbctl deploy --cluster prod --replicas 3
kmbctl scale --cluster prod --replicas 5
kmbctl status --cluster prod
```

### kmb - Database CLI

**Status:** Planned for v0.6.0

**Purpose:** Interactive SQL shell and database operations.

**Planned commands:**
```bash
kmb connect localhost:5432
kmb query "SELECT * FROM patients"
kmb export --table patients --format csv
```

---

## Installation

CLI tools are included in the Kimberlite distribution:

```bash
# Install from crates.io
cargo install kimberlite-cli

# Or build from source
git clone https://github.com/kimberlitedb/kimberlite
cd kimberlite
cargo build --release --bin vopr
```

---

## Shell Completions

Generate shell completions for tab-completion:

```bash
# Bash
vopr completions bash > /usr/local/etc/bash_completion.d/vopr

# Zsh
vopr completions zsh > /usr/local/share/zsh/site-functions/_vopr

# Fish
vopr completions fish > ~/.config/fish/completions/vopr.fish

# PowerShell
vopr completions powershell > $PROFILE/../Completions/vopr.ps1
```

See [Shell Completions Guide](../../coding/guides/shell-completions.md) for details.

---

## Related Documentation

- **[VOPR CLI Reference](vopr.md)** - Complete VOPR command documentation
- **[Testing Guide](../../coding/guides/testing.md)** - How to test applications
- **[Troubleshooting](../../operating/troubleshooting.md)** - Common CLI issues
