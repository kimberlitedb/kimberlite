//! Adapters to make simulation components compatible with kernel traits.
//!
//! This module bridges the gap between the kernel's trait-based abstraction
//! (Clock, Storage, Network) and the simulation's specific implementations
//! (SimClock, SimStorage, SimNetwork).
//!
//! ## Example
//!
//! ```ignore
//! use kimberlite_sim::{SimClock, SimStorage, SimNetwork};
//! use kimberlite_sim::kernel_adapter::{ClockAdapter, StorageAdapter, NetworkAdapter};
//! use kimberlite_kernel::Runtime;
//!
//! let clock = ClockAdapter::new(SimClock::new());
//! let storage = StorageAdapter::new(SimStorage::reliable());
//! let network = NetworkAdapter::new(SimNetwork::new(...));
//!
//! let runtime = Runtime::new(clock, storage, network);
//! ```

use std::collections::HashMap;

use bytes::Bytes;
use kimberlite_kernel::traits::{
    Clock, Network, NetworkError, NetworkMessage, NetworkStats, ReplicaId, Storage, StorageError,
    StorageStats as KernelStorageStats,
};
use kimberlite_types::{Offset, StreamId, StreamMetadata, TenantId};

use crate::{SimClock, SimNetwork, SimStorage, StorageConfig};

// ============================================================================
// Clock Adapter
// ============================================================================

/// Adapter that implements the kernel Clock trait for SimClock.
pub struct ClockAdapter {
    clock: SimClock,
}

impl ClockAdapter {
    /// Creates a new clock adapter.
    pub fn new(clock: SimClock) -> Self {
        Self { clock }
    }

    /// Returns a reference to the underlying SimClock.
    pub fn inner(&self) -> &SimClock {
        &self.clock
    }

    /// Returns a mutable reference to the underlying SimClock.
    pub fn inner_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}

impl Clock for ClockAdapter {
    fn now_ns(&self) -> u64 {
        self.clock.now()
    }

    fn sleep_ns(&mut self, duration_ns: u64) {
        // In simulation, "sleep" advances the clock
        self.clock.advance_by(duration_ns);
    }
}

// ============================================================================
// Storage Adapter
// ============================================================================

/// Adapter that implements the kernel Storage trait for SimStorage.
///
/// Maps stream-based operations to block-based storage.
pub struct StorageAdapter {
    storage: SimStorage,
    /// Stream metadata: StreamId -> StreamMetadata
    metadata: HashMap<StreamId, StreamMetadata>,
    /// Stream data: StreamId -> Vec<(Offset, Bytes)>
    streams: HashMap<StreamId, Vec<(Offset, Bytes)>>,
}

impl StorageAdapter {
    /// Creates a new storage adapter with reliable configuration.
    pub fn new_reliable() -> Self {
        Self {
            storage: SimStorage::reliable(),
            metadata: HashMap::new(),
            streams: HashMap::new(),
        }
    }

    /// Creates a new storage adapter with the given SimStorage.
    pub fn new(storage: SimStorage) -> Self {
        Self {
            storage,
            metadata: HashMap::new(),
            streams: HashMap::new(),
        }
    }

    /// Creates a new storage adapter with custom configuration.
    pub fn with_config(config: StorageConfig) -> Self {
        Self {
            storage: SimStorage::new(config),
            metadata: HashMap::new(),
            streams: HashMap::new(),
        }
    }

    /// Returns a reference to the underlying SimStorage.
    pub fn inner(&self) -> &SimStorage {
        &self.storage
    }

    /// Returns a mutable reference to the underlying SimStorage.
    pub fn inner_mut(&mut self) -> &mut SimStorage {
        &mut self.storage
    }
}

impl Storage for StorageAdapter {
    fn append(
        &mut self,
        stream_id: StreamId,
        base_offset: Offset,
        events: Vec<Bytes>,
    ) -> Result<(), StorageError> {
        // Get or create stream
        let stream = self.streams.entry(stream_id).or_insert_with(Vec::new);

        // Append events
        for (i, event) in events.into_iter().enumerate() {
            let offset = base_offset + Offset::from(i as u64);
            stream.push((offset, event));
        }

        Ok(())
    }

    fn read(
        &self,
        stream_id: StreamId,
        from_offset: Offset,
        to_offset: Offset,
    ) -> Result<Vec<Bytes>, StorageError> {
        let stream = self
            .streams
            .get(&stream_id)
            .ok_or(StorageError::StreamNotFound(stream_id))?;

        let mut result = Vec::new();
        for (offset, event) in stream {
            if *offset >= from_offset && *offset < to_offset {
                result.push(event.clone());
            }
        }

        Ok(result)
    }

    fn write_metadata(&mut self, metadata: StreamMetadata) -> Result<(), StorageError> {
        self.metadata.insert(metadata.stream_id, metadata);
        Ok(())
    }

    fn read_metadata(&self, stream_id: StreamId) -> Result<StreamMetadata, StorageError> {
        self.metadata
            .get(&stream_id)
            .cloned()
            .ok_or(StorageError::StreamNotFound(stream_id))
    }

    fn sync(&mut self) -> Result<(), StorageError> {
        // SimStorage fsync is more complex - for now just return success
        // TODO: Wire this to SimStorage::fsync when needed
        Ok(())
    }

    fn stats(&self) -> KernelStorageStats {
        let sim_stats = self.storage.stats();

        KernelStorageStats {
            bytes_written: sim_stats.bytes_written,
            bytes_read: sim_stats.bytes_read,
            fsync_count: sim_stats.fsyncs,
            corruption_errors: sim_stats.reads_corrupted,
        }
    }
}

// ============================================================================
// Network Adapter
// ============================================================================

/// Adapter that implements the kernel Network trait for SimNetwork.
pub struct NetworkAdapter {
    network: SimNetwork,
    /// Local replica ID (reserved for future use)
    _replica_id: ReplicaId,
    /// Default tenant for messages
    tenant_id: TenantId,
    /// Message receive queue
    recv_queue: Vec<NetworkMessage>,
}

impl NetworkAdapter {
    /// Creates a new network adapter for the given replica.
    pub fn new(network: SimNetwork, replica_id: ReplicaId, tenant_id: TenantId) -> Self {
        Self {
            network,
            _replica_id: replica_id,
            tenant_id,
            recv_queue: Vec::new(),
        }
    }

    /// Returns a reference to the underlying SimNetwork.
    pub fn inner(&self) -> &SimNetwork {
        &self.network
    }

    /// Returns a mutable reference to the underlying SimNetwork.
    pub fn inner_mut(&mut self) -> &mut SimNetwork {
        &mut self.network
    }

    /// Delivers a message to this replica's receive queue.
    ///
    /// Called by the simulation when a message is ready for delivery.
    pub fn deliver(&mut self, from_replica: ReplicaId, payload: Bytes) {
        self.recv_queue.push(NetworkMessage {
            from_replica,
            tenant_id: self.tenant_id,
            payload,
        });
    }
}

impl Network for NetworkAdapter {
    fn send(&mut self, _to_replica: ReplicaId, _message: Bytes) -> Result<(), NetworkError> {
        // TODO: Wire this to SimNetwork::send when we integrate with event scheduling
        // For now, just return success
        Ok(())
    }

    fn recv(&mut self) -> Option<NetworkMessage> {
        if self.recv_queue.is_empty() {
            None
        } else {
            Some(self.recv_queue.remove(0))
        }
    }

    fn stats(&self) -> NetworkStats {
        let sim_stats = self.network.stats();

        NetworkStats {
            messages_sent: sim_stats.messages_sent,
            messages_received: sim_stats.messages_delivered,
            messages_dropped: sim_stats.messages_dropped,
            messages_delayed: 0, // SimNetworkStats doesn't track this separately
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NetworkConfig;
    use kimberlite_types::{DataClass, Placement};

    #[test]
    fn clock_adapter_implements_clock_trait() {
        let mut clock = ClockAdapter::new(SimClock::new());

        assert_eq!(clock.now_ns(), 0);

        clock.sleep_ns(1_000_000); // 1ms
        assert_eq!(clock.now_ns(), 1_000_000);

        clock.sleep_ns(500_000); // 0.5ms
        assert_eq!(clock.now_ns(), 1_500_000);
    }

    #[test]
    fn storage_adapter_append_and_read() {
        let mut storage = StorageAdapter::new_reliable();
        let stream_id = StreamId::new(1);

        // Append some events
        let events = vec![
            Bytes::from("event1"),
            Bytes::from("event2"),
            Bytes::from("event3"),
        ];
        storage
            .append(stream_id, Offset::from(0u64), events.clone())
            .unwrap();

        // Read them back
        let read_events = storage
            .read(stream_id, Offset::from(0u64), Offset::from(3u64))
            .unwrap();

        assert_eq!(read_events.len(), 3);
        assert_eq!(read_events[0], events[0]);
        assert_eq!(read_events[1], events[1]);
        assert_eq!(read_events[2], events[2]);
    }

    #[test]
    fn storage_adapter_metadata_roundtrip() {
        let mut storage = StorageAdapter::new_reliable();
        let stream_id = StreamId::new(42);

        let metadata = StreamMetadata::new(
            stream_id,
            kimberlite_types::StreamName::new("test-stream"),
            DataClass::NonPHI,
            Placement::Global,
        );

        storage.write_metadata(metadata.clone()).unwrap();

        let read_metadata = storage.read_metadata(stream_id).unwrap();
        assert_eq!(read_metadata.stream_id, metadata.stream_id);
    }

    #[test]
    fn storage_adapter_stream_not_found() {
        let storage = StorageAdapter::new_reliable();
        let result = storage.read(StreamId::new(999), Offset::from(0u64), Offset::from(10u64));

        assert!(matches!(result, Err(StorageError::StreamNotFound(_))));
    }

    #[test]
    fn network_adapter_send_and_recv() {
        let network = SimNetwork::new(NetworkConfig::reliable());
        let mut adapter = NetworkAdapter::new(network, ReplicaId::new(1), TenantId::new(100));

        // Simulate delivering a message
        adapter.deliver(ReplicaId::new(2), Bytes::from("hello"));

        // Receive it
        let msg = adapter.recv().unwrap();
        assert_eq!(msg.from_replica, ReplicaId::new(2));
        assert_eq!(msg.payload, Bytes::from("hello"));

        // Queue should be empty now
        assert!(adapter.recv().is_none());
    }

    #[test]
    fn storage_adapter_stats_tracking() {
        let mut storage = StorageAdapter::new_reliable();
        let stream_id = StreamId::new(1);

        // Write some data
        storage
            .append(
                stream_id,
                Offset::from(0u64),
                vec![Bytes::from("test data")],
            )
            .unwrap();

        let stats = storage.stats();
        // Stats tracking depends on underlying SimStorage implementation
        assert_eq!(stats.corruption_errors, 0);
    }
}
