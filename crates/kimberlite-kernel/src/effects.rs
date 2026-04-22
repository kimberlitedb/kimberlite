//! Effects produced by the kernel.
//!
//! Effects represent side effects that the runtime must execute after
//! a command is applied. The kernel is pure - it produces effects but
//! never executes them directly.

use bytes::Bytes;
use kimberlite_types::{AuditAction, Offset, StreamId, StreamMetadata, TenantId};
use serde::{Deserialize, Serialize};

use crate::command::TableId;
use crate::masking::MaskingPolicyRecord;
use crate::state::{IndexMetadata, TableMetadata};

/// An effect to be executed by the runtime.
///
/// Effects are produced by [`super::kernel::apply_committed`] and describe
/// actions that must be performed outside the pure kernel (storage writes,
/// projection updates, audit logging).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    // ========================================================================
    // Event Stream Effects
    // ========================================================================
    /// Write events to the durable storage layer.
    StorageAppend {
        /// The stream to append to.
        stream_id: StreamId,
        /// Starting offset for this batch.
        base_offset: Offset,
        /// The events to persist.
        events: Vec<Bytes>,
    },

    /// Persist stream metadata to the metadata store.
    StreamMetadataWrite(StreamMetadata),

    /// Notify projections that new events are available.
    WakeProjection {
        /// The stream with new events.
        stream_id: StreamId,
        /// First new event offset (inclusive).
        from_offset: Offset,
        /// Last new event offset (exclusive).
        to_offset: Offset,
    },

    /// Append an entry to the immutable audit log.
    AuditLogAppend(AuditAction),

    // ========================================================================
    // DDL Effects (schema changes)
    // ========================================================================
    /// Persist table metadata after CREATE TABLE.
    ///
    /// The embedded `TableMetadata` carries `tenant_id`, so runtime fan-out
    /// has the tenant context it needs without a separate field here.
    TableMetadataWrite(TableMetadata),

    /// Remove table metadata after DROP TABLE.
    TableMetadataDrop {
        tenant_id: TenantId,
        table_id: TableId,
    },

    /// Persist index metadata after CREATE INDEX.
    IndexMetadataWrite(IndexMetadata),

    // ========================================================================
    // DML Effects (data manipulation)
    // ========================================================================
    /// Update projection after INSERT/UPDATE/DELETE.
    ///
    /// The projection engine reads the event from the stream and applies
    /// it to the B+tree store.
    UpdateProjection {
        tenant_id: TenantId,
        table_id: TableId,
        from_offset: Offset,
        to_offset: Offset,
    },

    // ========================================================================
    // Masking Policy Effects (v0.6.0 Tier 2 #7)
    // ========================================================================
    /// Persist a newly-created masking policy record.
    MaskingPolicyWrite(MaskingPolicyRecord),

    /// Remove a masking policy record from durable storage.
    MaskingPolicyDrop { tenant_id: TenantId, name: String },

    /// Persist a new (table, column) → policy attachment.
    MaskingAttachmentWrite {
        tenant_id: TenantId,
        table_id: TableId,
        column_name: String,
        policy_name: String,
    },

    /// Remove a (table, column) → policy attachment.
    MaskingAttachmentDrop {
        tenant_id: TenantId,
        table_id: TableId,
        column_name: String,
    },
}
