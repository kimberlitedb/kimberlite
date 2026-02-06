//! Network adapter trait for simulation vs production message passing.
//!
//! This module provides a trait-based abstraction for network communication:
//! - **Deterministic simulation**: Use `SimNetwork` with delays, drops, partitions
//! - **Production use**: Could use `TokioNetwork` or other async runtime
//!
//! # Performance
//!
//! The `Network` trait is on the cold path (I/O), so trait objects are acceptable.
//! Methods do NOT need `#[inline]` as they involve buffering and scheduling.

use std::collections::HashSet;

// Re-export types from parent module
pub use crate::network::{
    Message, MessageId, NetworkConfig, NetworkStats, Partition, RejectReason, SendResult,
    SimNetwork,
};
pub use crate::rng::SimRng;

/// Trait for network communication (simulation or production).
///
/// Implementations handle message passing with optional delays, drops, and partitions.
pub trait Network {
    /// Registers a node in the network.
    ///
    /// Nodes must be registered before they can send or receive messages.
    fn register_node(&mut self, node_id: u64);

    /// Sends a message through the network.
    ///
    /// # Arguments
    ///
    /// * `from` - Source node ID
    /// * `to` - Destination node ID
    /// * `payload` - Message payload (opaque bytes)
    /// * `current_time_ns` - Current simulation time (for delay calculation)
    /// * `rng` - Random number generator (for delays, drops, etc.)
    ///
    /// # Returns
    ///
    /// - `SendResult::Queued` if message was accepted
    /// - `SendResult::Dropped` if message was simulated as lost
    /// - `SendResult::Rejected` if message was rejected (partition, queue full, etc.)
    fn send(
        &mut self,
        from: u64,
        to: u64,
        payload: Vec<u8>,
        current_time_ns: u64,
        rng: &mut SimRng,
    ) -> SendResult;

    /// Delivers all messages ready by the given time.
    ///
    /// Returns messages in deterministic delivery order.
    fn deliver_ready(&mut self, current_time_ns: u64) -> Vec<Message>;

    /// Creates a network partition between two groups.
    ///
    /// # Arguments
    ///
    /// * `group_a` - Nodes in partition group A
    /// * `group_b` - Nodes in partition group B
    /// * `symmetric` - If true, both directions are blocked
    ///
    /// # Returns
    ///
    /// Partition ID that can be used with `heal_partition()`.
    fn create_partition(
        &mut self,
        group_a: HashSet<u64>,
        group_b: HashSet<u64>,
        symmetric: bool,
    ) -> u64;

    /// Heals a network partition (removes the partition).
    ///
    /// # Returns
    ///
    /// `true` if the partition was found and removed, `false` otherwise.
    fn heal_partition(&mut self, partition_id: u64) -> bool;

    /// Checks if communication from `from` to `to` is blocked by a partition.
    fn is_partitioned(&self, from: u64, to: u64) -> bool;

    /// Returns network statistics (for monitoring and debugging).
    fn stats(&self) -> NetworkStats;
}

// ============================================================================
// Simulation Implementation
// ============================================================================

impl Network for SimNetwork {
    fn register_node(&mut self, node_id: u64) {
        SimNetwork::register_node(self, node_id);
    }

    fn send(
        &mut self,
        from: u64,
        to: u64,
        payload: Vec<u8>,
        current_time_ns: u64,
        rng: &mut SimRng,
    ) -> SendResult {
        SimNetwork::send(self, from, to, payload, current_time_ns, rng)
    }

    fn deliver_ready(&mut self, current_time_ns: u64) -> Vec<Message> {
        SimNetwork::deliver_ready(self, current_time_ns)
    }

    fn create_partition(
        &mut self,
        group_a: HashSet<u64>,
        group_b: HashSet<u64>,
        symmetric: bool,
    ) -> u64 {
        SimNetwork::create_partition(self, group_a, group_b, symmetric)
    }

    fn heal_partition(&mut self, partition_id: u64) -> bool {
        SimNetwork::heal_partition(self, partition_id)
    }

    fn is_partitioned(&self, from: u64, to: u64) -> bool {
        SimNetwork::is_partitioned(self, from, to)
    }

    fn stats(&self) -> NetworkStats {
        SimNetwork::stats(self).clone()
    }
}

// ============================================================================
// Production Implementation (Sketch)
// ============================================================================

/// Tokio-based network for production use (sketch).
///
/// **Note**: This is a sketch for architectural demonstration.
/// Full implementation would use async/await and tokio channels.
#[cfg(not(test))]
#[derive(Default)]
pub struct TokioNetwork {
    // Would contain tokio channels, node registry, etc.
    _placeholder: (),
}

#[cfg(not(test))]
impl TokioNetwork {
    /// Creates a new Tokio-based network.
    pub fn new() -> Self {
        Self { _placeholder: () }
    }
}

#[cfg(not(test))]
impl Network for TokioNetwork {
    fn register_node(&mut self, _node_id: u64) {
        // Would register node in runtime
    }

    fn send(
        &mut self,
        _from: u64,
        _to: u64,
        _payload: Vec<u8>,
        _current_time_ns: u64,
        _rng: &mut SimRng,
    ) -> SendResult {
        // Would send via tokio channel
        SendResult::Queued {
            message_id: MessageId::from_raw(0),
            deliver_at_ns: 0,
        }
    }

    fn deliver_ready(&mut self, _current_time_ns: u64) -> Vec<Message> {
        // Would poll tokio channels
        Vec::new()
    }

    fn create_partition(
        &mut self,
        _group_a: HashSet<u64>,
        _group_b: HashSet<u64>,
        _symmetric: bool,
    ) -> u64 {
        // Partitions not applicable in production
        0
    }

    fn heal_partition(&mut self, _partition_id: u64) -> bool {
        false
    }

    fn is_partitioned(&self, _from: u64, _to: u64) -> bool {
        false
    }

    fn stats(&self) -> NetworkStats {
        NetworkStats::default()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_network_trait_impl() {
        let mut network: Box<dyn Network> = Box::new(SimNetwork::reliable());
        let mut rng = SimRng::new(12345);

        // Register nodes
        network.register_node(0);
        network.register_node(1);

        // Send a message
        let result = network.send(0, 1, vec![1, 2, 3], 0, &mut rng);
        assert!(matches!(result, SendResult::Queued { .. }));

        // Deliver messages
        if let SendResult::Queued { deliver_at_ns, .. } = result {
            let delivered = network.deliver_ready(deliver_at_ns);
            assert_eq!(delivered.len(), 1);
            assert_eq!(delivered[0].from, 0);
            assert_eq!(delivered[0].to, 1);
            assert_eq!(delivered[0].payload, vec![1, 2, 3]);
        }
    }

    #[test]
    fn sim_network_partition_via_trait() {
        let mut network: Box<dyn Network> = Box::new(SimNetwork::reliable());
        let mut rng = SimRng::new(12345);

        network.register_node(0);
        network.register_node(1);

        // Create partition
        let mut group_a = HashSet::new();
        group_a.insert(0);
        let mut group_b = HashSet::new();
        group_b.insert(1);

        let partition_id = network.create_partition(group_a, group_b, true);

        // Verify partition blocks communication
        assert!(network.is_partitioned(0, 1));
        assert!(network.is_partitioned(1, 0));

        // Send should be rejected
        let result = network.send(0, 1, vec![1, 2, 3], 0, &mut rng);
        assert!(matches!(
            result,
            SendResult::Rejected {
                reason: RejectReason::Partitioned
            }
        ));

        // Heal partition
        assert!(network.heal_partition(partition_id));
        assert!(!network.is_partitioned(0, 1));

        // Send should now succeed
        let result = network.send(0, 1, vec![1, 2, 3], 0, &mut rng);
        assert!(matches!(result, SendResult::Queued { .. }));
    }

    #[test]
    fn sim_network_stats_via_trait() {
        let mut network: Box<dyn Network> = Box::new(SimNetwork::reliable());
        let mut rng = SimRng::new(12345);

        network.register_node(0);
        network.register_node(1);

        // Send a message
        network.send(0, 1, vec![1, 2, 3], 0, &mut rng);

        // Check stats
        let stats = network.stats();
        assert_eq!(stats.messages_sent, 1);
    }

    #[test]
    fn sim_network_lossy_drops_messages() {
        let config = NetworkConfig {
            drop_probability: 1.0, // Always drop
            ..NetworkConfig::default()
        };
        let mut network: Box<dyn Network> = Box::new(SimNetwork::new(config));
        let mut rng = SimRng::new(12345);

        network.register_node(0);
        network.register_node(1);

        // Send should be dropped
        let result = network.send(0, 1, vec![1, 2, 3], 0, &mut rng);
        assert!(matches!(result, SendResult::Dropped));

        // Stats should show drop
        let stats = network.stats();
        assert_eq!(stats.messages_sent, 1);
        assert_eq!(stats.messages_dropped, 1);
    }

    #[test]
    fn sim_network_unknown_destination_rejected() {
        let mut network: Box<dyn Network> = Box::new(SimNetwork::reliable());
        let mut rng = SimRng::new(12345);

        network.register_node(0);
        // Node 1 is NOT registered

        let result = network.send(0, 1, vec![1, 2, 3], 0, &mut rng);
        assert!(matches!(
            result,
            SendResult::Rejected {
                reason: RejectReason::UnknownDestination
            }
        ));
    }

    #[test]
    fn sim_network_deterministic_delivery_order() {
        let mut network = SimNetwork::reliable();
        let mut rng = SimRng::new(12345);

        network.register_node(0);
        network.register_node(1);

        // Send multiple messages
        let mut delivery_times = Vec::new();
        for i in 0..5_u64 {
            if let SendResult::Queued { deliver_at_ns, .. } =
                network.send(0, 1, vec![i as u8], 1000 * i, &mut rng)
            {
                delivery_times.push(deliver_at_ns);
            }
        }

        // Deliver all at once
        let max_time = *delivery_times.iter().max().unwrap();
        let delivered = network.deliver_ready(max_time);

        // Messages should be delivered in time order (deterministic)
        assert_eq!(delivered.len(), 5);
        for i in 0..4 {
            assert!(delivered[i].deliver_at_ns <= delivered[i + 1].deliver_at_ns);
        }
    }
}
