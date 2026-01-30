//! Tenant-scoped handle for database operations.
//!
//! A `TenantHandle` provides operations scoped to a specific tenant ID.
//! All data isolation and access control is handled at this layer.

use bytes::Bytes;
use kmb_kernel::Command;
use kmb_kernel::command::{ColumnDefinition, IndexId, TableId};
use kmb_query::{
    ColumnName, ParsedCreateIndex, ParsedCreateTable, ParsedDelete, ParsedInsert, ParsedUpdate,
    QueryResult, Value, key_encoder::encode_key,
};
use kmb_store::ProjectionStore;
use kmb_types::{DataClass, Offset, Placement, StreamId, StreamName, TenantId};
use serde_json::json;

use crate::error::{KimberliteError, Result};
use crate::kimberlite::Kimberlite;

/// Result of executing a DDL/DML statement.
#[derive(Debug, Clone)]
pub enum ExecuteResult {
    /// Standard DML result (rows affected, log offset)
    Standard {
        rows_affected: u64,
        log_offset: Offset,
    },
    /// DML with RETURNING clause (includes returned rows)
    WithReturning {
        rows_affected: u64,
        log_offset: Offset,
        returned: QueryResult,
    },
}

impl ExecuteResult {
    /// Get the number of rows affected by the operation.
    pub fn rows_affected(&self) -> u64 {
        match self {
            ExecuteResult::Standard { rows_affected, .. } => *rows_affected,
            ExecuteResult::WithReturning { rows_affected, .. } => *rows_affected,
        }
    }

    /// Get the log offset of the operation.
    pub fn log_offset(&self) -> Offset {
        match self {
            ExecuteResult::Standard { log_offset, .. } => *log_offset,
            ExecuteResult::WithReturning { log_offset, .. } => *log_offset,
        }
    }

    /// Get the returned rows, if this is a WithReturning result.
    pub fn returned(&self) -> Option<&QueryResult> {
        match self {
            ExecuteResult::Standard { .. } => None,
            ExecuteResult::WithReturning { returned, .. } => Some(returned),
        }
    }
}

/// A tenant-scoped handle for database operations.
///
/// All operations through this handle are scoped to the tenant ID
/// specified when creating the handle.
///
/// # Example
///
/// ```ignore
/// let db = Kimberlite::open("./data")?;
/// let tenant = db.tenant(TenantId::new(1));
///
/// // Create a stream for this tenant
/// tenant.create_stream("orders", DataClass::NonPHI)?;
///
/// // Append events
/// tenant.append("orders", vec![b"order_created".to_vec()])?;
///
/// // Query data
/// let results = tenant.query("SELECT * FROM events LIMIT 10", &[])?;
/// ```
#[derive(Clone)]
pub struct TenantHandle {
    db: Kimberlite,
    tenant_id: TenantId,
}

impl TenantHandle {
    /// Creates a new tenant handle.
    pub(crate) fn new(db: Kimberlite, tenant_id: TenantId) -> Self {
        Self { db, tenant_id }
    }

    /// Returns the tenant ID for this handle.
    pub fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }

    /// Creates a new stream for this tenant.
    ///
    /// # Arguments
    ///
    /// * `name` - Stream name (must be unique within the tenant)
    /// * `data_class` - Data classification (`PHI`, `NonPHI`, etc.)
    ///
    /// # Example
    ///
    /// ```ignore
    /// tenant.create_stream("audit_log", DataClass::PHI)?;
    /// ```
    pub fn create_stream(
        &self,
        name: impl Into<String>,
        data_class: DataClass,
    ) -> Result<StreamId> {
        let stream_name = StreamName::new(name);

        // For now, use tenant_id as stream_id base
        // Future: proper stream ID allocation
        let tenant_id_val: u64 = self.tenant_id.into();
        let stream_id = StreamId::new(tenant_id_val * 1_000_000 + 1);

        self.db.submit(Command::create_stream(
            stream_id,
            stream_name,
            data_class,
            Placement::Global,
        ))?;

        Ok(stream_id)
    }

    /// Creates a stream with automatic ID allocation.
    pub fn create_stream_auto(&self, name: impl Into<String>, data_class: DataClass) -> Result<()> {
        let stream_name = StreamName::new(name);

        self.db.submit(Command::create_stream_with_auto_id(
            stream_name,
            data_class,
            Placement::Global,
        ))
    }

    /// Appends events to a stream.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - The stream to append to
    /// * `events` - Events to append
    ///
    /// # Returns
    ///
    /// Returns the offset of the first appended event.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let events = vec![
    ///     b"event1".to_vec(),
    ///     b"event2".to_vec(),
    /// ];
    /// let offset = tenant.append(stream_id, events)?;
    /// ```
    pub fn append(&self, stream_id: StreamId, events: Vec<Vec<u8>>) -> Result<Offset> {
        let inner = self
            .db
            .inner()
            .read()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        // Get current stream offset
        let stream = inner
            .kernel_state
            .get_stream(&stream_id)
            .ok_or(KimberliteError::StreamNotFound(stream_id))?;

        let expected_offset = stream.current_offset;
        drop(inner);

        let events: Vec<Bytes> = events.into_iter().map(Bytes::from).collect();

        self.db
            .submit(Command::append_batch(stream_id, events, expected_offset))?;

        // Return the offset of the first event (which is expected_offset, the starting offset)
        Ok(expected_offset)
    }

    /// Executes a write operation via SQL.
    ///
    /// Note: In Kimberlite, writes go through the append-only log.
    /// SQL writes are translated to stream appends.
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL statement (INSERT, UPDATE, DELETE, CREATE TABLE, etc.)
    /// * `params` - Query parameters
    ///
    /// # Example
    ///
    /// ```ignore
    /// tenant.execute(
    ///     "INSERT INTO users (id, name) VALUES ($1, $2)",
    ///     &[Value::BigInt(1), Value::Text("Alice".into())],
    /// )?;
    /// ```
    pub fn execute(&self, sql: &str, params: &[Value]) -> Result<ExecuteResult> {
        use kmb_query::{ParsedStatement, parse_statement};

        // Parse the SQL statement
        let parsed = parse_statement(sql)?;

        match parsed {
            ParsedStatement::Select(_) => {
                // SELECT goes through query path, not execute
                Err(KimberliteError::Query(
                    kmb_query::QueryError::UnsupportedFeature(
                        "use query() for SELECT statements".to_string(),
                    ),
                ))
            }

            ParsedStatement::CreateTable(create_table) => self.execute_create_table(create_table),

            ParsedStatement::DropTable(table_name) => self.execute_drop_table(&table_name),

            ParsedStatement::CreateIndex(create_index) => self.execute_create_index(create_index),

            ParsedStatement::Insert(insert) => self.execute_insert(insert, params),

            ParsedStatement::Update(update) => self.execute_update(update, params),

            ParsedStatement::Delete(delete) => self.execute_delete(delete, params),
        }
    }

    /// Executes a SQL query against the current state.
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL SELECT statement
    /// * `params` - Query parameters
    ///
    /// # Returns
    ///
    /// Query results as rows.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let results = tenant.query(
    ///     "SELECT * FROM events WHERE offset > $1 LIMIT 10",
    ///     &[Value::BigInt(100)],
    /// )?;
    /// for row in results.rows() {
    ///     println!("{:?}", row);
    /// }
    /// ```
    pub fn query(&self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        let mut inner = self
            .db
            .inner()
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        // Clone the query engine to work around borrow checker
        // This is cheap since QueryEngine only holds a Schema reference
        let engine = inner.query_engine.clone();
        let result = engine.query(&mut inner.projection_store, sql, params)?;

        Ok(result)
    }

    /// Executes a SQL query at a specific log position (point-in-time query).
    ///
    /// This is essential for compliance: query the state as it was at a
    /// specific point in the log.
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL SELECT statement
    /// * `params` - Query parameters
    /// * `position` - Log position to query at
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Get state as of log position 1000
    /// let results = tenant.query_at(
    ///     "SELECT * FROM users WHERE id = $1",
    ///     &[Value::BigInt(42)],
    ///     Offset::new(1000),
    /// )?;
    /// ```
    pub fn query_at(&self, sql: &str, params: &[Value], position: Offset) -> Result<QueryResult> {
        let mut inner = self
            .db
            .inner()
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        // Validate position is not ahead of current
        let current_pos = kmb_store::ProjectionStore::applied_position(&inner.projection_store);
        if position > current_pos {
            return Err(KimberliteError::PositionAhead {
                requested: position,
                current: current_pos,
            });
        }

        // Clone the query engine to work around borrow checker
        let engine = inner.query_engine.clone();
        let result = engine.query_at(&mut inner.projection_store, sql, params, position)?;

        Ok(result)
    }

    /// Returns the current log position for this tenant.
    pub fn log_position(&self) -> Result<Offset> {
        self.db.log_position()
    }

    /// Reads events from a stream starting at an offset.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - The stream to read from
    /// * `from_offset` - Starting offset (inclusive)
    /// * `max_bytes` - Maximum number of bytes to read
    ///
    /// # Example
    ///
    /// ```ignore
    /// let events = tenant.read_events(stream_id, Offset::ZERO, 1024 * 1024_u64)?;
    /// for event in events {
    ///     process(event);
    /// }
    /// ```
    pub fn read_events(
        &self,
        stream_id: StreamId,
        from_offset: Offset,
        max_bytes: u64,
    ) -> Result<Vec<Bytes>> {
        let inner = self
            .db
            .inner()
            .read()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let events = inner.storage.read_from(stream_id, from_offset, max_bytes)?;
        Ok(events)
    }

    // ========================================================================
    // DDL/DML Implementation Helpers
    // ========================================================================

    fn execute_create_table(&self, create_table: ParsedCreateTable) -> Result<ExecuteResult> {
        // Validate that a primary key is defined
        if create_table.primary_key.is_empty() {
            return Err(KimberliteError::Query(kmb_query::QueryError::ParseError(
                format!(
                    "table '{}' must have a PRIMARY KEY defined",
                    create_table.table_name
                ),
            )));
        }

        // Generate a unique table ID based on table name hash
        // This is a temporary solution; a proper implementation would use
        // an ID allocator from the kernel state
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.tenant_id.hash(&mut hasher);
        create_table.table_name.hash(&mut hasher);
        let table_id = TableId::new(hasher.finish());

        // Convert parsed columns to kernel ColumnDefinitions
        let columns = create_table
            .columns
            .into_iter()
            .map(|col| ColumnDefinition {
                name: col.name,
                data_type: col.data_type,
                nullable: col.nullable,
            })
            .collect();

        let cmd = Command::CreateTable {
            table_id,
            table_name: create_table.table_name,
            columns,
            primary_key: create_table.primary_key,
        };

        self.db.submit(cmd)?;

        Ok(ExecuteResult::Standard {
            rows_affected: 0,
            log_offset: self.log_position()?,
        })
    }

    fn execute_drop_table(&self, table_name: &str) -> Result<ExecuteResult> {
        // Look up table ID by name
        let inner = self
            .db
            .inner()
            .read()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        // Find table by name in kernel state
        let table_id = inner
            .kernel_state
            .tables()
            .iter()
            .find(|(_, meta)| meta.table_name == table_name)
            .map(|(id, _)| *id)
            .ok_or_else(|| KimberliteError::TableNotFound(table_name.to_string()))?;

        drop(inner);

        let cmd = Command::DropTable { table_id };
        self.db.submit(cmd)?;

        Ok(ExecuteResult::Standard {
            rows_affected: 0,
            log_offset: self.log_position()?,
        })
    }

    fn execute_create_index(&self, create_index: ParsedCreateIndex) -> Result<ExecuteResult> {
        // Look up table ID
        let inner = self
            .db
            .inner()
            .read()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let table_id = inner
            .kernel_state
            .tables()
            .iter()
            .find(|(_, meta)| meta.table_name == create_index.table_name)
            .map(|(id, _)| *id)
            .ok_or_else(|| KimberliteError::TableNotFound(create_index.table_name.clone()))?;

        drop(inner);

        // Auto-allocate index ID based on hash of table name + index name
        // This avoids overflow issues with large table IDs
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        table_id.hash(&mut hasher);
        create_index.index_name.hash(&mut hasher);
        let index_id = IndexId::new(hasher.finish());

        let cmd = Command::CreateIndex {
            index_id,
            table_id,
            index_name: create_index.index_name,
            columns: create_index.columns,
        };

        self.db.submit(cmd)?;

        Ok(ExecuteResult::Standard {
            rows_affected: 0,
            log_offset: self.log_position()?,
        })
    }

    fn execute_insert(&self, insert: ParsedInsert, params: &[Value]) -> Result<ExecuteResult> {
        // Look up table metadata
        let inner = self
            .db
            .inner()
            .read()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let (table_id, table_meta) = inner
            .kernel_state
            .tables()
            .iter()
            .find(|(_, meta)| meta.table_name == insert.table)
            .map(|(id, meta)| (*id, meta.clone()))
            .ok_or_else(|| KimberliteError::TableNotFound(insert.table.clone()))?;

        drop(inner);

        // Determine column names to use
        let column_names: Vec<String> = if insert.columns.is_empty() {
            // No columns specified - use all columns from table definition in order
            table_meta
                .columns
                .iter()
                .map(|col| col.name.clone())
                .collect()
        } else {
            // Validate that specified columns exist
            validate_columns_exist(&insert.columns, &table_meta.columns)?;
            insert.columns.clone()
        };

        // Process each row
        let mut rows_affected = 0;
        let mut last_offset = self.log_position()?;
        let mut inserted_pk_keys = Vec::new();  // Track PKs for RETURNING

        for row_values in &insert.values {
            // Validate value count matches column count for this row
            if column_names.len() != row_values.len() {
                return Err(KimberliteError::Query(
                    kmb_query::QueryError::TypeMismatch {
                        expected: format!("{} values", column_names.len()),
                        actual: format!("{} values provided", row_values.len()),
                    },
                ));
            }

            // Bind parameters to placeholders for this row
            let bound_values = bind_parameters(row_values, params)?;

            // Validate column types and constraints for this row
            validate_insert_values(
                &column_names,
                &bound_values,
                &table_meta.columns,
                &table_meta.primary_key,
            )?;

            // Build primary key for duplicate detection
            let mut pk_values = Vec::new();
            for pk_col in &table_meta.primary_key {
                let col_idx = column_names
                    .iter()
                    .position(|name| name == pk_col)
                    .ok_or_else(|| {
                        KimberliteError::internal(format!(
                            "Primary key column '{}' not found in INSERT columns",
                            pk_col
                        ))
                    })?;
                pk_values.push(bound_values[col_idx].clone());
            }

            let pk_key = encode_key(&pk_values);

            // Check for duplicate primary key
            let mut inner = self
                .db
                .inner()
                .write()
                .map_err(|_| KimberliteError::internal("lock poisoned"))?;

            // Convert kernel TableId to store TableId
            let store_table_id = kmb_store::TableId::from(table_id.0);

            if inner.projection_store.get(store_table_id, &pk_key)?.is_some() {
                return Err(KimberliteError::Query(
                    kmb_query::QueryError::ConstraintViolation(format!(
                        "Duplicate primary key in table '{}': {:?}",
                        insert.table, pk_values
                    )),
                ));
            }

            drop(inner);

            // Serialize row data as JSON event
            let mut row_map = serde_json::Map::new();
            for (col, val) in column_names.iter().zip(bound_values.iter()) {
                row_map.insert(col.clone(), value_to_json(val));
            }

            let event = json!({
                "type": "insert",
                "table": insert.table,
                "data": row_map,
            });

            let row_data = Bytes::from(serde_json::to_vec(&event).map_err(|e| {
                KimberliteError::internal(format!("JSON serialization failed: {}", e))
            })?);

            let cmd = Command::Insert { table_id, row_data };
            self.db.submit(cmd)?;

            rows_affected += 1;
            last_offset = self.log_position()?;

            // Track PK for RETURNING clause
            if insert.returning.is_some() {
                inserted_pk_keys.push(pk_key.clone());
            }
        }

        // If RETURNING clause present, query back the inserted rows
        if let Some(returning_cols) = &insert.returning {
            // Validate that all RETURNING columns exist
            validate_columns_exist(returning_cols, &table_meta.columns)?;

            let mut returned_rows = Vec::new();
            let mut inner = self
                .db
                .inner()
                .write()
                .map_err(|_| KimberliteError::internal("lock poisoned"))?;

            let store_table_id = kmb_store::TableId::from(table_id.0);

            for pk_key in &inserted_pk_keys {
                // Query the row from projection store
                if let Some(row_bytes) = inner.projection_store.get(store_table_id, pk_key)? {
                    // Deserialize row data
                    let row_json: serde_json::Value = serde_json::from_slice(&row_bytes)
                        .map_err(|e| KimberliteError::internal(format!("Failed to deserialize row: {}", e)))?;

                    // Extract requested columns
                    let mut row_values = Vec::new();
                    if let Some(obj) = row_json.as_object() {
                        for col in returning_cols {
                            let value = obj.get(col)
                                .ok_or_else(|| KimberliteError::internal(format!("Column '{}' not found in row", col)))?;
                            row_values.push(json_to_value(value)?);
                        }
                    } else {
                        return Err(KimberliteError::internal("Row is not a JSON object"));
                    }

                    returned_rows.push(row_values);
                }
            }

            drop(inner);

            Ok(ExecuteResult::WithReturning {
                rows_affected,
                log_offset: last_offset,
                returned: QueryResult {
                    columns: returning_cols.iter().map(|s| ColumnName::new(s.clone())).collect(),
                    rows: returned_rows,
                },
            })
        } else {
            Ok(ExecuteResult::Standard {
                rows_affected,
                log_offset: last_offset,
            })
        }
    }

    fn execute_update(&self, update: ParsedUpdate, params: &[Value]) -> Result<ExecuteResult> {
        // Look up table metadata
        let mut inner = self
            .db
            .inner()
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let (table_id, table_meta) = inner
            .kernel_state
            .tables()
            .iter()
            .find(|(_, meta)| meta.table_name == update.table)
            .map(|(id, meta)| (*id, meta.clone()))
            .ok_or_else(|| KimberliteError::TableNotFound(update.table.clone()))?;

        // Bind parameters in SET assignments
        let bound_assignments: Vec<(String, Value)> = update
            .assignments
            .into_iter()
            .map(|(col, val)| {
                let bound_val = if let Value::Placeholder(idx) = val {
                    if idx == 0 || idx > params.len() {
                        return Err(KimberliteError::Query(
                            kmb_query::QueryError::ParseError(format!(
                                "parameter ${idx} out of bounds (have {} parameters)",
                                params.len()
                            )),
                        ));
                    }
                    params[idx - 1].clone()
                } else {
                    val
                };
                Ok((col, bound_val))
            })
            .collect::<Result<Vec<_>>>()?;

        // Build a SELECT query to find all matching rows
        let select = kmb_query::ParsedSelect {
            table: update.table.clone(),
            columns: Some(table_meta.primary_key.iter().map(|c| c.clone().into()).collect()),
            predicates: update.predicates.clone(),
            order_by: vec![],
            limit: None,
            aggregates: vec![],
            group_by: vec![],
            distinct: false,
        };

        // Plan and execute the query
        let schema = inner.query_engine.schema().clone();
        let plan = kmb_query::plan_query(&schema, &select, params)?;
        let table_def = schema
            .get_table(&update.table.clone().into())
            .ok_or_else(|| KimberliteError::TableNotFound(update.table.clone()))?;
        let matching_rows = kmb_query::execute(&mut inner.projection_store, &plan, table_def)?;

        drop(inner);

        // For each matched row, submit an UPDATE command
        let mut rows_affected = 0;
        let mut last_offset = self.log_position()?;
        let mut updated_pk_keys = Vec::new();  // Track PKs for RETURNING

        for row in &matching_rows.rows {
            // Build WHERE clause with primary key values
            let mut pk_predicates = Vec::new();
            for (idx, pk_col) in table_meta.primary_key.iter().enumerate() {
                pk_predicates.push((pk_col.clone(), row[idx].clone()));
            }

            let predicates_json: Vec<serde_json::Value> = pk_predicates
                .iter()
                .map(|(col, val)| {
                    json!({
                        "op": "eq",
                        "column": col,
                        "values": [value_to_json(val)],
                    })
                })
                .collect();

            let event = json!({
                "type": "update",
                "table": update.table,
                "set": bound_assignments,
                "where": predicates_json,
            });

            let row_data = Bytes::from(serde_json::to_vec(&event).map_err(|e| {
                KimberliteError::internal(format!("JSON serialization failed: {}", e))
            })?);

            let cmd = Command::Update { table_id, row_data };
            self.db.submit(cmd)?;

            rows_affected += 1;
            last_offset = self.log_position()?;

            // Track PK for RETURNING clause
            if update.returning.is_some() {
                // Build PK key from row values
                let pk_values: Vec<Value> = table_meta
                    .primary_key
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| row[idx].clone())
                    .collect();
                updated_pk_keys.push(encode_key(&pk_values));
            }
        }

        // If RETURNING clause present, query back the updated rows
        if let Some(returning_cols) = &update.returning {
            // Validate that all RETURNING columns exist
            validate_columns_exist(returning_cols, &table_meta.columns)?;

            let mut returned_rows = Vec::new();
            let mut inner = self
                .db
                .inner()
                .write()
                .map_err(|_| KimberliteError::internal("lock poisoned"))?;

            let store_table_id = kmb_store::TableId::from(table_id.0);

            for pk_key in &updated_pk_keys {
                // Query the updated row from projection store
                if let Some(row_bytes) = inner.projection_store.get(store_table_id, pk_key)? {
                    // Deserialize row data
                    let row_json: serde_json::Value = serde_json::from_slice(&row_bytes)
                        .map_err(|e| KimberliteError::internal(format!("Failed to deserialize row: {}", e)))?;

                    // Extract requested columns
                    let mut row_values = Vec::new();
                    if let Some(obj) = row_json.as_object() {
                        for col in returning_cols {
                            let value = obj.get(col)
                                .ok_or_else(|| KimberliteError::internal(format!("Column '{}' not found in row", col)))?;
                            row_values.push(json_to_value(value)?);
                        }
                    } else {
                        return Err(KimberliteError::internal("Row is not a JSON object"));
                    }

                    returned_rows.push(row_values);
                }
            }

            drop(inner);

            Ok(ExecuteResult::WithReturning {
                rows_affected,
                log_offset: last_offset,
                returned: QueryResult {
                    columns: returning_cols.iter().map(|s| ColumnName::new(s.clone())).collect(),
                    rows: returned_rows,
                },
            })
        } else {
            Ok(ExecuteResult::Standard {
                rows_affected,
                log_offset: last_offset,
            })
        }
    }

    fn execute_delete(&self, delete: ParsedDelete, params: &[Value]) -> Result<ExecuteResult> {
        // Look up table metadata
        let mut inner = self
            .db
            .inner()
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let (table_id, table_meta) = inner
            .kernel_state
            .tables()
            .iter()
            .find(|(_, meta)| meta.table_name == delete.table)
            .map(|(id, meta)| (*id, meta.clone()))
            .ok_or_else(|| KimberliteError::TableNotFound(delete.table.clone()))?;

        // Build a SELECT query to find all matching rows
        let select = kmb_query::ParsedSelect {
            table: delete.table.clone(),
            columns: Some(table_meta.primary_key.iter().map(|c| c.clone().into()).collect()),
            predicates: delete.predicates.clone(),
            order_by: vec![],
            limit: None,
            aggregates: vec![],
            group_by: vec![],
            distinct: false,
        };

        // Plan and execute the query
        let schema = inner.query_engine.schema().clone();
        let plan = kmb_query::plan_query(&schema, &select, params)?;
        let table_def = schema
            .get_table(&delete.table.clone().into())
            .ok_or_else(|| KimberliteError::TableNotFound(delete.table.clone()))?;
        let matching_rows = kmb_query::execute(&mut inner.projection_store, &plan, table_def)?;

        drop(inner);

        // For each matched row, submit a DELETE command
        let mut rows_affected = 0;
        let mut last_offset = self.log_position()?;
        let mut deleted_rows: Vec<Vec<Value>> = Vec::new();  // Store row data for RETURNING (before deletion)

        for row in &matching_rows.rows {
            // If RETURNING, capture the full row before deletion
            if let Some(returning_cols) = &delete.returning {
                // Validate that all RETURNING columns exist
                if deleted_rows.is_empty() {
                    validate_columns_exist(returning_cols, &table_meta.columns)?;
                }

                // Build PK key and query the full row
                let pk_values: Vec<Value> = table_meta
                    .primary_key
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| row[idx].clone())
                    .collect();
                let pk_key = encode_key(&pk_values);

                let mut inner = self
                    .db
                    .inner()
                    .write()
                    .map_err(|_| KimberliteError::internal("lock poisoned"))?;

                let store_table_id = kmb_store::TableId::from(table_id.0);

                if let Some(row_bytes) = inner.projection_store.get(store_table_id, &pk_key)? {
                    // Deserialize row data
                    let row_json: serde_json::Value = serde_json::from_slice(&row_bytes)
                        .map_err(|e| KimberliteError::internal(format!("Failed to deserialize row: {}", e)))?;

                    // Extract requested columns
                    let mut row_values = Vec::new();
                    if let Some(obj) = row_json.as_object() {
                        for col in returning_cols {
                            let value = obj.get(col)
                                .ok_or_else(|| KimberliteError::internal(format!("Column '{}' not found in row", col)))?;
                            row_values.push(json_to_value(value)?);
                        }
                    } else {
                        return Err(KimberliteError::internal("Row is not a JSON object"));
                    }

                    deleted_rows.push(row_values);
                }

                drop(inner);
            }

            // Build WHERE clause with primary key values
            let mut pk_predicates = Vec::new();
            for (idx, pk_col) in table_meta.primary_key.iter().enumerate() {
                pk_predicates.push((pk_col.clone(), row[idx].clone()));
            }

            let predicates_json: Vec<serde_json::Value> = pk_predicates
                .iter()
                .map(|(col, val)| {
                    json!({
                        "op": "eq",
                        "column": col,
                        "values": [value_to_json(val)],
                    })
                })
                .collect();

            let event = json!({
                "type": "delete",
                "table": delete.table,
                "where": predicates_json,
            });

            let row_data = Bytes::from(serde_json::to_vec(&event).map_err(|e| {
                KimberliteError::internal(format!("JSON serialization failed: {}", e))
            })?);

            let cmd = Command::Delete { table_id, row_data };
            self.db.submit(cmd)?;

            rows_affected += 1;
            last_offset = self.log_position()?;
        }

        // Return result with or without RETURNING data
        if let Some(returning_cols) = delete.returning {
            Ok(ExecuteResult::WithReturning {
                rows_affected,
                log_offset: last_offset,
                returned: QueryResult {
                    columns: returning_cols.iter().map(|s| ColumnName::new(s.clone())).collect(),
                    rows: deleted_rows,
                },
            })
        } else {
            Ok(ExecuteResult::Standard {
                rows_affected,
                log_offset: last_offset,
            })
        }
    }
}

/// Validates that all specified columns exist in the table schema.
fn validate_columns_exist(
    columns: &[String],
    table_columns: &[kmb_kernel::command::ColumnDefinition],
) -> Result<()> {
    for col_name in columns {
        if !table_columns.iter().any(|c| &c.name == col_name) {
            return Err(KimberliteError::Query(kmb_query::QueryError::ParseError(
                format!("column '{}' does not exist in table", col_name),
            )));
        }
    }
    Ok(())
}

/// Validates that values match their column types and constraints.
fn validate_insert_values(
    column_names: &[String],
    values: &[Value],
    table_columns: &[kmb_kernel::command::ColumnDefinition],
    primary_key_cols: &[String],
) -> Result<()> {
    for (col_name, value) in column_names.iter().zip(values.iter()) {
        // Find the column definition
        let col_def = table_columns
            .iter()
            .find(|c| &c.name == col_name)
            .ok_or_else(|| {
                KimberliteError::Query(kmb_query::QueryError::ParseError(format!(
                    "column '{}' not found in table schema",
                    col_name
                )))
            })?;

        // Check NOT NULL constraint
        if !col_def.nullable && value.is_null() {
            return Err(KimberliteError::Query(kmb_query::QueryError::TypeMismatch {
                expected: format!("non-NULL value for column '{}'", col_name),
                actual: "NULL".to_string(),
            }));
        }

        // Check primary key NULL constraint
        if primary_key_cols.contains(col_name) && value.is_null() {
            return Err(KimberliteError::Query(kmb_query::QueryError::TypeMismatch {
                expected: format!("non-NULL value for primary key column '{}'", col_name),
                actual: "NULL".to_string(),
            }));
        }

        // Type validation (basic check - NULL is compatible with any type)
        if !value.is_null() {
            let expected_type = match col_def.data_type.as_str() {
                "BIGINT" => Some(kmb_query::DataType::BigInt),
                "TEXT" => Some(kmb_query::DataType::Text),
                "BOOLEAN" => Some(kmb_query::DataType::Boolean),
                "TIMESTAMP" => Some(kmb_query::DataType::Timestamp),
                "BYTES" => Some(kmb_query::DataType::Bytes),
                _ => None,
            };

            if let Some(expected) = expected_type {
                if !value.is_compatible_with(expected) {
                    return Err(KimberliteError::Query(
                        kmb_query::QueryError::TypeMismatch {
                            expected: format!("{:?} for column '{}'", expected, col_name),
                            actual: format!("{:?}", value.data_type()),
                        },
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Converts a predicate to a JSON-serializable format.
fn predicate_to_json(pred: &kmb_query::Predicate, params: &[Value]) -> Result<serde_json::Value> {
    use kmb_query::{Predicate, PredicateValue};

    let (op, col, values) = match pred {
        Predicate::Eq(col, val) => ("eq", col.as_str(), vec![val]),
        Predicate::Lt(col, val) => ("lt", col.as_str(), vec![val]),
        Predicate::Le(col, val) => ("le", col.as_str(), vec![val]),
        Predicate::Gt(col, val) => ("gt", col.as_str(), vec![val]),
        Predicate::Ge(col, val) => ("ge", col.as_str(), vec![val]),
        Predicate::In(col, vals) => ("in", col.as_str(), vals.iter().collect()),
        Predicate::Like(col, pattern) => {
            // Convert LIKE pattern to PredicateValue::String for processing
            return Ok(serde_json::json!({
                "op": "like",
                "column": col.as_str(),
                "pattern": pattern,
            }));
        }
        Predicate::IsNull(col) => {
            return Ok(serde_json::json!({
                "op": "is_null",
                "column": col.as_str(),
            }));
        }
        Predicate::IsNotNull(col) => {
            return Ok(serde_json::json!({
                "op": "is_not_null",
                "column": col.as_str(),
            }));
        }
        Predicate::Or(left_preds, right_preds) => {
            // Recursively convert OR predicates
            let left_json: Result<Vec<serde_json::Value>> = left_preds
                .iter()
                .map(|p| predicate_to_json(p, params))
                .collect();
            let right_json: Result<Vec<serde_json::Value>> = right_preds
                .iter()
                .map(|p| predicate_to_json(p, params))
                .collect();

            return Ok(serde_json::json!({
                "op": "or",
                "left": left_json?,
                "right": right_json?,
            }));
        }
    };

    // Convert predicate values to actual values (binding parameters)
    let bound_values: Result<Vec<serde_json::Value>> = values
        .into_iter()
        .map(|pv| {
            let val = match pv {
                PredicateValue::Int(n) => Value::BigInt(*n),
                PredicateValue::String(s) => Value::Text(s.clone()),
                PredicateValue::Bool(b) => Value::Boolean(*b),
                PredicateValue::Null => Value::Null,
                PredicateValue::Literal(v) => v.clone(),
                PredicateValue::Param(idx) => {
                    if *idx == 0 || *idx > params.len() {
                        return Err(KimberliteError::Query(
                            kmb_query::QueryError::ParseError(format!(
                                "parameter ${idx} out of bounds (have {} parameters)",
                                params.len()
                            )),
                        ));
                    }
                    params[idx - 1].clone()
                }
            };
            Ok(value_to_json(&val))
        })
        .collect();

    Ok(json!({
        "op": op,
        "column": col,
        "values": bound_values?,
    }))
}

/// Binds parameters to placeholders in a list of values.
///
/// Replaces all `Value::Placeholder(idx)` with the corresponding value from `params`.
/// Parameter indices are 1-indexed (i.e., $1, $2, ...).
///
/// # Errors
///
/// Returns an error if:
/// - A placeholder index is out of bounds
/// - Parameter count doesn't match placeholder count
fn bind_parameters(values: &[Value], params: &[Value]) -> Result<Vec<Value>> {
    let mut bound = Vec::with_capacity(values.len());

    for value in values {
        match value {
            Value::Placeholder(idx) => {
                // Parameter indices are 1-indexed ($1, $2, ...)
                if *idx == 0 || *idx > params.len() {
                    return Err(KimberliteError::Query(
                        kmb_query::QueryError::ParseError(format!(
                            "parameter ${idx} out of bounds (have {} parameters)",
                            params.len()
                        )),
                    ));
                }
                // idx is 1-indexed, so subtract 1 for array access
                bound.push(params[idx - 1].clone());
            }
            other => bound.push(other.clone()),
        }
    }

    Ok(bound)
}

/// Converts a `Value` to a `serde_json::Value`.
///
/// # Panics
///
/// Panics if the value is a `Placeholder` (should be bound before conversion).
fn value_to_json(val: &Value) -> serde_json::Value {
    // Use the Value::to_json() method which handles all types
    val.to_json()
}

/// Converts a JSON value to a kmb_query::Value.
/// Makes reasonable assumptions about types (e.g., numbers become BigInt).
fn json_to_value(json: &serde_json::Value) -> Result<Value> {
    match json {
        serde_json::Value::Null => Ok(Value::Null),
        serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            // Try i64 first (BigInt), then f64 (Real)
            if let Some(i) = n.as_i64() {
                Ok(Value::BigInt(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Real(f))
            } else {
                Err(KimberliteError::internal(format!("Unsupported number type: {}", n)))
            }
        }
        serde_json::Value::String(s) => {
            // Could be Text, Bytes (base64), UUID, or Decimal
            // For simplicity, assume Text
            Ok(Value::Text(s.clone()))
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            // Assume JSON type
            Ok(Value::Json(json.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_tenant_create_stream() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        let stream_id = tenant.create_stream("test", DataClass::NonPHI).unwrap();
        let stream_id_val: u64 = stream_id.into();
        assert!(stream_id_val > 0);
    }

    #[test]
    fn test_tenant_append_and_read() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        let stream_id = tenant.create_stream("events", DataClass::NonPHI).unwrap();

        // Append events
        tenant
            .append(stream_id, vec![b"event1".to_vec(), b"event2".to_vec()])
            .unwrap();

        // Read back
        let events = tenant
            .read_events(stream_id, Offset::ZERO, 1024 * 1024_u64)
            .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(&events[0][..], b"event1");
        assert_eq!(&events[1][..], b"event2");
    }

    #[test]
    fn test_tenant_create_table_via_sql() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Execute CREATE TABLE - should succeed without error
        let result = tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // DDL doesn't affect rows
        assert_eq!(result.rows_affected(), 0);
    }

    #[test]
    fn test_tenant_insert_via_sql() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table first
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert row
        let result = tenant
            .execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", &[])
            .unwrap();

        assert_eq!(result.rows_affected(), 1);
        assert!(result.log_offset().as_u64() > 0);
    }

    #[test]
    fn test_tenant_execute_rejects_select() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // SELECT should be rejected by execute()
        let result = tenant.execute("SELECT * FROM users", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_tenant_insert_and_query() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert rows
        tenant
            .execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", &[])
            .unwrap();

        tenant
            .execute("INSERT INTO users (id, name) VALUES (2, 'Bob')", &[])
            .unwrap();

        // Query the data back
        let result = tenant.query("SELECT * FROM users ORDER BY id", &[]);

        match result {
            Ok(qr) => {
                // Verify we got the rows back
                assert_eq!(qr.rows.len(), 2, "should have 2 rows");

                // Check column names
                assert_eq!(qr.columns.len(), 2);
                assert_eq!(qr.columns[0].as_str(), "id");
                assert_eq!(qr.columns[1].as_str(), "name");

                // Check first row
                assert_eq!(qr.rows[0][0], Value::BigInt(1));
                assert_eq!(qr.rows[0][1], Value::Text("Alice".to_string()));

                // Check second row
                assert_eq!(qr.rows[1][0], Value::BigInt(2));
                assert_eq!(qr.rows[1][1], Value::Text("Bob".to_string()));
            }
            Err(e) => {
                panic!("Query failed: {}", e);
            }
        }
    }

    #[test]
    fn test_query_with_where_clause() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert rows
        tenant
            .execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", &[])
            .unwrap();

        tenant
            .execute("INSERT INTO users (id, name) VALUES (2, 'Bob')", &[])
            .unwrap();

        // Query with WHERE clause
        let result = tenant
            .query("SELECT name FROM users WHERE id = 2", &[])
            .unwrap();

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::Text("Bob".to_string()));
    }

    #[test]
    fn test_query_select_specific_columns() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table with multiple columns
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, age BIGINT, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert row
        tenant
            .execute(
                "INSERT INTO users (id, name, age) VALUES (1, 'Alice', 30)",
                &[],
            )
            .unwrap();

        // Query specific columns
        let result = tenant
            .query("SELECT name, age FROM users WHERE id = 1", &[])
            .unwrap();

        assert_eq!(result.columns.len(), 2);
        assert_eq!(result.columns[0].as_str(), "name");
        assert_eq!(result.columns[1].as_str(), "age");

        assert_eq!(result.rows[0][0], Value::Text("Alice".to_string()));
        assert_eq!(result.rows[0][1], Value::BigInt(30));
    }

    #[test]
    fn test_multiple_table_operations() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create first table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Create second table
        tenant
            .execute(
                "CREATE TABLE orders (order_id BIGINT NOT NULL, user_id BIGINT NOT NULL, amount BIGINT, PRIMARY KEY (order_id))",
                &[],
            )
            .unwrap();

        // Insert into both tables
        tenant
            .execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", &[])
            .unwrap();

        tenant
            .execute(
                "INSERT INTO orders (order_id, user_id, amount) VALUES (100, 1, 5000)",
                &[],
            )
            .unwrap();

        // Verify both tables exist
        // (indirectly verified by successful inserts)
    }

    #[test]
    fn test_create_and_drop_table() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE temp (id BIGINT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Drop table
        let result = tenant.execute("DROP TABLE temp", &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_duplicate_table_creation_fails() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Try to create same table again
        let result = tenant.execute(
            "CREATE TABLE users (id BIGINT NOT NULL, PRIMARY KEY (id))",
            &[],
        );

        // Should fail
        assert!(result.is_err());
    }

    #[test]
    fn test_create_index() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Create index
        let result = tenant.execute("CREATE INDEX idx_name ON users (name)", &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_operation() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert
        tenant
            .execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", &[])
            .unwrap();

        // Update
        let result = tenant.execute("UPDATE users SET name = 'Alice Updated' WHERE id = 1", &[]);
        if let Err(ref e) = result {
            eprintln!("UPDATE failed: {:?}", e);
        }
        assert!(result.is_ok());
        assert_eq!(result.unwrap().rows_affected(), 1);
    }

    #[test]
    fn test_delete_operation() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert
        tenant
            .execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", &[])
            .unwrap();

        // Delete
        let result = tenant.execute("DELETE FROM users WHERE id = 1", &[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().rows_affected(), 1);
    }

    #[test]
    fn test_composite_primary_key() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table with composite primary key
        tenant
            .execute(
                "CREATE TABLE orders (
                    user_id BIGINT NOT NULL,
                    order_id BIGINT NOT NULL,
                    amount BIGINT,
                    PRIMARY KEY (user_id, order_id)
                )",
                &[],
            )
            .unwrap();

        // Insert with composite key
        let result = tenant.execute(
            "INSERT INTO orders (user_id, order_id, amount) VALUES (1, 100, 5000)",
            &[],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_table_requires_primary_key() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Try to create table without PRIMARY KEY
        let result = tenant.execute(
            "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL)",
            &[],
        );

        // Should fail
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("must have a PRIMARY KEY"),
            "Expected PRIMARY KEY error, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_insert_without_column_names() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE patients (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // INSERT without specifying column names
        tenant
            .execute("INSERT INTO patients VALUES (1, 'Jane Doe')", &[])
            .unwrap();

        // Query the data back
        let result = tenant.query("SELECT * FROM patients", &[]).unwrap();

        // Verify we got the row back with correct values
        assert_eq!(result.rows.len(), 1, "should have 1 row");
        assert_eq!(result.rows[0][0], Value::BigInt(1));
        assert_eq!(result.rows[0][1], Value::Text("Jane Doe".to_string()));
    }

    // ========================================================================
    // Comprehensive Tests for SQL Engine Fixes
    // ========================================================================

    #[test]
    fn test_parameterized_insert() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // INSERT with parameters
        tenant
            .execute(
                "INSERT INTO users (id, name) VALUES ($1, $2)",
                &[Value::BigInt(42), Value::Text("Alice".to_string())],
            )
            .unwrap();

        // Query back
        let result = tenant.query("SELECT * FROM users WHERE id = 42", &[]).unwrap();

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::BigInt(42));
        assert_eq!(result.rows[0][1], Value::Text("Alice".to_string()));
        // Verify no NULL values
        assert!(!result.rows[0].iter().any(|v| v.is_null()));
    }

    #[test]
    fn test_update_then_select_roundtrip() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert
        tenant
            .execute("INSERT INTO users VALUES (1, 'Alice')", &[])
            .unwrap();

        // Update
        tenant
            .execute("UPDATE users SET name = 'Bob' WHERE id = 1", &[])
            .unwrap();

        // Query back - should return updated value
        let result = tenant.query("SELECT name FROM users WHERE id = 1", &[]).unwrap();

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::Text("Bob".to_string()));
    }

    #[test]
    fn test_delete_then_select_empty() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert
        tenant
            .execute("INSERT INTO users VALUES (1, 'Alice')", &[])
            .unwrap();

        // Delete
        tenant
            .execute("DELETE FROM users WHERE id = 1", &[])
            .unwrap();

        // Query back - should be empty
        let result = tenant.query("SELECT * FROM users WHERE id = 1", &[]).unwrap();

        assert_eq!(result.rows.len(), 0);
    }

    #[test]
    fn test_null_in_not_null_column_rejected() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table with NOT NULL constraint
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Try to insert NULL into NOT NULL column
        let result = tenant.execute(
            "INSERT INTO users (id, name) VALUES ($1, $2)",
            &[Value::BigInt(1), Value::Null],
        );

        // Should fail
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("NULL") || err_msg.contains("non-NULL"));
    }

    #[test]
    fn test_null_in_primary_key_rejected() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Try to insert NULL into primary key
        let result = tenant.execute(
            "INSERT INTO users (id, name) VALUES ($1, $2)",
            &[Value::Null, Value::Text("Alice".to_string())],
        );

        // Should fail
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_column_name_rejected() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Try to insert into non-existent column
        let result = tenant.execute(
            "INSERT INTO users (id, invalid_column) VALUES (1, 'Alice')",
            &[],
        );

        // Should fail
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("invalid_column") || err_msg.contains("does not exist"));
    }

    #[test]
    fn test_composite_primary_key_update_delete() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table with composite primary key
        tenant
            .execute(
                "CREATE TABLE orders (
                    user_id BIGINT NOT NULL,
                    order_id BIGINT NOT NULL,
                    amount BIGINT,
                    PRIMARY KEY (user_id, order_id)
                )",
                &[],
            )
            .unwrap();

        // Insert
        tenant
            .execute(
                "INSERT INTO orders (user_id, order_id, amount) VALUES (1, 100, 5000)",
                &[],
            )
            .unwrap();

        // Update with composite key WHERE clause
        tenant
            .execute(
                "UPDATE orders SET amount = 6000 WHERE user_id = 1 AND order_id = 100",
                &[],
            )
            .unwrap();

        // Verify update
        let result = tenant
            .query(
                "SELECT amount FROM orders WHERE user_id = 1 AND order_id = 100",
                &[],
            )
            .unwrap();

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::BigInt(6000));

        // Delete with composite key
        tenant
            .execute("DELETE FROM orders WHERE user_id = 1 AND order_id = 100", &[])
            .unwrap();

        // Verify deletion
        let result = tenant
            .query(
                "SELECT * FROM orders WHERE user_id = 1 AND order_id = 100",
                &[],
            )
            .unwrap();

        assert_eq!(result.rows.len(), 0);
    }

    #[test]
    fn test_tenant_multi_row_insert() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, age BIGINT, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert 3 rows in a single statement
        let result = tenant
            .execute(
                "INSERT INTO users (id, name, age) VALUES (1, 'Alice', 25), (2, 'Bob', 30), (3, 'Charlie', 35)",
                &[],
            )
            .unwrap();

        assert_eq!(result.rows_affected(), 3, "Should insert 3 rows");

        // Verify all rows were inserted
        let query_result = tenant
            .query("SELECT id, name, age FROM users ORDER BY id", &[])
            .unwrap();

        assert_eq!(query_result.rows.len(), 3);
        assert_eq!(query_result.rows[0][0], Value::BigInt(1));
        assert_eq!(query_result.rows[0][1], Value::Text("Alice".to_string()));
        assert_eq!(query_result.rows[0][2], Value::BigInt(25));

        assert_eq!(query_result.rows[1][0], Value::BigInt(2));
        assert_eq!(query_result.rows[1][1], Value::Text("Bob".to_string()));
        assert_eq!(query_result.rows[1][2], Value::BigInt(30));

        assert_eq!(query_result.rows[2][0], Value::BigInt(3));
        assert_eq!(query_result.rows[2][1], Value::Text("Charlie".to_string()));
        assert_eq!(query_result.rows[2][2], Value::BigInt(35));
    }

    #[test]
    fn test_tenant_multi_row_insert_100_rows() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE numbers (id BIGINT NOT NULL, value BIGINT, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Build INSERT with 100 rows
        let mut values = Vec::new();
        for i in 1..=100 {
            values.push(format!("({}, {})", i, i * 10));
        }
        let sql = format!("INSERT INTO numbers (id, value) VALUES {}", values.join(", "));

        let result = tenant.execute(&sql, &[]).unwrap();

        assert_eq!(result.rows_affected(), 100, "Should insert 100 rows");

        // Verify count
        let query_result = tenant
            .query("SELECT COUNT(*) FROM numbers", &[])
            .unwrap();

        assert_eq!(query_result.rows.len(), 1);
        assert_eq!(query_result.rows[0][0], Value::BigInt(100));
    }

    #[test]
    fn test_duplicate_key_detection() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert first row - should succeed
        tenant
            .execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", &[])
            .unwrap();

        // Insert same id again - should fail with ConstraintViolation
        let result = tenant.execute("INSERT INTO users (id, name) VALUES (1, 'Bob')", &[]);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, KimberliteError::Query(kmb_query::QueryError::ConstraintViolation(_))),
            "Expected ConstraintViolation, got {:?}",
            err
        );
    }

    #[test]
    fn test_duplicate_key_detection_in_batch() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert first row
        tenant
            .execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", &[])
            .unwrap();

        // Try batch insert with duplicate - should fail on the duplicate
        let result = tenant.execute(
            "INSERT INTO users (id, name) VALUES (2, 'Bob'), (1, 'Duplicate'), (3, 'Charlie')",
            &[],
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, KimberliteError::Query(kmb_query::QueryError::ConstraintViolation(_))),
            "Expected ConstraintViolation, got {:?}",
            err
        );

        // Verify only the first new row (Bob) was inserted before the error
        let query_result = tenant
            .query("SELECT COUNT(*) FROM users", &[])
            .unwrap();

        // Should have Alice (original) + Bob (first in batch before duplicate)
        assert_eq!(query_result.rows[0][0], Value::BigInt(2));
    }

    #[test]
    fn test_multi_row_update_accurate_count() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, status TEXT NOT NULL, age BIGINT, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert multiple rows
        tenant
            .execute(
                "INSERT INTO users (id, status, age) VALUES (1, 'inactive', 25), (2, 'inactive', 30), (3, 'active', 35), (4, 'inactive', 40)",
                &[],
            )
            .unwrap();

        // Update multiple rows - should affect 3 rows with status='inactive'
        let result = tenant
            .execute("UPDATE users SET status = 'active' WHERE status = 'inactive'", &[])
            .unwrap();

        assert_eq!(result.rows_affected(), 3, "Should update 3 inactive users");

        // Verify all rows are now active
        let query_result = tenant
            .query("SELECT COUNT(*) FROM users WHERE status = 'active'", &[])
            .unwrap();

        assert_eq!(query_result.rows[0][0], Value::BigInt(4));
    }

    #[test]
    fn test_multi_row_delete_accurate_count() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, age BIGINT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert multiple rows
        tenant
            .execute(
                "INSERT INTO users (id, age) VALUES (1, 20), (2, 25), (3, 30), (4, 35), (5, 40)",
                &[],
            )
            .unwrap();

        // Delete rows where age >= 30 - should affect 3 rows
        let result = tenant
            .execute("DELETE FROM users WHERE age >= 30", &[])
            .unwrap();

        assert_eq!(result.rows_affected(), 3, "Should delete 3 users with age >= 30");

        // Verify only 2 rows remain
        let query_result = tenant
            .query("SELECT COUNT(*) FROM users", &[])
            .unwrap();

        assert_eq!(query_result.rows[0][0], Value::BigInt(2));

        // Verify remaining rows have age < 30
        let query_result = tenant
            .query("SELECT id FROM users ORDER BY id", &[])
            .unwrap();

        assert_eq!(query_result.rows.len(), 2);
        assert_eq!(query_result.rows[0][0], Value::BigInt(1));
        assert_eq!(query_result.rows[1][0], Value::BigInt(2));
    }

    #[test]
    fn test_insert_returning_single_row() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert with RETURNING
        let result = tenant
            .execute("INSERT INTO users (id, name) VALUES (1, 'Alice') RETURNING id, name", &[])
            .unwrap();

        // Verify result type
        assert_eq!(result.rows_affected(), 1);
        assert!(result.returned().is_some());

        let returned = result.returned().unwrap();
        assert_eq!(returned.columns.len(), 2);
        assert_eq!(returned.rows.len(), 1);
        assert_eq!(returned.rows[0][0], Value::BigInt(1));
        assert_eq!(returned.rows[0][1], Value::Text("Alice".to_string()));
    }

    #[test]
    fn test_insert_returning_multiple_rows() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Multi-row insert with RETURNING
        let result = tenant
            .execute(
                "INSERT INTO users (id, name) VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Charlie') RETURNING id, name",
                &[],
            )
            .unwrap();

        assert_eq!(result.rows_affected(), 3);
        let returned = result.returned().unwrap();
        assert_eq!(returned.rows.len(), 3);

        // Verify each returned row
        assert_eq!(returned.rows[0][0], Value::BigInt(1));
        assert_eq!(returned.rows[0][1], Value::Text("Alice".to_string()));
        assert_eq!(returned.rows[1][0], Value::BigInt(2));
        assert_eq!(returned.rows[1][1], Value::Text("Bob".to_string()));
        assert_eq!(returned.rows[2][0], Value::BigInt(3));
        assert_eq!(returned.rows[2][1], Value::Text("Charlie".to_string()));
    }

    #[test]
    fn test_update_returning() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table and insert data
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, age BIGINT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        tenant
            .execute(
                "INSERT INTO users (id, name, age) VALUES (1, 'Alice', 25), (2, 'Bob', 30)",
                &[],
            )
            .unwrap();

        // Update with RETURNING
        let result = tenant
            .execute("UPDATE users SET age = 26 WHERE id = 1 RETURNING id, name, age", &[])
            .unwrap();

        assert_eq!(result.rows_affected(), 1);
        let returned = result.returned().unwrap();
        assert_eq!(returned.rows.len(), 1);

        // Verify returned row has updated value
        assert_eq!(returned.rows[0][0], Value::BigInt(1));
        assert_eq!(returned.rows[0][1], Value::Text("Alice".to_string()));
        assert_eq!(returned.rows[0][2], Value::BigInt(26));  // Updated age
    }

    #[test]
    fn test_delete_returning() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table and insert data
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        tenant
            .execute(
                "INSERT INTO users (id, name) VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Charlie')",
                &[],
            )
            .unwrap();

        // Delete with RETURNING
        let result = tenant
            .execute("DELETE FROM users WHERE id IN (1, 2) RETURNING id, name", &[])
            .unwrap();

        assert_eq!(result.rows_affected(), 2);
        let returned = result.returned().unwrap();
        assert_eq!(returned.rows.len(), 2);

        // Verify returned deleted rows
        assert_eq!(returned.rows[0][0], Value::BigInt(1));
        assert_eq!(returned.rows[0][1], Value::Text("Alice".to_string()));
        assert_eq!(returned.rows[1][0], Value::BigInt(2));
        assert_eq!(returned.rows[1][1], Value::Text("Bob".to_string()));

        // Verify rows are actually deleted
        let query_result = tenant.query("SELECT id FROM users ORDER BY id", &[]).unwrap();
        assert_eq!(query_result.rows.len(), 1);  // Only Charlie remains
        assert_eq!(query_result.rows[0][0], Value::BigInt(3));
    }

    #[test]
    fn test_returning_partial_columns() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create table with multiple columns
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, age BIGINT NOT NULL, email TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert with RETURNING only some columns
        let result = tenant
            .execute(
                "INSERT INTO users (id, name, age, email) VALUES (1, 'Alice', 25, 'alice@example.com') RETURNING id, email",
                &[],
            )
            .unwrap();

        let returned = result.returned().unwrap();
        assert_eq!(returned.columns.len(), 2);
        assert_eq!(returned.rows[0].len(), 2);
        assert_eq!(returned.rows[0][0], Value::BigInt(1));
        assert_eq!(returned.rows[0][1], Value::Text("alice@example.com".to_string()));
    }
}
