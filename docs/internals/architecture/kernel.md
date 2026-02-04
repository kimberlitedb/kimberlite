# Kernel - Pure Functional State Machine

The kernel is the heart of Kimberlite: a pure, deterministic state machine.

## Core Principle: Functional Core / Imperative Shell

**The kernel is pure.** No IO, no clocks, no randomness. All side effects live at the edges.

```rust
// Core (pure) - The kernel
fn apply_committed(state: State, cmd: Command) -> Result<(State, Vec<Effect>)>

// Shell (impure) - The runtime
impl Runtime {
    fn execute_effect(&mut self, effect: Effect) -> Result<()>
}
```

This separation makes the kernel:
- **Testable:** No mocking required
- **Deterministic:** Same inputs → same outputs (always)
- **Verifiable:** Easier to prove correctness
- **Reproducible:** Replay logs to debug issues

See [Pressurecraft](../../concepts/pressurecraft.md) for the philosophy behind this pattern.

## State Machine Interface

The kernel exposes a simple interface:

```rust
pub trait StateMachine {
    type State;
    type Command;
    type Effect;
    type Error;

    /// Apply a committed command to the state.
    /// Must be deterministic: same state + command → same result.
    fn apply(
        state: &Self::State,
        command: Self::Command,
    ) -> Result<(Self::State, Vec<Self::Effect>), Self::Error>;
}
```

**Key properties:**

1. **Pure function:** No side effects during `apply()`
2. **Deterministic:** Same inputs produce same outputs
3. **Returns effects:** Side effects are data, not actions

## Command Types

Commands represent operations to perform:

```rust
pub enum Command {
    // Tenant management
    CreateTenant(CreateTenantCommand),
    DeleteTenant(DeleteTenantCommand),

    // Data operations
    Insert(InsertCommand),
    Update(UpdateCommand),
    Delete(DeleteCommand),

    // Query operations (read-only)
    Query(QueryCommand),

    // Schema operations
    CreateTable(CreateTableCommand),
    DropTable(DropTableCommand),
    CreateIndex(CreateIndexCommand),

    // System operations
    Checkpoint(CheckpointCommand),
    Compact(CompactCommand),
}
```

Each command is validated before entering the kernel:

```rust
impl Command {
    pub fn validate(&self) -> Result<()> {
        match self {
            Command::CreateTenant(cmd) => {
                if cmd.tenant_id == TenantId::new(0) {
                    return Err(Error::InvalidTenantId);
                }
                // ... more validation
            }
            // ... other commands
        }
        Ok(())
    }
}
```

## Effect Types

Effects represent side effects to execute **after** the state transition:

```rust
pub enum Effect {
    // I/O effects
    WriteToLog(WriteToLogEffect),
    FlushToDisk(FlushToDiskEffect),
    DeleteFile(DeleteFileEffect),

    // Network effects
    SendMessage(SendMessageEffect),
    BroadcastMessage(BroadcastMessageEffect),

    // Timer effects
    SetTimer(SetTimerEffect),
    CancelTimer(CancelTimerEffect),

    // Notification effects
    NotifyClient(NotifyClientEffect),
    TriggerAlert(TriggerAlertEffect),
}
```

**Why effects?**

Instead of performing IO directly, the kernel returns a list of effects. The runtime executes them:

```rust
// Kernel (pure)
fn apply_committed(state: State, cmd: Command) -> Result<(State, Vec<Effect>)> {
    let new_state = /* derive new state */;
    let effects = vec![
        Effect::WriteToLog(entry),
        Effect::NotifyClient(result),
    ];
    Ok((new_state, effects))
}

// Runtime (impure)
impl Runtime {
    fn execute_effect(&mut self, effect: Effect) -> Result<()> {
        match effect {
            Effect::WriteToLog(entry) => self.log.append(entry),
            Effect::NotifyClient(result) => self.client.send(result),
            // ... etc
        }
    }
}
```

This separation allows:
- Testing kernel without IO
- Replaying state transitions
- Auditing side effects
- Deferring IO for batching

## State Structure

The kernel maintains minimal state:

```rust
pub struct KernelState {
    // Tenant registry
    tenants: HashMap<TenantId, TenantMetadata>,

    // Schema metadata
    tables: HashMap<(TenantId, TableId), TableSchema>,
    indexes: HashMap<(TenantId, IndexId), IndexSchema>,

    // Projection state
    projections: HashMap<(TenantId, ProjectionId), ProjectionState>,

    // Idempotency tracking
    idempotency_cache: LruCache<IdempotencyId, (Position, Result)>,

    // Metrics (not persisted)
    metrics: Metrics,
}
```

**Key principle:** State is **derived from the log**. If you delete the state and replay the log, you get identical state back.

## Determinism Guarantees

The kernel is deterministic by construction:

### No Clocks

```rust
// BAD: Non-deterministic
let timestamp = Utc::now();

// GOOD: Deterministic
let timestamp = command.timestamp;  // Provided by runtime
```

Timestamps come from outside the kernel (VSR assigns them during consensus).

### No Random Numbers

```rust
// BAD: Non-deterministic
let id = rand::random::<u64>();

// GOOD: Deterministic
let id = command.id;  // Provided by client
```

IDs are generated outside the kernel (clients generate idempotency IDs).

### No IO

```rust
// BAD: Non-deterministic
let data = fs::read("config.toml")?;

// GOOD: Deterministic
let data = state.config.clone();  // State passed in
```

All data comes through function parameters, not IO.

### Bounded Loops

```rust
// BAD: Unbounded
while condition {
    // ...
}

// GOOD: Bounded
for _ in 0..MAX_ITERATIONS {
    if !condition { break; }
    // ...
}
```

Prevents infinite loops that could hang the system.

## Testing the Kernel

Because the kernel is pure, tests are simple:

```rust
#[test]
fn test_insert_command() {
    let state = KernelState::new();
    let cmd = Command::Insert(InsertCommand {
        tenant_id: TenantId::new(1),
        table: "patients".to_string(),
        data: vec![/* ... */],
    });

    let (new_state, effects) = apply_committed(&state, cmd).unwrap();

    // Assert state changed correctly
    assert_eq!(new_state.tables.len(), state.tables.len() + 1);

    // Assert effects generated
    assert_eq!(effects.len(), 2);
    assert!(matches!(effects[0], Effect::WriteToLog(_)));
    assert!(matches!(effects[1], Effect::NotifyClient(_)));
}
```

No mocks, no async, no flaky tests. Just pure functions.

## Property-Based Testing

The kernel uses property-based testing (proptest) to find edge cases:

```rust
proptest! {
    #[test]
    fn applying_commands_is_associative(
        cmds in prop::collection::vec(arbitrary_command(), 1..100)
    ) {
        let mut state1 = KernelState::new();
        let mut state2 = KernelState::new();

        // Apply commands one at a time
        for cmd in &cmds {
            let (new_state, _) = apply_committed(&state1, cmd.clone()).unwrap();
            state1 = new_state;
        }

        // Apply commands in batch (if supported)
        let (new_state, _) = apply_committed_batch(&state2, cmds).unwrap();
        state2 = new_state;

        // States should be identical
        assert_eq!(state1, state2);
    }
}
```

See [Property Testing](../testing/property-testing.md) for more examples.

## Idempotency

The kernel tracks idempotency IDs to prevent duplicate operations:

```rust
fn apply_committed(state: &State, cmd: Command) -> Result<(State, Vec<Effect>)> {
    // Check if we've seen this idempotency ID before
    if let Some(idempotency_id) = cmd.idempotency_id() {
        if let Some((position, result)) = state.idempotency_cache.get(&idempotency_id) {
            // We've already processed this command
            return Ok((state.clone(), vec![
                Effect::NotifyClient(ClientNotification {
                    result: result.clone(),
                    position: *position,
                    is_replay: true,
                })
            ]));
        }
    }

    // First time seeing this command, process it
    let (new_state, mut effects) = apply_command_inner(state, cmd)?;

    // Cache the result
    if let Some(idempotency_id) = cmd.idempotency_id() {
        new_state.idempotency_cache.insert(
            idempotency_id,
            (new_state.position, result.clone()),
        );
    }

    Ok((new_state, effects))
}
```

See [Compliance](../../concepts/compliance.md) for why idempotency matters.

## Error Handling

The kernel uses typed errors:

```rust
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("Tenant not found: {0}")]
    TenantNotFound(TenantId),

    #[error("Table not found: {tenant}/{table}")]
    TableNotFound { tenant: TenantId, table: String },

    #[error("Duplicate idempotency ID: {0}")]
    DuplicateIdempotencyId(IdempotencyId),

    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    #[error("State transition failed: {0}")]
    StateTransitionFailed(String),
}
```

**No panics in the kernel.** All errors are explicit and recoverable.

## Assertion Density

The kernel has high assertion density (2+ assertions per function):

```rust
fn apply_insert(state: &State, cmd: InsertCommand) -> Result<(State, Vec<Effect>)> {
    // Preconditions
    assert!(cmd.tenant_id != TenantId::new(0), "tenant_id cannot be zero");
    assert!(!cmd.data.is_empty(), "data cannot be empty");
    assert!(state.tenants.contains_key(&cmd.tenant_id), "tenant must exist");

    // ... apply logic ...

    // Postconditions
    assert!(new_state.position > state.position, "position must advance");
    assert!(!effects.is_empty(), "must generate at least one effect");

    Ok((new_state, effects))
}
```

See [Assertions](../testing/assertions.md) for the complete assertion strategy.

## Performance

The kernel is optimized for throughput:

- **Zero-copy:** Use `Bytes` and `Arc<[u8]>` to avoid cloning
- **Minimal allocations:** Reuse buffers where possible
- **No async:** Purely synchronous (async is in the runtime layer)
- **Small state:** Keep only what's necessary in memory

**Benchmark:** 100k+ applies/sec on commodity hardware.

## Future Work

- **Snapshots:** Checkpoint state to avoid replaying entire log
- **Parallel apply:** Apply independent commands in parallel
- **WASM kernel:** Compile kernel to WebAssembly for portability

See [ROADMAP.md](../../../ROADMAP.md) for details.

## Related Documentation

- **[Pressurecraft](../../concepts/pressurecraft.md)** - Philosophy behind FCIS pattern
- **[Testing Overview](../testing/overview.md)** - How we test the kernel
- **[Property Testing](../testing/property-testing.md)** - Property-based testing strategies
- **[Assertions](../testing/assertions.md)** - Assertion density and safety

---

**Key Takeaway:** The kernel is a pure, deterministic state machine. It takes commands and state, returns new state and effects. No IO, no clocks, no randomness—just pure functions.
