//! Commands that can be submitted to the kernel.
//!
//! Commands represent requests to modify system state. They are validated
//! and committed through VSR consensus before being applied to the kernel.

use bytes::Bytes;
use kimberlite_types::{DataClass, Offset, Placement, SealReason, StreamId, StreamName, TenantId};
use serde::{Deserialize, Serialize};

use crate::masking::{MaskingStrategyKind, RoleGuard};

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

    /// Add a column to an existing SQL table.
    ///
    /// Append-only semantics: existing rows on disk are NOT rewritten.
    /// Readers at schema_version N see only the N columns that existed at
    /// their read checkpoint; readers at schema_version N+1 see the new
    /// column as NULL for every pre-alter row and as the written value for
    /// every post-alter row.
    ///
    /// Preconditions:
    ///   * Table exists.
    ///   * Command's `tenant_id` matches the table's owner.
    ///   * Column name is not already present on the table.
    ///
    /// Postconditions:
    ///   * Table's `schema_version` bumps by exactly 1 (monotonicity).
    ///   * `columns` vector grows by exactly 1 entry at the end.
    ///   * An audit-log effect is emitted for the schema change.
    AlterTableAddColumn {
        tenant_id: TenantId,
        table_id: TableId,
        column: ColumnDefinition,
    },

    /// Drop a column from an existing SQL table.
    ///
    /// Append-only semantics: existing rows on disk retain the column's
    /// bytes; the planner projects them away at read time. The column name
    /// is free to be re-used by a subsequent ADD COLUMN (schema versions
    /// still advance strictly).
    ///
    /// Preconditions:
    ///   * Table exists.
    ///   * Command's `tenant_id` matches the table's owner.
    ///   * Column name is present on the table.
    ///   * Column is NOT part of the primary key (dropping a PK column
    ///     would invalidate every persisted key and is rejected at the
    ///     kernel boundary, not silently tolerated).
    ///
    /// Postconditions:
    ///   * Table's `schema_version` bumps by exactly 1.
    ///   * `columns` vector shrinks by exactly 1 entry.
    ///   * An audit-log effect is emitted for the schema change.
    AlterTableDropColumn {
        tenant_id: TenantId,
        table_id: TableId,
        column_name: String,
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

    // ------------------------------------------------------------------------
    // Masking Policy Commands (v0.6.0 Tier 2 #7)
    // ------------------------------------------------------------------------
    //
    // Column-level masking policy CRUD. The surface is SQL-native:
    //
    //   CREATE MASKING POLICY <name> AS <CASE-expr>           → CreateMaskingPolicy
    //   ALTER TABLE t ALTER COLUMN c SET MASKING POLICY <n>   → AttachMaskingPolicy
    //   ALTER TABLE t ALTER COLUMN c DROP MASKING POLICY      → DetachMaskingPolicy
    //   DROP MASKING POLICY <name>                            → DropMaskingPolicy
    //
    // Policies and attachments are durable kernel state — they round-trip
    // cleanly through backup/restore via command-log replay.
    /// Create a named masking policy in a tenant's catalogue.
    ///
    /// Preconditions:
    ///   * Policy name is unique within the tenant.
    ///
    /// Postconditions:
    ///   * `(tenant_id, name) → MaskingPolicyRecord` is stored in state.
    ///   * An audit-log effect is emitted.
    CreateMaskingPolicy {
        tenant_id: TenantId,
        name: String,
        strategy: MaskingStrategyKind,
        role_guard: RoleGuard,
    },

    /// Attach a pre-created masking policy to a specific column.
    ///
    /// Preconditions:
    ///   * Target table exists and is owned by `tenant_id`.
    ///   * Column exists on the table.
    ///   * Policy exists in the tenant's catalogue.
    ///   * The column is not already attached to a masking policy
    ///     (one-policy-per-column; detach first to change).
    ///
    /// Postconditions:
    ///   * `(tenant_id, table_id, column_name) → policy_name` is stored.
    ///   * An audit-log effect is emitted.
    AttachMaskingPolicy {
        tenant_id: TenantId,
        table_id: TableId,
        column_name: String,
        policy_name: String,
    },

    /// Detach the masking policy (if any) from a column.
    ///
    /// Preconditions:
    ///   * Target table exists and is owned by `tenant_id`.
    ///   * Column has a masking policy attached.
    ///
    /// Postconditions:
    ///   * Attachment is removed from state.
    ///   * An audit-log effect is emitted.
    DetachMaskingPolicy {
        tenant_id: TenantId,
        table_id: TableId,
        column_name: String,
    },

    /// Drop a masking policy from the tenant's catalogue.
    ///
    /// Preconditions:
    ///   * Policy exists in the tenant's catalogue.
    ///   * **No column attachments reference this policy** — detach
    ///     first. This mirrors PostgreSQL's behaviour with dependent
    ///     objects and keeps detached state from silently leaking an
    ///     un-masked column.
    ///
    /// Postconditions:
    ///   * `(tenant_id, name)` entry is removed from the catalogue.
    ///   * An audit-log effect is emitted.
    DropMaskingPolicy {
        tenant_id: TenantId,
        name: String,
    },
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
