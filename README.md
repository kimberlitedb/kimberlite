# Kimberlite

**The compliance-first database for regulated industries.**

Kimberlite is a verifiable, durable database engine designed for environments where data integrity, auditability, and trust are non-negotiable. Built around a single principle:

> **All data is an immutable, ordered log. All state is a derived view.**

## Quick Start

```bash
# Download (or build from source)
curl -Lo kimberlite.zip https://kimberlite.dev/download && unzip kimberlite.zip

# Initialize and start
./kimberlite init ./data --development
./kimberlite start --address 3000 ./data

# Connect (new terminal)
./kimberlite repl --address 127.0.0.1:3000
```

```sql
kimberlite> CREATE TABLE patients (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id));
kimberlite> INSERT INTO patients VALUES (1, 'Jane Doe');
kimberlite> SELECT * FROM patients;
-- id | name
-- ---+---------
--  1 | Jane Doe
```

## Documentation

- [Quick Start](https://kimberlite.dev/docs/quick-start) - Get running in 90 seconds
- [CLI Reference](https://kimberlite.dev/docs/reference/cli) - All commands
- [SQL Reference](https://kimberlite.dev/docs/reference/sql) - Supported SQL syntax
- [Architecture](https://kimberlite.dev/architecture) - How Kimberlite works

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

- **Immutable Audit Trail** - Every change logged with hash chaining
- **Time Travel Queries** - Reconstruct any historical state
- **Multi-Tenant Isolation** - Per-tenant logs and encryption keys
- **Viewstamped Replication** - Durable, totally-ordered writes
- **SQL Interface** - Familiar query language for compliance lookloads

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

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for details.

## Status

> **Early Development** - Core architecture is feature-complete. Interfaces may change.

## SDKs

Kimberlite provides idiomatic client libraries for multiple languages:

| Language   | Status | Package | Install |
|------------|--------|---------|---------|
| Rust       | ✅ Ready | `kimberlite` | `cargo add kimberlite` |
| Python     | 🚧 In Progress | `kimberlite` | `pip install kimberlite` |
| TypeScript | 🚧 In Progress | `@kimberlite/client` | `npm install @kimberlite/client` |
| Go         | 📋 Planned | `github.com/kimberlitedb/kimberlite-go` | `go get ...` |
| Java       | 📋 Planned | `com.kimberlite:kimberlite-client` | Maven/Gradle |
| C#         | 📋 Planned | `Kimberlite.Client` | `dotnet add package ...` |
| C++        | 📋 Planned | `kimberlite-cpp` | Coming soon |

See [docs/SDK.md](docs/SDK.md) for architecture and [docs/PROTOCOL.md](docs/PROTOCOL.md) for wire protocol specification.

## License

Apache 2.0

## Contributing

- Read [CLAUDE.md](CLAUDE.md) for development guidelines
- Review [docs/PRESSURECRAFT.md](docs/PRESSURECRAFT.md) for coding standards
- Open issues for design discussions
