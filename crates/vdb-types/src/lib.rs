//! # vdb-types: Core types for VerityDB
//!
//! This crate contains shared types used across the VerityDB system:
//! - Entity IDs ([`TenantId`], [`StreamId`], [`Offset`], [`GroupId`])
//! - Data classification ([`DataClass`])
//! - Placement rules ([`Placement`], [`Region`])
//! - Stream metadata ([`StreamMetadata`])
//! - Audit actions ([`AuditAction`])

use std::{
    fmt::Display,
    ops::{Add, AddAssign, Sub},
};

use bytes::Bytes;
use serde::{Deserialize, Serialize};

// ============================================================================
// Entity IDs
// ============================================================================

/// Unique identifier for a tenant (organization/customer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TenantId(u64);

impl TenantId {
    /// Creates a new tenant ID.
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl From<u64> for TenantId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<TenantId> for u64 {
    fn from(id: TenantId) -> Self {
        id.0
    }
}

/// Unique identifier for a stream within the system.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct StreamId(u64);

impl StreamId {
    /// Creates a new stream ID.
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl Display for StreamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for StreamId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<StreamId> for u64 {
    fn from(id: StreamId) -> Self {
        id.0
    }
}

/// Position of an event within a stream.
///
/// Offsets are zero-indexed and sequential. The first event in a stream
/// has offset 0, the second has offset 1, and so on.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct Offset(u64);

impl Offset {
    /// Creates a new offset.
    pub fn new(offset: u64) -> Self {
        Self(offset)
    }

    /// Returns the offset as a u64.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Display for Offset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add for Offset {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Offset {
    fn add_assign(&mut self, rhs: Self) {
        self = self + rhs;
    }
}

impl Sub for Offset {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl From<u64> for Offset {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<Offset> for u64 {
    fn from(offset: Offset) -> Self {
        offset.0
    }
}

/// Unique identifier for a replication group.
///
/// Streams are assigned to groups based on their placement policy.
/// Each group runs its own VSR consensus instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GroupId(u64);

impl GroupId {
    /// Creates a new group ID.
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl From<u64> for GroupId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<GroupId> for u64 {
    fn from(id: GroupId) -> Self {
        id.0
    }
}

// ============================================================================
// Stream Name
// ============================================================================

/// Human-readable name for a stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StreamName(String);

impl StreamName {
    /// Creates a new stream name.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for StreamName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for StreamName {
    fn from(name: String) -> Self {
        Self(name)
    }
}

impl From<&str> for StreamName {
    fn from(name: &str) -> Self {
        Self(name.to_string())
    }
}

impl From<StreamName> for String {
    fn from(value: StreamName) -> Self {
        value.0
    }
}

// ============================================================================
// Data Classification
// ============================================================================

/// Classification of data for compliance purposes.
///
/// This determines how data is handled, encrypted, and where it can be stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DataClass {
    /// Protected Health Information - subject to HIPAA restrictions.
    /// Must be encrypted at rest and in transit, with strict access controls.
    PHI,
    /// Non-PHI data that doesn't contain health information.
    /// Still encrypted but with fewer placement restrictions.
    NonPHI,
    /// Data that has been de-identified per HIPAA Safe Harbor or Expert Determination.
    /// Can be replicated globally and used for analytics.
    Deidentified,
}

// ============================================================================
// Placement
// ============================================================================

/// Placement policy for a stream.
///
/// Determines where data can be stored and replicated.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Placement {
    /// Data must remain within the specified region.
    /// Required for PHI to comply with data residency requirements.
    Region(Region),
    /// Data can be replicated globally across all regions.
    /// Only valid for NonPHI or Deidentified data.
    Global,
}

/// Geographic region for data placement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Region {
    /// US East (N. Virginia) - us-east-1
    USEast1,
    /// Asia Pacific (Sydney) - ap-southeast-2
    APSoutheast2,
    /// Custom region identifier
    Custom(String),
    // TODO: Add more default regions (eu-west-1, etc.)
}

impl Region {
    /// Creates a custom region with the given identifier.
    pub fn custom(name: impl Into<String>) -> Self {
        Self::Custom(name.into())
    }
}

impl Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Region::USEast1 => write!(f, "us-east-1"),
            Region::APSoutheast2 => write!(f, "ap-southeast-2"),
            Region::Custom(custom) => write!(f, "{custom}"),
        }
    }
}

// ============================================================================
// Stream Metadata
// ============================================================================

/// Metadata describing a stream's configuration and current state.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StreamMetadata {
    /// Unique identifier for this stream.
    pub stream_id: StreamId,
    /// Human-readable name.
    pub stream_name: StreamName,
    /// Data classification for compliance.
    pub data_class: DataClass,
    /// Where this stream's data must reside.
    pub placement: Placement,
    /// Current offset (number of events in the stream).
    pub current_offset: Offset,
}

impl StreamMetadata {
    /// Creates new stream metadata with offset initialized to 0.
    pub fn new(
        stream_id: StreamId,
        stream_name: StreamName,
        data_class: DataClass,
        placement: Placement,
    ) -> Self {
        Self {
            stream_id,
            stream_name,
            data_class,
            placement,
            current_offset: Offset::default(),
        }
    }
}

// ============================================================================
// Batch payload
// ============================================================================
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BatchPayload {
    /// The stream to append to.
    pub stream_id: StreamId,
    /// The events to append (serialized as bytes).
    pub events: Vec<Bytes>,
    /// Expected current offset for optimistic concurrency.
    /// If the stream's actual offset differs, the command fails.
    pub expected_offset: Offset,
}

// ============================================================================
// Audit Actions
// ============================================================================

/// Actions recorded in the audit log.
///
/// Every state-changing operation produces an audit action for compliance tracking.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AuditAction {
    /// A new stream was created.
    StreamCreated {
        stream_id: StreamId,
        stream_name: StreamName,
        data_class: DataClass,
        placement: Placement,
    },
    /// Events were appended to a stream.
    EventsAppended {
        stream_id: StreamId,
        /// Number of events appended.
        count: u32,
        /// Starting offset of the appended events.
        from_offset: Offset,
    },
    // TODO: StreamArchived, PolicyChanged, etc.
}
