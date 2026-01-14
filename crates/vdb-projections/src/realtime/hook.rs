//! Transaction hooks for capturing SQLite mutations with strong consistency.
//!
//! This module provides the core event capture mechanism for VerityDB using
//! two cooperating SQLite hooks:
//!
//! - **`preupdate_hook`**: Fires before each INSERT/UPDATE/DELETE, captures
//!   changes into a buffer
//! - **`commit_hook`**: Fires before COMMIT, persists buffer to event log,
//!   returns `false` to rollback if persistence fails
//!
//! # Strong Consistency Guarantee
//!
//! Events are persisted to the durable event log **before** SQLite commits.
//! If VSR consensus fails, the SQLite transaction rolls back. This ensures
//! no write can exist in SQLite that isn't in the event log.
//!
//! # Healthcare Compliance
//!
//! This design is critical for HIPAA compliance:
//! - Every mutation is captured in an immutable audit trail
//! - Point-in-time recovery is always possible
//! - No data can "slip through" without being logged

use bytes::Bytes;
use sqlx::{
    SqlitePool,
    sqlite::{PreupdateHookResult, SqliteOperation},
};
use std::sync::{Arc, Mutex};
use vdb_types::{EventPersister, StreamId};

use crate::{ChangeEvent, ProjectionError, RowId, SqlValue, TableName, schema::SchemaCache};

/// Shared state between the preupdate_hook and commit_hook.
///
/// Both hooks hold an `Arc<HookContext>`:
/// - `preupdate_hook`: Captures `ChangeEvent`s into the buffer
/// - `commit_hook`: Drains buffer, persists to event log, returns success/failure
///
/// # Thread Safety
///
/// The buffer is protected by a `Mutex`. Since SQLite serializes writes
/// (one writer at a time), contention is minimal. The mutex ensures memory
/// safety if hooks were ever called from different threads.
#[derive(Clone, Debug)]
pub struct HookContext {
    /// Events captured during the current transaction.
    /// Cleared by `commit_hook` after successful persistence.
    buffer: Arc<Mutex<Vec<ChangeEvent>>>,
    /// Schema cache for table→column lookups during event capture.
    schema_cache: Arc<SchemaCache>,
    /// Handle to persist events to the durable log (implements VSR consensus).
    persister: Arc<dyn EventPersister>,
    /// Stream ID for this projection's events.
    stream_id: StreamId,
}

/// Installs SQLite hooks for transparent event capture with strong consistency.
///
/// `TransactionHooks` sets up two cooperating SQLite hooks:
///
/// 1. **`preupdate_hook`**: Fires before each INSERT/UPDATE/DELETE, captures
///    the change as a `ChangeEvent` into a buffer.
///
/// 2. **`commit_hook`**: Fires just before COMMIT, persists buffered events
///    to the durable event log via VSR consensus. Returns `false` to trigger
///    rollback if persistence fails.
///
/// # Healthcare Compliance Guarantee
///
/// This design ensures **strong consistency** between SQLite and the event log:
/// - Events are persisted to event log **before** SQLite commits
/// - If VSR consensus fails, the SQLite transaction rolls back
/// - No write can exist in SQLite that isn't in the event log
///
/// # Example
///
/// ```ignore
/// use vdb_projections::realtime::TransactionHooks;
/// use std::sync::Arc;
///
/// let hooks = TransactionHooks::new(pool, schema_cache);
/// hooks.install(persister, stream_id).await?;
///
/// // Now any INSERT/UPDATE/DELETE on this connection will:
/// // 1. Capture the change (preupdate_hook)
/// // 2. Persist to event log before commit (commit_hook)
/// // 3. Rollback if persistence fails
/// ```
#[derive(Debug, Clone)]
pub struct TransactionHooks {
    pool: SqlitePool,
    ctx: Arc<HookContext>,
}

impl TransactionHooks {
    /// Creates a new `TransactionHooks` instance.
    ///
    /// This creates the shared [`HookContext`] but does not install the hooks.
    /// Call [`install()`](Self::install) to register hooks on a connection.
    ///
    /// # Arguments
    ///
    /// * `pool` - SQLite connection pool (should be the write pool)
    /// * `schema_cache` - Shared schema cache for column name lookups
    /// * `persister` - Implementation of [`EventPersister`] (typically wraps Runtime)
    /// * `stream_id` - Stream ID for this projection's events
    pub fn new(
        pool: SqlitePool,
        schema_cache: Arc<SchemaCache>,
        persister: Arc<dyn EventPersister>,
        stream_id: StreamId,
    ) -> Self {
        let ctx = HookContext {
            schema_cache,
            buffer: Arc::new(Mutex::new(Vec::new())),
            persister,
            stream_id,
        };
        Self {
            pool,
            ctx: Arc::new(ctx),
        }
    }

    /// Installs both `preupdate_hook` and `commit_hook` on a connection.
    ///
    /// After this call, any write transaction on the acquired connection will:
    /// 1. Capture changes via `preupdate_hook` into the buffer
    /// 2. Persist buffer to event log via `commit_hook` before commit
    /// 3. Rollback if persistence fails (VSR consensus unavailable)
    ///
    /// # Errors
    ///
    /// Returns [`ProjectionError`] if the connection cannot be acquired.
    ///
    /// # Panics
    ///
    /// The `preupdate_hook` will panic if a mutation occurs on a table not in
    /// the schema cache. This indicates the table was created outside VerityDB
    /// migrations, which is a programming error.
    pub async fn install(&self) -> Result<(), ProjectionError> {
        let mut conn = self.pool.acquire().await?;
        let mut handle = conn.lock_handle().await?;

        let ctx_for_preupdate: Arc<HookContext> = Arc::clone(&self.ctx);
        let ctx_for_commit: Arc<HookContext> = Arc::clone(&self.ctx);

        // Spawn the hook
        handle.set_preupdate_hook(move |result: PreupdateHookResult<'_>| {
            let table = result.table;
            let column_count = result.get_column_count();

            let table_name = TableName::from_sqlite(table);
            if table_name.is_internal() {
                return; // skip, don't panic
            }

            let columns = ctx_for_preupdate.schema_cache
                .get_columns(&table_name).unwrap_or_else(|| {
                    panic!(
                    "table '{}' not in schema cache - was it created outside of VerityDB migrations?",
                        table_name
                )
                });

            let change_event = match result.operation {
                SqliteOperation::Insert => {
                    let row_id = RowId::from(result.get_new_row_id().unwrap());
                    let mut values = Vec::with_capacity(column_count as usize);
                    for i in 0..column_count {
                        let value_ref = result.get_new_column_value(i).unwrap();
                        values.push(SqlValue::try_from(value_ref).unwrap());
                    }
                    ChangeEvent::Insert {
                        table_name,
                        row_id,
                        column_names: columns,
                        values,
                    }
                }
                SqliteOperation::Update => {
                    let row_id = RowId::from(result.get_old_row_id().unwrap());
                    let mut old_values = Vec::with_capacity(column_count as usize);
                    let mut new_values = Vec::with_capacity(column_count as usize);
                    for i in 0..column_count {
                        let old_value_ref = result.get_old_column_value(i).unwrap();
                        let new_value_ref = result.get_new_column_value(i).unwrap();
                        old_values.push((
                            columns[i as usize].clone(),
                            SqlValue::try_from(old_value_ref).unwrap(),
                        ));
                        new_values.push((
                            columns[i as usize].clone(),
                            SqlValue::try_from(new_value_ref).unwrap(),
                        ));
                    }
                    ChangeEvent::Update {
                        table_name,
                        row_id,
                        old_values,
                        new_values,
                    }
                }
                SqliteOperation::Delete => {
                    let row_id = RowId::from(result.get_old_row_id().unwrap());
                    let mut deleted_values = Vec::with_capacity(column_count as usize);
                    for i in 0..column_count {
                        let deleted_value_ref = result.get_old_column_value(i).unwrap();
                        deleted_values.push(SqlValue::try_from(deleted_value_ref).unwrap());
                    }

                    ChangeEvent::Delete {
                        table_name,
                        row_id,
                        deleted_values,
                    }
                }
                _ => {
                    return;
                }
            };

            ctx_for_preupdate.buffer.lock().unwrap().push(change_event);
        });

        handle.set_commit_hook(move || {
            let events = std::mem::take(&mut *ctx_for_commit.buffer.lock().unwrap());

            if events.is_empty() {
                return true;
            };

            // Serialize all events - if any fail, it's a bug in VerityDB
            let serialized_events: Result<Vec<Bytes>, _> = events
                .iter()
                .map(|event| serde_json::to_vec(event).map(Bytes::from))
                .collect();

            let serialized_events = match serialized_events {
                Ok(events) => events,
                Err(e) => {
                    // Don't panic in FFI callback - log and rollback safely
                    tracing::error!(error = %e, "BUG: Failed to serialize ChangeEvent");
                    return false;
                }
            };

            ctx_for_commit
                .persister
                .persist_blocking(ctx_for_commit.stream_id, serialized_events)
                .is_ok()
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use vdb_types::{Offset, PersistError};

    /// Mock persister that tracks calls and can be configured to fail.
    #[derive(Debug)]
    struct MockPersister {
        call_count: AtomicUsize,
        should_fail: bool,
        last_events: Mutex<Vec<Vec<u8>>>,
    }

    impl MockPersister {
        fn new(should_fail: bool) -> Self {
            Self {
                call_count: AtomicUsize::new(0),
                should_fail,
                last_events: Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    impl EventPersister for MockPersister {
        fn persist_blocking(
            &self,
            _stream_id: StreamId,
            events: Vec<Bytes>,
        ) -> Result<Offset, PersistError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);

            // Store events for inspection
            let mut last = self.last_events.lock().unwrap();
            *last = events.iter().map(|b| b.to_vec()).collect();

            if self.should_fail {
                Err(PersistError::ConsensusFailed)
            } else {
                Ok(Offset::new(events.len() as i64))
            }
        }
    }

    mod hook_context {
        use super::*;

        #[test]
        fn new_creates_empty_buffer() {
            let schema_cache = Arc::new(SchemaCache::new());
            let persister: Arc<dyn EventPersister> = Arc::new(MockPersister::new(false));

            let ctx = HookContext {
                buffer: Arc::new(Mutex::new(Vec::new())),
                schema_cache,
                persister,
                stream_id: StreamId::new(1),
            };

            assert!(ctx.buffer.lock().unwrap().is_empty());
        }

        #[test]
        fn buffer_can_hold_events() {
            let schema_cache = Arc::new(SchemaCache::new());
            let persister: Arc<dyn EventPersister> = Arc::new(MockPersister::new(false));

            let ctx = HookContext {
                buffer: Arc::new(Mutex::new(Vec::new())),
                schema_cache,
                persister,
                stream_id: StreamId::new(1),
            };

            // Simulate preupdate_hook adding an event
            let event = ChangeEvent::Insert {
                table_name: TableName::from("users".to_string()),
                row_id: RowId::from(1i64),
                column_names: vec![],
                values: vec![],
            };
            ctx.buffer.lock().unwrap().push(event);

            assert_eq!(ctx.buffer.lock().unwrap().len(), 1);
        }

        #[test]
        fn buffer_drain_clears_events() {
            let schema_cache = Arc::new(SchemaCache::new());
            let persister: Arc<dyn EventPersister> = Arc::new(MockPersister::new(false));

            let ctx = HookContext {
                buffer: Arc::new(Mutex::new(Vec::new())),
                schema_cache,
                persister,
                stream_id: StreamId::new(1),
            };

            // Add events
            let event = ChangeEvent::Insert {
                table_name: TableName::from("users".to_string()),
                row_id: RowId::from(1i64),
                column_names: vec![],
                values: vec![],
            };
            ctx.buffer.lock().unwrap().push(event);

            // Drain (simulating commit_hook)
            let events = std::mem::take(&mut *ctx.buffer.lock().unwrap());

            assert_eq!(events.len(), 1);
            assert!(ctx.buffer.lock().unwrap().is_empty());
        }
    }

    mod mock_persister {
        use super::*;

        #[test]
        fn tracks_call_count() {
            let persister = MockPersister::new(false);

            assert_eq!(persister.call_count(), 0);

            persister.persist_blocking(StreamId::new(1), vec![Bytes::from("test")]).unwrap();
            assert_eq!(persister.call_count(), 1);

            persister.persist_blocking(StreamId::new(1), vec![]).unwrap();
            assert_eq!(persister.call_count(), 2);
        }

        #[test]
        fn returns_ok_when_configured() {
            let persister = MockPersister::new(false);
            let result = persister.persist_blocking(StreamId::new(1), vec![Bytes::from("test")]);
            assert!(result.is_ok());
        }

        #[test]
        fn returns_err_when_configured_to_fail() {
            let persister = MockPersister::new(true);
            let result = persister.persist_blocking(StreamId::new(1), vec![Bytes::from("test")]);
            assert!(matches!(result, Err(PersistError::ConsensusFailed)));
        }
    }

    mod serialization {
        use super::*;
        use crate::SqlValue;

        #[test]
        fn change_event_serializes_to_json() {
            let event = ChangeEvent::Insert {
                table_name: TableName::from("patients".to_string()),
                row_id: RowId::from(42i64),
                column_names: vec![
                    "id".to_string().into(),
                    "name".to_string().into(),
                ],
                values: vec![
                    SqlValue::Integer(42),
                    SqlValue::Text("John Doe".to_string()),
                ],
            };

            let json = serde_json::to_vec(&event);
            assert!(json.is_ok());

            // Verify it can be deserialized back
            let bytes = json.unwrap();
            let restored: ChangeEvent = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(event, restored);
        }

        #[test]
        fn all_event_types_serialize() {
            let events = vec![
                ChangeEvent::Insert {
                    table_name: TableName::from("t".to_string()),
                    row_id: RowId::from(1i64),
                    column_names: vec![],
                    values: vec![],
                },
                ChangeEvent::Update {
                    table_name: TableName::from("t".to_string()),
                    row_id: RowId::from(1i64),
                    old_values: vec![],
                    new_values: vec![],
                },
                ChangeEvent::Delete {
                    table_name: TableName::from("t".to_string()),
                    row_id: RowId::from(1i64),
                    deleted_values: vec![],
                },
            ];

            for event in events {
                let result = serde_json::to_vec(&event);
                assert!(result.is_ok(), "Failed to serialize {:?}", event);
            }
        }
    }
}
