# VOPR Scenarios - Complete Reference

**Audience:** Kimberlite contributors and maintainers

This document provides detailed information about all 46 VOPR test scenarios. For user-facing VOPR documentation, see [/docs/reference/cli/vopr.md](../../docs/reference/cli/vopr.md).

## Overview

VOPR tests Kimberlite through 46 scenarios organized into 11 phases:

- **Phase 0: Core (6 scenarios)** - Baseline, network congestion, gray failures, multi-tenancy
- **Phase 1: Byzantine (11 scenarios)** - Protocol-level attacks on consensus safety
- **Phase 2: Corruption (3 scenarios)** - Storage corruption detection and recovery
- **Phase 3: Crash/Recovery (3 scenarios)** - Crash scenarios and recovery paths
- **Phase 4: Gray Failures (2 scenarios)** - Partial failures (slow disk, intermittent network)
- **Phase 5: Race Conditions (2 scenarios)** - Concurrent operations
- **Phase 6: Clock (3 scenarios)** - Clock synchronization and drift
- **Phase 7: Client Sessions (3 scenarios)** - Client session management (VRR-based)
- **Phase 8: Repair/Timeout (5 scenarios)** - Repair budget, timeouts, primary abdication
- **Phase 9: Scrubbing (4 scenarios)** - Background data integrity checks
- **Phase 10: Reconfiguration (3 scenarios)** - Cluster membership changes

**Total:** 46 scenarios

---

## Phase 0: Core Scenarios (6)

### 1. Baseline
- **Enum:** `ScenarioType::Baseline`
- **Purpose:** Normal operation without faults to establish baseline performance
- **Faults:** None
- **Invariants Tested:** All (baseline reference)
- **Expected Behavior:** 100% pass rate, establishes throughput baseline
- **Usage:** `cargo run --bin vopr -- run --scenario baseline --iterations 10000`

### 2. SwizzleClogging
- **Enum:** `ScenarioType::SwizzleClogging`
- **Purpose:** Intermittent network congestion and link flapping
- **Faults:** Network congestion, packet delays
- **Invariants Tested:** VSR agreement, commit history, replica consistency
- **Expected Behavior:** Cluster remains available despite network issues
- **Usage:** `cargo run --bin vopr -- run --scenario swizzle_clogging --iterations 5000`

### 3. GrayFailures
- **Enum:** `ScenarioType::GrayFailures`
- **Purpose:** Partial node failures - slow responses, intermittent errors, read-only nodes
- **Faults:** Slow responses, partial failures, intermittent errors
- **Invariants Tested:** Agreement, timeout handling, recovery safety
- **Expected Behavior:** System degrades gracefully, no safety violations
- **Usage:** `cargo run --bin vopr -- run --scenario gray_failures --iterations 5000`

### 4. MultiTenantIsolation
- **Enum:** `ScenarioType::MultiTenantIsolation`
- **Purpose:** Multiple tenants with independent data, testing isolation under faults
- **Faults:** Network, storage, crash (multi-tenant workload)
- **Invariants Tested:** Tenant isolation, MVCC visibility, applied position monotonicity
- **Expected Behavior:** Tenant data remains isolated despite faults
- **Usage:** `cargo run --bin vopr -- run --scenario multi_tenant_isolation --iterations 10000`

### 5. TimeCompression
- **Enum:** `ScenarioType::TimeCompression`
- **Purpose:** 10x accelerated time to test long-running operations
- **Faults:** Time acceleration (10x)
- **Invariants Tested:** Clock-dependent operations, timeouts, background tasks
- **Expected Behavior:** Long-running operations complete correctly under time compression
- **Usage:** `cargo run --bin vopr -- run --scenario time_compression --iterations 1000`

### 6. Combined
- **Enum:** `ScenarioType::Combined`
- **Purpose:** All fault types enabled simultaneously for stress testing
- **Faults:** Network, storage, crash, clock, Byzantine (all)
- **Invariants Tested:** All
- **Expected Behavior:** Cluster survives even with all faults active
- **Usage:** `cargo run --bin vopr -- run --scenario combined --iterations 10000`
- **Note:** Most comprehensive test, recommended for pre-release validation

---

## Phase 1: Byzantine Attacks (11)

These scenarios test Byzantine fault tolerance - malicious replicas attempting to violate consensus safety.

### 7. ByzantineViewChangeMerge
- **Enum:** `ScenarioType::ByzantineViewChangeMerge`
- **Purpose:** Attack: Force view change after commits, inject conflicting entries
- **Target Invariant:** `vsr_agreement`
- **Attack:** Malicious replica forces view change, then injects conflicting log entries
- **Expected Detection:** Agreement checker detects conflicting commits
- **Bug Found:** Bug #1 (2024-01) - View change could overwrite committed entries
- **Status:** Fixed in v0.2.0

### 8. ByzantineCommitDesync
- **Enum:** `ScenarioType::ByzantineCommitDesync`
- **Purpose:** Attack: Send StartView with high commit_number but truncated log
- **Target Invariant:** `vsr_prefix_property`
- **Attack:** Malicious primary sends StartView with commit_number=100 but only 50 log entries
- **Expected Detection:** Prefix property checker detects log/commit mismatch
- **Bug Found:** Bug #2 (2024-01) - Commit number could advance past log length
- **Status:** Fixed in v0.2.0

### 9. ByzantineInflatedCommit
- **Enum:** `ScenarioType::ByzantineInflatedCommit`
- **Purpose:** Attack: Inflate commit_number in DoViewChange messages
- **Target Invariant:** `vsr_prefix_property`
- **Attack:** Malicious replica sends DoViewChange with inflated commit_number
- **Expected Detection:** Prefix property checker detects inflation
- **Bug Found:** Bug #3 (2024-02) - DoViewChange accepted inflated commit numbers
- **Status:** Fixed in v0.2.1

### 10. ByzantineInvalidMetadata
- **Enum:** `ScenarioType::ByzantineInvalidMetadata`
- **Purpose:** Attack: Send log entries with invalid metadata (wrong view, commit_number)
- **Target Invariant:** `log_consistency`
- **Attack:** Malicious replica sends Prepare with view=5 when cluster is in view=3
- **Expected Detection:** Log consistency checker rejects invalid metadata
- **Bug Found:** Bug #4 (2024-02) - Metadata validation was incomplete
- **Status:** Fixed in v0.2.1

### 11. ByzantineMaliciousViewChange
- **Enum:** `ScenarioType::ByzantineMaliciousViewChange`
- **Purpose:** Attack: Malicious view change selection (prefer stale logs)
- **Target Invariant:** `vsr_view_change_safety`
- **Attack:** Malicious replicas collaborate to elect a primary with stale log
- **Expected Detection:** View change safety checker prevents log truncation
- **Bug Found:** Bug #5 (2024-03) - View change could elect stale replica
- **Status:** Fixed in v0.2.2

### 12. ByzantineLeaderRace
- **Enum:** `ScenarioType::ByzantineLeaderRace`
- **Purpose:** Attack: Race condition in leader election
- **Target Invariant:** `vsr_agreement`
- **Attack:** Multiple replicas claim to be primary simultaneously
- **Expected Detection:** Agreement checker detects split-brain
- **Bug Found:** Bug #6 (2024-03) - Race in StartView acceptance
- **Status:** Fixed in v0.2.2

### 13. ByzantineDvcTailLengthMismatch
- **Enum:** `ScenarioType::ByzantineDvcTailLengthMismatch`
- **Purpose:** Attack: DoViewChange log_tail length mismatch
- **Target Invariant:** `log_consistency`
- **Attack:** Send DoViewChange with log_tail_length != actual log_tail.len()
- **Expected Detection:** DoViewChange validation rejects mismatched lengths
- **Bug Found:** Bug 3.1 (2024-12) - Allowed mismatched tail lengths
- **Status:** Fixed in v0.3.4

### 14. ByzantineDvcIdenticalClaims
- **Enum:** `ScenarioType::ByzantineDvcIdenticalClaims`
- **Purpose:** Attack: DoViewChange with identical claims from different replicas
- **Target Invariant:** `vsr_view_change_safety`
- **Attack:** Two replicas send DoViewChange with same (view, commit_number) but different logs
- **Expected Detection:** View change safety checker detects conflicting claims
- **Bug Found:** Bug 3.3 (2024-12) - Accepted identical claims without log comparison
- **Status:** Fixed in v0.3.4

### 15. ByzantineOversizedStartView
- **Enum:** `ScenarioType::ByzantineOversizedStartView`
- **Purpose:** Attack: Oversized StartView log_tail (DoS via memory exhaustion)
- **Target Invariant:** Resource limits
- **Attack:** Send StartView with log_tail containing 10GB of data
- **Expected Detection:** Message size validation rejects oversized messages
- **Bug Found:** Bug 3.4 (2024-12) - No max message size check
- **Status:** Fixed in v0.3.4 (added 16MB message size limit)

### 16. ByzantineInvalidRepairRange
- **Enum:** `ScenarioType::ByzantineInvalidRepairRange`
- **Purpose:** Attack: Invalid repair range (start > end, negative range)
- **Target Invariant:** Repair protocol safety
- **Attack:** Send RequestRepair with invalid range [100, 50]
- **Expected Detection:** Repair validation rejects invalid ranges
- **Bug Found:** Bug 3.5 (2025-01) - Allowed invalid repair ranges
- **Status:** Fixed in v0.3.5

### 17. ByzantineInvalidKernelCommand
- **Enum:** `ScenarioType::ByzantineInvalidKernelCommand`
- **Purpose:** Attack: Invalid kernel command in log entry
- **Target Invariant:** Kernel command validation
- **Attack:** Send Prepare with malformed or semantically invalid command
- **Expected Detection:** Kernel rejects invalid commands
- **Bug Found:** Bug 3.2 (2024-12) - Kernel accepted some invalid commands
- **Status:** Fixed in v0.3.4

---

## Phase 2: Corruption Detection (3)

These scenarios test storage corruption detection and recovery.

### 18. CorruptionBitFlip
- **Enum:** `ScenarioType::CorruptionBitFlip`
- **Purpose:** Random bit flip in log entry
- **Fault:** Single-bit flip in storage
- **Invariants Tested:** `hash_chain`, `log_consistency`, checksum validation
- **Expected Behavior:** CRC32 checksum detects corruption, hash chain breaks
- **Usage:** `cargo run --bin vopr -- run --scenario corruption_bit_flip --iterations 5000`

### 19. CorruptionChecksumValidation
- **Enum:** `ScenarioType::CorruptionChecksumValidation`
- **Purpose:** Checksum validation test (multiple corruption types)
- **Fault:** Various corruption patterns (bit flips, truncation, insertion)
- **Invariants Tested:** Checksum validation
- **Expected Behavior:** All corruption types detected by CRC32
- **Usage:** `cargo run --bin vopr -- run --scenario corruption_checksum_validation --iterations 10000`

### 20. CorruptionSilentDiskFailure
- **Enum:** `ScenarioType::CorruptionSilentDiskFailure`
- **Purpose:** Silent disk failure (returns wrong data without error)
- **Fault:** Disk returns stale/wrong data
- **Invariants Tested:** `hash_chain`, checksum validation
- **Expected Behavior:** Hash chain detects wrong data even if checksum passes
- **Usage:** `cargo run --bin vopr -- run --scenario corruption_silent_disk_failure --iterations 5000`

---

## Phase 3: Crash/Recovery (3)

These scenarios test crash scenarios and recovery paths.

### 21. CrashDuringCommit
- **Enum:** `ScenarioType::CrashDuringCommit`
- **Purpose:** Crash during commit application
- **Fault:** Replica crashes mid-commit
- **Invariants Tested:** `vsr_recovery_safety`, `log_consistency`
- **Expected Behavior:** Recovery restores consistent state
- **Usage:** `cargo run --bin vopr -- run --scenario crash_during_commit --iterations 5000`

### 22. CrashDuringViewChange
- **Enum:** `ScenarioType::CrashDuringViewChange`
- **Purpose:** Crash during view change protocol
- **Fault:** Replica crashes during DoViewChange/StartView
- **Invariants Tested:** `vsr_view_change_safety`, `vsr_recovery_safety`
- **Expected Behavior:** View change completes or safely aborts
- **Usage:** `cargo run --bin vopr -- run --scenario crash_during_view_change --iterations 5000`

### 23. RecoveryCorruptLog
- **Enum:** `ScenarioType::RecoveryCorruptLog`
- **Purpose:** Recovery with corrupt log
- **Fault:** Log corruption discovered during recovery
- **Invariants Tested:** `vsr_recovery_safety`, `hash_chain`
- **Expected Behavior:** Recovery detects corruption, repairs from peers
- **Usage:** `cargo run --bin vopr -- run --scenario recovery_corrupt_log --iterations 5000`

---

## Phase 4: Gray Failures (2)

These scenarios test partial failure modes that are difficult to detect.

### 24. GrayFailureSlowDisk
- **Enum:** `ScenarioType::GrayFailureSlowDisk`
- **Purpose:** Slow disk I/O (high latency, variable performance)
- **Fault:** Disk latency spikes (p99 > 1s)
- **Invariants Tested:** Timeout handling, liveness
- **Expected Behavior:** System remains available despite slow disk
- **Usage:** `cargo run --bin vopr -- run --scenario gray_failure_slow_disk --iterations 5000`

### 25. GrayFailureIntermittentNetwork
- **Enum:** `ScenarioType::GrayFailureIntermittentNetwork`
- **Purpose:** Intermittent network (packets arrive out of order, duplicated)
- **Fault:** Network instability (packet loss, duplication, reordering)
- **Invariants Tested:** `vsr_agreement`, message deduplication
- **Expected Behavior:** Protocol handles network instability gracefully
- **Usage:** `cargo run --bin vopr -- run --scenario gray_failure_intermittent_network --iterations 5000`

---

## Phase 5: Race Conditions (2)

These scenarios test concurrent operations that may race.

### 26. RaceConcurrentViewChanges
- **Enum:** `ScenarioType::RaceConcurrentViewChanges`
- **Purpose:** Multiple view changes triggered simultaneously
- **Fault:** Network partition triggers concurrent view changes
- **Invariants Tested:** `vsr_view_change_safety`, `vsr_agreement`
- **Expected Behavior:** Only one view change succeeds, others abort safely
- **Usage:** `cargo run --bin vopr -- run --scenario race_concurrent_view_changes --iterations 5000`

### 27. RaceCommitDuringDvc
- **Enum:** `ScenarioType::RaceCommitDuringDvc`
- **Purpose:** Commit arrives during DoViewChange processing
- **Fault:** Network heals mid-view-change
- **Invariants Tested:** `vsr_view_change_safety`, `vsr_agreement`
- **Expected Behavior:** View change completes or safely transitions back to normal
- **Usage:** `cargo run --bin vopr -- run --scenario race_commit_during_dvc --iterations 5000`

---

## Phase 6: Clock Synchronization (3)

These scenarios test clock drift, NTP failures, and time-related issues.

### 28. ClockDrift
- **Enum:** `ScenarioType::ClockDrift`
- **Purpose:** Clock drift detection and tolerance
- **Fault:** Replica clocks drift apart (within tolerance)
- **Invariants Tested:** Marzullo's algorithm, commit timestamps
- **Expected Behavior:** System tolerates drift up to configured threshold
- **Usage:** `cargo run --bin vopr -- run --scenario clock_drift --iterations 5000`

### 29. ClockOffsetExceeded
- **Enum:** `ScenarioType::ClockOffsetExceeded`
- **Purpose:** Clock offset exceeds tolerance
- **Fault:** Replica clock drifts beyond threshold
- **Invariants Tested:** Clock offset detection
- **Expected Behavior:** Replica detects excessive drift, refuses to participate
- **Usage:** `cargo run --bin vopr -- run --scenario clock_offset_exceeded --iterations 5000`

### 30. ClockNtpFailure
- **Enum:** `ScenarioType::ClockNtpFailure`
- **Purpose:** NTP-style failures (unreachable, wrong time, etc.)
- **Fault:** Clock sync fails (NTP unreachable)
- **Invariants Tested:** Clock fault detection
- **Expected Behavior:** System uses fallback clock source or local clock
- **Usage:** `cargo run --bin vopr -- run --scenario clock_ntp_failure --iterations 5000`

---

## Phase 7: Client Sessions (3)

These scenarios test client session management based on VRR (Viewstamped Replication Revisited).

### 31. ClientSessionCrash
- **Enum:** `ScenarioType::ClientSessionCrash`
- **Purpose:** Successive client crashes (VRR Bug #1)
- **Fault:** Client crashes and reconnects repeatedly
- **Invariants Tested:** Session consistency, exactly-once semantics
- **Expected Behavior:** Idempotency ensures exactly-once execution despite crashes
- **Usage:** `cargo run --bin vopr -- run --scenario client_session_crash --iterations 5000`

### 32. ClientSessionViewChangeLockout
- **Enum:** `ScenarioType::ClientSessionViewChangeLockout`
- **Purpose:** View change lockout prevention (VRR Bug #2)
- **Fault:** View change during client request
- **Invariants Tested:** Session forwarding, liveness
- **Expected Behavior:** Client request forwarded to new primary, no lockout
- **Usage:** `cargo run --bin vopr -- run --scenario client_session_view_change_lockout --iterations 5000`

### 33. ClientSessionEviction
- **Enum:** `ScenarioType::ClientSessionEviction`
- **Purpose:** Deterministic session eviction (when session table full)
- **Fault:** Session table capacity exceeded
- **Invariants Tested:** Session eviction policy, fairness
- **Expected Behavior:** LRU eviction, evicted clients retry successfully
- **Usage:** `cargo run --bin vopr -- run --scenario client_session_eviction --iterations 5000`

---

## Phase 8: Repair Budget & Timeouts (5)

These scenarios test repair protocol, EWMA-based selection, timeouts, and primary abdication.

### 34. RepairBudgetPreventsStorm
- **Enum:** `ScenarioType::RepairBudgetPreventsStorm`
- **Purpose:** Repair budget prevents repair storms
- **Fault:** Multiple replicas need repair simultaneously
- **Invariants Tested:** Repair budget enforcement
- **Expected Behavior:** Repair requests rate-limited to prevent network saturation
- **Usage:** `cargo run --bin vopr -- run --scenario repair_budget_prevents_storm --iterations 5000`

### 35. RepairEwmaSelection
- **Enum:** `ScenarioType::RepairEwmaSelection`
- **Purpose:** EWMA-based smart replica selection for repair
- **Fault:** Some replicas consistently slow
- **Invariants Tested:** Repair efficiency
- **Expected Behavior:** Repair preferentially uses fast replicas (EWMA-weighted)
- **Usage:** `cargo run --bin vopr -- run --scenario repair_ewma_selection --iterations 5000`

### 36. RepairSyncTimeout
- **Enum:** `ScenarioType::RepairSyncTimeout`
- **Purpose:** Sync timeout escalates to state transfer
- **Fault:** Repair takes too long (large gap)
- **Invariants Tested:** State transfer fallback
- **Expected Behavior:** After timeout, switches from repair to state transfer
- **Usage:** `cargo run --bin vopr -- run --scenario repair_sync_timeout --iterations 5000`

### 37. PrimaryAbdicatePartition
- **Enum:** `ScenarioType::PrimaryAbdicatePartition`
- **Purpose:** Primary abdicates when partitioned from quorum
- **Fault:** Primary loses contact with majority
- **Invariants Tested:** Liveness, availability
- **Expected Behavior:** Primary steps down, new primary elected
- **Usage:** `cargo run --bin vopr -- run --scenario primary_abdicate_partition --iterations 5000`

### 38. CommitStallDetection
- **Enum:** `ScenarioType::CommitStallDetection`
- **Purpose:** Commit stall detection triggers view change
- **Fault:** Primary stops making progress
- **Invariants Tested:** Liveness
- **Expected Behavior:** Backups detect stall, trigger view change
- **Usage:** `cargo run --bin vopr -- run --scenario commit_stall_detection --iterations 5000`

---

## Phase 9: Storage Integrity / Scrubbing (4)

These scenarios test background data integrity checks (scrubbing).

### 39. ScrubDetectsCorruption
- **Enum:** `ScenarioType::ScrubDetectsCorruption`
- **Purpose:** Scrub detects corruption via checksum validation
- **Fault:** Silent corruption in storage
- **Invariants Tested:** Background scrubbing, corruption detection
- **Expected Behavior:** Scrubber detects corruption, logs alert
- **Usage:** `cargo run --bin vopr -- run --scenario scrub_detects_corruption --iterations 5000`

### 40. ScrubCompletesTour
- **Enum:** `ScenarioType::ScrubCompletesTour`
- **Purpose:** Scrub tour completes within time limit
- **Fault:** None (tests scrub performance)
- **Invariants Tested:** Scrub completeness
- **Expected Behavior:** Full scrub completes within configured time window
- **Usage:** `cargo run --bin vopr -- run --scenario scrub_completes_tour --iterations 5000`

### 41. ScrubRateLimited
- **Enum:** `ScenarioType::ScrubRateLimited`
- **Purpose:** Scrub rate limiting respects IOPS budget
- **Fault:** None (tests scrub throttling)
- **Invariants Tested:** Scrub rate limiting
- **Expected Behavior:** Scrub stays within IOPS budget, doesn't impact foreground ops
- **Usage:** `cargo run --bin vopr -- run --scenario scrub_rate_limited --iterations 5000`

### 42. ScrubTriggersRepair
- **Enum:** `ScenarioType::ScrubTriggersRepair`
- **Purpose:** Scrub triggers repair on corruption detection
- **Fault:** Corruption detected by scrubber
- **Invariants Tested:** Scrub → Repair integration
- **Expected Behavior:** Scrubber detects corruption, triggers repair from peers
- **Usage:** `cargo run --bin vopr -- run --scenario scrub_triggers_repair --iterations 5000`

---

## Phase 10: Cluster Reconfiguration (3)

These scenarios test cluster membership changes (add/remove replicas).

### 43. ReconfigAddReplicas
- **Enum:** `ScenarioType::ReconfigAddReplicas`
- **Purpose:** Add replicas (3 → 5)
- **Fault:** None (tests reconfig protocol)
- **Invariants Tested:** Reconfiguration safety, data consistency
- **Expected Behavior:** New replicas join, catch up, participate in quorum
- **Usage:** `cargo run --bin vopr -- run --scenario reconfig_add_replicas --iterations 5000`

### 44. ReconfigRemoveReplicas
- **Enum:** `ScenarioType::ReconfigRemoveReplicas`
- **Purpose:** Remove replicas (5 → 3)
- **Fault:** None (tests reconfig protocol)
- **Invariants Tested:** Reconfiguration safety, quorum transition
- **Expected Behavior:** Replicas removed gracefully, quorum maintained
- **Usage:** `cargo run --bin vopr -- run --scenario reconfig_remove_replicas --iterations 5000`

### 45. ReconfigDuringPartition
- **Enum:** `ScenarioType::ReconfigDuringPartition`
- **Purpose:** Reconfiguration during network partition
- **Fault:** Network partition during reconfiguration
- **Invariants Tested:** Reconfiguration safety under faults
- **Expected Behavior:** Reconfiguration completes or safely aborts
- **Usage:** `cargo run --bin vopr -- run --scenario reconfig_during_partition --iterations 5000`

---

## Running Scenarios

### Individual Scenario

```bash
# Run specific scenario
cargo run --bin vopr -- run --scenario baseline --iterations 10000

# With specific seed (for reproduction)
cargo run --bin vopr -- run --scenario combined --seed 42 --iterations 1000

# Save failures
cargo run --bin vopr -- run --scenario byzantine_commit_desync --save-failures
```

### All Scenarios

```bash
# Run all 46 scenarios (10k iterations each)
just vopr-full 10000

# Quick smoke test (100 iterations each)
just vopr-quick
```

### Scenario Groups by Phase

```bash
# Phase 0: Core
for scenario in baseline swizzle_clogging gray_failures multi_tenant_isolation time_compression combined; do
  cargo run --bin vopr -- run --scenario $scenario --iterations 5000
done

# Phase 1: Byzantine
for scenario in byzantine_view_change_merge byzantine_commit_desync byzantine_inflated_commit byzantine_invalid_metadata byzantine_malicious_view_change byzantine_leader_race byzantine_dvc_tail_length_mismatch byzantine_dvc_identical_claims byzantine_oversized_start_view byzantine_invalid_repair_range byzantine_invalid_kernel_command; do
  cargo run --bin vopr -- run --scenario $scenario --iterations 5000
done

# Etc. for other phases...
```

---

## Scenario Coverage Matrix

| Phase | Scenarios | Invariants Tested | Bugs Found | Status |
|-------|-----------|-------------------|------------|--------|
| 0: Core | 6 | All | - | ✅ Stable |
| 1: Byzantine | 11 | VSR safety | 6 bugs | ✅ Fixed |
| 2: Corruption | 3 | Hash chain, CRC32 | - | ✅ Stable |
| 3: Crash/Recovery | 3 | Recovery safety | - | ✅ Stable |
| 4: Gray Failures | 2 | Timeout, liveness | - | ✅ Stable |
| 5: Race Conditions | 2 | View change safety | - | ✅ Stable |
| 6: Clock | 3 | Marzullo, timestamps | - | ✅ Stable |
| 7: Client Sessions | 3 | Session consistency | 2 bugs (VRR) | ✅ Fixed |
| 8: Repair/Timeout | 5 | Repair budget, EWMA | - | ✅ Stable |
| 9: Scrubbing | 4 | Background integrity | - | ✅ Stable |
| 10: Reconfiguration | 3 | Reconfig safety | - | 🚧 In progress |

---

## Extending Scenarios

### Adding a New Scenario

1. **Add enum variant** to `ScenarioType` in `scenarios.rs`:
   ```rust
   pub enum ScenarioType {
       // ...
       MyNewScenario,
   }
   ```

2. **Implement `name()` and `description()`**:
   ```rust
   Self::MyNewScenario => "My New Scenario",
   // ...
   Self::MyNewScenario => "Description of what this tests",
   ```

3. **Create scenario configuration** in `create_scenario()`:
   ```rust
   Self::MyNewScenario => {
       let mut config = default_config(self);
       // Configure faults, workload, etc.
       config
   }
   ```

4. **Add tests**:
   ```rust
   #[test]
   fn test_my_new_scenario() {
       let scenario = ScenarioType::MyNewScenario;
       let config = scenario.create_scenario();
       // Run simulation, assert invariants
   }
   ```

5. **Document** in this file (add to appropriate phase section above)

### Debugging Failed Scenarios

When a scenario fails:

1. **Reproduce with seed**:
   ```bash
   cargo run --bin vopr -- repro failure-20260205-143022.kmb --verbose
   ```

2. **Visualize timeline**:
   ```bash
   cargo run --bin vopr -- timeline failure.kmb --show-messages --show-faults
   ```

3. **Find first failing event**:
   ```bash
   cargo run --bin vopr -- bisect failure.kmb
   ```

4. **Minimize test case**:
   ```bash
   cargo run --bin vopr -- minimize failure.kmb --output minimal.kmb
   ```

5. **Analyze with dashboard**:
   ```bash
   cargo run --bin vopr -- dashboard --data-dir ./failures/
   # Open http://localhost:8080
   ```

---

## Related Documentation

- [VOPR CLI Reference](/docs/reference/cli/vopr.md) - Public CLI documentation
- [VOPR Overview](overview.md) - VOPR architecture and capabilities
- [VOPR Deployment](deployment.md) - AWS testing infrastructure
- [VOPR Debugging](debugging.md) - Advanced debugging techniques
- [Writing Scenarios](writing-scenarios.md) - How to add new scenarios

---

**Last updated:** 2026-02-05 (v0.4.0)
**Scenario count:** 46 scenarios across 11 phases
