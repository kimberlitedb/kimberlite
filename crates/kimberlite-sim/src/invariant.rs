//! Invariant checkers for simulation testing.
//!
//! Invariant checkers continuously verify correctness properties during
//! simulation. If an invariant is violated, the simulation can stop
//! immediately with a detailed error.
//!
//! # Available Checkers
//!
//! - [`HashChainChecker`]: Verifies hash chain integrity
//! - [`LogConsistencyChecker`]: Verifies log reads match committed writes
//! - [`ReplicaConsistencyChecker`]: Verifies byte-for-byte replica consistency

use kimberlite_crypto::ChainHash;

use crate::SimError;
use crate::instrumentation::invariant_tracker;

// ============================================================================
// Invariant Result
// ============================================================================

/// Result of an invariant check.
#[derive(Debug, Clone)]
pub enum InvariantResult {
    /// The invariant holds.
    Ok,
    /// The invariant is violated.
    Violated {
        /// Name of the violated invariant.
        invariant: String,
        /// Description of the violation.
        message: String,
        /// Additional context.
        context: Vec<(String, String)>,
    },
}

impl InvariantResult {
    /// Returns true if the invariant holds.
    pub fn is_ok(&self) -> bool {
        matches!(self, InvariantResult::Ok)
    }

    /// Converts to a `SimError` if violated.
    pub fn into_error(self, time_ns: u64) -> Option<SimError> {
        match self {
            InvariantResult::Ok => None,
            InvariantResult::Violated { message, .. } => {
                Some(SimError::InvariantViolation { message, time_ns })
            }
        }
    }
}

// ============================================================================
// Invariant Checker Trait
// ============================================================================

/// Trait for invariant checkers.
///
/// Invariant checkers verify that correctness properties hold during simulation.
pub trait InvariantChecker {
    /// Returns the name of this checker.
    fn name(&self) -> &'static str;

    /// Resets the checker to its initial state.
    fn reset(&mut self);
}

// ============================================================================
// Hash Chain Checker
// ============================================================================

/// Verifies hash chain integrity.
///
/// The hash chain checker maintains the expected chain state and verifies
/// that each new record correctly links to the previous one.
#[derive(Debug)]
pub struct HashChainChecker {
    /// The last seen chain hash.
    last_hash: Option<ChainHash>,
    /// The last seen offset.
    last_offset: Option<u64>,
    /// Number of records checked.
    records_checked: u64,
}

impl HashChainChecker {
    /// Creates a new hash chain checker.
    pub fn new() -> Self {
        Self {
            last_hash: None,
            last_offset: None,
            records_checked: 0,
        }
    }

    /// Checks a record against the expected chain state.
    ///
    /// # Arguments
    ///
    /// * `offset` - The record's offset in the log
    /// * `prev_hash` - The record's claimed previous hash
    /// * `current_hash` - The hash of this record
    pub fn check_record(
        &mut self,
        offset: u64,
        prev_hash: &ChainHash,
        current_hash: &ChainHash,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("hash_chain_integrity");

        // Check offset monotonicity
        if let Some(last_offset) = self.last_offset {
            if offset != last_offset + 1 {
                return InvariantResult::Violated {
                    invariant: "hash_chain_offset_monotonic".to_string(),
                    message: format!("offset gap: expected {}, got {}", last_offset + 1, offset),
                    context: vec![
                        ("last_offset".to_string(), last_offset.to_string()),
                        ("current_offset".to_string(), offset.to_string()),
                    ],
                };
            }
        } else if offset != 0 {
            // First record should be at offset 0
            return InvariantResult::Violated {
                invariant: "hash_chain_starts_at_zero".to_string(),
                message: format!("first record should be at offset 0, got {offset}"),
                context: vec![("offset".to_string(), offset.to_string())],
            };
        }

        // Check hash chain linkage
        if let Some(expected_prev) = &self.last_hash {
            if prev_hash != expected_prev {
                return InvariantResult::Violated {
                    invariant: "hash_chain_linkage".to_string(),
                    message: "hash chain broken: prev_hash doesn't match".to_string(),
                    context: vec![
                        ("offset".to_string(), offset.to_string()),
                        ("expected_prev".to_string(), format!("{expected_prev:?}")),
                        ("actual_prev".to_string(), format!("{prev_hash:?}")),
                    ],
                };
            }
        } else {
            // First record should have zero prev_hash
            let zero_hash = ChainHash::from_bytes(&[0u8; 32]);
            if *prev_hash != zero_hash {
                return InvariantResult::Violated {
                    invariant: "hash_chain_genesis".to_string(),
                    message: "first record should have zero prev_hash".to_string(),
                    context: vec![
                        ("offset".to_string(), offset.to_string()),
                        ("prev_hash".to_string(), format!("{prev_hash:?}")),
                    ],
                };
            }
        }

        // Update state
        self.last_hash = Some(*current_hash);
        self.last_offset = Some(offset);
        self.records_checked += 1;

        InvariantResult::Ok
    }

    /// Returns the number of records checked.
    pub fn records_checked(&self) -> u64 {
        self.records_checked
    }

    /// Returns the last verified offset.
    pub fn last_offset(&self) -> Option<u64> {
        self.last_offset
    }

    /// Returns the last verified hash.
    pub fn last_hash(&self) -> Option<&ChainHash> {
        self.last_hash.as_ref()
    }
}

impl Default for HashChainChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for HashChainChecker {
    fn name(&self) -> &'static str {
        "HashChainChecker"
    }

    fn reset(&mut self) {
        self.last_hash = None;
        self.last_offset = None;
        self.records_checked = 0;
    }
}

// ============================================================================
// Log Consistency Checker
// ============================================================================

/// Verifies log consistency across multiple views.
///
/// This checker ensures that once a record is committed, it remains
/// consistent across all subsequent reads.
#[derive(Debug)]
pub struct LogConsistencyChecker {
    /// Known committed records: offset -> (hash, `payload_hash`)
    committed: std::collections::HashMap<u64, (ChainHash, [u8; 32])>,
}

impl LogConsistencyChecker {
    /// Creates a new log consistency checker.
    pub fn new() -> Self {
        Self {
            committed: std::collections::HashMap::new(),
        }
    }

    /// Records a committed entry.
    pub fn record_commit(&mut self, offset: u64, chain_hash: ChainHash, payload_hash: [u8; 32]) {
        self.committed.insert(offset, (chain_hash, payload_hash));
    }

    /// Verifies a read against known commits.
    pub fn verify_read(
        &self,
        offset: u64,
        chain_hash: &ChainHash,
        payload_hash: &[u8; 32],
    ) -> InvariantResult {
        if let Some((expected_chain, expected_payload)) = self.committed.get(&offset) {
            if chain_hash != expected_chain {
                return InvariantResult::Violated {
                    invariant: "log_consistency_chain_hash".to_string(),
                    message: "chain hash mismatch on read".to_string(),
                    context: vec![
                        ("offset".to_string(), offset.to_string()),
                        ("expected".to_string(), format!("{expected_chain:?}")),
                        ("actual".to_string(), format!("{chain_hash:?}")),
                    ],
                };
            }
            if payload_hash != expected_payload {
                return InvariantResult::Violated {
                    invariant: "log_consistency_payload".to_string(),
                    message: "payload hash mismatch on read".to_string(),
                    context: vec![
                        ("offset".to_string(), offset.to_string()),
                        ("expected".to_string(), hex::encode(expected_payload)),
                        ("actual".to_string(), hex::encode(payload_hash)),
                    ],
                };
            }
        }
        InvariantResult::Ok
    }

    /// Returns the number of committed entries tracked.
    pub fn committed_count(&self) -> usize {
        self.committed.len()
    }
}

impl Default for LogConsistencyChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for LogConsistencyChecker {
    fn name(&self) -> &'static str {
        "LogConsistencyChecker"
    }

    fn reset(&mut self) {
        self.committed.clear();
    }
}

// ============================================================================
// Replica Consistency Checker
// ============================================================================

/// State of a single replica's log.
#[derive(Debug, Clone)]
pub struct ReplicaState {
    /// Replica identifier.
    pub replica_id: u64,
    /// Current log length (number of entries).
    pub log_length: u64,
    /// Hash of the log contents (for byte-for-byte comparison).
    pub log_hash: [u8; 32],
    /// Last update time.
    pub last_update_ns: u64,
}

/// Verifies byte-for-byte consistency across replicas.
///
/// Inspired by `TigerBeetle`'s replica consistency checking, this verifier ensures
/// that all replicas that have caught up to the same log position have identical
/// content. This detects:
///
/// - Byzantine failures (replicas diverging)
/// - Bugs in replication logic
/// - Storage corruption that went undetected
///
/// # How It Works
///
/// 1. Each replica reports its state (log length + content hash)
/// 2. When multiple replicas report the same log length, their hashes must match
/// 3. A violation indicates a critical consistency bug
///
/// # Performance
///
/// Uses a length-indexed lookup to only compare replicas at the same log length,
/// reducing O(n²) to O(replicas_at_same_length).
#[derive(Debug)]
pub struct ReplicaConsistencyChecker {
    /// Known replica states: `replica_id` -> state
    replicas: std::collections::HashMap<u64, ReplicaState>,
    /// Index: log_length -> Vec<replica_id> for fast lookup
    replica_index: std::collections::HashMap<u64, Vec<u64>>,
    /// Consistency violations detected.
    violations: Vec<ConsistencyViolation>,
}

/// A detected consistency violation.
#[derive(Debug, Clone)]
pub struct ConsistencyViolation {
    /// Log length where divergence was detected.
    pub log_length: u64,
    /// Replicas that disagree.
    pub divergent_replicas: Vec<(u64, [u8; 32])>, // (replica_id, hash)
    /// Time when violation was detected.
    pub detected_at_ns: u64,
}

impl ReplicaConsistencyChecker {
    /// Creates a new replica consistency checker.
    pub fn new() -> Self {
        Self {
            replicas: std::collections::HashMap::new(),
            replica_index: std::collections::HashMap::new(),
            violations: Vec::new(),
        }
    }

    /// Updates the state of a replica.
    ///
    /// Returns a violation if this update reveals inconsistency.
    /// The replica state is always tracked, even when divergence is detected.
    ///
    /// # Performance
    ///
    /// Uses the length index to only check replicas at the same log length,
    /// reducing from O(all_replicas) to O(replicas_at_same_length).
    pub fn update_replica(
        &mut self,
        replica_id: u64,
        log_length: u64,
        log_hash: [u8; 32],
        time_ns: u64,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("replica_consistency");

        // Remove from old length index if replica already exists
        if let Some(old_state) = self.replicas.get(&replica_id) {
            if old_state.log_length != log_length {
                if let Some(old_length_replicas) = self.replica_index.get_mut(&old_state.log_length) {
                    old_length_replicas.retain(|&id| id != replica_id);
                }
            }
        }

        // Check against other replicas at the same log length using the index
        let mut violation_result = None;
        if let Some(replicas_at_length) = self.replica_index.get(&log_length) {
            for &other_id in replicas_at_length {
                if other_id == replica_id {
                    continue;
                }

                if let Some(other_state) = self.replicas.get(&other_id) {
                    if other_state.log_hash != log_hash {
                        let violation = ConsistencyViolation {
                            log_length,
                            divergent_replicas: vec![
                                (other_id, other_state.log_hash),
                                (replica_id, log_hash),
                            ],
                            detected_at_ns: time_ns,
                        };
                        self.violations.push(violation);

                        violation_result = Some(InvariantResult::Violated {
                            invariant: "replica_consistency".to_string(),
                            message: format!(
                                "replicas {other_id} and {replica_id} diverge at log length {log_length}"
                            ),
                            context: vec![
                                ("log_length".to_string(), log_length.to_string()),
                                ("replica_a".to_string(), other_id.to_string()),
                                ("hash_a".to_string(), hex::encode(&other_state.log_hash)),
                                ("replica_b".to_string(), replica_id.to_string()),
                                ("hash_b".to_string(), hex::encode(&log_hash)),
                            ],
                        });
                        break;
                    }
                }
            }
        }

        // Update replica state
        self.replicas.insert(
            replica_id,
            ReplicaState {
                replica_id,
                log_length,
                log_hash,
                last_update_ns: time_ns,
            },
        );

        // Add to length index
        self.replica_index.entry(log_length).or_insert_with(Vec::new).push(replica_id);

        violation_result.unwrap_or(InvariantResult::Ok)
    }

    /// Performs a full consistency check across all replicas.
    ///
    /// Groups replicas by log length and verifies hash consistency within each group.
    ///
    /// # Correctness
    ///
    /// Checks ALL length groups and reports all violations, not just the first one.
    /// This ensures we detect all divergences, not just the first encountered.
    pub fn check_all(&self) -> InvariantResult {
        // Group replicas by log length
        let mut by_length: std::collections::HashMap<u64, Vec<&ReplicaState>> =
            std::collections::HashMap::new();

        for state in self.replicas.values() {
            by_length.entry(state.log_length).or_default().push(state);
        }

        // Collect ALL violations across all groups
        let mut all_violations = Vec::new();

        // Check consistency within each group
        for (length, replicas) in by_length {
            if replicas.len() < 2 {
                continue;
            }

            let first_hash = &replicas[0].log_hash;
            for replica in &replicas[1..] {
                if &replica.log_hash != first_hash {
                    all_violations.push(format!(
                        "Length {}: replicas {} and {} diverge",
                        length, replicas[0].replica_id, replica.replica_id
                    ));
                }
            }
        }

        // If any violations found, report them all
        if !all_violations.is_empty() {
            return InvariantResult::Violated {
                invariant: "replica_consistency".to_string(),
                message: format!("{} divergence(s) detected", all_violations.len()),
                context: vec![
                    ("violation_count".to_string(), all_violations.len().to_string()),
                    ("violations".to_string(), all_violations.join("; ")),
                ],
            };
        }

        InvariantResult::Ok
    }

    /// Returns the number of replicas being tracked.
    pub fn replica_count(&self) -> usize {
        self.replicas.len()
    }

    /// Returns the number of violations detected.
    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }

    /// Returns all detected violations.
    pub fn violations(&self) -> &[ConsistencyViolation] {
        &self.violations
    }

    /// Returns the state of a specific replica.
    pub fn get_replica(&self, replica_id: u64) -> Option<&ReplicaState> {
        self.replicas.get(&replica_id)
    }
}

impl Default for ReplicaConsistencyChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for ReplicaConsistencyChecker {
    fn name(&self) -> &'static str {
        "ReplicaConsistencyChecker"
    }

    fn reset(&mut self) {
        self.replicas.clear();
        self.replica_index.clear();
        self.violations.clear();
    }
}

// ============================================================================
// Hex encoding helper (minimal, no external dep)
// ============================================================================

mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            use std::fmt::Write;
            write!(s, "{b:02x}").expect("formatting cannot fail");
        }
        s
    }
}

// ============================================================================
// Replica Head Progress Checker
// ============================================================================

/// Verifies that replica (view, op) heads never regress.
///
/// Inspired by `TigerBeetle`'s `StateChecker`, this ensures replicas make
/// forward progress and never roll back their committed state.
#[derive(Debug)]
pub struct ReplicaHeadChecker {
    /// Replica heads: `replica_id` -> (view, op)
    replica_heads: std::collections::HashMap<u64, (u32, u64)>,
}

impl ReplicaHeadChecker {
    /// Creates a new replica head checker.
    pub fn new() -> Self {
        Self {
            replica_heads: std::collections::HashMap::new(),
        }
    }

    /// Updates the head position of a replica.
    ///
    /// Returns a violation if the replica's head regressed.
    pub fn update_head(&mut self, replica_id: u64, view: u32, op: u64) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("replica_head_progress");

        if let Some((prev_view, prev_op)) = self.replica_heads.get(&replica_id) {
            // View/op must never regress
            if view < *prev_view || (view == *prev_view && op < *prev_op) {
                return InvariantResult::Violated {
                    invariant: "replica_head_progress".to_string(),
                    message: format!(
                        "replica {replica_id} regressed from ({prev_view},{prev_op}) to ({view},{op})"
                    ),
                    context: vec![
                        ("replica_id".to_string(), replica_id.to_string()),
                        ("prev_view".to_string(), prev_view.to_string()),
                        ("prev_op".to_string(), prev_op.to_string()),
                        ("new_view".to_string(), view.to_string()),
                        ("new_op".to_string(), op.to_string()),
                    ],
                };
            }
        }

        self.replica_heads.insert(replica_id, (view, op));
        InvariantResult::Ok
    }

    /// Returns the current head for a replica.
    pub fn get_head(&self, replica_id: u64) -> Option<(u32, u64)> {
        self.replica_heads.get(&replica_id).copied()
    }

    /// Returns the number of replicas being tracked.
    pub fn replica_count(&self) -> usize {
        self.replica_heads.len()
    }
}

impl Default for ReplicaHeadChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for ReplicaHeadChecker {
    fn name(&self) -> &'static str {
        "ReplicaHeadChecker"
    }

    fn reset(&mut self) {
        self.replica_heads.clear();
    }
}

// ============================================================================
// Commit History Checker
// ============================================================================

/// Verifies commit history has no gaps or duplicates.
///
/// Inspired by `TigerBeetle`'s operation monotonicity checking, this ensures
/// operations are committed in sequential order without skips.
#[derive(Debug)]
pub struct CommitHistoryChecker {
    /// Last committed operation number.
    last_op: Option<u64>,
    /// Total commits recorded.
    commit_count: u64,
}

impl CommitHistoryChecker {
    /// Creates a new commit history checker.
    pub fn new() -> Self {
        Self {
            last_op: None,
            commit_count: 0,
        }
    }

    /// Records a commit with the given operation number.
    ///
    /// Returns a violation if there's a gap or duplicate.
    pub fn record_commit(&mut self, op: u64) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("commit_history_monotonic");

        if let Some(last) = self.last_op {
            if op != last + 1 {
                return InvariantResult::Violated {
                    invariant: "commit_history_monotonic".to_string(),
                    message: format!("commit gap: expected op {}, got {}", last + 1, op),
                    context: vec![
                        ("last_op".to_string(), last.to_string()),
                        ("current_op".to_string(), op.to_string()),
                        ("commit_count".to_string(), self.commit_count.to_string()),
                    ],
                };
            }
        } else if op != 0 {
            return InvariantResult::Violated {
                invariant: "commit_history_starts_at_zero".to_string(),
                message: format!("first commit should be op 0, got {op}"),
                context: vec![("op".to_string(), op.to_string())],
            };
        }

        self.last_op = Some(op);
        self.commit_count += 1;
        InvariantResult::Ok
    }

    /// Returns the last committed operation number.
    pub fn last_op(&self) -> Option<u64> {
        self.last_op
    }

    /// Returns the total number of commits recorded.
    pub fn commit_count(&self) -> u64 {
        self.commit_count
    }
}

impl Default for CommitHistoryChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for CommitHistoryChecker {
    fn name(&self) -> &'static str {
        "CommitHistoryChecker"
    }

    fn reset(&mut self) {
        self.last_op = None;
        self.commit_count = 0;
    }
}

// ============================================================================
// Client Session Checker
// ============================================================================

/// Client session information for tracking replies.
#[derive(Debug, Clone)]
pub struct ClientSession {
    /// Client identifier.
    pub client_id: u64,
    /// Last request number processed for this client.
    pub last_request_num: u64,
    /// Last reply sent to this client.
    pub last_reply: Vec<u8>,
    /// Timestamp of last update.
    pub last_update_ns: u64,
}

/// Verifies client session semantics and idempotent retry behavior.
///
/// Inspired by `TigerBeetle`'s client reply tracking, this ensures that:
/// - Clients see monotonically increasing request numbers
/// - Retried requests receive the same reply (idempotency)
/// - No request numbers are skipped
///
/// This is critical for exactly-once semantics in distributed systems.
#[derive(Debug)]
pub struct ClientSessionChecker {
    /// Client sessions: `client_id` -> session state
    sessions: std::collections::HashMap<u64, ClientSession>,
}

impl ClientSessionChecker {
    /// Creates a new client session checker.
    pub fn new() -> Self {
        Self {
            sessions: std::collections::HashMap::new(),
        }
    }

    /// Records a request and reply for a client.
    ///
    /// Returns a violation if:
    /// - Request number regresses
    /// - Request number skips ahead (gap)
    /// - Retry gets different reply
    pub fn record_request(
        &mut self,
        client_id: u64,
        request_num: u64,
        reply: Vec<u8>,
        time_ns: u64,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("client_session_monotonic");

        if let Some(session) = self.sessions.get(&client_id) {
            // Check for regression
            if request_num < session.last_request_num {
                return InvariantResult::Violated {
                    invariant: "client_session_monotonic".to_string(),
                    message: format!(
                        "client {client_id} request number regressed from {} to {request_num}",
                        session.last_request_num
                    ),
                    context: vec![
                        ("client_id".to_string(), client_id.to_string()),
                        (
                            "last_request".to_string(),
                            session.last_request_num.to_string(),
                        ),
                        ("current_request".to_string(), request_num.to_string()),
                    ],
                };
            }

            // Check for retry (same request number)
            if request_num == session.last_request_num {
                // Must return same reply for idempotency
                if reply != session.last_reply {
                    return InvariantResult::Violated {
                        invariant: "client_session_idempotent".to_string(),
                        message: format!(
                            "client {client_id} retry of request {request_num} got different reply"
                        ),
                        context: vec![
                            ("client_id".to_string(), client_id.to_string()),
                            ("request_num".to_string(), request_num.to_string()),
                            (
                                "expected_reply_len".to_string(),
                                session.last_reply.len().to_string(),
                            ),
                            ("actual_reply_len".to_string(), reply.len().to_string()),
                        ],
                    };
                }
                // Same reply is OK for retry
                return InvariantResult::Ok;
            }

            // Check for gap in request numbers
            if request_num != session.last_request_num + 1 {
                return InvariantResult::Violated {
                    invariant: "client_session_no_gaps".to_string(),
                    message: format!(
                        "client {client_id} request number gap: expected {}, got {request_num}",
                        session.last_request_num + 1
                    ),
                    context: vec![
                        ("client_id".to_string(), client_id.to_string()),
                        (
                            "last_request".to_string(),
                            session.last_request_num.to_string(),
                        ),
                        ("current_request".to_string(), request_num.to_string()),
                    ],
                };
            }
        } else if request_num != 0 {
            // First request must be 0
            return InvariantResult::Violated {
                invariant: "client_session_starts_at_zero".to_string(),
                message: format!("client {client_id} first request should be 0, got {request_num}"),
                context: vec![
                    ("client_id".to_string(), client_id.to_string()),
                    ("request_num".to_string(), request_num.to_string()),
                ],
            };
        }

        // Update session
        self.sessions.insert(
            client_id,
            ClientSession {
                client_id,
                last_request_num: request_num,
                last_reply: reply,
                last_update_ns: time_ns,
            },
        );

        InvariantResult::Ok
    }

    /// Gets the session for a client.
    pub fn get_session(&self, client_id: u64) -> Option<&ClientSession> {
        self.sessions.get(&client_id)
    }

    /// Returns the number of active client sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for ClientSessionChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for ClientSessionChecker {
    fn name(&self) -> &'static str {
        "ClientSessionChecker"
    }

    fn reset(&mut self) {
        self.sessions.clear();
    }
}

// ============================================================================
// Storage Determinism Checker
// ============================================================================

/// Verifies storage state is deterministic across replicas.
///
/// Inspired by `TigerBeetle`'s storage checker, this ensures that replicas
/// with identical operations produce byte-for-byte identical storage.
/// This catches non-deterministic storage bugs (compaction, LSM trees, etc.).
#[derive(Debug)]
pub struct StorageDeterminismChecker {
    /// Storage checksums per replica: `replica_id` -> checksum
    replica_checksums: std::collections::HashMap<u64, [u8; 32]>,
    /// Kernel state hashes per replica: `replica_id` -> state_hash
    replica_kernel_hashes: std::collections::HashMap<u64, [u8; 32]>,
    /// Last check time (for tracking).
    last_check_ns: u64,
}

impl StorageDeterminismChecker {
    /// Creates a new storage determinism checker.
    pub fn new() -> Self {
        Self {
            replica_checksums: std::collections::HashMap::new(),
            replica_kernel_hashes: std::collections::HashMap::new(),
            last_check_ns: 0,
        }
    }

    /// Records a storage checksum for a replica.
    ///
    /// Returns a violation if replicas have divergent storage.
    pub fn record_checksum(
        &mut self,
        replica_id: u64,
        checksum: [u8; 32],
        time_ns: u64,
    ) -> InvariantResult {
        // Track invariant execution
        invariant_tracker::record_invariant_execution("storage_determinism");

        self.last_check_ns = time_ns;

        // Check against all other replicas
        for (other_id, other_checksum) in &self.replica_checksums {
            if *other_id != replica_id && checksum != *other_checksum {
                return InvariantResult::Violated {
                    invariant: "storage_determinism".to_string(),
                    message: format!("replicas {other_id} and {replica_id} have divergent storage"),
                    context: vec![
                        ("replica_a".to_string(), other_id.to_string()),
                        ("checksum_a".to_string(), hex::encode(other_checksum)),
                        ("replica_b".to_string(), replica_id.to_string()),
                        ("checksum_b".to_string(), hex::encode(&checksum)),
                        ("time_ns".to_string(), time_ns.to_string()),
                    ],
                };
            }
        }

        self.replica_checksums.insert(replica_id, checksum);
        InvariantResult::Ok
    }

    /// Returns the number of replicas being tracked.
    pub fn replica_count(&self) -> usize {
        self.replica_checksums.len()
    }

    /// Returns the last check time.
    pub fn last_check_time(&self) -> u64 {
        self.last_check_ns
    }

    /// Records both storage checksum and kernel state hash for a replica.
    ///
    /// Returns a violation if replicas have divergent storage or kernel state.
    ///
    /// This is the preferred method for Phase 3+ determinism checking as it
    /// validates both storage-level and kernel-level state consistency.
    pub fn record_full_state(
        &mut self,
        replica_id: u64,
        storage_checksum: [u8; 32],
        kernel_state_hash: [u8; 32],
        time_ns: u64,
    ) -> InvariantResult {
        // Track invariant execution (both storage and kernel determinism)
        invariant_tracker::record_invariant_execution("storage_determinism");
        invariant_tracker::record_invariant_execution("kernel_state_determinism");

        self.last_check_ns = time_ns;

        // Check storage checksums against all other replicas
        for (other_id, other_checksum) in &self.replica_checksums {
            if *other_id != replica_id && storage_checksum != *other_checksum {
                return InvariantResult::Violated {
                    invariant: "storage_determinism".to_string(),
                    message: format!("replicas {other_id} and {replica_id} have divergent storage"),
                    context: vec![
                        ("replica_a".to_string(), other_id.to_string()),
                        (
                            "storage_checksum_a".to_string(),
                            hex::encode(other_checksum),
                        ),
                        ("replica_b".to_string(), replica_id.to_string()),
                        (
                            "storage_checksum_b".to_string(),
                            hex::encode(&storage_checksum),
                        ),
                        ("time_ns".to_string(), time_ns.to_string()),
                    ],
                };
            }
        }

        // Check kernel state hashes against all other replicas
        for (other_id, other_hash) in &self.replica_kernel_hashes {
            if *other_id != replica_id && kernel_state_hash != *other_hash {
                return InvariantResult::Violated {
                    invariant: "kernel_state_determinism".to_string(),
                    message: format!(
                        "replicas {other_id} and {replica_id} have divergent kernel state"
                    ),
                    context: vec![
                        ("replica_a".to_string(), other_id.to_string()),
                        ("kernel_hash_a".to_string(), hex::encode(other_hash)),
                        ("replica_b".to_string(), replica_id.to_string()),
                        ("kernel_hash_b".to_string(), hex::encode(&kernel_state_hash)),
                        ("time_ns".to_string(), time_ns.to_string()),
                    ],
                };
            }
        }

        self.replica_checksums.insert(replica_id, storage_checksum);
        self.replica_kernel_hashes
            .insert(replica_id, kernel_state_hash);
        InvariantResult::Ok
    }
}

impl Default for StorageDeterminismChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for StorageDeterminismChecker {
    fn name(&self) -> &'static str {
        "StorageDeterminismChecker"
    }

    fn reset(&mut self) {
        self.replica_checksums.clear();
        self.replica_kernel_hashes.clear();
        self.last_check_ns = 0;
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_crypto::chain_hash;

    #[test]
    fn hash_chain_checker_valid_chain() {
        let mut checker = HashChainChecker::new();

        // Genesis record
        let hash0 = chain_hash(None, b"genesis");
        let zero_hash = ChainHash::from_bytes(&[0u8; 32]);
        let result = checker.check_record(0, &zero_hash, &hash0);
        assert!(result.is_ok());

        // Second record
        let hash1 = chain_hash(Some(&hash0), b"second");
        let result = checker.check_record(1, &hash0, &hash1);
        assert!(result.is_ok());

        // Third record
        let hash2 = chain_hash(Some(&hash1), b"third");
        let result = checker.check_record(2, &hash1, &hash2);
        assert!(result.is_ok());

        assert_eq!(checker.records_checked(), 3);
    }

    #[test]
    fn hash_chain_checker_detects_broken_chain() {
        let mut checker = HashChainChecker::new();

        // Genesis record
        let hash0 = chain_hash(None, b"genesis");
        let zero_hash = ChainHash::from_bytes(&[0u8; 32]);
        checker.check_record(0, &zero_hash, &hash0);

        // Second record with WRONG prev_hash
        let wrong_prev = ChainHash::from_bytes(&[1u8; 32]);
        let hash1 = chain_hash(Some(&hash0), b"second");
        let result = checker.check_record(1, &wrong_prev, &hash1);

        assert!(!result.is_ok());
        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "hash_chain_linkage");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn hash_chain_checker_detects_offset_gap() {
        let mut checker = HashChainChecker::new();

        // Genesis record
        let hash0 = chain_hash(None, b"genesis");
        let zero_hash = ChainHash::from_bytes(&[0u8; 32]);
        checker.check_record(0, &zero_hash, &hash0);

        // Skip to offset 5 (should fail)
        let hash5 = chain_hash(Some(&hash0), b"skipped");
        let result = checker.check_record(5, &hash0, &hash5);

        assert!(!result.is_ok());
        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "hash_chain_offset_monotonic");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn hash_chain_checker_first_record_must_be_zero() {
        let mut checker = HashChainChecker::new();

        // Try to start at offset 1
        let hash = chain_hash(None, b"wrong start");
        let zero_hash = ChainHash::from_bytes(&[0u8; 32]);
        let result = checker.check_record(1, &zero_hash, &hash);

        assert!(!result.is_ok());
        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "hash_chain_starts_at_zero");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn hash_chain_checker_genesis_must_have_zero_prev() {
        let mut checker = HashChainChecker::new();

        // Genesis with non-zero prev_hash
        let hash = chain_hash(None, b"genesis");
        let non_zero_prev = ChainHash::from_bytes(&[1u8; 32]);
        let result = checker.check_record(0, &non_zero_prev, &hash);

        assert!(!result.is_ok());
        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "hash_chain_genesis");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn hash_chain_checker_reset() {
        let mut checker = HashChainChecker::new();

        // Add some records
        let hash0 = chain_hash(None, b"genesis");
        let zero_hash = ChainHash::from_bytes(&[0u8; 32]);
        checker.check_record(0, &zero_hash, &hash0);

        assert_eq!(checker.records_checked(), 1);

        // Reset
        checker.reset();

        assert_eq!(checker.records_checked(), 0);
        assert!(checker.last_offset().is_none());
        assert!(checker.last_hash().is_none());
    }

    #[test]
    fn log_consistency_checker_basic() {
        let mut checker = LogConsistencyChecker::new();

        let hash = ChainHash::from_bytes(&[1u8; 32]);
        let payload_hash = [2u8; 32];

        checker.record_commit(0, hash, payload_hash);

        // Verify matching read
        let result = checker.verify_read(0, &hash, &payload_hash);
        assert!(result.is_ok());

        // Verify unknown offset (should pass - no record)
        let result = checker.verify_read(1, &hash, &payload_hash);
        assert!(result.is_ok());
    }

    #[test]
    fn log_consistency_checker_detects_mismatch() {
        let mut checker = LogConsistencyChecker::new();

        let hash = ChainHash::from_bytes(&[1u8; 32]);
        let payload_hash = [2u8; 32];

        checker.record_commit(0, hash, payload_hash);

        // Verify with wrong chain hash
        let wrong_hash = ChainHash::from_bytes(&[3u8; 32]);
        let result = checker.verify_read(0, &wrong_hash, &payload_hash);
        assert!(!result.is_ok());

        // Verify with wrong payload hash
        let wrong_payload = [4u8; 32];
        let result = checker.verify_read(0, &hash, &wrong_payload);
        assert!(!result.is_ok());
    }

    // ========================================================================
    // Replica Consistency Checker Tests
    // ========================================================================

    #[test]
    fn replica_consistency_single_replica() {
        let mut checker = ReplicaConsistencyChecker::new();

        let result = checker.update_replica(1, 100, [1u8; 32], 1000);
        assert!(result.is_ok());

        assert_eq!(checker.replica_count(), 1);
    }

    #[test]
    fn replica_consistency_matching_replicas() {
        let mut checker = ReplicaConsistencyChecker::new();

        // Two replicas at same length with same hash
        let hash = [42u8; 32];
        assert!(checker.update_replica(1, 100, hash, 1000).is_ok());
        assert!(checker.update_replica(2, 100, hash, 1000).is_ok());

        assert!(checker.check_all().is_ok());
    }

    #[test]
    fn replica_consistency_different_lengths_ok() {
        let mut checker = ReplicaConsistencyChecker::new();

        // Replicas at different lengths can have different hashes
        assert!(checker.update_replica(1, 100, [1u8; 32], 1000).is_ok());
        assert!(checker.update_replica(2, 200, [2u8; 32], 1000).is_ok());

        assert!(checker.check_all().is_ok());
    }

    #[test]
    fn replica_consistency_detects_divergence() {
        let mut checker = ReplicaConsistencyChecker::new();

        // Two replicas at same length with DIFFERENT hashes
        assert!(checker.update_replica(1, 100, [1u8; 32], 1000).is_ok());

        let result = checker.update_replica(2, 100, [2u8; 32], 1000);
        assert!(!result.is_ok());

        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "replica_consistency");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }

        assert_eq!(checker.violation_count(), 1);
    }

    #[test]
    fn replica_consistency_check_all_detects_divergence() {
        let mut checker = ReplicaConsistencyChecker::new();

        // Add replicas without checking (simulating batch update)
        checker.replicas.insert(
            1,
            ReplicaState {
                replica_id: 1,
                log_length: 100,
                log_hash: [1u8; 32],
                last_update_ns: 1000,
            },
        );
        checker.replicas.insert(
            2,
            ReplicaState {
                replica_id: 2,
                log_length: 100,
                log_hash: [2u8; 32],
                last_update_ns: 1000,
            },
        );

        assert!(!checker.check_all().is_ok());
    }

    #[test]
    fn replica_consistency_three_replicas() {
        let mut checker = ReplicaConsistencyChecker::new();

        let hash = [42u8; 32];
        assert!(checker.update_replica(1, 100, hash, 1000).is_ok());
        assert!(checker.update_replica(2, 100, hash, 1000).is_ok());
        assert!(checker.update_replica(3, 100, hash, 1000).is_ok());

        assert!(checker.check_all().is_ok());
        assert_eq!(checker.replica_count(), 3);
    }

    #[test]
    fn replica_consistency_reset() {
        let mut checker = ReplicaConsistencyChecker::new();

        checker.update_replica(1, 100, [1u8; 32], 1000);
        let _ = checker.update_replica(2, 100, [2u8; 32], 1000);

        assert_eq!(checker.replica_count(), 2);
        assert_eq!(checker.violation_count(), 1);

        checker.reset();

        assert_eq!(checker.replica_count(), 0);
        assert_eq!(checker.violation_count(), 0);
    }

    #[test]
    fn replica_consistency_get_replica() {
        let mut checker = ReplicaConsistencyChecker::new();

        checker.update_replica(1, 100, [42u8; 32], 1000);

        let state = checker.get_replica(1).expect("replica should exist");
        assert_eq!(state.replica_id, 1);
        assert_eq!(state.log_length, 100);
        assert_eq!(state.log_hash, [42u8; 32]);

        assert!(checker.get_replica(999).is_none());
    }

    // ========================================================================
    // Replica Head Checker Tests
    // ========================================================================

    #[test]
    fn replica_head_checker_basic() {
        let mut checker = ReplicaHeadChecker::new();

        // Initial update should succeed
        let result = checker.update_head(1, 0, 0);
        assert!(result.is_ok());

        // Forward progress should succeed
        let result = checker.update_head(1, 0, 1);
        assert!(result.is_ok());

        let result = checker.update_head(1, 1, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn replica_head_checker_detects_view_regression() {
        let mut checker = ReplicaHeadChecker::new();

        checker.update_head(1, 2, 5);

        // View regression should fail
        let result = checker.update_head(1, 1, 10);
        assert!(!result.is_ok());

        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "replica_head_progress");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn replica_head_checker_detects_op_regression() {
        let mut checker = ReplicaHeadChecker::new();

        checker.update_head(1, 0, 10);

        // Op regression in same view should fail
        let result = checker.update_head(1, 0, 5);
        assert!(!result.is_ok());
    }

    #[test]
    fn replica_head_checker_allows_same_position() {
        let mut checker = ReplicaHeadChecker::new();

        checker.update_head(1, 0, 5);

        // Same position should be allowed (idempotent updates)
        let result = checker.update_head(1, 0, 5);
        assert!(result.is_ok());
    }

    #[test]
    fn replica_head_checker_multiple_replicas() {
        let mut checker = ReplicaHeadChecker::new();

        // Different replicas are independent
        assert!(checker.update_head(1, 0, 10).is_ok());
        assert!(checker.update_head(2, 0, 5).is_ok());
        assert!(checker.update_head(3, 1, 0).is_ok());

        assert_eq!(checker.replica_count(), 3);
        assert_eq!(checker.get_head(1), Some((0, 10)));
        assert_eq!(checker.get_head(2), Some((0, 5)));
        assert_eq!(checker.get_head(3), Some((1, 0)));
    }

    #[test]
    fn replica_head_checker_reset() {
        let mut checker = ReplicaHeadChecker::new();

        checker.update_head(1, 0, 10);
        assert_eq!(checker.replica_count(), 1);

        checker.reset();

        assert_eq!(checker.replica_count(), 0);
        assert_eq!(checker.get_head(1), None);
    }

    // ========================================================================
    // Commit History Checker Tests
    // ========================================================================

    #[test]
    fn commit_history_checker_basic() {
        let mut checker = CommitHistoryChecker::new();

        // First commit must be 0
        let result = checker.record_commit(0);
        assert!(result.is_ok());

        // Sequential commits should succeed
        let result = checker.record_commit(1);
        assert!(result.is_ok());

        let result = checker.record_commit(2);
        assert!(result.is_ok());

        assert_eq!(checker.commit_count(), 3);
        assert_eq!(checker.last_op(), Some(2));
    }

    #[test]
    fn commit_history_checker_must_start_at_zero() {
        let mut checker = CommitHistoryChecker::new();

        // Starting at non-zero should fail
        let result = checker.record_commit(1);
        assert!(!result.is_ok());

        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "commit_history_starts_at_zero");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn commit_history_checker_detects_gap() {
        let mut checker = CommitHistoryChecker::new();

        checker.record_commit(0);
        checker.record_commit(1);

        // Skip to 5 (should fail)
        let result = checker.record_commit(5);
        assert!(!result.is_ok());

        match result {
            InvariantResult::Violated {
                invariant, message, ..
            } => {
                assert_eq!(invariant, "commit_history_monotonic");
                assert!(message.contains("expected op 2"));
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn commit_history_checker_detects_duplicate() {
        let mut checker = CommitHistoryChecker::new();

        checker.record_commit(0);
        checker.record_commit(1);

        // Try to commit 1 again (should fail)
        let result = checker.record_commit(1);
        assert!(!result.is_ok());
    }

    #[test]
    fn commit_history_checker_reset() {
        let mut checker = CommitHistoryChecker::new();

        checker.record_commit(0);
        checker.record_commit(1);

        assert_eq!(checker.commit_count(), 2);

        checker.reset();

        assert_eq!(checker.commit_count(), 0);
        assert_eq!(checker.last_op(), None);
    }

    // ========================================================================
    // Client Session Checker Tests
    // ========================================================================

    #[test]
    fn client_session_checker_basic() {
        let mut checker = ClientSessionChecker::new();

        // First request must be 0
        let result = checker.record_request(1, 0, b"reply0".to_vec(), 1000);
        assert!(result.is_ok());

        // Sequential requests should succeed
        let result = checker.record_request(1, 1, b"reply1".to_vec(), 2000);
        assert!(result.is_ok());

        let result = checker.record_request(1, 2, b"reply2".to_vec(), 3000);
        assert!(result.is_ok());

        assert_eq!(checker.session_count(), 1);
    }

    #[test]
    fn client_session_checker_must_start_at_zero() {
        let mut checker = ClientSessionChecker::new();

        // Starting at non-zero should fail
        let result = checker.record_request(1, 5, b"reply".to_vec(), 1000);
        assert!(!result.is_ok());

        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "client_session_starts_at_zero");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn client_session_checker_detects_regression() {
        let mut checker = ClientSessionChecker::new();

        checker.record_request(1, 0, b"reply0".to_vec(), 1000);
        checker.record_request(1, 1, b"reply1".to_vec(), 2000);
        checker.record_request(1, 2, b"reply2".to_vec(), 3000);

        // Now at request 2. Retrying request 2 is OK (current request)
        let result = checker.record_request(1, 2, b"reply2".to_vec(), 4000);
        assert!(result.is_ok());

        // But going back to request 1 should fail (regression to old request)
        let result = checker.record_request(1, 1, b"reply1".to_vec(), 5000);
        assert!(!result.is_ok());

        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "client_session_monotonic");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn client_session_checker_detects_gap() {
        let mut checker = ClientSessionChecker::new();

        checker.record_request(1, 0, b"reply0".to_vec(), 1000);

        // Skip to request 5 (should fail)
        let result = checker.record_request(1, 5, b"reply5".to_vec(), 2000);
        assert!(!result.is_ok());

        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "client_session_no_gaps");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn client_session_checker_idempotent_retry() {
        let mut checker = ClientSessionChecker::new();

        checker.record_request(1, 0, b"reply0".to_vec(), 1000);

        // Retry same request with same reply should succeed
        let result = checker.record_request(1, 0, b"reply0".to_vec(), 2000);
        assert!(result.is_ok());

        // Retry same request with different reply should fail
        let result = checker.record_request(1, 0, b"different".to_vec(), 3000);
        assert!(!result.is_ok());

        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "client_session_idempotent");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn client_session_checker_multiple_clients() {
        let mut checker = ClientSessionChecker::new();

        // Different clients are independent
        assert!(
            checker
                .record_request(1, 0, b"c1_r0".to_vec(), 1000)
                .is_ok()
        );
        assert!(
            checker
                .record_request(2, 0, b"c2_r0".to_vec(), 1000)
                .is_ok()
        );
        assert!(
            checker
                .record_request(3, 0, b"c3_r0".to_vec(), 1000)
                .is_ok()
        );

        assert!(
            checker
                .record_request(1, 1, b"c1_r1".to_vec(), 2000)
                .is_ok()
        );
        assert!(
            checker
                .record_request(2, 1, b"c2_r1".to_vec(), 2000)
                .is_ok()
        );

        assert_eq!(checker.session_count(), 3);

        let session1 = checker.get_session(1).expect("should have session");
        assert_eq!(session1.last_request_num, 1);
        assert_eq!(session1.last_reply, b"c1_r1");
    }

    #[test]
    fn client_session_checker_get_session() {
        let mut checker = ClientSessionChecker::new();

        checker.record_request(1, 0, b"reply".to_vec(), 1000);

        let session = checker.get_session(1).expect("should have session");
        assert_eq!(session.client_id, 1);
        assert_eq!(session.last_request_num, 0);
        assert_eq!(session.last_reply, b"reply");

        assert!(checker.get_session(999).is_none());
    }

    #[test]
    fn client_session_checker_reset() {
        let mut checker = ClientSessionChecker::new();

        checker.record_request(1, 0, b"reply".to_vec(), 1000);
        assert_eq!(checker.session_count(), 1);

        checker.reset();

        assert_eq!(checker.session_count(), 0);
        assert!(checker.get_session(1).is_none());
    }

    // ========================================================================
    // Storage Determinism Checker Tests
    // ========================================================================

    #[test]
    fn storage_determinism_checker_basic() {
        let mut checker = StorageDeterminismChecker::new();

        let checksum = [42u8; 32];
        let result = checker.record_checksum(1, checksum, 1000);
        assert!(result.is_ok());

        assert_eq!(checker.replica_count(), 1);
        assert_eq!(checker.last_check_time(), 1000);
    }

    #[test]
    fn storage_determinism_checker_matching_replicas() {
        let mut checker = StorageDeterminismChecker::new();

        let checksum = [42u8; 32];

        // All replicas with same checksum should be OK
        assert!(checker.record_checksum(1, checksum, 1000).is_ok());
        assert!(checker.record_checksum(2, checksum, 2000).is_ok());
        assert!(checker.record_checksum(3, checksum, 3000).is_ok());

        assert_eq!(checker.replica_count(), 3);
    }

    #[test]
    fn storage_determinism_checker_detects_divergence() {
        let mut checker = StorageDeterminismChecker::new();

        let checksum1 = [1u8; 32];
        let checksum2 = [2u8; 32];

        assert!(checker.record_checksum(1, checksum1, 1000).is_ok());

        // Different checksum should fail
        let result = checker.record_checksum(2, checksum2, 2000);
        assert!(!result.is_ok());

        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "storage_determinism");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn storage_determinism_checker_reset() {
        let mut checker = StorageDeterminismChecker::new();

        checker.record_checksum(1, [42u8; 32], 1000);
        assert_eq!(checker.replica_count(), 1);

        checker.reset();

        assert_eq!(checker.replica_count(), 0);
        assert_eq!(checker.last_check_time(), 0);
    }

    #[test]
    fn storage_determinism_checker_full_state_identical() {
        let mut checker = StorageDeterminismChecker::new();

        let storage_hash = [1u8; 32];
        let kernel_hash = [2u8; 32];

        // First replica
        let result1 = checker.record_full_state(1, storage_hash, kernel_hash, 1000);
        assert!(result1.is_ok());

        // Second replica with same state
        let result2 = checker.record_full_state(2, storage_hash, kernel_hash, 2000);
        assert!(result2.is_ok());
    }

    #[test]
    fn storage_determinism_checker_full_state_divergent_storage() {
        let mut checker = StorageDeterminismChecker::new();

        let storage_hash1 = [1u8; 32];
        let storage_hash2 = [2u8; 32]; // Different
        let kernel_hash = [10u8; 32];

        // First replica
        checker.record_full_state(1, storage_hash1, kernel_hash, 1000);

        // Second replica with divergent storage
        let result = checker.record_full_state(2, storage_hash2, kernel_hash, 2000);
        assert!(!result.is_ok());

        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "storage_determinism");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn storage_determinism_checker_full_state_divergent_kernel() {
        let mut checker = StorageDeterminismChecker::new();

        let storage_hash = [1u8; 32];
        let kernel_hash1 = [10u8; 32];
        let kernel_hash2 = [20u8; 32]; // Different

        // First replica
        checker.record_full_state(1, storage_hash, kernel_hash1, 1000);

        // Second replica with divergent kernel state
        let result = checker.record_full_state(2, storage_hash, kernel_hash2, 2000);
        assert!(!result.is_ok());

        match result {
            InvariantResult::Violated { invariant, .. } => {
                assert_eq!(invariant, "kernel_state_determinism");
            }
            InvariantResult::Ok => panic!("expected violation"),
        }
    }

    #[test]
    fn storage_determinism_checker_full_state_multiple_replicas() {
        let mut checker = StorageDeterminismChecker::new();

        let storage_hash = [1u8; 32];
        let kernel_hash = [2u8; 32];

        // Add 3 replicas with identical state
        assert!(
            checker
                .record_full_state(0, storage_hash, kernel_hash, 1000)
                .is_ok()
        );
        assert!(
            checker
                .record_full_state(1, storage_hash, kernel_hash, 2000)
                .is_ok()
        );
        assert!(
            checker
                .record_full_state(2, storage_hash, kernel_hash, 3000)
                .is_ok()
        );

        assert_eq!(checker.replica_count(), 3);
    }
}
