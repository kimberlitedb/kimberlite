---
title: "Cluster Reconfiguration Architecture Design"
section: "internals/design"
slug: "reconfiguration"
order: 2
---

# Cluster Reconfiguration Architecture Design

**Date:** 2026-02-05
**Status:** Design Phase
**Target:** Phase 4 - Cluster Operations

## Overview

This document specifies the architecture for VSR cluster reconfiguration, enabling zero-downtime addition and removal of replicas. The design is based on Raft's joint consensus algorithm, adapted for Viewstamped Replication.

## Goals

1. **Zero-Downtime Reconfiguration** - Add/remove replicas without service interruption
2. **Safety Preservation** - Never violate VSR safety guarantees during transitions
3. **Progress Guarantee** - Reconfigurations always complete or safely abort
4. **Simple API** - Easy-to-use reconfiguration commands

## Non-Goals (Future Work)

- Simultaneous multiple reconfigurations (only one at a time)
- Automatic cluster scaling based on load
- Cross-datacenter replication topology
- Dynamic leader rebalancing

## Background: Joint Consensus

### Why Joint Consensus?

Naive approach (direct switch from Config_old to Config_new) is unsafe:
- During transition, two disjoint quorums can form
- Split-brain scenario: both can commit conflicting operations
- **Example:** 3-node cluster → 5-node cluster
  - Old quorum: 2 of {A,B,C}
  - New quorum: 3 of {A,B,C,D,E}
  - If only A,B,C are online, they form old quorum
  - If D,E come online before A,B,C update, they can't form new quorum
  - But if some nodes use old config and others use new, split-brain!

### Raft's Joint Consensus Solution

Three-state transition:
1. **C_old (Stable)** - All nodes use old configuration
2. **C_old,new (Joint)** - Nodes use BOTH configurations, require quorum in BOTH
3. **C_new (Stable)** - All nodes use new configuration

**Key Invariant:** During joint consensus, operations require quorum in BOTH old and new configurations. This prevents split-brain because no disjoint quorums can form.

**Transition Protocol:**
```
C_old  --(propose C_old,new)-->  C_old,new  --(commit C_old,new)-->  C_new
                                    ▲
                                    │
                            Quorum in BOTH configs
```

1. Leader in C_old proposes C_old,new
2. C_old,new gets committed (requires quorum in C_old AND C_new)
3. Once C_old,new committed, automatically transition to C_new
4. C_new becomes the new stable configuration

## Kimberlite VSR Adaptation

### State Machine

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconfigState {
    /// Stable state with single configuration
    Stable {
        config: ClusterConfig,
    },

    /// Joint consensus state with two configurations
    Joint {
        old_config: ClusterConfig,
        new_config: ClusterConfig,
        /// OpNumber where C_old,new was committed
        joint_op: OpNumber,
    },
}
```

**States:**
- **Stable**: Normal operation, single configuration
- **Joint**: Temporary transition state, dual configurations

### Reconfiguration Commands

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconfigCommand {
    /// Add a new replica to the cluster
    AddReplica(ReplicaId),

    /// Remove a replica from the cluster
    RemoveReplica(ReplicaId),

    /// Replace multiple replicas (add + remove atomically)
    Replace {
        add: Vec<ReplicaId>,
        remove: Vec<ReplicaId>,
    },
}
```

**Command Processing:**
1. **Validation** - Check command is safe (no duplicates, odd cluster size, etc.)
2. **Propose C_old,new** - Create joint configuration, propose as Prepare
3. **Joint Consensus** - Wait for commit with quorum in BOTH configs
4. **Automatic Transition** - Once joint op committed, switch to C_new
5. **Stable** - Resume normal operation with new configuration

### Quorum Calculation

```rust
impl ReconfigState {
    /// Calculates quorum for the current state
    pub fn quorum_size(&self) -> usize {
        match self {
            ReconfigState::Stable { config } => config.quorum_size(),
            ReconfigState::Joint { old_config, new_config, .. } => {
                // Joint consensus: quorum in BOTH configs
                std::cmp::max(old_config.quorum_size(), new_config.quorum_size())
            }
        }
    }

    /// Checks if a set of replicas forms a quorum
    pub fn has_quorum(&self, replicas: &[ReplicaId]) -> bool {
        match self {
            ReconfigState::Stable { config } => {
                let count = replicas.iter().filter(|r| config.contains(**r)).count();
                count >= config.quorum_size()
            }
            ReconfigState::Joint { old_config, new_config, .. } => {
                let old_count = replicas.iter().filter(|r| old_config.contains(**r)).count();
                let new_count = replicas.iter().filter(|r| new_config.contains(**r)).count();

                old_count >= old_config.quorum_size() &&
                new_count >= new_config.quorum_size()
            }
        }
    }
}
```

**Key Insight:** Joint consensus quorum = MAX(Q_old, Q_new), and BOTH must be satisfied.

### Message Protocol Extensions

#### Reconfiguration Message

New message type for reconfiguration proposals:

```rust
pub enum MessageKind {
    // ... existing messages ...

    /// Reconfiguration proposal (only from leader)
    Reconfiguration,
}

pub struct ReconfigurationMessage {
    pub view: ViewNumber,
    pub op: OpNumber,
    pub command: ReconfigCommand,
    pub old_config: ClusterConfig,
    pub new_config: ClusterConfig,
}
```

#### Prepare Message Extension

Existing Prepare messages carry reconfiguration data:

```rust
pub struct PrepareMessage {
    pub view: ViewNumber,
    pub op: OpNumber,
    pub command: Command,

    // NEW: Optional reconfiguration
    pub reconfig: Option<ReconfigCommand>,
}
```

**Why extend Prepare?** Reconfiguration is a special operation that goes through the normal Prepare → PrepareOK → Commit flow, ensuring it's durably replicated before taking effect.

### Integration with ReplicaState

#### State Extension

```rust
pub struct ReplicaState {
    // ... existing fields ...

    /// Current reconfiguration state
    pub(crate) reconfig_state: ReconfigState,

    /// Pending reconfiguration (if any)
    pub(crate) pending_reconfig: Option<ReconfigCommand>,
}
```

#### Event Handling

```rust
pub enum ReplicaEvent {
    // ... existing events ...

    /// Reconfiguration command from client/admin
    ReconfigCommand(ReconfigCommand),
}

impl ReplicaState {
    fn on_reconfig_command(mut self, cmd: ReconfigCommand) -> (Self, ReplicaOutput) {
        // Only leader can process reconfigurations
        if !self.is_leader() {
            return (self, ReplicaOutput::error("not leader"));
        }

        // Only one reconfiguration at a time
        if !matches!(self.reconfig_state, ReconfigState::Stable { .. }) {
            return (self, ReplicaOutput::error("reconfiguration in progress"));
        }

        // Validate command
        let new_config = match self.validate_reconfig(&cmd) {
            Ok(cfg) => cfg,
            Err(e) => return (self, ReplicaOutput::error(e)),
        };

        // Transition to joint consensus
        let old_config = self.config.clone();
        self.reconfig_state = ReconfigState::Joint {
            old_config: old_config.clone(),
            new_config: new_config.clone(),
            joint_op: self.op_number.next(),
        };

        // Propose C_old,new as Prepare
        self.propose_with_reconfig(cmd)
    }
}
```

### View Change Integration

**Critical Question:** What happens if a view change occurs during reconfiguration?

**Answer:** Joint consensus persists across view changes.

```rust
impl ReplicaState {
    fn on_do_view_change(&mut self, msg: DoViewChangeMessage) {
        // ... existing view change logic ...

        // Preserve reconfiguration state across view changes
        // The new leader inherits the joint consensus state
        if let Some(reconfig) = msg.reconfig_state {
            self.reconfig_state = reconfig;
        }
    }
}
```

**Key Points:**
1. Joint consensus state is included in DoViewChange messages
2. New leader inherits the reconfiguration state
3. If C_old,new was proposed but not committed, new leader continues
4. If C_old,new was committed, new leader completes transition to C_new

### Leader Election with Reconfiguration

**Question:** Which configuration determines the leader during joint consensus?

**Answer:** Use the OLD configuration for leader election during joint consensus.

```rust
impl ReconfigState {
    /// Returns the configuration to use for leader election
    pub fn leader_config(&self) -> &ClusterConfig {
        match self {
            ReconfigState::Stable { config } => config,
            ReconfigState::Joint { old_config, .. } => old_config,
        }
    }
}
```

**Rationale:**
- Ensures leader election remains stable during transition
- Avoids flip-flopping leadership if new replicas aren't ready
- Once C_new is stable, leadership can rotate to include new replicas

## Safety Invariants

### Invariant 1: Single Reconfiguration

**Property:** At most one reconfiguration is in progress at any time.

**Enforcement:**
- `on_reconfig_command()` rejects new reconfigurations if not in Stable state
- Joint consensus must complete before new reconfiguration can start

### Invariant 2: Quorum Intersection

**Property:** Any two quorums (old, new, or joint) always intersect.

**Proof:**
- In Stable: Standard quorum majority (n/2 + 1)
- In Joint: Requires quorum in BOTH old and new
  - Q_old ≥ |C_old|/2 + 1
  - Q_new ≥ |C_new|/2 + 1
  - Any Q_old and Q_new must intersect within C_old ∩ C_new

### Invariant 3: Configuration Validity

**Property:** All configurations maintain odd cluster size and no duplicates.

**Enforcement:**
```rust
fn validate_reconfig(&self, cmd: &ReconfigCommand) -> Result<ClusterConfig> {
    let new_replicas = match cmd {
        ReconfigCommand::AddReplica(id) => {
            let mut replicas: Vec<_> = self.config.replicas().collect();
            if replicas.contains(id) {
                return Err("replica already in cluster");
            }
            replicas.push(*id);
            replicas
        }
        ReconfigCommand::RemoveReplica(id) => {
            let mut replicas: Vec<_> = self.config.replicas().collect();
            if !replicas.contains(id) {
                return Err("replica not in cluster");
            }
            replicas.retain(|r| r != id);
            replicas
        }
        // ... Replace case ...
    };

    // Validate odd size
    if new_replicas.len() % 2 == 0 {
        return Err("cluster size must be odd");
    }

    Ok(ClusterConfig::new(new_replicas))
}
```

### Invariant 4: Monotonic Progress

**Property:** Once C_old,new is committed, the system always transitions to C_new.

**Enforcement:**
- Commit handler automatically transitions to C_new when joint op committed
- Recovery and view change preserve joint state until transition completes

## Timeout Handling

### Reconfiguration Timeout

New timeout type for detecting stuck reconfigurations:

```rust
pub enum TimeoutKind {
    // ... existing timeouts ...

    /// Reconfiguration timeout (detects stuck reconfigurations)
    ReconfigTimeout,
}
```

**Behavior:**
- If joint consensus doesn't complete within timeout, abort and revert to C_old
- Leader retries reconfiguration proposal
- After max retries, give up and require manual intervention

**Configuration:**
```rust
pub struct TimeoutConfig {
    // ... existing timeouts ...

    /// Time to wait for reconfiguration to complete
    pub reconfig_timeout: Duration,  // Default: 30 seconds
}
```

## Example Scenarios

### Scenario 1: Add Replica (3 → 5 nodes)

**Initial:** C_old = {A, B, C} (quorum = 2)

**Steps:**
1. Admin sends `ReconfigCommand::AddReplica(D)` to leader A
2. A validates: C_new = {A, B, C, D, E} would be even → REJECT
3. Admin sends `ReconfigCommand::Replace { add: [D, E], remove: [] }`
4. A validates: C_new = {A, B, C, D, E} (quorum = 3) → OK
5. A proposes C_old,new at op 100
6. Joint consensus: Need 2 acks from {A,B,C} AND 3 acks from {A,B,C,D,E}
   - Effectively need 3 acks from {A,B,C} since D,E might not be ready
7. Once op 100 committed, automatically transition to C_new
8. New stable state: C_new = {A, B, C, D, E}

### Scenario 2: Remove Replica (5 → 3 nodes)

**Initial:** C_old = {A, B, C, D, E} (quorum = 3)

**Steps:**
1. Admin sends `ReconfigCommand::Replace { add: [], remove: [D, E] }`
2. Leader proposes C_old,new = {A,B,C,D,E} → {A,B,C}
3. Joint consensus: Need 3 acks from {A,B,C,D,E} AND 2 acks from {A,B,C}
4. Once committed, transition to C_new = {A,B,C}
5. D and E can be decommissioned safely

### Scenario 3: View Change During Reconfiguration

**Initial:** C_old = {A, B, C}, leader A proposes C_old,new to add D,E

**Failure:** Leader A crashes before C_old,new commits

**Recovery:**
1. B starts view change (view 1)
2. B collects DoViewChange messages from C_old (need quorum of 2)
3. DoViewChange messages include reconfig_state = Joint
4. B becomes new leader in view 1, inherits Joint state
5. B re-proposes C_old,new at next op
6. Joint consensus continues with B as leader
7. Once committed, transition to C_new

**Key:** Reconfiguration survives view changes.

## Implementation Plan

### Phase 4.1: Core Reconfiguration (~600 LOC)

1. **reconfiguration.rs** - State machine, validation, quorum calculation
2. **message.rs** - Extend Prepare with reconfig field
3. **replica/state.rs** - Add reconfig_state field, integrate quorum calculation

### Phase 4.2: Command Processing (~400 LOC)

1. **replica/normal.rs** - Implement on_reconfig_command()
2. **replica/view_change.rs** - Extend DoViewChange with reconfig state
3. **config.rs** - Add reconfig validation helpers

### Phase 4.3: Testing (~500 LOC)

1. **Unit tests** - State transitions, quorum calculation, validation
2. **Integration tests** - Add/remove replica scenarios
3. **VOPR scenarios** - Reconfiguration under faults

## Open Questions

### Q1: Should we support removing the current leader?

**Answer:** YES, but with automatic leader transfer.

**Approach:**
- If RemoveReplica(leader), leader initiates view change before proposing
- New leader (not being removed) completes reconfiguration

### Q2: How to handle new replicas that are far behind?

**Answer:** Two-phase approach:

1. **Phase 1: Catch-up** - New replica added as "standby" (read-only)
2. **Phase 2: Promotion** - Once caught up, promote to full voting member

**Extension:** Add `StandbyReplica` state (implemented separately in standby.rs)

### Q3: Can we abort a reconfiguration mid-flight?

**Answer:** Only if C_old,new hasn't been committed yet.

**Approach:**
- Before commit: Leader can abort, revert to C_old
- After commit: No abort, must complete transition to C_new

**Implementation:** Add `ReconfigCommand::Abort` (future work)

## Comparison with TigerBeetle

According to the plan, TigerBeetle has "⚠️ Partial support" for reconfiguration. Kimberlite's full implementation will provide:

1. **Complete joint consensus** - Full Raft-style safety
2. **Integration with VSR** - View changes preserve reconfiguration
3. **Comprehensive testing** - VOPR scenarios for all edge cases

## References

1. **Raft Paper** - "In Search of an Understandable Consensus Algorithm" (Section 6: Cluster membership changes)
2. **VRR Paper** - "Viewstamped Replication Revisited" (Lamport, Schneider)
3. **TigerBeetle Documentation** - Cluster reconfiguration notes
4. **Kimberlite VSR Implementation** - Existing view change protocol

---

**Status:** Design Complete
**Next Step:** Implementation (Task #2 - Implement reconfiguration state machine)
