---
title: "Client Session Management"
section: "internals"
slug: "client-sessions"
order: 5
---

# Client Session Management

**Module:** `crates/kimberlite-vsr/src/client_sessions.rs`
**TLA+ Spec:** `specs/tla/ClientSessions.tla`
**Kani Proofs:** `crates/kimberlite-vsr/src/kani_proofs.rs` (Proofs #26-29)
**VOPR Scenarios:** 3 scenarios (ClientSessionCrash, ClientSessionViewChangeLockout, ClientSessionEviction)

---

## Overview

Kimberlite's client session management fixes **two critical bugs** found in the VRR (Viewstamped Replication Revisited) paper. These bugs cause request collisions and client lockouts in production systems.

### VRR Bug #1: Successive Client Crashes

**Problem:**
When a client crashes and restarts, it resets its request number to 0. If the server still has the old session cached, it returns the reply for request #0 from the **previous client incarnation**, not the current one.

**Example:**
```
1. Client connects, sends request #0 → gets reply "A"
2. Client crashes, cache entry remains
3. Client reconnects, sends request #0 → gets reply "A" (WRONG!)
   Should execute new request, not return cached "A"
```

**Impact:** Clients get wrong results from previous session, causing data corruption.

**Fix:** Explicit session registration. Each client must register before sending requests. Registration creates a new session with a fresh request number space.

---

### VRR Bug #2: Uncommitted Request Table Updates

**Problem:**
The VRR paper updates the client table when a request is **prepared** (not yet committed). During a view change, the new leader doesn't have uncommitted prepares, so it rejects requests from clients whose table was updated but not committed. **Client is locked out.**

**Example:**
```
1. Primary prepares client request #5 → updates client table to request_number = 5
2. View change occurs before commit
3. New primary doesn't have request #5
4. Client retries request #5 → rejected (not > 5 in new primary's table)
   Client cannot make progress!
```

**Impact:** Clients become permanently locked out after view changes, requiring manual intervention.

**Fix:** Separate committed vs uncommitted tracking. Only update the committed table after a request is actually committed. Track uncommitted requests separately and discard them on view change.

---

## Solution Architecture

### Separate Tracking

```rust
pub struct ClientSessions {
    /// Committed sessions with cached replies (survives view changes)
    committed: HashMap<ClientId, CommittedSession>,

    /// Uncommitted sessions (discarded on view changes)
    uncommitted: HashMap<ClientId, UncommittedSession>,

    /// Priority queue for deterministic eviction (oldest first)
    eviction_queue: BinaryHeap<Reverse<SessionEviction>>,

    /// Configuration (max_sessions limit)
    config: ClientSessionsConfig,
}
```

### Key Operations

**1. Session Registration (Bug #1 Fix)**
```rust
let client_id = sessions.register_client();
// Each registration creates a new session ID
// Even if request numbers overlap, different IDs prevent collisions
```

**2. Uncommitted Tracking (Bug #2 Fix - Part 1)**
```rust
// During prepare phase: record as uncommitted
sessions.record_uncommitted(client_id, request_number, preparing_op)?;
```

**3. Commit (Bug #2 Fix - Part 2)**
```rust
// After consensus: move from uncommitted to committed
sessions.commit_request(
    client_id,
    request_number,
    committed_op,
    reply_op,
    cached_effects,
    commit_timestamp,
)?;
```

**4. View Change (Bug #2 Fix - Part 3)**
```rust
// New leader discards uncommitted sessions
sessions.discard_uncommitted();
// Committed sessions preserved, uncommitted gone
```

**5. Deterministic Eviction**
```rust
// When max_sessions exceeded, evict oldest by commit_timestamp
// All replicas evict same session (deterministic)
evict_oldest();
```

---

## Implementation Details

### CommittedSession

```rust
pub struct CommittedSession {
    /// Highest committed request number for this client
    pub request_number: u64,

    /// Op number where committed
    pub committed_op: OpNumber,

    /// Op number to return as reply
    pub reply_op: OpNumber,

    /// Cached effects (for idempotent retry)
    pub cached_effects: Vec<Effect>,

    /// Timestamp when committed (for deterministic eviction)
    pub commit_timestamp: Timestamp,
}
```

### UncommittedSession

```rust
pub struct UncommittedSession {
    /// Request number being prepared (not yet committed)
    pub request_number: u64,

    /// Op number where preparing
    pub preparing_op: OpNumber,
}
```

### Configuration

```rust
pub struct ClientSessionsConfig {
    /// Maximum concurrent sessions (default: 100,000)
    pub max_sessions: usize,
}
```

---

## Formal Verification

### TLA+ Specification (`specs/tla/ClientSessions.tla`)

**Properties Verified:**

1. **NoRequestCollision**: Client crash with request number reset doesn't return wrong cached replies
2. **NoClientLockout**: View change doesn't prevent valid client requests from being processed
3. **DeterministicEviction**: All replicas evict same sessions (by commit_timestamp)
4. **RequestNumberMonotonic**: Within a client session, request numbers only increase
5. **CommittedSessionsSurviveViewChange**: View changes preserve committed sessions
6. **NoDuplicateCommits**: A client cannot commit the same request number twice

**Model checked:** TLC verifies all invariants hold.

### Kani Proofs (4 proofs)

1. **Proof 26: No request collision after client crash**
   - Property: Separate committed/uncommitted prevents wrong cached replies
   - Verified: Client crash with reset returns correct (not cached) reply

2. **Proof 27: Committed and uncommitted sessions are independent**
   - Property: Uncommitted sessions don't interfere with committed lookups
   - Verified: Duplicate detection only checks committed sessions

3. **Proof 28: View change transfers only committed sessions**
   - Property: Uncommitted sessions discarded, committed preserved
   - Verified: `discard_uncommitted()` clears only uncommitted

4. **Proof 29: Session eviction is deterministic**
   - Property: Eviction by commit_timestamp produces same result across replicas
   - Verified: Oldest session (by timestamp) always evicted first

### Production Assertions (6 assertions)

All use `assert!()` (not `debug_assert!()`) for production enforcement:

1. **Committed slot monotonicity** (`record_uncommitted:323`)
   - `request_number > committed.request_number`
   - Prevents VRR Bug #1 (request collisions)

2. **No duplicate commits** (`commit_request:381`)
   - `existing.request_number != request_number`
   - A client cannot commit same request number twice

3. **Session capacity enforcement** (`commit_request:403`)
   - `committed.len() <= max_sessions + 1`
   - Prevents unbounded memory growth

4. **Eviction verification** (`commit_request:414`)
   - `committed.len() <= max_sessions` after eviction
   - Ensures eviction actually worked

5. **Backups clear uncommitted** (`discard_uncommitted:444`)
   - `uncommitted.is_empty()` after discard
   - Prevents VRR Bug #2 (client lockout)

6. **Eviction determinism** (`evict_oldest:462`)
   - `committed.len() == count_before - 1`
   - Exactly one session evicted (deterministic)

---

## VOPR Testing (3 scenarios)

### 1. ClientSessionCrash (VRR Bug #1)

**Test:** Successive client crashes with request number reset
**Verify:** No wrong cached replies returned
**Config:** 15s runtime, 40K events, 10% gray failures (simulates crashes)

### 2. ClientSessionViewChangeLockout (VRR Bug #2)

**Test:** Uncommitted sessions during view change
**Verify:** Clients not locked out after view change
**Config:** 20s runtime, 35K events, 15% drop probability (triggers view changes)

### 3. ClientSessionEviction

**Test:** Deterministic eviction under load (100K sessions)
**Verify:** All replicas evict same sessions (oldest by commit_timestamp)
**Config:** 15s runtime, 100K events, 3 tenants for session pressure

**All scenarios pass:** 1M iterations per scenario, 0 violations

---

## Performance Characteristics

- **Memory per session:** ~120 bytes (committed) or ~40 bytes (uncommitted)
- **Registration overhead:** O(1) - increment counter
- **Duplicate check:** O(1) - HashMap lookup
- **Record uncommitted:** O(1) - HashMap insert
- **Commit:** O(log N) - priority queue insert for eviction
- **Eviction:** O(log N) - priority queue pop
- **View change:** O(U) - clear U uncommitted sessions

**Typical overhead:** <1% for 100K concurrent sessions

---

## Integration with VSR

### Primary Replica

```rust
// On client request
if let Some(cached) = sessions.check_duplicate(client_id, request_number) {
    // Return cached reply (idempotent retry)
    return Ok(cached.cached_effects.clone());
}

// On prepare
sessions.record_uncommitted(client_id, request_number, preparing_op)?;

// On commit
let timestamp = clock.realtime_synchronized().unwrap();
sessions.commit_request(
    client_id,
    request_number,
    committed_op,
    reply_op,
    effects,
    timestamp,
)?;
```

### Backup Replica

```rust
// On view change: new leader discards uncommitted
sessions.discard_uncommitted();
```

---

## Debugging Guide

### Common Issues

**Issue:** Client gets wrong cached reply
**Diagnosis:** Check if client properly registered session
**Fix:** Ensure `register_client()` called before sending requests

**Issue:** Client locked out after view change
**Diagnosis:** Uncommitted session not discarded
**Fix:** Verify `discard_uncommitted()` called during view change

**Issue:** Memory growth from sessions
**Diagnosis:** Eviction not working or max_sessions too high
**Fix:** Check eviction_queue, verify commit_timestamp ordering

### Assertions That Catch Bugs

| Assertion | What It Catches | VRR Bug |
|-----------|----------------|---------|
| `request_number > committed.request_number` | Request number collision | Bug #1 |
| `existing.request_number != request_number` | Duplicate commit | Bug #1 |
| `uncommitted.is_empty()` after view change | Uncommitted not discarded | Bug #2 |
| `committed.len() <= max_sessions` | Eviction failure | Eviction |
| `committed.len() == count_before - 1` | Non-deterministic eviction | Eviction |

---

## References

### Academic Papers
- Liskov, B., & Cowling, J. (2012). "Viewstamped Replication Revisited" (VRR paper with bugs)

### Industry Implementations
- TigerBeetle: `src/vsr/client_sessions.zig` (inspiration for fixes)
- FoundationDB: Client request deduplication (similar approach)

### Internal Documentation
- `docs/concepts/compliance.md` - Idempotency for HIPAA/GDPR
- `docs/traceability_matrix.md` - TLA+ → Rust → VOPR traceability

---

## Future Work

- [ ] **Cross-cluster session transfer** (for disaster recovery)
- [ ] **Session persistence** (survive replica crashes, not just view changes)
- [ ] **Adaptive max_sessions** (scale based on load)
- [ ] **Session TTL** (auto-expire inactive sessions)

---

**Implementation Status:** ✅ Complete (Phase 1.2 - v0.3.0)
**Verification:** 4 Kani proofs, 3 VOPR scenarios, 1 TLA+ spec, 6 production assertions
**VRR Bugs Fixed:** Bug #1 (request collisions), Bug #2 (client lockout)
