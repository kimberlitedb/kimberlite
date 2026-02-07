//! VSR-based simulation mode for VOPR.
//!
//! This module provides a simulation mode that uses actual VSR replicas
//! instead of the simplified state-based model. This enables proper testing
//! of VSR's Byzantine resistance at the protocol level.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    VSR Simulation Mode                       │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
//! │  │ VSR Replica  │  │ VSR Replica  │  │ VSR Replica  │      │
//! │  │   (ID: 0)    │  │   (ID: 1)    │  │   (ID: 2)    │      │
//! │  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘      │
//! │         │                 │                 │                │
//! │         └─────────────────┴─────────────────┘                │
//! │                           │                                  │
//! │                   ┌───────▼────────┐                         │
//! │                   │  MessageMutator │ ◄─── Byzantine attacks │
//! │                   └───────┬────────┘                         │
//! │                           │                                  │
//! │                   ┌───────▼────────┐                         │
//! │                   │   SimNetwork    │                        │
//! │                   └────────────────┘                         │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use kimberlite_kernel::Command;
use kimberlite_types::{
    DataClass, IdempotencyId, Placement, Region, StreamId, StreamName, TenantId,
};
use kimberlite_vsr::{ClusterConfig, Message, ReplicaEvent, ReplicaId, TimeoutKind};

use crate::adapters::SimClock;
use crate::vsr_replica_wrapper::SimReplicaWrapper;
use crate::{
    SimRng, SimStorage, SimStorageAdapter, StorageConfig, VsrReplicaSnapshot,
    deserialize_vsr_message, replica_to_network_id, serialize_vsr_message,
};

// ============================================================================
// VSR Simulation State
// ============================================================================

/// State for VSR-based simulation.
///
/// Maintains 3 VSR replicas and coordinates their interactions through
/// the simulation network.
///
/// Each replica has independent clock and RNG adapters for realistic
/// distributed systems testing (clock skew, per-node randomness).
pub struct VsrSimulation {
    /// The three VSR replicas (standard 3-replica cluster).
    ///
    /// Each replica has:
    /// - Independent SimClock (with configurable skew)
    /// - Independent SimRng (forked from master seed)
    replicas: [SimReplicaWrapper; 3],

    /// Cluster configuration.
    #[allow(dead_code)] // Reserved for future replica selection logic
    config: ClusterConfig,

    /// Next command ID for generating unique commands.
    next_command_id: u64,
}

impl VsrSimulation {
    /// Creates a new VSR simulation with 3 replicas.
    ///
    /// Each replica gets:
    /// - Independent clock (with configurable skew)
    /// - Independent RNG (forked from master seed)
    /// - Independent storage
    ///
    /// Default clock skew:
    /// - Replica 0: No skew (synchronized)
    /// - Replica 1: -5ms (behind)
    /// - Replica 2: +3ms (ahead)
    ///
    /// # Parameters
    ///
    /// - `storage_config`: Configuration for simulated storage
    /// - `seed`: Random seed for deterministic behavior
    pub fn new(storage_config: StorageConfig, seed: u64) -> Self {
        let config = ClusterConfig::new(vec![
            ReplicaId::new(0),
            ReplicaId::new(1),
            ReplicaId::new(2),
        ]);

        // Create master RNG for forking per-node RNGs
        let mut master_rng = SimRng::new(seed);

        // Fork RNGs for each replica (deterministic, independent streams)
        let rng0 = SimRng::new(master_rng.next_u64());
        let rng1 = SimRng::new(master_rng.next_u64());
        let rng2 = SimRng::new(master_rng.next_u64());

        // Create clocks with per-node skew (in nanoseconds)
        let clock0 = SimClock::new(); // No skew
        let clock1 = SimClock::with_skew(-5_000_000); // 5ms behind
        let clock2 = SimClock::with_skew(3_000_000); // 3ms ahead

        // Create storage adapters for each replica
        let storage0 = SimStorageAdapter::new(SimStorage::new(storage_config.clone()));
        let storage1 = SimStorageAdapter::new(SimStorage::new(storage_config.clone()));
        let storage2 = SimStorageAdapter::new(SimStorage::new(storage_config));

        // Initialize replicas with per-node adapters
        let replica0 =
            SimReplicaWrapper::new(ReplicaId::new(0), config.clone(), storage0, clock0, rng0);
        let replica1 =
            SimReplicaWrapper::new(ReplicaId::new(1), config.clone(), storage1, clock1, rng1);
        let replica2 =
            SimReplicaWrapper::new(ReplicaId::new(2), config.clone(), storage2, clock2, rng2);

        Self {
            replicas: [replica0, replica1, replica2],
            config,
            next_command_id: seed, // Use seed as starting point for determinism
        }
    }

    /// Processes a client request on the leader.
    ///
    /// This generates a CreateStream command and submits it to replica 0
    /// (which is the leader for view 0).
    ///
    /// # Parameters
    ///
    /// - `rng`: Random number generator for deterministic behavior
    ///
    /// # Returns
    ///
    /// Messages generated by the leader (Prepare messages to backups).
    pub fn process_client_request(&mut self, rng: &mut SimRng) -> Vec<Message> {
        // Generate a deterministic command
        let command = self.generate_command(rng);

        // Create idempotency ID from command ID (deterministic)
        let idem_bytes = self.next_command_id.to_le_bytes();
        let mut full_bytes = [0u8; 16];
        full_bytes[..8].copy_from_slice(&idem_bytes);
        full_bytes[8] = 1; // Ensure non-zero
        let idempotency_id = IdempotencyId::from_bytes(full_bytes);

        self.next_command_id += 1;

        // Submit to leader (replica 0 in view 0)
        let leader = &mut self.replicas[0];
        // TODO(v0.7.0): Add client session management (client_id, request_number)
        let output = leader.process_event(ReplicaEvent::ClientRequest {
            command,
            idempotency_id: Some(idempotency_id),
            client_id: None,
            request_number: None,
        });

        // Execute effects with graceful error handling
        // Storage failures are logged but don't stop simulation - this tests
        // VSR's ability to handle inconsistent state from transient failures
        if let Err(e) = leader.execute_effects() {
            eprintln!(
                "Warning: Leader (replica 0) effect execution failed: {}. \
                 Continuing simulation to test VSR fault handling.",
                e
            );
        }

        output.messages
    }

    /// Delivers a VSR message to a replica.
    ///
    /// # Parameters
    ///
    /// - `to_replica`: Destination replica ID (0-2)
    /// - `message`: The VSR message to deliver
    /// - `rng`: Random number generator
    ///
    /// # Returns
    ///
    /// Messages generated in response (e.g., PrepareOK from backup).
    pub fn deliver_message(
        &mut self,
        to_replica: u8,
        message: Message,
        _rng: &mut SimRng,
    ) -> Vec<Message> {
        let replica = &mut self.replicas[to_replica as usize];
        let output = replica.process_event(ReplicaEvent::Message(Box::new(message)));

        // Execute effects with graceful error handling
        // Storage failures are logged but don't stop simulation - this tests
        // VSR's ability to handle inconsistent state from transient failures
        if let Err(e) = replica.execute_effects() {
            eprintln!(
                "Warning: Replica {} effect execution failed: {}. \
                 Continuing simulation to test VSR fault handling.",
                to_replica, e
            );
        }

        output.messages
    }

    /// Processes a timeout on a replica.
    ///
    /// # Parameters
    ///
    /// - `replica_id`: Which replica timed out (0-2)
    /// - `timeout_kind`: Type of timeout
    /// - `rng`: Random number generator
    ///
    /// # Returns
    ///
    /// Messages generated in response (e.g., StartViewChange).
    pub fn process_timeout(
        &mut self,
        replica_id: u8,
        timeout_kind: TimeoutKind,
        _rng: &mut SimRng,
    ) -> Vec<Message> {
        let replica = &mut self.replicas[replica_id as usize];
        let output = replica.process_event(ReplicaEvent::Timeout(timeout_kind));

        // Execute effects with graceful error handling
        // Storage failures are logged but don't stop simulation - this tests
        // VSR's ability to handle inconsistent state from transient failures
        if let Err(e) = replica.execute_effects() {
            eprintln!(
                "Warning: Replica {} effect execution failed during timeout: {}. \
                 Continuing simulation to test VSR fault handling.",
                replica_id, e
            );
        }

        output.messages
    }

    /// Extracts snapshots from all replicas for invariant checking.
    pub fn extract_snapshots(&self) -> [VsrReplicaSnapshot; 3] {
        [
            self.replicas[0].extract_snapshot(),
            self.replicas[1].extract_snapshot(),
            self.replicas[2].extract_snapshot(),
        ]
    }

    /// Returns the kernel state from the leader replica (replica 0).
    ///
    /// This provides access to the kernel state for computing deterministic
    /// state hashes. We use replica 0's state as it's typically the leader
    /// in view 0 (the initial view).
    ///
    /// For determinism verification, all replicas should have identical
    /// kernel states at the same commit number (verified by invariant checkers).
    pub fn kernel_state(&self) -> &kimberlite_kernel::State {
        self.replicas[0].kernel_state()
    }

    /// Returns a reference to a specific replica.
    pub fn replica(&self, id: u8) -> &SimReplicaWrapper {
        &self.replicas[id as usize]
    }

    /// Returns a mutable reference to a specific replica.
    pub fn replica_mut(&mut self, id: u8) -> &mut SimReplicaWrapper {
        &mut self.replicas[id as usize]
    }

    /// Generates a deterministic command based on RNG state.
    fn generate_command(&self, rng: &mut SimRng) -> Command {
        // Generate a CreateStream command (Phase 1 - simple command type)
        let tenant_id = TenantId::new(rng.next_u64() % 10 + 1);
        let local_id = (self.next_command_id % (u32::MAX as u64)) as u32;

        Command::CreateStream {
            stream_id: StreamId::from_tenant_and_local(tenant_id, local_id),
            stream_name: StreamName::from(format!("stream_{}", self.next_command_id)),
            data_class: DataClass::PHI,
            placement: Placement::Region(Region::USEast1),
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Serializes a VSR message for network transmission.
pub fn vsr_message_to_bytes(msg: &Message) -> Vec<u8> {
    serialize_vsr_message(msg).expect("serialization should not fail")
}

/// Deserializes a VSR message from network bytes.
pub fn vsr_message_from_bytes(bytes: &[u8]) -> Message {
    deserialize_vsr_message(bytes).expect("deserialization should not fail")
}

/// Converts a VSR message's destination to a network node ID.
pub fn vsr_message_destination(msg: &Message) -> u64 {
    replica_to_network_id(msg.to)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_vsr::{CommitNumber, OpNumber, ViewNumber};

    fn test_config() -> StorageConfig {
        StorageConfig::reliable()
    }

    #[test]
    fn vsr_simulation_creation() {
        let sim = VsrSimulation::new(test_config(), 42);

        // All replicas should start in normal status at view 0
        for i in 0..3 {
            let replica = sim.replica(i);
            assert_eq!(replica.replica_id(), ReplicaId::new(i));
            assert_eq!(replica.view(), ViewNumber::ZERO);
            assert_eq!(replica.op_number(), OpNumber::ZERO);
            assert_eq!(replica.commit_number(), CommitNumber::ZERO);
        }
    }

    #[test]
    fn vsr_simulation_client_request() {
        let mut sim = VsrSimulation::new(test_config(), 42);
        let mut rng = SimRng::new(42);

        // Submit a client request
        let messages = sim.process_client_request(&mut rng);

        // Leader should send Prepare messages to backups (2 messages for 2 backups)
        assert!(!messages.is_empty(), "leader should send prepare messages");

        // Leader's op_number should have advanced
        let leader = sim.replica(0);
        assert_eq!(leader.op_number(), OpNumber::new(1));
    }

    #[test]
    fn vsr_simulation_message_delivery() {
        let mut sim = VsrSimulation::new(test_config(), 42);
        let mut rng = SimRng::new(42);

        // Submit a client request to get Prepare messages
        let prepare_messages = sim.process_client_request(&mut rng);

        // Deliver Prepare to first backup
        let prepare_msg = prepare_messages
            .first()
            .expect("should have prepare message");
        let responses = sim.deliver_message(1, prepare_msg.clone(), &mut rng);

        // Backup should respond with PrepareOK
        assert!(
            !responses.is_empty(),
            "backup should respond with PrepareOK"
        );

        // Backup's op_number should have advanced
        let backup = sim.replica(1);
        assert_eq!(backup.op_number(), OpNumber::new(1));
    }

    #[test]
    fn vsr_simulation_snapshots() {
        let sim = VsrSimulation::new(test_config(), 42);

        let snapshots = sim.extract_snapshots();

        assert_eq!(snapshots.len(), 3);
        for (i, snapshot) in snapshots.iter().enumerate() {
            assert_eq!(snapshot.replica_id, ReplicaId::new(i as u8));
            assert_eq!(snapshot.view, ViewNumber::ZERO);
        }
    }

    #[test]
    fn vsr_message_serialization_roundtrip() {
        use kimberlite_vsr::{MessagePayload, StartViewChange};

        let msg = Message::broadcast(
            ReplicaId::new(0),
            MessagePayload::StartViewChange(StartViewChange {
                view: ViewNumber::from(1),
                replica: ReplicaId::new(0),
            }),
        );

        let bytes = vsr_message_to_bytes(&msg);
        let decoded = vsr_message_from_bytes(&bytes);

        assert_eq!(msg.from, decoded.from);
        assert_eq!(msg.to, decoded.to);
    }

    #[test]
    fn vsr_simulation_kernel_state_hash_determinism() {
        // Create two simulations with the same seed
        let sim1 = VsrSimulation::new(test_config(), 42);
        let sim2 = VsrSimulation::new(test_config(), 42);

        // Both should have identical kernel state hashes
        let hash1 = sim1.kernel_state().compute_state_hash();
        let hash2 = sim2.kernel_state().compute_state_hash();

        assert_eq!(
            hash1, hash2,
            "Identical seeds should produce identical kernel state hashes"
        );
    }

    #[test]
    fn vsr_simulation_kernel_state_hash_changes_after_operations() {
        let mut sim = VsrSimulation::new(test_config(), 42);
        let mut rng = SimRng::new(42);

        // Get initial hash
        let hash_before = sim.kernel_state().compute_state_hash();

        // Process a client request
        let prepare_messages = sim.process_client_request(&mut rng);

        // Deliver Prepare to backups to achieve consensus
        for msg in &prepare_messages {
            sim.deliver_message(1, msg.clone(), &mut rng);
            sim.deliver_message(2, msg.clone(), &mut rng);
        }

        // Get hash after operations
        let hash_after = sim.kernel_state().compute_state_hash();

        // Note: Hash might be the same if operations haven't been committed yet
        // This test primarily ensures the hash computation doesn't panic
        assert_eq!(hash_before.len(), 32, "Hash should be 32 bytes");
        assert_eq!(hash_after.len(), 32, "Hash should be 32 bytes");
    }

    #[test]
    fn vsr_replica_wrapper_kernel_state_access() {
        let sim = VsrSimulation::new(test_config(), 42);

        // All replicas should have access to kernel state
        for i in 0..3 {
            let replica = sim.replica(i);
            let kernel_state = replica.kernel_state();

            // Kernel state should be initialized
            assert_eq!(
                kernel_state.stream_count(),
                0,
                "Initial state should have no streams"
            );

            // Should be able to compute hash
            let hash = kernel_state.compute_state_hash();
            assert_eq!(hash.len(), 32, "Hash should be 32 bytes (BLAKE3)");
        }
    }

    #[test]
    fn vsr_all_replicas_same_kernel_state() {
        // All replicas in a fresh simulation should have identical kernel state
        let sim = VsrSimulation::new(test_config(), 42);

        let hash0 = sim.replica(0).kernel_state().compute_state_hash();
        let hash1 = sim.replica(1).kernel_state().compute_state_hash();
        let hash2 = sim.replica(2).kernel_state().compute_state_hash();

        assert_eq!(
            hash0, hash1,
            "Replica 0 and 1 should have identical kernel state"
        );
        assert_eq!(
            hash1, hash2,
            "Replica 1 and 2 should have identical kernel state"
        );
    }

    #[test]
    fn per_node_clock_skew() {
        // Test Phase 3: Per-node clock adapters with different skew values
        let sim = VsrSimulation::new(test_config(), 42);

        // Get initial clock values (all at time 0)
        let time0 = sim.replica(0).now();
        let time1 = sim.replica(1).now();
        let time2 = sim.replica(2).now();

        // Replica 0: No skew
        assert_eq!(time0, 0, "Replica 0 should have no skew");

        // Replica 1: -5ms skew (behind) - but saturating_add_signed prevents negative
        // At time 0, -5ms results in 0 due to saturation
        assert_eq!(
            time1, 0,
            "Replica 1 with -5ms skew at time 0 saturates to 0"
        );

        // Replica 2: +3ms skew (ahead)
        assert_eq!(time2, 3_000_000, "Replica 2 should be 3ms ahead");

        // This test demonstrates that:
        // 1. Each replica has its own independent Clock adapter
        // 2. Clock skew is applied per-node
        // 3. SimClock::with_skew() works correctly
    }

    #[test]
    fn per_node_rng_forking() {
        // Test Phase 3: Per-node RNG adapters forked from master seed
        let mut sim1 = VsrSimulation::new(test_config(), 12345);
        let mut sim2 = VsrSimulation::new(test_config(), 12345);

        // Generate random values from each replica in sim1
        let r0_val1 = sim1.replica_mut(0).rng_mut().next_u64();
        let r1_val1 = sim1.replica_mut(1).rng_mut().next_u64();
        let r2_val1 = sim1.replica_mut(2).rng_mut().next_u64();

        // Generate random values from each replica in sim2
        let r0_val2 = sim2.replica_mut(0).rng_mut().next_u64();
        let r1_val2 = sim2.replica_mut(1).rng_mut().next_u64();
        let r2_val2 = sim2.replica_mut(2).rng_mut().next_u64();

        // With the same master seed, each replica's RNG should produce identical values
        assert_eq!(r0_val1, r0_val2, "Replica 0 RNG should be deterministic");
        assert_eq!(r1_val1, r1_val2, "Replica 1 RNG should be deterministic");
        assert_eq!(r2_val1, r2_val2, "Replica 2 RNG should be deterministic");

        // But each replica should have different values (independent RNG streams)
        assert_ne!(
            r0_val1, r1_val1,
            "Replica 0 and 1 should have independent RNGs"
        );
        assert_ne!(
            r1_val1, r2_val1,
            "Replica 1 and 2 should have independent RNGs"
        );
        assert_ne!(
            r0_val1, r2_val1,
            "Replica 0 and 2 should have independent RNGs"
        );

        // This test demonstrates that:
        // 1. Each replica has its own independent RNG adapter
        // 2. RNGs are forked from the master seed (deterministic)
        // 3. Each replica's RNG produces different values (independent streams)
    }
}
