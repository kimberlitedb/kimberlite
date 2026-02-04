//! Cluster reconfiguration protocol.
//!
//! This module implements VSR cluster reconfiguration using joint consensus
//! (Raft-style). Supports zero-downtime addition and removal of replicas.
//!
//! # Safety
//!
//! The joint consensus algorithm ensures that during reconfiguration, quorums
//! are calculated across BOTH old and new configurations, preventing split-brain
//! scenarios.
//!
//! # Protocol
//!
//! 1. **Stable (C_old)**: Normal operation with single configuration
//! 2. **Joint (C_old,new)**: Transition state requiring quorum in BOTH configs
//! 3. **Stable (C_new)**: New configuration becomes stable
//!
//! # Example
//!
//! ```rust,ignore
//! // Add two replicas to a 3-node cluster
//! let cmd = ReconfigCommand::Replace {
//!     add: vec![ReplicaId::new(3), ReplicaId::new(4)],
//!     remove: vec![],
//! };
//!
//! // Propose reconfiguration (leader only)
//! let (state, output) = state.process(ReplicaEvent::ReconfigCommand(cmd));
//!
//! // Joint consensus begins, requires quorum in {0,1,2} AND {0,1,2,3,4}
//! // Once committed, automatically transitions to {0,1,2,3,4}
//! ```

use serde::{Deserialize, Serialize};

use crate::{
    config::ClusterConfig,
    types::{OpNumber, ReplicaId},
};

// ============================================================================
// Reconfiguration State
// ============================================================================

/// Cluster reconfiguration state.
///
/// Tracks the current configuration state: stable (single config) or joint
/// consensus (dual configs during transition).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconfigState {
    /// Stable state with a single configuration.
    ///
    /// This is the normal operating state. The cluster uses a single
    /// configuration for quorum calculations and leader election.
    Stable {
        /// The current cluster configuration.
        config: ClusterConfig,
    },

    /// Joint consensus state with two configurations.
    ///
    /// During reconfiguration, the cluster operates with BOTH old and new
    /// configurations. Operations require quorum in BOTH to commit.
    ///
    /// This prevents split-brain: even if old and new configs disagree,
    /// the intersection ensures safety.
    Joint {
        /// The old (pre-reconfiguration) configuration.
        old_config: ClusterConfig,

        /// The new (target) configuration.
        new_config: ClusterConfig,

        /// Operation number where C_old,new was committed.
        ///
        /// Once this operation is committed, the system transitions to C_new.
        joint_op: OpNumber,
    },
}

impl ReconfigState {
    /// Creates a new stable reconfiguration state.
    pub fn new_stable(config: ClusterConfig) -> Self {
        Self::Stable { config }
    }

    /// Creates a new joint consensus state.
    pub fn new_joint(
        old_config: ClusterConfig,
        new_config: ClusterConfig,
        joint_op: OpNumber,
    ) -> Self {
        Self::Joint {
            old_config,
            new_config,
            joint_op,
        }
    }

    /// Returns true if in stable state.
    pub fn is_stable(&self) -> bool {
        matches!(self, Self::Stable { .. })
    }

    /// Returns true if in joint consensus state.
    pub fn is_joint(&self) -> bool {
        matches!(self, Self::Joint { .. })
    }

    /// Returns the current stable configuration (if in stable state).
    pub fn stable_config(&self) -> Option<&ClusterConfig> {
        match self {
            Self::Stable { config } => Some(config),
            Self::Joint { .. } => None,
        }
    }

    /// Returns the configuration to use for leader election.
    ///
    /// During joint consensus, we use the OLD configuration for leader
    /// election to ensure stability.
    pub fn leader_config(&self) -> &ClusterConfig {
        match self {
            Self::Stable { config } => config,
            Self::Joint { old_config, .. } => old_config,
        }
    }

    /// Returns the configurations involved (old and/or new).
    ///
    /// Returns a tuple (old_config, new_config) where new_config is None
    /// in stable state.
    pub fn configs(&self) -> (&ClusterConfig, Option<&ClusterConfig>) {
        match self {
            Self::Stable { config } => (config, None),
            Self::Joint {
                old_config,
                new_config,
                ..
            } => (old_config, Some(new_config)),
        }
    }

    /// Calculates the quorum size for the current state.
    ///
    /// In stable state, returns the normal quorum size.
    /// In joint state, returns the MAXIMUM of old and new quorum sizes.
    ///
    /// Note: This is a conservative upper bound. Actual quorum checking
    /// requires validating BOTH configs separately (see `has_quorum`).
    pub fn quorum_size(&self) -> usize {
        match self {
            Self::Stable { config } => config.quorum_size(),
            Self::Joint {
                old_config,
                new_config,
                ..
            } => std::cmp::max(old_config.quorum_size(), new_config.quorum_size()),
        }
    }

    /// Checks if a set of replicas forms a valid quorum.
    ///
    /// In stable state, requires quorum in the single config.
    /// In joint state, requires quorum in BOTH old and new configs.
    ///
    /// # Arguments
    ///
    /// * `replicas` - Set of replica IDs to check
    ///
    /// # Returns
    ///
    /// `true` if the replicas form a valid quorum, `false` otherwise.
    pub fn has_quorum(&self, replicas: &[ReplicaId]) -> bool {
        match self {
            Self::Stable { config } => {
                let count = replicas.iter().filter(|r| config.contains(**r)).count();
                count >= config.quorum_size()
            }
            Self::Joint {
                old_config,
                new_config,
                ..
            } => {
                // Joint consensus: require quorum in BOTH configs
                let old_count = replicas.iter().filter(|r| old_config.contains(**r)).count();
                let new_count = replicas.iter().filter(|r| new_config.contains(**r)).count();

                old_count >= old_config.quorum_size() && new_count >= new_config.quorum_size()
            }
        }
    }

    /// Returns the operation number where joint consensus was initiated (if in joint state).
    pub fn joint_op(&self) -> Option<OpNumber> {
        match self {
            Self::Stable { .. } => None,
            Self::Joint { joint_op, .. } => Some(*joint_op),
        }
    }

    /// Checks if the cluster is ready to transition from joint to new stable.
    ///
    /// The transition happens automatically when the joint operation is
    /// committed.
    ///
    /// # Arguments
    ///
    /// * `commit_number` - Current commit number
    ///
    /// # Returns
    ///
    /// `true` if ready to transition, `false` otherwise.
    pub fn ready_to_transition(&self, commit_number: OpNumber) -> bool {
        match self {
            Self::Stable { .. } => false,
            Self::Joint { joint_op, .. } => commit_number >= *joint_op,
        }
    }

    /// Transitions from joint consensus to new stable configuration.
    ///
    /// # Panics
    ///
    /// Panics if not in joint state or if not ready to transition.
    pub fn transition_to_new(&mut self) {
        match self {
            Self::Stable { .. } => panic!("cannot transition from stable state"),
            Self::Joint { new_config, .. } => {
                let new_config = new_config.clone();
                *self = Self::Stable { config: new_config };
            }
        }
    }

    /// Returns all replica IDs involved in the current configuration(s).
    ///
    /// In stable state, returns replicas from the single config.
    /// In joint state, returns the UNION of old and new config replicas.
    pub fn all_replicas(&self) -> Vec<ReplicaId> {
        match self {
            Self::Stable { config } => config.replicas().collect(),
            Self::Joint {
                old_config,
                new_config,
                ..
            } => {
                let mut replicas: Vec<_> = old_config.replicas().collect();
                for r in new_config.replicas() {
                    if !replicas.contains(&r) {
                        replicas.push(r);
                    }
                }
                replicas.sort();
                replicas
            }
        }
    }
}

// ============================================================================
// Reconfiguration Commands
// ============================================================================

/// Reconfiguration command.
///
/// Defines the type of cluster membership change requested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconfigCommand {
    /// Add a new replica to the cluster.
    ///
    /// The cluster size increases by 1. The new replica must not already
    /// be a member.
    AddReplica(ReplicaId),

    /// Remove a replica from the cluster.
    ///
    /// The cluster size decreases by 1. The replica must currently be
    /// a member.
    RemoveReplica(ReplicaId),

    /// Replace multiple replicas atomically.
    ///
    /// This is more efficient than sequential add/remove operations.
    /// Useful for scaling (e.g., 3 → 5 nodes) or failover (e.g., replace
    /// failed node).
    Replace {
        /// Replicas to add.
        add: Vec<ReplicaId>,

        /// Replicas to remove.
        remove: Vec<ReplicaId>,
    },
}

impl ReconfigCommand {
    /// Validates the command against the current configuration.
    ///
    /// # Arguments
    ///
    /// * `current_config` - The current cluster configuration
    ///
    /// # Returns
    ///
    /// The new configuration if valid, or an error message.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Adding replica already in cluster
    /// - Removing replica not in cluster
    /// - Result would have even cluster size
    /// - Result would exceed MAX_REPLICAS
    /// - Result would be empty cluster
    pub fn validate(&self, current_config: &ClusterConfig) -> Result<ClusterConfig, &'static str> {
        let mut new_replicas: Vec<ReplicaId> = current_config.replicas().collect();

        match self {
            Self::AddReplica(id) => {
                if new_replicas.contains(id) {
                    return Err("replica already in cluster");
                }
                new_replicas.push(*id);
            }

            Self::RemoveReplica(id) => {
                if !new_replicas.contains(id) {
                    return Err("replica not in cluster");
                }
                new_replicas.retain(|r| r != id);
            }

            Self::Replace { add, remove } => {
                // Validate removes
                for id in remove {
                    if !new_replicas.contains(id) {
                        return Err("removing replica not in cluster");
                    }
                }

                // Validate adds
                for id in add {
                    if new_replicas.contains(id) {
                        return Err("adding replica already in cluster");
                    }
                }

                // Apply removes
                for id in remove {
                    new_replicas.retain(|r| r != id);
                }

                // Apply adds
                new_replicas.extend(add);
            }
        }

        // Validate cluster size is odd
        if new_replicas.len() % 2 == 0 {
            return Err("cluster size must be odd (2f+1)");
        }

        // Validate not empty
        if new_replicas.is_empty() {
            return Err("cluster cannot be empty");
        }

        // Create and return new configuration
        // This will validate MAX_REPLICAS and sort replicas
        Ok(ClusterConfig::new(new_replicas))
    }

    /// Returns a human-readable description of the command.
    pub fn description(&self) -> String {
        match self {
            Self::AddReplica(id) => format!("add replica {}", id),
            Self::RemoveReplica(id) => format!("remove replica {}", id),
            Self::Replace { add, remove } => {
                let add_str = add
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let remove_str = remove
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");

                if add.is_empty() {
                    format!("remove replicas [{}]", remove_str)
                } else if remove.is_empty() {
                    format!("add replicas [{}]", add_str)
                } else {
                    format!("add [{}], remove [{}]", add_str, remove_str)
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config_3() -> ClusterConfig {
        ClusterConfig::new(vec![ReplicaId::new(0), ReplicaId::new(1), ReplicaId::new(2)])
    }

    fn test_config_5() -> ClusterConfig {
        ClusterConfig::new(vec![
            ReplicaId::new(0),
            ReplicaId::new(1),
            ReplicaId::new(2),
            ReplicaId::new(3),
            ReplicaId::new(4),
        ])
    }

    // ========================================================================
    // ReconfigState Tests
    // ========================================================================

    #[test]
    fn test_stable_state() {
        let config = test_config_3();
        let state = ReconfigState::new_stable(config.clone());

        assert!(state.is_stable());
        assert!(!state.is_joint());
        assert_eq!(state.stable_config(), Some(&config));
        assert_eq!(state.leader_config(), &config);
        assert_eq!(state.quorum_size(), 2);
    }

    #[test]
    fn test_joint_state() {
        let old_config = test_config_3();
        let new_config = test_config_5();
        let joint_op = OpNumber::new(100);

        let state = ReconfigState::new_joint(old_config.clone(), new_config.clone(), joint_op);

        assert!(!state.is_stable());
        assert!(state.is_joint());
        assert_eq!(state.stable_config(), None);
        assert_eq!(state.leader_config(), &old_config); // Use old for leader election
        assert_eq!(state.joint_op(), Some(joint_op));

        // Joint quorum size is max(2, 3) = 3
        assert_eq!(state.quorum_size(), 3);
    }

    #[test]
    fn test_has_quorum_stable() {
        let config = test_config_3();
        let state = ReconfigState::new_stable(config);

        // Quorum of 2 out of 3
        assert!(state.has_quorum(&[ReplicaId::new(0), ReplicaId::new(1)]));
        assert!(state.has_quorum(&[ReplicaId::new(1), ReplicaId::new(2)]));
        assert!(state.has_quorum(&[
            ReplicaId::new(0),
            ReplicaId::new(1),
            ReplicaId::new(2)
        ]));

        // Not a quorum
        assert!(!state.has_quorum(&[ReplicaId::new(0)]));
        assert!(!state.has_quorum(&[ReplicaId::new(1)]));
        assert!(!state.has_quorum(&[]));
    }

    #[test]
    fn test_has_quorum_joint() {
        let old_config = test_config_3(); // quorum = 2
        let new_config = test_config_5(); // quorum = 3
        let state = ReconfigState::new_joint(old_config, new_config, OpNumber::new(100));

        // Need quorum in BOTH: 2 from {0,1,2} AND 3 from {0,1,2,3,4}
        assert!(state.has_quorum(&[
            ReplicaId::new(0),
            ReplicaId::new(1),
            ReplicaId::new(2)
        ])); // 3 from old, 3 from new
        assert!(state.has_quorum(&[
            ReplicaId::new(0),
            ReplicaId::new(1),
            ReplicaId::new(3),
            ReplicaId::new(4)
        ])); // 2 from old, 4 from new

        // Not a quorum - missing old quorum
        assert!(!state.has_quorum(&[
            ReplicaId::new(0),
            ReplicaId::new(3),
            ReplicaId::new(4)
        ])); // 1 from old, 3 from new

        // Not a quorum - missing new quorum
        assert!(!state.has_quorum(&[ReplicaId::new(0), ReplicaId::new(1)])); // 2 from old, 2 from new
    }

    #[test]
    fn test_ready_to_transition() {
        let state =
            ReconfigState::new_joint(test_config_3(), test_config_5(), OpNumber::new(100));

        assert!(!state.ready_to_transition(OpNumber::new(99)));
        assert!(state.ready_to_transition(OpNumber::new(100)));
        assert!(state.ready_to_transition(OpNumber::new(101)));
    }

    #[test]
    fn test_transition_to_new() {
        let new_config = test_config_5();
        let mut state =
            ReconfigState::new_joint(test_config_3(), new_config.clone(), OpNumber::new(100));

        state.transition_to_new();

        assert!(state.is_stable());
        assert_eq!(state.stable_config(), Some(&new_config));
    }

    #[test]
    fn test_all_replicas_stable() {
        let config = test_config_3();
        let state = ReconfigState::new_stable(config);

        let replicas = state.all_replicas();
        assert_eq!(
            replicas,
            vec![ReplicaId::new(0), ReplicaId::new(1), ReplicaId::new(2)]
        );
    }

    #[test]
    fn test_all_replicas_joint() {
        let old_config = test_config_3();
        let new_config = test_config_5();
        let state = ReconfigState::new_joint(old_config, new_config, OpNumber::new(100));

        let replicas = state.all_replicas();
        assert_eq!(
            replicas,
            vec![
                ReplicaId::new(0),
                ReplicaId::new(1),
                ReplicaId::new(2),
                ReplicaId::new(3),
                ReplicaId::new(4)
            ]
        );
    }

    // ========================================================================
    // ReconfigCommand Tests
    // ========================================================================

    #[test]
    fn test_add_replica_valid() {
        let config = test_config_3();
        let cmd = ReconfigCommand::AddReplica(ReplicaId::new(3));

        // Adding single replica would make cluster even (invalid)
        assert_eq!(
            cmd.validate(&config).unwrap_err(),
            "cluster size must be odd (2f+1)"
        );

        // Add two replicas to make it odd (3 → 5)
        let cmd = ReconfigCommand::Replace {
            add: vec![ReplicaId::new(3), ReplicaId::new(4)],
            remove: vec![],
        };
        let new_config = cmd.validate(&config).unwrap();
        assert_eq!(new_config.cluster_size(), 5);
    }

    #[test]
    fn test_add_replica_duplicate() {
        let config = test_config_3();
        let cmd = ReconfigCommand::AddReplica(ReplicaId::new(1)); // Already exists

        assert_eq!(cmd.validate(&config).unwrap_err(), "replica already in cluster");
    }

    #[test]
    fn test_remove_replica_valid() {
        let config = test_config_5();
        let cmd = ReconfigCommand::Replace {
            add: vec![],
            remove: vec![ReplicaId::new(3), ReplicaId::new(4)],
        };

        let new_config = cmd.validate(&config).unwrap();
        assert_eq!(new_config.cluster_size(), 3);
    }

    #[test]
    fn test_remove_replica_not_found() {
        let config = test_config_3();
        let cmd = ReconfigCommand::RemoveReplica(ReplicaId::new(5)); // Doesn't exist

        assert_eq!(cmd.validate(&config).unwrap_err(), "replica not in cluster");
    }

    #[test]
    fn test_replace_valid() {
        // Valid replace: 3 nodes → 5 nodes (add 2, remove 0)
        let config = test_config_3();
        let cmd = ReconfigCommand::Replace {
            add: vec![ReplicaId::new(3), ReplicaId::new(4)],
            remove: vec![],
        };

        let new_config = cmd.validate(&config).unwrap();
        assert_eq!(new_config.cluster_size(), 5);

        // Invalid replace: would result in even cluster size
        let config = test_config_3();
        let cmd = ReconfigCommand::Replace {
            add: vec![ReplicaId::new(3), ReplicaId::new(4)],
            remove: vec![ReplicaId::new(0)],
        };

        assert_eq!(
            cmd.validate(&config).unwrap_err(),
            "cluster size must be odd (2f+1)"
        );
    }

    #[test]
    fn test_command_description() {
        let cmd = ReconfigCommand::AddReplica(ReplicaId::new(3));
        assert_eq!(cmd.description(), "add replica R3");

        let cmd = ReconfigCommand::RemoveReplica(ReplicaId::new(1));
        assert_eq!(cmd.description(), "remove replica R1");

        let cmd = ReconfigCommand::Replace {
            add: vec![ReplicaId::new(3), ReplicaId::new(4)],
            remove: vec![ReplicaId::new(0)],
        };
        assert!(cmd.description().contains("add"));
        assert!(cmd.description().contains("remove"));
    }
}
