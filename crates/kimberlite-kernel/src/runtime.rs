//! Runtime layer that executes kernel effects.
//!
//! The kernel is pure and produces effects. The runtime takes these effects
//! and executes them using concrete implementations of Clock, Network, and
//! Storage traits.
//!
//! ## Example
//!
//! ```ignore
//! use kimberlite_kernel::{Runtime, State, Command, apply_committed};
//! use kimberlite_kernel::runtime::{SystemClock, InMemoryStorage, NoOpNetwork};
//!
//! let clock = SystemClock::new();
//! let storage = InMemoryStorage::new();
//! let network = NoOpNetwork;
//!
//! let mut runtime = Runtime::new(clock, storage, network);
//! let state = State::new();
//!
//! let cmd = Command::create_stream(...);
//! let (new_state, effects) = apply_committed(state, cmd)?;
//!
//! // Execute all effects
//! runtime.execute_effects(effects)?;
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use kimberlite_types::{Offset, StreamId, StreamMetadata};

use crate::effects::Effect;
use crate::traits::{
    Clock, Network, NetworkError, NetworkMessage, NetworkStats, ReplicaId, Storage, StorageError,
    StorageStats,
};

/// Runtime that executes kernel effects using pluggable traits.
///
/// Generic over Clock, Storage, and Network to enable both production
/// and simulation testing.
pub struct Runtime<C, S, N>
where
    C: Clock,
    S: Storage,
    N: Network,
{
    /// Clock for time-based operations.
    pub clock: C,
    /// Storage layer for durability.
    pub storage: S,
    /// Network layer for replication.
    pub network: N,
}

impl<C, S, N> Runtime<C, S, N>
where
    C: Clock,
    S: Storage,
    N: Network,
{
    /// Creates a new runtime with the given implementations.
    pub fn new(clock: C, storage: S, network: N) -> Self {
        Self {
            clock,
            storage,
            network,
        }
    }

    /// Executes a single effect.
    pub fn execute_effect(&mut self, effect: Effect) -> Result<(), RuntimeError> {
        match effect {
            Effect::StorageAppend {
                stream_id,
                base_offset,
                events,
            } => {
                self.storage
                    .append(stream_id, base_offset, events)
                    .map_err(RuntimeError::Storage)?;
            }

            Effect::StreamMetadataWrite(metadata) => {
                self.storage
                    .write_metadata(metadata)
                    .map_err(RuntimeError::Storage)?;
            }

            Effect::WakeProjection {
                stream_id: _,
                from_offset: _,
                to_offset: _,
            }
            | Effect::AuditLogAppend(_)
            | Effect::TableMetadataWrite(_) => {
                // TODO: Implement projection wakeup, audit logging, and table metadata
                // For now, these are all no-ops
            }

            Effect::TableMetadataDrop(_table_id) => {
                // TODO: Implement table metadata deletion
                // For now, this is a no-op
            }

            Effect::IndexMetadataWrite(_metadata) => {
                // TODO: Implement index metadata persistence
                // For now, this is a no-op
            }

            Effect::UpdateProjection {
                table_id: _,
                from_offset: _,
                to_offset: _,
            } => {
                // TODO: Implement projection update when projection engine exists
                // For now, this is a no-op
            }
        }

        Ok(())
    }

    /// Executes all effects in order.
    ///
    /// Stops at the first error and returns it.
    pub fn execute_effects(&mut self, effects: Vec<Effect>) -> Result<(), RuntimeError> {
        for effect in effects {
            self.execute_effect(effect)?;
        }
        Ok(())
    }

    /// Returns a reference to the clock.
    pub fn clock(&self) -> &C {
        &self.clock
    }

    /// Returns a mutable reference to the clock.
    pub fn clock_mut(&mut self) -> &mut C {
        &mut self.clock
    }

    /// Returns a reference to the storage.
    pub fn storage(&self) -> &S {
        &self.storage
    }

    /// Returns a mutable reference to the storage.
    pub fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    /// Returns a reference to the network.
    pub fn network(&self) -> &N {
        &self.network
    }

    /// Returns a mutable reference to the network.
    pub fn network_mut(&mut self) -> &mut N {
        &mut self.network
    }
}

/// Errors that can occur during effect execution.
#[derive(thiserror::Error, Debug)]
pub enum RuntimeError {
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("network error: {0}")]
    Network(String),

    #[error("internal error: {0}")]
    Internal(String),
}

// ============================================================================
// Production Implementations
// ============================================================================

/// Production clock using system time.
pub struct SystemClock {
    epoch: SystemTime,
}

impl SystemClock {
    /// Creates a new system clock.
    pub fn new() -> Self {
        Self { epoch: UNIX_EPOCH }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_ns(&self) -> u64 {
        SystemTime::now()
            .duration_since(self.epoch)
            .expect("system time before epoch")
            .as_nanos() as u64
    }

    fn sleep_ns(&mut self, duration_ns: u64) {
        std::thread::sleep(std::time::Duration::from_nanos(duration_ns));
    }
}

/// In-memory storage for testing and development.
///
/// Not suitable for production - data is lost on restart.
pub struct InMemoryStorage {
    /// Streams: StreamId -> Vec<(Offset, Event)>
    streams: HashMap<StreamId, Vec<(Offset, Bytes)>>,
    /// Metadata: StreamId -> StreamMetadata
    metadata: HashMap<StreamId, StreamMetadata>,
    /// Statistics (uses AtomicU64 for thread-safe interior mutability)
    bytes_written: AtomicU64,
    bytes_read: AtomicU64,
    fsync_count: AtomicU64,
}

impl InMemoryStorage {
    /// Creates a new in-memory storage.
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
            metadata: HashMap::new(),
            bytes_written: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
            fsync_count: AtomicU64::new(0),
        }
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Storage for InMemoryStorage {
    fn append(
        &mut self,
        stream_id: StreamId,
        base_offset: Offset,
        events: Vec<Bytes>,
    ) -> Result<(), StorageError> {
        let stream = self.streams.entry(stream_id).or_default();

        for (i, event) in events.into_iter().enumerate() {
            let offset = base_offset + Offset::from(i as u64);
            let bytes = event.len() as u64;
            self.bytes_written.fetch_add(bytes, Ordering::Relaxed);
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
                let bytes = event.len() as u64;
                self.bytes_read.fetch_add(bytes, Ordering::Relaxed);
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
        self.fsync_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn stats(&self) -> StorageStats {
        StorageStats {
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            bytes_read: self.bytes_read.load(Ordering::Relaxed),
            fsync_count: self.fsync_count.load(Ordering::Relaxed),
            corruption_errors: 0,
        }
    }
}

/// No-op network for single-node testing.
///
/// All sends succeed immediately, recv always returns None.
pub struct NoOpNetwork {
    stats: NetworkStats,
}

impl NoOpNetwork {
    /// Creates a new no-op network.
    pub fn new() -> Self {
        Self {
            stats: NetworkStats::default(),
        }
    }
}

impl Default for NoOpNetwork {
    fn default() -> Self {
        Self::new()
    }
}

impl Network for NoOpNetwork {
    fn send(&mut self, _to_replica: ReplicaId, _message: Bytes) -> Result<(), NetworkError> {
        self.stats.messages_sent += 1;
        Ok(())
    }

    fn recv(&mut self) -> Option<NetworkMessage> {
        None
    }

    fn stats(&self) -> NetworkStats {
        self.stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{NetworkError, NetworkMessage, NetworkStats, ReplicaId, StorageStats};
    use bytes::Bytes;
    use kimberlite_types::{Offset, StreamId, StreamMetadata};

    // Mock implementations for testing

    struct MockClock {
        now_ns: u64,
    }

    impl Clock for MockClock {
        fn now_ns(&self) -> u64 {
            self.now_ns
        }

        fn sleep_ns(&mut self, duration_ns: u64) {
            self.now_ns += duration_ns;
        }
    }

    struct MockStorage {
        appends: Vec<(StreamId, Offset, usize)>,
    }

    impl Storage for MockStorage {
        fn append(
            &mut self,
            stream_id: StreamId,
            base_offset: Offset,
            events: Vec<Bytes>,
        ) -> Result<(), StorageError> {
            self.appends.push((stream_id, base_offset, events.len()));
            Ok(())
        }

        fn read(
            &self,
            _stream_id: StreamId,
            _from_offset: Offset,
            _to_offset: Offset,
        ) -> Result<Vec<Bytes>, StorageError> {
            Ok(vec![])
        }

        fn write_metadata(&mut self, _metadata: StreamMetadata) -> Result<(), StorageError> {
            Ok(())
        }

        fn read_metadata(&self, _stream_id: StreamId) -> Result<StreamMetadata, StorageError> {
            Err(StorageError::StreamNotFound(StreamId::new(0)))
        }

        fn sync(&mut self) -> Result<(), StorageError> {
            Ok(())
        }

        fn stats(&self) -> StorageStats {
            StorageStats::default()
        }
    }

    struct MockNetwork;

    impl Network for MockNetwork {
        fn send(&mut self, _to_replica: ReplicaId, _message: Bytes) -> Result<(), NetworkError> {
            Ok(())
        }

        fn recv(&mut self) -> Option<NetworkMessage> {
            None
        }

        fn stats(&self) -> NetworkStats {
            NetworkStats::default()
        }
    }

    #[test]
    fn runtime_executes_storage_append() {
        let clock = MockClock { now_ns: 0 };
        let storage = MockStorage {
            appends: Vec::new(),
        };
        let network = MockNetwork;

        let mut runtime = Runtime::new(clock, storage, network);

        let effect = Effect::StorageAppend {
            stream_id: StreamId::new(1),
            base_offset: Offset::from(0u64),
            events: vec![Bytes::from("event1"), Bytes::from("event2")],
        };

        runtime.execute_effect(effect).unwrap();

        assert_eq!(runtime.storage.appends.len(), 1);
        assert_eq!(runtime.storage.appends[0].0, StreamId::new(1));
        assert_eq!(runtime.storage.appends[0].1, Offset::from(0u64));
        assert_eq!(runtime.storage.appends[0].2, 2); // 2 events
    }

    #[test]
    fn runtime_executes_multiple_effects() {
        let clock = MockClock { now_ns: 0 };
        let storage = MockStorage {
            appends: Vec::new(),
        };
        let network = MockNetwork;

        let mut runtime = Runtime::new(clock, storage, network);

        let effects = vec![
            Effect::StorageAppend {
                stream_id: StreamId::new(1),
                base_offset: Offset::from(0u64),
                events: vec![Bytes::from("e1")],
            },
            Effect::StorageAppend {
                stream_id: StreamId::new(2),
                base_offset: Offset::from(0u64),
                events: vec![Bytes::from("e2")],
            },
        ];

        runtime.execute_effects(effects).unwrap();

        assert_eq!(runtime.storage.appends.len(), 2);
    }

    #[test]
    fn clock_advances_on_sleep() {
        let mut clock = MockClock { now_ns: 1000 };

        assert_eq!(clock.now_ns(), 1000);

        clock.sleep_ns(500);
        assert_eq!(clock.now_ns(), 1500);

        clock.sleep_ns(1000);
        assert_eq!(clock.now_ns(), 2500);
    }

    #[test]
    fn runtime_accessors_work() {
        let clock = MockClock { now_ns: 12345 };
        let storage = MockStorage {
            appends: Vec::new(),
        };
        let network = MockNetwork;

        let runtime = Runtime::new(clock, storage, network);

        assert_eq!(runtime.clock().now_ns(), 12345);
        assert_eq!(runtime.storage().appends.len(), 0);
    }
}
