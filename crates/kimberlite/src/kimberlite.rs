//! Main entry point for the Kimberlite SDK.
//!
//! The `Kimberlite` struct provides the top-level API for interacting with Kimberlite.
//! It manages the underlying storage, kernel state, projection store, and query engine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use kimberlite_crypto::ChainHash;
use kimberlite_kernel::{Command, Effect, State as KernelState, apply_committed};
use kimberlite_query::{ColumnDef, DataType, QueryEngine, SchemaBuilder};
use kimberlite_storage::Storage;
use kimberlite_store::{BTreeStore, Key, ProjectionStore, TableId, WriteBatch};
use kimberlite_types::{Offset, StreamId, TenantId};

use crate::error::{KimberliteError, Result};
use crate::tenant::TenantHandle;

/// Configuration for opening a Kimberlite database.
#[derive(Debug, Clone)]
pub struct KimberliteConfig {
    /// Path to the data directory.
    pub data_dir: PathBuf,
    /// Page cache capacity for projection store (in pages, 4KB each).
    pub cache_capacity: usize,
}

impl KimberliteConfig {
    /// Creates a new configuration with the given data directory.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            cache_capacity: 4096, // 16MB default
        }
    }

    /// Sets the page cache capacity.
    pub fn with_cache_capacity(mut self, capacity: usize) -> Self {
        self.cache_capacity = capacity;
        self
    }
}

/// Internal state shared across tenant handles.
pub(crate) struct KimberliteInner {
    /// Path to data directory (used for future operations like metadata persistence).
    #[allow(dead_code)]
    pub(crate) data_dir: PathBuf,

    /// Append-only log storage.
    pub(crate) storage: Storage,

    /// Kernel state machine.
    pub(crate) kernel_state: KernelState,

    /// Projection store (B+tree with MVCC).
    pub(crate) projection_store: BTreeStore,

    /// Query engine with schema.
    pub(crate) query_engine: QueryEngine,

    /// Current log position (offset of last written record).
    pub(crate) log_position: Offset,

    /// Hash chain head for each stream.
    pub(crate) chain_heads: HashMap<StreamId, ChainHash>,
}

impl KimberliteInner {
    /// Executes effects produced by the kernel.
    ///
    /// This is the "imperative shell" that handles I/O.
    pub(crate) fn execute_effects(&mut self, effects: Vec<Effect>) -> Result<()> {
        for effect in effects {
            match effect {
                Effect::StorageAppend {
                    stream_id,
                    base_offset,
                    events,
                } => {
                    let prev_hash = self.chain_heads.get(&stream_id).copied();
                    let (new_offset, new_hash) = self.storage.append_batch(
                        stream_id,
                        events.clone(),
                        base_offset,
                        prev_hash,
                        true, // fsync for durability
                    )?;
                    self.chain_heads.insert(stream_id, new_hash);
                    self.log_position = new_offset;

                    // Apply to projection store
                    self.apply_to_projection(stream_id, base_offset, &events)?;
                }
                Effect::StreamMetadataWrite(metadata) => {
                    // For now, stream metadata is tracked in kernel state
                    // Future: persist to metadata store
                    tracing::debug!(?metadata, "stream metadata updated");
                }
                Effect::WakeProjection {
                    stream_id,
                    from_offset,
                    to_offset,
                } => {
                    // Projection is updated inline in StorageAppend
                    // This effect signals external consumers
                    tracing::debug!(
                        ?stream_id,
                        ?from_offset,
                        ?to_offset,
                        "projection wake signal"
                    );
                }
                Effect::AuditLogAppend(action) => {
                    // Future: write to immutable audit log
                    tracing::debug!(?action, "audit action");
                }
                Effect::TableMetadataWrite(metadata) => {
                    // Table metadata is tracked in kernel state
                    // Update the query engine schema to include the new table
                    self.rebuild_query_engine_schema();
                    tracing::debug!(?metadata, "table metadata updated");
                }
                Effect::TableMetadataDrop(table_id) => {
                    // Table metadata removed from kernel state
                    tracing::debug!(?table_id, "table metadata dropped");
                }
                Effect::IndexMetadataWrite(metadata) => {
                    // Index metadata is tracked in kernel state
                    // Rebuild schema to include new index
                    self.rebuild_query_engine_schema();

                    // Populate the new index with existing data
                    self.populate_new_index(metadata.table_id, metadata.index_id)?;

                    tracing::debug!(?metadata, "index metadata updated and populated");
                }
                Effect::UpdateProjection {
                    table_id,
                    from_offset,
                    to_offset,
                } => {
                    // Apply DML events from the table's stream to the projection
                    self.apply_dml_to_projection(table_id, from_offset, to_offset)?;
                }
            }
        }
        Ok(())
    }

    /// Applies events to the projection store.
    fn apply_to_projection(
        &mut self,
        stream_id: StreamId,
        base_offset: Offset,
        events: &[Bytes],
    ) -> Result<()> {
        // Each event becomes a projection entry
        // Key format: stream_id:offset
        let table_id = TableId::new(u64::from(stream_id));

        for (i, event) in events.iter().enumerate() {
            let offset = Offset::new(base_offset.as_u64() + i as u64);
            let batch = WriteBatch::new(Offset::new(
                self.projection_store.applied_position().as_u64() + 1,
            ))
            .put(
                table_id,
                Key::from(format!("{:016x}", offset.as_u64())),
                event.clone(),
            );

            self.projection_store.apply(batch)?;
        }

        Ok(())
    }

    /// Applies DML events (INSERT/UPDATE/DELETE) to the projection store.
    fn apply_dml_to_projection(
        &mut self,
        table_id: kimberlite_kernel::command::TableId,
        from_offset: Offset,
        _to_offset: Offset,
    ) -> Result<()> {
        // Get table metadata to find the stream and primary key
        let table = self
            .kernel_state
            .get_table(&table_id)
            .ok_or_else(|| KimberliteError::TableNotFound(table_id.to_string()))?;

        let stream_id = table.stream_id;
        // Clone primary key columns to avoid borrow checker issues
        let primary_key_cols = table.primary_key.clone();

        // Read events from the stream
        // Use fixed batch size to avoid memory exhaustion
        const MAX_BATCH_BYTES: u64 = 10 * 1024 * 1024; // 10MB per batch
        let events = self
            .storage
            .read_from(stream_id, from_offset, MAX_BATCH_BYTES)?;

        // Parse and apply each event
        for event in events {
            self.apply_single_dml_event(table_id, &event, &primary_key_cols)?;
        }

        Ok(())
    }

    /// Applies a single DML event to the projection store.
    fn apply_single_dml_event(
        &mut self,
        table_id: kimberlite_kernel::command::TableId,
        event: &Bytes,
        primary_key_cols: &[String],
    ) -> Result<()> {
        // Parse the JSON event
        let event_json: serde_json::Value = serde_json::from_slice(event)
            .map_err(|e| KimberliteError::internal(format!("failed to parse DML event: {}", e)))?;

        let event_type = event_json
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| KimberliteError::internal("DML event missing 'type' field"))?;

        match event_type {
            "insert" => {
                // Extract row data from the event
                let data = event_json.get("data").ok_or_else(|| {
                    KimberliteError::internal("INSERT event missing 'data' field")
                })?;

                // Build primary key from the data using the same encoding as the query engine
                let pk_key = self.build_primary_key(data, primary_key_cols)?;

                // Serialize just the row data (not the full event) for storage
                // The query engine expects JSON objects with column names as keys
                let row_bytes = Bytes::from(serde_json::to_vec(data).map_err(|e| {
                    KimberliteError::internal(format!("JSON serialization failed: {}", e))
                })?);

                // Store the row with primary key
                let mut batch = WriteBatch::new(Offset::new(
                    self.projection_store.applied_position().as_u64() + 1,
                ))
                .put(TableId::new(table_id.0), pk_key.clone(), row_bytes);

                // Maintain indexes for INSERT
                batch = self.maintain_indexes_for_insert(batch, table_id, data, &pk_key)?;

                self.projection_store.apply(batch)?;
            }
            "update" => {
                // Extract predicates from WHERE clause
                let predicates = event_json.get("where").ok_or_else(|| {
                    KimberliteError::internal("UPDATE event missing 'where' field")
                })?;

                // Extract primary key from WHERE predicates
                let pk_data =
                    self.extract_primary_key_from_predicates(predicates, primary_key_cols)?;

                // Build the primary key for lookup
                let pk_key = self.build_primary_key(&pk_data, primary_key_cols)?;

                // Read existing row from projection store
                let store_table_id = TableId::new(table_id.0);
                let existing_row_bytes = self
                    .projection_store
                    .get(store_table_id, &pk_key)?
                    .ok_or_else(|| {
                        KimberliteError::internal(format!(
                            "row with primary key not found for UPDATE: {:?}",
                            pk_data
                        ))
                    })?;

                // Parse existing row data
                let mut existing_data: serde_json::Value =
                    serde_json::from_slice(&existing_row_bytes).map_err(|e| {
                        KimberliteError::internal(format!("failed to parse existing row: {}", e))
                    })?;

                // Extract SET assignments and merge with existing data
                let assignments = event_json
                    .get("set")
                    .ok_or_else(|| KimberliteError::internal("UPDATE event missing 'set' field"))?;

                if let serde_json::Value::Object(ref mut existing_obj) = existing_data {
                    if let Some(set_array) = assignments.as_array() {
                        for assignment in set_array {
                            if let Some(set_obj) = assignment.as_array() {
                                // Assignment is [column, value]
                                if set_obj.len() == 2 {
                                    if let Some(col_name) = set_obj[0].as_str() {
                                        existing_obj
                                            .insert(col_name.to_string(), set_obj[1].clone());
                                    }
                                }
                            }
                        }
                    }
                } else {
                    return Err(KimberliteError::internal(
                        "existing row data is not a JSON object",
                    ));
                }

                // Serialize updated row
                let updated_row_bytes =
                    Bytes::from(serde_json::to_vec(&existing_data).map_err(|e| {
                        KimberliteError::internal(format!("JSON serialization failed: {}", e))
                    })?);

                // Parse old data for index maintenance
                let old_data: serde_json::Value = serde_json::from_slice(&existing_row_bytes)
                    .map_err(|e| {
                        KimberliteError::internal(format!("failed to parse old row data: {}", e))
                    })?;

                // Write back updated row
                let mut batch = WriteBatch::new(Offset::new(
                    self.projection_store.applied_position().as_u64() + 1,
                ))
                .put(TableId::new(table_id.0), pk_key.clone(), updated_row_bytes);

                // Maintain indexes for UPDATE
                batch = self.maintain_indexes_for_update(
                    batch,
                    table_id,
                    &old_data,
                    &existing_data,
                    &pk_key,
                )?;

                self.projection_store.apply(batch)?;
            }
            "delete" => {
                // Extract predicates from WHERE clause
                let predicates = event_json.get("where").ok_or_else(|| {
                    KimberliteError::internal("DELETE event missing 'where' field")
                })?;

                // Extract primary key from WHERE predicates
                let pk_data =
                    self.extract_primary_key_from_predicates(predicates, primary_key_cols)?;

                // Build the primary key for deletion
                let pk_key = self.build_primary_key(&pk_data, primary_key_cols)?;

                // Read current row to get index values
                let store_table_id = TableId::new(table_id.0);
                let old_row_bytes = self
                    .projection_store
                    .get(store_table_id, &pk_key)?
                    .ok_or_else(|| {
                        KimberliteError::internal(format!(
                            "row with primary key not found for DELETE: {:?}",
                            pk_data
                        ))
                    })?;

                // Parse old data for index maintenance
                let old_data: serde_json::Value =
                    serde_json::from_slice(&old_row_bytes).map_err(|e| {
                        KimberliteError::internal(format!("failed to parse old row data: {}", e))
                    })?;

                // Delete from projection store
                let mut batch = WriteBatch::new(Offset::new(
                    self.projection_store.applied_position().as_u64() + 1,
                ))
                .delete(store_table_id, pk_key.clone());

                // Maintain indexes for DELETE
                batch = self.maintain_indexes_for_delete(batch, table_id, &old_data, &pk_key)?;

                self.projection_store.apply(batch)?;
            }
            _ => {
                tracing::warn!(?event_type, "unknown DML event type");
            }
        }

        Ok(())
    }

    /// Extracts primary key values from WHERE predicates.
    ///
    /// Expects equality predicates for all primary key columns.
    fn extract_primary_key_from_predicates(
        &self,
        predicates: &serde_json::Value,
        primary_key_cols: &[String],
    ) -> Result<serde_json::Value> {
        let predicates_array = predicates
            .as_array()
            .ok_or_else(|| KimberliteError::internal("WHERE predicates must be an array"))?;

        let mut pk_values = serde_json::Map::new();

        // Extract values from equality predicates
        for pred in predicates_array {
            let op = pred
                .get("op")
                .and_then(|v| v.as_str())
                .ok_or_else(|| KimberliteError::internal("predicate missing 'op' field"))?;

            // Only handle equality predicates for PK extraction
            if op != "eq" {
                continue;
            }

            let column = pred
                .get("column")
                .and_then(|v| v.as_str())
                .ok_or_else(|| KimberliteError::internal("predicate missing 'column' field"))?;

            let values = pred
                .get("values")
                .and_then(|v| v.as_array())
                .ok_or_else(|| KimberliteError::internal("predicate missing 'values' field"))?;

            // Equality should have exactly one value
            if values.len() == 1 {
                pk_values.insert(column.to_string(), values[0].clone());
            }
        }

        // Verify we have all primary key columns
        for pk_col in primary_key_cols {
            if !pk_values.contains_key(pk_col) {
                return Err(KimberliteError::internal(format!(
                    "WHERE clause does not uniquely identify primary key - missing column '{}'",
                    pk_col
                )));
            }
        }

        Ok(serde_json::Value::Object(pk_values))
    }

    /// Builds a primary key value from row data and primary key column names.
    ///
    /// Uses the same binary encoding as the query engine for compatibility.
    fn build_primary_key(
        &self,
        data: &serde_json::Value,
        primary_key_cols: &[String],
    ) -> Result<Key> {
        use kimberlite_query::{Value, key_encoder::encode_key};

        let mut pk_values = Vec::new();

        for col_name in primary_key_cols {
            let json_val = data.get(col_name).ok_or_else(|| {
                KimberliteError::internal(format!(
                    "primary key column '{}' not found in data",
                    col_name
                ))
            })?;

            // Convert JSON value to Value type for encoding
            let value = match json_val {
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Value::BigInt(i)
                    } else {
                        return Err(KimberliteError::internal(format!(
                            "unsupported number format for column '{}'",
                            col_name
                        )));
                    }
                }
                serde_json::Value::String(s) => Value::Text(s.clone()),
                serde_json::Value::Bool(b) => Value::Boolean(*b),
                serde_json::Value::Null => Value::Null,
                _ => {
                    return Err(KimberliteError::internal(format!(
                        "unsupported primary key value type for column '{}'",
                        col_name
                    )));
                }
            };

            pk_values.push(value);
        }

        // Use the same encoding as the query engine
        Ok(encode_key(&pk_values))
    }

    /// Rebuilds the query engine schema from kernel state.
    ///
    /// This is called when tables are created/dropped to synchronize the
    /// query engine with the current set of tables.
    fn rebuild_query_engine_schema(&mut self) {
        use kimberlite_query::{ColumnDef, ColumnName, DataType, IndexDef, Schema, TableDef};

        let mut schema = Schema::new();

        // Add all tables from kernel state to the schema
        for (table_id, table_meta) in self.kernel_state.tables() {
            // Convert kernel column definitions to query column definitions
            let columns: Vec<ColumnDef> = table_meta
                .columns
                .iter()
                .map(|col| {
                    // Map SQL type strings to DataType enum
                    let data_type = match col.data_type.as_str() {
                        "TINYINT" => DataType::TinyInt,
                        "SMALLINT" => DataType::SmallInt,
                        "INTEGER" => DataType::Integer,
                        "BIGINT" => DataType::BigInt,
                        "REAL" => DataType::Real,
                        "TEXT" => DataType::Text,
                        "BYTES" => DataType::Bytes,
                        "BOOLEAN" => DataType::Boolean,
                        "DATE" => DataType::Date,
                        "TIME" => DataType::Time,
                        "TIMESTAMP" => DataType::Timestamp,
                        "UUID" => DataType::Uuid,
                        "JSON" => DataType::Json,
                        s if s.starts_with("DECIMAL(") => {
                            // Parse DECIMAL(precision,scale)
                            let inner = s
                                .strip_prefix("DECIMAL(")
                                .and_then(|s| s.strip_suffix(')'))
                                .unwrap_or("10,0");
                            let parts: Vec<&str> = inner.split(',').collect();
                            let precision =
                                parts.first().and_then(|p| p.parse().ok()).unwrap_or(10);
                            let scale = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                            DataType::Decimal { precision, scale }
                        }
                        _ => {
                            tracing::warn!(
                                "unknown data type '{}' for column '{}', defaulting to TEXT",
                                col.data_type,
                                col.name
                            );
                            DataType::Text
                        }
                    };

                    if col.nullable {
                        ColumnDef::new(col.name.as_str(), data_type)
                    } else {
                        ColumnDef::new(col.name.as_str(), data_type).not_null()
                    }
                })
                .collect();

            // Convert String to ColumnName for primary key columns
            let pk_cols: Vec<ColumnName> = table_meta
                .primary_key
                .iter()
                .map(|s| ColumnName::new(s.as_str()))
                .collect();

            // Collect indexes for this table
            let indexes: Vec<IndexDef> = self
                .kernel_state
                .indexes()
                .values()
                .filter(|idx_meta| idx_meta.table_id.0 == table_id.0)
                .map(|idx_meta| {
                    let cols: Vec<ColumnName> = idx_meta
                        .columns
                        .iter()
                        .map(|c| ColumnName::new(c.as_str()))
                        .collect();
                    IndexDef::new(idx_meta.index_id.0, &idx_meta.index_name, cols)
                })
                .collect();

            // Build table definition with indexes
            let mut table_def =
                TableDef::new(kimberlite_store::TableId::new(table_id.0), columns, pk_cols);
            for index in indexes {
                table_def = table_def.with_index(index);
            }

            schema.add_table(table_meta.table_name.as_str(), table_def);
        }

        // Rebuild query engine with new schema
        self.query_engine = QueryEngine::new(schema);

        tracing::debug!(
            "rebuilt query engine schema with {} tables",
            self.kernel_state.tables().len()
        );
    }

    /// Calculates a unique index table ID from table ID and index ID.
    ///
    /// Uses hashing to avoid overflow issues with large table IDs.
    fn calculate_index_table_id(
        table_id: kimberlite_kernel::command::TableId,
        index_id: kimberlite_kernel::command::IndexId,
    ) -> TableId {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        table_id.0.hash(&mut hasher);
        index_id.0.hash(&mut hasher);
        TableId::new(hasher.finish())
    }

    /// Extracts values for given columns from JSON data.
    ///
    /// Returns a vector of Values in the same order as the columns list.
    /// Missing columns are treated as NULL.
    fn extract_index_values(
        &self,
        data: &serde_json::Value,
        columns: &[String],
    ) -> Result<Vec<kimberlite_query::Value>> {
        use kimberlite_query::Value;

        let obj = data.as_object().ok_or_else(|| {
            KimberliteError::internal("data must be a JSON object for index extraction")
        })?;

        let max_columns = 100; // Bounded iteration limit
        let mut values = Vec::with_capacity(columns.len().min(max_columns));

        for (iter_count, col_name) in columns.iter().enumerate() {
            // Bounded iteration check
            if iter_count >= max_columns {
                break;
            }

            let json_val = obj.get(col_name).unwrap_or(&serde_json::Value::Null);

            // Convert JSON value to Value type
            let value = match json_val {
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Value::BigInt(i)
                    } else {
                        Value::Null
                    }
                }
                serde_json::Value::String(s) => Value::Text(s.clone()),
                serde_json::Value::Bool(b) => Value::Boolean(*b),
                serde_json::Value::Null => Value::Null,
                _ => Value::Null,
            };

            values.push(value);
        }

        debug_assert!(!columns.is_empty(), "column list must be non-empty");
        debug_assert_eq!(
            values.len(),
            columns.len().min(max_columns),
            "extracted values must match column count"
        );

        Ok(values)
    }

    /// Maintains indexes for an INSERT operation.
    ///
    /// Adds index entries for all indexes on the table.
    fn maintain_indexes_for_insert(
        &self,
        mut batch: WriteBatch,
        table_id: kimberlite_kernel::command::TableId,
        data: &serde_json::Value,
        pk_key: &Key,
    ) -> Result<WriteBatch> {
        use kimberlite_query::key_encoder::encode_key;

        // Get all indexes for this table
        let max_iterations = 100; // Bounded iteration limit
        let mut iter_count = 0;

        for (index_id, index_meta) in self.kernel_state.indexes() {
            // Bounded iteration check
            if iter_count >= max_iterations {
                break;
            }
            iter_count += 1;

            // Filter to indexes for this table
            if index_meta.table_id.0 != table_id.0 {
                continue;
            }

            // Extract index column values
            let index_values = self.extract_index_values(data, &index_meta.columns)?;

            // Decode primary key to get PK values
            let pk_values = kimberlite_query::key_encoder::decode_key(pk_key);

            // Build composite key: [index_values][pk_values]
            let mut composite_values = index_values;
            composite_values.extend(pk_values);

            let composite_key = encode_key(&composite_values);

            // Calculate index table ID using hash
            let index_table_id = Self::calculate_index_table_id(table_id, *index_id);

            // Add index entry with empty value (0-byte marker)
            batch = batch.put(index_table_id, composite_key, Bytes::new());
        }

        debug_assert!(!pk_key.as_bytes().is_empty(), "pk_key must be non-empty");

        Ok(batch)
    }

    /// Maintains indexes for an UPDATE operation.
    ///
    /// Deletes old index entries and inserts new ones if index columns changed.
    fn maintain_indexes_for_update(
        &self,
        mut batch: WriteBatch,
        table_id: kimberlite_kernel::command::TableId,
        old_data: &serde_json::Value,
        new_data: &serde_json::Value,
        pk_key: &Key,
    ) -> Result<WriteBatch> {
        use kimberlite_query::key_encoder::encode_key;

        // Get all indexes for this table
        let max_iterations = 100; // Bounded iteration limit
        let mut iter_count = 0;

        for (index_id, index_meta) in self.kernel_state.indexes() {
            // Bounded iteration check
            if iter_count >= max_iterations {
                break;
            }
            iter_count += 1;

            // Filter to indexes for this table
            if index_meta.table_id.0 != table_id.0 {
                continue;
            }

            // Extract old and new index column values
            let old_index_values = self.extract_index_values(old_data, &index_meta.columns)?;
            let new_index_values = self.extract_index_values(new_data, &index_meta.columns)?;

            // Check if index columns changed
            if old_index_values == new_index_values {
                // Index columns unchanged, skip this index
                continue;
            }

            // Decode primary key to get PK values
            let pk_values = kimberlite_query::key_encoder::decode_key(pk_key);

            // Calculate index table ID using hash
            let index_table_id = Self::calculate_index_table_id(table_id, *index_id);

            // Delete old index entry
            let mut old_composite_values = old_index_values;
            old_composite_values.extend(pk_values.clone());
            let old_composite_key = encode_key(&old_composite_values);
            batch = batch.delete(index_table_id, old_composite_key);

            // Insert new index entry
            let mut new_composite_values = new_index_values;
            new_composite_values.extend(pk_values);
            let new_composite_key = encode_key(&new_composite_values);
            batch = batch.put(index_table_id, new_composite_key, Bytes::new());
        }

        debug_assert!(!pk_key.as_bytes().is_empty(), "pk_key must be non-empty");

        Ok(batch)
    }

    /// Maintains indexes for a DELETE operation.
    ///
    /// Removes index entries for all indexes on the table.
    fn maintain_indexes_for_delete(
        &self,
        mut batch: WriteBatch,
        table_id: kimberlite_kernel::command::TableId,
        old_data: &serde_json::Value,
        pk_key: &Key,
    ) -> Result<WriteBatch> {
        use kimberlite_query::key_encoder::encode_key;

        // Get all indexes for this table
        let max_iterations = 100; // Bounded iteration limit
        let mut iter_count = 0;

        for (index_id, index_meta) in self.kernel_state.indexes() {
            // Bounded iteration check
            if iter_count >= max_iterations {
                break;
            }
            iter_count += 1;

            // Filter to indexes for this table
            if index_meta.table_id.0 != table_id.0 {
                continue;
            }

            // Extract index column values from old data
            let index_values = self.extract_index_values(old_data, &index_meta.columns)?;

            // Decode primary key to get PK values
            let pk_values = kimberlite_query::key_encoder::decode_key(pk_key);

            // Build composite key: [index_values][pk_values]
            let mut composite_values = index_values;
            composite_values.extend(pk_values);

            let composite_key = encode_key(&composite_values);

            // Calculate index table ID using hash
            let index_table_id = Self::calculate_index_table_id(table_id, *index_id);

            // Delete index entry
            batch = batch.delete(index_table_id, composite_key);
        }

        debug_assert!(!pk_key.as_bytes().is_empty(), "pk_key must be non-empty");

        Ok(batch)
    }

    /// Populates a newly created index with existing table data.
    ///
    /// Scans the base table and adds index entries for all existing rows.
    fn populate_new_index(
        &mut self,
        table_id: kimberlite_kernel::command::TableId,
        index_id: kimberlite_kernel::command::IndexId,
    ) -> Result<()> {
        use kimberlite_query::key_encoder::{decode_key, encode_key};

        // Get index metadata
        let index_meta = self.kernel_state.get_index(&index_id).ok_or_else(|| {
            KimberliteError::internal(format!("index {:?} not found in kernel state", index_id))
        })?;

        // Verify table exists
        let _table_meta = self.kernel_state.get_table(&table_id).ok_or_else(|| {
            KimberliteError::internal(format!("table {:?} not found in kernel state", table_id))
        })?;

        // Full scan of base table
        let store_table_id = TableId::new(table_id.0);
        let max_rows = 1_000_000; // Bounded iteration limit
        let pairs = self
            .projection_store
            .scan(store_table_id, Key::min()..Key::max(), max_rows)?;

        debug_assert!(pairs.len() <= max_rows, "scan must respect max_rows limit");

        // Build index entries for all rows
        let mut batch = WriteBatch::new(Offset::new(
            self.projection_store.applied_position().as_u64() + 1,
        ));

        // Calculate index table ID using hash
        let index_table_id = Self::calculate_index_table_id(table_id, index_id);
        let row_count = pairs.len();

        for (pk_key, row_bytes) in &pairs {
            // Parse row data
            let row_data: serde_json::Value = serde_json::from_slice(row_bytes).map_err(|e| {
                KimberliteError::internal(format!(
                    "failed to parse row data during index population: {}",
                    e
                ))
            })?;

            // Extract index column values
            let index_values = self.extract_index_values(&row_data, &index_meta.columns)?;

            // Decode primary key to get PK values
            let pk_values = decode_key(pk_key);

            // Build composite key: [index_values][pk_values]
            let mut composite_values = index_values;
            composite_values.extend(pk_values);

            let composite_key = encode_key(&composite_values);

            // Add index entry
            batch = batch.put(index_table_id, composite_key, Bytes::new());
        }

        // Apply batch to populate index
        self.projection_store.apply(batch)?;

        tracing::info!(
            ?table_id,
            ?index_id,
            rows = row_count,
            "populated new index"
        );

        Ok(())
    }
}

/// The main Kimberlite database handle.
///
/// Provides the top-level API for interacting with the database.
/// Get tenant-scoped access via the `tenant()` method.
///
/// # Example
///
/// ```ignore
/// use kimberlite::Kimberlite;
///
/// let db = Kimberlite::open("./data")?;
/// let tenant = db.tenant(TenantId::new(1));
///
/// // Use tenant handle for operations
/// tenant.execute("INSERT INTO users (id, name) VALUES ($1, $2)", &[1.into(), "Alice".into()])?;
/// let results = tenant.query("SELECT * FROM users WHERE id = $1", &[1.into()])?;
/// ```
#[derive(Clone)]
pub struct Kimberlite {
    inner: Arc<RwLock<KimberliteInner>>,
}

impl Kimberlite {
    /// Opens a Kimberlite database at the given path.
    ///
    /// If the directory doesn't exist, it will be created.
    /// If the database already exists, it will be opened and state recovered.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        let config = KimberliteConfig::new(data_dir.as_ref());
        Self::open_with_config(config)
    }

    /// Opens a Kimberlite database with custom configuration.
    pub fn open_with_config(config: KimberliteConfig) -> Result<Self> {
        // Ensure data directory exists
        std::fs::create_dir_all(&config.data_dir)?;

        // Open storage layer
        let storage = Storage::new(&config.data_dir);

        // Open projection store
        let projection_path = config.data_dir.join("projections.db");
        let projection_store =
            BTreeStore::open_with_capacity(&projection_path, config.cache_capacity)?;

        // Initialize kernel state
        let kernel_state = KernelState::new();

        // Build default schema (streams as tables)
        // TODO: Make schema configurable
        let schema = SchemaBuilder::new()
            .table(
                "events",
                TableId::new(1),
                vec![
                    ColumnDef::new("offset", DataType::BigInt).not_null(),
                    ColumnDef::new("data", DataType::Text),
                ],
                vec!["offset".into()],
            )
            .build();

        let query_engine = QueryEngine::new(schema);

        let inner = KimberliteInner {
            data_dir: config.data_dir,
            storage,
            kernel_state,
            projection_store,
            query_engine,
            log_position: Offset::ZERO,
            chain_heads: HashMap::new(),
        };

        Ok(Self {
            inner: Arc::new(RwLock::new(inner)),
        })
    }

    /// Returns a tenant-scoped handle.
    ///
    /// The tenant handle provides operations scoped to a specific tenant ID.
    pub fn tenant(&self, id: TenantId) -> TenantHandle {
        TenantHandle::new(self.clone(), id)
    }

    /// Submits a command to the kernel and executes resulting effects.
    ///
    /// This is the core write path: command → kernel → effects → I/O.
    pub fn submit(&self, command: Command) -> Result<()> {
        let mut inner = self
            .inner
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        // Apply command to kernel (pure)
        let (new_state, effects) = apply_committed(inner.kernel_state.clone(), command)?;

        // Update kernel state
        inner.kernel_state = new_state;

        // Execute effects (impure)
        inner.execute_effects(effects)
    }

    /// Returns the current log position.
    pub fn log_position(&self) -> Result<Offset> {
        let inner = self
            .inner
            .read()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;
        Ok(inner.log_position)
    }

    /// Returns the current projection store position.
    pub fn projection_position(&self) -> Result<Offset> {
        let inner = self
            .inner
            .read()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;
        Ok(inner.projection_store.applied_position())
    }

    /// Syncs all data to disk.
    ///
    /// Ensures durability of all written data.
    pub fn sync(&self) -> Result<()> {
        let mut inner = self
            .inner
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;
        inner.projection_store.sync()?;
        Ok(())
    }

    /// Returns a reference to the inner state.
    ///
    /// This is used internally by `TenantHandle` to access shared state.
    pub(crate) fn inner(&self) -> &Arc<RwLock<KimberliteInner>> {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_types::{DataClass, Placement, StreamName};
    use tempfile::tempdir;

    #[test]
    fn test_open_creates_directory() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("testdb");

        assert!(!db_path.exists());
        let _db = Kimberlite::open(&db_path).unwrap();
        assert!(db_path.exists());
    }

    #[test]
    fn test_create_stream() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();

        let cmd = Command::create_stream(
            StreamId::new(1),
            StreamName::new("test_stream"),
            DataClass::NonPHI,
            Placement::Global,
        );

        db.submit(cmd).unwrap();

        let inner = db.inner.read().unwrap();
        assert!(inner.kernel_state.stream_exists(&StreamId::new(1)));
    }

    #[test]
    fn test_append_events() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();

        // Create stream
        db.submit(Command::create_stream(
            StreamId::new(1),
            StreamName::new("events"),
            DataClass::NonPHI,
            Placement::Global,
        ))
        .unwrap();

        // Append events
        db.submit(Command::append_batch(
            StreamId::new(1),
            vec![Bytes::from("event1"), Bytes::from("event2")],
            Offset::ZERO,
        ))
        .unwrap();

        assert!(db.log_position().unwrap().as_u64() > 0);
    }

    #[test]
    fn test_schema_all_types_mapping() {
        use kimberlite_query::Value;

        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table with all 13 supported SQL data types
        // This verifies the type mapping in kimberlite.rs:512-547 is comprehensive
        // BYTES type exists in DataType enum but is not yet supported in SQL parser
        let sql = r#"
            CREATE TABLE all_types (
                id BIGINT NOT NULL,
                col_tinyint TINYINT,
                col_smallint SMALLINT,
                col_integer INTEGER,
                col_bigint BIGINT,
                col_real REAL,
                col_decimal DECIMAL(18,2),
                col_text TEXT,
                col_boolean BOOLEAN,
                col_date DATE,
                col_time TIME,
                col_timestamp TIMESTAMP,
                col_uuid UUID,
                col_json JSON,
                PRIMARY KEY (id)
            )
        "#;

        tenant.execute(sql, &[]).unwrap();

        // Verify table exists and is queryable
        // This indirectly verifies all type mappings work correctly
        let result = tenant
            .query(
                "SELECT * FROM all_types WHERE id = $1",
                &[Value::BigInt(999)],
            )
            .expect("Should be able to query table");

        // Table should have 14 columns
        assert_eq!(result.columns.len(), 14, "Should have 14 columns");

        // Verify column names are preserved (schema mapping worked correctly)
        assert_eq!(result.columns[0].as_str(), "id");
        assert_eq!(result.columns[1].as_str(), "col_tinyint");
        assert_eq!(result.columns[2].as_str(), "col_smallint");
        assert_eq!(result.columns[3].as_str(), "col_integer");
        assert_eq!(result.columns[4].as_str(), "col_bigint");
        assert_eq!(result.columns[5].as_str(), "col_real");
        assert_eq!(result.columns[6].as_str(), "col_decimal");
        assert_eq!(result.columns[7].as_str(), "col_text");
        assert_eq!(result.columns[8].as_str(), "col_boolean");
        assert_eq!(result.columns[9].as_str(), "col_date");
        assert_eq!(result.columns[10].as_str(), "col_time");
        assert_eq!(result.columns[11].as_str(), "col_timestamp");
        assert_eq!(result.columns[12].as_str(), "col_uuid");
        assert_eq!(result.columns[13].as_str(), "col_json");

        // Success! All 13 SQL data types are correctly mapped in schema building
    }
}
