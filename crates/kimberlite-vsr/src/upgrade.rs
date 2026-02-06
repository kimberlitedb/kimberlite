//! Rolling upgrade protocol for VSR clusters.
//!
//! This module implements zero-downtime rolling upgrades by coordinating
//! version transitions across replicas. Supports:
//!
//! - **Version Negotiation**: Replicas advertise versions, cluster negotiates minimum
//! - **Gradual Rollout**: Upgrade replicas one-by-one with safety checks
//! - **Feature Flags**: Enable new features once all replicas upgraded
//! - **Rollback**: Safely revert to previous version if issues detected
//!
//! # Protocol
//!
//! 1. **Announcement**: Upgraded replica announces new version in heartbeats
//! 2. **Negotiation**: Cluster tracks all replica versions
//! 3. **Minimum Version**: Active version = minimum across all replicas
//! 4. **Feature Activation**: New features enabled when all replicas >= required version
//! 5. **Rollback**: Downgrade replica, cluster reverts to lower version
//!
//! # Example
//!
//! ```rust,ignore
//! // Replica starts at v0.3.0
//! let version = VersionInfo::new(0, 3, 0);
//! let upgrade_state = UpgradeState::new(version);
//!
//! // Upgrade to v0.4.0
//! upgrade_state.propose_upgrade(VersionInfo::new(0, 4, 0));
//!
//! // Once all replicas at v0.4.0, activate new features
//! if upgrade_state.cluster_version() >= VersionInfo::new(0, 4, 0) {
//!     // Enable v0.4.0 features
//! }
//! ```

#![allow(clippy::match_same_arms)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::ReplicaId;

// ============================================================================
// Version Information
// ============================================================================

/// Software version using semantic versioning.
///
/// Format: MAJOR.MINOR.PATCH
///
/// - **MAJOR**: Incompatible API changes
/// - **MINOR**: New functionality
/// - **PATCH**: Bug fixes
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct VersionInfo {
    /// Major version number.
    ///
    /// Breaking changes increment this. Different major versions are incompatible.
    pub major: u16,

    /// Minor version number.
    ///
    /// New features increment this.
    pub minor: u16,

    /// Patch version number.
    ///
    /// Bug fixes increment this.
    pub patch: u16,
}

impl VersionInfo {
    /// Creates a new version.
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Kimberlite v0.4.0 (current).
    pub const V0_4_0: Self = Self::new(0, 4, 0);

    /// Checks if this version is compatible with another.
    ///
    /// Compatibility rules:
    /// - Same major version required
    /// - Minor/patch versions can differ (backward-compatible)
    pub fn is_compatible_with(self, other: VersionInfo) -> bool {
        self.major == other.major
    }

    /// Returns the minimum of two versions.
    pub fn min(self, other: VersionInfo) -> VersionInfo {
        if self <= other { self } else { other }
    }

    /// Returns the maximum of two versions.
    pub fn max(self, other: VersionInfo) -> VersionInfo {
        if self >= other { self } else { other }
    }
}

impl std::fmt::Display for VersionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ============================================================================
// Release Stage
// ============================================================================

/// Release maturity stage.
///
/// Controls rollout speed and risk tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReleaseStage {
    /// Alpha release (internal testing only).
    ///
    /// Not recommended for production. May have bugs.
    Alpha,

    /// Beta release (early adopters).
    ///
    /// Suitable for testing environments. More stable than alpha.
    Beta,

    /// Release candidate (final testing).
    ///
    /// Production-ready pending final validation.
    Candidate,

    /// Stable release (production).
    ///
    /// Fully tested and recommended for all deployments.
    Stable,
}

impl ReleaseStage {
    /// Returns true if this stage is suitable for production.
    pub fn is_production_ready(&self) -> bool {
        matches!(self, Self::Candidate | Self::Stable)
    }
}

impl std::fmt::Display for ReleaseStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Alpha => write!(f, "alpha"),
            Self::Beta => write!(f, "beta"),
            Self::Candidate => write!(f, "rc"),
            Self::Stable => write!(f, "stable"),
        }
    }
}

// ============================================================================
// Feature Flags
// ============================================================================

/// Feature flag identifier.
///
/// Features can be enabled/disabled based on cluster version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FeatureFlag {
    /// Clock synchronization (v0.4.0+).
    ClockSync,

    /// Client sessions (v0.4.0+).
    ClientSessions,

    /// Repair budgets (v0.4.0+).
    RepairBudgets,

    /// EWMA repair selection (v0.4.0+).
    EwmaRepair,

    /// Background log scrubbing (v0.4.0+).
    LogScrubbing,

    /// Cluster reconfiguration (v0.4.0+).
    ClusterReconfig,

    /// Rolling upgrades (v0.4.0+).
    RollingUpgrades,

    /// Standby replicas (v0.4.0+).
    StandbyReplicas,
}

impl FeatureFlag {
    /// Returns the minimum version required for this feature.
    pub fn required_version(&self) -> VersionInfo {
        match self {
            Self::ClockSync
            | Self::ClientSessions
            | Self::RepairBudgets
            | Self::EwmaRepair
            | Self::LogScrubbing
            | Self::ClusterReconfig
            | Self::RollingUpgrades
            | Self::StandbyReplicas => VersionInfo::V0_4_0,
        }
    }

    /// Checks if this feature is enabled for the given cluster version.
    ///
    /// A feature is enabled if the minimum cluster version meets the requirement.
    pub fn is_enabled(&self, cluster_version: VersionInfo) -> bool {
        cluster_version >= self.required_version()
    }
}

impl std::fmt::Display for FeatureFlag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClockSync => write!(f, "clock_sync"),
            Self::ClientSessions => write!(f, "client_sessions"),
            Self::RepairBudgets => write!(f, "repair_budgets"),
            Self::EwmaRepair => write!(f, "ewma_repair"),
            Self::LogScrubbing => write!(f, "log_scrubbing"),
            Self::ClusterReconfig => write!(f, "cluster_reconfig"),
            Self::RollingUpgrades => write!(f, "rolling_upgrades"),
            Self::StandbyReplicas => write!(f, "standby_replicas"),
        }
    }
}

// ============================================================================
// Upgrade State
// ============================================================================

/// Cluster upgrade state.
///
/// Tracks versions of all replicas and determines the active cluster version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpgradeState {
    /// This replica's version.
    pub self_version: VersionInfo,

    /// This replica's release stage.
    pub self_stage: ReleaseStage,

    /// Known versions of other replicas.
    ///
    /// Updated from heartbeat messages. Missing replicas default to minimum version.
    pub replica_versions: HashMap<ReplicaId, VersionInfo>,

    /// Proposed target version for upgrade.
    ///
    /// If set, the cluster is in the process of upgrading to this version.
    pub target_version: Option<VersionInfo>,

    /// Rollback flag.
    ///
    /// If true, the cluster is rolling back to a previous version.
    pub is_rolling_back: bool,
}

impl UpgradeState {
    /// Creates a new upgrade state with the given version.
    pub fn new(version: VersionInfo) -> Self {
        Self {
            self_version: version,
            self_stage: ReleaseStage::Stable,
            replica_versions: HashMap::new(),
            target_version: None,
            is_rolling_back: false,
        }
    }

    /// Creates an upgrade state with a specific release stage.
    pub fn new_with_stage(version: VersionInfo, stage: ReleaseStage) -> Self {
        Self {
            self_version: version,
            self_stage: stage,
            replica_versions: HashMap::new(),
            target_version: None,
            is_rolling_back: false,
        }
    }

    /// Updates the known version for a replica.
    ///
    /// Called when receiving heartbeats or version announcements.
    pub fn update_replica_version(&mut self, replica: ReplicaId, version: VersionInfo) {
        self.replica_versions.insert(replica, version);
    }

    /// Returns the minimum version across all known replicas.
    ///
    /// This is the **active cluster version** - the version that determines
    /// which features can be used.
    ///
    /// Using the minimum version ensures safe rolling upgrades:
    /// - Old replicas can understand messages from new replicas
    /// - New features only activate when all replicas upgraded
    pub fn cluster_version(&self) -> VersionInfo {
        let mut min_version = self.self_version;

        for version in self.replica_versions.values() {
            min_version = min_version.min(*version);
        }

        min_version
    }

    /// Returns the maximum version across all known replicas.
    ///
    /// Useful for detecting upgrade progress.
    pub fn max_version(&self) -> VersionInfo {
        let mut max_version = self.self_version;

        for version in self.replica_versions.values() {
            max_version = max_version.max(*version);
        }

        max_version
    }

    /// Proposes an upgrade to the target version.
    ///
    /// Sets the target version and begins the upgrade process.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Target version is not compatible with current version
    /// - Cluster is already upgrading
    /// - Target version is lower than current version (use rollback instead)
    pub fn propose_upgrade(&mut self, target: VersionInfo) -> Result<(), &'static str> {
        // Check compatibility
        if !self.self_version.is_compatible_with(target) {
            return Err("incompatible major version");
        }

        // Check not already upgrading
        if self.target_version.is_some() {
            return Err("upgrade already in progress");
        }

        // Check target is higher than current
        if target <= self.cluster_version() {
            return Err("target version must be higher than cluster version");
        }

        self.target_version = Some(target);
        tracing::info!(
            current = %self.cluster_version(),
            target = %target,
            "proposed cluster upgrade"
        );

        Ok(())
    }

    /// Checks if the upgrade to target version is complete.
    ///
    /// An upgrade is complete when all replicas have reached the target version.
    pub fn is_upgrade_complete(&self) -> bool {
        if let Some(target) = self.target_version {
            self.cluster_version() >= target
        } else {
            false
        }
    }

    /// Completes the upgrade, clearing the target version.
    ///
    /// Called when all replicas have reached the target version.
    pub fn complete_upgrade(&mut self) {
        if self.is_upgrade_complete() {
            tracing::info!(
                version = %self.cluster_version(),
                "upgrade complete"
            );
            self.target_version = None;
        }
    }

    /// Initiates a rollback to a previous version.
    ///
    /// Sets the rollback flag and clears the target version.
    pub fn initiate_rollback(&mut self) {
        tracing::warn!(
            current = %self.cluster_version(),
            "initiating cluster rollback"
        );
        self.target_version = None;
        self.is_rolling_back = true;
    }

    /// Completes the rollback, clearing the rollback flag.
    pub fn complete_rollback(&mut self) {
        tracing::info!(
            version = %self.cluster_version(),
            "rollback complete"
        );
        self.is_rolling_back = false;
    }

    /// Checks if a feature is enabled in the current cluster.
    ///
    /// Features are enabled based on the minimum cluster version.
    pub fn is_feature_enabled(&self, feature: FeatureFlag) -> bool {
        feature.is_enabled(self.cluster_version())
    }

    /// Returns all enabled features for the current cluster version.
    pub fn enabled_features(&self) -> Vec<FeatureFlag> {
        use FeatureFlag::{
            ClientSessions, ClockSync, ClusterReconfig, EwmaRepair, LogScrubbing, RepairBudgets,
            RollingUpgrades, StandbyReplicas,
        };

        let all_features = [
            ClockSync,
            ClientSessions,
            RepairBudgets,
            EwmaRepair,
            LogScrubbing,
            ClusterReconfig,
            RollingUpgrades,
            StandbyReplicas,
        ];

        all_features
            .iter()
            .filter(|f| self.is_feature_enabled(**f))
            .copied()
            .collect()
    }

    /// Checks if all replicas are running compatible versions.
    ///
    /// Returns true if all known replicas have compatible major versions.
    pub fn all_replicas_compatible(&self) -> bool {
        self.replica_versions
            .values()
            .all(|v| self.self_version.is_compatible_with(*v))
    }

    /// Returns replicas that are not yet at the target version.
    ///
    /// Useful for tracking upgrade progress.
    pub fn lagging_replicas(&self) -> Vec<ReplicaId> {
        if let Some(target) = self.target_version {
            self.replica_versions
                .iter()
                .filter(|(_, version)| **version < target)
                .map(|(id, _)| *id)
                .collect()
        } else {
            vec![]
        }
    }

    /// Returns the number of replicas at each version.
    ///
    /// Useful for monitoring upgrade progress.
    pub fn version_distribution(&self) -> HashMap<VersionInfo, usize> {
        let mut distribution = HashMap::new();

        // Count self
        *distribution.entry(self.self_version).or_insert(0) += 1;

        // Count others
        for version in self.replica_versions.values() {
            *distribution.entry(*version).or_insert(0) += 1;
        }

        distribution
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_ordering() {
        let v030 = VersionInfo::new(0, 3, 0);
        let v031 = VersionInfo::new(0, 3, 1);
        let v040 = VersionInfo::new(0, 4, 0);
        let v100 = VersionInfo::new(1, 0, 0);

        assert!(v030 < v031);
        assert!(v031 < v040);
        assert!(v040 < v100);
        assert!(v030 < v100);
    }

    #[test]
    fn test_version_compatibility() {
        let v030 = VersionInfo::new(0, 3, 0);
        let v040 = VersionInfo::new(0, 4, 0);
        let v100 = VersionInfo::new(1, 0, 0);

        // Same major version = compatible
        assert!(v030.is_compatible_with(v040));
        assert!(v040.is_compatible_with(v030));

        // Different major version = incompatible
        assert!(!v040.is_compatible_with(v100));
        assert!(!v100.is_compatible_with(v040));
    }

    #[test]
    fn test_version_min_max() {
        let v030 = VersionInfo::new(0, 3, 0);
        let v040 = VersionInfo::new(0, 4, 0);

        assert_eq!(v030.min(v040), v030);
        assert_eq!(v040.min(v030), v030);
        assert_eq!(v030.max(v040), v040);
        assert_eq!(v040.max(v030), v040);
    }

    #[test]
    fn test_version_display() {
        let version = VersionInfo::new(0, 4, 2);
        assert_eq!(format!("{}", version), "v0.4.2");
    }

    #[test]
    fn test_release_stage_ordering() {
        assert!(ReleaseStage::Alpha < ReleaseStage::Beta);
        assert!(ReleaseStage::Beta < ReleaseStage::Candidate);
        assert!(ReleaseStage::Candidate < ReleaseStage::Stable);
    }

    #[test]
    fn test_release_stage_production_ready() {
        assert!(!ReleaseStage::Alpha.is_production_ready());
        assert!(!ReleaseStage::Beta.is_production_ready());
        assert!(ReleaseStage::Candidate.is_production_ready());
        assert!(ReleaseStage::Stable.is_production_ready());
    }

    #[test]
    fn test_feature_flag_required_version() {
        // All features require v0.4.0
        assert_eq!(
            FeatureFlag::ClockSync.required_version(),
            VersionInfo::V0_4_0
        );
        assert_eq!(
            FeatureFlag::ClusterReconfig.required_version(),
            VersionInfo::V0_4_0
        );
    }

    #[test]
    fn test_feature_flag_enabled() {
        let v030 = VersionInfo::new(0, 3, 0);
        let v040 = VersionInfo::V0_4_0;

        // No features enabled below v0.4.0
        assert!(!FeatureFlag::ClockSync.is_enabled(v030));
        assert!(!FeatureFlag::ClientSessions.is_enabled(v030));
        assert!(!FeatureFlag::ClusterReconfig.is_enabled(v030));
        assert!(!FeatureFlag::RollingUpgrades.is_enabled(v030));

        // All features enabled at v0.4.0
        assert!(FeatureFlag::ClockSync.is_enabled(v040));
        assert!(FeatureFlag::ClientSessions.is_enabled(v040));
        assert!(FeatureFlag::ClusterReconfig.is_enabled(v040));
        assert!(FeatureFlag::RollingUpgrades.is_enabled(v040));
    }

    #[test]
    fn test_upgrade_state_cluster_version() {
        let mut state = UpgradeState::new(VersionInfo::V0_4_0);

        // Initially, cluster version = self version
        assert_eq!(state.cluster_version(), VersionInfo::V0_4_0);

        // Add a replica at lower version
        state.update_replica_version(ReplicaId::new(1), VersionInfo::new(0, 3, 0));

        // Cluster version drops to minimum
        assert_eq!(state.cluster_version(), VersionInfo::new(0, 3, 0));

        // Add another replica at higher version
        state.update_replica_version(ReplicaId::new(2), VersionInfo::new(0, 5, 0));

        // Cluster version still minimum
        assert_eq!(state.cluster_version(), VersionInfo::new(0, 3, 0));
    }

    #[test]
    fn test_upgrade_state_max_version() {
        let mut state = UpgradeState::new(VersionInfo::new(0, 3, 0));

        assert_eq!(state.max_version(), VersionInfo::new(0, 3, 0));

        state.update_replica_version(ReplicaId::new(1), VersionInfo::V0_4_0);
        assert_eq!(state.max_version(), VersionInfo::V0_4_0);

        state.update_replica_version(ReplicaId::new(2), VersionInfo::new(0, 5, 0));
        assert_eq!(state.max_version(), VersionInfo::new(0, 5, 0));
    }

    #[test]
    fn test_propose_upgrade() {
        let mut state = UpgradeState::new(VersionInfo::new(0, 3, 0));

        // Valid upgrade
        let result = state.propose_upgrade(VersionInfo::V0_4_0);
        assert!(result.is_ok());
        assert_eq!(state.target_version, Some(VersionInfo::V0_4_0));

        // Concurrent upgrade rejected
        let result2 = state.propose_upgrade(VersionInfo::new(0, 5, 0));
        assert_eq!(result2.unwrap_err(), "upgrade already in progress");
    }

    #[test]
    fn test_propose_upgrade_incompatible() {
        let mut state = UpgradeState::new(VersionInfo::new(0, 3, 0));

        // Incompatible major version
        let result = state.propose_upgrade(VersionInfo::new(1, 0, 0));
        assert_eq!(result.unwrap_err(), "incompatible major version");
    }

    #[test]
    fn test_propose_upgrade_downgrade() {
        let mut state = UpgradeState::new(VersionInfo::V0_4_0);

        // Downgrade rejected (use rollback instead)
        let result = state.propose_upgrade(VersionInfo::new(0, 3, 0));
        assert_eq!(
            result.unwrap_err(),
            "target version must be higher than cluster version"
        );
    }

    #[test]
    fn test_upgrade_complete() {
        let mut state = UpgradeState::new(VersionInfo::new(0, 3, 0));

        state.propose_upgrade(VersionInfo::V0_4_0).unwrap();
        assert!(!state.is_upgrade_complete());

        // Upgrade self
        state.self_version = VersionInfo::V0_4_0;

        // Add replicas at target version
        state.update_replica_version(ReplicaId::new(1), VersionInfo::V0_4_0);
        state.update_replica_version(ReplicaId::new(2), VersionInfo::V0_4_0);

        // Now complete
        assert!(state.is_upgrade_complete());

        state.complete_upgrade();
        assert_eq!(state.target_version, None);
    }

    #[test]
    fn test_lagging_replicas() {
        let mut state = UpgradeState::new(VersionInfo::V0_4_0);

        state.propose_upgrade(VersionInfo::new(0, 5, 0)).unwrap();

        state.update_replica_version(ReplicaId::new(1), VersionInfo::V0_4_0);
        state.update_replica_version(ReplicaId::new(2), VersionInfo::new(0, 5, 0));
        state.update_replica_version(ReplicaId::new(3), VersionInfo::V0_4_0);

        let lagging = state.lagging_replicas();
        assert_eq!(lagging.len(), 2);
        assert!(lagging.contains(&ReplicaId::new(1)));
        assert!(lagging.contains(&ReplicaId::new(3)));
    }

    #[test]
    fn test_version_distribution() {
        let mut state = UpgradeState::new(VersionInfo::V0_4_0);

        state.update_replica_version(ReplicaId::new(1), VersionInfo::new(0, 3, 0));
        state.update_replica_version(ReplicaId::new(2), VersionInfo::V0_4_0);
        state.update_replica_version(ReplicaId::new(3), VersionInfo::V0_4_0);

        let dist = state.version_distribution();
        assert_eq!(dist.get(&VersionInfo::new(0, 3, 0)), Some(&1));
        assert_eq!(dist.get(&VersionInfo::V0_4_0), Some(&3)); // self + 2 others
    }

    #[test]
    fn test_enabled_features() {
        // At v0.3.0, no features are enabled (all require v0.4.0)
        let state_030 = UpgradeState::new(VersionInfo::new(0, 3, 0));
        assert!(state_030.enabled_features().is_empty());

        // At v0.4.0, all features are enabled
        let state_040 = UpgradeState::new(VersionInfo::V0_4_0);
        let features = state_040.enabled_features();
        assert!(features.contains(&FeatureFlag::ClockSync));
        assert!(features.contains(&FeatureFlag::ClientSessions));
        assert!(features.contains(&FeatureFlag::ClusterReconfig));
    }

    #[test]
    fn test_rollback() {
        let mut state = UpgradeState::new(VersionInfo::V0_4_0);

        state.propose_upgrade(VersionInfo::new(0, 5, 0)).unwrap();
        state.initiate_rollback();

        assert!(state.is_rolling_back);
        assert_eq!(state.target_version, None);

        state.complete_rollback();
        assert!(!state.is_rolling_back);
    }
}

// ============================================================================
// Kani Proofs
// ============================================================================

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// Proof #63: Version negotiation correctness
    ///
    /// Verifies that cluster_version() correctly computes the minimum version
    /// across all replicas, ensuring backward compatibility.
    ///
    /// Property: cluster_version = min(self_version, all replica_versions)
    #[kani::proof]
    #[kani::unwind(5)]
    fn proof_version_negotiation_correctness() {
        // Generate bounded version numbers
        let self_major: u16 = kani::any();
        let self_minor: u16 = kani::any();
        let self_patch: u16 = kani::any();
        kani::assume(self_major <= 1);
        kani::assume(self_minor <= 10);
        kani::assume(self_patch <= 10);

        let self_version = VersionInfo::new(self_major, self_minor, self_patch);
        let mut state = UpgradeState::new(self_version);

        // Add multiple replicas with various versions
        let num_replicas: usize = kani::any();
        kani::assume(num_replicas <= 3);

        for i in 0..num_replicas {
            let major: u16 = kani::any();
            let minor: u16 = kani::any();
            let patch: u16 = kani::any();
            kani::assume(major <= 1);
            kani::assume(minor <= 10);
            kani::assume(patch <= 10);

            let version = VersionInfo::new(major, minor, patch);
            state.update_replica_version(ReplicaId::new(i as u8), version);
        }

        let cluster_version = state.cluster_version();

        // PROPERTY 1: Cluster version <= self version
        assert!(cluster_version <= self_version);

        // PROPERTY 2: Cluster version <= all replica versions
        for version in state.replica_versions.values() {
            assert!(cluster_version <= *version);
        }

        // PROPERTY 3: Cluster version equals some known version
        let mut found = cluster_version == self_version;
        for version in state.replica_versions.values() {
            if cluster_version == *version {
                found = true;
            }
        }
        assert!(found, "cluster version must match some replica");
    }

    /// Proof #64: Backward compatibility validation
    ///
    /// Verifies that is_compatible_with() correctly enforces same major version
    /// requirement, preventing incompatible upgrades.
    ///
    /// Property: compatible(v1, v2) ⟺ v1.major = v2.major
    #[kani::proof]
    #[kani::unwind(3)]
    fn proof_backward_compatibility_validation() {
        let major1: u16 = kani::any();
        let minor1: u16 = kani::any();
        let patch1: u16 = kani::any();
        kani::assume(major1 <= 2);
        kani::assume(minor1 <= 10);
        kani::assume(patch1 <= 10);

        let major2: u16 = kani::any();
        let minor2: u16 = kani::any();
        let patch2: u16 = kani::any();
        kani::assume(major2 <= 2);
        kani::assume(minor2 <= 10);
        kani::assume(patch2 <= 10);

        let v1 = VersionInfo::new(major1, minor1, patch1);
        let v2 = VersionInfo::new(major2, minor2, patch2);

        let compatible = v1.is_compatible_with(v2);

        // PROPERTY: Compatibility ⟺ same major version
        if major1 == major2 {
            assert!(compatible, "same major version should be compatible");
        } else {
            assert!(
                !compatible,
                "different major version should be incompatible"
            );
        }

        // PROPERTY: Compatibility is symmetric
        assert_eq!(
            v1.is_compatible_with(v2),
            v2.is_compatible_with(v1),
            "compatibility must be symmetric"
        );

        // PROPERTY: Version is compatible with itself
        assert!(
            v1.is_compatible_with(v1),
            "version must be compatible with itself"
        );
    }

    /// Proof #65: Feature flag activation safety
    ///
    /// Verifies that features are only enabled when all replicas have the
    /// required version, ensuring safe feature rollout.
    ///
    /// Property: feature.is_enabled(cluster_version) ⟹ ∀ replica: version >= required_version
    #[kani::proof]
    #[kani::unwind(4)]
    fn proof_feature_flag_activation_safety() {
        // Generate bounded version
        let major: u16 = kani::any();
        let minor: u16 = kani::any();
        let patch: u16 = kani::any();
        kani::assume(major == 0); // Focus on major version 0
        kani::assume(minor <= 5);
        kani::assume(patch <= 5);

        let cluster_version = VersionInfo::new(major, minor, patch);

        // Test ClockSync feature (requires v0.3.0)
        let clock_sync_enabled = FeatureFlag::ClockSync.is_enabled(cluster_version);
        let clock_sync_required = FeatureFlag::ClockSync.required_version();

        // PROPERTY: Feature enabled ⟹ cluster_version >= required_version
        if clock_sync_enabled {
            assert!(
                cluster_version >= clock_sync_required,
                "enabled feature must meet version requirement"
            );
        }

        // PROPERTY: cluster_version >= required_version ⟹ Feature enabled
        if cluster_version >= clock_sync_required {
            assert!(clock_sync_enabled, "sufficient version must enable feature");
        }

        // Test ClusterReconfig feature (requires v0.4.0)
        let reconfig_enabled = FeatureFlag::ClusterReconfig.is_enabled(cluster_version);
        let reconfig_required = FeatureFlag::ClusterReconfig.required_version();

        if reconfig_enabled {
            assert!(
                cluster_version >= reconfig_required,
                "enabled feature must meet version requirement"
            );
        }

        if cluster_version >= reconfig_required {
            assert!(reconfig_enabled, "sufficient version must enable feature");
        }

        // PROPERTY: All features enabled at v0.4.0+
        if cluster_version >= VersionInfo::V0_4_0 {
            assert!(FeatureFlag::ClockSync.is_enabled(cluster_version));
            assert!(FeatureFlag::ClientSessions.is_enabled(cluster_version));
            assert!(FeatureFlag::RepairBudgets.is_enabled(cluster_version));
        }
    }

    /// Proof #66: Version ordering transitivity
    ///
    /// Verifies that version ordering is transitive, which is critical for
    /// correctly computing minimum versions.
    ///
    /// Property: v1 < v2 ∧ v2 < v3 ⟹ v1 < v3
    #[kani::proof]
    #[kani::unwind(4)]
    fn proof_version_ordering_transitivity() {
        let v1_major: u16 = kani::any();
        let v1_minor: u16 = kani::any();
        let v1_patch: u16 = kani::any();
        kani::assume(v1_major <= 1);
        kani::assume(v1_minor <= 5);
        kani::assume(v1_patch <= 5);

        let v2_major: u16 = kani::any();
        let v2_minor: u16 = kani::any();
        let v2_patch: u16 = kani::any();
        kani::assume(v2_major <= 1);
        kani::assume(v2_minor <= 5);
        kani::assume(v2_patch <= 5);

        let v3_major: u16 = kani::any();
        let v3_minor: u16 = kani::any();
        let v3_patch: u16 = kani::any();
        kani::assume(v3_major <= 1);
        kani::assume(v3_minor <= 5);
        kani::assume(v3_patch <= 5);

        let v1 = VersionInfo::new(v1_major, v1_minor, v1_patch);
        let v2 = VersionInfo::new(v2_major, v2_minor, v2_patch);
        let v3 = VersionInfo::new(v3_major, v3_minor, v3_patch);

        // PROPERTY: Transitivity of <
        if v1 < v2 && v2 < v3 {
            assert!(v1 < v3, "ordering must be transitive");
        }

        // PROPERTY: Transitivity of <=
        if v1 <= v2 && v2 <= v3 {
            assert!(v1 <= v3, "ordering must be transitive");
        }

        // PROPERTY: min is associative
        let min12_3 = v1.min(v2).min(v3);
        let min1_23 = v1.min(v2.min(v3));
        assert_eq!(min12_3, min1_23, "min must be associative");

        // PROPERTY: min is commutative
        assert_eq!(v1.min(v2), v2.min(v1), "min must be commutative");
    }

    /// Proof #67: Upgrade proposal validation
    ///
    /// Verifies that propose_upgrade() correctly validates upgrade requests,
    /// preventing invalid upgrades.
    ///
    /// Property: Invalid upgrades are rejected with appropriate error
    #[kani::proof]
    #[kani::unwind(3)]
    fn proof_upgrade_proposal_validation() {
        let current_major: u16 = kani::any();
        let current_minor: u16 = kani::any();
        kani::assume(current_major <= 1);
        kani::assume(current_minor <= 5);

        let target_major: u16 = kani::any();
        let target_minor: u16 = kani::any();
        kani::assume(target_major <= 2);
        kani::assume(target_minor <= 10);

        let current = VersionInfo::new(current_major, current_minor, 0);
        let target = VersionInfo::new(target_major, target_minor, 0);

        let mut state = UpgradeState::new(current);
        let result = state.propose_upgrade(target);

        // PROPERTY: Incompatible major version rejected
        if current_major != target_major {
            assert!(
                result.is_err(),
                "incompatible major version must be rejected"
            );
            if let Err(e) = result {
                assert_eq!(e, "incompatible major version");
            }
        }

        // PROPERTY: Downgrade rejected
        if target <= current {
            assert!(result.is_err(), "downgrade must be rejected");
        }

        // PROPERTY: Valid upgrade accepted
        if current_major == target_major && target > current {
            assert!(result.is_ok(), "valid upgrade must be accepted");
            assert_eq!(state.target_version, Some(target));
        }
    }
}
