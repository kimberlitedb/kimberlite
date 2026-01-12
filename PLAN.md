# VerityDB Implementation Plan

## Overview

**VerityDB** is a compliance-native, healthcare-focused system-of-record database. It combines:
- High-performance event log with durable store
- Viewstamped Replication (VSR) for quorum-based HA
- Cap'n Proto for wire protocol (and event serialization)
- SQLCipher-encrypted SQLite projections
- First-class PHI placement controls (regional vs global)
- Audit-first design for HIPAA compliance

**Key differentiator**: "PHI stays regional by construction, non-PHI can be global."

**Dogfooding strategy**: VerityDB will be developed alongside Notebar (clinic management SaaS) to validate the architecture under real clinical workloads.

---

## Architecture Principles

### Functional Core / Imperative Shell
- **Kernel (pure)**: Commands → State + Effects. No IO, no clocks, deterministic.
- **Shell (impure)**: RPC, auth, VSR transport, storage IO, SQLite connections.

### Vertical Slices
Organize by domain capability, not horizontal layers:
- `streams` - create stream, append, retention
- `consumers` - fetch, ack, redelivery
- `projections` - create, apply, snapshots
- `policy` - PHI guard, RBAC, placement
- `audit` - immutable audit events

### Two Developer Modes

VerityDB supports two mutually exclusive modes per database:

1. **Power User Mode** - Full event sourcing / CQRS
   - Developer defines custom domain events
   - Developer implements custom projection handlers
   - Full control over event schemas and projection logic

2. **CRUD Mode** - Transparent SQL experience
   - Developer writes normal SQL migrations
   - System auto-generates events from INSERT/UPDATE/DELETE
   - Feels like regular SQLite, any ORM works
   - Event sourcing happens "under the hood"

---

## Workspace Structure

```
veritydb/
  Cargo.toml                      # Workspace root
  PLAN.md                         # This file
  rust-toolchain.toml
  schemas/
    veritydb.capnp                # Cap'n Proto schemas
  crates/
    vdb-types/                    # IDs, enums, placement, data_class
    vdb-wire/                     # Cap'n Proto codecs
    vdb-kernel/                   # Functional core (commands → effects)
    vdb-vsr/                      # VSR replication engine
    vdb-storage/                  # Append-only segment store
    vdb-directory/                # Placement router (stream → group mapping)
    vdb-projections/              # SQLCipher projection runtime (dual-mode)
    vdb-runtime/                  # Orchestrator: propose → commit → apply → execute
    vdb-server/                   # Cap'n Proto RPC server
    vdb-client/                   # Client SDK
    vdb-admin/                    # CLI tooling
```

---

## Implementation Phases

### Phase 1: Foundation (Milestone A) ✅ COMPLETE

**Goal**: Core types, kernel, single-node storage - enough to prove architecture.

#### 1.1 Create Workspace Structure ✅
- [x] Root `Cargo.toml` with workspace dependencies
- [x] `rust-toolchain.toml` (stable)
- [x] Create all crate directories with `Cargo.toml`

#### 1.2 `vdb-types` - Shared Types ✅
```rust
// Core IDs
pub struct TenantId(pub u64);
pub struct StreamId(pub u64);
pub struct Offset(pub u64);
pub struct GroupId(pub u64);

// Data classification (compliance-critical)
pub enum DataClass { PHI, NonPHI, Deidentified }

// Placement (PHI regional enforcement)
pub enum Placement {
    Region(Region),  // PHI must stay here
    Global,          // Non-PHI can replicate globally
}
```

#### 1.3 `vdb-kernel` - Functional Core ✅
- [x] `command.rs` - Command enum (CreateStream, AppendBatch)
- [x] `effects.rs` - Effect enum (StorageAppend, WakeProjection, Audit)
- [x] `state.rs` - In-memory state (streams metadata)
- [x] `kernel.rs` - `apply_committed(State, Command) -> (State, Vec<Effect>)`

#### 1.4 `vdb-storage` - Append-Only Log ✅
- [x] Segment file format: `[offset:u64][len:u32][payload][crc32:u32]`
- [x] `append_batch(stream, events, expected_offset, fsync)`
- [x] `read_from(stream, from, max_bytes)` with zero-copy
- [x] CRC checksums per record

#### 1.5 `vdb-directory` - Placement Router ✅
- [x] `group_for_placement(placement) -> GroupId`
- [x] PHI streams → regional group
- [x] Non-PHI streams → global group

#### 1.6 `vdb-vsr` - Consensus Abstraction ✅
- [x] `trait GroupReplicator { async fn propose(group, cmd) }`
- [x] `SingleNodeGroupReplicator` - dev mode (commits immediately)

#### 1.7 `vdb-runtime` - Orchestrator ✅
- [x] `Runtime<R: GroupReplicator>`
- [x] `create_stream()` - route via directory, propose
- [x] `append()` - validate placement, propose
- [x] `execute_effects()` - StorageAppend works, WakeProjection is stubbed

---

### Phase 2: Projections (Milestone B) ← CURRENT

**Goal**: Dual-mode projection engine with SQLCipher support.

#### 2.1 Projection Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  Developer Code (any ORM: sqlx, diesel, sea-orm)                 │
└──────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│                VerityDB Connection Wrapper                       │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  Mode: CRUD                     Mode: Power User           │  │
│  │  - SqlInterceptor parses SQL    - Custom events            │  │
│  │  - Auto-generates events        - Custom handlers          │  │
│  │  - SELECT → fast path           - Full control             │  │
│  └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
              │                                    │
              │ writes (on COMMIT)                 │ reads (fast path)
              ▼                                    ▼
┌─────────────────────────┐          ┌─────────────────────────────┐
│     Event Log           │          │    SQLite Projection        │
│  (append-only, durable) │────────► │  (queryable state)          │
└─────────────────────────┘  replay  └─────────────────────────────┘
```

#### 2.2 `vdb-projections` Module Structure

```
crates/vdb-projections/src/
    lib.rs                 # ProjectionEngine, mode configuration, re-exports
    error.rs               # Extended error types
    pool.rs                # SQLite connection pools (existing)
    checkpoint.rs          # Checkpoint tracking (existing, needs fixes)

    # Shared abstractions
    event.rs               # Event trait, EventEnvelope
    runner.rs              # ProjectionRunner - applies events to SQLite

    # CRUD Mode
    crud/
        mod.rs             # CRUD module entry
        schema.rs          # SchemaRegistry - parsed from migrations
        interceptor.rs     # SqlInterceptor - routes SQL statements
        transaction.rs     # TransactionBuffer - holds events until COMMIT
        events.rs          # CrudEvent types (Insert/Update/Delete)

    # Power User Mode
    power/
        mod.rs             # Power user module entry
        handler.rs         # ProjectionHandler trait
```

#### 2.3 CRUD Mode Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| SQL Interception | Proxy + Migration Scan | Most transparent, any ORM works |
| Transaction Handling | Buffer until COMMIT | Clean log, atomic event writes |
| UPDATE Events | Full row snapshots | Enables point-in-time reconstruction |
| Mode Mixing | Single mode per DB | Simpler mental model |

#### 2.4 Key Data Structures

**Mode Configuration:**
```rust
pub enum ProjectionMode {
    PowerUser,
    Crud(CrudConfig),
}

pub struct CrudConfig {
    pub migrations_path: PathBuf,
}
```

**CRUD Events:**
```rust
pub enum CrudEvent {
    Insert {
        table: String,
        row_id: i64,
        columns: Vec<String>,
        values: Vec<JsonValue>,
    },
    Update {
        table: String,
        row_id: i64,
        old_values: Vec<JsonValue>,  // Full row before
        new_values: Vec<JsonValue>,  // Full row after
    },
    Delete {
        table: String,
        row_id: i64,
        deleted_row: Vec<JsonValue>, // Full row at deletion
    },
}
```

**Schema Registry:**
```rust
pub struct SchemaRegistry {
    tables: HashMap<String, TableSchema>,
}

pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub primary_key: Vec<String>,
}
```

**Transaction Buffer:**
```rust
pub struct TransactionBuffer {
    tx_id: u64,
    events: VecDeque<CrudEvent>,
}
// Events held in memory until COMMIT, then written atomically
```

**Power User Handler:**
```rust
#[async_trait]
pub trait ProjectionHandler: Send + Sync {
    type Event: Event;
    fn name(&self) -> &str;
    async fn handle(&self, event: Self::Event, db: &SqlitePool) -> Result<()>;
}
```

#### 2.5 Implementation Steps

- [ ] **Step 1**: Fix existing compilation errors in `checkpoint.rs`
  - Implement `sqlx::Type<Sqlite>` and `sqlx::Decode` for `Offset`
  - Fix doc comment placement

- [ ] **Step 2**: Add dependencies to `vdb-projections/Cargo.toml`
  ```toml
  sqlparser = "0.53"    # SQL parsing
  serde_json = "1"      # Event serialization
  dashmap = "6"         # Concurrent transaction map
  ```

- [ ] **Step 3**: Implement shared event abstractions
  - `event.rs` - `Event` trait, `EventEnvelope`

- [ ] **Step 4**: Implement CRUD mode schema registry
  - `crud/schema.rs` - Parse CREATE TABLE from migrations
  - `crud/events.rs` - `CrudEvent` enum

- [ ] **Step 5**: Implement SQL interceptor
  - `crud/interceptor.rs` - Use sqlparser-rs
  - SELECT → `DirectRead` (fast path to SQLite)
  - INSERT/UPDATE/DELETE → generate `CrudEvent`, return `Mutation`
  - BEGIN/COMMIT/ROLLBACK → `TransactionControl`

- [ ] **Step 6**: Implement transaction buffer
  - `crud/transaction.rs`
  - `TransactionManager` holds active transactions per connection
  - `commit()` drains buffer, serializes, appends to event log

- [ ] **Step 7**: Implement ProjectionRunner
  - `runner.rs`
  - Called on `WakeProjection` effect
  - Reads events from storage, applies to SQLite, updates checkpoint

- [ ] **Step 8**: Implement Power User mode
  - `power/handler.rs` - `ProjectionHandler` trait
  - Registration API for custom handlers

- [ ] **Step 9**: Create ProjectionEngine entry point
  - `lib.rs` - `ProjectionEngine` struct
  - `handle_wake()` for effect handling
  - `execute()` / `query()` for CRUD mode SQL

- [ ] **Step 10**: Integrate with runtime
  - Wire `WakeProjection` effect to projection engine
  - Add `vdb-projections` dependency to `vdb-runtime`

#### 2.6 Files to Modify

| File | Change |
|------|--------|
| `crates/vdb-projections/src/lib.rs` | Add `ProjectionEngine`, mode config |
| `crates/vdb-projections/src/checkpoint.rs` | Fix sqlx trait bounds for `Offset` |
| `crates/vdb-projections/Cargo.toml` | Add sqlparser, serde_json, dashmap |
| `crates/vdb-runtime/src/lib.rs` | Wire `WakeProjection` to projection engine |
| `crates/vdb-runtime/Cargo.toml` | Add vdb-projections dependency |

#### 2.7 New Files to Create

| File | Purpose |
|------|---------|
| `crates/vdb-projections/src/event.rs` | Event trait, envelope |
| `crates/vdb-projections/src/runner.rs` | ProjectionRunner |
| `crates/vdb-projections/src/crud/mod.rs` | CRUD module |
| `crates/vdb-projections/src/crud/schema.rs` | SchemaRegistry |
| `crates/vdb-projections/src/crud/interceptor.rs` | SQL parser/router |
| `crates/vdb-projections/src/crud/transaction.rs` | Transaction buffering |
| `crates/vdb-projections/src/crud/events.rs` | CrudEvent types |
| `crates/vdb-projections/src/power/mod.rs` | Power user module |
| `crates/vdb-projections/src/power/handler.rs` | ProjectionHandler trait |

---

### Phase 3: Wire Protocol (Milestone C)

**Goal**: Cap'n Proto RPC, end-to-end client-server communication.

#### 3.1 `vdb-wire` - Cap'n Proto
- [ ] Schema: `PublishReq`, `PublishRes`, `CreateStreamReq`
- [ ] `build.rs` with capnpc
- [ ] Re-export generated types

#### 3.2 `vdb-server` - RPC Server
- [ ] TCP listener with Cap'n Proto RPC
- [ ] `VerityDb::publish()` implementation
- [ ] `VerityDb::create_stream()` implementation
- [ ] mTLS placeholder

#### 3.3 `vdb-client` - SDK
- [ ] `VerityClient::connect(addr)`
- [ ] `publish(tenant, stream, payloads, opts) -> Offsets`
- [ ] `Durability::LocalQuorum | GeoDurable`

---

### Phase 4: VSR Clustering (Milestone D)

**Goal**: 3-node quorum durability, leader failover.

#### 4.1 Real VSR Implementation
- [ ] `VsrGroup` - manages one consensus group
- [ ] Prepare/Commit phases
- [ ] Quorum tracking (majority)
- [ ] View change protocol
- [ ] Log persistence (command log + commit index)

#### 4.2 Cluster Membership
- [ ] Static config initially
- [ ] Reconfiguration command (later)

#### 4.3 Snapshotting
- [ ] Periodic state machine snapshots
- [ ] Snapshot install for new/lagging replicas

#### 4.4 Network Layer
- [ ] Node-to-node communication
- [ ] Heartbeats
- [ ] Timeout/election

---

### Phase 5: Multi-Region + Compliance (Milestone E)

**Goal**: PHI regional enforcement, geo-replication, full audit trail.

#### 5.1 Multi-Group Architecture
- [ ] Multiple VSR groups (per-region PHI groups)
- [ ] Global non-PHI group (control plane)
- [ ] Async cross-region replication for DR

#### 5.2 Policy Enforcement
- [ ] Policy is replicated metadata
- [ ] Append rejects if placement violated
- [ ] Audit event on every policy decision

#### 5.3 Encryption
- [ ] Envelope encryption for records (key_id in metadata)
- [ ] Key rotation command
- [ ] BYOK/KMS integration stub

#### 5.4 Backup/Restore
- [ ] Closed segment upload to object storage
- [ ] Projection snapshot backup
- [ ] Restore workflow

#### 5.5 Audit Stream
- [ ] Immutable audit stream per tenant
- [ ] Every command/effect logged
- [ ] Export format for regulators

---

### Phase 6: SDK + CLI Polish (Milestone F)

**Goal**: Great developer experience for healthcare startups.

#### 6.1 `vdb-admin` CLI
- [ ] `stream create --tenant --stream --data-class --placement`
- [ ] `projection init --config`
- [ ] `migrate apply --db --dir`
- [ ] `projection replay --from`
- [ ] `projection run` (continuous)
- [ ] `projection status`

#### 6.2 First-Party Projection Templates
- [ ] `LatestById` - common healthcare pattern
- [ ] `Timeline` - patient timeline view
- [ ] Config-driven schema generation

---

## Example Usage

### CRUD Mode (Transparent SQL)

```rust
// Developer just uses normal SQL - feels like SQLite
let db = VerityDb::open_crud("./data", "./migrations", &key).await?;

// INSERT → event generated automatically
sqlx::query("INSERT INTO patients (name, dob) VALUES (?, ?)")
    .bind("John Doe")
    .bind("1990-01-15")
    .execute(&db.pool())
    .await?;

// SELECT → fast path, directly to SQLite
let patients: Vec<Patient> = sqlx::query_as("SELECT * FROM patients")
    .fetch_all(&db.pool())
    .await?;

// Under the hood: full audit trail, point-in-time recovery, compliance "for free"
```

### Power User Mode (Custom Events)

```rust
// Developer defines domain events
#[derive(Serialize, Deserialize)]
struct PatientAdmitted {
    patient_id: Uuid,
    ward: String,
    admitted_at: DateTime<Utc>,
}

// Developer implements custom projection
struct AdmissionProjection;

#[async_trait]
impl ProjectionHandler for AdmissionProjection {
    type Event = PatientAdmitted;

    fn name(&self) -> &str { "admissions" }

    async fn handle(&self, event: PatientAdmitted, db: &SqlitePool) -> Result<()> {
        sqlx::query("INSERT INTO current_admissions (patient_id, ward, admitted_at) VALUES (?, ?, ?)")
            .bind(event.patient_id)
            .bind(event.ward)
            .bind(event.admitted_at)
            .execute(db)
            .await?;
        Ok(())
    }
}

// Register and use
let engine = ProjectionEngine::power_user();
engine.register(AdmissionProjection);
```

---

## Verification Plan

### Phase 2 Verification (Current)
```bash
# Build and test
cargo build --workspace
cargo test --workspace

# Integration test: CRUD mode
# 1. Create migrations
# 2. Open CRUD mode database
# 3. Execute INSERT
# 4. Verify event in log
# 5. Verify row in projection
```

### Phase 3 Verification
```bash
# Start server
cargo run -p vdb-server -- --data-dir ./data --region au-syd

# Publish from client
cargo run -p vdb-client -- publish --tenant 1 --stream notes --payload "hello"

# Verify projection
sqlite3 ./data/projections/t_1/notes.db "SELECT * FROM projection_meta;"
```

---

## Dependencies

```toml
[workspace.dependencies]
# Core
anyhow = "1"
thiserror = "1"
bytes = "1"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
async-trait = "0.1"
futures = "0.3"

# Cap'n Proto
capnp = "0.19"
capnp-rpc = "0.19"
capnpc = "0.19"

# SQLite (sqlx for async, libsqlite3-sys for SQLCipher)
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
libsqlite3-sys = { version = "0.30", features = ["bundled-sqlcipher"] }

# SQL Parsing (for CRUD mode)
sqlparser = "0.53"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Concurrency
dashmap = "6"

# Utilities
uuid = { version = "1", features = ["v4", "serde"] }
time = { version = "0.3", features = ["serde"] }
tokio-util = { version = "0.7", features = ["compat"] }
tempfile = "3"
proptest = "1"
```

---

## Design Decisions (Confirmed)

1. **Region naming**: User-defined with sensible AWS defaults (e.g., `ap-southeast-2`, `us-east-1`, `eu-west-1`).

2. **Key management**: KMS interface abstraction with multiple providers:
   - `EnvKeyProvider` - reads from environment variables (self-hosted, local dev)
   - `FileKeyProvider` - reads from protected config file
   - `AwsKmsProvider` - (future) integrates with AWS KMS

3. **Event serialization**: Cap'n Proto for all events. Matches wire protocol, enables zero-copy reads.

4. **CRUD Mode**: Proxy + Migration Scan approach with:
   - Buffer events until COMMIT (atomic writes)
   - Full row snapshots for UPDATE/DELETE
   - Single mode per database (no mixing CRUD + Power User)

5. **Priority**: Dogfood in Notebar ASAP. Minimal viable single-node first, then iterate.

---

## Notebar Integration (Dogfooding)

### Notebar Events (Power User Mode)
1. `NoteCreated`, `NoteAmended`, `NoteSigned` (clinical notes)
2. `AppointmentScheduled`, `AppointmentRescheduled`, `AppointmentCancelled`
3. `PatientCreated`, `PatientUpdated`
4. `ProviderCreated`, `ProviderUpdated`

### Notebar Projections
- `notes_current` - latest version of each note
- `appointments_current` - current appointment state
- `patient_timeline` - chronological patient activity
