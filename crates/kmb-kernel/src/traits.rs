//! Trait abstractions for testability.
//!
//! The kernel is pure and produces effects. A runtime layer executes these
//! effects using concrete implementations of Clock, Network, and Storage.
//!
//! This design enables:
//! - **Production**: SystemClock, TcpNetwork, FileStorage
//! - **Simulation**: SimClock, SimNetwork, SimStorage (VOPR testing)
//! - **Testing**: MockClock, MockNetwork, InMemoryStorage

use bytes::Bytes;
use kmb_types::{Offset, StreamId, StreamMetadata, TenantId};

// ============================================================================
// Clock Trait
// ============================================================================

/// Abstraction for time-based operations.
///
/// Production uses system time, simulation uses discrete event time.
pub trait Clock: Send + Sync {
    /// Returns the current time in nanoseconds since epoch.
    fn now_ns(&self) -> u64;

    /// Returns the current time in milliseconds since epoch.
    fn now_ms(&self) -> u64 {
        self.now_ns() / 1_000_000
    }

    /// Sleeps for the specified duration in nanoseconds.
    ///
    /// In simulation, this schedules a wake event rather than blocking.
    fn sleep_ns(&mut self, duration_ns: u64);
}

// ============================================================================
// Storage Trait
// ============================================================================

/// Abstraction for durable storage operations.
///
/// Production uses file-based append-only log, simulation uses in-memory
/// storage with fault injection.
pub trait Storage: Send + Sync {
    /// Appends events to a stream.
    ///
    /// # Arguments
    /// - `stream_id`: The stream to append to
    /// - `base_offset`: Starting offset for this batch
    /// - `events`: Events to persist
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(StorageError)` on failure (corruption, out of space, etc.)
    fn append(
        &mut self,
        stream_id: StreamId,
        base_offset: Offset,
        events: Vec<Bytes>,
    ) -> Result<(), StorageError>;

    /// Reads events from a stream.
    ///
    /// # Arguments
    /// - `stream_id`: The stream to read from
    /// - `from_offset`: First offset to read (inclusive)
    /// - `to_offset`: Last offset to read (exclusive)
    ///
    /// # Returns
    /// - `Ok(events)` on success
    /// - `Err(StorageError)` on failure
    fn read(
        &self,
        stream_id: StreamId,
        from_offset: Offset,
        to_offset: Offset,
    ) -> Result<Vec<Bytes>, StorageError>;

    /// Persists stream metadata.
    fn write_metadata(&mut self, metadata: StreamMetadata) -> Result<(), StorageError>;

    /// Reads stream metadata.
    fn read_metadata(&self, stream_id: StreamId) -> Result<StreamMetadata, StorageError>;

    /// Forces all buffered writes to durable storage (fsync).
    fn sync(&mut self) -> Result<(), StorageError>;

    /// Returns storage statistics (bytes written, corruption detected, etc.).
    fn stats(&self) -> StorageStats;
}

/// Statistics from the storage layer.
#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    /// Total bytes written.
    pub bytes_written: u64,
    /// Total bytes read.
    pub bytes_read: u64,
    /// Number of fsync operations.
    pub fsync_count: u64,
    /// Number of corruption errors detected.
    pub corruption_errors: u64,
}

/// Errors from storage operations.
#[derive(thiserror::Error, Debug, Clone)]
pub enum StorageError {
    #[error("stream {0} not found")]
    StreamNotFound(StreamId),

    #[error("offset {offset} out of range for stream {stream_id}")]
    OffsetOutOfRange { stream_id: StreamId, offset: Offset },

    #[error("corruption detected in stream {stream_id} at offset {offset}")]
    CorruptionDetected { stream_id: StreamId, offset: Offset },

    #[error("write failed: {0}")]
    WriteFailed(String),

    #[error("read failed: {0}")]
    ReadFailed(String),

    #[error("out of disk space")]
    OutOfSpace,

    #[error("IO error: {0}")]
    Io(String),
}

// ============================================================================
// Network Trait
// ============================================================================

/// Abstraction for network operations.
///
/// Production uses TCP/TLS connections, simulation uses message queue with
/// fault injection (delays, drops, partitions).
pub trait Network: Send + Sync {
    /// Sends a message to a replica.
    ///
    /// # Arguments
    /// - `to_replica`: Replica ID to send to
    /// - `message`: Serialized message bytes
    ///
    /// # Returns
    /// - `Ok(())` if message was sent (may still be dropped/delayed by network)
    /// - `Err(NetworkError)` if send failed immediately
    fn send(&mut self, to_replica: ReplicaId, message: Bytes) -> Result<(), NetworkError>;

    /// Receives the next message from the network.
    ///
    /// # Returns
    /// - `Some(message)` if a message is available
    /// - `None` if no messages pending
    fn recv(&mut self) -> Option<NetworkMessage>;

    /// Returns network statistics (messages sent/received, drops, etc.).
    fn stats(&self) -> NetworkStats;
}

/// A message received from the network.
#[derive(Debug, Clone)]
pub struct NetworkMessage {
    /// Source replica that sent this message.
    pub from_replica: ReplicaId,
    /// Tenant this message belongs to.
    pub tenant_id: TenantId,
    /// Serialized message payload.
    pub payload: Bytes,
}

/// Statistics from the network layer.
#[derive(Debug, Clone, Default)]
pub struct NetworkStats {
    /// Messages sent successfully.
    pub messages_sent: u64,
    /// Messages received.
    pub messages_received: u64,
    /// Messages dropped (simulation only).
    pub messages_dropped: u64,
    /// Messages delayed (simulation only).
    pub messages_delayed: u64,
}

/// Errors from network operations.
#[derive(thiserror::Error, Debug, Clone)]
pub enum NetworkError {
    #[error("connection to replica {0} failed")]
    ConnectionFailed(ReplicaId),

    #[error("message too large: {size} bytes (max: {max})")]
    MessageTooLarge { size: usize, max: usize },

    #[error("network partition: cannot reach replica {0}")]
    Partitioned(ReplicaId),

    #[error("send buffer full")]
    BufferFull,

    #[error("IO error: {0}")]
    Io(String),
}

/// Identifier for a replica in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReplicaId(pub u64);

impl ReplicaId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for ReplicaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "replica:{}", self.0)
    }
}
