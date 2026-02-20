---
title: "Crate Structure"
section: "internals/architecture"
slug: "crate-structure"
order: 1
---

# Crate Structure

Detailed breakdown of Kimberlite's Cargo workspace organization.

## Workspace Organization

Kimberlite is organized as a Cargo workspace with 30 crates, divided into 5 layers.

## Layer 1: Foundation

Core primitives used by everything above. No dependencies on higher layers.

### kimberlite-types

**Purpose:** Core type definitions (IDs, offsets, positions, enums)

**Key types:**
- `TenantId` - Newtype wrapper for tenant identifiers
- `StreamId` - Logical grouping within a tenant
- `Offset` - Position in a stream
- `LogPosition` - Global position in the log
- `EventType` - Enum of all event types

**Dependencies:** Minimal (serde, thiserror)

**Location:** `crates/kimberlite-types/`

### kimberlite-crypto

**Purpose:** Cryptographic primitives

**Key functionality:**
- SHA-256 hashing (compliance paths, FIPS 180-4)
- BLAKE3 hashing (internal hot paths, 10x faster)
- Ed25519 signatures (asymmetric crypto)
- AES-256-GCM encryption (symmetric crypto)
- ChaCha20-Poly1305 encryption (alternative cipher)

**Dependencies:** sha2, blake3, ed25519-dalek, aes-gcm, chacha20poly1305, rand, zeroize

**Location:** `crates/kimberlite-crypto/`

**See also:** [Cryptography Deep Dive](crypto.md)

### kimberlite-storage

**Purpose:** Append-only log implementation

**Key functionality:**
- Segment-based log storage
- CRC32 checksums per record
- Hash chaining for tamper evidence
- Sequential writes (append-only)
- Bounded reads (offset + limit)

**Dependencies:** kimberlite-types, kimberlite-crypto

**Location:** `crates/kimberlite-storage/`

**See also:** [Storage Deep Dive](storage.md)

---

## Layer 2: Core

State machine, consensus, storage, and query execution.

### kimberlite-kernel

**Purpose:** Pure functional state machine (Command → State + Effects)

**Key functionality:**
- `apply_committed()` - Pure function to derive new state
- Effect types (IO, Network, Timer)
- State transitions
- Deterministic replay

**Dependencies:** kimberlite-types

**Location:** `crates/kimberlite-kernel/`

**See also:** [Kernel Deep Dive](kernel.md)

### kimberlite-vsr

**Purpose:** Viewstamped Replication consensus

**Key functionality:**
- VSR protocol implementation (Prepare, PrepareOK, Commit, etc.)
- View change handling
- Leader election
- Log repair and state transfer
- Single-node mode (development)

**Dependencies:** kimberlite-types, kimberlite-storage

**Location:** `crates/kimberlite-vsr/`

**See also:** [Consensus](..//docs/concepts/consensus)

### kimberlite-store

**Purpose:** B+tree projection store with MVCC

**Key functionality:**
- B+tree index (on-disk)
- MVCC (multi-version concurrency control)
- Page cache management
- 4KB page alignment
- Point-in-time snapshots

**Dependencies:** kimberlite-types

**Location:** `crates/kimberlite-store/`

### kimberlite-query

**Purpose:** SQL subset parser and executor

**Key functionality:**
- SQL parser (using sqlparser crate)
- Query planner
- Query executor
- Table/index management
- AS OF queries (time-travel)

**Dependencies:** kimberlite-types, kimberlite-store, sqlparser, rust_decimal

**Location:** `crates/kimberlite-query/`

---

## Layer 3: Coordination

Orchestrates propose → commit → apply → execute.

### kmb-runtime (planned)

**Purpose:** Runtime that coordinates kernel + VSR + store

**Key functionality:**
- Orchestration of propose → commit → apply → execute flow
- Effect execution (IO, network, timers)
- Background tasks (scrubbing, compaction)
- Metrics collection

**Dependencies:** kimberlite-kernel, kimberlite-vsr, kimberlite-store

**Location:** `crates/kmb-runtime/` (not yet implemented)

**Status:** Planned for v0.5.0

### kimberlite-directory

**Purpose:** Placement routing, tenant-to-shard mapping

**Key functionality:**
- Tenant → VSR group mapping
- Regional placement enforcement
- Shard discovery
- Directory caching

**Dependencies:** kimberlite-types

**Location:** `crates/kimberlite-directory/`

---

## Layer 4: Protocol

Network communication and serialization.

### kimberlite-wire

**Purpose:** Binary wire protocol definitions

**Key functionality:**
- Message framing
- Serialization/deserialization
- Protocol versioning
- CRC32 checksums on messages

**Dependencies:** bytes, serde, postcard

**Location:** `crates/kimberlite-wire/`

### kimberlite-server

**Purpose:** RPC server daemon

**Key functionality:**
- Network I/O (using mio)
- Connection management
- Request routing
- Authentication/authorization
- TLS support

**Dependencies:** kmb-runtime, kimberlite-wire, mio, rustls

**Location:** `crates/kimberlite-server/`

---

## Layer 5: Client

SDKs and tools for applications.

### kimberlite (facade)

**Purpose:** High-level SDK for applications

**Key functionality:**
- Re-exports all public APIs
- Simplified connection management
- Ergonomic query interface
- Connection pooling

**Dependencies:** kimberlite-client, kimberlite-types

**Location:** `crates/kimberlite/`

### kimberlite-client

**Purpose:** Low-level RPC client

**Key functionality:**
- Connection to kimberlite-server
- Request/response handling
- Retry logic
- Timeouts

**Dependencies:** kimberlite-wire, tokio

**Location:** `crates/kimberlite-client/`

### kimberlite-admin

**Purpose:** CLI administration tool

**Key functionality:**
- Cluster management commands
- Tenant management
- Backup/restore
- Diagnostics

**Dependencies:** kimberlite-client, clap

**Location:** `crates/kimberlite-admin/`

**Status:** Planned for v0.5.0

---

## Supporting Crates

### Testing & Simulation

#### kimberlite-sim

**Purpose:** VOPR deterministic simulation testing framework

**Key functionality:**
- 46 test scenarios across 10 phases
- 19 invariant checkers
- Fault injection (network, storage, crash, Byzantine)
- Deterministic RNG (same seed → same execution)
- Event logging and .kmb bundle generation

**Dependencies:** kimberlite-vsr, kimberlite-kernel, proptest, rand

**Location:** `crates/kimberlite-sim/`

**See also:** [VOPR Overview](../../../docs-internal/vopr/overview.md)

#### kimberlite-sim-macros

**Purpose:** Procedural macros for VOPR

**Location:** `crates/kimberlite-sim-macros/`

### Configuration & Utilities

#### kimberlite-config

**Purpose:** Configuration file parsing and validation

**Dependencies:** config, serde, directories

**Location:** `crates/kimberlite-config/`

#### kimberlite-dev

**Purpose:** Development utilities (CLI for local testing)

**Location:** `crates/kimberlite-dev/`

#### kimberlite-cli

**Purpose:** Main CLI binary (kmb command)

**Status:** Planned for v0.6.0

**Location:** `crates/kimberlite-cli/`

### Advanced Features

#### kimberlite-sharing

**Purpose:** Cross-tenant data sharing

**Dependencies:** kimberlite-types, kimberlite-kernel

**Location:** `crates/kimberlite-sharing/`

**See also:** [Data Sharing Design](../design/data-sharing.md)

#### kimberlite-migration

**Purpose:** Schema migration tooling

**Location:** `crates/kimberlite-migration/`

#### kimberlite-cluster

**Purpose:** Cluster management and reconfiguration

**Status:** In progress (v0.5.0)

**Location:** `crates/kimberlite-cluster/`

#### kimberlite-studio

**Purpose:** Web-based UI for query editor and visualization

**Status:** Planned for v0.7.0

**Location:** `crates/kimberlite-studio/`

#### kimberlite-mcp

**Purpose:** Model Context Protocol server for LLM integration

**Dependencies:** kimberlite-agent-protocol

**Location:** `crates/kimberlite-mcp/`

**See also:** [LLM Integration Design](../design/llm-integration.md)

#### kimberlite-ffi

**Purpose:** Foreign function interface for C/C++ integration

**Status:** Planned for v0.8.0

**Location:** `crates/kimberlite-ffi/`

#### kimberlite-bench

**Purpose:** Benchmarking suite

**Dependencies:** criterion, hdrhistogram

**Location:** `crates/kimberlite-bench/`

### Protocol & Agent

#### kimberlite-agent-protocol

**Purpose:** LLM agent protocol definitions

**Location:** `crates/kimberlite-agent-protocol/`

**See also:** [Agent Protocol Reference](..//docs/reference/agent-protocol)

---

## Dependency Graph

```
┌───────────────────────────────────────────────────────┐
│ Layer 5: Client                                       │
│                                                       │
│  kimberlite (facade)                                  │
│  kimberlite-client                                    │
│  kimberlite-admin                                     │
│  kimberlite-cli                                       │
└───────────────────────┬───────────────────────────────┘
                        │
┌───────────────────────▼───────────────────────────────┐
│ Layer 4: Protocol                                     │
│                                                       │
│  kimberlite-wire                                      │
│  kimberlite-server                                    │
└───────────────────────┬───────────────────────────────┘
                        │
┌───────────────────────▼───────────────────────────────┐
│ Layer 3: Coordination                                 │
│                                                       │
│  kmb-runtime                                          │
│  kimberlite-directory                                 │
│  kimberlite-cluster                                   │
└───────────────────────┬───────────────────────────────┘
                        │
┌───────────────────────▼───────────────────────────────┐
│ Layer 2: Core                                         │
│                                                       │
│  kimberlite-kernel                                    │
│  kimberlite-vsr                                       │
│  kimberlite-store                                     │
│  kimberlite-query                                     │
└───────────────────────┬───────────────────────────────┘
                        │
┌───────────────────────▼───────────────────────────────┐
│ Layer 1: Foundation                                   │
│                                                       │
│  kimberlite-types                                     │
│  kimberlite-crypto                                    │
│  kimberlite-storage                                   │
└───────────────────────────────────────────────────────┘
```

**Key principle:** Dependencies flow downward only. Foundation crates have no dependencies on higher layers.

---

## Build Profiles

Defined in workspace `Cargo.toml`:

### Development (default)

```toml
[profile.dev]
opt-level = 0
debug = true
```

Fast compilation, debugging enabled.

### Release

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 16
```

Optimized for performance.

### Release-Official (distribution)

```toml
[profile.release-official]
inherits = "release"
lto = "fat"          # Full link-time optimization
codegen-units = 1    # Maximum optimization
strip = true         # Remove debug symbols
panic = "abort"      # Smaller binaries
```

Used for official releases and benchmarks.

---

## Workspace Lints

Enforced workspace-wide:

```toml
[workspace.lints.rust]
unsafe_code = "deny"  # No unsafe code allowed

[workspace.lints.clippy]
all = "warn"
pedantic = "warn"
```

### Allowed Clippy Lints

During early development, some pedantic lints are allowed:

```toml
module_name_repetitions = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
cast_possible_truncation = "allow"
```

These will be tightened before v1.0.0.

---

## Testing Infrastructure

- **Unit tests:** In `src/tests.rs` per crate
- **Integration tests:** In `tests/` per crate
- **Property tests:** Using proptest
- **Simulation tests:** kimberlite-sim (VOPR)
- **Benchmarks:** kimberlite-bench

---

## Related Documentation

- **[Kernel Deep Dive](kernel.md)** - State machine implementation
- **[Storage Deep Dive](storage.md)** - Log format and segments
- **[Cryptography Deep Dive](crypto.md)** - Hash algorithms and encryption
- **[Architecture Overview](..//docs/concepts/architecture)** - High-level architecture

---

**Total:** 30 crates organized into 5 layers with strict dependency direction.
