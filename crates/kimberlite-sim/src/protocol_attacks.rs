//! Protocol-aware Byzantine attack library for VSR testing.
//!
//! This module provides pre-built Byzantine attack patterns targeting specific
//! VSR protocol invariants. Each attack is designed to test a particular safety
//! or liveness property.
//!
//! ## Attack Categories
//!
//! - **Split-Brain**: Leader equivocation and view divergence
//! - **Safety Violations**: Commit before quorum, conflicting commits
//! - **Liveness Attacks**: View change blocking, message withholding
//! - **State Corruption**: Invalid checksums, log inconsistencies
//!
//! ## Usage
//!
//! ```ignore
//! let attack = ProtocolAttack::split_brain(vec![0, 1], vec![2, 3, 4]);
//! let mutator = attack.to_message_mutator();
//! ```

use crate::message_mutator::{MessageFieldMutation, MessageTypeFilter};
use kimberlite_vsr::ReplicaId;

// Re-export MessageMutationRule since we generate them
pub use crate::message_mutator::MessageMutationRule;

// ============================================================================
// Attack Patterns
// ============================================================================

/// Pre-defined protocol attack patterns.
#[derive(Debug, Clone)]
pub enum ProtocolAttack {
    /// Split-brain: Send different DoViewChange messages to different replica groups.
    ///
    /// Byzantine leader sends inconsistent view change messages to split the cluster.
    /// Group A sees commit_number=N, Group B sees commit_number=M (where M > N).
    SplitBrain {
        group_a: Vec<ReplicaId>,
        group_b: Vec<ReplicaId>,
        commit_diff: u64,
    },

    /// Malicious Leader: Commit ahead of quorum.
    ///
    /// Leader sends Commit messages before receiving PrepareOk from quorum.
    /// Tests that followers reject commits without proper PrepareOk trail.
    MaliciousLeaderEarlyCommit { ahead_by: u64 },

    /// Equivocation: Send different Prepare messages for same op_number.
    ///
    /// Byzantine leader sends conflicting Prepare messages to different replicas.
    /// Tests that followers detect and reject equivocation.
    PrepareEquivocation {
        target_op: u64,
        variant_seed: u64,
    },

    /// Replay Attack: Re-send old messages after view change.
    ///
    /// Byzantine node replays messages from previous view to confuse replicas.
    /// Tests that replicas reject stale view numbers.
    ReplayOldView { old_view: u64 },

    /// Invalid DVC: Conflicting log tails in DoViewChange.
    ///
    /// Send DoViewChange with log_tail that conflicts with previously committed entries.
    /// Tests that new leader validates log tails before accepting DVC.
    InvalidDvcConflictingTail { conflict_seed: u64 },

    /// Mismatched Checksums: Corrupt checksums in log entries.
    ///
    /// Send log entries with checksums that don't match the data.
    /// Tests that replicas validate checksums before accepting entries.
    CorruptChecksums { corruption_rate: f64 },

    /// View Change Blocking: Withhold DoViewChange from specific replicas.
    ///
    /// Byzantine node selectively drops DVC messages to delay view change.
    /// Tests liveness under partial Byzantine behavior.
    ViewChangeBlocking { blocked_replicas: Vec<ReplicaId> },

    /// Prepare Flooding: Send excessive Prepare messages to overwhelm replicas.
    ///
    /// Tests backpressure and resource exhaustion defenses.
    PrepareFlood { rate_multiplier: u32 },

    /// Commit Number Inflation: Gradually inflate commit_number over time.
    ///
    /// Start with correct commit_number, gradually increase inflation.
    /// Tests detection of subtle commit number inconsistencies.
    CommitInflationGradual {
        initial_amount: u64,
        increase_per_message: u64,
    },

    /// Selective Silence: Respond to some replicas but not others.
    ///
    /// Byzantine node creates asymmetric network partitions by ignoring
    /// messages from specific replicas.
    SelectiveSilence { ignored_replicas: Vec<ReplicaId> },
}

impl ProtocolAttack {
    /// Converts this attack into message mutation rules.
    pub fn to_mutation_rules(&self) -> Vec<MessageMutationRule> {
        match self {
            Self::SplitBrain {
                group_a,
                group_b,
                commit_diff,
            } => vec![MessageMutationRule {
                target: MessageTypeFilter::DoViewChange,
                from_replica: None,
                to_replica: None,
                mutation: MessageFieldMutation::Fork {
                    group_a: group_a.clone(),
                    mutation_a: Box::new(MessageFieldMutation::InflateCommitNumber { amount: 0 }),
                    group_b: group_b.clone(),
                    mutation_b: Box::new(MessageFieldMutation::InflateCommitNumber {
                        amount: *commit_diff,
                    }),
                },
                probability: 1.0,
                deliver: true,
            }],

            Self::MaliciousLeaderEarlyCommit { ahead_by } => vec![MessageMutationRule {
                target: MessageTypeFilter::Commit,
                from_replica: None,
                to_replica: None,
                mutation: MessageFieldMutation::InflateCommitNumber { amount: *ahead_by },
                probability: 1.0,
                deliver: true,
            }],

            Self::PrepareEquivocation {
                target_op: _,
                variant_seed,
            } => vec![MessageMutationRule {
                target: MessageTypeFilter::Prepare,
                from_replica: None,
                to_replica: None,
                mutation: MessageFieldMutation::Fork {
                    group_a: vec![ReplicaId::new(0), ReplicaId::new(1)],
                    mutation_a: Box::new(MessageFieldMutation::ConflictingLogTail {
                        conflict_seed: *variant_seed,
                    }),
                    group_b: vec![ReplicaId::new(2), ReplicaId::new(3), ReplicaId::new(4)],
                    mutation_b: Box::new(MessageFieldMutation::ConflictingLogTail {
                        conflict_seed: variant_seed.wrapping_add(1),
                    }),
                },
                probability: 1.0,
                deliver: true,
            }],

            Self::ReplayOldView { old_view: _ } => {
                // TODO: Implement view number manipulation
                // This requires adding ViewNumberMutation to MessageFieldMutation
                vec![]
            }

            Self::InvalidDvcConflictingTail { conflict_seed } => vec![MessageMutationRule {
                target: MessageTypeFilter::DoViewChange,
                from_replica: None,
                to_replica: None,
                mutation: MessageFieldMutation::ConflictingLogTail {
                    conflict_seed: *conflict_seed,
                },
                probability: 1.0,
                deliver: true,
            }],

            Self::CorruptChecksums { corruption_rate: _ } => {
                // TODO: Implement checksum corruption
                // This requires adding ChecksumCorruption to MessageFieldMutation
                vec![]
            }

            Self::ViewChangeBlocking { blocked_replicas: _ } => {
                // This is implemented via selective message dropping, not mutation
                // Would be handled by ByzantineReplicaWrapper's message intercept
                vec![]
            }

            Self::PrepareFlood { rate_multiplier: _ } => {
                // Flooding is a rate-based attack, not a mutation
                // Would be handled by ByzantineReplicaWrapper
                vec![]
            }

            Self::CommitInflationGradual {
                initial_amount,
                increase_per_message: _,
            } => vec![MessageMutationRule {
                target: MessageTypeFilter::Commit,
                from_replica: None,
                to_replica: None,
                mutation: MessageFieldMutation::InflateCommitNumber {
                    amount: *initial_amount,
                },
                probability: 1.0,
                deliver: true,
                // TODO: Implement adaptive increase behavior
            }],

            Self::SelectiveSilence { ignored_replicas: _ } => {
                // Selective silence is handled by message interception
                // Would be implemented in ByzantineReplicaWrapper
                vec![]
            }
        }
    }

    /// Creates a split-brain attack targeting two replica groups.
    pub fn split_brain(
        group_a: Vec<ReplicaId>,
        group_b: Vec<ReplicaId>,
        commit_diff: u64,
    ) -> Self {
        Self::SplitBrain {
            group_a,
            group_b,
            commit_diff,
        }
    }

    /// Creates a malicious leader attack that commits ahead of quorum.
    pub fn malicious_leader(ahead_by: u64) -> Self {
        Self::MaliciousLeaderEarlyCommit { ahead_by }
    }

    /// Creates an equivocation attack for a specific operation number.
    pub fn equivocation(target_op: u64, seed: u64) -> Self {
        Self::PrepareEquivocation {
            target_op,
            variant_seed: seed,
        }
    }

    /// Creates an invalid DVC attack with conflicting log tail.
    pub fn invalid_dvc(seed: u64) -> Self {
        Self::InvalidDvcConflictingTail {
            conflict_seed: seed,
        }
    }

    /// Creates a gradual commit inflation attack.
    pub fn gradual_inflation(initial: u64, increase: u64) -> Self {
        Self::CommitInflationGradual {
            initial_amount: initial,
            increase_per_message: increase,
        }
    }

    /// Returns a human-readable description of this attack.
    pub fn description(&self) -> &'static str {
        match self {
            Self::SplitBrain { .. } => "Split-brain: Fork DoViewChange to different replica groups",
            Self::MaliciousLeaderEarlyCommit { .. } => "Malicious leader: Commit ahead of PrepareOk quorum",
            Self::PrepareEquivocation { .. } => "Equivocation: Different Prepare messages for same op_number",
            Self::ReplayOldView { .. } => "Replay attack: Re-send old messages from previous view",
            Self::InvalidDvcConflictingTail { .. } => "Invalid DVC: Conflicting log tail in DoViewChange",
            Self::CorruptChecksums { .. } => "Corrupt checksums: Invalid checksums in log entries",
            Self::ViewChangeBlocking { .. } => "View change blocking: Withhold DVC from specific replicas",
            Self::PrepareFlood { .. } => "Prepare flooding: Overwhelm replicas with excessive Prepare messages",
            Self::CommitInflationGradual { .. } => "Gradual commit inflation: Slowly increase commit_number over time",
            Self::SelectiveSilence { .. } => "Selective silence: Ignore messages from specific replicas",
        }
    }
}

// ============================================================================
// Future Extensions (for reference)
// ============================================================================

/// Attack conditions (Future enhancement - not yet implemented).
///
/// These would allow conditional attack application based on protocol state.
#[allow(dead_code)]
#[derive(Debug, Clone)]
enum AttackCondition {
    OpNumberEquals(u64),
    ViewNumberEquals(u64),
}

/// Adaptive behavior (Future enhancement - not yet implemented).
///
/// These would allow attacks to evolve based on replica responses.
#[allow(dead_code)]
#[derive(Debug, Clone)]
enum AdaptiveBehavior {
    IncreaseInflation { increase_per_message: u64 },
}

// ============================================================================
// Attack Catalog
// ============================================================================

/// Pre-configured attacks for common test scenarios.
pub struct AttackCatalog;

impl AttackCatalog {
    /// Standard attack set for basic Byzantine testing.
    pub fn standard_suite() -> Vec<ProtocolAttack> {
        vec![
            ProtocolAttack::split_brain(
                vec![ReplicaId::new(0), ReplicaId::new(1)],
                vec![ReplicaId::new(2), ReplicaId::new(3), ReplicaId::new(4)],
                100,
            ),
            ProtocolAttack::malicious_leader(50),
            ProtocolAttack::equivocation(10, 42),
            ProtocolAttack::invalid_dvc(12345),
        ]
    }

    /// Aggressive attack set for stress testing.
    pub fn aggressive_suite() -> Vec<ProtocolAttack> {
        vec![
            ProtocolAttack::split_brain(
                vec![ReplicaId::new(0), ReplicaId::new(1)],
                vec![ReplicaId::new(2), ReplicaId::new(3), ReplicaId::new(4)],
                500,
            ),
            ProtocolAttack::malicious_leader(200),
            ProtocolAttack::gradual_inflation(10, 5),
            ProtocolAttack::PrepareEquivocation {
                target_op: 50,
                variant_seed: 999,
            },
            ProtocolAttack::InvalidDvcConflictingTail {
                conflict_seed: 777,
            },
        ]
    }

    /// Subtle attacks for detecting edge cases.
    pub fn subtle_suite() -> Vec<ProtocolAttack> {
        vec![
            ProtocolAttack::gradual_inflation(1, 1),
            ProtocolAttack::CommitInflationGradual {
                initial_amount: 0,
                increase_per_message: 1,
            },
        ]
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_brain_creates_rules() {
        let attack = ProtocolAttack::split_brain(
            vec![ReplicaId::new(0)],
            vec![ReplicaId::new(1)],
            100,
        );
        let rules = attack.to_mutation_rules();

        assert_eq!(rules.len(), 1);
        assert!(matches!(rules[0].target, MessageTypeFilter::DoViewChange));
    }

    #[test]
    fn malicious_leader_creates_commit_mutation() {
        let attack = ProtocolAttack::malicious_leader(50);
        let rules = attack.to_mutation_rules();

        assert_eq!(rules.len(), 1);
        assert!(matches!(rules[0].target, MessageTypeFilter::Commit));
    }

    #[test]
    fn attack_descriptions() {
        let attacks = AttackCatalog::standard_suite();

        for attack in attacks {
            let desc = attack.description();
            assert!(!desc.is_empty());
        }
    }

    #[test]
    fn standard_suite_has_multiple_attacks() {
        let suite = AttackCatalog::standard_suite();
        assert!(suite.len() >= 4);
    }
}
