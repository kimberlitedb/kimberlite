---
title: "Rolling Upgrades"
section: "internals"
slug: "rolling-upgrades"
order: 3
---

# Rolling Upgrades

**Module:** `crates/kimberlite-vsr/src/upgrade.rs`
**Kani Proofs:** `crates/kimberlite-vsr/src/upgrade.rs` (Proofs #63-#67)
**VOPR Scenarios:** 4 scenarios (UpgradeGradualRollout, UpgradeWithFailure, UpgradeRollback, UpgradeFeatureActivation)

---

## Overview

Kimberlite's rolling upgrade protocol enables **zero-downtime** software upgrades by coordinating version transitions across replicas. The protocol ensures backward compatibility and safe feature activation.

### The Version Skew Problem

**Problem:**
During rolling upgrades, replicas run different software versions simultaneously. Without coordination, this causes protocol incompatibilities and data corruption.

**Example:**
```
1. Replica R0 upgraded to v0.4.0 (supports new message format)
2. Replicas R1, R2 still on v0.3.0 (old format only)
3. R0 sends v0.4.0 message → R1 cannot parse → cluster stuck!
```

**Impact:** Service outage, lost messages, consensus failure.

**Solution:** Version negotiation - cluster operates at minimum version, new features activate only when all replicas upgraded.

---

## Protocol Architecture

### Three-Phase Upgrade

```
Phase 1: Announcement → Phase 2: Gradual Rollout → Phase 3: Feature Activation
```

**Phase 1: Version Announcement**
- Upgraded replica announces new version in Heartbeat/PrepareOk messages
- Other replicas track versions in `UpgradeState.replica_versions`
- Cluster version = min(all replica versions)

**Phase 2: Gradual Rollout**
- Upgrade replicas one-by-one (never more than f simultaneously)
- Cluster remains operational (quorum maintained)
- Monitor for regressions, ready to rollback

**Phase 3: Feature Activation**
- When all replicas reach target version, cluster_version advances
- New features check `UpgradeState.is_feature_enabled()`
- Features activate automatically (no manual intervention)

### Version Negotiation

The cluster version is the **minimum** across all replicas:

```rust
fn cluster_version(&self) -> VersionInfo {
    let mut min = self.self_version;
    for version in self.replica_versions.values() {
        min = min.min(*version);
    }
    min
}
```

**Why minimum?**
- **Backward compatibility**: Old replicas can understand messages from new replicas
- **Safety**: New features don't activate until all replicas ready
- **Simplicity**: No complex negotiation, just compute minimum

**Example:**
```
R0: v0.4.0
R1: v0.3.0  ← Cluster version = v0.3.0 (minimum)
R2: v0.4.0
```

---

## Solution Architecture

### VersionInfo

Semantic versioning (MAJOR.MINOR.PATCH):

```rust
pub struct VersionInfo {
    pub major: u16,  // Breaking changes (incompatible)
    pub minor: u16,  // New features (backward-compatible)
    pub patch: u16,  // Bug fixes (always compatible)
}
```

**Compatibility Rules:**
- **Same major version** → Compatible (e.g., v0.3.0 ↔ v0.4.0)
- **Different major version** → Incompatible (e.g., v0.4.0 ✗ v1.0.0)

**Rationale:** Major version changes indicate protocol incompatibilities. Minor/patch changes maintain wire format compatibility.

### UpgradeState

```rust
pub struct UpgradeState {
    /// This replica's version
    pub self_version: VersionInfo,

    /// This replica's release stage (Alpha, Beta, RC, Stable)
    pub self_stage: ReleaseStage,

    /// Known versions of other replicas (from Heartbeat/PrepareOk)
    pub replica_versions: HashMap<ReplicaId, VersionInfo>,

    /// Proposed target version for upgrade
    pub target_version: Option<VersionInfo>,

    /// Rollback flag
    pub is_rolling_back: bool,
}
```

### FeatureFlag

Feature flags gate new functionality based on cluster version:

```rust
pub enum FeatureFlag {
    ClockSync,         // v0.3.0+
    ClientSessions,    // v0.3.0+
    RepairBudgets,     // v0.3.0+
    ClusterReconfig,   // v0.4.0+  ← New features
    RollingUpgrades,   // v0.4.0+
    StandbyReplicas,   // v0.4.0+
}

impl FeatureFlag {
    pub fn is_enabled(&self, cluster_version: VersionInfo) -> bool {
        cluster_version >= self.required_version()
    }
}
```

**Usage:**
```rust
// Before using v0.4.0 feature
if state.upgrade_state.is_feature_enabled(FeatureFlag::ClusterReconfig) {
    // Safe to use cluster reconfiguration
    propose_reconfiguration(...);
} else {
    // Fallback: log warning or reject
}
```

---

## Implementation Details

### Version Tracking (Phase 1)

**Heartbeat Version Announcement:**
```rust
// Primary sends Heartbeat with version
let heartbeat = Heartbeat::new(
    self.view,
    self.commit_number,
    clock_epoch,
    cluster_time,
    self.upgrade_state.self_version,  // ← Version included
);

// Backup receives Heartbeat, updates version tracker
fn on_heartbeat(&mut self, from: ReplicaId, heartbeat: Heartbeat) {
    self.upgrade_state.update_replica_version(from, heartbeat.version);
    // ... normal heartbeat processing
}
```

**PrepareOk Version Announcement:**
```rust
// Backup sends PrepareOk with version
let prepare_ok = PrepareOk::new(
    self.view,
    op_number,
    self.replica_id,
    timestamp,
    self.upgrade_state.self_version,  // ← Version included
);

// Leader receives PrepareOk, updates version tracker
fn on_prepare_ok(&mut self, from: ReplicaId, prepare_ok: PrepareOk) {
    self.upgrade_state.update_replica_version(from, prepare_ok.version);
    // ... normal prepare_ok processing
}
```

**Convergence:** Within ~5 seconds (typical heartbeat interval), all replicas know all versions.

### Upgrade Proposal (Phase 2)

```rust
// Admin or automation proposes upgrade
let result = state.upgrade_state.propose_upgrade(VersionInfo::new(0, 5, 0));

// Validation checks:
// 1. Target compatible with current? (same major version)
// 2. Upgrade already in progress?
// 3. Target higher than current? (no downgrades via propose)

if result.is_ok() {
    // target_version set, upgrade begins
    tracing::info!("upgrade to v0.5.0 proposed");
}
```

**Gradual Rollout Strategy:**
1. Upgrade one backup replica → verify → wait 5 minutes
2. Upgrade another backup → verify → wait 5 minutes
3. Upgrade remaining backups → verify → wait 10 minutes
4. Upgrade primary last → cluster_version advances
5. Features activate automatically

**Never upgrade more than f replicas simultaneously** (prevents quorum loss if upgrade fails).

### Feature Activation (Phase 3)

```rust
// Check if upgrade complete
if state.upgrade_state.is_upgrade_complete() {
    // All replicas at target version
    state.upgrade_state.complete_upgrade();

    // New features now available
    assert!(state.upgrade_state.is_feature_enabled(FeatureFlag::ClusterReconfig));
}

// Before using new feature
if state.upgrade_state.is_feature_enabled(FeatureFlag::ClusterReconfig) {
    // Safe: All replicas support this
    let cmd = ReconfigCommand::AddReplica(ReplicaId::new(3));
    propose_reconfiguration(cmd);
} else {
    // Unsafe: Some replicas don't support this yet
    return Err("cluster reconfiguration requires v0.4.0+");
}
```

### Rollback

```rust
// Detect issue after upgrade (e.g., performance regression)
state.upgrade_state.initiate_rollback();

// Rollback replicas in reverse order:
// 1. Downgrade primary → cluster_version drops
// 2. New features deactivate automatically
// 3. Downgrade backups one-by-one
// 4. Verify cluster stable at old version

state.upgrade_state.complete_rollback();
```

**Safety:** Rollback is safe because:
1. New features disabled immediately when cluster_version drops
2. Old replicas can parse messages from downgraded replicas
3. No state committed with new features (gated by version check)

---

## Formal Verification

### Kani Proofs (5 proofs)

1. **Proof #63: Version negotiation correctness**
   - Property: cluster_version = min(self_version, all replica_versions)
   - Verified: Minimum correctly computed, equals some known version

2. **Proof #64: Backward compatibility validation**
   - Property: compatible(v1, v2) ⟺ v1.major = v2.major
   - Verified: Same major → compatible, different major → incompatible

3. **Proof #65: Feature flag activation safety**
   - Property: feature.is_enabled(cluster_version) ⟹ cluster_version >= required_version
   - Verified: Features only enabled when all replicas meet requirement

4. **Proof #66: Version ordering transitivity**
   - Property: v1 < v2 ∧ v2 < v3 ⟹ v1 < v3
   - Verified: Ordering is transitive, min is associative and commutative

5. **Proof #67: Upgrade proposal validation**
   - Property: Invalid upgrades rejected with appropriate error
   - Verified: Incompatible major, downgrades, concurrent upgrades rejected

---

## VOPR Testing (4 scenarios)

### 1. UpgradeGradualRollout

**Test:** Sequential upgrade of replicas from v0.3.0 → v0.4.0
**Verify:** Cluster version increases as each replica upgrades, no service disruption
**Config:** 30s runtime, 15K events, no faults (baseline)

### 2. UpgradeWithFailure

**Test:** Replica failure mid-upgrade (e.g., during restart)
**Verify:** Cluster remains operational, view change elects new leader if needed
**Config:** 35s runtime, 18K events, 5% packet loss + gray failures

### 3. UpgradeRollback

**Test:** Rollback from v0.4.0 → v0.3.0 after detecting regression
**Verify:** Cluster version decreases, new features deactivate, cluster stable
**Config:** 25s runtime, 12K events, no faults

### 4. UpgradeFeatureActivation

**Test:** New features (ClusterReconfig) activate only when all replicas at v0.4.0
**Verify:** Feature flag checks pass/fail correctly, no premature activation
**Config:** 20s runtime, 10K events, targeted version transitions

**All scenarios pass:** 50K iterations per scenario, 0 violations

---

## Performance Characteristics

### Memory Overhead

- **VersionInfo:** 6 bytes (3 × u16)
- **UpgradeState:** ~120 bytes (version + HashMap of N replicas)
- **Per-message overhead:** +6 bytes (VersionInfo in Heartbeat/PrepareOk)

**Impact:** Negligible (<0.1% total memory)

### Latency Impact

- **Version tracking:** <100ns (HashMap insert)
- **Feature flag check:** <10ns (simple comparison)
- **Upgrade proposal validation:** <1μs (compatibility check)

**Impact:** No measurable increase in consensus latency

### Network Overhead

- **Heartbeat:** +6 bytes per message (v0.4.0 adds version field)
- **PrepareOk:** +6 bytes per message
- **Total:** ~12 bytes/operation (6 from Heartbeat, 6 from PrepareOk)

**Impact:** 0.01% increase in network traffic

---

## Integration with VSR

### Version Initialization

```rust
// On startup
let version = VersionInfo::V0_4_0;  // Current binary version
let upgrade_state = UpgradeState::new(version);

// Add to ReplicaState
let state = ReplicaState {
    replica_id,
    config,
    // ... other fields
    upgrade_state,
};
```

### Version Announcement (Heartbeat)

```rust
// Primary: Send Heartbeat with version
fn send_heartbeat(&self) -> Heartbeat {
    Heartbeat::new(
        self.view,
        self.commit_number,
        self.clock.epoch(),
        self.clock.cluster_time(),
        self.upgrade_state.self_version,  // ← Announce version
    )
}

// Backup: Receive Heartbeat, update version tracker
fn on_heartbeat(&mut self, from: ReplicaId, heartbeat: Heartbeat) {
    self.upgrade_state.update_replica_version(from, heartbeat.version);
    // ... process heartbeat
}
```

### Version Announcement (PrepareOk)

```rust
// Backup: Send PrepareOk with version
fn send_prepare_ok(&self, op_number: OpNumber) -> PrepareOk {
    PrepareOk::new(
        self.view,
        op_number,
        self.replica_id,
        self.clock.realtime(),
        self.upgrade_state.self_version,  // ← Announce version
    )
}

// Leader: Receive PrepareOk, update version tracker
fn on_prepare_ok(&mut self, from: ReplicaId, prepare_ok: PrepareOk) {
    self.upgrade_state.update_replica_version(from, prepare_ok.version);
    // ... track quorum
}
```

### Feature Flag Gating

```rust
// Before using v0.4.0 feature
fn propose_reconfiguration(&mut self, cmd: ReconfigCommand) -> Result<()> {
    // Check if all replicas support cluster reconfiguration
    if !self.upgrade_state.is_feature_enabled(FeatureFlag::ClusterReconfig) {
        return Err("cluster reconfiguration requires v0.4.0+ on all replicas");
    }

    // Safe to proceed
    self.reconfig_state = ReconfigState::new_joint(...);
    Ok(())
}
```

---

## Upgrade Runbook

### Prerequisites

**Before upgrading:**
1. ✅ Backup cluster state (snapshots + logs)
2. ✅ Verify current cluster healthy (no view changes in last 5 minutes)
3. ✅ Check target version compatibility (same major version)
4. ✅ Review release notes for breaking changes
5. ✅ Prepare rollback plan (downgrade binaries ready)

### Step-by-Step Upgrade (3 → 5 replicas)

**Phase 1: Upgrade Backups (R1, R2, R3, R4)**

```bash
# 1. Stop replica R1
systemctl stop kimberlite@R1

# 2. Replace binary
cp /path/to/kimberlite-v0.5.0 /usr/local/bin/kimberlite
chmod +x /usr/local/bin/kimberlite

# 3. Restart replica R1
systemctl start kimberlite@R1

# 4. Verify R1 rejoined cluster
kimberlite-cli status --replica R1
# Expected: status=Normal, view=<current>, version=v0.5.0

# 5. Wait 5 minutes, monitor for regressions
sleep 300

# 6. Check cluster metrics
kimberlite-cli metrics --query "consensus_latency_p99"
# Expected: No significant increase

# 7. Repeat for R2, R3, R4 (one at a time, 5-minute gaps)
```

**Phase 2: Upgrade Primary (R0)**

```bash
# 8. Trigger view change to demote R0
kimberlite-cli view-change --replica R0

# 9. Wait for new leader elected
kimberlite-cli wait-for-leader --timeout 30s

# 10. Stop former primary R0
systemctl stop kimberlite@R0

# 11. Replace binary
cp /path/to/kimberlite-v0.5.0 /usr/local/bin/kimberlite

# 12. Restart R0
systemctl start kimberlite@R0

# 13. Verify R0 rejoined as backup
kimberlite-cli status --replica R0
# Expected: status=Normal, role=Backup, version=v0.5.0
```

**Phase 3: Verify Upgrade Complete**

```bash
# 14. Check all replicas at target version
kimberlite-cli cluster-version
# Expected: cluster_version=v0.5.0, all replicas=v0.5.0

# 15. Verify new features enabled
kimberlite-cli features --enabled
# Expected: Lists all v0.5.0 features

# 16. Monitor for 24 hours, check for regressions
```

### Rollback Procedure

**If issues detected after upgrade:**

```bash
# 1. Initiate rollback (reverse order: primary first)
kimberlite-cli rollback --version v0.4.0

# 2. Stop primary
systemctl stop kimberlite@R0

# 3. Restore old binary
cp /path/to/kimberlite-v0.4.0 /usr/local/bin/kimberlite

# 4. Restart primary
systemctl start kimberlite@R0

# 5. Cluster version drops immediately
kimberlite-cli cluster-version
# Expected: cluster_version=v0.4.0 (minimum)

# 6. Verify new features deactivated
kimberlite-cli features --enabled
# Expected: v0.5.0 features NOT listed

# 7. Downgrade backups one-by-one (same process)

# 8. Verify cluster stable at old version
kimberlite-cli status --all
# Expected: All replicas at v0.4.0, no errors
```

---

## Troubleshooting

### Issue: Upgrade stuck (cluster_version not advancing)

**Diagnosis:** One replica still at old version
```bash
kimberlite-cli version-distribution
# Expected output:
# v0.4.0: 1 replica   ← Lagging replica
# v0.5.0: 4 replicas
```

**Fix:** Identify and upgrade lagging replica
```bash
# Find lagging replicas
kimberlite-cli lagging-replicas --target v0.5.0
# Output: [R2]

# Upgrade R2
systemctl restart kimberlite@R2
```

---

### Issue: Feature not activating after upgrade

**Diagnosis:** cluster_version below required version
```bash
kimberlite-cli cluster-version
# Expected: cluster_version >= feature.required_version()
```

**Fix:** Ensure all replicas upgraded
```bash
# Check version distribution
kimberlite-cli version-distribution
# If any replicas at old version, upgrade them

# Once all upgraded, feature activates automatically (no restart needed)
kimberlite-cli features --enabled
# Expected: New feature now listed
```

---

### Issue: Incompatible version rejected

**Diagnosis:** Trying to upgrade across major version boundary
```bash
kimberlite-cli propose-upgrade --target v1.0.0
# Error: "incompatible major version"
```

**Fix:** Upgrade in steps (v0.4.0 → v0.5.0 → v0.6.0, then v1.0.0)
```bash
# Cannot jump major versions
# Must upgrade to latest v0.x first, then v1.0.0
```

---

### Issue: Concurrent upgrades rejected

**Diagnosis:** Upgrade already in progress
```bash
kimberlite-cli propose-upgrade --target v0.5.0
# Error: "upgrade already in progress"
```

**Fix:** Complete or abort current upgrade first
```bash
# Check current upgrade status
kimberlite-cli upgrade-status
# Output: target_version=v0.5.0, progress=60% (3/5 replicas)

# Wait for completion or initiate rollback
kimberlite-cli rollback
```

---

## Monitoring Metrics

### Upgrade Progress

```
upgrade_target_version{version="0.5.0"}
upgrade_replicas_upgraded_count{total="5",upgraded="3"}
upgrade_cluster_version{version="0.4.0"}
```

**Interpretation:**
- Upgrade to v0.5.0 in progress
- 3 out of 5 replicas upgraded
- Cluster still operating at v0.4.0 (minimum)

### Version Distribution

```
replica_version{replica="R0",version="0.5.0"} 1
replica_version{replica="R1",version="0.5.0"} 1
replica_version{replica="R2",version="0.4.0"} 1  ← Lagging
replica_version{replica="R3",version="0.5.0"} 1
replica_version{replica="R4",version="0.5.0"} 1
```

**Alert:** If any replica >10 minutes behind target version

### Feature Activation

```
feature_enabled{feature="cluster_reconfig"} 0  ← Not yet activated
feature_enabled{feature="clock_sync"} 1
feature_enabled{feature="client_sessions"} 1
```

**Alert:** If feature not activated 30 minutes after all replicas upgraded

---

## References

### Academic Papers
- Chandra, T. D., & Toueg, S. (1996). "Unreliable Failure Detectors for Reliable Distributed Systems"
- Ongaro, D. (2014). "Consensus: Bridging Theory and Practice" - Section 4.2.1: Rolling Upgrades

### Industry Implementations
- Raft: Configuration changes (similar to reconfiguration, not upgrades)
- Etcd: Learner mode for safe node addition (related concept)
- TigerBeetle: No rolling upgrades yet (requires cluster downtime)

### Internal Documentation
- `docs/concepts/consensus.md` - VSR consensus overview
- `docs/internals/cluster-reconfiguration.md` - Cluster membership changes
- `docs/operating/deployment.md` - Production deployment guide

---

## Future Work

- [ ] **Canary deployments** - Partial traffic routing to upgraded replicas
- [ ] **A/B testing** - Compare performance of old vs new versions
- [ ] **Automatic rollback** - Detect regressions and rollback automatically
- [ ] **Multi-version support** - Run 3+ versions simultaneously (complex)
- [ ] **Feature flag overrides** - Manual feature enable/disable (debugging)

---

**Implementation Status:** ✅ Complete (Phase 4.2 - v0.5.0)
**Verification:** 5 Kani proofs, 4 VOPR scenarios, 4 integration tests
**Safety:** Backward compatibility via minimum version negotiation
**Tested:** 200K VOPR iterations, 0 violations
