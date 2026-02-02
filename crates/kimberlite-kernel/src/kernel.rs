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

use kimberlite_types::{
    AuditAction, DataClass, Offset, Placement, StreamId, StreamMetadata, StreamName,
};

use crate::command::{Command, TableId};
use crate::effects::Effect;
use crate::state::State;

/// Applies a committed command to the state, producing new state and effects.
///
/// Takes ownership of state, returns new state. No cloning of the `BTreeMap`.
#[allow(clippy::too_many_lines)]
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
            // Precondition: stream doesn't exist yet
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

            // Postcondition: metadata has correct stream_id
            assert_eq!(
                meta.stream_id, stream_id,
                "metadata stream_id mismatch: expected {:?}, got {:?}",
                stream_id, meta.stream_id
            );

            // Effects need their own copies of metadata
            effects.push(Effect::StreamMetadataWrite(meta.clone()));
            effects.push(Effect::AuditLogAppend(AuditAction::StreamCreated {
                stream_id,
                stream_name,
                data_class,
                placement,
            }));

            // Postcondition: exactly 2 effects (metadata write + audit)
            assert_eq!(
                effects.len(),
                2,
                "CreateStream must produce exactly 2 effects for audit completeness, got {}",
                effects.len()
            );

            let new_state = state.with_stream(meta.clone());

            // Postcondition: stream now exists in new state
            assert!(
                new_state.stream_exists(&stream_id),
                "stream {stream_id:?} must exist after creation"
            );
            // Postcondition: stream has zero offset initially
            debug_assert_eq!(
                new_state.get_stream(&stream_id).unwrap().current_offset,
                Offset::ZERO
            );

            Ok((new_state, effects))
        }

        Command::CreateStreamWithAutoId {
            stream_name,
            data_class,
            placement,
        } => {
            let (new_state, meta) =
                state.with_new_stream(stream_name.clone(), data_class, placement.clone());

            // Postcondition: auto-generated stream exists
            debug_assert!(new_state.stream_exists(&meta.stream_id));
            // Postcondition: initial offset is zero
            debug_assert_eq!(
                new_state
                    .get_stream(&meta.stream_id)
                    .unwrap()
                    .current_offset,
                Offset::ZERO
            );

            effects.push(Effect::StreamMetadataWrite(meta.clone()));
            effects.push(Effect::AuditLogAppend(AuditAction::StreamCreated {
                stream_id: meta.stream_id,
                stream_name,
                data_class,
                placement,
            }));

            // Postcondition: exactly 2 effects
            debug_assert_eq!(effects.len(), 2);

            Ok((new_state, effects))
        }

        Command::AppendBatch {
            stream_id,
            events,
            expected_offset,
        } => {
            // Precondition: stream must exist
            let stream = state
                .get_stream(&stream_id)
                .ok_or(KernelError::StreamNotFound(stream_id))?;

            // Precondition: expected offset must match current offset (optimistic concurrency)
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

            // Invariant: offset never decreases (append-only guarantee)
            assert!(
                new_offset >= base_offset,
                "offset must never decrease: base={}, new={}",
                base_offset.as_u64(),
                new_offset.as_u64()
            );
            // Invariant: new offset = base + count
            assert_eq!(
                new_offset,
                base_offset + Offset::from(event_count as u64),
                "offset arithmetic error: base={}, count={}, expected={}, got={}",
                base_offset.as_u64(),
                event_count,
                (base_offset + Offset::from(event_count as u64)).as_u64(),
                new_offset.as_u64()
            );

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

            // Postcondition: exactly 3 effects (storage + projection + audit)
            assert_eq!(
                effects.len(),
                3,
                "AppendEvents must produce exactly 3 effects for audit completeness, got {}",
                effects.len()
            );

            let new_state = state.with_updated_offset(stream_id, new_offset);

            // Postcondition: offset advanced correctly
            debug_assert_eq!(
                new_state.get_stream(&stream_id).unwrap().current_offset,
                new_offset
            );
            // Postcondition: offset increased by event count
            debug_assert_eq!(
                new_state.get_stream(&stream_id).unwrap().current_offset,
                base_offset + Offset::from(event_count as u64)
            );

            Ok((new_state, effects))
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
            // Precondition: table ID must be unique
            if state.table_exists(&table_id) {
                return Err(KernelError::TableIdUniqueConstraint(table_id));
            }

            // Precondition: table name must be unique
            if state.table_name_exists(&table_name) {
                return Err(KernelError::TableNameUniqueConstraint(table_name));
            }

            // Precondition: columns list must not be empty
            debug_assert!(!columns.is_empty(), "table must have at least one column");

            // Create underlying stream for table events
            // Convention: table data stored in stream "__table_{name}"
            let stream_name = StreamName::new(format!("__table_{table_name}"));
            let (new_state, stream_meta) = state.with_new_stream(
                stream_name,
                DataClass::NonPHI, // Default, can be configured per table
                Placement::Global,
            );

            // Postcondition: backing stream was created
            debug_assert!(new_state.stream_exists(&stream_meta.stream_id));

            effects.push(Effect::StreamMetadataWrite(stream_meta.clone()));

            // Create table metadata linking to the stream
            let table_meta = crate::state::TableMetadata {
                table_id,
                table_name: table_name.clone(),
                columns: columns.clone(),
                primary_key: primary_key.clone(),
                stream_id: stream_meta.stream_id,
            };

            // Postcondition: table metadata references the backing stream
            debug_assert_eq!(table_meta.stream_id, stream_meta.stream_id);

            // Add table to state using with_table_metadata instead
            let final_state = new_state.with_table_metadata(table_meta.clone());

            // Postcondition: table now exists in state
            debug_assert!(final_state.table_exists(&table_id));

            effects.push(Effect::TableMetadataWrite(table_meta));

            // Audit log entry for table creation
            effects.push(Effect::AuditLogAppend(AuditAction::EventsAppended {
                stream_id: stream_meta.stream_id,
                count: 1,
                from_offset: Offset::ZERO,
            }));

            // Postcondition: exactly 3 effects (stream metadata + table metadata + audit)
            debug_assert_eq!(effects.len(), 3);

            Ok((final_state, effects))
        }

        Command::DropTable { table_id } => {
            // Precondition: table must exist
            if !state.table_exists(&table_id) {
                return Err(KernelError::TableNotFound(table_id));
            }

            effects.push(Effect::TableMetadataDrop(table_id));

            // Postcondition: exactly 1 effect (drop metadata)
            debug_assert_eq!(effects.len(), 1);

            let new_state = state.without_table(table_id);

            // Postcondition: table no longer exists
            debug_assert!(!new_state.table_exists(&table_id));

            Ok((new_state, effects))
        }

        Command::CreateIndex {
            index_id,
            table_id,
            index_name,
            columns,
        } => {
            // Precondition: table must exist
            let _table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            // Precondition: index ID must be unique
            if state.index_exists(&index_id) {
                return Err(KernelError::IndexIdUniqueConstraint(index_id));
            }

            // Precondition: indexed columns must not be empty
            debug_assert!(!columns.is_empty(), "index must cover at least one column");

            let index_meta = crate::state::IndexMetadata {
                index_id,
                index_name,
                table_id,
                columns,
            };

            // Postcondition: index metadata references correct table
            debug_assert_eq!(index_meta.table_id, table_id);

            effects.push(Effect::IndexMetadataWrite(index_meta.clone()));

            // Postcondition: exactly 1 effect (index metadata write)
            debug_assert_eq!(effects.len(), 1);

            let new_state = state.with_index(index_meta);

            // Postcondition: index now exists
            debug_assert!(new_state.index_exists(&index_id));

            Ok((new_state, effects))
        }

        // ====================================================================
        // DML Commands (SQL data manipulation)
        // ====================================================================
        Command::Insert { table_id, row_data } => {
            // Precondition: table must exist
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            let stream_id = table.stream_id;

            // Precondition: backing stream must exist
            let stream = state
                .get_stream(&stream_id)
                .ok_or(KernelError::StreamNotFound(stream_id))?;

            let base_offset = stream.current_offset;
            let new_offset = base_offset + Offset::from(1);

            // Invariant: offset monotonically increases
            debug_assert!(new_offset > base_offset);
            // Invariant: single row insert increments offset by 1
            debug_assert_eq!(new_offset, base_offset + Offset::from(1));

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

            // Postcondition: exactly 3 effects (storage + projection + audit)
            debug_assert_eq!(effects.len(), 3);

            let new_state = state.with_updated_offset(stream_id, new_offset);

            // Postcondition: stream offset advanced by 1
            debug_assert_eq!(
                new_state.get_stream(&stream_id).unwrap().current_offset,
                new_offset
            );

            Ok((new_state, effects))
        }

        Command::Update { table_id, row_data } => {
            // Precondition: table must exist
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            let stream_id = table.stream_id;

            // Precondition: backing stream must exist
            let stream = state
                .get_stream(&stream_id)
                .ok_or(KernelError::StreamNotFound(stream_id))?;

            let base_offset = stream.current_offset;
            let new_offset = base_offset + Offset::from(1);

            // Invariant: offset monotonically increases
            debug_assert!(new_offset > base_offset);

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

            // Postcondition: exactly 3 effects
            debug_assert_eq!(effects.len(), 3);

            let new_state = state.with_updated_offset(stream_id, new_offset);

            // Postcondition: offset advanced correctly
            debug_assert_eq!(
                new_state.get_stream(&stream_id).unwrap().current_offset,
                new_offset
            );

            Ok((new_state, effects))
        }

        Command::Delete { table_id, row_data } => {
            // Precondition: table must exist
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            let stream_id = table.stream_id;

            // Precondition: backing stream must exist
            let stream = state
                .get_stream(&stream_id)
                .ok_or(KernelError::StreamNotFound(stream_id))?;

            let base_offset = stream.current_offset;
            let new_offset = base_offset + Offset::from(1);

            // Invariant: offset monotonically increases (delete is append-only)
            debug_assert!(new_offset > base_offset);

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

            // Postcondition: exactly 3 effects
            debug_assert_eq!(effects.len(), 3);

            let new_state = state.with_updated_offset(stream_id, new_offset);

            // Postcondition: offset advanced correctly
            debug_assert_eq!(
                new_state.get_stream(&stream_id).unwrap().current_offset,
                new_offset
            );

            Ok((new_state, effects))
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
