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
    AuditAction, DataClass, Offset, Placement, StreamId, StreamMetadata, StreamName, TenantId,
};

use crate::command::{Command, TableId};
use crate::effects::Effect;
use crate::state::State;

/// Stream-name prefix for a table's backing event stream.
///
/// Final stream name is `"{TABLE_STREAM_PREFIX}{tenant_id}_{table_name}"`,
/// making the stream name globally unique even when two tenants create a
/// table with the same user-visible name. The tenant scoping is
/// defense-in-depth; StreamId already encodes tenant in its upper bits.
pub const TABLE_STREAM_PREFIX: &str = "__table_";

/// Applies a committed command to the state, producing new state and effects.
///
/// Takes ownership of state, returns new state. No cloning of the `BTreeMap`.
#[allow(clippy::too_many_lines)]
pub fn apply_committed(state: State, cmd: Command) -> Result<(State, Vec<Effect>), KernelError> {
    // Pre-allocate effects based on command variant to avoid heap reallocations
    let mut effects = Vec::with_capacity(match &cmd {
        Command::CreateStream { .. } | Command::CreateStreamWithAutoId { .. } => 2,
        Command::AppendBatch { .. }
        | Command::CreateTable { .. }
        | Command::Insert { .. }
        | Command::Update { .. }
        | Command::Delete { .. } => 3,
        Command::DropTable { .. } | Command::CreateIndex { .. } => 1,
    });

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
                new_state.get_stream(&stream_id).expect("postcondition: stream must exist after successful mutation").current_offset,
                Offset::ZERO
            );

            // DST: stream creation postconditions
            kimberlite_properties::always!(
                new_state.get_stream(&stream_id).is_some(),
                "kernel.stream_exists_after_create",
                "stream must exist in state after successful CreateStream"
            );
            kimberlite_properties::always!(
                new_state.get_stream(&stream_id).expect("postcondition: stream must exist after creation").current_offset == Offset::ZERO,
                "kernel.stream_zero_offset_after_create",
                "newly created stream must have offset zero"
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
                    .expect("postcondition: auto-id stream must exist after creation")
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

            // DST: offset monotonicity — the foundational append-only invariant
            kimberlite_properties::always!(
                new_offset >= base_offset,
                "kernel.offset_monotonicity",
                "stream offset must never decrease after append"
            );
            kimberlite_properties::always!(
                new_offset == base_offset + Offset::from(event_count as u64),
                "kernel.offset_arithmetic",
                "new offset must equal base + event count"
            );
            // DST: coverage signal — simulation should exercise multi-event batches
            kimberlite_properties::sometimes!(
                event_count > 1,
                "kernel.multi_event_batch",
                "simulation should sometimes append batches with multiple events"
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
                new_state.get_stream(&stream_id).expect("postcondition: stream must exist after successful mutation").current_offset,
                new_offset
            );
            // Postcondition: offset increased by event count
            debug_assert_eq!(
                new_state.get_stream(&stream_id).expect("postcondition: stream must exist after successful mutation").current_offset,
                base_offset + Offset::from(event_count as u64)
            );

            // DST: state consistency after append
            kimberlite_properties::always!(
                new_state.get_stream(&stream_id).expect("postcondition: stream must exist after append").current_offset == new_offset,
                "kernel.append_offset_consistent",
                "state offset must match computed new_offset after append"
            );

            Ok((new_state, effects))
        }

        // ====================================================================
        // DDL Commands (SQL table management)
        // ====================================================================
        Command::CreateTable {
            tenant_id,
            table_id,
            table_name,
            columns,
            primary_key,
        } => {
            // Precondition: table ID must be unique
            if state.table_exists(&table_id) {
                return Err(KernelError::TableIdUniqueConstraint(table_id));
            }

            // Precondition: (tenant, name) pair must be unique. Two
            // different tenants MAY own tables with the same name — the
            // check was global before and silently collapsed their
            // catalogs.
            if state.table_name_exists_for_tenant(tenant_id, &table_name) {
                return Err(KernelError::TableNameUniqueConstraint {
                    tenant_id,
                    table_name,
                });
            }

            // Precondition: columns list must not be empty
            debug_assert!(!columns.is_empty(), "table must have at least one column");

            // Create the table's backing stream. The name embeds tenant_id
            // so two tenants with same-named tables produce distinct stream
            // names. StreamId is already tenant-scoped via bit-packing,
            // this is defense-in-depth + human-readable provenance.
            let stream_name = StreamName::new(format!(
                "{TABLE_STREAM_PREFIX}{tenant}_{table_name}",
                tenant = u64::from(tenant_id)
            ));
            let (new_state, stream_meta) = state.with_new_stream(
                stream_name,
                DataClass::Public, // Default, can be configured per table
                Placement::Global,
            );

            // Postcondition: backing stream was created (production assert;
            // a missing stream here would corrupt every subsequent DML).
            assert!(
                new_state.stream_exists(&stream_meta.stream_id),
                "postcondition: backing stream missing after with_new_stream"
            );

            effects.push(Effect::StreamMetadataWrite(stream_meta.clone()));

            // Create table metadata linking to the stream.
            let table_meta = crate::state::TableMetadata {
                tenant_id,
                table_id,
                table_name: table_name.clone(),
                columns: columns.clone(),
                primary_key: primary_key.clone(),
                stream_id: stream_meta.stream_id,
            };

            // Postcondition: table metadata references the backing stream
            debug_assert_eq!(table_meta.stream_id, stream_meta.stream_id);
            // Postcondition: metadata carries the command's tenant
            debug_assert_eq!(table_meta.tenant_id, tenant_id);

            // Add table to state using with_table_metadata
            let final_state = new_state.with_table_metadata(table_meta.clone());

            // Postcondition: table now exists in state
            assert!(
                final_state.table_exists(&table_id),
                "postcondition: table registration failed silently"
            );

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

        Command::DropTable {
            tenant_id,
            table_id,
        } => {
            // Precondition: table must exist
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            // Precondition: caller must own the table.
            ensure_tenant_owns_table(tenant_id, table_id, table.tenant_id)?;

            effects.push(Effect::TableMetadataDrop {
                tenant_id,
                table_id,
            });

            // Postcondition: exactly 1 effect (drop metadata)
            debug_assert_eq!(effects.len(), 1);

            let new_state = state.without_table(table_id);

            // Postcondition: table no longer exists
            debug_assert!(!new_state.table_exists(&table_id));

            Ok((new_state, effects))
        }

        Command::CreateIndex {
            tenant_id,
            index_id,
            table_id,
            index_name,
            columns,
        } => {
            // Precondition: table must exist
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            // Precondition: caller must own the table.
            ensure_tenant_owns_table(tenant_id, table_id, table.tenant_id)?;

            // Precondition: index ID must be unique
            if state.index_exists(&index_id) {
                return Err(KernelError::IndexIdUniqueConstraint(index_id));
            }

            // Precondition: indexed columns must not be empty
            debug_assert!(!columns.is_empty(), "index must cover at least one column");

            let index_meta = crate::state::IndexMetadata {
                tenant_id,
                index_id,
                index_name,
                table_id,
                columns,
            };

            // Postcondition: index metadata references correct table and tenant
            debug_assert_eq!(index_meta.table_id, table_id);
            debug_assert_eq!(index_meta.tenant_id, tenant_id);

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
        Command::Insert {
            tenant_id,
            table_id,
            row_data,
        } => {
            // Precondition: table must exist
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            // Precondition: caller must own the table. Cross-tenant writes
            // are a compliance-grade violation — the surrounding code must
            // never submit one; if it does, this is a NEVER property.
            ensure_tenant_owns_table(tenant_id, table_id, table.tenant_id)?;

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
                tenant_id,
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
                new_state.get_stream(&stream_id).expect("postcondition: stream must exist after successful mutation").current_offset,
                new_offset
            );

            Ok((new_state, effects))
        }

        Command::Update {
            tenant_id,
            table_id,
            row_data,
        } => {
            // Precondition: table must exist
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            // Precondition: caller must own the table.
            ensure_tenant_owns_table(tenant_id, table_id, table.tenant_id)?;

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
                tenant_id,
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
                new_state.get_stream(&stream_id).expect("postcondition: stream must exist after successful mutation").current_offset,
                new_offset
            );

            Ok((new_state, effects))
        }

        Command::Delete {
            tenant_id,
            table_id,
            row_data,
        } => {
            // Precondition: table must exist
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            // Precondition: caller must own the table.
            ensure_tenant_owns_table(tenant_id, table_id, table.tenant_id)?;

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
                tenant_id,
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
                new_state.get_stream(&stream_id).expect("postcondition: stream must exist after successful mutation").current_offset,
                new_offset
            );

            Ok((new_state, effects))
        }
    }
}

/// Enforces that the caller's `tenant_id` matches the table's owning tenant.
///
/// This is a NEVER property: the surrounding code must never construct a
/// DDL/DML command that targets another tenant's table. If it does, we
/// return an error *and* panic in debug — the debug panic captures stack
/// context for `.kmb` replay, the error path keeps production safe.
#[inline]
fn ensure_tenant_owns_table(
    cmd_tenant_id: TenantId,
    table_id: TableId,
    table_tenant_id: TenantId,
) -> Result<(), KernelError> {
    if cmd_tenant_id != table_tenant_id {
        debug_assert!(
            false,
            "cross-tenant table access: table {table_id} owned by {table_tenant_id}, command from {cmd_tenant_id}",
        );
        return Err(KernelError::CrossTenantTableAccess {
            table_id,
            expected_tenant: table_tenant_id,
            actual_tenant: cmd_tenant_id,
        });
    }
    Ok(())
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

    #[error("table with name '{table_name}' already exists in tenant {tenant_id}")]
    TableNameUniqueConstraint {
        tenant_id: TenantId,
        table_name: String,
    },

    #[error("table with id {0} not found")]
    TableNotFound(TableId),

    /// A DDL/DML command targeted a table owned by a different tenant.
    ///
    /// This is a **compliance-grade** violation — the caller holds an
    /// authenticated identity for `actual_tenant` but tried to write to a
    /// table owned by `expected_tenant`. Surfaces to audit as a breach
    /// indicator, not a client-error "not found".
    #[error(
        "cross-tenant table access: table {table_id} owned by tenant {expected_tenant}, \
         command came from tenant {actual_tenant}"
    )]
    CrossTenantTableAccess {
        table_id: TableId,
        expected_tenant: TenantId,
        actual_tenant: TenantId,
    },

    // Index errors
    #[error("index with id {0} already exists")]
    IndexIdUniqueConstraint(crate::command::IndexId),

    // General errors
    #[error("not implemented: {0}")]
    NotImplemented(String),
}

/// Applies a batch of committed commands, accumulating state changes and effects.
///
/// This is more efficient than calling [`apply_committed`] in a loop because:
/// - Effects vector is pre-allocated based on total command count
/// - State transitions are chained without intermediate allocations
///
/// On error, returns the error and all effects produced by successfully applied
/// commands before the failure (for partial rollback).
pub fn apply_committed_batch(
    state: State,
    commands: Vec<Command>,
) -> Result<(State, Vec<Effect>), KernelError> {
    // Pre-allocate for ~3 effects per command (the common case for DML/append)
    let commands_len = commands.len();
    let mut all_effects = Vec::with_capacity(commands_len * 3);
    let mut current_state = state;

    for cmd in commands {
        let (new_state, effects) = apply_committed(current_state, cmd)?;
        all_effects.extend(effects);
        current_state = new_state;
    }

    // DST: batch processing always produces at least one effect per command
    kimberlite_properties::always!(
        all_effects.len() >= commands_len,
        "kernel.batch_min_effects",
        "batch processing must produce at least one effect per command"
    );

    Ok((current_state, all_effects))
}
