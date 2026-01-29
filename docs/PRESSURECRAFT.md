# PRESSURECRAFT

> *"Diamonds form 150 kilometers below the surface, at pressures exceeding 50 kilobars and temperatures above 1000°C. They remain there for billions of years—unchanged, stable, enduring. Only rare volcanic eruptions through kimberlite pipes bring them to the surface."*

Diamonds don't become valuable by accident. They're forged under immense pressure over geological time. The pressure doesn't break them—it creates their structure. Their hardness, their clarity, their brilliance—all products of conditions that would destroy lesser materials.

This document is about writing code with the same property.

---

## Why Pressurecraft?

> *"Simplicity is prerequisite for reliability."*
> — Edsger W. Dijkstra

Kimberlite is a compliance-first database for regulated industries—healthcare, finance, legal. Our users stake their businesses on our correctness. An invalid state is not a bug to fix in the next sprint; it is a fault line waiting to rupture during an audit, a lawsuit, or a breach investigation.

Our architecture mirrors the geology:
- **The append-only log** is the stable core—immutable, pressure-forged, enduring
- **Kimberlite** is the system that extracts value from that core
- **Projections** are the diamonds—valuable, structured artifacts derived from the unchanging log

We optimize for three things, in this order:

1. **Correctness** — Code that cannot be wrong is better than code that is tested to be right.
2. **Auditability** — Every state change must be traceable. If it's not in the log, it didn't happen.
3. **Simplicity** — Every abstraction is a potential crack, invisible until stress reveals it.

We do not optimize for writing speed. We optimize for *reading over decades*. The code you write today will be read by auditors, regulators, and engineers who haven't been hired yet. They will thank you or curse you based on what you leave behind.

There is no "quick fix" in Kimberlite. There is only *correct* or *fractured*.

---

## The Five Principles

### 1. Functional Core, Imperative Shell

> *"The purpose of abstraction is not to be vague, but to create a new semantic level in which one can be absolutely precise."*
> — Edsger W. Dijkstra

**This is a mandatory pattern for all Kimberlite code.**

Diamonds do not change in the depths. Earthquakes, volcanic eruptions, tectonic shifts—these happen at the surface, not in the crystalline core. The core remains inert, unchanged, *pure*.

Our kernel follows the same principle. It is a pure, deterministic state machine. All side effects—I/O, clocks, randomness—live at the edges, in the imperative shell. The shell handles the chaos of the real world. The core remains crystalline.

**The Core (Pure)**:
- Takes commands and current state
- Returns new state and effects to execute
- No I/O, no clocks, no randomness
- Trivially testable with unit tests

**The Shell (Impure)**:
- Handles RPC, authentication, network I/O
- Manages storage, file handles, sockets
- Provides clocks, random numbers when needed
- Executes effects produced by the core

**Why This Matters**:
- *Deterministic replay*: Given the same log, we get the same state. Always. This is how we prove correctness to auditors.
- *Testing*: The core can be tested exhaustively without mocks.
- *Simulation*: We can run thousands of simulated nodes in a single process.
- *Debugging*: Reproduce any bug by replaying the log.

**Function-Level FCIS**:

```rust
// GOOD: Pure core function — the unchanging diamond
fn apply_command(state: &State, cmd: Command) -> (State, Vec<Effect>) {
    match cmd {
        Command::CreateTenant { id, config } => {
            let new_state = state.with_tenant(id, config);
            let effects = vec![Effect::LogEvent(Event::TenantCreated { id })];
            (new_state, effects)
        }
        // ...
    }
}

// BAD: Impure core function — cracks in the crystal
fn apply_command(state: &mut State, cmd: Command) -> Result<()> {
    match cmd {
        Command::CreateTenant { id, config } => {
            state.add_tenant(id, config);
            log::info!("Created tenant {}", id);  // Side effect in core!
            self.storage.write(&state)?;          // I/O in core!
            Ok(())
        }
    }
}
```

**Struct-Level FCIS** (for types requiring randomness):

Every type that needs I/O must separate pure core from impure shell. This is structural—the pattern is encoded in the type itself.

```rust
impl EncryptionKey {
    // ========================================================================
    // Functional Core (pure, testable)
    // ========================================================================

    /// Pure construction from bytes — no IO, fully testable.
    /// Restricted to pub(crate) to prevent misuse with weak random.
    pub(crate) fn from_random_bytes(bytes: [u8; 32]) -> Self {
        debug_assert!(bytes.iter().any(|&b| b != 0), "bytes are all zeros");
        Self(bytes)
    }

    /// Restoration from stored bytes — pure.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }

    // ========================================================================
    // Imperative Shell (IO boundary)
    // ========================================================================

    /// Generates a new key — IO happens here, delegates to pure core.
    pub fn generate() -> Self {
        let random_bytes = generate_random();  // IO isolated here
        Self::from_random_bytes(random_bytes)  // Delegate to pure function
    }
}
```

The pattern is always the same:
1. `from_random_bytes()` — Pure core, `pub(crate)` to prevent weak input
2. `from_bytes()` — Pure restoration from storage
3. `generate()` — Impure shell that calls the pure core

---

### 2. Make Illegal States Unrepresentable

> *"I call it my billion-dollar mistake. It was the invention of the null reference in 1965."*
> — Tony Hoare

A flaw in a diamond is a place where invalid structures can exist. The goal is to eliminate flaws entirely—to build structures that *cannot* fracture because the fracture planes don't exist.

Use Rust's type system to prevent bugs at compile time, not runtime. If the compiler accepts it, it should be correct. If it's incorrect, the compiler should reject it.

**Use enums over booleans**:
```rust
// BAD: Boolean blindness — two bools = four states, only three are valid
struct Request {
    is_authenticated: bool,
    is_admin: bool,
}

// GOOD: States are explicit — impossible to be admin but not authenticated
enum RequestAuth {
    Anonymous,
    Authenticated(UserId),
    Admin(AdminId),
}
```

**Use newtypes over primitives**:
```rust
// BAD: Three u64s — which is which?
fn transfer(from: u64, to: u64, amount: u64) -> Result<()>;

// GOOD: Types prevent mixups at compile time
fn transfer(from: TenantId, to: TenantId, amount: Credits) -> Result<()>;
```

**Use Option/Result over sentinel values**:
```rust
// BAD: Magic value — what if -1 is a valid offset someday?
const NOT_FOUND: i64 = -1;
fn find_offset(key: &[u8]) -> i64;

// GOOD: Explicit absence
fn find_offset(key: &[u8]) -> Option<Offset>;
```

**Encode state machines in types** — compile-time crystallization:
```rust
// BAD: Runtime state checking — easy to forget, easy to get wrong
struct Transaction {
    state: TransactionState,
}
impl Transaction {
    fn commit(&mut self) -> Result<()> {
        if self.state != TransactionState::Prepared {
            return Err(Error::InvalidState);  // Runtime explosion
        }
        // ...
    }
}

// GOOD: Compile-time state enforcement — invalid transitions don't compile
struct PreparedTransaction { /* ... */ }
struct CommittedTransaction { /* ... */ }

impl PreparedTransaction {
    fn commit(self) -> CommittedTransaction {
        // Can only be called on PreparedTransaction — by construction
        CommittedTransaction { /* ... */ }
    }
}
```

---

### 3. Parse, Don't Validate

> *"Data dominates. If you've chosen the right data structures and organized things well, the algorithms will almost always be self-evident."*
> — Rob Pike

Carbon becomes diamond through pressure and time. Once crystallized, it doesn't need to be re-validated as diamond. The transformation is permanent.

Validation is checking that data meets constraints. Parsing is *transforming* data into a representation that *cannot violate* those constraints. Validate once, at the boundary. Parse into types that carry the proof of validity with them.

**The Pattern**:
1. Untrusted input arrives (bytes, JSON, user strings)
2. Parse into strongly-typed representation (or reject with clear error)
3. All internal code works with known-valid types
4. Never re-validate what's already been parsed

```rust
// BAD: Validate repeatedly — carbon that never crystallizes
fn process_tenant_id(id: &str) -> Result<()> {
    if !is_valid_tenant_id(id) {
        return Err(Error::InvalidTenantId);
    }
    // ... use id as &str, hoping every caller remembered to validate
}

// GOOD: Parse once, use safely — diamond
struct TenantId(u64);

impl TenantId {
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let id: u64 = s.parse()?;
        if id == 0 {
            return Err(ParseError::ZeroId);
        }
        Ok(TenantId(id))  // This TenantId is valid by construction
    }
}

fn process_tenant(id: TenantId) {
    // id is guaranteed valid — the type system carries the proof
}
```

**Apply at every boundary**:
- *Network*: Parse wire protocol into Request types
- *Storage*: Parse bytes into Record types
- *Config*: Parse TOML/JSON into Config types
- *User input*: Parse strings into domain types

The boundary is where the pressure is applied. Everything inside is crystalline.

---

### 4. Assertion Density

> *"Program testing can be used to show the presence of bugs, but never to show their absence."*
> — Edsger W. Dijkstra

Geologists don't wait for eruptions to discover the earth's structure. They deploy seismic sensors—instruments that detect structural problems before they propagate into catastrophe.

Assertions are our seismic sensors. Every function should have at least two: a precondition (what must be true when we enter) and a postcondition (what must be true when we leave). These aren't just checks—they're executable documentation of invariants.

**Assertions Document Invariants**:
```rust
fn apply_batch(log: &mut Log, batch: WriteBatch) -> AppliedIndex {
    // Precondition: batch must be non-empty
    assert!(!batch.is_empty(), "empty batch submitted");

    // Precondition: batch must be in order (expensive, debug only)
    debug_assert!(batch.is_sorted(), "batch entries out of order");

    let start_idx = log.next_index();

    for entry in batch {
        log.append(entry);
    }

    let end_idx = log.next_index();

    // Postcondition: we wrote exactly batch.len() entries
    assert_eq!(
        end_idx.0 - start_idx.0,
        batch.len() as u64,
        "applied count mismatch"
    );

    AppliedIndex(end_idx.0 - 1)
}
```

**Paired Assertions** — write site and read site:

Like core samples that verify the geological record, paired assertions verify that what was written is what gets read.

```rust
// Write site
fn write_record(storage: &mut Storage, record: &Record) {
    let checksum = crc32(&record.data);
    storage.write_u32(checksum);
    storage.write(&record.data);
}

// Read site — paired assertion verifies write
fn read_record(storage: &Storage, offset: Offset) -> Record {
    let stored_checksum = storage.read_u32(offset);
    let data = storage.read_bytes(offset + 4);
    let computed_checksum = crc32(&data);

    assert_eq!(
        stored_checksum, computed_checksum,
        "record corruption at offset {:?}", offset
    );

    Record { data }
}
```

**Debug vs Release**:
- `assert!()` — Critical invariants. Always checked. The ground truth.
- `debug_assert!()` — Expensive checks. Debug builds only. The deep survey.

```rust
fn process_entries(entries: &[Entry]) {
    // Cheap: always check
    assert!(!entries.is_empty());

    // Expensive: debug only — O(n) check
    debug_assert!(entries.windows(2).all(|w| w[0].index < w[1].index));
}
```

---

### 5. Explicit Control Flow

> *"There are two ways of constructing a software design: One way is to make it so simple that there are obviously no deficiencies, and the other way is to make it so complicated that there are no obvious deficiencies."*
> — C.A.R. Hoare

Deep geological processes are where pressure builds invisibly until the eruption arrives. Hidden control flow is the software equivalent: callbacks, implicit recursion, unbounded loops. You don't see the problem building until it's too late.

Control flow should be visible and bounded. No hidden depths. No unexpected collapses.

**No Recursion** — convert to explicit iteration with bounds:
```rust
// BAD: Unbounded recursion — hidden pressure
fn traverse(node: &Node) {
    process(node);
    for child in &node.children {
        traverse(child);  // Stack overflow waiting to happen
    }
}

// GOOD: Explicit iteration with known depth
fn traverse(root: &Node, max_depth: usize) {
    let mut stack = vec![(root, 0)];

    while let Some((node, depth)) = stack.pop() {
        assert!(depth <= max_depth, "max depth exceeded");
        process(node);

        for child in &node.children {
            stack.push((child, depth + 1));
        }
    }
}
```

**Push Ifs Up, Fors Down** — clear layering:

Like the thermal structure of the Earth—distinct layers with known boundaries—control flow should stratify cleanly. Decisions at the top, iteration at the bottom.

```rust
// BAD: Conditionals buried in the depths
fn process_all(items: &[Item], mode: Mode) {
    for item in items {
        process_one(item, mode);  // Branch hidden inside
    }
}

// GOOD: Decisions at the surface, iteration below
fn process_all(items: &[Item], mode: Mode) {
    match mode {
        Mode::Fast => {
            for item in items {
                process_fast(item);
            }
        }
        Mode::Safe => {
            for item in items {
                process_safe(item);
            }
        }
    }
}
```

**Bounded Loops** — known depths:
```rust
// BAD: Unbounded retry — drilling forever
loop {
    if try_connect().is_ok() {
        break;
    }
}

// GOOD: Explicit bounds — we know how deep we'll go
const MAX_RETRIES: usize = 3;
for attempt in 0..MAX_RETRIES {
    if try_connect().is_ok() {
        return Ok(());
    }
}
Err(Error::ConnectionFailed { attempts: MAX_RETRIES })
```

---

## Code Style

Names are the stratigraphy of your codebase. Run your eye down a file and you should read its history—what came from where, what belongs to what, what relates to what.

### Naming Conventions

**General Rules**:
- `snake_case` for functions, variables, modules
- `PascalCase` for types, traits, enums
- `SCREAMING_SNAKE_CASE` for constants
- No abbreviations except: `id`, `idx`, `len`, `ctx`

**Domain-Specific Names**:
```rust
// IDs are always suffixed — the type tells you what you're holding
tenant_id: TenantId
record_id: RecordId
stream_id: StreamId

// Indexes are always suffixed
applied_idx: AppliedIndex
commit_idx: CommitIndex

// Offsets are explicit about what they offset
byte_offset: ByteOffset
log_offset: LogOffset
```

**Verb Conventions**:
```rust
// Constructors
fn new() -> Self              // Infallible, default config
fn with_config(cfg) -> Self   // Infallible, custom config
fn try_new() -> Result<Self>  // Fallible

// Conversions
fn as_bytes(&self) -> &[u8]   // Borrowed view, no allocation
fn to_bytes(&self) -> Vec<u8> // Owned copy, allocates
fn into_bytes(self) -> Vec<u8> // Consuming, may or may not allocate

// Queries
fn is_empty(&self) -> bool    // Boolean predicate
fn len(&self) -> usize        // Count
fn get(&self, k) -> Option<V> // Fallible lookup
fn find(&self, k) -> Option<V> // Search

// Mutations
fn set(&mut self, v)          // Replace
fn push(&mut self, v)         // Append
fn insert(&mut self, k, v)    // Add with key
fn remove(&mut self, k)       // Delete
fn clear(&mut self)           // Reset
```

### Function Structure

**70-Line Soft Limit**: If a function exceeds 70 lines, it probably does too much. This isn't a hard rule—it's a warning.

**Main Logic First**:
```rust
impl Storage {
    // Public API at the top — the surface
    pub fn append(&mut self, record: Record) -> Offset {
        self.validate_record(&record);
        let offset = self.write_record(&record);
        self.update_index(offset, &record);
        offset
    }

    pub fn read(&self, offset: Offset) -> Record {
        // ...
    }

    // Private helpers below — the depths
    fn validate_record(&self, record: &Record) { /* ... */ }
    fn write_record(&mut self, record: &Record) -> Offset { /* ... */ }
}
```

**Early Returns for Guard Clauses**:
```rust
fn process(input: &Input) -> Result<Output> {
    // Reject invalid states immediately
    if input.is_empty() {
        return Err(Error::EmptyInput);
    }
    if !input.is_valid() {
        return Err(Error::InvalidInput);
    }

    // Happy path follows — no nesting, clear flow
    let result = transform(input);
    Ok(result)
}
```

### Module Organization

```
crate_name/
├── lib.rs           # Public API, re-exports
├── types.rs         # Core types (or types/)
├── error.rs         # Error types
├── traits.rs        # Public traits
├── internal/        # Private implementation
│   ├── mod.rs
│   ├── parser.rs
│   └── writer.rs
└── tests/           # Integration tests
    └── integration.rs
```

---

## Error Handling

Errors are not failures. Errors are information. The only true failure is an error that goes unrecorded—a flaw that propagates without triggering a sensor.

### Error Types

Use `thiserror` for library errors (specific, typed), `anyhow` for application errors (convenient, contextual):

```rust
// Library code: Specific error types with full context
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("record not found at offset {0:?}")]
    NotFound(Offset),

    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: u32, actual: u32 },

    #[error("storage full: {current} / {max} bytes")]
    StorageFull { current: u64, max: u64 },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// Application code: Anyhow for convenient context chaining
use anyhow::{Context, Result};

fn load_config() -> Result<Config> {
    let path = find_config_path()?;
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config from {:?}", path))?;
    let config: Config = toml::from_str(&content)
        .context("failed to parse config")?;
    Ok(config)
}
```

### No Unwrap in Library Code

```rust
// BAD: Panic on error — silent fracture
fn get_record(&self, offset: Offset) -> Record {
    self.storage.read(offset).unwrap()
}

// GOOD: Propagate error — information preserved
fn get_record(&self, offset: Offset) -> Result<Record, StorageError> {
    self.storage.read(offset)
}
```

### Expect Over Unwrap for True Invariants

When unwrap is justified (states that are truly impossible if the code is correct), use `expect` with a reason. The reason is documentation—both for future readers and for the panic message if you were wrong.

```rust
// BAD: No context for impossible panic
let first = items.first().unwrap();

// GOOD: Documents why this can't fail
let first = items.first().expect("items guaranteed non-empty by validation above");
```

---

## Dependency Policy

Every dependency is a foreign formation attached to your crystal. It may be stable, or it may introduce flaws you can't see. Question whether it belongs.

### Tier 1: Trusted Core
Well-audited, stable, load-bearing. Always acceptable:
- `std` (Rust standard library)
- `serde`, `serde_json` (serialization)
- `thiserror`, `anyhow` (errors)
- `tracing` (logging/observability)
- `bytes` (byte manipulation)

### Tier 2: Carefully Evaluated
Use when necessary, evaluate each version:
- `mio` (async I/O primitives)
- `tokio` (async runtime—minimize features)
- `sqlparser` (SQL parsing)
- `proptest` (property testing)

### Tier 3: Cryptography
Never roll our own. Use well-audited crates only:
- `sha2` (SHA-256, FIPS 180-4)
- `ed25519-dalek` (signatures, FIPS 186-5)
- `aes-gcm` (AES-256-GCM, FIPS 197)
- `getrandom` (OS CSPRNG, SP 800-90A/B)

### Dependency Checklist

Before adding any dependency:

1. **Necessity**: Can we implement this in under 200 lines?
2. **Quality**: Is it maintained? Last commit? Issue response time?
3. **Security**: Has it been audited? Any CVEs?
4. **Size**: Impact on compile time and binary size?
5. **Stability**: Stable API? Does it follow semver?
6. **Transitive deps**: What does it pull in?

```toml
# Document why each dependency exists
[dependencies]
# Core serialization — stable, well-audited, used everywhere
serde = { version = "1", features = ["derive"] }

# Error handling for library code — zero cost, standard practice
thiserror = "2"

# Structured logging — async-safe, widely adopted
tracing = "0.1"
```

---

## The Deep Time Perspective

> *"Audit trails are strata."*

Run your finger down a cliff face and you're reading history. Every layer tells what happened, when, and in what order. If it's not in the rock, it wasn't deposited. If it's not in the log, it didn't happen.

Kimberlite is built for industries where code outlasts careers. The engineer who wrote a function may have left the company years ago. The regulator reading the audit trail has never seen your codebase. The security researcher examining a breach has only the evidence you left behind.

Write code with the patience of deep time:

- **Make it compile-time** over runtime — let the type system carry proofs
- **Make it explicit** over implicit — no hidden control flow, no magic
- **Make it boring** over clever — obvious code survives; clever code erodes
- **Make it traceable** over convenient — if it's not logged, it didn't happen

Clever code is tectonic drift—imperceptible motion that ends in earthquakes. You won't notice the problem until the audit, the breach, the lawsuit. By then, the original author is gone and you're reading code that seemed reasonable at the time.

---

## Closing

> *"In the depth of winter, I finally learned that within me there lay an invincible summer."*
> — Albert Camus

Diamonds survive not through resistance, but through structural perfection. They have no flaws to exploit, no hidden weaknesses to fracture under pressure. They don't fight the forces around them—they simply have nothing for them to break.

Write code with the same property.

Write code that will still be correct when the auditor arrives. Write code that will still be readable when you've moved on. Write code with the patience of deep time—because in regulated industries, someone will be reading it in ten years, and they will thank you or curse you based on what you write today.

Be the diamond.
