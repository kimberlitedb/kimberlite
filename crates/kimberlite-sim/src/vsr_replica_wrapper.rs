//! VSR replica wrapper for simulation testing.
//!
//! This module provides a wrapper around `kimberlite_vsr::ReplicaState` that
//! adapts it for use in the VOPR simulation harness. The wrapper:
//!
//! - Manages a VSR replica's state and lifecycle
//! - Tracks rejected messages for Byzantine testing
//! - Integrates with SimStorage via SimStorageAdapter
//! - Provides snapshot capabilities for invariant checking
//!
//! ## Design
//!
//! The wrapper follows the FCIS pattern:
//! - VSR ReplicaState is pure (no I/O)
//! - Wrapper executes effects through SimStorageAdapter
//! - All randomness comes from SimRng
//!
//! ## Usage
//!
//! ```ignore
//! let mut wrapper = VsrReplicaWrapper::new(ReplicaId::new(0), config, storage);
//! let output = wrapper.process_event(ReplicaEvent::ClientRequest { ... });
//! wrapper.execute_effects(&output.effects, rng)?;
//! ```

use kimberlite_kernel::Effect;
use kimberlite_vsr::{
    ClusterConfig, CommitNumber, LogEntry, Message, OpNumber, ReplicaEvent, ReplicaId,
    ReplicaOutput, ReplicaState, ReplicaStatus, ViewNumber,
};

use crate::SimError;
use crate::SimRng;
use crate::adapters::{Clock, Rng, SimClock};
use crate::sim_storage_adapter::SimStorageAdapter;

// ============================================================================
// Type Aliases
// ============================================================================

/// Type alias for VSR replica wrapper in simulation mode.
///
/// Uses SimClock and SimRng for deterministic simulation testing.
pub type SimReplicaWrapper = VsrReplicaWrapper<SimClock, SimRng>;

// ============================================================================
// VSR Replica Wrapper
// ============================================================================

/// Wraps a VSR `ReplicaState` for simulation testing.
///
/// This wrapper provides:
/// - State management and event processing
/// - Effect execution via SimStorageAdapter
/// - Message rejection tracking for Byzantine testing
/// - Snapshot capabilities for invariant checking
/// - Per-node clock and RNG adapters for realistic testing
///
/// # Generics
///
/// - `C`: Clock adapter (SimClock for simulation, SystemClock for production)
/// - `R`: RNG adapter (SimRng for simulation, OsRngWrapper for production)
///
/// Hot paths (Clock, Rng) use generics for zero-cost abstraction.
#[derive(Debug)]
pub struct VsrReplicaWrapper<C: Clock, R: Rng> {
    /// The underlying VSR replica state.
    state: ReplicaState,

    /// Storage adapter for executing effects.
    storage: SimStorageAdapter,

    /// Clock adapter (per-node, with optional skew).
    clock: C,

    /// RNG adapter (per-node, forked from master RNG).
    rng: R,

    /// Rejected messages with reasons (for Byzantine testing).
    ///
    /// When VSR detects and rejects a Byzantine message, we track it here.
    /// Tests can check this to verify that attacks were detected.
    rejected_messages: Vec<(Message, String)>,

    /// Pending effects that haven't been executed yet.
    ///
    /// Effects accumulate across event processing and are executed
    /// in batches for efficiency.
    pending_effects: Vec<Effect>,
}

impl<C: Clock, R: Rng> VsrReplicaWrapper<C, R> {
    /// Creates a new VSR replica wrapper.
    ///
    /// # Parameters
    ///
    /// - `replica_id`: This replica's ID
    /// - `config`: Cluster configuration
    /// - `storage`: Storage adapter for effect execution
    /// - `clock`: Clock adapter (with optional skew)
    /// - `rng`: RNG adapter (forked from master RNG)
    pub fn new(
        replica_id: ReplicaId,
        config: ClusterConfig,
        storage: SimStorageAdapter,
        clock: C,
        rng: R,
    ) -> Self {
        let state = ReplicaState::new(replica_id, config);

        Self {
            state,
            storage,
            clock,
            rng,
            rejected_messages: Vec::new(),
            pending_effects: Vec::new(),
        }
    }

    /// Processes a replica event and returns the output.
    ///
    /// This is a pure function that transitions the replica state.
    /// Effects must be executed separately via [`Self::execute_effects`].
    ///
    /// # Returns
    ///
    /// The output containing messages to send and effects to execute.
    pub fn process_event(&mut self, event: ReplicaEvent) -> ReplicaOutput {
        // Process the event through VSR
        let (new_state, mut output) = self.state.clone().process(event);

        // Update state
        self.state = new_state;

        // Move effects to pending (no need to clone)
        self.pending_effects.append(&mut output.effects);

        output
    }

    /// Returns the current time from this replica's clock.
    #[inline]
    pub fn now(&self) -> u64 {
        self.clock.now()
    }

    /// Returns a mutable reference to this replica's RNG.
    ///
    /// Used for generating random values with per-node isolation.
    pub fn rng_mut(&mut self) -> &mut R {
        &mut self.rng
    }

    /// Records a rejected message for Byzantine testing.
    ///
    /// Called when VSR detects and rejects a malformed or Byzantine message.
    /// Tests can check `rejected_messages()` to verify attacks were detected.
    ///
    /// # Parameters
    ///
    /// - `message`: The rejected message
    /// - `reason`: Human-readable rejection reason
    pub fn record_rejection(&mut self, message: Message, reason: String) {
        self.rejected_messages.push((message, reason));
    }

    /// Returns all rejected messages.
    ///
    /// Used by tests to verify that Byzantine attacks were detected.
    pub fn rejected_messages(&self) -> &[(Message, String)] {
        &self.rejected_messages
    }

    /// Extracts a snapshot of the replica state for invariant checking.
    ///
    /// The snapshot contains all relevant state for checking VSR invariants
    /// like `commit_number <= op_number`, agreement, prefix property, etc.
    pub fn extract_snapshot(&self) -> VsrReplicaSnapshot {
        // Build log from individual entries
        let mut log = Vec::new();
        let mut op = OpNumber::new(1);
        while let Some(entry) = self.state.log_entry(op) {
            log.push(entry.clone());
            op = OpNumber::new(op.as_u64() + 1);
        }

        VsrReplicaSnapshot {
            replica_id: self.state.replica_id(),
            view: self.state.view(),
            op_number: self.state.op_number(),
            commit_number: self.state.commit_number(),
            log,
            status: self.state.status(),
        }
    }

    /// Returns this replica's ID.
    pub fn replica_id(&self) -> ReplicaId {
        self.state.replica_id()
    }

    /// Returns the current view number.
    pub fn view(&self) -> ViewNumber {
        self.state.view()
    }

    /// Returns the current op number.
    pub fn op_number(&self) -> OpNumber {
        self.state.op_number()
    }

    /// Returns the current commit number.
    pub fn commit_number(&self) -> CommitNumber {
        self.state.commit_number()
    }

    /// Returns the current replica status.
    pub fn status(&self) -> ReplicaStatus {
        self.state.status()
    }

    /// Returns the log length.
    pub fn log_len(&self) -> usize {
        self.state.log_len()
    }

    /// Returns a log entry at a specific operation number.
    pub fn log_entry(&self, op: OpNumber) -> Option<&LogEntry> {
        self.state.log_entry(op)
    }

    /// Returns a reference to the storage adapter.
    pub fn storage(&self) -> &SimStorageAdapter {
        &self.storage
    }

    /// Returns a mutable reference to the storage adapter.
    pub fn storage_mut(&mut self) -> &mut SimStorageAdapter {
        &mut self.storage
    }

    /// Returns the kernel state from the underlying VSR replica.
    ///
    /// This provides access to the pure kernel state for computing
    /// deterministic state hashes and verification.
    pub fn kernel_state(&self) -> &kimberlite_kernel::State {
        self.state.kernel_state()
    }

    /// Clears all rejected messages (for testing).
    pub fn clear_rejections(&mut self) {
        self.rejected_messages.clear();
    }
}

// ============================================================================
// Specialized Implementation for Simulation
// ============================================================================

impl SimReplicaWrapper {
    /// Executes pending effects through the storage adapter.
    ///
    /// This is the "imperative shell" that performs I/O. Effects are
    /// executed in order, and execution stops on the first error.
    ///
    /// Uses the replica's internal SimRng for storage latency simulation.
    ///
    /// # Returns
    ///
    /// `Ok(())` if all effects executed successfully, or an error on failure.
    pub fn execute_effects(&mut self) -> Result<(), SimError> {
        // Take pending effects to avoid borrow issues
        let effects = std::mem::take(&mut self.pending_effects);

        for effect in effects {
            self.storage.write_effect(&effect, &mut self.rng)?;
        }

        Ok(())
    }
}

// ============================================================================
// Replica Snapshot
// ============================================================================

/// Snapshot of VSR replica state for invariant checking.
///
/// This lightweight snapshot contains all the state needed to check
/// VSR invariants without requiring access to the full ReplicaState.
#[derive(Debug, Clone)]
pub struct VsrReplicaSnapshot {
    /// This replica's ID.
    pub replica_id: ReplicaId,

    /// Current view number.
    pub view: ViewNumber,

    /// Highest operation number in the log.
    pub op_number: OpNumber,

    /// Highest committed operation number.
    pub commit_number: CommitNumber,

    /// The replicated log of entries.
    pub log: Vec<LogEntry>,

    /// Current replica status.
    pub status: ReplicaStatus,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SimStorage, StorageConfig};
    use kimberlite_kernel::Command;
    use kimberlite_types::{
        DataClass, IdempotencyId, Placement, Region, StreamId, StreamName, TenantId,
    };

    fn test_config() -> ClusterConfig {
        ClusterConfig::new(vec![
            ReplicaId::new(0),
            ReplicaId::new(1),
            ReplicaId::new(2),
        ])
    }

    fn test_storage() -> SimStorageAdapter {
        let sim_storage = SimStorage::new(StorageConfig::reliable());
        SimStorageAdapter::new(sim_storage)
    }

    #[test]
    fn wrapper_creation() {
        let clock = SimClock::new();
        let rng = SimRng::new(42);
        let wrapper =
            VsrReplicaWrapper::new(ReplicaId::new(0), test_config(), test_storage(), clock, rng);

        assert_eq!(wrapper.replica_id(), ReplicaId::new(0));
        assert_eq!(wrapper.view(), ViewNumber::ZERO);
        assert_eq!(wrapper.op_number(), OpNumber::ZERO);
        assert_eq!(wrapper.commit_number(), CommitNumber::ZERO);
        assert_eq!(wrapper.status(), ReplicaStatus::Normal);
    }

    #[test]
    fn process_client_request() {
        let clock = SimClock::new();
        let rng = SimRng::new(42);
        let mut wrapper =
            VsrReplicaWrapper::new(ReplicaId::new(0), test_config(), test_storage(), clock, rng);

        // Leader (replica 0) can accept client requests in view 0
        let command = Command::CreateStream {
            stream_id: StreamId::from_tenant_and_local(TenantId::new(1), 1),
            stream_name: StreamName::from("test"),
            data_class: DataClass::PHI,
            placement: Placement::Region(Region::USEast1),
        };

        // Create idempotency ID from bytes (for testing)
        let idem_id = IdempotencyId::from_bytes([1u8; 16]); // Non-zero bytes for valid ID

        // TODO(v0.7.0): Add client session management (client_id, request_number)
        let output = wrapper.process_event(ReplicaEvent::ClientRequest {
            command,
            idempotency_id: Some(idem_id),
            client_id: None,
            request_number: None,
        });

        // Leader should send Prepare messages to backups
        assert!(!output.messages.is_empty());

        // Execute effects (uses internal RNG now)
        wrapper.execute_effects().expect("effects should execute");
    }

    #[test]
    fn snapshot_captures_state() {
        let clock = SimClock::new();
        let rng = SimRng::new(42);
        let wrapper =
            VsrReplicaWrapper::new(ReplicaId::new(1), test_config(), test_storage(), clock, rng);

        let snapshot = wrapper.extract_snapshot();

        assert_eq!(snapshot.replica_id, ReplicaId::new(1));
        assert_eq!(snapshot.view, ViewNumber::ZERO);
        assert_eq!(snapshot.op_number, OpNumber::ZERO);
        assert_eq!(snapshot.commit_number, CommitNumber::ZERO);
        assert_eq!(snapshot.status, ReplicaStatus::Normal);
        assert!(snapshot.log.is_empty());
    }

    #[test]
    fn rejection_tracking() {
        let clock = SimClock::new();
        let rng = SimRng::new(42);
        let mut wrapper =
            VsrReplicaWrapper::new(ReplicaId::new(0), test_config(), test_storage(), clock, rng);

        // Initially no rejections
        assert!(wrapper.rejected_messages().is_empty());

        // Record a rejection
        let msg = Message::broadcast(
            ReplicaId::new(0),
            kimberlite_vsr::MessagePayload::StartViewChange(kimberlite_vsr::StartViewChange {
                view: ViewNumber::from(1),
                replica: ReplicaId::new(0),
            }),
        );
        wrapper.record_rejection(msg.clone(), "test rejection".to_string());

        // Should be tracked
        assert_eq!(wrapper.rejected_messages().len(), 1);
        assert_eq!(wrapper.rejected_messages()[0].1, "test rejection");

        // Clear
        wrapper.clear_rejections();
        assert!(wrapper.rejected_messages().is_empty());
    }
}
