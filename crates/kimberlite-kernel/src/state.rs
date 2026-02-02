//! Kernel state management.
//!
//! The kernel maintains in-memory state that tracks all streams and their
//! current offsets. State transitions are done by taking ownership and
//! returning a new state (builder pattern).

use std::collections::BTreeMap;

use kimberlite_types::{DataClass, Offset, Placement, StreamId, StreamMetadata, StreamName};
use serde::{Deserialize, Serialize};

use crate::command::{ColumnDefinition, IndexId, TableId};

// ============================================================================
// Table Metadata
// ============================================================================

/// Metadata for a SQL table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableMetadata {
    pub table_id: TableId,
    pub table_name: String,
    pub columns: Vec<ColumnDefinition>,
    pub primary_key: Vec<String>,
    /// Underlying stream that stores this table's events.
    pub stream_id: StreamId,
}

/// Metadata for a SQL index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexMetadata {
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
    table_name_index: BTreeMap<String, TableId>,

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
    pub(crate) fn with_stream(mut self, meta: StreamMetadata) -> Self {
        self.streams.insert(meta.stream_id, meta);
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
    pub(crate) fn streams(&self) -> &BTreeMap<StreamId, StreamMetadata> {
        &self.streams
    }

    /// Creates a new stream with an auto-allocated ID.
    ///
    /// This is atomic - the ID allocation and stream insertion happen together,
    /// making it impossible to allocate an ID without creating the stream.
    pub(crate) fn with_new_stream(
        mut self,
        stream_name: StreamName,
        data_class: DataClass,
        placement: Placement,
    ) -> (Self, StreamMetadata) {
        let stream_id = self.next_stream_id;
        self.next_stream_id = self.next_stream_id + StreamId::new(1);

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

    /// Returns true if a table with the given name exists.
    pub fn table_name_exists(&self, name: &str) -> bool {
        self.table_name_index.contains_key(name)
    }

    /// Returns the metadata for a table, if it exists.
    pub fn get_table(&self, id: &TableId) -> Option<&TableMetadata> {
        self.tables.get(id)
    }

    /// Returns a reference to all tables.
    pub fn tables(&self) -> &std::collections::BTreeMap<TableId, TableMetadata> {
        &self.tables
    }

    /// Adds a table with pre-set metadata and returns the updated state.
    ///
    /// Internal to the kernel - external code should use `apply_committed`.
    pub(crate) fn with_table_metadata(mut self, meta: TableMetadata) -> Self {
        self.table_name_index
            .insert(meta.table_name.clone(), meta.table_id);
        self.tables.insert(meta.table_id, meta);
        self
    }

    /// Removes a table and returns the updated state.
    pub(crate) fn without_table(mut self, id: TableId) -> Self {
        if let Some(meta) = self.tables.remove(&id) {
            self.table_name_index.remove(&meta.table_name);
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
    pub fn table_name_index(&self) -> &BTreeMap<String, TableId> {
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
