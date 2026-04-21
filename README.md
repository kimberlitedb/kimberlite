<p align="center">
  <img src="website/public/images/kimberlite-icon-512.png" alt="Kimberlite Logo" width="200"/>
</p>

# Kimberlite

[![Crates.io](https://img.shields.io/crates/v/kimberlite.svg)](https://crates.io/crates/kimberlite)
[![Downloads](https://img.shields.io/crates/d/kimberlite.svg)](https://crates.io/crates/kimberlite)
[![Documentation](https://docs.rs/kimberlite/badge.svg)](https://docs.rs/kimberlite)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org)
[![Edition](https://img.shields.io/badge/edition-2024-blue.svg)](https://doc.rust-lang.org/edition-guide/)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![CI](https://github.com/kimberlitedb/kimberlite/workflows/CI/badge.svg)](https://github.com/kimberlitedb/kimberlite/actions/workflows/ci.yml)
[![VOPR](https://img.shields.io/badge/testing-VOPR-green.svg)](docs/internals/testing/overview.md)
[![Fuzz Nightly](https://github.com/kimberlitedb/kimberlite/actions/workflows/fuzz.yml/badge.svg)](https://github.com/kimberlitedb/kimberlite/actions/workflows/fuzz.yml)
[![Formal Verification](https://img.shields.io/badge/verified-formal%20spec%20%2B%20bounded%20proofs-success.svg)](docs/concepts/formal-verification.md)
[![Discord](https://img.shields.io/discord/1468161583787151493?label=discord&logo=discord&color=5865F2)](https://discord.gg/QPChWYjD)

**A compliance-first database for regulated industries.**

Built for healthcare, finance, legal, and government—where data integrity is non-negotiable.

🔬 **Developer Preview** - Explore deterministic database concepts through production-quality code

Kimberlite is a verifiable, durable database engine designed for environments where data integrity, auditability, and trust are non-negotiable. Built around a single principle:

> **All data is an immutable, ordered log. All state is a derived view.**

## Why Kimberlite?

**The compliance tax is real.** In regulated industries, you're forced to build:
- Immutable audit trails for every change
- Cryptographic proof of data integrity
- Per-tenant encryption and isolation
- Point-in-time reconstruction

Most teams bolt these onto existing databases. **Kimberlite builds them in.**

**Key approach:**
- **Immutable audit trail** - Hash-chained append-only log means every action is recorded
- **Time-travel queries** - Reconstruct any point-in-time state via MVCC (`AT OFFSET` today; `AS OF TIMESTAMP` planned v0.6)
- **Multi-tenant isolation** - Cryptographic boundaries prevent cross-tenant access
- **Multi-layer verification** - TLA+ protocol specs, Coq crypto proofs, Alloy structural models, Ivy Byzantine invariants, Kani bounded model checking, MIRI UB detection ([details](docs/concepts/formal-verification.md))

**Target industries (designed for):** Healthcare (HIPAA-ready), Finance (SOC 2-ready), Legal (chain-of-custody), Government (FedRAMP patterns)

## Who Should Explore This

- 🏥 **Healthcare developers** - Build HIPAA-ready EHR systems with built-in audit trails
- 💰 **Finance engineers** - Create SOC 2-ready applications with cryptographic guarantees
- ⚖️ **Legal tech builders** - Implement chain-of-custody with tamper-evident storage
- 🔬 **Database researchers** - Study formally specified consensus and immutable log architectures

**Perfect for learning.** Not yet recommended for production deployments (see [Status](#status) below).

## Quick Start

**5-minute quickstart:** See [Getting Started](docs/start/quick-start.md) for a complete tutorial with explanations.

**TL;DR:**

```bash
# Install (see docs/start/installation.md for all options)
curl -fsSL https://kimberlite.dev/install.sh | sh

# Initialize (or: kimberlite init  for interactive wizard)
kimberlite init myproject
kimberlite dev
# Studio: http://localhost:5555, DB: 127.0.0.1:5432
```

Try time-travel queries:
```sql
CREATE TABLE patients (id INTEGER, name TEXT);
INSERT INTO patients VALUES (1, 'Alice'), (2, 'Bob');

-- View current state
SELECT * FROM patients;

-- View state as of a specific log offset (MVCC time-travel)
SELECT * FROM patients AT OFFSET 0;
-- AS OF TIMESTAMP '...' planned for v0.6; track ROADMAP.md.
```

## Documentation

- [Quick Start](https://kimberlite.dev/docs/quick-start) - Get running in 90 seconds
- [CLI Reference](https://kimberlite.dev/docs/reference/cli) - All commands
- [SQL Reference](https://kimberlite.dev/docs/reference/sql) - Supported SQL syntax
- [Architecture](https://kimberlite.dev/architecture) - How Kimberlite works
- [Roadmap](ROADMAP.md) - Future features and enhancements
- [Changelog](CHANGELOG.md) - Release history and completed work
- [Contributing](CONTRIBUTING.md) - How to contribute

## Building from Source

```bash
# Clone and build
git clone https://github.com/kimberlitedb/kimberlite.git
cd kimberlite
cargo build --release -p kimberlite-cli

# Binary is at ./target/release/kimberlite
```

### Development Commands

```bash
just build          # Debug build
just build-release  # Release build
just test           # Run all tests
just nextest        # Faster test runner
just clippy         # Linting
just pre-commit     # Run before committing
```

## Key Features

**What Makes Kimberlite Unique:**

- ✅ **Immutable audit trail** - Hash-chained append-only log (SHA-256 for compliance, BLAKE3 for performance)
- ✅ **Time-travel queries** - MVCC enables `AT OFFSET` queries today; `AS OF TIMESTAMP` planned v0.6
- ✅ **Deterministic core** - Functional Core / Imperative Shell pattern enables perfect replication
- ✅ **Multi-tenant isolation** - Per-tenant storage with cryptographic boundaries
- ✅ **Multi-layer verification** - TLA+ protocol specs (TLC in PR CI, TLAPS nightly), Coq crypto proofs, Alloy structural models, Ivy Byzantine invariants, Kani bounded model checking, MIRI undefined-behavior detection ([details](docs/concepts/formal-verification.md))
- ✅ **SQL interface** - Core DDL/DML + SELECT with aggregates, GROUP BY/HAVING, UNION, INNER/LEFT JOIN, CTEs, subqueries, window functions; RIGHT/FULL OUTER JOIN + transactions planned
- ✅ **Tamper-evidence** - CRC32 checksums + hash chains detect corruption
- ✅ **Viewstamped Replication (VSR)** - Full multi-node consensus (Normal, ViewChange, Recovery, Repair, StateTransfer, Reconfiguration)
- ✅ **RBAC/ABAC enforcement** - Per-role row/column filters; HIPAA, FedRAMP, PCI pre-built policies
- ✅ **Security hardened** - 40-finding pre-launch audit completed; message signatures, replay protection, DoS limits
- 🚧 **v0.5.0 preview** - Audit query, subject export, breach notification endpoints: SDK wrappers shipped; server handlers return `NotImplemented` until v0.5.0 (see [SDK parity matrix](docs/reference/sdk/parity.md))

**Legend**: ✅ Shipped in v0.4 | 🚧 Scheduled v0.5+

## Use Cases

Kimberlite is designed for:
- Healthcare (EHR, clinical data, HIPAA)
- Financial services (audit trails, transaction records)
- Legal systems (chain of custody, evidence)
- Government (regulated records, compliance)

## Examples

See the [examples/](examples/) directory for:
- [quickstart/](examples/quickstart/) - Getting started
- [rust/](examples/rust/) - Rust SDK examples
- [docker/](examples/docker/) - Docker deployments
- [healthcare/](examples/healthcare/) - HIPAA-ready schema

## Architecture

```
┌──────────────────────────────────────────────────┐
│  Kernel (pure state machine: Cmd -> State + FX)  │
├──────────────────────────────────────────────────┤
│  Append-Only Log (hash-chained, CRC32)           │
├──────────────────────────────────────────────────┤
│  Crypto (SHA-256, BLAKE3, AES-256-GCM, Ed25519)  │
└──────────────────────────────────────────────────┘
```

See [docs/concepts/architecture.md](docs/concepts/architecture.md) for details.

## Why Kimberlite vs. Traditional Databases?

| Feature | PostgreSQL | Kimberlite |
|---------|-----------|-----------|
| **Data model** | Mutable tables | Immutable log + derived views |
| **Audit trail** | Manual triggers | Built-in (every write logged) |
| **Time-travel** | Extensions (complex) | Native SQL (`AS OF TIMESTAMP`) |
| **Integrity** | Checksums | Hash chains + CRC32 |
| **Consensus** | Streaming replication | VSR (deterministic, multi-node) |
| **Best for** | General OLTP | Compliance-heavy workloads |

**Trade-offs:** Kimberlite trades some write throughput for built-in auditability and tamper-evidence. Quantitative re-baseline against v0.4.x is scheduled for v0.5.0; see [FAQ](docs/reference/faq.md) for the qualitative comparison.

## Learning Resources

### Documentation Deep Dive

- [docs/concepts/architecture.md](docs/concepts/architecture.md) - FCIS pattern, kernel design, consensus
- [docs/internals/testing/assertions-inventory.md](docs/internals/testing/assertions-inventory.md) - Production assertion policy + paired `#[should_panic]` tests
- [docs/internals/testing/overview.md](docs/internals/testing/overview.md) - VOPR deterministic simulation testing
- [docs/concepts/pressurecraft.md](docs/concepts/pressurecraft.md) - Code quality standards
- [docs/concepts/compliance.md](docs/concepts/compliance.md) - HIPAA-ready, SOC 2-ready, GDPR-ready patterns

## Community

- 💬 [Discord](https://discord.gg/QPChWYjD) - Join for real-time support, design discussions, and community
- 📖 [Documentation](docs/) - Comprehensive architecture and usage guides
- 🐛 [Issues](https://github.com/kimberlitedb/kimberlite/issues) - Bug reports and feature requests
- 💡 [Discussions](https://github.com/kimberlitedb/kimberlite/discussions) - Questions, ideas, and design conversations
- ❓ [FAQ](docs/reference/faq.md) - Frequently asked questions

## Status

> **v0.x — Developer Preview.** Stable enough for prototypes, learning,
> internal tools, and compliance research. Not yet battle-tested at scale.
>
> - ✅ **Core is solid:** 1,300+ tests, deterministic simulation, production-grade crypto.
> - ✅ **Architecture is stable:** FCIS pattern, immutable log, full multi-node VSR consensus.
> - ✅ **Security hardened:** 40-finding pre-launch audit completed (RBAC, message auth, durability, supply-chain pins).
> - ✅ **SDKs are production-grade:** Rust, TypeScript, and Python SDKs ship full data-plane + compliance + admin surface, with connection pooling and real-time subscriptions. See [SDK parity matrix](docs/reference/sdk/parity.md).
> - ⚠️ **Wire protocol may still evolve** between minor versions. See [`CHANGELOG.md`](CHANGELOG.md) for the current version and any breaking changes.
>
> **Use for:** internal tools, prototypes, learning database internals, compliance research.
>
> **Wait for v1.0 (Q2 2027) if you need:** API stability guarantees, large-scale production deployment, commercial support.

## SDKs

Kimberlite provides idiomatic client libraries for multiple languages:

| Language   | Status | Package | Install |
|------------|--------|---------|---------|
| Rust       | ✅ Ready | `kimberlite` | `cargo add kimberlite` |
| Python     | 🚧 Beta | `kimberlite` | `pip install kimberlite` |
| TypeScript | ✅ Ready (Node 18/20/22/24, prebuilt napi-rs) | `@kimberlite/client` | `npm install @kimberlite/client` |
| Go         | 📋 Deferred post-v0.4 | — | See [ROADMAP](ROADMAP.md) |
| Java       | 📋 Planned | `com.kimberlite:kimberlite-client` | Maven/Gradle |
| C#         | 📋 Planned | `Kimberlite.Client` | `dotnet add package ...` |
| C++        | 📋 Planned | `kimberlite-cpp` | Coming soon |

See [docs/reference/sdk/overview.md](docs/reference/sdk/overview.md) for architecture and [docs/reference/protocol.md](docs/reference/protocol.md) for wire protocol specification.

## License

Apache 2.0

## Contributing

- Read [CLAUDE.md](CLAUDE.md) for development guidelines
- Review [docs/concepts/pressurecraft.md](docs/concepts/pressurecraft.md) for coding standards
- Open issues for design discussions
