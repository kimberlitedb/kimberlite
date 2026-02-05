# VSR Production Readiness Gap Analysis

**Date:** 2026-02-04
**Task:** Compare Kimberlite VSR with TigerBeetle to identify missing production features
**Status:** Analysis Complete

## Executive Summary

After comprehensive analysis of TigerBeetle's production VSR implementation and comparison with Kimberlite's current state, I've identified **34 production-readiness gaps** across 7 categories. Kimberlite has an excellent foundation (~4,100 LOC with complete protocol implementation), but lacks several critical production hardening features.

**Current Strengths:**
- ✅ All 14 VSR message types (Prepare, PrepareOk, Commit, Heartbeat, etc.)
- ✅ Complete view changes, recovery (PAR with NACK), repair, state transfer
- ✅ Byzantine detection and rejection with checksum validation
- ✅ 4-copy atomic superblock for crash recovery
- ✅ Merkle tree checkpoints with Ed25519 signatures
- ✅ VOPR simulation Phase 1 (27 scenarios, 19 invariant checkers)
- ✅ Idempotency table for duplicate detection

**Critical Gaps (P0):**
- ❌ No cluster-wide clock synchronization (compliance risk)
- ❌ Missing explicit client session management (VRR paper bugs)
- ❌ No repair rate limiting (can cause repair storms)
- ❌ Limited timeout coverage (potential deadlocks)

**Production Effort:** ~6,750 LOC across 5 phases, estimated 30 weeks

---

## Gap Analysis: 34 Missing Features

### CATEGORY 1: Clock Synchronization (CRITICAL - P0)

#### Why Critical
TigerBeetle's documentation states: "Clock drift can render clusters unusable." For compliance (healthcare, finance), accurate timestamps on audit trails are mandatory. Without cluster consensus on time, timeout values become unreliable.

| # | Feature | TigerBeetle | Kimberlite | Why It Matters | Complexity |
|---|---------|-------------|------------|----------------|------------|
| 1 | Cluster-wide synchronized clock | ✅ clock.zig (~1,100 LOC) | ❌ None | Accurate audit timestamps for compliance | Large |
| 2 | Marzullo's algorithm | ✅ marzullo.zig (~300 LOC) | ❌ None | Finds clock interval consistent with quorum | Medium |
| 3 | Clock offset tolerance checking | ✅ Validates max drift | ❌ None | Detects clock misconfiguration before corruption | Small |
| 4 | Ping/pong clock sample exchange | ✅ Embedded in messages | ❌ None | Measures network delay and clock offsets | Small |
| 5 | Epoch-based synchronization windows | ✅ Multi-second epochs | ❌ None | Amortizes sample collection cost | Medium |

**TigerBeetle Approach:**
- Only primary assigns timestamps (prevents replica disagreement)
- Collects clock samples from all replicas via ping/pong
- Uses Marzullo's algorithm to find interval consistent with quorum
- Enforces `clock_offset_tolerance_max` (rejects outliers)
- Epoch-based: collects samples over several seconds, installs synchronized time

**Implementation Files:**
- `crates/kimberlite-vsr/src/clock.rs` (NEW - ~800 LOC)
- `crates/kimberlite-vsr/src/marzullo.rs` (NEW - ~300 LOC)
- `crates/kimberlite-vsr/src/message.rs` (MODIFY - add clock samples to Heartbeat)
- `crates/kimberlite-vsr/src/replica/state.rs` (MODIFY - integrate Clock struct)

**Estimated Effort:** 3-4 weeks, ~1,400 LOC

---

### CATEGORY 2: Client Session Management (CRITICAL - P0)

#### Why Critical
TigerBeetle's code comments cite **two bugs found in the VRR paper**:

1. **Successive client crashes** cause request number collisions → wrong cached reply returned to client
2. **Updating client table for uncommitted requests** can lock clients out after view change (new leader doesn't have uncommitted state)

Kimberlite has `IdempotencyTable` but lacks explicit session registration and committed/uncommitted tracking separation.

| # | Feature | TigerBeetle | Kimberlite | Why It Matters | Complexity |
|---|---------|-------------|------------|----------------|------------|
| 6 | Client session registration | ✅ Explicit via state machine | ❌ Implicit only | Prevents request number collisions | Medium |
| 7 | Committed vs uncommitted tracking | ✅ Separate structures | ⚠️ Single IdempotencyTable | Prevents client lockout after view change | Medium |
| 8 | Deterministic client eviction | ✅ Oldest commit timestamp | ❌ None (MAX_ENTRIES limit) | Ensures consistent eviction across replicas | Small |
| 9 | Reply slot management | ✅ Per-client slots | ⚠️ Has IdempotencyId | Prevents reply loss during eviction | Small |

**TigerBeetle Approach:**
- Explicit session registration before first request
- Separate tracking for committed vs uncommitted requests
- Deterministic eviction based on oldest commit (all replicas agree)
- Reply slots prevent data loss during eviction

**Implementation Files:**
- `crates/kimberlite-vsr/src/client_sessions.rs` (NEW - ~400 LOC)
- `crates/kimberlite-vsr/src/idempotency.rs` (MODIFY - extend to ClientSessions)
- `crates/kimberlite-vsr/src/replica/normal.rs` (MODIFY - check sessions before processing)

**Estimated Effort:** 2-3 weeks, ~850 LOC

---

### CATEGORY 3: Repair & Recovery ✅ COMPLETE (Phase 1.3 - v0.4.0)

#### Why Critical
Without repair budgets, lagging replicas can flood the cluster with repair requests. TigerBeetle found this overwhelms send queues (sized to only 4 messages) and causes cascading failures.

| # | Feature | TigerBeetle | Kimberlite | Why It Matters | Complexity |
|---|---------|-------------|------------|----------------|------------|
| 10 | Repair budget (journal) | ✅ RepairBudgetJournal | ✅ RepairBudget | Prevents repair storms from overwhelming cluster | Medium |
| 11 | Repair budget (grid/storage) | ✅ RepairBudgetGrid | ✅ RepairBudget (unified) | Rate-limits storage repair requests | Medium |
| 12 | Per-replica latency tracking (EWMA) | ✅ Exponentially weighted moving avg | ✅ EWMA with alpha=0.2 | Routes repairs to fastest replicas first | Small |
| 13 | Repair request expiry | ✅ 500ms timeout | ✅ 500ms timeout | Prevents stale repair requests from accumulating | Small |
| 14 | Experiment chance for repair | ✅ 10% random selection | ✅ 10% random selection | Discovers if "slow" replicas recovered | Small |
| 15 | Inflight repair limit | ✅ Max 2 per replica | ✅ Max 2 per replica | Prevents network queue overflow | Small |

**Kimberlite Implementation (Phase 1.3):**
- RepairBudget module with credit-based rate limiting
- EWMA latency tracking (alpha = 0.2, TigerBeetle-validated value)
- 90% fastest replica selection, 10% experiment chance
- Hard limit of 2 inflight repairs per replica (MAX_INFLIGHT_PER_REPLICA)
- 500ms expiry with timeout penalty (2x EWMA on failure)

**Implementation Files:**
- `crates/kimberlite-vsr/src/repair_budget.rs` ✅ COMPLETE (~737 LOC)
- `crates/kimberlite-vsr/src/replica/repair.rs` (integration pending)

**Formal Verification:**
- TLA+ spec: `specs/tla/RepairBudget.tla` (6 properties verified)
- Kani proofs: 3 proofs (#30-32: inflight bounded, budget replenishment, EWMA correctness)
- VOPR scenarios: 3 scenarios (RepairBudgetPreventsStorm, RepairEwmaSelection, RepairSyncTimeout)
- Production assertions: 4 assertions (inflight limit, accounting, EWMA bounds, stale removal)

**Documentation:** `docs/internals/repair-budget.md` (comprehensive guide)

**Status:** ✅ COMPLETE - Phase 1.3 (February 2026)

---

### CATEGORY 4: Storage Integrity (HIGH PRIORITY - P1)

#### Why Critical
Background scrubbing detects latent sector errors before they cause double-fault data loss. Google study (2007) found >60% of latent errors discovered by scrubbers, not active reads.

| # | Feature | TigerBeetle | Kimberlite | Why It Matters | Complexity |
|---|---------|-------------|------------|----------------|------------|
| 16 | Background data scrubbing | ✅ grid_scrubber.zig (~700 LOC) | ❌ None | Detects silent corruption before double-fault | Medium |
| 17 | Latent sector error detection | ✅ Reads + validates checksums | ❌ None | Prevents data loss from disk failures | Medium |
| 18 | Scrub tour tracking | ✅ Tours entire log/grid | ❌ None | Ensures all data validated periodically | Medium |
| 19 | Scrub rate limiting | ✅ Reserve IOPS budget | ❌ None | Prevents scrubbing from impacting performance | Small |
| 20 | Scrub origin randomization | ✅ PRNG-based start | ❌ None | Avoids synchronized scrub load spikes | Small |

**TigerBeetle Approach:**
- Background process tours entire data set
- Validates checksums on every block
- PRNG-based origin prevents replica synchronization
- Rate-limited to reserve IOPS for production traffic
- Triggers repair automatically on corruption detection

**Implementation Files:**
- `crates/kimberlite-vsr/src/log_scrubber.rs` (NEW - ~600 LOC)
- `crates/kimberlite-storage/src/log.rs` (MODIFY - expose scrub interface)

**Estimated Effort:** 2-3 weeks, ~800 LOC

---

### CATEGORY 5: Timeouts & Liveness (MEDIUM PRIORITY - P1)

#### Why Important
Comprehensive timeout coverage prevents deadlocks and ensures liveness under all failure modes. Primary abdicate timeout is particularly critical to avoid deadlock when leader is partitioned.

| # | Feature | TigerBeetle | Kimberlite | Why It Matters | Complexity |
|---|---------|-------------|------------|----------------|------------|
| 21 | Ping timeout | ✅ Always running | ❌ None | Detects network failures early | Small |
| 22 | Prepare timeout | ✅ Resend prepare | ⚠️ Has Prepare | Leader retries on backup silence | Small |
| 23 | Primary abdicate timeout | ✅ Leader steps down | ❌ None | Prevents deadlock when leader partitioned | Small |
| 24 | Commit message timeout | ✅ Heartbeat fallback | ⚠️ Has Heartbeat | Ensures commit progress notification | Small |
| 25 | Start view change window | ✅ Waits for votes | ❌ None | Prevents premature view change | Small |
| 26 | Repair sync timeout | ✅ Triggers state transfer | ❌ None | Escalates repair to full state sync | Small |
| 27 | Grid repair budget timeout | ✅ Replenish credits | ❌ None | Resets repair budget periodically | Small |
| 28 | Commit stall timeout | ✅ Apply backpressure | ❌ None | Prevents unbounded pipeline growth | Medium |

**TigerBeetle Approach:**
- 8+ timeout types covering all protocol phases
- Primary abdicate: leader stops sending heartbeats if partitioned
- Commit stall: detects when commits not progressing (triggers backpressure)
- Repair sync: escalates from repair to state transfer

**Implementation Files:**
- `crates/kimberlite-vsr/src/replica/mod.rs` (MODIFY - expand TimeoutKind enum)
- `crates/kimberlite-vsr/src/event_loop.rs` (MODIFY - handle new timeouts)

**Estimated Effort:** 2 weeks, ~350 LOC

---

### CATEGORY 6: Cluster Operations (MEDIUM PRIORITY - P1/P2)

#### Why Important
Production clusters require zero-downtime operations: adding/removing nodes, rolling upgrades, standby replicas for DR. Without these, every cluster change requires downtime.

| # | Feature | TigerBeetle | Kimberlite | Why It Matters | Complexity |
|---|---------|-------------|------------|----------------|------------|
| 29 | Cluster reconfiguration | ⚠️ Partial support | ❌ None | Add/remove nodes without downtime | Large |
| 30 | Rolling upgrades | ✅ Version negotiation | ❌ None | Upgrade cluster without stopping service | Large |
| 31 | Replica standby mode | ✅ Read-only followers | ❌ None | Disaster recovery and read scaling | Medium |
| 32 | Dynamic membership changes | ⚠️ Partial support | ❌ None | Adjust cluster size for load/failures | Large |

**TigerBeetle Approach:**
- Joint consensus algorithm for membership changes (Raft-style)
- Version tracking + negotiation for rolling upgrades
- Standby replicas participate in reads, not writes

**Implementation Files:**
- `crates/kimberlite-vsr/src/reconfiguration.rs` (NEW - ~600 LOC)
- `crates/kimberlite-vsr/src/upgrade.rs` (NEW - ~800 LOC)
- `crates/kimberlite-vsr/src/standby.rs` (NEW - ~400 LOC)

**Estimated Effort:** 6-8 weeks, ~2,200 LOC

---

### CATEGORY 7: Observability & Debugging (LOW PRIORITY - P2/P3)

#### Why Useful
Production debugging requires rich instrumentation. TigerBeetle has extensive tracing hooks for simulation testing and production monitoring.

| # | Feature | TigerBeetle | Kimberlite | Why It Matters | Complexity |
|---|---------|-------------|------------|----------------|------------|
| 33 | Structured tracing | ✅ Tracer struct | ⚠️ Has `tracing` crate | Debug production issues | Medium |
| 34 | Test context hooks | ✅ event_callback | ⚠️ Has VOPR | Deterministic testing infrastructure | Small |

**Implementation Files:**
- `crates/kimberlite-vsr/src/instrumentation.rs` (MODIFY - enhance metrics)

**Estimated Effort:** 1-2 weeks, ~400 LOC

---

## Implementation Roadmap

### Phase 1: Clock Synchronization & Client Sessions (P0 - 8 weeks)

**Goal:** Fix critical correctness and compliance gaps

**Deliverables:**
1. **Clock Synchronization** (~1,400 LOC)
   - Marzullo's algorithm for interval intersection
   - Clock struct with epoch/window tracking
   - Embed clock samples in Heartbeat messages
   - Timestamp monotonicity validation

2. **Client Session Management** (~850 LOC)
   - Extend IdempotencyTable to ClientSessions
   - Explicit session registration
   - Deterministic eviction policy
   - Separate committed/uncommitted tracking

**Testing Requirements:**
- VOPR scenarios: clock drift, NTP failures, client crashes
- VRR paper bugs: request collisions, client lockout after view change
- Clock offset tolerance violations

**Critical Files:**
- `crates/kimberlite-vsr/src/clock.rs` (NEW)
- `crates/kimberlite-vsr/src/marzullo.rs` (NEW)
- `crates/kimberlite-vsr/src/client_sessions.rs` (NEW)
- `crates/kimberlite-vsr/src/replica/state.rs` (MODIFY)

---

### Phase 2: Repair Budgets & Timeouts (P0/P1 - 6 weeks)

**Goal:** Prevent repair storms and improve liveness

**Deliverables:**
1. **Repair Budgets** (~750 LOC)
   - RepairBudget with EWMA latency tracking
   - Smart replica selection (prioritize fast replicas, 10% experiment)
   - Inflight limits (max 2 per replica)
   - Request expiry (500ms timeout)

2. **Extended Timeouts** (~350 LOC)
   - Ping, primary abdicate, repair sync timeouts
   - Commit stall detection with backpressure

**Testing Requirements:**
- VOPR scenarios: repair storms, lagging backups, partitioned primary
- Verify repair budget prevents queue overflow
- Verify primary abdicate prevents deadlock

**Critical Files:**
- `crates/kimberlite-vsr/src/repair_budget.rs` (NEW)
- `crates/kimberlite-vsr/src/replica/repair.rs` (MODIFY)
- `crates/kimberlite-vsr/src/replica/mod.rs` (MODIFY - expand TimeoutKind)

---

### Phase 3: Storage Integrity (P1 - 4 weeks)

**Goal:** Proactive fault detection

**Deliverables:**
1. **Background Scrubbing** (~800 LOC)
   - LogScrubber with tour tracking
   - Incremental read + checksum validation
   - PRNG-based origin randomization
   - Rate limiting to preserve IOPS

**Testing Requirements:**
- Inject latent sector errors
- Verify scrubber detects corruption
- Verify automatic repair triggered

**Critical Files:**
- `crates/kimberlite-vsr/src/log_scrubber.rs` (NEW)
- `crates/kimberlite-storage/src/log.rs` (MODIFY)

---

### Phase 4: Cluster Operations (P1/P2 - 8 weeks)

**Goal:** Zero-downtime operational flexibility

**Deliverables:**
1. **Cluster Reconfiguration** (~1,000 LOC)
   - Joint consensus algorithm (Raft-style)
   - Reconfiguration command processing
   - Membership change state machine

2. **Rolling Upgrades** (~800 LOC)
   - Version tracking in messages
   - Release negotiation protocol
   - Upgrade state machine

3. **Standby Replicas** (~400 LOC)
   - Read-only follower mode
   - Promotion to active replica

**Testing Requirements:**
- Add/remove replica scenarios
- Rolling upgrade with 3+ versions
- Standby promotion scenarios

**Critical Files:**
- `crates/kimberlite-vsr/src/reconfiguration.rs` (NEW)
- `crates/kimberlite-vsr/src/upgrade.rs` (NEW)
- `crates/kimberlite-vsr/src/standby.rs` (NEW)

---

### Phase 5: Observability & Polish (P2/P3 - 4 weeks)

**Goal:** Production-grade monitoring

**Deliverables:**
1. **Enhanced Instrumentation** (~400 LOC)
   - Structured metrics (latency histograms, throughput counters)
   - OpenTelemetry integration
   - Performance profiling hooks

2. **Documentation**
   - Production deployment guide
   - Monitoring runbook
   - Incident response playbook

**Critical Files:**
- `crates/kimberlite-vsr/src/instrumentation.rs` (MODIFY)
- `docs/PRODUCTION.md` (NEW)

---

## Effort Summary

| Phase | Priority | Duration | LOC | Key Deliverables |
|-------|----------|----------|-----|------------------|
| 1 | P0 | 8 weeks | ~2,250 | Clock sync, client sessions |
| 2 | P0/P1 | 6 weeks | ~1,100 | Repair budgets, timeouts |
| 3 | P1 | 4 weeks | ~800 | Background scrubbing |
| 4 | P1/P2 | 8 weeks | ~2,200 | Reconfiguration, upgrades |
| 5 | P2/P3 | 4 weeks | ~400 | Observability, docs |
| **Total** | | **30 weeks** | **~6,750** | Production-ready VSR |

**Current Kimberlite VSR:** ~13,366 LOC
**After Completion:** ~20,116 LOC (+50% growth)

---

## Verification Plan

### Testing Strategy

1. **VOPR Integration (Phase 2 from VSR_INTEGRATION.md)**
   - Connect VsrSimulation to main VOPR event loop
   - Add `--vsr-mode` CLI flag
   - Wire up all 19 invariant checkers
   - Test all 27 scenarios with full VSR protocol

2. **New VOPR Scenarios**
   - Clock drift and NTP failures (Phase 1)
   - Repair storms with lagging backups (Phase 2)
   - Latent sector errors (Phase 3)
   - Dynamic reconfiguration (Phase 4)
   - Rolling upgrades with 3 versions (Phase 4)

3. **Property-Based Testing**
   - Clock synchronization with proptest
   - Client session eviction consistency
   - Repair budget allocation fairness

4. **Long-Duration Testing**
   - 10M+ operations with time compression
   - Multi-day runs with fault injection
   - Memory leak detection

### Success Criteria

**Phase 1:**
- ✅ Clock offset stays within tolerance under faults
- ✅ Client crashes don't cause request collisions
- ✅ View changes preserve uncommitted client sessions

**Phase 2:**
- ✅ Repair requests never exceed budget
- ✅ Partitioned leader steps down (no deadlock)
- ✅ Commit stall detected and backpressure applied

**Phase 3:**
- ✅ 100% latent sector error detection
- ✅ Automatic repair triggered on corruption
- ✅ Scrubbing completes full tour within 24 hours

**Phase 4:**
- ✅ Add/remove replica with zero downtime
- ✅ Rolling upgrade across 3 versions
- ✅ Standby promotion preserves data

**Phase 5:**
- ✅ Metrics exported to Prometheus/OpenTelemetry
- ✅ Runbook covers all failure modes
- ✅ Performance profiling identifies bottlenecks

---

## Risk Assessment

### High Risk Items

1. **Clock Synchronization Complexity**
   - **Risk:** Marzullo's algorithm subtle, easy to get wrong
   - **Mitigation:** Property testing, compare with TigerBeetle reference
   - **Fallback:** Use simplified approach (primary clock only, tolerance checks)

2. **Client Session Migration**
   - **Risk:** Breaking change to existing IdempotencyTable
   - **Mitigation:** Phased rollout, backward compatibility layer
   - **Fallback:** Keep IdempotencyTable, add ClientSessions separately

3. **VOPR Integration Phase 2**
   - **Risk:** VsrSimulation not fully integrated into main loop
   - **Mitigation:** Prioritize Phase 2 integration before new features
   - **Fallback:** Manual scenario testing without full VOPR

### Medium Risk Items

4. **Repair Budget Tuning**
   - **Risk:** Budget parameters (EWMA weight, experiment %, timeout) need empirical tuning
   - **Mitigation:** Extensive benchmarking, expose as config
   - **Fallback:** Start with conservative values from TigerBeetle

5. **Cluster Reconfiguration State Explosion**
   - **Risk:** Joint consensus adds significant state machine complexity
   - **Mitigation:** Incremental implementation, exhaustive testing
   - **Fallback:** Simple add-only, remove requires restart

---

## Comparison: Kimberlite vs TigerBeetle

### What Kimberlite Does Well

1. **Rust vs Zig:** Memory safety without runtime overhead
2. **FCIS Pattern:** Clear separation of pure/impure code
3. **Comprehensive Testing:** VOPR framework with 27 scenarios, 19 invariants
4. **Byzantine Detection:** Checksum validation before processing (TigerBeetle-inspired)
5. **Protocol-Aware Recovery:** NACK-based safe truncation

### Where TigerBeetle Excels

1. **Clock Synchronization:** Sophisticated cluster-wide consensus on time
2. **Operational Maturity:** Rolling upgrades, reconfiguration, standby replicas
3. **Fault Tolerance:** Repair budgets prevent cascading failures
4. **Storage Integrity:** Proactive scrubbing detects latent errors
5. **Production Hardening:** Years of battle-testing in production

### Kimberlite's Competitive Advantages

1. **Compliance Focus:** Healthcare (HIPAA), finance (SOX), legal (data retention)
2. **SQL Support:** SQL-92 + DuckDB extensions (TigerBeetle has custom API)
3. **Multi-Tenancy:** Built-in tenant isolation via kimberlite-directory
4. **Append-Only Foundation:** Immutable log enables forensic audits
5. **Cryptographic Verification:** SHA-256 hash chains, Ed25519 signatures

---

## Next Steps

1. **Review & Prioritize:** Validate gap analysis, adjust priorities based on roadmap
2. **Resource Allocation:** Assign 2 engineers for critical path (Phases 1-2)
3. **VOPR Phase 2:** Complete VSR Mode integration before new features
4. **Begin Phase 1:** Clock synchronization (highest compliance impact)
5. **Continuous Testing:** Run VOPR scenarios nightly, track coverage metrics

---

## References

### TigerBeetle Files Analyzed
- `/inspiration/tigerbeetle/src/vsr/replica.zig` (562KB - core protocol)
- `/inspiration/tigerbeetle/src/vsr/clock.zig` (40KB - clock synchronization)
- `/inspiration/tigerbeetle/src/vsr/client_sessions.zig` (13KB)
- `/inspiration/tigerbeetle/src/vsr/repair_budget.zig` (14KB)
- `/inspiration/tigerbeetle/src/vsr/grid_scrubber.zig` (35KB)
- `/inspiration/tigerbeetle/src/vsr/journal.zig` (114KB)

### Kimberlite Files Reviewed
- `crates/kimberlite-vsr/src/replica/state.rs` (800+ LOC)
- `crates/kimberlite-vsr/src/idempotency.rs` (200+ LOC)
- `crates/kimberlite-vsr/src/superblock.rs` (400+ LOC)
- `crates/kimberlite-sim/VSR_INTEGRATION.md`
- `docs/TESTING.md`

### External Resources
- TigerBeetle blog: "Three Clocks are Better than One"
- VRR paper bugs (cited in TigerBeetle client_sessions.zig)
- Google study on latent sector errors (2007)
- Marzullo's algorithm (interval intersection for clock sync)
