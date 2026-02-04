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
/// - **MINOR**: Backward-compatible functionality
/// - **PATCH**: Backward-compatible bug fixes
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct VersionInfo {
    /// Major version number.
    ///
    /// Breaking changes increment this. Different major versions are incompatible.
    pub major: u16,

    /// Minor version number.
    ///
    /// New features increment this. Minor versions are backward-compatible.
    pub minor: u16,

    /// Patch version number.
    ///
    /// Bug fixes increment this. Always backward-compatible.
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

    /// Kimberlite v0.3.0 (previous).
    pub const V0_3_0: Self = Self::new(0, 3, 0);

    /// Checks if this version is compatible with another.
    ///
    /// Compatibility rules:
    /// - Same major version required
    /// - Minor/patch versions can differ (backward-compatible)
    pub fn is_compatible_with(&self, other: &VersionInfo) -> bool {
        self.major == other.major
    }

    /// Returns the minimum of two versions.
    pub fn min(&self, other: &VersionInfo) -> VersionInfo {
        if self <= other {
            *self
        } else {
            *other
        }
    }

    /// Returns the maximum of two versions.
    pub fn max(&self, other: &VersionInfo) -> VersionInfo {
        if self >= other {
            *self
        } else {
            *other
        }
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
    /// Phase 1: Clock synchronization (v0.3.0+).
    ClockSync,

    /// Phase 1: Client sessions (v0.3.0+).
    ClientSessions,

    /// Phase 2: Repair budgets (v0.3.0+).
    RepairBudgets,

    /// Phase 2: EWMA repair selection (v0.3.0+).
    EwmaRepair,

    /// Phase 3: Background log scrubbing (v0.3.0+).
    LogScrubbing,

    /// Phase 4: Cluster reconfiguration (v0.4.0+).
    ClusterReconfig,

    /// Phase 4: Rolling upgrades (v0.4.0+).
    RollingUpgrades,

    /// Phase 4: Standby replicas (v0.4.0+).
    StandbyReplicas,
}

impl FeatureFlag {
    /// Returns the minimum version required for this feature.
    pub fn required_version(&self) -> VersionInfo {
        match self {
            Self::ClockSync => VersionInfo::V0_3_0,
            Self::ClientSessions => VersionInfo::V0_3_0,
            Self::RepairBudgets => VersionInfo::V0_3_0,
            Self::EwmaRepair => VersionInfo::V0_3_0,
            Self::LogScrubbing => VersionInfo::V0_3_0,
            Self::ClusterReconfig => VersionInfo::V0_4_0,
            Self::RollingUpgrades => VersionInfo::V0_4_0,
            Self::StandbyReplicas => VersionInfo::V0_4_0,
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
    /// # Safety
    ///
    /// Using the minimum version ensures backward compatibility:
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
        if !self.self_version.is_compatible_with(&target) {
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
        use FeatureFlag::*;

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
        self.replica_versions.values().all(|v| {
            self.self_version.is_compatible_with(v)
        })
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
        assert!(v030.is_compatible_with(&v040));
        assert!(v040.is_compatible_with(&v030));

        // Different major version = incompatible
        assert!(!v040.is_compatible_with(&v100));
        assert!(!v100.is_compatible_with(&v040));
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
        assert_eq!(
            FeatureFlag::ClockSync.required_version(),
            VersionInfo::V0_3_0
        );
        assert_eq!(
            FeatureFlag::ClusterReconfig.required_version(),
            VersionInfo::V0_4_0
        );
    }

    #[test]
    fn test_feature_flag_enabled() {
        let v030 = VersionInfo::V0_3_0;
        let v040 = VersionInfo::V0_4_0;

        // v0.3.0 features enabled at v0.3.0
        assert!(FeatureFlag::ClockSync.is_enabled(v030));
        assert!(FeatureFlag::ClientSessions.is_enabled(v030));

        // v0.4.0 features NOT enabled at v0.3.0
        assert!(!FeatureFlag::ClusterReconfig.is_enabled(v030));
        assert!(!FeatureFlag::RollingUpgrades.is_enabled(v030));

        // v0.4.0 features enabled at v0.4.0
        assert!(FeatureFlag::ClusterReconfig.is_enabled(v040));
        assert!(FeatureFlag::RollingUpgrades.is_enabled(v040));

        // v0.3.0 features still enabled at v0.4.0
        assert!(FeatureFlag::ClockSync.is_enabled(v040));
    }

    #[test]
    fn test_upgrade_state_cluster_version() {
        let mut state = UpgradeState::new(VersionInfo::V0_4_0);

        // Initially, cluster version = self version
        assert_eq!(state.cluster_version(), VersionInfo::V0_4_0);

        // Add a replica at lower version
        state.update_replica_version(ReplicaId::new(1), VersionInfo::V0_3_0);

        // Cluster version drops to minimum
        assert_eq!(state.cluster_version(), VersionInfo::V0_3_0);

        // Add another replica at higher version
        state.update_replica_version(ReplicaId::new(2), VersionInfo::new(0, 5, 0));

        // Cluster version still minimum
        assert_eq!(state.cluster_version(), VersionInfo::V0_3_0);
    }

    #[test]
    fn test_upgrade_state_max_version() {
        let mut state = UpgradeState::new(VersionInfo::V0_3_0);

        assert_eq!(state.max_version(), VersionInfo::V0_3_0);

        state.update_replica_version(ReplicaId::new(1), VersionInfo::V0_4_0);
        assert_eq!(state.max_version(), VersionInfo::V0_4_0);

        state.update_replica_version(ReplicaId::new(2), VersionInfo::new(0, 5, 0));
        assert_eq!(state.max_version(), VersionInfo::new(0, 5, 0));
    }

    #[test]
    fn test_propose_upgrade() {
        let mut state = UpgradeState::new(VersionInfo::V0_3_0);

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
        let mut state = UpgradeState::new(VersionInfo::V0_3_0);

        // Incompatible major version
        let result = state.propose_upgrade(VersionInfo::new(1, 0, 0));
        assert_eq!(result.unwrap_err(), "incompatible major version");
    }

    #[test]
    fn test_propose_upgrade_downgrade() {
        let mut state = UpgradeState::new(VersionInfo::V0_4_0);

        // Downgrade rejected (use rollback instead)
        let result = state.propose_upgrade(VersionInfo::V0_3_0);
        assert_eq!(result.unwrap_err(), "target version must be higher than cluster version");
    }

    #[test]
    fn test_upgrade_complete() {
        let mut state = UpgradeState::new(VersionInfo::V0_3_0);

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

        state.update_replica_version(ReplicaId::new(1), VersionInfo::V0_3_0);
        state.update_replica_version(ReplicaId::new(2), VersionInfo::V0_4_0);
        state.update_replica_version(ReplicaId::new(3), VersionInfo::V0_4_0);

        let dist = state.version_distribution();
        assert_eq!(dist.get(&VersionInfo::V0_3_0), Some(&1));
        assert_eq!(dist.get(&VersionInfo::V0_4_0), Some(&3)); // self + 2 others
    }

    #[test]
    fn test_enabled_features() {
        let state = UpgradeState::new(VersionInfo::V0_3_0);

        let features = state.enabled_features();
        assert!(features.contains(&FeatureFlag::ClockSync));
        assert!(features.contains(&FeatureFlag::ClientSessions));
        assert!(!features.contains(&FeatureFlag::ClusterReconfig));
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
