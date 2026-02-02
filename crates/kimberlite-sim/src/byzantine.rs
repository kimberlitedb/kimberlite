//! Byzantine fault injection for adversarial testing.
//!
//! This module provides Byzantine behavior injection to test consensus safety
//! invariants under malicious or buggy replica conditions.
//!
//! ## Attack Vectors
//!
//! - **Message Corruption**: Send invalid or conflicting VSR messages
//! - **Metadata Manipulation**: Inflate commit numbers, mismatched op numbers
//! - **Log Tampering**: Truncate logs, insert conflicting entries
//! - **View Change Attacks**: Send malicious DoViewChange/StartView messages
//!
//! ## Usage
//!
//! ```ignore
//! let injector = ByzantineInjector::new()
//!     .with_corrupt_start_view_log(true)
//!     .with_inflate_commit_number(0.5);
//! ```

use crate::{MessageFieldMutation, MessageMutationRule, MessageTypeFilter, SimRng};
use kimberlite_vsr::{CommitNumber, OpNumber, ReplicaId};
use serde::{Deserialize, Serialize};

// ============================================================================
// Byzantine Injector Configuration
// ============================================================================

/// Byzantine fault injector for testing consensus safety.
#[derive(Debug, Clone)]
pub struct ByzantineInjector {
    /// Configuration for Byzantine behavior.
    config: ByzantineConfig,
    /// Target replica to make Byzantine (None = random selection).
    byzantine_replica: Option<ReplicaId>,
}

impl ByzantineInjector {
    /// Creates a new Byzantine injector with default configuration.
    pub fn new() -> Self {
        Self {
            config: ByzantineConfig::default(),
            byzantine_replica: None,
        }
    }

    /// Sets the replica to behave Byzantine.
    pub fn with_byzantine_replica(mut self, replica: ReplicaId) -> Self {
        self.byzantine_replica = Some(replica);
        self
    }

    /// Enables StartView log corruption.
    pub fn with_corrupt_start_view_log(mut self, enabled: bool) -> Self {
        self.config.corrupt_start_view_log = enabled;
        self
    }

    /// Enables Prepare entry corruption.
    pub fn with_corrupt_prepare_entry(mut self, enabled: bool) -> Self {
        self.config.corrupt_prepare_entry = enabled;
        self
    }

    /// Enables sending conflicting log entries.
    pub fn with_send_conflicting_entries(mut self, enabled: bool) -> Self {
        self.config.send_conflicting_entries = enabled;
        self
    }

    /// Sets probability of inflating commit numbers.
    pub fn with_inflate_commit_number(mut self, probability: f64) -> Self {
        self.config.inflate_commit_probability = probability;
        self
    }

    /// Sets commit number inflation factor.
    pub fn with_commit_inflation_factor(mut self, factor: u64) -> Self {
        self.config.commit_inflation_factor = factor;
        self
    }

    /// Enables op number mismatch attacks.
    pub fn with_op_number_mismatch(mut self, enabled: bool) -> Self {
        self.config.op_number_mismatch = enabled;
        self
    }

    /// Enables log tail truncation.
    pub fn with_truncate_log_tail(mut self, enabled: bool) -> Self {
        self.config.truncate_log_tail = enabled;
        self
    }

    /// Returns the configuration.
    pub fn config(&self) -> &ByzantineConfig {
        &self.config
    }

    /// Checks if a replica should behave Byzantine.
    pub fn is_byzantine(&self, replica: ReplicaId, rng: &mut SimRng) -> bool {
        if let Some(target) = self.byzantine_replica {
            target == replica
        } else {
            // Random selection with low probability
            rng.next_f64() < 0.2
        }
    }

    /// Should inflate commit number in this message?
    pub fn should_inflate_commit(&self, rng: &mut SimRng) -> bool {
        self.config.inflate_commit_probability > 0.0
            && rng.next_f64() < self.config.inflate_commit_probability
    }

    /// Calculate inflated commit number.
    pub fn inflate_commit(&self, original: CommitNumber) -> CommitNumber {
        let inflated = original.as_u64() + self.config.commit_inflation_factor;
        CommitNumber::new(OpNumber::new(inflated))
    }

    /// Builds message mutation rules from this injector's configuration.
    ///
    /// This converts the configuration into a set of MessageMutationRules
    /// that can be applied by the MessageMutator.
    pub fn build_mutation_rules(&self) -> Vec<MessageMutationRule> {
        let mut rules = Vec::new();

        // Rule: Inflate commit number in DoViewChange messages
        if self.config.inflate_commit_probability > 0.0 {
            rules.push(MessageMutationRule {
                target: MessageTypeFilter::DoViewChange,
                from_replica: self.byzantine_replica,
                to_replica: None,
                probability: self.config.inflate_commit_probability,
                mutation: MessageFieldMutation::InflateCommitNumber {
                    amount: self.config.commit_inflation_factor,
                },
                deliver: true,
            });

            // Also inflate in StartView messages
            rules.push(MessageMutationRule {
                target: MessageTypeFilter::StartView,
                from_replica: self.byzantine_replica,
                to_replica: None,
                probability: self.config.inflate_commit_probability,
                mutation: MessageFieldMutation::InflateCommitNumber {
                    amount: self.config.commit_inflation_factor,
                },
                deliver: true,
            });

            // And in Commit messages
            rules.push(MessageMutationRule {
                target: MessageTypeFilter::Commit,
                from_replica: self.byzantine_replica,
                to_replica: None,
                probability: self.config.inflate_commit_probability,
                mutation: MessageFieldMutation::InflateCommitNumber {
                    amount: self.config.commit_inflation_factor,
                },
                deliver: true,
            });
        }

        // Rule: Truncate log tail in DoViewChange/StartView
        if self.config.truncate_log_tail {
            rules.push(MessageMutationRule {
                target: MessageTypeFilter::DoViewChange,
                from_replica: self.byzantine_replica,
                to_replica: None,
                probability: 1.0,
                mutation: MessageFieldMutation::TruncateLogTail { max_entries: 1 },
                deliver: true,
            });

            rules.push(MessageMutationRule {
                target: MessageTypeFilter::StartView,
                from_replica: self.byzantine_replica,
                to_replica: None,
                probability: 1.0,
                mutation: MessageFieldMutation::TruncateLogTail { max_entries: 1 },
                deliver: true,
            });
        }

        // Rule: Send conflicting log entries
        if self.config.send_conflicting_entries {
            rules.push(MessageMutationRule {
                target: MessageTypeFilter::DoViewChange,
                from_replica: self.byzantine_replica,
                to_replica: None,
                probability: 1.0,
                mutation: MessageFieldMutation::ConflictingLogTail {
                    conflict_seed: 0xDEAD_BEEF,
                },
                deliver: true,
            });

            rules.push(MessageMutationRule {
                target: MessageTypeFilter::StartView,
                from_replica: self.byzantine_replica,
                to_replica: None,
                probability: 1.0,
                mutation: MessageFieldMutation::ConflictingLogTail {
                    conflict_seed: 0xDEAD_BEEF,
                },
                deliver: true,
            });
        }

        // Rule: Op number mismatch in Prepare messages
        if self.config.op_number_mismatch {
            rules.push(MessageMutationRule {
                target: MessageTypeFilter::Prepare,
                from_replica: self.byzantine_replica,
                to_replica: None,
                probability: 1.0,
                mutation: MessageFieldMutation::OpNumberMismatch { offset: 10 },
                deliver: true,
            });
        }

        rules
    }
}

impl Default for ByzantineInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Byzantine Configuration
// ============================================================================

/// Configuration for Byzantine fault injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByzantineConfig {
    /// Corrupt StartView message log entries.
    pub corrupt_start_view_log: bool,

    /// Corrupt Prepare message entries.
    pub corrupt_prepare_entry: bool,

    /// Send conflicting entries to different replicas.
    pub send_conflicting_entries: bool,

    /// Probability of inflating commit numbers (0.0 - 1.0).
    pub inflate_commit_probability: f64,

    /// Factor to inflate commit numbers by.
    pub commit_inflation_factor: u64,

    /// Send op_number that doesn't match entry metadata.
    pub op_number_mismatch: bool,

    /// Truncate log tail in StartView messages.
    pub truncate_log_tail: bool,
}

impl Default for ByzantineConfig {
    fn default() -> Self {
        Self {
            corrupt_start_view_log: false,
            corrupt_prepare_entry: false,
            send_conflicting_entries: false,
            inflate_commit_probability: 0.0,
            commit_inflation_factor: 100,
            op_number_mismatch: false,
            truncate_log_tail: false,
        }
    }
}

// ============================================================================
// Attack Patterns
// ============================================================================

/// Pre-configured Byzantine attack patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackPattern {
    /// Target Bug #1: View change log merge overwrites committed entries.
    ViewChangeMergeOverwrite,

    /// Target Bug #2: Commit number desynchronization.
    CommitNumberDesync,

    /// Target Bug #3: Inflated commit number in DoViewChange.
    InflatedCommitNumber,

    /// Target Bug #4: No entry metadata validation.
    InvalidEntryMetadata,

    /// Target Bug #5: View change log selection without validation.
    MaliciousViewChangeSelection,

    /// Target Bug #6: Leader selection race condition.
    LeaderSelectionRace,
}

impl AttackPattern {
    /// Returns a human-readable name for this attack pattern.
    pub fn name(&self) -> &'static str {
        match self {
            Self::ViewChangeMergeOverwrite => "View Change Merge Overwrite",
            Self::CommitNumberDesync => "Commit Number Desynchronization",
            Self::InflatedCommitNumber => "Inflated Commit Number",
            Self::InvalidEntryMetadata => "Invalid Entry Metadata",
            Self::MaliciousViewChangeSelection => "Malicious View Change Selection",
            Self::LeaderSelectionRace => "Leader Selection Race",
        }
    }

    /// Returns a description of what this attack targets.
    pub fn description(&self) -> &'static str {
        match self {
            Self::ViewChangeMergeOverwrite => {
                "Forces view change after commits, injects conflicting entries in StartView"
            }
            Self::CommitNumberDesync => "Sends StartView with high commit_number but truncated log",
            Self::InflatedCommitNumber => {
                "Byzantine replica claims impossibly high commit_number in DoViewChange"
            }
            Self::InvalidEntryMetadata => "Sends Prepare with mismatched entry metadata",
            Self::MaliciousViewChangeSelection => {
                "Sends DoViewChange with inconsistent log during view change"
            }
            Self::LeaderSelectionRace => "Creates asymmetric partition during leader selection",
        }
    }

    /// Returns the expected invariant violation.
    pub fn expected_violation(&self) -> &'static str {
        match self {
            Self::ViewChangeMergeOverwrite => "vsr_agreement",
            Self::CommitNumberDesync => "vsr_prefix_property",
            Self::InflatedCommitNumber => "vsr_durability",
            Self::InvalidEntryMetadata => "vsr_agreement",
            Self::MaliciousViewChangeSelection => "vsr_view_change_safety",
            Self::LeaderSelectionRace => "vsr_agreement",
        }
    }

    /// Returns the bounty value estimate in USD.
    pub fn bounty_value(&self) -> u32 {
        match self {
            Self::ViewChangeMergeOverwrite => 20_000,
            Self::CommitNumberDesync => 18_000,
            Self::InflatedCommitNumber => 10_000,
            Self::InvalidEntryMetadata => 3_000,
            Self::MaliciousViewChangeSelection => 10_000,
            Self::LeaderSelectionRace => 5_000,
        }
    }

    /// Creates a Byzantine injector configured for this attack pattern.
    pub fn injector(&self) -> ByzantineInjector {
        match self {
            Self::ViewChangeMergeOverwrite => ByzantineInjector::new()
                .with_corrupt_start_view_log(true)
                .with_send_conflicting_entries(true),

            Self::CommitNumberDesync => ByzantineInjector::new()
                .with_inflate_commit_number(1.0)
                .with_commit_inflation_factor(500)
                .with_truncate_log_tail(true),

            Self::InflatedCommitNumber => ByzantineInjector::new()
                .with_inflate_commit_number(1.0)
                .with_commit_inflation_factor(1000),

            Self::InvalidEntryMetadata => ByzantineInjector::new().with_op_number_mismatch(true),

            Self::MaliciousViewChangeSelection => ByzantineInjector::new()
                .with_corrupt_start_view_log(true)
                .with_truncate_log_tail(true),

            Self::LeaderSelectionRace => ByzantineInjector::new()
                .with_send_conflicting_entries(true)
                .with_inflate_commit_number(0.5),
        }
    }

    /// Returns all attack patterns.
    pub fn all() -> &'static [AttackPattern] {
        &[
            Self::ViewChangeMergeOverwrite,
            Self::CommitNumberDesync,
            Self::InflatedCommitNumber,
            Self::InvalidEntryMetadata,
            Self::MaliciousViewChangeSelection,
            Self::LeaderSelectionRace,
        ]
    }
}

// ============================================================================
// Byzantine Message Mutations
// ============================================================================

/// Mutations that can be applied to VSR messages for Byzantine testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageMutation {
    /// Replace log entry with conflicting entry.
    ConflictingEntry,
    /// Inflate commit number beyond actual committed ops.
    InflateCommitNumber,
    /// Truncate log tail to create gaps.
    TruncateLog,
    /// Mismatch op_number in message vs entry metadata.
    OpNumberMismatch,
}

impl MessageMutation {
    /// Returns a description of this mutation.
    pub fn description(&self) -> &'static str {
        match self {
            Self::ConflictingEntry => "Replace entry with different operation",
            Self::InflateCommitNumber => "Claim higher commit number than possible",
            Self::TruncateLog => "Truncate log to create gaps",
            Self::OpNumberMismatch => "Send op_number that doesn't match entry",
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byzantine_injector_creation() {
        let injector = ByzantineInjector::new()
            .with_corrupt_start_view_log(true)
            .with_inflate_commit_number(0.5);

        assert!(injector.config().corrupt_start_view_log);
        assert_eq!(injector.config().inflate_commit_probability, 0.5);
    }

    #[test]
    fn test_attack_patterns() {
        for pattern in AttackPattern::all() {
            assert!(!pattern.name().is_empty());
            assert!(!pattern.description().is_empty());
            assert!(!pattern.expected_violation().is_empty());
            assert!(pattern.bounty_value() > 0);
        }
    }

    #[test]
    fn test_commit_inflation() {
        let injector = ByzantineInjector::new()
            .with_inflate_commit_number(1.0)
            .with_commit_inflation_factor(500);

        let original = CommitNumber::new(OpNumber::new(50));
        let inflated = injector.inflate_commit(original);

        assert_eq!(inflated.as_u64(), 550);
    }

    #[test]
    fn test_should_inflate_commit() {
        let injector = ByzantineInjector::new().with_inflate_commit_number(1.0);

        let mut rng = SimRng::new(12345);

        // With probability 1.0, should always inflate
        assert!(injector.should_inflate_commit(&mut rng));
    }

    #[test]
    fn test_is_byzantine_with_target() {
        let target = ReplicaId::new(1);
        let injector = ByzantineInjector::new().with_byzantine_replica(target);

        let mut rng = SimRng::new(0);

        assert!(injector.is_byzantine(ReplicaId::new(1), &mut rng));
        assert!(!injector.is_byzantine(ReplicaId::new(0), &mut rng));
        assert!(!injector.is_byzantine(ReplicaId::new(2), &mut rng));
    }

    #[test]
    fn test_attack_pattern_injectors() {
        // Verify each attack pattern produces a valid injector
        for pattern in AttackPattern::all() {
            let injector = pattern.injector();
            let config = injector.config();

            // Each pattern should enable at least one attack vector
            let has_attack = config.corrupt_start_view_log
                || config.corrupt_prepare_entry
                || config.send_conflicting_entries
                || config.inflate_commit_probability > 0.0
                || config.op_number_mismatch
                || config.truncate_log_tail;

            assert!(has_attack, "Pattern {:?} has no attacks enabled", pattern);
        }
    }

    #[test]
    fn test_build_mutation_rules() {
        // Test with inflate commit number enabled
        let injector = ByzantineInjector::new()
            .with_inflate_commit_number(1.0)
            .with_commit_inflation_factor(500);

        let rules = injector.build_mutation_rules();

        // Should generate 3 rules (DoViewChange, StartView, Commit)
        assert_eq!(rules.len(), 3);

        // Verify all rules have correct probability
        for rule in &rules {
            assert_eq!(rule.probability, 1.0);
            assert!(rule.deliver);
        }
    }

    #[test]
    fn test_build_mutation_rules_truncate() {
        let injector = ByzantineInjector::new().with_truncate_log_tail(true);

        let rules = injector.build_mutation_rules();

        // Should generate 2 rules (DoViewChange, StartView)
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn test_build_mutation_rules_combined() {
        let injector = ByzantineInjector::new()
            .with_inflate_commit_number(0.5)
            .with_truncate_log_tail(true)
            .with_send_conflicting_entries(true)
            .with_op_number_mismatch(true);

        let rules = injector.build_mutation_rules();

        // Should generate multiple rules: 3 inflate + 2 truncate + 2 conflicting + 1 op_mismatch = 8
        assert_eq!(rules.len(), 8);
    }
}
