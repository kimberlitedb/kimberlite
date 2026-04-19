//! Commands that can be submitted to the kernel.
//!
//! Commands represent requests to modify system state. They are validated
//! and committed through VSR consensus before being applied to the kernel.

use bytes::Bytes;
use kimberlite_types::{DataClass, Offset, Placement, SealReason, StreamId, StreamName, TenantId};
use serde::{Deserialize, Serialize};

// ============================================================================
// Schema Types (simplified for kernel use)
// ============================================================================

/// SQL column definition for CREATE TABLE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnDefinition {
    pub name: String,
    pub data_type: String, // "BIGINT", "TEXT", "BOOLEAN", etc.
    pub nullable: bool,
}

/// SQL table ID (maps to underlying stream).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct TableId(pub u64);

impl TableId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

/// SQL index ID.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct IndexId(pub u64);

impl IndexId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for IndexId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for TableId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// Command Enum
// ============================================================================

/// A command to be applied to the kernel.
///
/// Commands are the inputs to the kernel's state machine. Each command
/// is validated, proposed to VSR, and once committed, applied to produce
/// a new state and effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    // ------------------------------------------------------------------------
    // Event Stream Commands (existing)
    // ------------------------------------------------------------------------
    /// Create a new event stream.
    CreateStream {
        stream_id: StreamId,
        stream_name: StreamName,
        data_class: DataClass,
        placement: Placement,
    },

    /// Create a stream with an auto-allocated ID
    CreateStreamWithAutoId {
        stream_name: StreamName,
        data_class: DataClass,
        placement: Placement,
    },

    /// Append a batch of events to an existing stream.
    AppendBatch {
        stream_id: StreamId,
        events: Vec<Bytes>,
        expected_offset: Offset,
    },

    // ------------------------------------------------------------------------
    // DDL Commands (new - SQL table management)
    // ------------------------------------------------------------------------
    //
    // Every DDL/DML command that references a table carries `tenant_id`.
    // Kernel handlers enforce that the tenant_id on the command matches
    // the tenant_id stored on the table's metadata. Cross-tenant access
    // is a compliance-grade safety violation and panics in production.
    /// Create a new SQL table.
    CreateTable {
        tenant_id: TenantId,
        table_id: TableId,
        table_name: String,
        columns: Vec<ColumnDefinition>,
        primary_key: Vec<String>, // Column names forming the primary key
    },

    /// Drop an existing SQL table.
    DropTable {
        tenant_id: TenantId,
        table_id: TableId,
    },

    /// Create a secondary index on a table.
    CreateIndex {
        tenant_id: TenantId,
        index_id: IndexId,
        table_id: TableId,
        index_name: String,
        columns: Vec<String>, // Column names in index
    },

    // ------------------------------------------------------------------------
    // DML Commands (new - SQL data manipulation)
    // ------------------------------------------------------------------------
    /// Insert a row into a table.
    Insert {
        tenant_id: TenantId,
        table_id: TableId,
        row_data: Bytes, // Serialized row (JSON or bincode)
    },

    /// Update rows matching a condition.
    Update {
        tenant_id: TenantId,
        table_id: TableId,
        row_data: Bytes, // Contains key + changes
    },

    /// Delete rows matching a condition.
    Delete {
        tenant_id: TenantId,
        table_id: TableId,
        row_data: Bytes, // Contains key to delete
    },

    // ------------------------------------------------------------------------
    // Tenant Lifecycle Commands (AUDIT-2026-04 H-5)
    // ------------------------------------------------------------------------
    //
    // Sealing is a standard healthcare-compliance SOP: freeze writes to
    // a tenant during forensic/audit/legal-hold operations while
    // keeping reads consistent. Before AUDIT-2026-04, no primitive
    // existed in the kernel for this — scripts resorted to ad-hoc
    // blocks at the API layer that could be bypassed by internal
    // callers. SealTenant makes the freeze structural.
    /// Seal a tenant against further mutation. Reads remain allowed.
    SealTenant {
        tenant_id: TenantId,
        reason: SealReason,
        /// Unix-ns timestamp the seal took effect. The runtime
        /// supplies this from its clock; the core is pure over the
        /// value.
        sealed_at_ns: u64,
    },

    /// Unseal a previously-sealed tenant. Mutations resume.
    UnsealTenant { tenant_id: TenantId },
}

impl Command {
    /// Creates a new `CreateStream` command.
    ///
    /// Takes ownership of `stream_name` and placement (heap data).
    /// `StreamId` and `DataClass` are Copy.
    pub fn create_stream(
        stream_id: StreamId,
        stream_name: StreamName,
        data_class: DataClass,
        placement: Placement,
    ) -> Self {
        Self::CreateStream {
            stream_id,
            stream_name,
            data_class,
            placement,
        }
    }

    pub fn create_stream_with_auto_id(
        stream_name: StreamName,
        data_class: DataClass,
        placement: Placement,
    ) -> Self {
        Self::CreateStreamWithAutoId {
            stream_name,
            data_class,
            placement,
        }
    }

    /// Creates a new `AppendBatch` command.
    pub fn append_batch(stream_id: StreamId, events: Vec<Bytes>, expected_offset: Offset) -> Self {
        Self::AppendBatch {
            stream_id,
            events,
            expected_offset,
        }
    }
}
