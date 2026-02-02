# VSR Integration into VOPR

## Overview

This document describes Phase 1 of integrating actual VSR replicas into the VOPR simulation harness. The integration enables testing VSR's Byzantine resistance at the protocol level instead of using a simplified state-based model.

## Components Implemented

### 1. VsrReplicaWrapper (`src/vsr_replica_wrapper.rs`)

Wraps `kimberlite_vsr::ReplicaState` for simulation testing:
- Manages VSR replica lifecycle
- Tracks rejected messages for Byzantine testing
- Executes effects through SimStorageAdapter
- Provides snapshots for invariant checking

**Tests**: 4/4 passing ✓

### 2. SimStorageAdapter (`src/sim_storage_adapter.rs`)

Adapts `SimStorage` for VSR effect execution:
- Handles all kernel Effect variants
- Provides deterministic storage behavior
- Integrates with SimRng for latency simulation

**Tests**: 5/5 passing ✓

### 3. VsrSimulation (`src/vsr_simulation.rs`)

High-level simulation coordinator for 3-replica VSR cluster:
- Initializes replicas with separate storage
- Processes client requests
- Delivers VSR messages between replicas
- Handles timeouts and other events
- Extracts snapshots for invariant checking

**Tests**: 5/5 passing ✓

### 4. Event Types (`src/event.rs`)

Added VSR-specific event kinds:
- `VsrClientRequest` - Client command submission
- `VsrMessage` - VSR protocol message delivery
- `VsrTimeout` - Timeout events (heartbeat, prepare, view change)
- `VsrTick` - Periodic housekeeping
- `VsrCrash` - Replica crash
- `VsrRecover` - Replica recovery

## Demo

Run the working example:

```bash
cargo run --package kimberlite-sim --example vsr_simulation_demo
```

Output shows:
- Client request processing
- Prepare message delivery to backups
- PrepareOK response handling
- Commit advancement on quorum
- Invariant validation (commit_number <= op_number)

## Integration into VOPR (Next Steps)

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        VOPR Simulation                       │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐    │
│  │ VSR Replica  │   │ VSR Replica  │   │ VSR Replica  │    │
│  │   (ID: 0)    │   │   (ID: 1)    │   │   (ID: 2)    │    │
│  └──────┬───────┘   └──────┬───────┘   └──────┬───────┘    │
│         │                  │                  │              │
│         └──────────────────┴──────────────────┘              │
│                            │                                 │
│                    ┌───────▼────────┐                        │
│                    │  MessageMutator │ ◄─ Byzantine attacks  │
│                    └───────┬────────┘                        │
│                            │                                 │
│                    ┌───────▼────────┐                        │
│                    │   SimNetwork    │ ◄─ Network faults     │
│                    └────────────────┘                        │
└─────────────────────────────────────────────────────────────┘
```

### Implementation Plan

#### Step 1: Add Mode Selection

Add `--vsr-mode` flag to VOPR CLI:

```rust
struct VoprConfig {
    // ... existing fields ...
    vsr_mode: bool,  // Use VSR replicas instead of simplified model
}
```

#### Step 2: Initialize VSR Simulation

In `run_simulation()` function:

```rust
let mut vsr_sim = if config.vsr_mode {
    Some(VsrSimulation::new(run.storage_config.clone(), run.seed))
} else {
    None
};
```

#### Step 3: Event Loop Integration

Replace simplified event processing with VSR events:

```rust
EventKind::VsrClientRequest { replica_id, command_bytes, .. } => {
    if let Some(ref mut vsr_sim) = vsr_sim {
        let messages = vsr_sim.process_client_request(&mut rng);

        // Schedule message deliveries
        for msg in messages {
            let bytes = vsr_message_to_bytes(&msg);
            let to = vsr_message_destination(&msg);

            // Apply MessageMutator if Byzantine scenario active
            let mutated_bytes = if let Some(ref mutator) = message_mutator {
                mutator.apply(&bytes, to, &mut rng).unwrap_or(bytes)
            } else {
                bytes
            };

            // Send through network
            network.send(msg.from.as_u8() as u64, to, mutated_bytes, current_time, &mut rng)?;
        }
    }
}

EventKind::VsrMessage { to_replica, message_bytes } => {
    if let Some(ref mut vsr_sim) = vsr_sim {
        let msg = vsr_message_from_bytes(&message_bytes);
        let responses = vsr_sim.deliver_message(to_replica, msg, &mut rng);

        // Schedule response deliveries...
    }
}
```

#### Step 4: Invariant Checking

Extract snapshots and run invariant checks:

```rust
if let Some(ref vsr_sim) = vsr_sim {
    let snapshots = vsr_sim.extract_snapshots();

    // Commit number consistency
    if let Some(ref mut checker) = commit_consistency_checker {
        for snapshot in &snapshots {
            checker.check_consistency(
                snapshot.replica_id,
                snapshot.op_number,
                snapshot.commit_number,
            )?;
        }
    }

    // Agreement checker
    if let Some(ref mut checker) = vsr_agreement {
        checker.check_agreement(&snapshots)?;
    }

    // Prefix property checker
    if let Some(ref mut checker) = vsr_prefix_property {
        checker.check_prefix_property(&snapshots)?;
    }
}
```

#### Step 5: Byzantine Integration

Wire up MessageMutator to mutate VSR messages:

```rust
// In Byzantine scenario config
let message_mutator = byzantine_injector.map(|inj| {
    let mut mutator = MessageMutator::new();

    // Configure mutations based on attack type
    if inj.config().inflate_commit_probability > 0.0 {
        mutator.add_rule(MessageMutationRule {
            target_replica: ReplicaId::new(2),
            message_type: MessageTypeFilter::DoViewChange,
            mutation: MessageFieldMutation::InflateCommitNumber {
                inflation_factor: 500
            },
        });
    }

    mutator
});
```

## Success Criteria

### Phase 1 (Complete) ✓

- [x] VsrReplicaWrapper created and tested
- [x] SimStorageAdapter created and tested
- [x] VSR event types added
- [x] VsrSimulation coordinator created
- [x] Demo shows end-to-end VSR operation
- [x] All 14 tests passing

### Phase 2 (Next)

- [ ] Integrate VsrSimulation into VOPR event loop
- [ ] Wire up invariant checkers to snapshots
- [ ] Add `--vsr-mode` CLI flag
- [ ] Run baseline scenario in VSR mode
- [ ] Verify determinism (same seed → same output)

### Phase 3 (Byzantine)

- [ ] Integrate MessageMutator
- [ ] Port Byzantine scenarios to VSR mode
- [ ] Verify attacks are detected (not causing test failures)
- [ ] All 27 scenarios pass in VSR mode

## Performance Considerations

**Expected overhead**: VSR mode will be slower than simplified model because:
- Real state machine processing (not just state updates)
- Effect execution through storage adapter
- Message serialization/deserialization
- Snapshot extraction for invariant checking

**Mitigation strategies**:
- Profile hot paths in Phase 5
- Reduce invariant check frequency if needed
- Use faster serialization format if bottleneck
- Keep simplified mode for quick iteration

## Testing

```bash
# Run all VSR integration tests
cargo test --package kimberlite-sim vsr_replica_wrapper
cargo test --package kimberlite-sim sim_storage_adapter
cargo test --package kimberlite-sim vsr_simulation

# Run demo
cargo run --package kimberlite-sim --example vsr_simulation_demo

# Future: Run VOPR with VSR mode
cargo run --bin vopr -- --vsr-mode --scenario baseline --iterations 100 --seed 12345
```

## Files Created

| File | Lines | Purpose | Status |
|------|-------|---------|--------|
| `src/vsr_replica_wrapper.rs` | 340 | VSR replica wrapper | ✓ Complete |
| `src/sim_storage_adapter.rs` | 320 | Storage adapter | ✓ Complete |
| `src/vsr_simulation.rs` | 280 | Simulation coordinator | ✓ Complete |
| `examples/vsr_simulation_demo.rs` | 130 | Working demo | ✓ Complete |
| `VSR_INTEGRATION.md` | (this file) | Documentation | ✓ Complete |

## Next Actions

1. **Immediate**: Create minimal VOPR integration (add `--vsr-mode` flag)
2. **Phase 2**: Wire up invariant checkers
3. **Phase 3**: Byzantine testing integration
4. **Phase 4**: View changes, crashes, recovery
5. **Phase 5**: Optimization and cleanup

## References

- Original plan: `/Users/jaredreyes/Developer/rust/kimberlite/.claude/projects/-Users-jaredreyes-Developer-rust-kimberlite/452091e4-3730-4efd-a28a-f50785a43df7.jsonl` (plan mode transcript)
- VSR implementation: `crates/kimberlite-vsr/src/replica/`
- VOPR binary: `crates/kimberlite-sim/src/bin/vopr.rs`
