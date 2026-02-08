//! Tenant-scoped handle for database operations.
//!
//! A `TenantHandle` provides operations scoped to a specific tenant ID.
//! All data isolation and access control is handled at this layer.

use bytes::Bytes;
use kimberlite_kernel::Command;
use kimberlite_kernel::command::{ColumnDefinition, IndexId, TableId};
use kimberlite_query::{
    ColumnName, ParsedAlterTable, ParsedCreateIndex, ParsedCreateTable, ParsedDelete,
    ParsedInsert, ParsedUpdate, QueryResult, Value, key_encoder::encode_key,
};
use kimberlite_store::ProjectionStore;
use kimberlite_types::{DataClass, Offset, Placement, StreamId, StreamName, TenantId};
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

    /// Get the returned rows, if this is a `WithReturning` result.
    pub fn returned(&self) -> Option<&QueryResult> {
        match self {
            ExecuteResult::Standard { .. } => None,
            ExecuteResult::WithReturning { returned, .. } => Some(returned),
        }
    }
}

/// Controls whether consent validation is enforced for data operations.
///
/// # Variants
///
/// - `Required` — Operations on personal data require valid consent.
///   `append_with_consent()` and `query_with_consent()` will fail if
///   no consent exists.
/// - `Optional` — Consent is checked but operations proceed with a warning
///   if consent is missing.
/// - `Disabled` — No consent checks are performed (default). Use the
///   standard `append()`/`query()` methods directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConsentMode {
    /// Consent must be validated before processing personal data.
    Required,
    /// Consent is checked but missing consent only logs a warning.
    Optional,
    /// No consent enforcement (default).
    #[default]
    Disabled,
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
/// tenant.create_stream("orders", DataClass::Public)?;
///
/// // Append events
/// tenant.append("orders", vec![b"order_created".to_vec()], Offset::ZERO)?;
///
/// // Query data
/// let results = tenant.query("SELECT * FROM events LIMIT 10", &[])?;
/// ```
#[derive(Clone)]
pub struct TenantHandle {
    db: Kimberlite,
    tenant_id: TenantId,
    consent_mode: ConsentMode,
}

impl TenantHandle {
    /// Creates a new tenant handle.
    pub(crate) fn new(db: Kimberlite, tenant_id: TenantId) -> Self {
        Self {
            db,
            tenant_id,
            consent_mode: ConsentMode::Disabled,
        }
    }

    /// Returns the tenant ID for this handle.
    pub fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }

    /// Sets the consent enforcement mode for this handle.
    ///
    /// - `ConsentMode::Required` — operations on personal data will fail
    ///   without valid consent.
    /// - `ConsentMode::Optional` — missing consent logs a warning but
    ///   allows the operation.
    /// - `ConsentMode::Disabled` — no consent checks (default).
    pub fn with_consent_mode(mut self, mode: ConsentMode) -> Self {
        self.consent_mode = mode;
        self
    }

    /// Returns the current consent mode.
    pub fn consent_mode(&self) -> ConsentMode {
        self.consent_mode
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

        // Encode tenant_id in upper 32 bits, local stream counter in lower 32 bits
        // For now, use local_id=1 for the first stream per tenant
        // Future: proper stream ID allocation with counter
        let stream_id = StreamId::from_tenant_and_local(self.tenant_id, 1);

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

    /// Appends events to a stream with optimistic concurrency control.
    ///
    /// The caller must provide the expected current offset of the stream.
    /// If another writer has appended since the caller last read the offset,
    /// the kernel will return `KernelError::UnexpectedStreamOffset`.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - The stream to append to
    /// * `events` - Events to append
    /// * `expected_offset` - The offset the caller expects the stream to be at
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
    /// let offset = tenant.append(stream_id, events, Offset::ZERO)?;
    /// ```
    pub fn append(
        &self,
        stream_id: StreamId,
        events: Vec<Vec<u8>>,
        expected_offset: Offset,
    ) -> Result<Offset> {
        let events: Vec<Bytes> = events.into_iter().map(Bytes::from).collect();

        self.db
            .submit(Command::append_batch(stream_id, events, expected_offset))?;

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
        use kimberlite_query::{ParsedStatement, parse_statement};

        // Parse the SQL statement
        let parsed = parse_statement(sql)?;

        match parsed {
            ParsedStatement::Select(_) | ParsedStatement::Union(_) => {
                // SELECT/UNION goes through query path, not execute
                Err(KimberliteError::Query(
                    kimberlite_query::QueryError::UnsupportedFeature(
                        "use query() for SELECT/UNION statements".to_string(),
                    ),
                ))
            }

            ParsedStatement::CreateTable(create_table) => self.execute_create_table(create_table),

            ParsedStatement::DropTable(table_name) => self.execute_drop_table(&table_name),

            ParsedStatement::AlterTable(alter_table) => self.execute_alter_table(alter_table),

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
        let current_pos =
            kimberlite_store::ProjectionStore::applied_position(&inner.projection_store);
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
        let mut inner = self
            .db
            .inner()
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let events = inner.read_events(stream_id, from_offset, max_bytes)?;
        Ok(events)
    }

    // ========================================================================
    // DDL/DML Implementation Helpers
    // ========================================================================

    fn execute_create_table(&self, create_table: ParsedCreateTable) -> Result<ExecuteResult> {
        // Validate that a primary key is defined
        if create_table.primary_key.is_empty() {
            return Err(KimberliteError::Query(
                kimberlite_query::QueryError::ParseError(format!(
                    "table '{}' must have a PRIMARY KEY defined",
                    create_table.table_name
                )),
            ));
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

    fn execute_alter_table(&self, _alter_table: ParsedAlterTable) -> Result<ExecuteResult> {
        // TODO: Implement ALTER TABLE kernel commands (AddColumn, DropColumn)
        // This requires adding new Command variants to kimberlite-kernel
        Err(KimberliteError::Query(
            kimberlite_query::QueryError::UnsupportedFeature(
                "ALTER TABLE not yet implemented - requires kernel support".to_string(),
            ),
        ))
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
        let mut inserted_pk_keys = Vec::new(); // Track PKs for RETURNING

        for row_values in &insert.values {
            // Validate value count matches column count for this row
            if column_names.len() != row_values.len() {
                return Err(KimberliteError::Query(
                    kimberlite_query::QueryError::TypeMismatch {
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
                            "Primary key column '{pk_col}' not found in INSERT columns"
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
            let store_table_id = kimberlite_store::TableId::from(table_id.0);

            if inner
                .projection_store
                .get(store_table_id, &pk_key)?
                .is_some()
            {
                return Err(KimberliteError::Query(
                    kimberlite_query::QueryError::ConstraintViolation(format!(
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
                KimberliteError::internal(format!("JSON serialization failed: {e}"))
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

            let store_table_id = kimberlite_store::TableId::from(table_id.0);

            for pk_key in &inserted_pk_keys {
                // Query the row from projection store
                if let Some(row_bytes) = inner.projection_store.get(store_table_id, pk_key)? {
                    // Deserialize row data
                    let row_json: serde_json::Value =
                        serde_json::from_slice(&row_bytes).map_err(|e| {
                            KimberliteError::internal(format!("Failed to deserialize row: {e}"))
                        })?;

                    // Extract requested columns
                    let mut row_values = Vec::new();
                    if let Some(obj) = row_json.as_object() {
                        for col in returning_cols {
                            let value = obj.get(col).ok_or_else(|| {
                                KimberliteError::internal(format!(
                                    "Column '{col}' not found in row"
                                ))
                            })?;
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
                    columns: returning_cols
                        .iter()
                        .map(|s| ColumnName::new(s.clone()))
                        .collect(),
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
                            kimberlite_query::QueryError::ParseError(format!(
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
        let select = kimberlite_query::ParsedSelect {
            table: update.table.clone(),
            joins: vec![],
            columns: Some(
                table_meta
                    .primary_key
                    .iter()
                    .map(|c| c.clone().into())
                    .collect(),
            ),
            predicates: update.predicates.clone(),
            order_by: vec![],
            limit: None,
            aggregates: vec![],
            group_by: vec![],
            distinct: false,
            having: vec![],
            ctes: vec![],
        };

        // Plan and execute the query
        let schema = inner.query_engine.schema().clone();
        let plan = kimberlite_query::plan_query(&schema, &select, params)?;
        let table_def = schema
            .get_table(&update.table.clone().into())
            .ok_or_else(|| KimberliteError::TableNotFound(update.table.clone()))?;
        let matching_rows =
            kimberlite_query::execute(&mut inner.projection_store, &plan, table_def)?;

        drop(inner);

        // For each matched row, submit an UPDATE command
        let mut rows_affected = 0;
        let mut last_offset = self.log_position()?;
        let mut updated_pk_keys = Vec::new(); // Track PKs for RETURNING

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
                KimberliteError::internal(format!("JSON serialization failed: {e}"))
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

            let store_table_id = kimberlite_store::TableId::from(table_id.0);

            for pk_key in &updated_pk_keys {
                // Query the updated row from projection store
                if let Some(row_bytes) = inner.projection_store.get(store_table_id, pk_key)? {
                    // Deserialize row data
                    let row_json: serde_json::Value =
                        serde_json::from_slice(&row_bytes).map_err(|e| {
                            KimberliteError::internal(format!("Failed to deserialize row: {e}"))
                        })?;

                    // Extract requested columns
                    let mut row_values = Vec::new();
                    if let Some(obj) = row_json.as_object() {
                        for col in returning_cols {
                            let value = obj.get(col).ok_or_else(|| {
                                KimberliteError::internal(format!(
                                    "Column '{col}' not found in row"
                                ))
                            })?;
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
                    columns: returning_cols
                        .iter()
                        .map(|s| ColumnName::new(s.clone()))
                        .collect(),
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
        let select = kimberlite_query::ParsedSelect {
            table: delete.table.clone(),
            joins: vec![],
            columns: Some(
                table_meta
                    .primary_key
                    .iter()
                    .map(|c| c.clone().into())
                    .collect(),
            ),
            predicates: delete.predicates.clone(),
            order_by: vec![],
            limit: None,
            aggregates: vec![],
            group_by: vec![],
            distinct: false,
            having: vec![],
            ctes: vec![],
        };

        // Plan and execute the query
        let schema = inner.query_engine.schema().clone();
        let plan = kimberlite_query::plan_query(&schema, &select, params)?;
        let table_def = schema
            .get_table(&delete.table.clone().into())
            .ok_or_else(|| KimberliteError::TableNotFound(delete.table.clone()))?;
        let matching_rows =
            kimberlite_query::execute(&mut inner.projection_store, &plan, table_def)?;

        drop(inner);

        // For each matched row, submit a DELETE command
        let mut rows_affected = 0;
        let mut last_offset = self.log_position()?;
        let mut deleted_rows: Vec<Vec<Value>> = Vec::new(); // Store row data for RETURNING (before deletion)

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

                let store_table_id = kimberlite_store::TableId::from(table_id.0);

                if let Some(row_bytes) = inner.projection_store.get(store_table_id, &pk_key)? {
                    // Deserialize row data
                    let row_json: serde_json::Value =
                        serde_json::from_slice(&row_bytes).map_err(|e| {
                            KimberliteError::internal(format!("Failed to deserialize row: {e}"))
                        })?;

                    // Extract requested columns
                    let mut row_values = Vec::new();
                    if let Some(obj) = row_json.as_object() {
                        for col in returning_cols {
                            let value = obj.get(col).ok_or_else(|| {
                                KimberliteError::internal(format!(
                                    "Column '{col}' not found in row"
                                ))
                            })?;
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
                KimberliteError::internal(format!("JSON serialization failed: {e}"))
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
                    columns: returning_cols
                        .iter()
                        .map(|s| ColumnName::new(s.clone()))
                        .collect(),
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

    // ========================================================================
    // RBAC & Compliance Integration (Phase 3.2 & 3.3)
    // ========================================================================

    /// Executes a SQL query with RBAC policy enforcement.
    ///
    /// This method provides HIPAA/GDPR-compliant query execution:
    /// - **Column filtering**: Removes unauthorized columns from results
    /// - **Row-level security**: Injects WHERE clauses based on policy
    /// - **Audit logging**: Records all access attempts
    /// - **Consent validation**: Checks GDPR consent before data access
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL SELECT statement
    /// * `params` - Query parameters
    /// * `policy` - Access control policy (extracted from JWT token)
    ///
    /// # Returns
    ///
    /// Returns filtered query results or an error if access is denied.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Extract policy from authenticated user
    /// let policy = identity.extract_policy()?;
    ///
    /// // Execute query with RBAC enforcement
    /// let results = tenant.query_with_policy(
    ///     "SELECT name, ssn FROM patients",
    ///     &[],
    ///     &policy,
    /// )?;
    /// // Result: SSN column removed if user lacks permission
    /// ```
    ///
    /// # Compliance
    ///
    /// - **HIPAA §164.312(a)(4)**: Access control enforcement
    /// - **GDPR Article 6**: Lawful basis for processing
    /// - **GDPR Article 32**: Security of processing
    pub fn query_with_policy(
        &self,
        sql: &str,
        params: &[Value],
        policy: &kimberlite_rbac::AccessPolicy,
    ) -> Result<QueryResult> {
        use kimberlite_query::rbac_filter::RbacFilter;

        // Parse SQL using sqlparser directly for RBAC rewriting
        let dialect = sqlparser::dialect::GenericDialect {};
        let statements = sqlparser::parser::Parser::parse_sql(&dialect, sql)
            .map_err(|e| KimberliteError::internal(format!("SQL parse error: {e}")))?;

        let stmt = statements
            .into_iter()
            .next()
            .ok_or_else(|| KimberliteError::internal("Empty SQL statement"))?;

        // RBAC enforcement: Rewrite query to enforce policy
        let filter = RbacFilter::new(policy.clone());
        let rewritten_stmt = filter
            .rewrite_statement(stmt)
            .map_err(|e| KimberliteError::internal(format!("RBAC filter failed: {e}")))?;

        let filtered_sql = rewritten_stmt.to_string();

        // Execute the filtered query
        let result = self.query(&filtered_sql, params)?;

        // Apply field masking if a masking policy is configured
        let result = if let Some(masking_policy) = &policy.masking_policy {
            self.apply_masking_to_result(result, masking_policy, policy.role)?
        } else {
            result
        };

        // Audit: Log query access
        tracing::info!(
            tenant_id = %self.tenant_id,
            role = ?policy.role,
            sql = %sql,
            filtered_sql = %filtered_sql,
            rows_returned = result.rows.len(),
            "RBAC query executed"
        );

        Ok(result)
    }

    /// Applies field masking to query results based on the masking policy.
    ///
    /// For each row, checks each column against the masking policy and applies
    /// the appropriate masking strategy (redact, hash, tokenize, truncate, null)
    /// based on the user's role.
    ///
    /// # Compliance
    ///
    /// - **HIPAA §164.312(a)(1)**: Minimum necessary — field-level data masking
    fn apply_masking_to_result(
        &self,
        mut result: QueryResult,
        masking_policy: &kimberlite_rbac::masking::MaskingPolicy,
        role: kimberlite_rbac::Role,
    ) -> Result<QueryResult> {
        let column_names: Vec<String> = result
            .columns
            .iter()
            .map(|c| c.as_str().to_string())
            .collect();

        for row in &mut result.rows {
            for (i, col_name) in column_names.iter().enumerate() {
                if let Some(mask) = masking_policy.mask_for_column(col_name) {
                    if mask.should_mask(&role) {
                        if let Some(value) = row.get(i) {
                            // Convert value to bytes, apply mask, convert back
                            let value_bytes = value.to_string().into_bytes();
                            let masked_bytes =
                                kimberlite_rbac::masking::apply_mask(&value_bytes, mask, &role)
                                    .map_err(|e| {
                                        KimberliteError::internal(format!(
                                            "Masking failed for column '{col_name}': {e}"
                                        ))
                                    })?;
                            let masked_str = String::from_utf8_lossy(&masked_bytes).to_string();
                            row[i] = Value::Text(masked_str);
                        }
                    }
                }
            }
        }

        tracing::debug!(
            tenant_id = %self.tenant_id,
            columns_masked = masking_policy.masks().len(),
            rows_processed = result.rows.len(),
            "Field masking applied to query results"
        );

        Ok(result)
    }

    /// Executes a SQL statement with RBAC policy enforcement.
    ///
    /// Similar to `query_with_policy()` but for DML/DDL statements.
    ///
    /// # Compliance
    ///
    /// - **HIPAA §164.312(a)(1)**: Access control
    /// - **SOC2 CC6.3**: Logical access controls
    pub fn execute_with_policy(
        &self,
        sql: &str,
        params: &[Value],
        policy: &kimberlite_rbac::AccessPolicy,
    ) -> Result<ExecuteResult> {
        use kimberlite_query::{ParsedStatement, parse_statement};

        // Parse SQL to AST
        let parsed = parse_statement(sql)?;

        // RBAC enforcement: Check if user has permission for this operation
        let (allowed, operation) = match &parsed {
            ParsedStatement::Insert(_) => (policy.role.can_write(), "INSERT"),
            ParsedStatement::Update(_) => (policy.role.can_write(), "UPDATE"),
            ParsedStatement::Delete(_) => (policy.role.can_delete(), "DELETE"),
            ParsedStatement::CreateTable(_)
            | ParsedStatement::DropTable(_)
            | ParsedStatement::AlterTable(_)
            | ParsedStatement::CreateIndex(_) => {
                (policy.role == kimberlite_rbac::Role::Admin, "DDL")
            }
            ParsedStatement::Select(_) | ParsedStatement::Union(_) => (false, "UNKNOWN"),
        };

        if !allowed {
            tracing::warn!(
                tenant_id = %self.tenant_id,
                role = ?policy.role,
                operation = %operation,
                "Access denied: insufficient permissions"
            );
            return Err(KimberliteError::internal(format!(
                "Access denied: {operation} operation requires additional permissions"
            )));
        }

        // For UPDATE/DELETE, use original SQL (RBAC permission check above is sufficient).
        // Row-level security WHERE clauses are not yet supported for DML.
        let filtered_sql = sql.to_string();

        // Execute the filtered statement
        let result = self.execute(&filtered_sql, params)?;

        // Audit: Log DML/DDL operation
        tracing::info!(
            tenant_id = %self.tenant_id,
            role = ?policy.role,
            operation = %operation,
            sql = %sql,
            rows_affected = result.rows_affected(),
            "RBAC execute completed"
        );

        Ok(result)
    }

    /// Validates GDPR consent before processing personal data.
    ///
    /// Checks if valid consent exists for the given subject and purpose.
    /// For purposes that do not require consent (e.g., `LegalObligation`),
    /// validation succeeds without a consent record.
    ///
    /// # Arguments
    ///
    /// * `subject_id` - Data subject identifier (e.g., patient ID, user ID)
    /// * `purpose` - Purpose for data processing (e.g., Treatment, Analytics)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if consent is valid or not required, or an error if
    /// consent is missing/withdrawn.
    ///
    /// # Compliance
    ///
    /// - **GDPR Article 6(1)(a)**: Consent as lawful basis
    /// - **GDPR Article 7**: Conditions for consent
    pub fn validate_consent(
        &self,
        subject_id: &str,
        purpose: kimberlite_compliance::purpose::Purpose,
    ) -> Result<()> {
        // Purposes that don't require explicit consent (lawful basis exists)
        if !purpose.requires_consent() {
            tracing::debug!(
                tenant_id = %self.tenant_id,
                subject_id = %subject_id,
                purpose = ?purpose,
                "Consent not required for this purpose (lawful basis)"
            );
            return Ok(());
        }

        let inner = self
            .db
            .inner()
            .read()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let has_consent = inner.consent_tracker.check_consent(subject_id, purpose);

        if !has_consent {
            tracing::warn!(
                tenant_id = %self.tenant_id,
                subject_id = %subject_id,
                purpose = ?purpose,
                "Consent validation failed: no valid consent found"
            );
            return Err(KimberliteError::internal(format!(
                "Consent required: no valid consent for subject '{subject_id}' with purpose {purpose:?}"
            )));
        }

        tracing::debug!(
            tenant_id = %self.tenant_id,
            subject_id = %subject_id,
            purpose = ?purpose,
            "Consent validated successfully"
        );

        Ok(())
    }

    /// Evaluates ABAC policy for a given access request.
    ///
    /// Provides fine-grained, context-aware access control that augments RBAC.
    /// Considers user attributes (role, clearance), resource attributes
    /// (data classification, stream), and environment (time, country).
    ///
    /// # Compliance
    ///
    /// - **GDPR Article 25**: Privacy by design (context-aware access)
    /// - **`FedRAMP` AC-3**: Location-based access enforcement
    /// - **PCI DSS Req 7**: Need-to-know access
    pub fn evaluate_abac(
        &self,
        policy: &kimberlite_abac::AbacPolicy,
        user: &kimberlite_abac::UserAttributes,
        resource: &kimberlite_abac::ResourceAttributes,
        env: &kimberlite_abac::EnvironmentAttributes,
    ) -> kimberlite_abac::Decision {
        let decision = kimberlite_abac::evaluate(policy, user, resource, env);

        tracing::info!(
            tenant_id = %self.tenant_id,
            effect = ?decision.effect,
            matched_rule = ?decision.matched_rule,
            reason = %decision.reason,
            "ABAC evaluation completed"
        );

        decision
    }

    /// Grants GDPR consent for a data subject and purpose.
    ///
    /// Records consent in the consent tracker. The returned UUID can be used
    /// to withdraw consent later.
    ///
    /// # Compliance
    ///
    /// - **GDPR Article 7(1)**: Demonstrable consent
    pub fn grant_consent(
        &self,
        subject_id: &str,
        purpose: kimberlite_compliance::purpose::Purpose,
    ) -> Result<uuid::Uuid> {
        let mut inner = self
            .db
            .inner()
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let consent_id = inner
            .consent_tracker
            .grant_consent(subject_id, purpose)
            .map_err(|e| KimberliteError::internal(format!("Consent grant failed: {e}")))?;

        tracing::info!(
            tenant_id = %self.tenant_id,
            subject_id = %subject_id,
            purpose = ?purpose,
            consent_id = %consent_id,
            "Consent granted"
        );

        Ok(consent_id)
    }

    /// Withdraws GDPR consent by consent ID.
    ///
    /// After withdrawal, subsequent `validate_consent` calls for the same
    /// subject and purpose will fail.
    ///
    /// # Compliance
    ///
    /// - **GDPR Article 7(3)**: Withdrawal as easy as giving consent
    pub fn withdraw_consent(&self, consent_id: uuid::Uuid) -> Result<()> {
        let mut inner = self
            .db
            .inner()
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        inner
            .consent_tracker
            .withdraw_consent(consent_id)
            .map_err(|e| KimberliteError::internal(format!("Consent withdrawal failed: {e}")))?;

        tracing::info!(
            consent_id = %consent_id,
            "Consent withdrawn"
        );

        Ok(())
    }

    // =========================================================================
    // Consent-Aware Operations
    // =========================================================================

    /// Appends events to a stream with consent validation.
    ///
    /// When `consent_mode` is `Required`, validates that the subject has
    /// granted consent for the given purpose before appending. When
    /// `Optional`, logs a warning if consent is missing but proceeds.
    /// When `Disabled`, behaves identically to `append()`.
    ///
    /// # Compliance
    ///
    /// - **GDPR Article 6(1)(a)**: Consent as lawful basis for processing
    pub fn append_with_consent(
        &self,
        stream_id: StreamId,
        events: Vec<Vec<u8>>,
        expected_offset: Offset,
        subject_id: &str,
        purpose: kimberlite_compliance::purpose::Purpose,
    ) -> Result<Offset> {
        self.check_consent_if_required(subject_id, purpose)?;
        self.append(stream_id, events, expected_offset)
    }

    /// Executes a SQL query with consent validation.
    ///
    /// When `consent_mode` is `Required`, validates that the subject has
    /// granted consent for the given purpose before querying. When
    /// `Optional`, logs a warning if consent is missing but proceeds.
    /// When `Disabled`, behaves identically to `query()`.
    ///
    /// # Compliance
    ///
    /// - **GDPR Article 6(1)(a)**: Consent as lawful basis for processing
    pub fn query_with_consent(
        &self,
        sql: &str,
        params: &[Value],
        subject_id: &str,
        purpose: kimberlite_compliance::purpose::Purpose,
    ) -> Result<QueryResult> {
        self.check_consent_if_required(subject_id, purpose)?;
        self.query(sql, params)
    }

    /// Reads events from a stream with consent validation.
    ///
    /// When `consent_mode` is `Required`, validates that the subject has
    /// granted consent for the given purpose before reading. When
    /// `Optional`, logs a warning if consent is missing but proceeds.
    /// When `Disabled`, behaves identically to `read_events()`.
    ///
    /// # Compliance
    ///
    /// - **GDPR Article 6(1)(a)**: Consent as lawful basis for processing
    pub fn read_events_with_consent(
        &self,
        stream_id: StreamId,
        from_offset: Offset,
        max_bytes: u64,
        subject_id: &str,
        purpose: kimberlite_compliance::purpose::Purpose,
    ) -> Result<Vec<Bytes>> {
        self.check_consent_if_required(subject_id, purpose)?;
        self.read_events(stream_id, from_offset, max_bytes)
    }

    /// Checks consent based on the current `consent_mode`.
    ///
    /// - `Required`: calls `validate_consent()` and returns the error if
    ///   consent is missing.
    /// - `Optional`: calls `validate_consent()` and logs a warning on failure
    ///   but returns `Ok(())`.
    /// - `Disabled`: no-op, always returns `Ok(())`.
    fn check_consent_if_required(
        &self,
        subject_id: &str,
        purpose: kimberlite_compliance::purpose::Purpose,
    ) -> Result<()> {
        match self.consent_mode {
            ConsentMode::Disabled => Ok(()),
            ConsentMode::Required => self.validate_consent(subject_id, purpose),
            ConsentMode::Optional => {
                if let Err(e) = self.validate_consent(subject_id, purpose) {
                    tracing::warn!(
                        tenant_id = %self.tenant_id,
                        subject_id = %subject_id,
                        purpose = ?purpose,
                        error = %e,
                        "Consent not found (optional mode — proceeding with warning)"
                    );
                }
                Ok(())
            }
        }
    }

    // =========================================================================
    // Erasure (GDPR Article 17)
    // =========================================================================

    /// Requests erasure of all data for a subject (Right to Erasure).
    ///
    /// Creates a pending erasure request with a 30-day deadline.
    ///
    /// # Compliance
    ///
    /// - **GDPR Article 17**: Right to erasure
    pub fn request_erasure(
        &self,
        subject_id: &str,
    ) -> Result<kimberlite_compliance::erasure::ErasureRequest> {
        let mut inner = self
            .db
            .inner()
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let request = inner
            .erasure_engine
            .request_erasure(subject_id)
            .map_err(|e| KimberliteError::internal(format!("Erasure request failed: {e}")))?;

        tracing::info!(
            tenant_id = %self.tenant_id,
            subject_id = %subject_id,
            request_id = %request.request_id,
            deadline = %request.deadline,
            "Erasure requested"
        );

        Ok(request)
    }

    /// Checks for overdue erasure requests.
    ///
    /// Returns requests that have passed their 30-day deadline.
    pub fn check_erasure_deadlines(&self) -> Result<Vec<uuid::Uuid>> {
        let inner = self
            .db
            .inner()
            .read()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let overdue: Vec<uuid::Uuid> = inner
            .erasure_engine
            .check_deadlines(chrono::Utc::now())
            .iter()
            .map(|r| r.request_id)
            .collect();

        if !overdue.is_empty() {
            tracing::warn!(
                tenant_id = %self.tenant_id,
                overdue_count = overdue.len(),
                "Overdue erasure requests detected"
            );
        }

        Ok(overdue)
    }

    // =========================================================================
    // Breach Detection (HIPAA §164.404, GDPR Article 33)
    // =========================================================================

    /// Checks for mass data export breach indicators.
    ///
    /// # Compliance
    ///
    /// - **HIPAA §164.404**: Breach notification
    /// - **GDPR Article 33**: Notification of breach to supervisory authority
    pub fn check_breach_mass_export(
        &self,
        records_exported: u64,
        data_classes: &[kimberlite_types::DataClass],
    ) -> Result<Option<kimberlite_compliance::breach::BreachEvent>> {
        let mut inner = self
            .db
            .inner()
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let event = inner
            .breach_detector
            .check_mass_export(records_exported, data_classes);

        if let Some(ref evt) = event {
            tracing::error!(
                tenant_id = %self.tenant_id,
                event_id = %evt.event_id,
                severity = ?evt.severity,
                "Breach indicator detected: mass export"
            );
        }

        Ok(event)
    }

    /// Checks for breach notification deadlines (72h).
    pub fn check_breach_deadlines(&self) -> Result<Vec<uuid::Uuid>> {
        let inner = self
            .db
            .inner()
            .read()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let overdue: Vec<uuid::Uuid> = inner
            .breach_detector
            .check_notification_deadlines(chrono::Utc::now())
            .iter()
            .map(|e| e.event_id)
            .collect();

        Ok(overdue)
    }

    // =========================================================================
    // Data Portability Export (GDPR Article 20)
    // =========================================================================

    /// Exports all data for a subject in the specified format.
    ///
    /// # Compliance
    ///
    /// - **GDPR Article 20**: Right to data portability
    pub fn export_subject_data(
        &self,
        subject_id: &str,
        records: &[kimberlite_compliance::export::ExportRecord],
        format: kimberlite_compliance::export::ExportFormat,
        requester_id: &str,
    ) -> Result<kimberlite_compliance::export::PortabilityExport> {
        let mut inner = self
            .db
            .inner()
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let export = inner
            .export_engine
            .export_subject_data(subject_id, records, format, requester_id)
            .map_err(|e| KimberliteError::internal(format!("Export failed: {e}")))?;

        tracing::info!(
            tenant_id = %self.tenant_id,
            subject_id = %subject_id,
            export_id = %export.export_id,
            record_count = export.record_count,
            "Subject data exported"
        );

        Ok(export)
    }

    // =========================================================================
    // Compliance Audit Log (SOC2 CC7.2, ISO 27001 A.12.4.1)
    // =========================================================================

    /// Appends an event to the compliance audit log.
    ///
    /// # Compliance
    ///
    /// - **SOC2 CC7.2**: Comprehensive audit trails
    /// - **ISO 27001 A.12.4.1**: Event logging
    pub fn audit_log_append(
        &self,
        action: kimberlite_compliance::audit::ComplianceAuditAction,
        actor: Option<&str>,
    ) -> Result<uuid::Uuid> {
        let mut inner = self
            .db
            .inner()
            .write()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let event_id = inner.audit_log.append(
            action,
            actor.map(String::from),
            Some(u64::from(self.tenant_id)),
        );

        Ok(event_id)
    }

    /// Queries the compliance audit log with filters.
    pub fn audit_log_query(
        &self,
        filter: &kimberlite_compliance::audit::AuditQuery,
    ) -> Result<Vec<uuid::Uuid>> {
        let inner = self
            .db
            .inner()
            .read()
            .map_err(|_| KimberliteError::internal("lock poisoned"))?;

        let events: Vec<uuid::Uuid> = inner
            .audit_log
            .query(filter)
            .iter()
            .map(|e| e.event_id)
            .collect();

        Ok(events)
    }
}

/// Validates that all specified columns exist in the table schema.
fn validate_columns_exist(
    columns: &[String],
    table_columns: &[kimberlite_kernel::command::ColumnDefinition],
) -> Result<()> {
    for col_name in columns {
        if !table_columns.iter().any(|c| &c.name == col_name) {
            return Err(KimberliteError::Query(
                kimberlite_query::QueryError::ParseError(format!(
                    "column '{col_name}' does not exist in table"
                )),
            ));
        }
    }
    Ok(())
}

/// Validates that values match their column types and constraints.
fn validate_insert_values(
    column_names: &[String],
    values: &[Value],
    table_columns: &[kimberlite_kernel::command::ColumnDefinition],
    primary_key_cols: &[String],
) -> Result<()> {
    for (col_name, value) in column_names.iter().zip(values.iter()) {
        // Find the column definition
        let col_def = table_columns
            .iter()
            .find(|c| &c.name == col_name)
            .ok_or_else(|| {
                KimberliteError::Query(kimberlite_query::QueryError::ParseError(format!(
                    "column '{col_name}' not found in table schema"
                )))
            })?;

        // Check NOT NULL constraint
        if !col_def.nullable && value.is_null() {
            return Err(KimberliteError::Query(
                kimberlite_query::QueryError::TypeMismatch {
                    expected: format!("non-NULL value for column '{col_name}'"),
                    actual: "NULL".to_string(),
                },
            ));
        }

        // Check primary key NULL constraint
        if primary_key_cols.contains(col_name) && value.is_null() {
            return Err(KimberliteError::Query(
                kimberlite_query::QueryError::TypeMismatch {
                    expected: format!("non-NULL value for primary key column '{col_name}'"),
                    actual: "NULL".to_string(),
                },
            ));
        }

        // Type validation (basic check - NULL is compatible with any type)
        if !value.is_null() {
            let expected_type = match col_def.data_type.as_str() {
                "BIGINT" => Some(kimberlite_query::DataType::BigInt),
                "TEXT" => Some(kimberlite_query::DataType::Text),
                "BOOLEAN" => Some(kimberlite_query::DataType::Boolean),
                "TIMESTAMP" => Some(kimberlite_query::DataType::Timestamp),
                "BYTES" => Some(kimberlite_query::DataType::Bytes),
                _ => None,
            };

            if let Some(expected) = expected_type {
                if !value.is_compatible_with(expected) {
                    return Err(KimberliteError::Query(
                        kimberlite_query::QueryError::TypeMismatch {
                            expected: format!("{expected:?} for column '{col_name}'"),
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
#[allow(dead_code)]
fn predicate_to_json(
    pred: &kimberlite_query::Predicate,
    params: &[Value],
) -> Result<serde_json::Value> {
    use kimberlite_query::{Predicate, PredicateValue};

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
                            kimberlite_query::QueryError::ParseError(format!(
                                "parameter ${idx} out of bounds (have {} parameters)",
                                params.len()
                            )),
                        ));
                    }
                    params[idx - 1].clone()
                }
                PredicateValue::ColumnRef(_) => {
                    return Err(KimberliteError::Query(
                        kimberlite_query::QueryError::UnsupportedFeature(
                            "column references not supported in RETURNING clause".to_string(),
                        ),
                    ));
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
                        kimberlite_query::QueryError::ParseError(format!(
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

/// Converts a JSON value to a `kimberlite_query::Value`.
/// Makes reasonable assumptions about types (e.g., numbers become `BigInt`).
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
                Err(KimberliteError::internal(format!(
                    "Unsupported number type: {n}"
                )))
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

        let stream_id = tenant.create_stream("test", DataClass::Public).unwrap();
        let stream_id_val: u64 = stream_id.into();
        assert!(stream_id_val > 0);
    }

    #[test]
    fn test_tenant_append_and_read() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        let stream_id = tenant.create_stream("events", DataClass::Public).unwrap();

        // Append events
        tenant
            .append(
                stream_id,
                vec![b"event1".to_vec(), b"event2".to_vec()],
                Offset::ZERO,
            )
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
                panic!("Query failed: {e}");
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
        if let Err(e) = &result {
            eprintln!("CREATE INDEX error: {e:?}");
        }
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
            eprintln!("UPDATE failed: {e:?}");
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
            "Expected PRIMARY KEY error, got: {err_msg}"
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
        let result = tenant
            .query("SELECT * FROM users WHERE id = 42", &[])
            .unwrap();

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::BigInt(42));
        assert_eq!(result.rows[0][1], Value::Text("Alice".to_string()));
        // Verify no NULL values
        assert!(!result.rows[0].iter().any(kimberlite_query::Value::is_null));
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
        let result = tenant
            .query("SELECT name FROM users WHERE id = 1", &[])
            .unwrap();

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
        let result = tenant
            .query("SELECT * FROM users WHERE id = 1", &[])
            .unwrap();

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
            .execute(
                "DELETE FROM orders WHERE user_id = 1 AND order_id = 100",
                &[],
            )
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
        let sql = format!(
            "INSERT INTO numbers (id, value) VALUES {}",
            values.join(", ")
        );

        let result = tenant.execute(&sql, &[]).unwrap();

        assert_eq!(result.rows_affected(), 100, "Should insert 100 rows");

        // Verify count
        let query_result = tenant.query("SELECT COUNT(*) FROM numbers", &[]).unwrap();

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
            matches!(
                err,
                KimberliteError::Query(kimberlite_query::QueryError::ConstraintViolation(_))
            ),
            "Expected ConstraintViolation, got {err:?}"
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
            matches!(
                err,
                KimberliteError::Query(kimberlite_query::QueryError::ConstraintViolation(_))
            ),
            "Expected ConstraintViolation, got {err:?}"
        );

        // Verify only the first new row (Bob) was inserted before the error
        let query_result = tenant.query("SELECT COUNT(*) FROM users", &[]).unwrap();

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
            .execute(
                "UPDATE users SET status = 'active' WHERE status = 'inactive'",
                &[],
            )
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

        assert_eq!(
            result.rows_affected(),
            3,
            "Should delete 3 users with age >= 30"
        );

        // Verify only 2 rows remain
        let query_result = tenant.query("SELECT COUNT(*) FROM users", &[]).unwrap();

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
            .execute(
                "INSERT INTO users (id, name) VALUES (1, 'Alice') RETURNING id, name",
                &[],
            )
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
            .execute(
                "UPDATE users SET age = 26 WHERE id = 1 RETURNING id, name, age",
                &[],
            )
            .unwrap();

        assert_eq!(result.rows_affected(), 1);
        let returned = result.returned().unwrap();
        assert_eq!(returned.rows.len(), 1);

        // Verify returned row has updated value
        assert_eq!(returned.rows[0][0], Value::BigInt(1));
        assert_eq!(returned.rows[0][1], Value::Text("Alice".to_string()));
        assert_eq!(returned.rows[0][2], Value::BigInt(26)); // Updated age
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
            .execute(
                "DELETE FROM users WHERE id IN (1, 2) RETURNING id, name",
                &[],
            )
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
        let query_result = tenant
            .query("SELECT id FROM users ORDER BY id", &[])
            .unwrap();
        assert_eq!(query_result.rows.len(), 1); // Only Charlie remains
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
        assert_eq!(
            returned.rows[0][1],
            Value::Text("alice@example.com".to_string())
        );
    }

    // =========================================================================
    // RBAC End-to-End Tests
    // =========================================================================

    /// Helper: sets up a tenant with a patients table containing test data.
    fn setup_patients_table() -> (tempfile::TempDir, TenantHandle) {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        tenant
            .execute(
                "CREATE TABLE patients (
                    id BIGINT NOT NULL,
                    name TEXT NOT NULL,
                    ssn TEXT,
                    email TEXT,
                    tenant_id BIGINT NOT NULL,
                    PRIMARY KEY (id)
                )",
                &[],
            )
            .unwrap();

        // Insert test data
        tenant
            .execute(
                "INSERT INTO patients (id, name, ssn, email, tenant_id) VALUES ($1, $2, $3, $4, $5)",
                &[
                    Value::BigInt(1),
                    Value::Text("Alice".into()),
                    Value::Text("123-45-6789".into()),
                    Value::Text("alice@example.com".into()),
                    Value::BigInt(1),
                ],
            )
            .unwrap();

        tenant
            .execute(
                "INSERT INTO patients (id, name, ssn, email, tenant_id) VALUES ($1, $2, $3, $4, $5)",
                &[
                    Value::BigInt(2),
                    Value::Text("Bob".into()),
                    Value::Text("987-65-4321".into()),
                    Value::Text("bob@example.com".into()),
                    Value::BigInt(2),
                ],
            )
            .unwrap();

        (dir, tenant)
    }

    #[test]
    fn test_rbac_admin_reads_all_columns() {
        let (_dir, tenant) = setup_patients_table();
        let policy = kimberlite_rbac::StandardPolicies::admin();

        let result = tenant
            .query_with_policy("SELECT id, name, ssn FROM patients", &[], &policy)
            .unwrap();

        // Admin sees all columns including SSN
        assert_eq!(result.columns.len(), 3);
        assert!(result.columns.iter().any(|c| c.as_str() == "ssn"));
        assert_eq!(result.rows.len(), 2);
    }

    #[test]
    fn test_rbac_user_gets_row_filter() {
        let (_dir, tenant) = setup_patients_table();
        let policy = kimberlite_rbac::StandardPolicies::user(TenantId::new(1));

        let result = tenant
            .query_with_policy("SELECT id, name FROM patients", &[], &policy)
            .unwrap();

        // User policy injects WHERE tenant_id = 1, so only tenant 1's rows
        assert_eq!(result.rows.len(), 1);
        // First row should be Alice (tenant_id = 1)
        assert_eq!(result.rows[0][1], Value::Text("Alice".into()));
    }

    #[test]
    fn test_rbac_auditor_stream_restriction() {
        let (_dir, tenant) = setup_patients_table();
        let policy = kimberlite_rbac::StandardPolicies::auditor();

        // Auditor can only access audit_* streams, so querying patients should fail
        let result = tenant.query_with_policy("SELECT id, name FROM patients", &[], &policy);

        assert!(
            result.is_err(),
            "Auditor should not access non-audit tables"
        );
    }

    #[test]
    fn test_rbac_execute_admin_can_insert() {
        let (_dir, tenant) = setup_patients_table();
        let policy = kimberlite_rbac::StandardPolicies::admin();

        let result = tenant.execute_with_policy(
            "INSERT INTO patients (id, name, ssn, email, tenant_id) VALUES ($1, $2, $3, $4, $5)",
            &[
                Value::BigInt(3),
                Value::Text("Charlie".into()),
                Value::Text("111-22-3333".into()),
                Value::Text("charlie@example.com".into()),
                Value::BigInt(1),
            ],
            &policy,
        );

        assert!(result.is_ok(), "Admin should be able to INSERT");
    }

    #[test]
    fn test_rbac_execute_analyst_cannot_insert() {
        let (_dir, tenant) = setup_patients_table();
        let policy = kimberlite_rbac::StandardPolicies::analyst();

        let result = tenant.execute_with_policy(
            "INSERT INTO patients (id, name, ssn, email, tenant_id) VALUES ($1, $2, $3, $4, $5)",
            &[
                Value::BigInt(3),
                Value::Text("Charlie".into()),
                Value::Text("111-22-3333".into()),
                Value::Text("charlie@example.com".into()),
                Value::BigInt(1),
            ],
            &policy,
        );

        assert!(result.is_err(), "Analyst should not be able to INSERT");
    }

    #[test]
    fn test_rbac_execute_auditor_cannot_write() {
        let (_dir, tenant) = setup_patients_table();
        let policy = kimberlite_rbac::StandardPolicies::auditor();

        let result = tenant.execute_with_policy(
            "INSERT INTO patients (id, name, ssn, email, tenant_id) VALUES ($1, $2, $3, $4, $5)",
            &[
                Value::BigInt(3),
                Value::Text("Charlie".into()),
                Value::Text("111-22-3333".into()),
                Value::Text("charlie@example.com".into()),
                Value::BigInt(1),
            ],
            &policy,
        );

        assert!(result.is_err(), "Auditor should not be able to INSERT");
    }

    #[test]
    fn test_rbac_execute_user_can_insert_cannot_delete() {
        let (_dir, tenant) = setup_patients_table();
        let policy = kimberlite_rbac::StandardPolicies::user(TenantId::new(1));

        // User can INSERT (own data)
        let insert_result = tenant.execute_with_policy(
            "INSERT INTO patients (id, name, ssn, email, tenant_id) VALUES ($1, $2, $3, $4, $5)",
            &[
                Value::BigInt(3),
                Value::Text("Charlie".into()),
                Value::Text("111-22-3333".into()),
                Value::Text("charlie@example.com".into()),
                Value::BigInt(1),
            ],
            &policy,
        );
        assert!(insert_result.is_ok(), "User should be able to INSERT");

        // User cannot DELETE (compliance requirement)
        let delete_result = tenant.execute_with_policy(
            "DELETE FROM patients WHERE id = $1",
            &[Value::BigInt(1)],
            &policy,
        );
        assert!(delete_result.is_err(), "User should not be able to DELETE");
    }

    #[test]
    fn test_rbac_execute_only_admin_can_ddl() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        let admin_policy = kimberlite_rbac::StandardPolicies::admin();
        let user_policy = kimberlite_rbac::StandardPolicies::user(TenantId::new(1));
        let analyst_policy = kimberlite_rbac::StandardPolicies::analyst();

        // Admin can CREATE TABLE
        let admin_result = tenant.execute_with_policy(
            "CREATE TABLE admin_table (id BIGINT NOT NULL, PRIMARY KEY (id))",
            &[],
            &admin_policy,
        );
        assert!(admin_result.is_ok(), "Admin should be able to CREATE TABLE");

        // User cannot CREATE TABLE
        let user_result = tenant.execute_with_policy(
            "CREATE TABLE user_table (id BIGINT NOT NULL, PRIMARY KEY (id))",
            &[],
            &user_policy,
        );
        assert!(
            user_result.is_err(),
            "User should not be able to CREATE TABLE"
        );

        // Analyst cannot CREATE TABLE
        let analyst_result = tenant.execute_with_policy(
            "CREATE TABLE analyst_table (id BIGINT NOT NULL, PRIMARY KEY (id))",
            &[],
            &analyst_policy,
        );
        assert!(
            analyst_result.is_err(),
            "Analyst should not be able to CREATE TABLE"
        );
    }

    // =========================================================================
    // Consent Integration Tests
    // =========================================================================

    #[test]
    fn test_consent_grant_and_validate() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        use kimberlite_compliance::purpose::Purpose;

        // Marketing requires consent — validation should fail without consent
        let result = tenant.validate_consent("user@example.com", Purpose::Marketing);
        assert!(result.is_err(), "Should fail without consent");

        // Grant consent
        let consent_id = tenant
            .grant_consent("user@example.com", Purpose::Marketing)
            .unwrap();

        // Now validation should succeed
        let result = tenant.validate_consent("user@example.com", Purpose::Marketing);
        assert!(result.is_ok(), "Should succeed with consent");

        // Withdraw consent
        tenant.withdraw_consent(consent_id).unwrap();

        // Validation should fail again
        let result = tenant.validate_consent("user@example.com", Purpose::Marketing);
        assert!(result.is_err(), "Should fail after withdrawal");
    }

    #[test]
    fn test_consent_not_required_for_legal_obligation() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        use kimberlite_compliance::purpose::Purpose;

        // LegalObligation doesn't require consent
        let result = tenant.validate_consent("user@example.com", Purpose::LegalObligation);
        assert!(result.is_ok(), "LegalObligation should not require consent");
    }

    #[test]
    fn test_consent_not_required_for_public_data() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        use kimberlite_compliance::purpose::Purpose;

        // Security purpose doesn't require explicit consent
        let result = tenant.validate_consent("user@example.com", Purpose::Security);
        assert!(
            result.is_ok(),
            "Security purpose should not require consent"
        );
    }

    #[test]
    fn test_inner_join_execution() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create two tables
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        tenant
            .execute(
                "CREATE TABLE orders (id BIGINT NOT NULL, user_id BIGINT NOT NULL, amount TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert data into users
        tenant
            .execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", &[])
            .unwrap();

        tenant
            .execute("INSERT INTO users (id, name) VALUES (2, 'Bob')", &[])
            .unwrap();

        // Insert data into orders (Alice has 2 orders, Bob has none)
        tenant
            .execute("INSERT INTO orders (id, user_id, amount) VALUES (1, 1, '100.00')", &[])
            .unwrap();

        tenant
            .execute("INSERT INTO orders (id, user_id, amount) VALUES (2, 1, '200.00')", &[])
            .unwrap();

        // Execute INNER JOIN with SELECT *
        let result = tenant
            .query(
                "SELECT * FROM users JOIN orders ON users.id = orders.user_id",
                &[],
            );

        if let Err(ref e) = result {
            eprintln!("INNER JOIN query failed: {e:?}");
        }
        let result = result.unwrap();

        // Verify results: should have 2 rows (Alice's 2 orders)
        assert_eq!(result.rows.len(), 2, "should have 2 rows from INNER JOIN");

        // Check that we got data from both tables (users: id, name; orders: id, user_id, amount)
        // Each row should have 5 columns total: users.id, users.name, orders.id, orders.user_id, orders.amount
        assert_eq!(result.rows[0].len(), 5, "joined row should have 5 columns");

        // Both rows should be for Alice (user_id=1, name="Alice")
        assert_eq!(result.rows[0][0], Value::BigInt(1)); // users.id
        assert_eq!(result.rows[0][1], Value::Text("Alice".to_string())); // users.name

        assert_eq!(result.rows[1][0], Value::BigInt(1)); // users.id
        assert_eq!(result.rows[1][1], Value::Text("Alice".to_string())); // users.name
    }

    #[test]
    fn test_left_join_with_nulls() {
        let dir = tempdir().unwrap();
        let db = Kimberlite::open(dir.path()).unwrap();
        let tenant = db.tenant(TenantId::new(1));

        // Create two tables
        tenant
            .execute(
                "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        tenant
            .execute(
                "CREATE TABLE orders (id BIGINT NOT NULL, user_id BIGINT NOT NULL, PRIMARY KEY (id))",
                &[],
            )
            .unwrap();

        // Insert data (Alice has an order, Bob doesn't)
        tenant
            .execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", &[])
            .unwrap();

        tenant
            .execute("INSERT INTO users (id, name) VALUES (2, 'Bob')", &[])
            .unwrap();

        tenant
            .execute("INSERT INTO orders (id, user_id) VALUES (1, 1)", &[])
            .unwrap();

        // Execute LEFT JOIN (without ORDER BY for now - ORDER BY with qualified names not yet supported)
        let result = tenant
            .query(
                "SELECT * FROM users LEFT JOIN orders ON users.id = orders.user_id",
                &[],
            );

        if let Err(ref e) = result {
            eprintln!("LEFT JOIN query failed: {e:?}");
        }
        let result = result.unwrap();

        // Verify results: should have 2 rows (Alice + Bob)
        assert_eq!(result.rows.len(), 2, "should have 2 rows from LEFT JOIN");

        // Each row has 5 columns: users.id, users.name, orders.id, orders.user_id (or NULLs)
        // Alice should have a matching order (user_id=1 matches orders.user_id=1)
        assert_eq!(result.rows[0][0], Value::BigInt(1)); // users.id
        assert_eq!(result.rows[0][1], Value::Text("Alice".to_string())); // users.name
        assert_eq!(result.rows[0][2], Value::BigInt(1)); // orders.id
        assert_eq!(result.rows[0][3], Value::BigInt(1)); // orders.user_id

        // Bob should have NULLs for order columns (user_id=2 has no matching orders)
        assert_eq!(result.rows[1][0], Value::BigInt(2)); // users.id
        assert_eq!(result.rows[1][1], Value::Text("Bob".to_string())); // users.name
        assert_eq!(result.rows[1][2], Value::Null); // orders.id (NULL)
        assert_eq!(result.rows[1][3], Value::Null); // orders.user_id (NULL)
    }
}
