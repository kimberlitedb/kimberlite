//! The kernel - pure functional core of `Kimberlite`.
//!
//! The kernel applies committed commands to produce new state and effects.
//! It is completely pure: no IO, no clocks, no randomness. This makes it
//! deterministic and easy to test.
//!
//! # Example
//!
//! ```ignore
//! let state = State::new();
//! let cmd = Command::create_stream(...);
//!
//! let (new_state, effects) = apply_committed(state, cmd)?;
//! // Runtime executes effects...
//! ```

use kmb_types::{AuditAction, DataClass, Offset, Placement, StreamId, StreamMetadata, StreamName};

use crate::command::{Command, TableId};
use crate::effects::Effect;
use crate::state::State;

/// Applies a committed command to the state, producing new state and effects.
///
/// Takes ownership of state, returns new state. No cloning of the `BTreeMap`.
pub fn apply_committed(state: State, cmd: Command) -> Result<(State, Vec<Effect>), KernelError> {
    let mut effects = Vec::new();

    match cmd {
        // ====================================================================
        // Event Stream Commands
        // ====================================================================
        Command::CreateStream {
            stream_id,
            stream_name,
            data_class,
            placement,
        } => {
            if state.stream_exists(&stream_id) {
                return Err(KernelError::StreamIdUniqueConstraint(stream_id));
            }

            // Create metadata
            let meta = StreamMetadata::new(
                stream_id,
                stream_name.clone(),
                data_class,
                placement.clone(),
            );

            // Effects need their own copies of metadata
            effects.push(Effect::StreamMetadataWrite(meta.clone()));
            effects.push(Effect::AuditLogAppend(AuditAction::StreamCreated {
                stream_id,
                stream_name,
                data_class,
                placement,
            }));

            // State takes ownership of meta
            Ok((state.with_stream(meta), effects))
        }

        Command::CreateStreamWithAutoId {
            stream_name,
            data_class,
            placement,
        } => {
            let (state, meta) =
                state.with_new_stream(stream_name.clone(), data_class, placement.clone());

            effects.push(Effect::StreamMetadataWrite(meta.clone()));
            effects.push(Effect::AuditLogAppend(AuditAction::StreamCreated {
                stream_id: meta.stream_id,
                stream_name,
                data_class,
                placement,
            }));

            Ok((state, effects))
        }

        Command::AppendBatch {
            stream_id,
            events,
            expected_offset,
        } => {
            let stream = state
                .get_stream(&stream_id)
                .ok_or(KernelError::StreamNotFound(stream_id))?;

            if stream.current_offset != expected_offset {
                return Err(KernelError::UnexpectedStreamOffset {
                    stream_id,
                    expected: expected_offset,
                    actual: stream.current_offset,
                });
            }

            let event_count = events.len();
            let base_offset = stream.current_offset;
            let new_offset = base_offset + Offset::from(event_count as u64);

            // StorageAppend takes ownership of events (moved, not cloned)
            effects.push(Effect::StorageAppend {
                stream_id,
                base_offset,
                events,
            });

            effects.push(Effect::WakeProjection {
                stream_id,
                from_offset: base_offset,
                to_offset: new_offset,
            });

            effects.push(Effect::AuditLogAppend(AuditAction::EventsAppended {
                stream_id,
                count: event_count as u32,
                from_offset: base_offset,
            }));

            Ok((state.with_updated_offset(stream_id, new_offset), effects))
        }

        // ====================================================================
        // DDL Commands (SQL table management)
        // ====================================================================
        Command::CreateTable {
            table_id,
            table_name,
            columns,
            primary_key,
        } => {
            // Validate table doesn't exist
            if state.table_exists(&table_id) {
                return Err(KernelError::TableIdUniqueConstraint(table_id));
            }

            if state.table_name_exists(&table_name) {
                return Err(KernelError::TableNameUniqueConstraint(table_name));
            }

            // Create underlying stream for table events
            // Convention: table data stored in stream "__table_{name}"
            let stream_name = StreamName::new(format!("__table_{}", table_name));
            let (state, stream_meta) = state.with_new_stream(
                stream_name,
                DataClass::NonPHI, // Default, can be configured per table
                Placement::Global,
            );

            effects.push(Effect::StreamMetadataWrite(stream_meta.clone()));

            // Create table metadata linking to the stream
            let table_meta = crate::state::TableMetadata {
                table_id,
                table_name: table_name.clone(),
                columns: columns.clone(),
                primary_key: primary_key.clone(),
                stream_id: stream_meta.stream_id,
            };

            // Add table to state using with_table_metadata instead
            let state = state.with_table_metadata(table_meta.clone());

            effects.push(Effect::TableMetadataWrite(table_meta));

            // Audit log entry for table creation
            effects.push(Effect::AuditLogAppend(AuditAction::EventsAppended {
                stream_id: stream_meta.stream_id,
                count: 1,
                from_offset: Offset::ZERO,
            }));

            Ok((state, effects))
        }

        Command::DropTable { table_id } => {
            if !state.table_exists(&table_id) {
                return Err(KernelError::TableNotFound(table_id));
            }

            effects.push(Effect::TableMetadataDrop(table_id));

            Ok((state.without_table(table_id), effects))
        }

        Command::CreateIndex {
            index_id,
            table_id,
            index_name,
            columns,
        } => {
            // Validate table exists
            let _table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            // Validate index doesn't exist
            if state.index_exists(&index_id) {
                return Err(KernelError::IndexIdUniqueConstraint(index_id));
            }

            let index_meta = crate::state::IndexMetadata {
                index_id,
                index_name,
                table_id,
                columns,
            };

            effects.push(Effect::IndexMetadataWrite(index_meta.clone()));

            Ok((state.with_index(index_meta), effects))
        }

        // ====================================================================
        // DML Commands (SQL data manipulation)
        // ====================================================================
        Command::Insert { table_id, row_data } => {
            // Get table metadata and its underlying stream
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            let stream_id = table.stream_id;
            let stream = state
                .get_stream(&stream_id)
                .ok_or(KernelError::StreamNotFound(stream_id))?;

            let base_offset = stream.current_offset;
            let new_offset = base_offset + Offset::from(1);

            // Append row as event to table's stream
            effects.push(Effect::StorageAppend {
                stream_id,
                base_offset,
                events: vec![row_data],
            });

            // Trigger projection update for this table
            effects.push(Effect::UpdateProjection {
                table_id,
                from_offset: base_offset,
                to_offset: new_offset,
            });

            effects.push(Effect::AuditLogAppend(AuditAction::EventsAppended {
                stream_id,
                count: 1,
                from_offset: base_offset,
            }));

            Ok((state.with_updated_offset(stream_id, new_offset), effects))
        }

        Command::Update { table_id, row_data } => {
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            let stream_id = table.stream_id;
            let stream = state
                .get_stream(&stream_id)
                .ok_or(KernelError::StreamNotFound(stream_id))?;

            let base_offset = stream.current_offset;
            let new_offset = base_offset + Offset::from(1);

            effects.push(Effect::StorageAppend {
                stream_id,
                base_offset,
                events: vec![row_data],
            });

            effects.push(Effect::UpdateProjection {
                table_id,
                from_offset: base_offset,
                to_offset: new_offset,
            });

            effects.push(Effect::AuditLogAppend(AuditAction::EventsAppended {
                stream_id,
                count: 1,
                from_offset: base_offset,
            }));

            Ok((state.with_updated_offset(stream_id, new_offset), effects))
        }

        Command::Delete { table_id, row_data } => {
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            let stream_id = table.stream_id;
            let stream = state
                .get_stream(&stream_id)
                .ok_or(KernelError::StreamNotFound(stream_id))?;

            let base_offset = stream.current_offset;
            let new_offset = base_offset + Offset::from(1);

            effects.push(Effect::StorageAppend {
                stream_id,
                base_offset,
                events: vec![row_data],
            });

            effects.push(Effect::UpdateProjection {
                table_id,
                from_offset: base_offset,
                to_offset: new_offset,
            });

            effects.push(Effect::AuditLogAppend(AuditAction::EventsAppended {
                stream_id,
                count: 1,
                from_offset: base_offset,
            }));

            Ok((state.with_updated_offset(stream_id, new_offset), effects))
        }
    }
}

/// Errors that can occur when applying commands to the kernel.
#[derive(thiserror::Error, Debug)]
pub enum KernelError {
    // Stream errors
    #[error("stream with id {0} already exists")]
    StreamIdUniqueConstraint(StreamId),

    #[error("stream with id {0} not found")]
    StreamNotFound(StreamId),

    #[error("offset mismatch for stream {stream_id}: expected {expected}, actual {actual}")]
    UnexpectedStreamOffset {
        stream_id: StreamId,
        expected: Offset,
        actual: Offset,
    },

    // Table errors
    #[error("table with id {0} already exists")]
    TableIdUniqueConstraint(TableId),

    #[error("table with name '{0}' already exists")]
    TableNameUniqueConstraint(String),

    #[error("table with id {0} not found")]
    TableNotFound(TableId),

    // Index errors
    #[error("index with id {0} already exists")]
    IndexIdUniqueConstraint(crate::command::IndexId),

    // General errors
    #[error("not implemented: {0}")]
    NotImplemented(String),
}
