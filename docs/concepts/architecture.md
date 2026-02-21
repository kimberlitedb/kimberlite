---
title: "Architecture"
section: "concepts"
slug: "architecture"
order: 2
---

# Architecture

High-level system architecture of Kimberlite.

## Core Invariant

Everything in Kimberlite derives from a single invariant:

```
State = Apply(InitialState, Log)
```

Or more precisely:

```
One ordered log → Deterministic apply → Snapshot state
```

**Implications:**

1. **The log is the source of truth**. The log is not a write-ahead log for a database—it IS the database. State is just a cache.

2. **State is derived, not authoritative**. Projections (materialized views) can be rebuilt at any time by replaying the log from the beginning.

3. **Replay must be deterministic**. Given the same log and the same initial state, apply must produce identical state. No randomness, no clocks, no external dependencies in the apply function.

4. **Consensus before acknowledgment**. A write is not acknowledged until it is durably committed to the log and replicated to a quorum.

## System Overview

<div class="doc-diagram-wrapper">
<figure class="interactive-section__figure"
        data-signals="{activeLayer: ''}"
        tabindex="0">
  <header class="interactive-section__figure-header">
    <span class="interactive-section__fig-label">Fig. 1</span>
    <span class="interactive-section__fig-caption">Five-layer architecture — dependencies flow downward. Click a layer to highlight its crates.</span>
  </header>

  <div class="interactive-section__figure-content arch-layers">

    <div class="arch-layers__row"
         role="button"
         tabindex="0"
         data-class:is-active="$activeLayer === 'client'"
         data-class:is-dimmed="$activeLayer && $activeLayer !== 'client'"
         data-on:click="$activeLayer = $activeLayer === 'client' ? '' : 'client'"
         data-on:keydown="(evt.key === 'Enter' || evt.key === ' ') && ($activeLayer = $activeLayer === 'client' ? '' : 'client')">
      <span class="arch-layers__label">Client Layer</span>
      <div class="arch-layers__crates">
        <span class="arch-layers__crate">kimberlite</span>
        <span class="arch-layers__crate">kimberlite-client</span>
        <span class="arch-layers__crate">kimberlite-admin</span>
      </div>
      <span class="arch-layers__arrow" aria-hidden="true">↓</span>
    </div>

    <div class="arch-layers__row"
         role="button"
         tabindex="0"
         data-class:is-active="$activeLayer === 'protocol'"
         data-class:is-dimmed="$activeLayer && $activeLayer !== 'protocol'"
         data-on:click="$activeLayer = $activeLayer === 'protocol' ? '' : 'protocol'"
         data-on:keydown="(evt.key === 'Enter' || evt.key === ' ') && ($activeLayer = $activeLayer === 'protocol' ? '' : 'protocol')">
      <span class="arch-layers__label">Protocol Layer</span>
      <div class="arch-layers__crates">
        <span class="arch-layers__crate">kimberlite-wire</span>
        <span class="arch-layers__crate">kimberlite-server</span>
      </div>
      <span class="arch-layers__arrow" aria-hidden="true">↓</span>
    </div>

    <div class="arch-layers__row"
         role="button"
         tabindex="0"
         data-class:is-active="$activeLayer === 'coordination'"
         data-class:is-dimmed="$activeLayer && $activeLayer !== 'coordination'"
         data-on:click="$activeLayer = $activeLayer === 'coordination' ? '' : 'coordination'"
         data-on:keydown="(evt.key === 'Enter' || evt.key === ' ') && ($activeLayer = $activeLayer === 'coordination' ? '' : 'coordination')">
      <span class="arch-layers__label">Coordination Layer</span>
      <div class="arch-layers__crates">
        <span class="arch-layers__crate">kmb-runtime</span>
        <span class="arch-layers__crate">kimberlite-directory</span>
      </div>
      <span class="arch-layers__arrow" aria-hidden="true">↓</span>
    </div>

    <div class="arch-layers__row"
         role="button"
         tabindex="0"
         data-class:is-active="$activeLayer === 'core'"
         data-class:is-dimmed="$activeLayer && $activeLayer !== 'core'"
         data-on:click="$activeLayer = $activeLayer === 'core' ? '' : 'core'"
         data-on:keydown="(evt.key === 'Enter' || evt.key === ' ') && ($activeLayer = $activeLayer === 'core' ? '' : 'core')">
      <span class="arch-layers__label">Core Layer</span>
      <div class="arch-layers__crates">
        <span class="arch-layers__crate">kimberlite-kernel</span>
        <span class="arch-layers__crate">kimberlite-vsr</span>
        <span class="arch-layers__crate">kimberlite-store</span>
        <span class="arch-layers__crate">kimberlite-query</span>
      </div>
      <span class="arch-layers__arrow" aria-hidden="true">↓</span>
    </div>

    <div class="arch-layers__row"
         role="button"
         tabindex="0"
         data-class:is-active="$activeLayer === 'foundation'"
         data-class:is-dimmed="$activeLayer && $activeLayer !== 'foundation'"
         data-on:click="$activeLayer = $activeLayer === 'foundation' ? '' : 'foundation'"
         data-on:keydown="(evt.key === 'Enter' || evt.key === ' ') && ($activeLayer = $activeLayer === 'foundation' ? '' : 'foundation')">
      <span class="arch-layers__label">Foundation Layer</span>
      <div class="arch-layers__crates">
        <span class="arch-layers__crate">kimberlite-types</span>
        <span class="arch-layers__crate">kimberlite-crypto</span>
        <span class="arch-layers__crate">kimberlite-storage</span>
      </div>
      <span class="arch-layers__arrow" aria-hidden="true" style="visibility:hidden">↓</span>
    </div>

  </div>

  <figcaption class="interactive-section__figure-footer [ cluster ]">
    <div class="interactive-section__legend">
      <span class="legend-item">Click a layer to highlight it — click again to clear.</span>
    </div>
  </figcaption>
</figure>
</div>

## Five Layers

### 1. Foundation Layer

**Purpose:** Core primitives used by everything above.

- `kimberlite-types` - Entity IDs (TenantId, StreamId, Offset)
- `kimberlite-crypto` - Cryptographic primitives (SHA-256, BLAKE3, AES-GCM, Ed25519)
- `kimberlite-storage` - Append-only log with CRC32 checksums

**No dependencies on higher layers.** Can be tested in complete isolation.

### 2. Core Layer

**Purpose:** State machine, consensus, storage, and query execution.

- `kimberlite-kernel` - Pure functional state machine (Command → State + Effects)
- `kimberlite-vsr` - Viewstamped Replication consensus
- `kimberlite-store` - B+tree projection store with MVCC
- `kimberlite-query` - SQL subset parser and executor

**Dependencies:** Foundation layer only.

### 3. Coordination Layer

**Purpose:** Orchestrate propose → commit → apply → execute.

- `kmb-runtime` - Orchestrates kernel + VSR + store
- `kimberlite-directory` - Tenant-to-shard placement routing

**Dependencies:** Core layer.

### 4. Protocol Layer

**Purpose:** Network communication and serialization.

- `kimberlite-wire` - Binary wire protocol
- `kimberlite-server` - RPC server daemon

**Dependencies:** Coordination layer.

### 5. Client Layer

**Purpose:** SDKs and tools for applications.

- `kimberlite` - High-level SDK
- `kimberlite-client` - Low-level RPC client
- `kimberlite-admin` - CLI administration tool

**Dependencies:** Protocol layer.

## Data Flow (Write)

1. **Client** sends a command (e.g., INSERT) via SDK
2. **Server** receives request, validates authentication
3. **Runtime** coordinates with consensus layer
4. **VSR** replicates command to quorum of nodes
5. **Log** durably stores the committed command
6. **Kernel** applies command to derive new state (pure function)
7. **Projections** materialize state for efficient queries
8. **Client** receives acknowledgment with position token

## Data Flow (Read)

1. **Client** sends query with consistency requirement
2. **Server** routes to appropriate projection
3. **Query** layer executes against projection store
4. **Results** returned to client

## Dependency Direction

Dependencies flow downward only: Client → Protocol → Coordination → Core → Foundation. This ensures core logic (kernel, storage, crypto) can be tested in isolation without mocking network or coordination layers.

## Design Principles

### 1. Functional Core / Imperative Shell

The kernel is pure—no IO, no clocks, no randomness. All side effects live at the edges.

```rust
// Core (pure)
fn apply_committed(state: State, cmd: Command) -> Result<(State, Vec<Effect>)>

// Shell (impure)
impl Runtime {
    fn execute_effect(&mut self, effect: Effect) -> Result<()>
}
```

See [Pressurecraft](pressurecraft.md) for details on this pattern.

### 2. Make Illegal States Unrepresentable

Use the type system to prevent invalid states:

```rust
// Bad: Can tenant_id be 0? Can it be negative?
struct Request {
    tenant_id: u64,
}

// Good: TenantId is a newtype, construction validates invariants
struct Request {
    tenant_id: TenantId,  // TenantId::new(0) would panic
}
```

### 3. Parse, Don't Validate

Validate at boundaries once, then use typed representations:

```rust
// Bad: Validate everywhere
fn process(id: String) {
    assert!(id.len() == 16);
    // ... do work ...
    some_function(id);  // Must re-validate
}

// Good: Validate once, use types
fn process(id: ValidatedId) {
    // id is guaranteed valid
    some_function(id);  // No re-validation needed
}
```

### 4. Assertion Density

Every function should have 2+ assertions (preconditions and postconditions).

As of v0.2.0, 38 critical assertions are enforced in production builds (not just debug). See [Assertions](/docs/internals/testing/assertions).

### 5. No Recursion

Use bounded loops with explicit limits. Recursion can exhaust the stack in production.

```rust
// Bad
fn process(node: Node) {
    if let Some(child) = node.child {
        process(child);  // Unbounded
    }
}

// Good
fn process(mut node: Node) {
    for _ in 0..MAX_DEPTH {
        if let Some(child) = node.child {
            node = child;
        } else {
            break;
        }
    }
}
```

## Key Subsystems

- **[Data Model](data-model.md)** - Logs, events, and projections
- **[Consensus](consensus.md)** - VSR protocol
- **[Multi-tenancy](multitenancy.md)** - Tenant isolation
- **[Compliance](compliance.md)** - Audit trails and tamper evidence

## Deep Dives

For implementation details, see:

- **[Crate Structure](/docs/internals/architecture/crate-structure)** - Detailed crate organization
- **[Kernel](/docs/internals/architecture/kernel)** - State machine internals
- **[Storage](/docs/internals/architecture/storage)** - Log format and segments
- **[Cryptography](/docs/internals/architecture/crypto)** - Hash algorithms and key management

---

**Key Takeaway:** Kimberlite's architecture makes compliance natural by making the log the source of truth and deriving all state from it. This isn't a trick—it's just taking event sourcing seriously.
