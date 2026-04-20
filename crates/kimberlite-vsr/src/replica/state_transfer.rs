//! State transfer protocol handlers.
//!
//! This module implements state transfer for replicas that are too far behind
//! to catch up via log repair and need a full checkpoint.
//!
//! # When State Transfer is Needed
//!
//! State transfer is triggered when:
//! 1. A replica receives a message from a much higher view
//! 2. Log repair fails because entries have been garbage collected
//! 3. A replica recovers and finds it's too far behind
//!
//! # Protocol Flow
//!
//! ```text
//! Stale Replica ──StateTransferRequest──► All
//!                    │
//!                    ▼ (f+1 responses)
//! Stale Replica ◄─StateTransferResponse── Healthy Replica
//!                    │
//!                    ▼ (apply checkpoint)
//!                 Normal Operation
//! ```

use std::collections::HashMap;

use kimberlite_kernel::Effect;
use kimberlite_types::Hash;

use crate::checkpoint::{CheckpointData, compute_merkle_root};
use crate::message::{
    MAX_STATE_TRANSFER_TAIL_LEN, MessagePayload, StateTransferRequest, StateTransferResponse,
};
use crate::types::{CommitNumber, LogEntry, Nonce, OpNumber, ReplicaId, ReplicaStatus, ViewNumber};

use super::{ReplicaOutput, ReplicaState, msg_broadcast, msg_to};

// ============================================================================
// State Transfer State
// ============================================================================

/// State tracked during a state transfer operation.
#[derive(Debug, Clone)]
pub struct StateTransferState {
    /// Nonce for matching responses to our request.
    pub nonce: Nonce,

    /// Our current checkpoint op (to only accept newer checkpoints).
    pub known_checkpoint: OpNumber,

    /// Responses received from replicas.
    pub responses: HashMap<ReplicaId, StateTransferResponse>,

    /// The target view we're trying to reach (if known).
    pub target_view: Option<ViewNumber>,
}

impl StateTransferState {
    /// Creates a new state transfer state.
    pub fn new(nonce: Nonce, known_checkpoint: OpNumber) -> Self {
        Self {
            nonce,
            known_checkpoint,
            responses: HashMap::new(),
            target_view: None,
        }
    }

    /// Creates a state transfer state with a target view.
    pub fn with_target_view(
        nonce: Nonce,
        known_checkpoint: OpNumber,
        target_view: ViewNumber,
    ) -> Self {
        Self {
            nonce,
            known_checkpoint,
            responses: HashMap::new(),
            target_view: Some(target_view),
        }
    }

    /// Returns the number of responses received.
    pub fn response_count(&self) -> usize {
        self.responses.len()
    }

    /// Returns the best (most recent) checkpoint from responses.
    ///
    /// Selects the checkpoint with the highest op number that has valid
    /// Merkle verification.
    pub fn best_checkpoint(&self) -> Option<&StateTransferResponse> {
        self.responses
            .values()
            .max_by_key(|r| r.checkpoint_op.as_u64())
    }
}

impl ReplicaState {
    // ========================================================================
    // State Transfer Initiation
    // ========================================================================

    /// Initiates state transfer to catch up with the cluster.
    ///
    /// Called when the replica is too far behind to use log repair.
    pub fn start_state_transfer(
        mut self,
        target_view: Option<ViewNumber>,
    ) -> (Self, ReplicaOutput) {
        // Generate a nonce for this request
        let nonce = Nonce::generate();

        // Record our highest known checkpoint
        let known_checkpoint: OpNumber = self.commit_number.into();

        // Initialize state transfer state
        self.state_transfer_state = Some(if let Some(view) = target_view {
            StateTransferState::with_target_view(nonce, known_checkpoint, view)
        } else {
            StateTransferState::new(nonce, known_checkpoint)
        });

        // Create request
        let request = StateTransferRequest::new(self.replica_id, nonce, known_checkpoint);

        // Broadcast to all replicas
        let msg = self.sign_message(msg_broadcast(
            self.replica_id,
            MessagePayload::StateTransferRequest(request),
        ));

        tracing::info!(
            replica = %self.replica_id,
            known_checkpoint = %known_checkpoint,
            target_view = ?target_view,
            "initiated state transfer"
        );

        (self, ReplicaOutput::with_messages(vec![msg]))
    }

    // ========================================================================
    // StateTransferRequest Handler
    // ========================================================================

    /// Handles a `StateTransferRequest` from another replica.
    ///
    /// Responds with our latest checkpoint if we have one newer than theirs.
    pub(crate) fn on_state_transfer_request(
        self,
        from: ReplicaId,
        request: &StateTransferRequest,
    ) -> (Self, ReplicaOutput) {
        // Don't respond to our own request
        if from == self.replica_id {
            return (self, ReplicaOutput::empty());
        }

        // If we're recovering or in state transfer ourselves, we can't help
        if self.status == ReplicaStatus::Recovering {
            return (self, ReplicaOutput::empty());
        }

        // Check if we have a checkpoint newer than what they know
        let our_checkpoint: OpNumber = self.commit_number.into();
        if our_checkpoint <= request.known_checkpoint {
            // We don't have anything newer to offer
            return (self, ReplicaOutput::empty());
        }

        // Build checkpoint data from our current state
        let checkpoint_data = self.build_checkpoint_data();

        // Convert MerkleRoot to Hash for the response
        let merkle_root = Hash::from_bytes(*checkpoint_data.log_root.as_bytes());

        // Serialize checkpoint data using serde_json
        let checkpoint_bytes = serde_json::to_vec(&checkpoint_data).unwrap_or_else(|_| Vec::new());

        // Ship the committed log tail covering
        // `(request.known_checkpoint, our_checkpoint]` when the gap fits
        // within the wire bound. The receiver replays these through the
        // normal apply path so its `AppliedCommit` fanout stays intact —
        // otherwise observers like the chaos write-log would silently
        // miss the caught-up ops. If the gap exceeds the bound we ship
        // an empty tail and warn; observer gap is then unavoidable.
        let (log_tail, tail_base_op) =
            build_log_tail(&self.log, request.known_checkpoint, our_checkpoint);
        if log_tail.is_empty() && our_checkpoint > request.known_checkpoint.next() {
            tracing::warn!(
                replica = %self.replica_id,
                known = %request.known_checkpoint,
                checkpoint = %our_checkpoint,
                gap = our_checkpoint.as_u64().saturating_sub(request.known_checkpoint.as_u64()),
                cap = MAX_STATE_TRANSFER_TAIL_LEN,
                "state-transfer gap exceeds tail bound; shipping empty tail (observers will have a gap)",
            );
        }

        // Create response
        let response = StateTransferResponse::new(
            self.replica_id,
            request.nonce,
            self.view,
            our_checkpoint,
            merkle_root,
            checkpoint_bytes,
            log_tail,
            tail_base_op,
            None, // Signature would require access to signing key
        );

        let msg = self.sign_message(msg_to(
            self.replica_id,
            from,
            MessagePayload::StateTransferResponse(response),
        ));

        tracing::debug!(
            replica = %self.replica_id,
            to = %from,
            checkpoint_op = %our_checkpoint,
            "sending state transfer response"
        );

        (self, ReplicaOutput::with_messages(vec![msg]))
    }

    // ========================================================================
    // StateTransferResponse Handler
    // ========================================================================

    /// Handles a `StateTransferResponse` from another replica.
    ///
    /// Applies the checkpoint if it's valid and newer than our current state.
    #[allow(clippy::needless_pass_by_value)] // response is cloned when stored
    pub(crate) fn on_state_transfer_response(
        mut self,
        from: ReplicaId,
        response: StateTransferResponse,
    ) -> (Self, ReplicaOutput) {
        // Must be waiting for state transfer
        let (nonce, known_checkpoint, quorum) = {
            let Some(ref st_state) = self.state_transfer_state else {
                return (self, ReplicaOutput::empty());
            };
            (
                st_state.nonce,
                st_state.known_checkpoint,
                self.config.quorum_size(),
            )
        };

        // Nonce must match
        if response.nonce != nonce {
            return (self, ReplicaOutput::empty());
        }

        // Checkpoint must be newer than what we requested
        if response.checkpoint_op <= known_checkpoint {
            return (self, ReplicaOutput::empty());
        }

        // Record the response
        if let Some(ref mut st_state) = self.state_transfer_state {
            st_state.responses.insert(from, response.clone());
        }

        // Check if we have enough responses to proceed
        let response_count = self
            .state_transfer_state
            .as_ref()
            .map_or(0, StateTransferState::response_count);

        if response_count < quorum {
            return (self, ReplicaOutput::empty());
        }

        // Select the best checkpoint
        let best_response = self
            .state_transfer_state
            .as_ref()
            .and_then(|s| s.best_checkpoint().cloned());

        let Some(best_response) = best_response else {
            return (self, ReplicaOutput::empty());
        };

        // CRITICAL: Verify quorum agreement on Merkle root
        // Count how many responses have the same Merkle root as best_response
        let merkle_root_agreement = self.state_transfer_state.as_ref().map_or(0, |s| {
            s.responses
                .values()
                .filter(|r| r.merkle_root == best_response.merkle_root)
                .count()
        });

        if merkle_root_agreement < quorum {
            tracing::warn!(
                replica = %self.replica_id,
                agreement_count = merkle_root_agreement,
                required = quorum,
                merkle_root = ?best_response.merkle_root,
                "insufficient quorum agreement on Merkle root - Byzantine attack detected"
            );

            #[cfg(feature = "sim")]
            crate::instrumentation::record_byzantine_rejection(
                "merkle_root_quorum_failure",
                self.replica_id,
                merkle_root_agreement as u64,
                quorum as u64,
            );

            self.state_transfer_state = None;
            return (self, ReplicaOutput::empty());
        }

        // Try to verify and apply the checkpoint
        // First validate without consuming self
        let checkpoint_data: CheckpointData =
            if let Ok(data) = serde_json::from_slice(&best_response.checkpoint_data) {
                data
            } else {
                tracing::warn!(
                    replica = %self.replica_id,
                    "failed to deserialize checkpoint data"
                );
                self.state_transfer_state = None;
                return (self, ReplicaOutput::empty());
            };

        // Verify Merkle root consistency within the response
        let expected_root = Hash::from_bytes(*checkpoint_data.log_root.as_bytes());
        if expected_root != best_response.merkle_root {
            tracing::warn!(
                replica = %self.replica_id,
                "Merkle root mismatch in state transfer (internal inconsistency)"
            );
            self.state_transfer_state = None;
            return (self, ReplicaOutput::empty());
        }

        // All validation passed. Adopt the view metadata up front — the
        // tail, when present, was produced under this view.
        self.view = best_response.checkpoint_view;
        self.last_normal_view = best_response.checkpoint_view;

        // Clear state transfer state and pending repair/recovery so the
        // replica is ready to transition to Normal whether or not we
        // apply a tail. `status` flips at the end to avoid tripping
        // status invariants mid-apply.
        self.state_transfer_state = None;
        self.repair_state = None;
        self.recovery_state = None;

        // Two paths from here:
        //
        // (A) Log tail included and contiguous with our current commit.
        //     Append to the log, advance `op_number` to checkpoint_op,
        //     then drive `apply_commits_up_to(checkpoint_op)` which runs
        //     the kernel per entry and returns per-op effects. These
        //     effects reach `event_loop::handle_output` and fire
        //     `AppliedCommit` for every caught-up op — exactly what a
        //     live prepare/commit path produces. Downstream observers
        //     (chaos write-log, future audit/projection) stay in sync.
        //
        // (B) No tail (sender log truncated past our checkpoint, or gap
        //     exceeds the wire bound). Fall back to the legacy "jump"
        //     behaviour: clear the log and snap `commit_number` to
        //     checkpoint_op. Observers on this replica have a gap for
        //     the missing ops, which is unavoidable.
        let tail = best_response.log_tail;
        let tail_base_op = best_response.tail_base_op;
        let checkpoint_op = best_response.checkpoint_op;
        let expected_base = self.commit_number.as_op_number().next();

        let applied_effects: Vec<Effect> = if !tail.is_empty()
            && tail_base_op == expected_base
            && tail.last().map(|e| e.op_number) == Some(checkpoint_op)
        {
            // Append the tail to our log. After this, `self.log` covers
            // ops `[1..=checkpoint_op]` (modulo any prior truncation on
            // THIS replica's log, which the apply loop tolerates as long
            // as the entry for each committed op is present).
            let appended = tail.len();
            self.log.extend(tail);
            self.op_number = checkpoint_op;

            let (new_self, effects) = self.apply_commits_up_to(CommitNumber::new(checkpoint_op));
            self = new_self;

            tracing::info!(
                replica = %self.replica_id,
                checkpoint_op = %checkpoint_op,
                appended,
                effects = effects.len(),
                "applied state-transfer tail via normal apply path"
            );

            effects
        } else {
            if !tail.is_empty() {
                tracing::warn!(
                    replica = %self.replica_id,
                    tail_base = %tail_base_op,
                    expected_base = %expected_base,
                    tail_len = tail.len(),
                    "state-transfer tail non-contiguous; falling back to jump-apply"
                );
            }

            self.op_number = checkpoint_op;
            self.commit_number = CommitNumber::new(checkpoint_op);
            self.log.clear();

            tracing::info!(
                replica = %self.replica_id,
                checkpoint_op = %checkpoint_op,
                checkpoint_view = %best_response.checkpoint_view,
                "applied state-transfer checkpoint (no tail — observer gap)"
            );

            Vec::new()
        };

        // Transition to normal status last — by now all mutation is
        // done and the replica is ready to serve again.
        self.status = ReplicaStatus::Normal;

        (
            self,
            ReplicaOutput {
                messages: Vec::new(),
                effects: applied_effects,
                committed_op: None,
            },
        )
    }

    // ========================================================================
    // Checkpoint Helpers
    // ========================================================================

    /// Builds checkpoint data from current state.
    fn build_checkpoint_data(&self) -> CheckpointData {
        // Build Merkle tree from log entries
        let log_root = compute_merkle_root(&self.log);

        let commit_op: OpNumber = self.commit_number.into();

        CheckpointData::new(
            commit_op,
            self.view,
            log_root,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        )
    }
}

/// Slices the committed tail `log[(known_checkpoint, checkpoint_op]]`
/// for inclusion in a `StateTransferResponse`. Returns an empty tail
/// (and `OpNumber::ZERO` as the neutral placeholder base) when:
///
/// - the gap is empty (`checkpoint_op == known_checkpoint + 1` with the
///   sender lacking entry `known_checkpoint + 1` — shouldn't happen in
///   practice but is handled defensively),
/// - the gap exceeds [`MAX_STATE_TRANSFER_TAIL_LEN`] (too expensive to
///   ship in one message), OR
/// - the sender has truncated past `known_checkpoint` (cannot synthesise
///   the tail; observers will have a gap).
///
/// The slice is *inclusive* on the top end: `checkpoint_op` itself is
/// the last entry shipped. The receiver uses `tail_base_op` to verify
/// contiguity with its own log.
fn build_log_tail(
    log: &[LogEntry],
    known_checkpoint: OpNumber,
    checkpoint_op: OpNumber,
) -> (Vec<LogEntry>, OpNumber) {
    if checkpoint_op <= known_checkpoint {
        return (Vec::new(), OpNumber::ZERO);
    }

    let gap = checkpoint_op
        .as_u64()
        .saturating_sub(known_checkpoint.as_u64());
    if gap as usize > MAX_STATE_TRANSFER_TAIL_LEN {
        return (Vec::new(), OpNumber::ZERO);
    }

    // `log_entry(op)` maps op N → log[N-1]. `known_checkpoint + 1` is
    // the first op we need to ship; `checkpoint_op` is the last.
    let first_op = known_checkpoint.as_u64().saturating_add(1);
    let last_op = checkpoint_op.as_u64();

    let Some(first_idx) = first_op.checked_sub(1).map(|v| v as usize) else {
        return (Vec::new(), OpNumber::ZERO);
    };
    let last_idx = match last_op.checked_sub(1) {
        Some(v) => v as usize,
        None => return (Vec::new(), OpNumber::ZERO),
    };
    if last_idx >= log.len() || first_idx > last_idx {
        // Sender's log doesn't cover the requested range — tail
        // unavailable. Receiver falls back to the legacy "jump"
        // behaviour and accepts the observer gap.
        return (Vec::new(), OpNumber::ZERO);
    }

    let tail: Vec<LogEntry> = log[first_idx..=last_idx].to_vec();
    (tail, OpNumber::new(first_op))
}

#[cfg(test)]
mod tail_builder_tests {
    use super::*;
    use crate::types::ViewNumber;
    use kimberlite_kernel::Command;
    use kimberlite_types::{DataClass, Placement, StreamId, StreamName};

    fn mk_entry(op: u64) -> LogEntry {
        LogEntry::new(
            OpNumber::new(op),
            ViewNumber::new(1),
            Command::create_stream(
                StreamId::new(op),
                StreamName::new(format!("s{op}")),
                DataClass::Public,
                Placement::Global,
            ),
            None,
            None,
            None,
        )
    }

    fn mk_log(n: u64) -> Vec<LogEntry> {
        (1..=n).map(mk_entry).collect()
    }

    #[test]
    fn tail_covers_expected_range() {
        let log = mk_log(10);
        let (tail, base) = build_log_tail(&log, OpNumber::new(3), OpNumber::new(7));
        let ops: Vec<u64> = tail.iter().map(|e| e.op_number.as_u64()).collect();
        assert_eq!(ops, vec![4, 5, 6, 7]);
        assert_eq!(base, OpNumber::new(4));
    }

    #[test]
    fn empty_when_gap_exceeds_bound() {
        let log = mk_log((MAX_STATE_TRANSFER_TAIL_LEN as u64) + 50);
        let (tail, base) = build_log_tail(
            &log,
            OpNumber::ZERO,
            OpNumber::new((MAX_STATE_TRANSFER_TAIL_LEN as u64) + 50),
        );
        assert!(tail.is_empty());
        assert_eq!(base, OpNumber::ZERO);
    }

    #[test]
    fn empty_when_sender_truncated() {
        // Log has ops 1..=5; requester knows 3; checkpoint claims 10
        // (impossible with this log but we handle defensively).
        let log = mk_log(5);
        let (tail, base) = build_log_tail(&log, OpNumber::new(3), OpNumber::new(10));
        assert!(tail.is_empty());
        assert_eq!(base, OpNumber::ZERO);
    }

    #[test]
    fn empty_when_requester_already_caught_up() {
        let log = mk_log(10);
        let (tail, _) = build_log_tail(&log, OpNumber::new(10), OpNumber::new(10));
        assert!(tail.is_empty());
    }
}

#[cfg(test)]
mod receiver_replay_tests {
    //! The critical correctness property enforced here: when a stale
    //! replica catches up via state transfer with a populated
    //! `log_tail`, it MUST produce `Effect`s for each caught-up op so
    //! the `AppliedCommit` fanout in `event_loop::handle_output` fires
    //! for every op — otherwise downstream observers (chaos write-log,
    //! future audit/projection consumers) silently drift out of sync
    //! with VSR's committed log.

    use super::*;
    use crate::checkpoint::compute_merkle_root;
    use crate::config::ClusterConfig;
    use crate::message::StateTransferResponse;
    use crate::replica::ReplicaState;
    use crate::types::{LogEntry, Nonce, ViewNumber};
    use kimberlite_kernel::Command;
    use kimberlite_types::{DataClass, Hash, Placement, StreamName};

    fn cluster_3() -> ClusterConfig {
        ClusterConfig::new(vec![
            ReplicaId::new(0),
            ReplicaId::new(1),
            ReplicaId::new(2),
        ])
    }

    /// Builds a committed log of `n` `CreateStream` entries at view 0.
    fn committed_log(n: u64) -> Vec<LogEntry> {
        (1..=n)
            .map(|op| {
                LogEntry::new(
                    OpNumber::new(op),
                    ViewNumber::ZERO,
                    Command::create_stream_with_auto_id(
                        StreamName::new(format!("s{op}")),
                        DataClass::Public,
                        Placement::Global,
                    ),
                    None,
                    None,
                    None,
                )
            })
            .collect()
    }

    /// Produces a `StateTransferResponse` carrying the full log as the
    /// tail from `known_checkpoint` up to the log's last op.
    fn response_with_tail(
        from: ReplicaId,
        nonce: Nonce,
        log: &[LogEntry],
        known_checkpoint: OpNumber,
    ) -> StateTransferResponse {
        let checkpoint_op = log.last().map(|e| e.op_number).unwrap_or(OpNumber::ZERO);
        let root = compute_merkle_root(log);
        let checkpoint_data =
            crate::checkpoint::CheckpointData::new(checkpoint_op, ViewNumber::ZERO, root, 0);
        let merkle_root = Hash::from_bytes(*root.as_bytes());
        let bytes = serde_json::to_vec(&checkpoint_data).expect("serialize checkpoint");

        let (tail, tail_base_op) = build_log_tail(log, known_checkpoint, checkpoint_op);
        StateTransferResponse::new(
            from,
            nonce,
            ViewNumber::ZERO,
            checkpoint_op,
            merkle_root,
            bytes,
            tail,
            tail_base_op,
            None,
        )
    }

    #[test]
    fn state_transfer_with_tail_emits_effects_per_op() {
        // Stale replica at commit 0 with no local log; cluster has 5
        // committed ops. State transfer should replay ops 1..=5 through
        // the normal apply path, producing one effect batch per op.
        let mut r0 = ReplicaState::new(ReplicaId::new(0), cluster_3());
        let (r0_after_start, _) = r0.start_state_transfer(None);
        r0 = r0_after_start;

        let nonce = r0
            .state_transfer_state
            .as_ref()
            .expect("state transfer state set")
            .nonce;

        let log = committed_log(5);
        let resp1 = response_with_tail(ReplicaId::new(1), nonce, &log, OpNumber::ZERO);
        let resp2 = response_with_tail(ReplicaId::new(2), nonce, &log, OpNumber::ZERO);

        // First response alone: not enough for quorum (need 2 of 3).
        let (mut r0, output) = r0.on_state_transfer_response(ReplicaId::new(1), resp1);
        assert!(output.effects.is_empty(), "first response must not apply");

        // Second response: quorum reached; apply replays the tail.
        let (r0_final, output) = r0.on_state_transfer_response(ReplicaId::new(2), resp2);
        r0 = r0_final;

        // Every caught-up op must have produced at least one effect.
        // `CreateStream` emits a `StreamMetadataWrite`, so effects.len()
        // >= log.len(). A weaker `>= 1` check would let a future
        // regression (e.g. only the last op fires) slip through.
        assert!(
            output.effects.len() >= log.len(),
            "expected at least {} effects (one per caught-up op), got {}",
            log.len(),
            output.effects.len(),
        );
        assert_eq!(r0.commit_number(), CommitNumber::new(OpNumber::new(5)));
        assert_eq!(r0.status(), ReplicaStatus::Normal);
    }

    #[test]
    fn state_transfer_without_tail_falls_back_to_jump() {
        // Stale replica; sender's response carries an empty tail (e.g.
        // gap exceeded the bound). Receiver must still advance to
        // checkpoint_op — just without observer fidelity.
        let mut r0 = ReplicaState::new(ReplicaId::new(0), cluster_3());
        let (r0_after_start, _) = r0.start_state_transfer(None);
        r0 = r0_after_start;

        let nonce = r0
            .state_transfer_state
            .as_ref()
            .expect("state transfer state set")
            .nonce;

        let log = committed_log(5);
        // Force an empty tail by claiming a checkpoint far past what
        // the (imaginary) sender log actually contains.
        let root = compute_merkle_root(&log);
        let checkpoint_data =
            crate::checkpoint::CheckpointData::new(OpNumber::new(5), ViewNumber::ZERO, root, 0);
        let bytes = serde_json::to_vec(&checkpoint_data).expect("serialize");
        let merkle_root = Hash::from_bytes(*root.as_bytes());

        let empty_tail_resp = |from| {
            StateTransferResponse::new(
                from,
                nonce,
                ViewNumber::ZERO,
                OpNumber::new(5),
                merkle_root,
                bytes.clone(),
                Vec::new(),
                OpNumber::ZERO,
                None,
            )
        };

        let (r0_mid, _) =
            r0.on_state_transfer_response(ReplicaId::new(1), empty_tail_resp(ReplicaId::new(1)));
        let (r0_final, output) = r0_mid
            .on_state_transfer_response(ReplicaId::new(2), empty_tail_resp(ReplicaId::new(2)));

        assert!(output.effects.is_empty(), "no tail means no effects");
        assert_eq!(
            r0_final.commit_number(),
            CommitNumber::new(OpNumber::new(5))
        );
        assert_eq!(r0_final.status(), ReplicaStatus::Normal);
    }
}
