//! Kernel state management.
//!
//! The kernel maintains in-memory state that tracks all streams and their
//! current offsets. State transitions are done by taking ownership and
//! returning a new state (builder pattern).

use std::collections::BTreeMap;

use kimberlite_types::{
    DataClass, Offset, Placement, StreamId, StreamMetadata, StreamName, TenantId,
};
use serde::{Deserialize, Serialize};

use crate::command::{ColumnDefinition, IndexId, TableId};

// ============================================================================
// Table Metadata
// ============================================================================

/// Metadata for a SQL table.
///
/// `tenant_id` is the owning tenant; the kernel enforces that any DDL/DML
/// command referencing this table carries the same tenant id. Tables with
/// the same `table_name` may coexist across different tenants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableMetadata {
    pub tenant_id: TenantId,
    pub table_id: TableId,
    pub table_name: String,
    pub columns: Vec<ColumnDefinition>,
    pub primary_key: Vec<String>,
    /// Underlying stream that stores this table's events.
    pub stream_id: StreamId,
}

/// Metadata for a SQL index.
///
/// `tenant_id` mirrors the owning table's tenant for symmetry and to keep
/// any index-level reads scoped the same way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexMetadata {
    pub tenant_id: TenantId,
    pub index_id: IndexId,
    pub index_name: String,
    pub table_id: TableId,
    pub columns: Vec<String>,
}

// ============================================================================
// Kernel State
// ============================================================================

/// The kernel's in-memory state.
///
/// State uses a builder pattern - methods take ownership of `self`, mutate,
/// and return `self`. This supports the functional core pattern while
/// avoiding unnecessary clones of the internal `BTreeMap`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct State {
    // Event streams
    streams: BTreeMap<StreamId, StreamMetadata>,
    next_stream_id: StreamId,

    // SQL tables
    tables: BTreeMap<TableId, TableMetadata>,
    next_table_id: TableId,
    /// Tenant-scoped name index: two tenants can own tables of the same name.
    ///
    /// A prior global `BTreeMap<String, TableId>` silently collapsed
    /// different tenants' catalogs — the isolation leak this index exists
    /// to prevent.
    table_name_index: BTreeMap<(TenantId, String), TableId>,

    // SQL indexes
    indexes: BTreeMap<IndexId, IndexMetadata>,
    next_index_id: IndexId,
}

impl State {
    /// Creates a new empty state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the metadata for a stream, if it exists.
    pub fn get_stream(&self, id: &StreamId) -> Option<&StreamMetadata> {
        self.streams.get(id)
    }

    /// Returns true if a stream with the given ID exists.
    pub fn stream_exists(&self, id: &StreamId) -> bool {
        self.streams.contains_key(id)
    }

    /// Adds a stream and returns the updated state.
    ///
    /// Internal to the kernel - external code should use `apply_committed`
    /// which handles validation and effects.
    ///
    /// Also advances `next_stream_id` past any explicit id >= the counter's
    /// current value. Without this, a later `with_new_stream` could pick
    /// the exact slot the explicit-id stream landed on — the two would
    /// share backing storage and every append would see the other's events.
    pub(crate) fn with_stream(mut self, meta: StreamMetadata) -> Self {
        let taken = meta.stream_id;
        self.streams.insert(taken, meta);
        if taken >= self.next_stream_id {
            self.next_stream_id = taken + StreamId::new(1);
        }
        self
    }

    /// Updates a stream's offset and returns the updated state.
    ///
    /// If the stream doesn't exist, returns self unchanged.
    ///
    /// Internal to the kernel - external code should use `apply_committed`
    /// which handles validation and effects.
    pub(crate) fn with_updated_offset(mut self, id: StreamId, new_offset: Offset) -> Self {
        if let Some(stream) = self.streams.get_mut(&id) {
            stream.current_offset = new_offset;
        }
        self
    }

    /// Returns the number of streams in the state.
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }

    /// Returns the next stream ID that will be allocated.
    pub fn next_stream_id(&self) -> StreamId {
        self.next_stream_id
    }

    /// Returns a reference to all streams.
    ///
    /// Made `pub` so observability callers outside the kernel (e.g. the
    /// chaos probe surface on `kimberlite-server`) can iterate streams
    /// without cloning the full state — reads are immutable and cheap.
    pub fn streams(&self) -> &BTreeMap<StreamId, StreamMetadata> {
        &self.streams
    }

    /// Creates a new stream with an auto-allocated ID.
    ///
    /// This is atomic - the ID allocation and stream insertion happen together,
    /// making it impossible to allocate an ID without creating the stream.
    ///
    /// The allocator walks `next_stream_id` forward until it lands on an
    /// unused slot. That walk is necessary because explicit-id streams
    /// (created via `Command::CreateStream` with a caller-chosen StreamId —
    /// for example, the `(tenant_id << 32) | local_id` scheme tenants use
    /// for application-level streams) do NOT advance `next_stream_id`. A
    /// naked `streams.insert(next_stream_id, …)` could otherwise clobber
    /// an explicit-id stream that happened to land on the same slot, and
    /// the two streams would then share backing storage — user events
    /// interleave with DML events and every subsequent append on either
    /// stream sees an offset view of events written for the other.
    pub(crate) fn with_new_stream(
        mut self,
        stream_name: StreamName,
        data_class: DataClass,
        placement: Placement,
    ) -> (Self, StreamMetadata) {
        while self.streams.contains_key(&self.next_stream_id) {
            self.next_stream_id = self.next_stream_id + StreamId::new(1);
        }
        let stream_id = self.next_stream_id;
        self.next_stream_id = self.next_stream_id + StreamId::new(1);

        // Invariant: we never overwrite an existing slot. The skip-forward
        // loop above is what makes that true in the presence of
        // explicit-id `CreateStream` commands.
        debug_assert!(
            !self.streams.contains_key(&stream_id),
            "with_new_stream must allocate a fresh slot"
        );

        let meta = StreamMetadata::new(stream_id, stream_name, data_class, placement);
        self.streams.insert(stream_id, meta.clone());

        (self, meta)
    }

    // ========================================================================
    // Table Management
    // ========================================================================

    /// Returns true if a table with the given ID exists.
    pub fn table_exists(&self, id: &TableId) -> bool {
        self.tables.contains_key(id)
    }

    /// Returns true if the given tenant owns a table with this name.
    ///
    /// A prior `table_name_exists(name)` accessor was global and let tenant
    /// A's catalog leak into tenant B's lookup. Callers must scope by
    /// tenant; the only callers without a tenant context are checkpoint /
    /// serialization paths that iterate `tables()` directly.
    pub fn table_name_exists_for_tenant(&self, tenant_id: TenantId, name: &str) -> bool {
        self.table_name_index
            .contains_key(&(tenant_id, name.to_string()))
    }

    /// Returns the metadata for a table, if it exists.
    ///
    /// Note: this does NOT verify tenant ownership. Use the command-level
    /// tenant check in `apply_committed` before acting on the result, or
    /// call [`Self::table_by_tenant_name`] when starting from a name.
    pub fn get_table(&self, id: &TableId) -> Option<&TableMetadata> {
        self.tables.get(id)
    }

    /// Returns the table metadata for `(tenant_id, name)` if present.
    ///
    /// This is the correct accessor for application-layer DDL/DML: never
    /// iterate `tables()` globally searching by name.
    pub fn table_by_tenant_name(
        &self,
        tenant_id: TenantId,
        name: &str,
    ) -> Option<&TableMetadata> {
        let table_id = self
            .table_name_index
            .get(&(tenant_id, name.to_string()))?;
        self.tables.get(table_id)
    }

    /// Returns an iterator over tables owned by a single tenant.
    ///
    /// Backed by a range scan on `table_name_index`, then a lookup per id.
    /// Prefer this over `tables().iter().filter(...)` to make the tenant
    /// filter part of the type/contract, not an easy-to-forget closure.
    pub fn tables_for_tenant(
        &self,
        tenant_id: TenantId,
    ) -> impl Iterator<Item = &TableMetadata> {
        let start = (tenant_id, String::new());
        let end = (TenantId::from(u64::from(tenant_id).saturating_add(1)), String::new());
        self.table_name_index
            .range(start..end)
            .filter_map(move |((t, _), table_id)| {
                debug_assert_eq!(*t, tenant_id);
                self.tables.get(table_id)
            })
    }

    /// Returns a reference to all tables.
    ///
    /// Reserved for checkpoint/restore and kernel-internal iteration.
    /// Application-layer code must use [`Self::tables_for_tenant`] or
    /// [`Self::table_by_tenant_name`] — iterating globally and filtering
    /// by `meta.table_name` is the shape of the isolation bug this module
    /// was hardened against.
    pub fn tables(&self) -> &std::collections::BTreeMap<TableId, TableMetadata> {
        &self.tables
    }

    /// Adds a table with pre-set metadata and returns the updated state.
    ///
    /// Internal to the kernel - external code should use `apply_committed`.
    pub(crate) fn with_table_metadata(mut self, meta: TableMetadata) -> Self {
        self.table_name_index
            .insert((meta.tenant_id, meta.table_name.clone()), meta.table_id);
        self.tables.insert(meta.table_id, meta);
        self
    }

    /// Removes a table and returns the updated state.
    pub(crate) fn without_table(mut self, id: TableId) -> Self {
        if let Some(meta) = self.tables.remove(&id) {
            self.table_name_index
                .remove(&(meta.tenant_id, meta.table_name));
        }
        self
    }

    /// Returns the number of tables.
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    /// Returns the next table ID that will be allocated.
    pub fn next_table_id(&self) -> TableId {
        self.next_table_id
    }

    /// Returns a reference to the table name index.
    pub fn table_name_index(&self) -> &BTreeMap<(TenantId, String), TableId> {
        &self.table_name_index
    }

    /// Returns the number of entries in the table name index.
    pub fn table_name_index_len(&self) -> usize {
        self.table_name_index.len()
    }

    // ========================================================================
    // Index Management
    // ========================================================================

    /// Returns true if an index with the given ID exists.
    pub fn index_exists(&self, id: &IndexId) -> bool {
        self.indexes.contains_key(id)
    }

    /// Returns the metadata for an index, if it exists.
    pub fn get_index(&self, id: &IndexId) -> Option<&IndexMetadata> {
        self.indexes.get(id)
    }

    /// Returns a reference to all indexes.
    pub fn indexes(&self) -> &std::collections::BTreeMap<IndexId, IndexMetadata> {
        &self.indexes
    }

    /// Adds an index and returns the updated state.
    pub(crate) fn with_index(mut self, meta: IndexMetadata) -> Self {
        self.indexes.insert(meta.index_id, meta);
        self
    }

    /// Returns the number of indexes.
    pub fn index_count(&self) -> usize {
        self.indexes.len()
    }

    /// Returns the next index ID that will be allocated.
    pub fn next_index_id(&self) -> IndexId {
        self.next_index_id
    }
}
