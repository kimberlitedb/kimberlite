# Pressurecraft: Learn FCIS by Building

> **Learn the Functional Core, Imperative Shell pattern by building a mini database kernel from scratch.**

Pressurecraft is an educational workspace that teaches the core architectural pattern behind Kimberlite: separating pure, deterministic logic (the "functional core") from side effects (the "imperative shell").

## Why "Pressurecraft"?

Like diamond formation under pressure, this workspace puts you under pressure to deeply understand kernel design by building it yourself. The name echoes "kimberlite" (the rock that brings diamonds to the surface) while emphasizing the craft of building pressure-tested, deterministic systems.

## Learning Path

This workspace builds complexity incrementally across five steps:

### Step 1: Pure vs. Impure Functions (`src/step1_pure_functions.rs`)

**What you'll learn:**
- The difference between pure and impure functions
- Why randomness, IO, and clocks make functions unpredictable
- How to refactor impure code into pure + shell

**Key concept:** A pure function with the same inputs ALWAYS produces the same output.

```bash
cargo test step1
```

### Step 2: Commands and Effects (`src/step2_commands_effects.rs`)

**What you'll learn:**
- How to represent state changes as data (Commands)
- How to represent side effects as data (Effects)
- The Command/Effect pattern without a kernel yet

**Key concept:** Don't DO the side effect. DESCRIBE it as data.

```bash
cargo test step2
```

### Step 3: State Machines (`src/step3_state_machine.rs`)

**What you'll learn:**
- Manual state transitions
- Validating state invariants
- State machine design with types

**Key concept:** State + Event → New State (no effects yet)

```bash
cargo test step3
```

### Step 4: Mini Kernel (`src/step4_mini_kernel.rs`)

**What you'll learn:**
- The `apply()` function signature
- Processing 2 commands: CreateStream and AppendBatch
- Producing both state changes AND effects

**Key concept:** Command → (New State, Effects)

```bash
cargo test step4
```

### Step 5: Full Kernel (`src/step5_full_kernel.rs`)

**What you'll learn:**
- Replicating Kimberlite's kernel functionality
- Handling all command types
- Rich error handling
- Precondition/postcondition assertions

**Key concept:** A production-ready kernel with full validation

```bash
cargo test step5
```

## Running Examples

### Counter Example (Simplest State Machine)

```bash
cargo run --example counter
```

Shows a trivial pure state machine: increment and decrement operations.

### Event Log Example (Append-Only Structure)

```bash
cargo run --example event_log
```

Shows how an append-only log can be modeled with FCIS.

### Mini Database Example (Realistic FCIS)

```bash
cargo run --example mini_database
```

Shows a complete mini database with CreateStream and AppendBatch, complete with validation and effects.

## Testing Determinism

The core property we're testing: **same input → same output, every time.**

```bash
# Run determinism proof tests
cargo test determinism

# Run FCIS property tests
cargo test fcis

# Run all tests with verbose output
cargo test -- --nocapture
```

## Key Design Principles

### 1. Functional Core, Imperative Shell (FCIS)

```
┌─────────────────────────────────────┐
│  Imperative Shell (Runtime)         │
│  - Reads commands from network       │
│  - Executes effects (IO, storage)   │
│  - Provides randomness, timestamps  │
│                                      │
│  ┌───────────────────────────────┐  │
│  │  Functional Core (Kernel)     │  │
│  │  - Pure functions only         │  │
│  │  - No IO, no clocks, no random │  │
│  │  - Deterministic state machine │  │
│  │  - Command → (State, Effects) │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
```

### 2. Make Illegal States Unrepresentable

Use types to enforce invariants at compile time:
- Enums over booleans
- Newtypes over primitives (`StreamId(u64)` not `u64`)
- State machines encoded in types

### 3. Parse, Don't Validate

Validate at boundaries once, then work with typed representations.

### 4. Assertion Density

Every function should have preconditions and postconditions. Assert assumptions.

## Comparing to Production Kernel

After completing the steps, compare your implementation to the real Kimberlite kernel:

| Pressurecraft | Production Kimberlite |
|---------------|----------------------|
| `step4_mini_kernel.rs:apply()` | `crates/kimberlite-kernel/src/kernel.rs:apply_committed()` |
| `step2_commands_effects.rs:Command` | `crates/kimberlite-kernel/src/command.rs:Command` |
| `step2_commands_effects.rs:Effect` | `crates/kimberlite-kernel/src/effects.rs:Effect` |
| `step3_state_machine.rs:State` | `crates/kimberlite-kernel/src/state.rs:State` |

## Video Series

This workspace is the companion to the "Inside the Kernel" video series:

1. **What is FCIS?** - Understanding pure vs impure (Step 1)
2. **The Kernel API** - Commands and Effects (Step 2)
3. **Why It Matters: Determinism** - Testing and verification (All steps)
4. **Building It Yourself** - Mini kernel walkthrough (Step 4)
5. **Production Kernel Tour** - Navigating real Kimberlite code (Step 5)

## Learning Outcomes

After completing Pressurecraft, you should be able to:

- ✅ Explain FCIS to another developer without notes
- ✅ Identify pure vs impure code by inspection
- ✅ Design a simple state machine using Command/Effect pattern
- ✅ Navigate Kimberlite's kernel.rs and understand each function's role
- ✅ Write tests that prove determinism
- ✅ Understand why separation of concerns enables replication

## Next Steps

1. **Start with Step 1:** `cargo test step1` and read `src/step1_pure_functions.rs`
2. **Run examples:** `cargo run --example counter` to see simplest case
3. **Progress through steps:** Each builds on the previous
4. **Compare to production:** Read `crates/kimberlite-kernel/src/kernel.rs` alongside Step 5
5. **Contribute:** Once you understand the kernel, you can contribute to Kimberlite!

## Philosophy

> "The best way to learn is to build it yourself."

Pressurecraft doesn't just explain FCIS - it makes you implement it from first principles. By the end, you'll have built a working kernel and understand exactly why Kimberlite is designed the way it is.

## Resources

- [Main Kimberlite Repository](../../README.md)
- [Architecture Documentation](../../docs/ARCHITECTURE.md)
- [Kimberlite Kernel Source](../../crates/kimberlite-kernel/src/kernel.rs)
- Video Series: *Coming soon*

---

**Build it. Test it. Understand it.** 🔨💎
