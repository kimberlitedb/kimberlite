//! Protocol-level message mutation for Byzantine testing.
//!
//! This module provides message interception and mutation capabilities
//! to test VSR protocol handlers under Byzantine conditions. Unlike the
//! state-corruption approach, this mutates messages AFTER they're created,
//! ensuring that protocol validation code is actually tested.
//!
//! ## Architecture
//!
//! ```text
//! VSR Replica → Message → [Mutator] → Network → Destination
//!                             ↑
//!                      Mutation Rules
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! let mutator = MessageMutator::new(vec![
//!     MessageMutationRule {
//!         target: MessageTypeFilter::DoViewChange,
//!         mutation: MessageFieldMutation::InflateCommitNumber { amount: 500 },
//!         probability: 0.5,
//!         ..Default::default()
//!     },
//! ]);
//!
//! if let Some(mutated_msg) = mutator.apply(&message, &mut rng) {
//!     // Send mutated message
//! }
//! ```

use crate::SimRng;
use kimberlite_vsr::{
    Commit, CommitNumber, DoViewChange, LogEntry, Message, MessagePayload, OpNumber, Prepare,
    PrepareOk, ReplicaId, StartView, ViewNumber,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Message Type Filtering
// ============================================================================

/// Filter for matching message types to apply mutations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageTypeFilter {
    /// Match any message type.
    Any,
    /// Match Prepare messages.
    Prepare,
    /// Match PrepareOk messages.
    PrepareOk,
    /// Match Commit messages.
    Commit,
    /// Match DoViewChange messages.
    DoViewChange,
    /// Match StartView messages.
    StartView,
    /// Match RepairRequest messages.
    RepairRequest,
    /// Match RepairResponse messages.
    RepairResponse,
    /// Match Heartbeat messages.
    Heartbeat,
}

impl MessageTypeFilter {
    /// Checks if a message matches this filter.
    pub fn matches(&self, message: &Message) -> bool {
        match (self, &message.payload) {
            (Self::Any, _) => true,
            (Self::Prepare, MessagePayload::Prepare(_)) => true,
            (Self::PrepareOk, MessagePayload::PrepareOk(_)) => true,
            (Self::Commit, MessagePayload::Commit(_)) => true,
            (Self::DoViewChange, MessagePayload::DoViewChange(_)) => true,
            (Self::StartView, MessagePayload::StartView(_)) => true,
            (Self::RepairRequest, MessagePayload::RepairRequest(_)) => true,
            (Self::RepairResponse, MessagePayload::RepairResponse(_)) => true,
            (Self::Heartbeat, MessagePayload::Heartbeat(_)) => true,
            _ => false,
        }
    }
}

// ============================================================================
// Field Mutations
// ============================================================================

/// Mutations that can be applied to message fields.
#[derive(Debug, Clone)]
pub enum MessageFieldMutation {
    /// Inflate commit_number by a fixed amount.
    InflateCommitNumber { amount: u64 },

    /// Inflate commit_number by a relative factor (e.g., 1.5 = 150% of original).
    InflateCommitNumberRelative { factor: f64 },

    /// Truncate log_tail to at most N entries.
    TruncateLogTail { max_entries: usize },

    /// Replace log_tail with conflicting entries (different hash chain).
    ConflictingLogTail { conflict_seed: u64 },

    /// Offset op_number by a delta (can be negative).
    OpNumberMismatch { offset: i64 },

    /// Send different mutations to different replica groups (asymmetric attack).
    Fork {
        group_a: Vec<ReplicaId>,
        mutation_a: Box<MessageFieldMutation>,
        group_b: Vec<ReplicaId>,
        mutation_b: Box<MessageFieldMutation>,
    },

    /// Apply multiple mutations in sequence.
    Composite(Vec<MessageFieldMutation>),

    /// Decrement view_number by a fixed amount (for replay attacks).
    ///
    /// **AUDIT-2026-03 H-1:** Byzantine attack pattern - replay old messages from previous view.
    DecrementViewNumber { amount: u64 },

    /// Corrupt message checksums with probability.
    ///
    /// **AUDIT-2026-03 H-1:** Byzantine attack pattern - send invalid checksums to test validation.
    CorruptChecksum { corruption_seed: u64 },

    /// Duplicate this message N times (for flooding attacks).
    ///
    /// **AUDIT-2026-03 H-1:** Byzantine attack pattern - overwhelm replicas with duplicate messages.
    DuplicateMessage { count: u32 },
}

impl MessageFieldMutation {
    /// Applies this mutation to a message.
    ///
    /// Returns a mutated copy of the message, or None if the mutation doesn't apply.
    pub fn apply(&self, message: &Message, to: ReplicaId, rng: &mut SimRng) -> Option<Message> {
        match self {
            Self::InflateCommitNumber { amount } => {
                self.apply_inflate_commit_fixed(message, *amount)
            }
            Self::InflateCommitNumberRelative { factor } => {
                self.apply_inflate_commit_relative(message, *factor)
            }
            Self::TruncateLogTail { max_entries } => {
                self.apply_truncate_log_tail(message, *max_entries)
            }
            Self::ConflictingLogTail { conflict_seed } => {
                self.apply_conflicting_log_tail(message, *conflict_seed, rng)
            }
            Self::OpNumberMismatch { offset } => self.apply_op_number_mismatch(message, *offset),
            Self::Fork {
                group_a,
                mutation_a,
                group_b,
                mutation_b,
            } => {
                // Choose which mutation based on destination replica
                if group_a.contains(&to) {
                    mutation_a.apply(message, to, rng)
                } else if group_b.contains(&to) {
                    mutation_b.apply(message, to, rng)
                } else {
                    // Replica not in either group - no mutation
                    None
                }
            }
            Self::Composite(mutations) => {
                // Apply mutations in sequence
                let mut current = message.clone();
                for mutation in mutations {
                    if let Some(mutated) = mutation.apply(&current, to, rng) {
                        current = mutated;
                    }
                }
                Some(current)
            }

            Self::DecrementViewNumber { amount } => {
                self.apply_decrement_view_number(message, *amount)
            }

            Self::CorruptChecksum { corruption_seed } => {
                self.apply_corrupt_checksum(message, *corruption_seed, rng)
            }

            Self::DuplicateMessage { count: _ } => {
                // Duplication is handled at the message delivery layer, not mutation
                // Return the original message unchanged
                Some(message.clone())
            }
        }
    }

    /// Inflates commit_number by a fixed amount.
    fn apply_inflate_commit_fixed(&self, message: &Message, amount: u64) -> Option<Message> {
        let mut mutated = message.clone();
        match &mut mutated.payload {
            MessagePayload::DoViewChange(dvc) => {
                let new_commit =
                    CommitNumber::new(OpNumber::new(dvc.commit_number.as_u64() + amount));
                *dvc = DoViewChange {
                    commit_number: new_commit,
                    ..dvc.clone()
                };
                Some(mutated)
            }
            MessagePayload::StartView(sv) => {
                let new_commit =
                    CommitNumber::new(OpNumber::new(sv.commit_number.as_u64() + amount));
                *sv = StartView {
                    commit_number: new_commit,
                    ..sv.clone()
                };
                Some(mutated)
            }
            MessagePayload::Commit(commit) => {
                let new_commit =
                    CommitNumber::new(OpNumber::new(commit.commit_number.as_u64() + amount));
                let mut commit_mut = commit.clone();
                commit_mut.commit_number = new_commit;
                mutated.payload = MessagePayload::Commit(commit_mut);
                Some(mutated)
            }
            _ => None, // Mutation doesn't apply to this message type
        }
    }

    /// Inflates commit_number by a relative factor.
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation
    )]
    fn apply_inflate_commit_relative(&self, message: &Message, factor: f64) -> Option<Message> {
        let mut mutated = message.clone();
        match &mut mutated.payload {
            MessagePayload::DoViewChange(dvc) => {
                let inflated = (dvc.commit_number.as_u64() as f64 * factor) as u64;
                let new_commit = CommitNumber::new(OpNumber::new(inflated));
                *dvc = DoViewChange {
                    commit_number: new_commit,
                    ..dvc.clone()
                };
                Some(mutated)
            }
            MessagePayload::StartView(sv) => {
                let inflated = (sv.commit_number.as_u64() as f64 * factor) as u64;
                let new_commit = CommitNumber::new(OpNumber::new(inflated));
                *sv = StartView {
                    commit_number: new_commit,
                    ..sv.clone()
                };
                Some(mutated)
            }
            MessagePayload::Commit(commit) => {
                let inflated = (commit.commit_number.as_u64() as f64 * factor) as u64;
                let new_commit = CommitNumber::new(OpNumber::new(inflated));
                let mut commit_mut = commit.clone();
                commit_mut.commit_number = new_commit;
                mutated.payload = MessagePayload::Commit(commit_mut);
                Some(mutated)
            }
            _ => None,
        }
    }

    /// Truncates log_tail to at most max_entries.
    fn apply_truncate_log_tail(&self, message: &Message, max_entries: usize) -> Option<Message> {
        let mut mutated = message.clone();
        match &mut mutated.payload {
            MessagePayload::DoViewChange(dvc) => {
                if dvc.log_tail.len() > max_entries {
                    let truncated = dvc.log_tail[..max_entries].to_vec();
                    *dvc = DoViewChange {
                        log_tail: truncated,
                        ..dvc.clone()
                    };
                    Some(mutated)
                } else {
                    None
                }
            }
            MessagePayload::StartView(sv) => {
                if sv.log_tail.len() > max_entries {
                    let truncated = sv.log_tail[..max_entries].to_vec();
                    *sv = StartView {
                        log_tail: truncated,
                        ..sv.clone()
                    };
                    Some(mutated)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Replaces log_tail with conflicting entries.
    fn apply_conflicting_log_tail(
        &self,
        message: &Message,
        conflict_seed: u64,
        _rng: &mut SimRng,
    ) -> Option<Message> {
        let mut mutated = message.clone();

        // Use conflict_seed to create deterministic but different entries
        let mut conflict_rng = SimRng::new(conflict_seed);

        match &mut mutated.payload {
            MessagePayload::DoViewChange(dvc) => {
                if dvc.log_tail.is_empty() {
                    return None;
                }

                // Generate conflicting log entries by mutating checksum
                // Keep op_number, view, and command the same, but corrupt the checksum
                let conflicting_tail: Vec<LogEntry> = dvc
                    .log_tail
                    .iter()
                    .map(|entry| {
                        // Generate a different checksum to simulate conflicting entry
                        let corrupted_checksum = conflict_rng.next_u64() as u32;

                        LogEntry {
                            op_number: entry.op_number,
                            view: entry.view,
                            command: entry.command.clone(),
                            idempotency_id: entry.idempotency_id.clone(),
                            client_id: entry.client_id,
                            request_number: entry.request_number,
                            checksum: corrupted_checksum,
                        }
                    })
                    .collect();

                *dvc = DoViewChange {
                    log_tail: conflicting_tail,
                    ..dvc.clone()
                };
                Some(mutated)
            }
            MessagePayload::StartView(sv) => {
                if sv.log_tail.is_empty() {
                    return None;
                }

                let conflicting_tail: Vec<LogEntry> = sv
                    .log_tail
                    .iter()
                    .map(|entry| {
                        // Generate a different checksum to simulate conflicting entry
                        let corrupted_checksum = conflict_rng.next_u64() as u32;

                        LogEntry {
                            op_number: entry.op_number,
                            view: entry.view,
                            command: entry.command.clone(),
                            idempotency_id: entry.idempotency_id.clone(),
                            client_id: entry.client_id,
                            request_number: entry.request_number,
                            checksum: corrupted_checksum,
                        }
                    })
                    .collect();

                *sv = StartView {
                    log_tail: conflicting_tail,
                    ..sv.clone()
                };
                Some(mutated)
            }
            _ => None,
        }
    }

    /// Applies op_number mismatch (offset the op_number).
    fn apply_op_number_mismatch(&self, message: &Message, offset: i64) -> Option<Message> {
        let mut mutated = message.clone();

        match &mut mutated.payload {
            MessagePayload::Prepare(prepare) => {
                // Offset the op_number in the Prepare message
                let current = prepare.op_number.as_u64() as i64;
                let new_op = (current + offset).max(0) as u64;
                let new_op_number = OpNumber::new(new_op);

                *prepare = Prepare {
                    op_number: new_op_number,
                    ..prepare.clone()
                };
                Some(mutated)
            }
            _ => None,
        }
    }

    /// Decrements view_number by a fixed amount (replay attack).
    ///
    /// **AUDIT-2026-03 H-1:** Byzantine attack - replay old messages from previous view.
    fn apply_decrement_view_number(&self, message: &Message, amount: u64) -> Option<Message> {
        let mut mutated = message.clone();

        match &mut mutated.payload {
            MessagePayload::Prepare(prepare) => {
                let current_view = prepare.view.as_u64();
                let new_view = current_view.saturating_sub(amount);
                *prepare = Prepare {
                    view: ViewNumber::new(new_view),
                    ..prepare.clone()
                };
                Some(mutated)
            }
            MessagePayload::PrepareOk(prepare_ok) => {
                let current_view = prepare_ok.view.as_u64();
                let new_view = current_view.saturating_sub(amount);
                *prepare_ok = PrepareOk {
                    view: ViewNumber::new(new_view),
                    ..prepare_ok.clone()
                };
                Some(mutated)
            }
            MessagePayload::DoViewChange(dvc) => {
                let current_view = dvc.view.as_u64();
                let new_view = current_view.saturating_sub(amount);
                *dvc = DoViewChange {
                    view: ViewNumber::new(new_view),
                    ..dvc.clone()
                };
                Some(mutated)
            }
            MessagePayload::StartView(sv) => {
                let current_view = sv.view.as_u64();
                let new_view = current_view.saturating_sub(amount);
                *sv = StartView {
                    view: ViewNumber::new(new_view),
                    ..sv.clone()
                };
                Some(mutated)
            }
            MessagePayload::Commit(commit) => {
                let current_view = commit.view.as_u64();
                let new_view = current_view.saturating_sub(amount);
                *commit = Commit {
                    view: ViewNumber::new(new_view),
                    ..commit.clone()
                };
                Some(mutated)
            }
            _ => None,
        }
    }

    /// Corrupts checksums in log entries.
    ///
    /// **AUDIT-2026-03 H-1:** Byzantine attack - send invalid checksums to test validation.
    fn apply_corrupt_checksum(&self, message: &Message, seed: u64, rng: &mut SimRng) -> Option<Message> {
        let mut mutated = message.clone();
        let mut corruption_rng = SimRng::new(seed.wrapping_add(rng.next_u64()));

        match &mut mutated.payload {
            MessagePayload::StartView(sv) => {
                // Corrupt checksums in log_tail
                let corrupted_tail: Vec<LogEntry> = sv
                    .log_tail
                    .iter()
                    .map(|entry| {
                        // Generate a random corrupted checksum
                        let corrupted_checksum = corruption_rng.next_u64() as u32;
                        LogEntry {
                            checksum: corrupted_checksum,
                            ..entry.clone()
                        }
                    })
                    .collect();

                *sv = StartView {
                    log_tail: corrupted_tail,
                    ..sv.clone()
                };
                Some(mutated)
            }
            MessagePayload::DoViewChange(dvc) => {
                // Corrupt checksums in log_tail
                let corrupted_tail: Vec<LogEntry> = dvc
                    .log_tail
                    .iter()
                    .map(|entry| {
                        let corrupted_checksum = corruption_rng.next_u64() as u32;
                        LogEntry {
                            checksum: corrupted_checksum,
                            ..entry.clone()
                        }
                    })
                    .collect();

                *dvc = DoViewChange {
                    log_tail: corrupted_tail,
                    ..dvc.clone()
                };
                Some(mutated)
            }
            MessagePayload::Prepare(prepare) => {
                // Corrupt checksum in the log entry within Prepare message
                let mut corrupted_entry = prepare.entry.clone();
                corrupted_entry.checksum = corruption_rng.next_u64() as u32;
                *prepare = Prepare {
                    entry: corrupted_entry,
                    ..prepare.clone()
                };
                Some(mutated)
            }
            _ => None,
        }
    }
}

// ============================================================================
// Mutation Rules
// ============================================================================

/// A rule that specifies when and how to mutate messages.
#[derive(Debug, Clone)]
pub struct MessageMutationRule {
    /// Which message types to target.
    pub target: MessageTypeFilter,

    /// Source replica filter (None = any source).
    pub from_replica: Option<ReplicaId>,

    /// Destination replica filter (None = any destination).
    pub to_replica: Option<ReplicaId>,

    /// Probability of applying this mutation (0.0 - 1.0).
    pub probability: f64,

    /// The mutation to apply.
    pub mutation: MessageFieldMutation,

    /// Whether to deliver the message after mutation (false = drop it).
    pub deliver: bool,
}

impl MessageMutationRule {
    /// Checks if this rule applies to a message.
    pub fn matches(&self, message: &Message, to: ReplicaId) -> bool {
        // Check message type
        if !self.target.matches(message) {
            return false;
        }

        // Check source filter
        if let Some(from) = self.from_replica {
            if message.from != from {
                return false;
            }
        }

        // Check destination filter
        if let Some(to_filter) = self.to_replica {
            if to != to_filter {
                return false;
            }
        }

        true
    }

    /// Attempts to apply this rule to a message.
    ///
    /// Returns Some(mutated_message) if the rule matched and probability check passed.
    pub fn try_apply(&self, message: &Message, to: ReplicaId, rng: &mut SimRng) -> Option<Message> {
        if !self.matches(message, to) {
            return None;
        }

        // Probability check
        if rng.next_f64() >= self.probability {
            return None;
        }

        // Apply mutation
        self.mutation.apply(message, to, rng)
    }
}

// ============================================================================
// Message Mutator
// ============================================================================

/// The message mutator applies Byzantine mutations to VSR messages.
#[derive(Debug)]
pub struct MessageMutator {
    /// Mutation rules to apply.
    rules: Vec<MessageMutationRule>,
    /// Statistics for tracking mutations.
    stats: MessageMutationStats,
}

impl MessageMutator {
    /// Creates a new message mutator with the given rules.
    pub fn new(rules: Vec<MessageMutationRule>) -> Self {
        Self {
            rules,
            stats: MessageMutationStats::default(),
        }
    }

    /// Applies mutations to a message based on the configured rules.
    ///
    /// Returns the mutated message if any rule applied, otherwise None.
    pub fn apply(&mut self, message: &Message, to: ReplicaId, rng: &mut SimRng) -> Option<Message> {
        let mut current = message.clone();
        let mut any_applied = false;

        for rule in &self.rules {
            if let Some(mutated) = rule.try_apply(&current, to, rng) {
                current = mutated;
                any_applied = true;
                self.stats.mutations_applied += 1;

                // Track by message type
                match &current.payload {
                    MessagePayload::DoViewChange(_) => {
                        self.stats.do_view_change_mutations += 1;
                    }
                    MessagePayload::StartView(_) => {
                        self.stats.start_view_mutations += 1;
                    }
                    MessagePayload::Prepare(_) => {
                        self.stats.prepare_mutations += 1;
                    }
                    MessagePayload::Commit(_) => {
                        self.stats.commit_mutations += 1;
                    }
                    _ => {}
                }

                // If rule says not to deliver, mark as dropped
                if !rule.deliver {
                    self.stats.messages_dropped += 1;
                    return None; // Don't deliver
                }
            }
        }

        self.stats.messages_processed += 1;

        if any_applied { Some(current) } else { None }
    }

    /// Returns the mutation statistics.
    pub fn stats(&self) -> &MessageMutationStats {
        &self.stats
    }

    /// Resets the mutation statistics.
    pub fn reset_stats(&mut self) {
        self.stats = MessageMutationStats::default();
    }
}

// ============================================================================
// Statistics
// ============================================================================

/// Statistics for message mutation tracking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageMutationStats {
    /// Total messages processed.
    pub messages_processed: u64,
    /// Total mutations applied.
    pub mutations_applied: u64,
    /// Messages dropped due to mutation rules.
    pub messages_dropped: u64,
    /// DoViewChange mutations applied.
    pub do_view_change_mutations: u64,
    /// StartView mutations applied.
    pub start_view_mutations: u64,
    /// Prepare mutations applied.
    pub prepare_mutations: u64,
    /// Commit mutations applied.
    pub commit_mutations: u64,
}

impl MessageMutationStats {
    /// Returns the mutation rate (mutations per message).
    pub fn mutation_rate(&self) -> f64 {
        if self.messages_processed == 0 {
            0.0
        } else {
            self.mutations_applied as f64 / self.messages_processed as f64
        }
    }

    /// Returns the drop rate (dropped per processed).
    pub fn drop_rate(&self) -> f64 {
        if self.messages_processed == 0 {
            0.0
        } else {
            self.messages_dropped as f64 / self.messages_processed as f64
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_kernel::Command;
    use kimberlite_types::{DataClass, Placement, Region, StreamId, StreamName, TenantId};
    use kimberlite_vsr::ViewNumber;

    fn make_test_command() -> Command {
        Command::CreateStream {
            stream_id: StreamId::from_tenant_and_local(TenantId::new(1), 1),
            stream_name: StreamName::from("test"),
            data_class: DataClass::PHI,
            placement: Placement::Region(Region::USEast1),
        }
    }

    #[test]
    fn test_message_type_filter_matches() {
        let prepare_msg = Message {
            from: ReplicaId::new(0),
            to: Some(ReplicaId::new(1)),
            payload: MessagePayload::Prepare(Prepare {
                view: ViewNumber::from(1),
                op_number: OpNumber::new(10),
                commit_number: CommitNumber::new(OpNumber::new(5)),
                entry: LogEntry {
                    op_number: OpNumber::new(10),
                    view: ViewNumber::from(1),
                    command: make_test_command(),
                    idempotency_id: None,
                    client_id: None,
                    request_number: None,
                    checksum: 0,
                },
                reconfig: None,
            }),
            signature: None, // Test message, no signature needed
        };

        assert!(MessageTypeFilter::Any.matches(&prepare_msg));
        assert!(MessageTypeFilter::Prepare.matches(&prepare_msg));
        assert!(!MessageTypeFilter::Commit.matches(&prepare_msg));
    }

    #[test]
    fn test_inflate_commit_number_fixed() {
        let dvc = DoViewChange {
            view: ViewNumber::from(2),
            last_normal_view: ViewNumber::from(1),
            op_number: OpNumber::new(100),
            commit_number: CommitNumber::new(OpNumber::new(50)),
            log_tail: vec![],
            replica: ReplicaId::new(0),
            reconfig_state: None,
        };

        let message = Message {
            from: ReplicaId::new(0),
            to: Some(ReplicaId::new(1)),
            payload: MessagePayload::DoViewChange(dvc),
            signature: None, // Test message, no signature needed
        };

        let mutation = MessageFieldMutation::InflateCommitNumber { amount: 500 };
        let mut rng = SimRng::new(0);

        let mutated = mutation
            .apply(&message, ReplicaId::new(1), &mut rng)
            .expect("mutation should apply");

        if let MessagePayload::DoViewChange(mutated_dvc) = &mutated.payload {
            assert_eq!(mutated_dvc.commit_number.as_u64(), 550); // 50 + 500
        } else {
            panic!("Expected DoViewChange payload");
        }
    }

    #[test]
    fn test_truncate_log_tail() {
        let log_tail = vec![
            LogEntry {
                op_number: OpNumber::new(1),
                view: ViewNumber::from(1),
                command: make_test_command(),
                idempotency_id: None,
                client_id: None,
                request_number: None,
                checksum: 0,
            },
            LogEntry {
                op_number: OpNumber::new(2),
                view: ViewNumber::from(1),
                command: make_test_command(),
                idempotency_id: None,
                client_id: None,
                request_number: None,
                checksum: 1,
            },
            LogEntry {
                op_number: OpNumber::new(3),
                view: ViewNumber::from(1),
                command: make_test_command(),
                idempotency_id: None,
                client_id: None,
                request_number: None,
                checksum: 2,
            },
        ];

        let dvc = DoViewChange {
            view: ViewNumber::from(2),
            last_normal_view: ViewNumber::from(1),
            op_number: OpNumber::new(3),
            commit_number: CommitNumber::new(OpNumber::new(3)),
            log_tail,
            replica: ReplicaId::new(0),
            reconfig_state: None,
        };

        let message = Message {
            from: ReplicaId::new(0),
            to: Some(ReplicaId::new(1)),
            payload: MessagePayload::DoViewChange(dvc),
            signature: None, // Test message, no signature needed
        };

        let mutation = MessageFieldMutation::TruncateLogTail { max_entries: 1 };
        let mut rng = SimRng::new(0);

        let mutated = mutation
            .apply(&message, ReplicaId::new(1), &mut rng)
            .expect("mutation should apply");

        if let MessagePayload::DoViewChange(mutated_dvc) = &mutated.payload {
            assert_eq!(mutated_dvc.log_tail.len(), 1);
            assert_eq!(mutated_dvc.log_tail[0].op_number.as_u64(), 1);
        } else {
            panic!("Expected DoViewChange payload");
        }
    }

    #[test]
    fn test_mutation_rule_probability() {
        let dvc = DoViewChange {
            view: ViewNumber::from(2),
            last_normal_view: ViewNumber::from(1),
            op_number: OpNumber::new(100),
            commit_number: CommitNumber::new(OpNumber::new(50)),
            log_tail: vec![],
            replica: ReplicaId::new(0),
            reconfig_state: None,
        };

        let message = Message {
            from: ReplicaId::new(0),
            to: Some(ReplicaId::new(1)),
            payload: MessagePayload::DoViewChange(dvc),
            signature: None, // Test message, no signature needed
        };

        let rule = MessageMutationRule {
            target: MessageTypeFilter::DoViewChange,
            from_replica: None,
            to_replica: None,
            probability: 1.0, // Always apply
            mutation: MessageFieldMutation::InflateCommitNumber { amount: 100 },
            deliver: true,
        };

        let mut rng = SimRng::new(42);
        let result = rule.try_apply(&message, ReplicaId::new(1), &mut rng);
        assert!(result.is_some());
    }

    #[test]
    fn test_message_mutator() {
        let rules = vec![MessageMutationRule {
            target: MessageTypeFilter::DoViewChange,
            from_replica: None,
            to_replica: None,
            probability: 1.0,
            mutation: MessageFieldMutation::InflateCommitNumber { amount: 500 },
            deliver: true,
        }];

        let mut mutator = MessageMutator::new(rules);

        let dvc = DoViewChange {
            view: ViewNumber::from(2),
            last_normal_view: ViewNumber::from(1),
            op_number: OpNumber::new(100),
            commit_number: CommitNumber::new(OpNumber::new(50)),
            log_tail: vec![],
            replica: ReplicaId::new(0),
            reconfig_state: None,
        };

        let message = Message {
            from: ReplicaId::new(0),
            to: Some(ReplicaId::new(1)),
            payload: MessagePayload::DoViewChange(dvc),
            signature: None, // Test message, no signature needed
        };

        let mut rng = SimRng::new(0);
        let result = mutator.apply(&message, ReplicaId::new(1), &mut rng);

        assert!(result.is_some());
        assert_eq!(mutator.stats().mutations_applied, 1);
        assert_eq!(mutator.stats().do_view_change_mutations, 1);
    }
}
