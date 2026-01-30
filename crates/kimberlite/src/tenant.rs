//! Tenant-scoped handle for database operations.
//!
//! A `TenantHandle` provides operations scoped to a specific tenant ID.
//! All data isolation and access control is handled at this layer.

use bytes::Bytes;
use kmb_kernel::Command;
use kmb_kernel::command::{ColumnDefinition, IndexId, TableId};
use kmb_query::{
    ParsedCreateIndex, ParsedCreateTable, ParsedDelete, ParsedInsert, ParsedUpdate, QueryResult,
    Value,
};
use kmb_types::{DataClass, Offset, Placement, StreamId, StreamName, TenantId};
use serde_json::json;

use crate::error::{KimberliteError, Result};
use crate::kimberlite::Kimberlite;

/// Result of executing a DDL/DML statement.
#[derive(Debug, Clone)]
pub struct ExecuteResult {
    /// Number of rows affected (for DML).
    pub rows_affected: u64,
    /// Log offset of the operation (for audit trail).
    pub log_offset: Offset,
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

        Ok(ExecuteResult {
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

        Ok(ExecuteResult {
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

        Ok(ExecuteResult {
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

        // Validate value count matches column count
        if column_names.len() != insert.values.len() {
            return Err(KimberliteError::Query(
                kmb_query::QueryError::TypeMismatch {
                    expected: format!("{} values", column_names.len()),
                    actual: format!("{} values provided", insert.values.len()),
                },
            ));
        }

        // Bind parameters to placeholders
        let bound_values = bind_parameters(&insert.values, params)?;

        // Validate column types and constraints
        validate_insert_values(
            &column_names,
            &bound_values,
            &table_meta.columns,
            &table_meta.primary_key,
        )?;

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

        Ok(ExecuteResult {
            rows_affected: 1,
            log_offset: self.log_position()?,
        })
    }

    fn execute_update(&self, update: ParsedUpdate, params: &[Value]) -> Result<ExecuteResult> {
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
            .find(|(_, meta)| meta.table_name == update.table)
            .map(|(id, _)| *id)
            .ok_or_else(|| KimberliteError::TableNotFound(update.table.clone()))?;

        drop(inner);

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

        // Serialize update as JSON event with structured predicates
        let predicates_json: Result<Vec<serde_json::Value>> = update
            .predicates
            .iter()
            .map(|p| predicate_to_json(p, params))
            .collect();

        let event = json!({
            "type": "update",
            "table": update.table,
            "set": bound_assignments,
            "where": predicates_json?,
        });

        let row_data = Bytes::from(serde_json::to_vec(&event).map_err(|e| {
            KimberliteError::internal(format!("JSON serialization failed: {}", e))
        })?);

        let cmd = Command::Update { table_id, row_data };
        self.db.submit(cmd)?;

        Ok(ExecuteResult {
            // Currently always 1 since we only support primary key updates (single row)
            // Multi-row updates would require table scan and COUNT tracking
            rows_affected: 1,
            log_offset: self.log_position()?,
        })
    }

    fn execute_delete(&self, delete: ParsedDelete, params: &[Value]) -> Result<ExecuteResult> {
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
            .find(|(_, meta)| meta.table_name == delete.table)
            .map(|(id, _)| *id)
            .ok_or_else(|| KimberliteError::TableNotFound(delete.table.clone()))?;

        drop(inner);

        // Serialize delete as JSON event with structured predicates
        let predicates_json: Result<Vec<serde_json::Value>> = delete
            .predicates
            .iter()
            .map(|p| predicate_to_json(p, params))
            .collect();

        let event = json!({
            "type": "delete",
            "table": delete.table,
            "where": predicates_json?,
        });

        let row_data = Bytes::from(serde_json::to_vec(&event).map_err(|e| {
            KimberliteError::internal(format!("JSON serialization failed: {}", e))
        })?);

        let cmd = Command::Delete { table_id, row_data };
        self.db.submit(cmd)?;

        Ok(ExecuteResult {
            // Currently always 1 since we only support primary key deletes (single row)
            // Multi-row deletes would require table scan and COUNT tracking
            rows_affected: 1,
            log_offset: self.log_position()?,
        })
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
    match val {
        Value::Null => serde_json::Value::Null,
        Value::BigInt(n) => json!(n),
        Value::Text(s) => json!(s),
        Value::Boolean(b) => json!(b),
        Value::Timestamp(t) => json!(t.as_nanos()),
        Value::Bytes(b) => {
            // Encode bytes as base64
            use base64::Engine;
            json!(base64::engine::general_purpose::STANDARD.encode(b))
        }
        Value::Placeholder(idx) => {
            panic!("Cannot convert unbound placeholder ${idx} to JSON - bind parameters first")
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
        assert_eq!(result.rows_affected, 0);
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

        assert_eq!(result.rows_affected, 1);
        assert!(result.log_offset.as_u64() > 0);
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
        assert_eq!(result.unwrap().rows_affected, 1);
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
        assert_eq!(result.unwrap().rows_affected, 1);
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
}
