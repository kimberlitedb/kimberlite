# Quick Start

Get Kimberlite up and running in less than 10 minutes.

## What is Kimberlite?

Kimberlite is a compliance-first database for regulated industries (healthcare, finance, legal). Built on one principle:

**All data is an immutable, ordered log. All state is a derived view.**

This makes compliance, auditing, and time-travel queries natural rather than bolted-on.

## Prerequisites

- Rust 1.88+ (install from [rustup.rs](https://rustup.rs))
- Basic familiarity with Rust or systems programming
- A terminal/command line

## Installation

### Option 1: Build from Source (Current)

```bash
# Clone the repository
git clone https://github.com/kimberlitedb/kimberlite.git
cd kimberlite

# Build the workspace
cargo build --workspace

# Run tests to verify
cargo test --workspace
```

### Option 2: Install from crates.io (Coming in v0.5.0)

```bash
# Not yet available
cargo install kimberlite-cli
```

## Current Status (v0.4.0)

Kimberlite is currently in **early development**. Available components:

✅ **Core Libraries** (stable):
- `kimberlite-types` - Entity IDs and type system
- `kimberlite-crypto` - Cryptographic primitives (SHA-256, BLAKE3, AES-GCM, Ed25519)
- `kimberlite-storage` - Append-only log with CRC32 checksums
- `kimberlite-kernel` - Pure functional state machine
- `kimberlite-vsr` - Viewstamped Replication consensus

✅ **Testing Infrastructure** (production-ready):
- VOPR - Deterministic simulation testing (46 scenarios, 19 invariants)
- Property testing with proptest
- Assertion density for safety-critical code

🚧 **In Progress**:
- `kimberlite-query` - SQL query engine
- `kimberlite-server` - Network server
- `kimberlite-client` - Client SDKs (Python, TypeScript, Rust, Go)

📋 **Planned** (see [ROADMAP.md](../../ROADMAP.md)):
- `kmb` CLI - Interactive SQL shell and database operations (v0.6.0)
- `kmbctl` CLI - Cluster management tool (v0.5.0)
- Studio UI - Web-based query editor (v0.7.0)

## Quick Start: Using Core Libraries

Since the full database server is still in development, here's how to use the core libraries today:

### 1. Add Dependencies

```toml
# Cargo.toml
[dependencies]
kimberlite-types = "0.4.0"
kimberlite-crypto = "0.4.0"
kimberlite-storage = "0.4.0"
kimberlite-kernel = "0.4.0"
```

### 2. Create a Simple Example

```rust
use kimberlite_types::{TenantId, StreamId, Offset};
use kimberlite_storage::AppendOnlyLog;
use kimberlite_crypto::HashChain;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a tenant
    let tenant_id = TenantId::new(1);
    println!("Created tenant: {}", tenant_id);

    // Create a stream for this tenant
    let stream_id = StreamId::new(tenant_id, 1);
    println!("Created stream: {}", stream_id);

    // Initialize storage (append-only log)
    let mut log = AppendOnlyLog::new("./data")?;

    // Append data
    let data = b"Patient record: Alice Smith, DOB: 1985-03-15";
    let offset = log.append(stream_id, data)?;
    println!("Appended at offset: {}", offset);

    // Read data back
    let entry = log.read_at(stream_id, offset)?;
    println!("Read: {:?}", std::str::from_utf8(&entry.data));

    Ok(())
}
```

### 3. Run It

```bash
cargo run
```

Output:
```
Created tenant: TenantId(1)
Created stream: StreamId { tenant: TenantId(1), stream: 1 }
Appended at offset: 0
Read: Ok("Patient record: Alice Smith, DOB: 1985-03-15")
```

## Quick Start: VOPR Testing

VOPR (Viewstamped Operation Replication) is our production-ready simulation testing tool.

### 1. Run Your First Simulation

```bash
# Run baseline scenario with 1000 iterations
cargo run --bin vopr -- run --scenario baseline --iterations 1000
```

Or use the Justfile shortcut:

```bash
just vopr
```

### 2. List Available Scenarios

```bash
cargo run --bin vopr -- scenarios
```

This shows all 46 test scenarios across 10 phases (Byzantine attacks, crash recovery, gray failures, etc.).

### 3. Reproduce a Failure

If a simulation finds a bug, it saves a `.kmb` bundle:

```bash
# Reproduce the exact failure
cargo run --bin vopr -- repro failure-20260205-143022.kmb

# Show failure details
cargo run --bin vopr -- show failure-20260205-143022.kmb --events
```

### 4. Visualize Timeline

```bash
# ASCII Gantt chart of what happened
cargo run --bin vopr -- timeline failure-20260205-143022.kmb
```

See [VOPR CLI Reference](../reference/cli/vopr.md) for all 10 commands.

## Common Workflows

### Building the Project

```bash
# Debug build (fast compilation)
just build

# Release build (optimized)
just build-release
```

### Running Tests

```bash
# All tests
just test

# Faster test runner (requires cargo-nextest)
just nextest

# Run a specific test
just test-one replica_commit
```

### Property Testing

```bash
# Run with more test cases
PROPTEST_CASES=10000 cargo test --workspace
```

### Code Quality

```bash
# Format code
just fmt

# Check formatting
just fmt-check

# Run clippy (linter)
just clippy

# Full pre-commit checks
just pre-commit
```

### VOPR Scenarios

```bash
# Run specific scenario
just vopr-scenario multi_tenant_isolation 10000

# List scenarios
just vopr-scenarios

# Quick smoke test
just vopr-quick

# Full test suite (all scenarios)
just vopr-full 10000
```

## Architecture Overview

Kimberlite follows a **Functional Core / Imperative Shell** pattern:

```
┌──────────────────────────────────────────────────┐
│  Kernel (pure state machine: Cmd → State + FX)   │
├──────────────────────────────────────────────────┤
│  Append-Only Log (hash-chained, CRC32)           │
├──────────────────────────────────────────────────┤
│  Crypto (SHA-256, BLAKE3, AES-256-GCM, Ed25519)  │
└──────────────────────────────────────────────────┘
```

**Key Principles:**
- **Immutability** - All data is append-only, nothing is ever deleted or modified
- **Pure Functions** - Core logic is deterministic and testable
- **Effects at Edges** - IO happens only at system boundaries
- **Type Safety** - Make illegal states unrepresentable

See [Architecture](../concepts/architecture.md) for details.

## Next Steps

### Learn Concepts

Understand why Kimberlite works the way it does:

- [Overview](../concepts/overview.md) - What is Kimberlite?
- [Data Model](../concepts/data-model.md) - Append-only logs and projections
- [Consensus](../concepts/consensus.md) - How VSR works
- [Pressurecraft](../concepts/pressurecraft.md) - Our coding philosophy

### Explore Internals

Dive deeper into implementation:

- [Crate Structure](../internals/architecture/crate-structure.md)
- [Kernel](../internals/architecture/kernel.md)
- [Storage](../internals/architecture/storage.md)
- [Testing Overview](../internals/testing/overview.md)

### Reference Documentation

Look up API details:

- [VOPR CLI](../reference/cli/vopr.md) - All 10 VOPR commands
- [Rust API](../reference/sdk/rust-api.md) - Rust SDK reference
- [Protocol](../reference/protocol.md) - Wire protocol spec

### Contribute

Want to help build Kimberlite?

- Read [CLAUDE.md](../../CLAUDE.md) - Development guide
- See [/docs-internal](../../docs-internal/) - Internal documentation
- Check [ROADMAP.md](../../ROADMAP.md) - What's being built
- Open [GitHub Issues](https://github.com/kimberlitedb/kimberlite/issues)

## Roadmap Highlights

**v0.5.0 (Q2 2026)**: Cluster management
- `kmbctl` CLI for cluster operations
- Multi-node consensus
- Reconfiguration (add/remove replicas)

**v0.6.0 (Q3 2026)**: Query engine
- SQL query support
- `kmb` CLI for interactive queries
- Python/TypeScript client SDKs

**v0.7.0 (Q4 2026)**: Observability & UI
- Studio UI (web-based query editor)
- Metrics and tracing
- Production monitoring

**v1.0.0 (Q1 2027)**: Production-ready
- Full HIPAA/GDPR compliance features
- Production hardening
- Performance optimization

See [ROADMAP.md](../../ROADMAP.md) for complete details.

## Getting Help

- **Documentation**: Browse [/docs](../)
- **Code Examples**: See `crates/*/examples/` directories
- **CLAUDE.md**: Development guide and build commands
- **GitHub Issues**: Report bugs and request features
- **GitHub Discussions**: Ask questions

## Cheat Sheet

```bash
# Build
just build                    # Debug build
just build-release            # Release build

# Test
just test                     # All tests
just nextest                  # Faster test runner
just test-one <name>          # Specific test

# Code quality
just fmt                      # Format code
just clippy                   # Run linter
just pre-commit               # All checks

# VOPR testing
just vopr                     # Run VOPR
just vopr-scenarios           # List scenarios
just vopr-quick               # Smoke test
just vopr-full 10000          # Full suite

# CI simulation
just ci                       # Quick CI checks
just ci-full                  # Full CI (includes security)
```

---

**Welcome to Kimberlite!** This is an early-stage project building a new kind of database. Come build with us.
