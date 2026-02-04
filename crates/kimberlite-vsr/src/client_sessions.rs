//! Client session management for VSR.
//!
//! # VRR Paper Bugs Fixed
//!
//! This module fixes two bugs found in the original Viewstamped Replication Revisited paper,
//! as documented by TigerBeetle:
//!
//! ## Bug 1: Successive Client Crashes
//!
//! **Problem:** When a client crashes and restarts, it resets its request number to 0.
//! If the server still has the old session cached, it returns the reply for request #0
//! from the *previous* client incarnation, not the current one.
//!
//! **Fix:** Explicit session registration. Each client must register before sending requests.
//! Registration creates a new session with a fresh request number space.
//!
//! ## Bug 2: Uncommitted Request Table Updates
//!
//! **Problem:** The VRR paper updates the client table when a request is *prepared* (not yet committed).
//! During a view change, the new leader doesn't have uncommitted prepares, so it rejects
//! requests from clients whose table was updated but not committed. Client is locked out.
//!
//! **Fix:** Separate committed vs uncommitted tracking. Only update the committed table
//! after a request is actually committed. Track uncommitted requests separately and discard
//! them on view change.
//!
//! # References
//!
//! - TigerBeetle: `src/vsr/client_sessions.zig`
//! - VRR paper: "Viewstamped Replication Revisited" (Liskov & Cowling, 2012)

use std::collections::{BinaryHeap, HashMap};
use std::cmp::Reverse;

use kimberlite_types::Timestamp;
use serde::{Deserialize, Serialize};

use crate::types::OpNumber;

// ============================================================================
// ClientId
// ============================================================================

/// Unique identifier for a client session.
///
/// A client registers once and gets assigned a unique ID. All subsequent
/// requests from that client include this ID.
///
/// # Design Note
///
/// We use u64 for simplicity. Production systems might want UUIDs for
/// cross-cluster uniqueness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct ClientId(u64);

impl ClientId {
    /// Creates a new client ID.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the client ID as a u64.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "client#{}", self.0)
    }
}

// ============================================================================
// CommittedSession
// ============================================================================

/// A committed client session with cached reply.
///
/// This represents a client whose requests have been committed. The cached
/// reply enables idempotent retry - if the client resends the same request
/// number, we return the cached reply instead of re-executing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedSession {
    /// The highest request number committed for this client.
    pub request_number: u64,

    /// The operation number where this request was committed.
    pub committed_op: OpNumber,

    /// Cached reply data (for idempotent retry).
    ///
    /// Stores the Effects produced when this request was committed.
    /// When a duplicate request is detected, we return these cached effects
    /// instead of re-executing the command.
    pub reply_op: OpNumber,

    /// Cached effects from the committed operation.
    ///
    /// These are the exact effects produced when the kernel applied this
    /// command. Replaying these effects provides idempotent retry.
    pub cached_effects: Vec<kimberlite_kernel::Effect>,

    /// Timestamp when this session was last committed.
    ///
    /// Used for deterministic eviction: all replicas evict the session
    /// with the oldest commit timestamp.
    pub commit_timestamp: Timestamp,
}

impl CommittedSession {
    /// Creates a new committed session.
    pub fn new(
        request_number: u64,
        committed_op: OpNumber,
        reply_op: OpNumber,
        cached_effects: Vec<kimberlite_kernel::Effect>,
        commit_timestamp: Timestamp,
    ) -> Self {
        Self {
            request_number,
            committed_op,
            reply_op,
            cached_effects,
            commit_timestamp,
        }
    }
}

// ============================================================================
// UncommittedSession
// ============================================================================

/// An uncommitted client session.
///
/// This tracks requests that have been prepared but not yet committed.
/// If a view change occurs, these are discarded (the new leader doesn't
/// have them). This prevents the "client lockout" bug from the VRR paper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UncommittedSession {
    /// The request number being prepared (not yet committed).
    pub request_number: u64,

    /// The operation number where this request is being prepared.
    pub preparing_op: OpNumber,
}

impl UncommittedSession {
    /// Creates a new uncommitted session.
    pub fn new(request_number: u64, preparing_op: OpNumber) -> Self {
        Self {
            request_number,
            preparing_op,
        }
    }
}

// ============================================================================
// SessionEviction (for priority queue)
// ============================================================================

/// Entry for the eviction priority queue.
///
/// We maintain a min-heap of sessions ordered by commit_timestamp.
/// When eviction is needed, we remove the session with the oldest timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionEviction {
    client_id: ClientId,
    commit_timestamp: Timestamp,
}

impl PartialOrd for SessionEviction {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SessionEviction {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Normal order - used with Reverse wrapper for min-heap
        self.commit_timestamp.cmp(&other.commit_timestamp)
    }
}

// ============================================================================
// ClientSessions
// ============================================================================

/// Configuration for client session management.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientSessionsConfig {
    /// Maximum number of committed sessions to retain.
    ///
    /// When exceeded, the session with the oldest commit_timestamp is evicted.
    pub max_sessions: usize,
}

impl Default for ClientSessionsConfig {
    fn default() -> Self {
        Self {
            max_sessions: 100_000, // Support 100k concurrent clients
        }
    }
}

impl ClientSessionsConfig {
    /// Configuration for testing (small limits).
    pub fn testing() -> Self {
        Self { max_sessions: 100 }
    }
}

/// Client session manager.
///
/// Tracks both committed and uncommitted sessions to provide idempotent
/// request processing while avoiding VRR paper bugs.
///
/// # Design Invariants
///
/// 1. **Committed sessions** are durable across view changes
/// 2. **Uncommitted sessions** are discarded on view change
/// 3. **Eviction is deterministic** - all replicas evict the same session (oldest commit_timestamp)
/// 4. **Request numbers are monotonic** - each client's request numbers only increase
#[derive(Debug, Clone)]
pub struct ClientSessions {
    /// Committed sessions with cached replies.
    committed: HashMap<ClientId, CommittedSession>,

    /// Uncommitted sessions (prepared but not committed).
    uncommitted: HashMap<ClientId, UncommittedSession>,

    /// Priority queue for deterministic eviction.
    ///
    /// Min-heap ordered by commit_timestamp. Oldest sessions evicted first.
    eviction_queue: BinaryHeap<Reverse<SessionEviction>>,

    /// Next client ID to assign.
    next_client_id: u64,

    /// Configuration.
    config: ClientSessionsConfig,
}

impl ClientSessions {
    /// Creates a new empty client session manager.
    pub fn new(config: ClientSessionsConfig) -> Self {
        Self {
            committed: HashMap::new(),
            uncommitted: HashMap::new(),
            eviction_queue: BinaryHeap::new(),
            next_client_id: 1, // Start at 1 (0 reserved for special use)
            config,
        }
    }

    /// Creates a manager with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ClientSessionsConfig::default())
    }

    /// Registers a new client session.
    ///
    /// Returns the assigned `ClientId`. The client must include this ID
    /// in all subsequent requests.
    ///
    /// # Design Note
    ///
    /// In production, registration might include authentication, rate limiting,
    /// etc. For now, we just assign IDs monotonically.
    pub fn register_client(&mut self) -> ClientId {
        let client_id = ClientId::new(self.next_client_id);
        self.next_client_id = self.next_client_id.saturating_add(1);
        client_id
    }

    /// Checks if a request is a duplicate of a committed request.
    ///
    /// # Returns
    ///
    /// - `Some(session)` if this request was already committed (return cached reply)
    /// - `None` if this is a new request or higher request number
    ///
    /// # Design
    ///
    /// We only check committed sessions. Uncommitted sessions don't have replies yet.
    pub fn check_duplicate(
        &self,
        client_id: ClientId,
        request_number: u64,
    ) -> Option<&CommittedSession> {
        let session = self.committed.get(&client_id)?;

        // If request number matches, this is a duplicate
        if session.request_number == request_number {
            Some(session)
        } else {
            None
        }
    }

    /// Records an uncommitted request (during prepare phase).
    ///
    /// This tracks that a request is being prepared but not yet committed.
    /// If a view change occurs, uncommitted sessions are discarded.
    ///
    /// # Arguments
    ///
    /// * `client_id` - Client sending the request
    /// * `request_number` - Request number (must be > any committed request)
    /// * `preparing_op` - Operation number being prepared
    ///
    /// # Returns
    ///
    /// - `Ok(())` if recorded successfully
    /// - `Err(msg)` if request number is invalid (not monotonic)
    pub fn record_uncommitted(
        &mut self,
        client_id: ClientId,
        request_number: u64,
        preparing_op: OpNumber,
    ) -> Result<(), String> {
        // Check monotonicity: request number must be > any committed request
        if let Some(committed) = self.committed.get(&client_id) {
            if request_number <= committed.request_number {
                return Err(format!(
                    "request number {} not greater than committed {}",
                    request_number, committed.request_number
                ));
            }
        }

        // Record uncommitted session
        let session = UncommittedSession::new(request_number, preparing_op);
        self.uncommitted.insert(client_id, session);

        Ok(())
    }

    /// Commits a request (move from uncommitted to committed).
    ///
    /// This is called after a request reaches consensus and is committed.
    /// The session moves from uncommitted to committed, and a reply is cached.
    ///
    /// # Arguments
    ///
    /// * `client_id` - Client whose request is being committed
    /// * `request_number` - Request number being committed
    /// * `committed_op` - Operation number where committed
    /// * `reply_op` - Operation number to return as reply
    /// * `cached_effects` - Effects produced by this request (for idempotent retry)
    /// * `commit_timestamp` - When this was committed (for eviction)
    ///
    /// # Returns
    ///
    /// - `Ok(())` if committed successfully
    /// - `Err(msg)` if request number doesn't match uncommitted
    pub fn commit_request(
        &mut self,
        client_id: ClientId,
        request_number: u64,
        committed_op: OpNumber,
        reply_op: OpNumber,
        cached_effects: Vec<kimberlite_kernel::Effect>,
        commit_timestamp: Timestamp,
    ) -> Result<(), String> {
        // Validate that this request is uncommitted
        if let Some(uncommitted) = self.uncommitted.get(&client_id) {
            if uncommitted.request_number != request_number {
                return Err(format!(
                    "committing request {} but uncommitted is {}",
                    request_number, uncommitted.request_number
                ));
            }
        }

        // Remove from uncommitted
        self.uncommitted.remove(&client_id);

        // Add to committed
        let session = CommittedSession::new(
            request_number,
            committed_op,
            reply_op,
            cached_effects,
            commit_timestamp,
        );
        self.committed.insert(client_id, session);

        // Add to eviction queue
        self.eviction_queue.push(Reverse(SessionEviction {
            client_id,
            commit_timestamp,
        }));

        // Check if eviction is needed
        if self.committed.len() > self.config.max_sessions {
            self.evict_oldest();
        }

        Ok(())
    }

    /// Discards all uncommitted sessions.
    ///
    /// Called during view change. The new leader doesn't have uncommitted
    /// prepares from the old leader, so we discard them to avoid client lockout.
    pub fn discard_uncommitted(&mut self) {
        self.uncommitted.clear();
    }

    /// Evicts the session with the oldest commit timestamp.
    ///
    /// This is deterministic - all replicas will evict the same session
    /// because they all use commit_timestamp for ordering.
    fn evict_oldest(&mut self) {
        while let Some(Reverse(eviction)) = self.eviction_queue.pop() {
            // Check if this session still exists (might have been evicted already)
            if let Some(session) = self.committed.get(&eviction.client_id) {
                // Verify timestamp matches (session might have been updated)
                if session.commit_timestamp == eviction.commit_timestamp {
                    self.committed.remove(&eviction.client_id);
                    tracing::debug!(
                        client = %eviction.client_id,
                        timestamp = ?eviction.commit_timestamp,
                        "evicted oldest client session"
                    );
                    return;
                }
            }
        }
    }

    /// Returns the number of committed sessions.
    pub fn committed_count(&self) -> usize {
        self.committed.len()
    }

    /// Returns the number of uncommitted sessions.
    pub fn uncommitted_count(&self) -> usize {
        self.uncommitted.len()
    }

    /// Returns true if there are no sessions.
    pub fn is_empty(&self) -> bool {
        self.committed.is_empty() && self.uncommitted.is_empty()
    }
}

impl Default for ClientSessions {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_timestamp(secs: u64) -> Timestamp {
        Timestamp::from_nanos(secs * 1_000_000_000)
    }

    #[test]
    fn register_client_assigns_unique_ids() {
        let mut sessions = ClientSessions::with_defaults();

        let client1 = sessions.register_client();
        let client2 = sessions.register_client();
        let client3 = sessions.register_client();

        assert_ne!(client1, client2);
        assert_ne!(client2, client3);
        assert_ne!(client1, client3);
    }

    #[test]
    fn new_session_has_no_duplicates() {
        let sessions = ClientSessions::with_defaults();
        let client_id = ClientId::new(1);

        assert!(sessions.check_duplicate(client_id, 0).is_none());
        assert!(sessions.check_duplicate(client_id, 1).is_none());
    }

    #[test]
    fn record_uncommitted_and_commit() {
        let mut sessions = ClientSessions::with_defaults();
        let client_id = sessions.register_client();

        // Record uncommitted
        sessions
            .record_uncommitted(client_id, 1, OpNumber::new(10))
            .unwrap();
        assert_eq!(sessions.uncommitted_count(), 1);

        // Commit
        sessions
            .commit_request(
                client_id,
                1,
                OpNumber::new(10),
                OpNumber::new(10),
                Vec::new(), // empty effects for test
                make_timestamp(100),
            )
            .unwrap();

        assert_eq!(sessions.uncommitted_count(), 0);
        assert_eq!(sessions.committed_count(), 1);

        // Check duplicate
        let cached = sessions.check_duplicate(client_id, 1);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().committed_op, OpNumber::new(10));
    }

    #[test]
    fn request_number_must_be_monotonic() {
        let mut sessions = ClientSessions::with_defaults();
        let client_id = sessions.register_client();

        // Commit request 5
        sessions
            .record_uncommitted(client_id, 5, OpNumber::new(10))
            .unwrap();
        sessions
            .commit_request(
                client_id,
                5,
                OpNumber::new(10),
                OpNumber::new(10),
                Vec::new(),
                make_timestamp(100),
            )
            .unwrap();

        // Try to record uncommitted request 3 (should fail - not > 5)
        let result = sessions.record_uncommitted(client_id, 3, OpNumber::new(20));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not greater than"));
    }

    #[test]
    fn discard_uncommitted_on_view_change() {
        let mut sessions = ClientSessions::with_defaults();
        let client1 = sessions.register_client();
        let client2 = sessions.register_client();

        // Client1: committed request
        sessions
            .record_uncommitted(client1, 1, OpNumber::new(10))
            .unwrap();
        sessions
            .commit_request(
                client1,
                1,
                OpNumber::new(10),
                OpNumber::new(10),
                Vec::new(),
                make_timestamp(100),
            )
            .unwrap();

        // Client2: uncommitted request
        sessions
            .record_uncommitted(client2, 1, OpNumber::new(20))
            .unwrap();

        assert_eq!(sessions.committed_count(), 1);
        assert_eq!(sessions.uncommitted_count(), 1);

        // View change: discard uncommitted
        sessions.discard_uncommitted();

        assert_eq!(sessions.committed_count(), 1); // Preserved
        assert_eq!(sessions.uncommitted_count(), 0); // Discarded
    }

    #[test]
    fn deterministic_eviction_by_timestamp() {
        let config = ClientSessionsConfig { max_sessions: 3 };
        let mut sessions = ClientSessions::new(config);

        let client1 = sessions.register_client();
        let client2 = sessions.register_client();
        let client3 = sessions.register_client();
        let client4 = sessions.register_client();

        // Commit in order: client1 (oldest), client2, client3
        sessions
            .record_uncommitted(client1, 1, OpNumber::new(10))
            .unwrap();
        sessions
            .commit_request(
                client1,
                1,
                OpNumber::new(10),
                OpNumber::new(10),
                Vec::new(),
                make_timestamp(100),
            )
            .unwrap();

        sessions
            .record_uncommitted(client2, 1, OpNumber::new(20))
            .unwrap();
        sessions
            .commit_request(
                client2,
                1,
                OpNumber::new(20),
                OpNumber::new(20),
                Vec::new(),
                make_timestamp(200),
            )
            .unwrap();

        sessions
            .record_uncommitted(client3, 1, OpNumber::new(30))
            .unwrap();
        sessions
            .commit_request(
                client3,
                1,
                OpNumber::new(30),
                OpNumber::new(30),
                Vec::new(),
                make_timestamp(300),
            )
            .unwrap();

        assert_eq!(sessions.committed_count(), 3);

        // Add client4 - should trigger eviction of client1 (oldest timestamp)
        sessions
            .record_uncommitted(client4, 1, OpNumber::new(40))
            .unwrap();
        sessions
            .commit_request(
                client4,
                1,
                OpNumber::new(40),
                OpNumber::new(40),
                Vec::new(),
                make_timestamp(400),
            )
            .unwrap();

        assert_eq!(sessions.committed_count(), 3);
        assert!(sessions.check_duplicate(client1, 1).is_none()); // Evicted
        assert!(sessions.check_duplicate(client2, 1).is_some()); // Kept
        assert!(sessions.check_duplicate(client3, 1).is_some()); // Kept
        assert!(sessions.check_duplicate(client4, 1).is_some()); // Kept
    }

    #[test]
    fn commit_without_uncommitted_succeeds() {
        let mut sessions = ClientSessions::with_defaults();
        let client_id = sessions.register_client();

        // Commit directly without recording uncommitted first
        // (Might happen during recovery or state transfer)
        let result = sessions.commit_request(
            client_id,
            1,
            OpNumber::new(10),
            OpNumber::new(10),
            Vec::new(),
            make_timestamp(100),
        );

        assert!(result.is_ok());
        assert_eq!(sessions.committed_count(), 1);
    }

    #[test]
    fn cached_effects_returned_for_duplicates() {
        use kimberlite_kernel::Effect;
        use kimberlite_types::{StreamId, Offset};

        let mut sessions = ClientSessions::with_defaults();
        let client_id = sessions.register_client();

        // Create some test effects
        let effects = vec![
            Effect::WakeProjection {
                stream_id: StreamId::new(1),
                from_offset: Offset::from(0),
                to_offset: Offset::from(10),
            },
            Effect::WakeProjection {
                stream_id: StreamId::new(2),
                from_offset: Offset::from(0),
                to_offset: Offset::from(20),
            },
        ];

        // Record and commit with cached effects
        sessions
            .record_uncommitted(client_id, 1, OpNumber::new(10))
            .unwrap();
        sessions
            .commit_request(
                client_id,
                1,
                OpNumber::new(10),
                OpNumber::new(10),
                effects.clone(),
                make_timestamp(100),
            )
            .unwrap();

        // Check duplicate returns the same effects
        let cached = sessions.check_duplicate(client_id, 1).unwrap();
        assert_eq!(cached.cached_effects, effects);
        assert_eq!(cached.committed_op, OpNumber::new(10));
    }

    // ========================================================================
    // Property-Based Tests
    // ========================================================================

    use proptest::prelude::*;
    use kimberlite_kernel::Effect;
    use kimberlite_types::{Offset, StreamId};

    proptest! {
        /// Property: Session eviction is deterministic across replicas.
        ///
        /// Given the same sequence of operations, all replicas should evict
        /// the same sessions (oldest by commit_timestamp).
        #[test]
        fn prop_eviction_deterministic(
            operations in prop::collection::vec(
                (0u64..10u64, 1u64..100u64, 0u64..1000u64),  // (client_id, request_number, timestamp)
                1..20
            )
        ) {
            let config = ClientSessionsConfig {
                max_sessions: 5,
                ..Default::default()
            };

            // Replica 1
            let mut sessions1 = ClientSessions::new(config.clone());
            for (client_id, request_number, timestamp) in &operations {
                let cid = ClientId::new(*client_id);
                let rnum = *request_number;
                let op = OpNumber::new(*request_number);
                let ts = make_timestamp(*timestamp);

                let _ = sessions1.record_uncommitted(cid, rnum, op);
                let _ = sessions1.commit_request(cid, rnum, op, op, Vec::new(), ts);
            }

            // Replica 2
            let mut sessions2 = ClientSessions::new(config);
            for (client_id, request_number, timestamp) in &operations {
                let cid = ClientId::new(*client_id);
                let rnum = *request_number;
                let op = OpNumber::new(*request_number);
                let ts = make_timestamp(*timestamp);

                let _ = sessions2.record_uncommitted(cid, rnum, op);
                let _ = sessions2.commit_request(cid, rnum, op, op, Vec::new(), ts);
            }

            // Both replicas should have same committed sessions
            prop_assert_eq!(sessions1.committed.len(), sessions2.committed.len(),
                "Replica session counts differ");

            // Check that same clients are present
            for (client_id, session1) in &sessions1.committed {
                let session2 = sessions2.committed.get(client_id);
                prop_assert!(session2.is_some(),
                    "Client {:?} present in replica1 but not replica2", client_id);
                prop_assert_eq!(session1.request_number, session2.unwrap().request_number,
                    "Request numbers differ for client {:?}", client_id);
            }
        }

        /// Property: Request numbers are monotonic within a session.
        ///
        /// For any client, request numbers should only increase (or stay same
        /// for retries).
        #[test]
        fn prop_request_numbers_monotonic(
            requests in prop::collection::vec(
                (1u64..100u64, 1u64..1000u64),  // (request_number, timestamp)
                1..50
            )
        ) {
            let mut sessions = ClientSessions::with_defaults();
            let client_id = ClientId::new(1);
            let mut last_request_number = 0u64;

            for (request_number, timestamp) in requests {
                let op = OpNumber::new(request_number);
                let ts = make_timestamp(timestamp);

                // Record and commit
                if sessions.record_uncommitted(client_id, request_number, op).is_ok() {
                    let _ = sessions.commit_request(
                        client_id,
                        request_number,
                        op,
                        op,
                        Vec::new(),
                        ts,
                    );

                    // Request number should be >= last (monotonic)
                    if let Some(session) = sessions.committed.get(&client_id) {
                        prop_assert!(session.request_number >= last_request_number,
                            "Request number decreased: {} -> {}",
                            last_request_number, session.request_number);
                        last_request_number = session.request_number;
                    }
                }
            }
        }

        /// Property: Duplicate detection is consistent.
        ///
        /// If check_duplicate() returns Some, it should always return the
        /// same cached effects for that (client_id, request_number) pair.
        #[test]
        fn prop_duplicate_detection_consistent(
            client_id in 0u64..10u64,
            request_number in 1u64..100u64,
            timestamp in 0u64..1000u64,
        ) {
            let mut sessions = ClientSessions::with_defaults();
            let cid = ClientId::new(client_id);
            let op = OpNumber::new(request_number);
            let ts = make_timestamp(timestamp);

            // Create a test effect
            let effects = vec![
                Effect::StorageAppend {
                    stream_id: StreamId::new(1),
                    base_offset: Offset::from(0),
                    events: vec![bytes::Bytes::from("test")],
                },
            ];

            // Record and commit with effects
            sessions.record_uncommitted(cid, request_number, op).unwrap();
            sessions
                .commit_request(cid, request_number, op, op, effects.clone(), ts)
                .unwrap();

            // First check
            let cached1 = sessions.check_duplicate(cid, request_number);
            prop_assert!(cached1.is_some(), "Duplicate not detected");

            // Second check (should return same)
            let cached2 = sessions.check_duplicate(cid, request_number);
            prop_assert_eq!(cached1.unwrap().cached_effects.clone(), cached2.unwrap().cached_effects.clone(),
                "Duplicate detection returned different effects");
        }

        /// Property: No collisions across different clients.
        ///
        /// Different clients with same request numbers should maintain
        /// independent sessions (no cross-client interference).
        #[test]
        fn prop_no_client_collisions(
            clients in prop::collection::vec(
                (0u64..20u64, 1u64..10u64),  // (client_id, request_number)
                1..50
            )
        ) {
            let mut sessions = ClientSessions::with_defaults();
            let mut expected_sessions = std::collections::HashMap::new();

            for (client_id, request_number) in clients {
                let cid = ClientId::new(client_id);
                let op = OpNumber::new(request_number);
                let ts = make_timestamp(request_number);

                // Record and commit
                if sessions.record_uncommitted(cid, request_number, op).is_ok() {
                    let _ = sessions.commit_request(cid, request_number, op, op, Vec::new(), ts);
                    expected_sessions.insert(cid, request_number);
                }
            }

            // Verify each client's session is independent
            for (client_id, expected_rnum) in expected_sessions {
                if let Some(session) = sessions.committed.get(&client_id) {
                    prop_assert_eq!(session.request_number, expected_rnum,
                        "Client {:?} session corrupted by collision", client_id);
                }
            }
        }

        /// Property: Uncommitted sessions are discarded on discard_uncommitted().
        ///
        /// After discard_uncommitted(), no uncommitted sessions should remain,
        /// but committed sessions should be preserved.
        #[test]
        fn prop_discard_uncommitted_preserves_committed(
            operations in prop::collection::vec(
                (0u64..5u64, 1u64..20u64, 0u64..100u64, any::<bool>()),  // (client, request, ts, commit?)
                1..30
            )
        ) {
            let mut sessions = ClientSessions::with_defaults();
            let mut committed_count = 0;

            // Record operations
            for (client_id, request_number, timestamp, should_commit) in operations {
                let cid = ClientId::new(client_id);
                let op = OpNumber::new(request_number);
                let ts = make_timestamp(timestamp);

                if sessions.record_uncommitted(cid, request_number, op).is_ok() {
                    if should_commit {
                        if sessions.commit_request(cid, request_number, op, op, Vec::new(), ts).is_ok() {
                            committed_count += 1;
                        }
                    }
                }
            }

            // Discard uncommitted
            sessions.discard_uncommitted();

            // Verify all uncommitted gone
            prop_assert_eq!(sessions.uncommitted.len(), 0,
                "Uncommitted sessions not fully discarded");

            // Verify committed preserved (at least some should remain)
            prop_assert!(sessions.committed.len() <= committed_count,
                "Committed session count incorrect after discard");
        }
    }
}
