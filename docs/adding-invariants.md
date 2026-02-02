# Adding Invariants to VOPR

This guide explains how to add new invariant checkers to the VOPR simulation framework and wire them into the binary.

## Overview

VOPR supports 23+ invariant checkers organized into categories:

1. **Core** (6 invariants): Always-on foundational checks
2. **VSR** (4 invariants): Consensus correctness properties
3. **Projection** (4 invariants): MVCC and state machine checks
4. **Query** (6 invariants): SQL correctness properties
5. **SQL Oracles** (3 invariants): Expensive metamorphic testing

All invariants are enabled by default except SQL oracles (opt-in due to 10-100x performance cost).

## Anatomy of an Invariant Checker

### 1. Define the Checker

Invariant checkers live in `crates/kimberlite-sim/src/`:

```rust
/// Checks that all replicas agree on committed operations.
#[derive(Debug)]
pub struct AgreementChecker {
    commits: HashMap<(ReplicaId, ViewNumber, OpNumber), ChainHash>,
}

impl AgreementChecker {
    pub fn new() -> Self {
        Self {
            commits: HashMap::new(),
        }
    }

    pub fn record_commit(
        &mut self,
        replica: ReplicaId,
        view: ViewNumber,
        op: OpNumber,
        hash: &ChainHash,
    ) -> InvariantResult {
        // Check for conflicting hashes
        if let Some(existing) = self.commits.get(&(replica, view, op)) {
            if existing != hash {
                return Err(ConsistencyViolation::new(
                    "VSR Agreement",
                    format!("Replica {} diverged at view={}, op={}", replica, view, op),
                ));
            }
        }
        self.commits.insert((replica, view, op), *hash);
        Ok(())
    }
}
```

### 2. Export from Module

Add to `crates/kimberlite-sim/src/lib.rs`:

```rust
pub use vsr_invariants::{
    AgreementChecker,
    PrefixPropertyChecker,
    // ...
};
```

## Wiring into VOPR Binary

### Step 1: Import the Checker

Add to `crates/kimberlite-sim/src/bin/vopr.rs`:

```rust
use kimberlite_sim::{
    // ... existing ...
    AgreementChecker, PrefixPropertyChecker, // Add new checkers
};
```

### Step 2: Add Configuration Flag

Add field to `InvariantConfig` struct in `vopr.rs`:

```rust
#[derive(Debug, Clone)]
struct InvariantConfig {
    // ... existing fields ...
    enable_vsr_agreement: bool,
}

impl Default for InvariantConfig {
    fn default() -> Self {
        Self {
            // ... existing defaults ...
            enable_vsr_agreement: true, // Default enabled
        }
    }
}
```

### Step 3: Conditional Instantiation

In `run_simulation()`, instantiate conditionally:

```rust
// VSR invariants
let mut vsr_agreement = config
    .invariant_config
    .enable_vsr_agreement
    .then(AgreementChecker::new);
```

Uses `Option<T>` for zero-cost abstraction when disabled.

### Step 4: Wire into Event Loop

Add checks at appropriate event type:

**For VSR invariants** (use `EventKind::Custom(3)` - replica state update):

```rust
3 => {
    // ... existing replica code ...

    // VSR Agreement check
    if let Some(ref mut checker) = vsr_agreement {
        use kimberlite_vsr::{OpNumber, ReplicaId, ViewNumber};

        let op_hash = kimberlite_crypto::ChainHash::from_bytes(&log_hash);
        let result = checker.record_commit(
            ReplicaId::new(replica_id as u8),
            ViewNumber::from(view as u64),
            OpNumber::from(op),
            &op_hash,
        );

        if !result.is_ok() {
            return make_violation(
                "vsr_agreement".to_string(),
                format!("VSR agreement violated at view={}, op={}", view, op),
                sim.events_processed(),
                &mut trace,
            );
        }
    }
}
```

**Event types by category:**
- Core: Various events (write/read/replica update)
- VSR: `EventKind::Custom(3)` (replica state)
- Projection: Need `EventKind::ProjectionApplied` (future)
- Query: Need `EventKind::QueryExecuted` (future)

### Step 5: Add CLI Arguments

Add to `parse_args()` in `vopr.rs`:

```rust
"--enable-vsr-invariants" => {
    config.invariant_config.enable_vsr_agreement = true;
    config.invariant_config.enable_vsr_prefix_property = true;
    // ... other VSR invariants ...
}

"--enable-invariant" => {
    i += 1;
    if i < args.len() {
        match args[i].as_str() {
            "vsr_agreement" => config.invariant_config.enable_vsr_agreement = true,
            // ... other invariants ...
            _ => eprintln!("Warning: Unknown invariant '{}'", args[i]),
        }
    }
}

"--disable-invariant" => {
    // Same pattern, set to false
}

"--list-invariants" => {
    println!("VSR (consensus correctness):");
    println!("  vsr_agreement, vsr_prefix_property, ...");
    std::process::exit(0);
}
```

### Step 6: Update Help Text

Add to `print_help()`:

```
INVARIANT CONTROL:
    --enable-vsr-invariants     Enable all VSR invariants
    --enable-invariant <NAME>   Enable specific invariant
    --disable-invariant <NAME>  Disable specific invariant
    --list-invariants           List all available
```

### Step 7: Update Coverage Validation

Add to `is_invariant_enabled()`:

```rust
fn is_invariant_enabled(name: &str, inv_config: &InvariantConfig) -> bool {
    match name {
        // ... existing ...
        "vsr_agreement" => inv_config.enable_vsr_agreement,
        _ => false,
    }
}
```

## Event Loop Integration Points

### Current Events

- `Custom(0)`: Write operation
- `Custom(1)`: Read operation
- `Custom(2)`: Network message
- `Custom(3)`: Replica state update ← **VSR checks here**
- `Custom(4)`: Read-Modify-Write
- `Custom(5)`: Scan operation
- `StorageComplete`: Operation completion
- `InvariantCheck`: Periodic checking

### Future Events (TODO)

```rust
ProjectionApplied {
    projection_id: u64,
    applied_position: u64,
    batch_size: usize,
},

QueryExecuted {
    query_id: u64,
    sql: String,
    result_rows: usize,
},
```

## Testing Your Invariant

### 1. Build and Basic Test

```bash
cargo build --release -p kimberlite-sim --bin vopr

./target/release/vopr \
    --iterations 1000 \
    --enable-invariant vsr_agreement \
    --core-invariants-only \
    --verbose
```

### 2. Determinism Test

```bash
./target/release/vopr \
    --iterations 100 \
    --seed 12345 \
    --check-determinism \
    --enable-invariant vsr_agreement
```

### 3. Canary Test

Create intentional bug in `canary.rs`:

```rust
#[cfg(feature = "canary-agreement-violation")]
fn simulate_violation() {
    // Intentionally commit different hashes
}
```

Map in `scripts/test-canaries.sh`:

```bash
declare -A CANARY_INVARIANTS=(
    ["canary-agreement-violation"]="vsr_agreement"
)
```

## CLI Flag Conventions

### Group Flags

```bash
--enable-vsr-invariants      # All VSR
--enable-projection-invariants
--enable-query-invariants
--enable-sql-oracles         # Opt-in (expensive)
```

### Individual Flags

```bash
--enable-invariant vsr_agreement
--disable-invariant projection_mvcc
```

### Special Flags

```bash
--core-invariants-only       # Disable all except core 6
--list-invariants            # Show all available
```

### Environment Variables

For scripts (`vopr-overnight.sh`):

```bash
export VOPR_ENABLE_VSR_INVARIANTS=1
export VOPR_ENABLE_SQL_ORACLES=1
export VOPR_CORE_ONLY=1
export VOPR_INVARIANTS="vsr_agreement,linearizability"
```

## Performance Considerations

### Zero-Cost Abstraction

```rust
// GOOD: Zero cost when disabled
let mut checker = config.enable_foo.then(FooChecker::new);

// BAD: Always allocates
let mut checker = FooChecker::new();
if config.enable_foo { ... }
```

### Expensive Invariants

Default to **disabled** for expensive checks:

```rust
impl Default for InvariantConfig {
    fn default() -> Self {
        Self {
            enable_sql_tlp: false,  // Opt-in only
        }
    }
}
```

## Integration Checklist

- [ ] Checker struct defined and exported
- [ ] Field added to `InvariantConfig`
- [ ] Default value set (true for most, false for expensive)
- [ ] Conditional instantiation in `run_simulation()`
- [ ] Wired into event loop
- [ ] CLI flags added (group and individual)
- [ ] Help text updated
- [ ] `is_invariant_enabled()` updated
- [ ] Determinism test passes
- [ ] Coverage tracking works

## Examples

### Active: VSR Agreement

✅ Fully integrated:

```rust
if let Some(ref mut checker) = vsr_agreement {
    let result = checker.record_commit(...);
    if !result.is_ok() {
        return make_violation(...);
    }
}
```

### Future: Projection MVCC

⏳ Needs `ProjectionApplied` event:

```rust
let mut _projection_mvcc = config
    .invariant_config
    .enable_projection_mvcc_visibility
    .then(MvccVisibilityChecker::new);

// TODO: Wire when event added
```

## Common Pitfalls

1. **Forgetting `Option<T>`**: Always use `if let Some(ref mut checker) = ...`
2. **Breaking determinism**: No `rand()`, no `SystemTime::now()`
3. **False positives**: Test with multiple seeds
4. **Missing coverage**: Update `is_invariant_enabled()`
5. **Type mismatches**: Cast u32→u64 for ViewNumber

## Further Reading

- [VOPR Confidence Levels](./vopr-confidence.md)
- [Canary Testing](./canary-testing.md)
- [Invariants Overview](./invariants.md)
