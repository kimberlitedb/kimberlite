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
    UpsertResolution,
};

use crate::command::{Command, TableId};
use crate::effects::Effect;
use crate::masking::MaskingPolicyRecord;
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
    // Pre-allocate effects based on command variant to avoid heap reallocations.
    // Arms are kept separated by intent (audit vs metadata vs data-plane) even
    // though they collapse to the same count — clippy's match_same_arms is
    // silenced because the grouping is documentation, not duplication.
    #[allow(clippy::match_same_arms)]
    let mut effects = Vec::with_capacity(match &cmd {
        Command::CreateStream { .. } | Command::CreateStreamWithAutoId { .. } => 2,
        Command::AppendBatch { .. }
        | Command::CreateTable { .. }
        | Command::Insert { .. }
        | Command::Update { .. }
        | Command::Delete { .. } => 3,
        // Upsert: Inserted/Updated emit the same 3-effect shape as DML.
        // NoOp emits only the audit entry; we over-allocate by 2 in that
        // path rather than branch twice — Vec drops the slack on return.
        Command::Upsert { .. } => 3,
        Command::DropTable { .. } | Command::CreateIndex { .. } => 1,
        // AlterTable*: 1 metadata rewrite + 1 audit-log entry.
        Command::AlterTableAddColumn { .. } | Command::AlterTableDropColumn { .. } => 2,
        // AUDIT-2026-04 H-5: Seal/Unseal emit exactly one audit effect.
        Command::SealTenant { .. } | Command::UnsealTenant { .. } => 1,
        // v0.6.0 Tier 2 #7: masking policy CRUD — 1 durable write + 1 audit.
        Command::CreateMaskingPolicy { .. }
        | Command::AttachMaskingPolicy { .. }
        | Command::DetachMaskingPolicy { .. }
        | Command::DropMaskingPolicy { .. } => 2,
    });

    // AUDIT-2026-04 H-5: every mutating command must be rejected if
    // its tenant is sealed. Reads are unaffected. This is a pure
    // state lookup — no I/O, no cloning — and precedes table lookups
    // so "TenantSealed" is reported instead of a confusing
    // "TableNotFound" when the sealed tenant's table is still around.
    if let Some(cmd_tenant) = mutating_tenant_id(&cmd) {
        if state.is_tenant_sealed(cmd_tenant) {
            return Err(KernelError::TenantSealed {
                tenant_id: cmd_tenant,
            });
        }
    }

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
                new_state
                    .get_stream(&stream_id)
                    .expect("postcondition: stream must exist after successful mutation")
                    .current_offset,
                Offset::ZERO
            );

            // DST: stream creation postconditions
            kimberlite_properties::always!(
                new_state.get_stream(&stream_id).is_some(),
                "kernel.stream_exists_after_create",
                "stream must exist in state after successful CreateStream"
            );
            kimberlite_properties::always!(
                new_state
                    .get_stream(&stream_id)
                    .expect("postcondition: stream must exist after creation")
                    .current_offset
                    == Offset::ZERO,
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
                new_state
                    .get_stream(&stream_id)
                    .expect("postcondition: stream must exist after successful mutation")
                    .current_offset,
                new_offset
            );
            // Postcondition: offset increased by event count
            debug_assert_eq!(
                new_state
                    .get_stream(&stream_id)
                    .expect("postcondition: stream must exist after successful mutation")
                    .current_offset,
                base_offset + Offset::from(event_count as u64)
            );

            // DST: state consistency after append
            kimberlite_properties::always!(
                new_state
                    .get_stream(&stream_id)
                    .expect("postcondition: stream must exist after append")
                    .current_offset
                    == new_offset,
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
            // Schema version starts at 1 (pressurecraft invariant: every
            // live TableMetadata has schema_version >= 1; every AlterTable*
            // increments strictly).
            let table_meta = crate::state::TableMetadata {
                tenant_id,
                table_id,
                table_name: table_name.clone(),
                columns: columns.clone(),
                primary_key: primary_key.clone(),
                stream_id: stream_meta.stream_id,
                schema_version: crate::state::TableMetadata::initial_schema_version(),
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

            // AUDIT-2026-04 S2.9 — production postcondition for the
            // exact invariant the April 2026 projection-table bug
            // violated. The name index MUST be keyed by
            // (tenant_id, table_name) and the entry for this call
            // MUST map to our new table_id. A regression to a
            // globally-keyed index would collapse two tenants'
            // same-named tables, and this assertion would fire.
            assert!(
                final_state
                    .table_name_index()
                    .get(&(tenant_id, table_name.clone()))
                    == Some(&table_id),
                "postcondition: (tenant_id, table_name) must resolve to this table's id"
            );
            // Postcondition: the new table's metadata carries our tenant.
            // Defends against a state-layer bug that silently drops
            // the tenant when indexing.
            assert!(
                final_state
                    .get_table(&table_id)
                    .is_some_and(|t| t.tenant_id == tenant_id),
                "postcondition: stored table metadata must carry the command's tenant_id"
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

            // Capture the name BEFORE consuming `state` — needed by
            // the postcondition below.
            let table_name = table.table_name.clone();

            effects.push(Effect::TableMetadataDrop {
                tenant_id,
                table_id,
            });

            // Postcondition: exactly 1 effect (drop metadata)
            debug_assert_eq!(effects.len(), 1);

            let new_state = state.without_table(table_id);

            // Postcondition: table no longer exists
            debug_assert!(!new_state.table_exists(&table_id));

            // AUDIT-2026-04 S2.9 — production postcondition: the
            // (tenant, name) entry in the name index is gone too.
            // A bug that removed only the id→metadata mapping but
            // left the name index pointing at the defunct id would
            // make subsequent `CreateTable(same_name)` fail the
            // uniqueness check despite the table being dropped.
            assert!(
                !new_state
                    .table_name_index()
                    .contains_key(&(tenant_id, table_name)),
                "postcondition: (tenant_id, name) must be cleared after DropTable"
            );

            Ok((new_state, effects))
        }

        // ----------------------------------------------------------------
        // ALTER TABLE — ADD COLUMN (ROADMAP v0.5.0 item B).
        // ----------------------------------------------------------------
        //
        // Schema evolution is append-only at the storage layer: we do NOT
        // rewrite existing rows. The planner materialises NULLs for the
        // new column when reading rows persisted before the alter. This
        // preserves the log's immutability and keeps the change
        // deterministic under VSR replay.
        Command::AlterTableAddColumn {
            tenant_id,
            table_id,
            column,
        } => {
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;
            ensure_tenant_owns_table(tenant_id, table_id, table.tenant_id)?;

            // Precondition: column name must not already exist on the table.
            // Uniqueness is name-case-sensitive to mirror the SQL catalog.
            if table.columns.iter().any(|c| c.name == column.name) {
                return Err(KernelError::ColumnAlreadyExists {
                    table_id,
                    column_name: column.name,
                });
            }

            let prior_version = table.schema_version;
            let prior_column_count = table.columns.len();

            // Build the new metadata with the column appended and the
            // schema_version bumped by exactly 1.
            let mut new_meta = table.clone();
            new_meta.columns.push(column.clone());
            new_meta.schema_version = prior_version
                .checked_add(1)
                .expect("schema_version overflow: more than u32::MAX ALTER TABLEs on one table");

            // Production-grade monotonicity check.
            assert!(
                new_meta.schema_version > prior_version,
                "schema_version must be strictly increasing (was {prior_version}, now {})",
                new_meta.schema_version,
            );
            // Production-grade column-count invariant.
            assert_eq!(
                new_meta.columns.len(),
                prior_column_count + 1,
                "ADD COLUMN must grow columns by exactly one",
            );

            effects.push(Effect::TableMetadataWrite(new_meta.clone()));
            effects.push(Effect::AuditLogAppend(AuditAction::EventsAppended {
                stream_id: table.stream_id,
                count: 1,
                from_offset: Offset::ZERO,
            }));
            debug_assert_eq!(effects.len(), 2);

            let new_state = state.with_table_metadata(new_meta);

            // Postcondition: the freshly-stored metadata reflects what we
            // just wrote. Guards against a state-layer bug that silently
            // drops the version bump.
            assert!(
                new_state
                    .get_table(&table_id)
                    .is_some_and(|t| t.schema_version == prior_version + 1
                        && t.columns.len() == prior_column_count + 1),
                "postcondition: ALTER TABLE ADD COLUMN did not persist schema_version/column-count",
            );

            Ok((new_state, effects))
        }

        // ----------------------------------------------------------------
        // ALTER TABLE — DROP COLUMN (ROADMAP v0.5.0 item B).
        // ----------------------------------------------------------------
        //
        // Same append-only semantics as ADD COLUMN. Dropping a primary-key
        // column is rejected structurally — allowing it would orphan
        // every persisted key.
        Command::AlterTableDropColumn {
            tenant_id,
            table_id,
            column_name,
        } => {
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;
            ensure_tenant_owns_table(tenant_id, table_id, table.tenant_id)?;

            // Precondition: column must exist on the table.
            if !table.columns.iter().any(|c| c.name == column_name) {
                return Err(KernelError::ColumnNotFound {
                    table_id,
                    column_name,
                });
            }

            // Precondition: column must NOT be part of the primary key.
            if table.primary_key.iter().any(|pk| pk == &column_name) {
                return Err(KernelError::CannotDropPrimaryKeyColumn {
                    table_id,
                    column_name,
                });
            }

            let prior_version = table.schema_version;
            let prior_column_count = table.columns.len();

            let mut new_meta = table.clone();
            new_meta.columns.retain(|c| c.name != column_name);
            new_meta.schema_version = prior_version
                .checked_add(1)
                .expect("schema_version overflow: more than u32::MAX ALTER TABLEs on one table");

            assert!(
                new_meta.schema_version > prior_version,
                "schema_version must be strictly increasing (was {prior_version}, now {})",
                new_meta.schema_version,
            );
            assert_eq!(
                new_meta.columns.len(),
                prior_column_count - 1,
                "DROP COLUMN must shrink columns by exactly one",
            );

            effects.push(Effect::TableMetadataWrite(new_meta.clone()));
            effects.push(Effect::AuditLogAppend(AuditAction::EventsAppended {
                stream_id: table.stream_id,
                count: 1,
                from_offset: Offset::ZERO,
            }));
            debug_assert_eq!(effects.len(), 2);

            let new_state = state.with_table_metadata(new_meta);

            assert!(
                new_state
                    .get_table(&table_id)
                    .is_some_and(|t| t.schema_version == prior_version + 1
                        && t.columns.len() == prior_column_count - 1
                        && !t.columns.iter().any(|c| c.name == column_name)),
                "postcondition: ALTER TABLE DROP COLUMN did not persist removal",
            );

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

            // AUDIT-2026-04 S2.9 — production postcondition for the
            // per-tenant binding of stored index metadata. If
            // `state.with_index` silently dropped the tenant_id (or
            // if a refactor keyed the index store globally), this
            // check fires. Mirror of the CreateTable invariant.
            assert!(
                new_state
                    .get_index(&index_id)
                    .is_some_and(|m| m.tenant_id == tenant_id && m.table_id == table_id),
                "postcondition: stored index metadata must carry the command's (tenant_id, table_id)"
            );

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
                new_state
                    .get_stream(&stream_id)
                    .expect("postcondition: stream must exist after successful mutation")
                    .current_offset,
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
                new_state
                    .get_stream(&stream_id)
                    .expect("postcondition: stream must exist after successful mutation")
                    .current_offset,
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
                new_state
                    .get_stream(&stream_id)
                    .expect("postcondition: stream must exist after successful mutation")
                    .current_offset,
                new_offset
            );

            Ok((new_state, effects))
        }

        // ====================================================================
        // UPSERT (v0.6.0 Tier 1 #3) — `INSERT ... ON CONFLICT`
        // ====================================================================
        //
        // One command, one resolution, one event. The old notebar helper
        // did UPDATE + conditional INSERT (two round-trips, a visible
        // dual-write window). The kernel collapses that to a single
        // atomic decision driven by the `conflict_exists` flag the
        // shell computed via a point-lookup on the projection store.
        //
        // The resolution discriminator (Inserted | Updated | NoOp) is
        // embedded in the AuditAction payload, so replaying the log
        // never has to infer intent from adjacent events.
        Command::Upsert {
            tenant_id,
            table_id,
            row_data,
            conflict_exists,
            do_nothing,
        } => {
            // Precondition: table must exist.
            let table = state
                .get_table(&table_id)
                .ok_or(KernelError::TableNotFound(table_id))?;

            // Precondition: caller must own the table. Cross-tenant
            // writes are a compliance-grade violation — identical
            // guard to Insert/Update/Delete.
            ensure_tenant_owns_table(tenant_id, table_id, table.tenant_id)?;

            let stream_id = table.stream_id;

            let stream = state
                .get_stream(&stream_id)
                .ok_or(KernelError::StreamNotFound(stream_id))?;

            // Deterministic resolution: the (conflict_exists, do_nothing)
            // pair is total; every upsert maps to exactly one variant.
            let resolution = match (conflict_exists, do_nothing) {
                (false, _) => UpsertResolution::Inserted,
                (true, false) => UpsertResolution::Updated,
                (true, true) => UpsertResolution::NoOp,
            };

            // Production assertion — this is the "#[should_panic]"
            // invariant the task calls out: every UpsertApplied event
            // MUST carry one of the three resolution variants. If an
            // intermediate refactor were to introduce a fourth state
            // (e.g. `Pending`) without updating this match, the
            // total-match above would fail at compile time; the
            // assertion here defends the runtime boundary in case the
            // command payload were constructed via `Default` or
            // deserialised from a forward-incompatible wire.
            assert!(
                matches!(
                    resolution,
                    UpsertResolution::Inserted
                        | UpsertResolution::Updated
                        | UpsertResolution::NoOp
                ),
                "upsert resolution discriminator missing — every UpsertApplied event \
                 must carry Inserted | Updated | NoOp, got {resolution:?}",
            );

            let base_offset = stream.current_offset;

            match resolution {
                UpsertResolution::Inserted | UpsertResolution::Updated => {
                    let new_offset = base_offset + Offset::from(1);

                    // Invariant: offset strictly increases on every
                    // mutating upsert branch.
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
                    effects.push(Effect::AuditLogAppend(AuditAction::UpsertApplied {
                        stream_id,
                        resolution,
                        from_offset: base_offset,
                    }));

                    // Postcondition: exactly 3 effects on the mutating
                    // branches — the same shape as Insert/Update/Delete.
                    debug_assert_eq!(effects.len(), 3);

                    // DST: offset monotonicity extends to the upsert path.
                    kimberlite_properties::always!(
                        new_offset > base_offset,
                        "kernel.upsert_offset_monotonicity",
                        "upsert's storage append must increment stream offset"
                    );
                    kimberlite_properties::sometimes!(
                        matches!(resolution, UpsertResolution::Updated),
                        "kernel.upsert_updated_branch",
                        "simulation should sometimes hit the Updated resolution"
                    );

                    let new_state = state.with_updated_offset(stream_id, new_offset);
                    Ok((new_state, effects))
                }
                UpsertResolution::NoOp => {
                    // DO NOTHING branch: no storage append, no
                    // projection update. Only the audit record proves
                    // the upsert was applied. This preserves the
                    // append-only log property (we never rewrite or
                    // skip a position) and is idempotent under retry.
                    effects.push(Effect::AuditLogAppend(AuditAction::UpsertApplied {
                        stream_id,
                        resolution,
                        from_offset: base_offset,
                    }));
                    debug_assert_eq!(effects.len(), 1);

                    kimberlite_properties::sometimes!(
                        true,
                        "kernel.upsert_noop_branch",
                        "simulation should sometimes hit the DO NOTHING no-op branch"
                    );

                    Ok((state, effects))
                }
            }
        }

        // ====================================================================
        // Tenant Lifecycle (AUDIT-2026-04 H-5)
        // ====================================================================
        Command::SealTenant {
            tenant_id,
            reason,
            sealed_at_ns,
        } => {
            // Precondition: not already sealed (idempotence via
            // explicit error so double-seals are visible in audit).
            if state.is_tenant_sealed(tenant_id) {
                return Err(KernelError::TenantAlreadySealed { tenant_id });
            }

            let new_state = state.with_sealed_tenant(tenant_id, reason, sealed_at_ns);

            // Postcondition: the tenant is now sealed.
            assert!(
                new_state.is_tenant_sealed(tenant_id),
                "postcondition: tenant must be sealed after SealTenant",
            );

            effects.push(Effect::AuditLogAppend(AuditAction::TenantSealed {
                tenant_id,
                reason,
            }));

            // Postcondition: exactly 1 effect (audit).
            debug_assert_eq!(effects.len(), 1);

            Ok((new_state, effects))
        }

        Command::UnsealTenant { tenant_id } => {
            // Precondition: must actually be sealed — unseal without
            // seal is a no-op, but we surface it as a distinct error
            // so ledger reconstructions can spot corrupt input.
            if !state.is_tenant_sealed(tenant_id) {
                return Err(KernelError::TenantNotSealed { tenant_id });
            }

            let new_state = state.with_unsealed_tenant(tenant_id);

            assert!(
                !new_state.is_tenant_sealed(tenant_id),
                "postcondition: tenant must be unsealed after UnsealTenant",
            );

            effects.push(Effect::AuditLogAppend(AuditAction::TenantUnsealed {
                tenant_id,
            }));

            debug_assert_eq!(effects.len(), 1);

            Ok((new_state, effects))
        }

        // ====================================================================
        // Masking Policy Commands (v0.6.0 Tier 2 #7)
        // ====================================================================
        Command::CreateMaskingPolicy {
            tenant_id,
            name,
            strategy,
            role_guard,
        } => apply_create_masking_policy(state, tenant_id, name, strategy, role_guard, effects),

        Command::AttachMaskingPolicy {
            tenant_id,
            table_id,
            column_name,
            policy_name,
        } => apply_attach_masking_policy(
            state,
            tenant_id,
            table_id,
            column_name,
            policy_name,
            effects,
        ),

        Command::DetachMaskingPolicy {
            tenant_id,
            table_id,
            column_name,
        } => apply_detach_masking_policy(state, tenant_id, table_id, column_name, effects),

        Command::DropMaskingPolicy { tenant_id, name } => {
            apply_drop_masking_policy(state, tenant_id, name, effects)
        }
    }
}

// ----------------------------------------------------------------
// Masking policy handlers — extracted to keep `apply_committed`
// within the too-many-lines lint budget.
// ----------------------------------------------------------------

fn apply_create_masking_policy(
    state: State,
    tenant_id: TenantId,
    name: String,
    strategy: crate::masking::MaskingStrategyKind,
    role_guard: crate::masking::RoleGuard,
    mut effects: Vec<Effect>,
) -> Result<(State, Vec<Effect>), KernelError> {
    // Precondition: policy name is non-empty.
    if name.is_empty() {
        return Err(KernelError::MaskingPolicyNameEmpty);
    }
    // Precondition: policy name is unique per tenant.
    if state.masking_policy_exists(tenant_id, &name) {
        return Err(KernelError::MaskingPolicyAlreadyExists {
            tenant_id,
            name,
        });
    }

    let record = MaskingPolicyRecord {
        tenant_id,
        name: name.clone(),
        strategy,
        role_guard,
    };

    effects.push(Effect::MaskingPolicyWrite(record.clone()));
    effects.push(Effect::AuditLogAppend(AuditAction::MaskingPolicyCreated {
        tenant_id,
        policy_name: name.clone(),
    }));
    debug_assert_eq!(effects.len(), 2);

    let new_state = state.with_masking_policy(record);

    // Postcondition: policy is now visible in state.
    assert!(
        new_state.masking_policy_exists(tenant_id, &name),
        "postcondition: policy `{name}` must exist for tenant {tenant_id} after CreateMaskingPolicy",
    );

    Ok((new_state, effects))
}

fn apply_attach_masking_policy(
    state: State,
    tenant_id: TenantId,
    table_id: TableId,
    column_name: String,
    policy_name: String,
    mut effects: Vec<Effect>,
) -> Result<(State, Vec<Effect>), KernelError> {
    // Precondition: target table exists and the tenant owns it.
    let table = state
        .get_table(&table_id)
        .ok_or(KernelError::TableNotFound(table_id))?;
    ensure_tenant_owns_table(tenant_id, table_id, table.tenant_id)?;

    // Precondition: column exists on the table.
    if !table.columns.iter().any(|c| c.name == column_name) {
        return Err(KernelError::ColumnNotFound {
            table_id,
            column_name,
        });
    }

    // Precondition: the named policy exists in the tenant's catalogue.
    if !state.masking_policy_exists(tenant_id, &policy_name) {
        return Err(KernelError::MaskingPolicyNotFound {
            tenant_id,
            name: policy_name,
        });
    }

    // Precondition: column is not already masked. Callers must
    // DETACH then ATTACH to change — we reject the silent overwrite
    // because stacking policies is not yet modelled.
    if state
        .masking_attachment(tenant_id, table_id, &column_name)
        .is_some()
    {
        return Err(KernelError::MaskingPolicyAlreadyAttached {
            table_id,
            column_name,
        });
    }

    effects.push(Effect::MaskingAttachmentWrite {
        tenant_id,
        table_id,
        column_name: column_name.clone(),
        policy_name: policy_name.clone(),
    });
    effects.push(Effect::AuditLogAppend(AuditAction::MaskingPolicyAttached {
        tenant_id,
        table_id: table_id.0,
        column_name: column_name.clone(),
        policy_name: policy_name.clone(),
    }));
    debug_assert_eq!(effects.len(), 2);

    let new_state =
        state.with_masking_attachment(tenant_id, table_id, column_name.clone(), policy_name);

    // Postcondition: attachment is now visible.
    assert!(
        new_state
            .masking_attachment(tenant_id, table_id, &column_name)
            .is_some(),
        "postcondition: attachment must exist after AttachMaskingPolicy",
    );

    Ok((new_state, effects))
}

fn apply_detach_masking_policy(
    state: State,
    tenant_id: TenantId,
    table_id: TableId,
    column_name: String,
    mut effects: Vec<Effect>,
) -> Result<(State, Vec<Effect>), KernelError> {
    let table = state
        .get_table(&table_id)
        .ok_or(KernelError::TableNotFound(table_id))?;
    ensure_tenant_owns_table(tenant_id, table_id, table.tenant_id)?;

    if state
        .masking_attachment(tenant_id, table_id, &column_name)
        .is_none()
    {
        return Err(KernelError::MaskingPolicyNotAttached {
            table_id,
            column_name,
        });
    }

    effects.push(Effect::MaskingAttachmentDrop {
        tenant_id,
        table_id,
        column_name: column_name.clone(),
    });
    effects.push(Effect::AuditLogAppend(AuditAction::MaskingPolicyDetached {
        tenant_id,
        table_id: table_id.0,
        column_name: column_name.clone(),
    }));
    debug_assert_eq!(effects.len(), 2);

    let new_state = state.without_masking_attachment(tenant_id, table_id, &column_name);

    assert!(
        new_state
            .masking_attachment(tenant_id, table_id, &column_name)
            .is_none(),
        "postcondition: attachment must be gone after DetachMaskingPolicy",
    );

    Ok((new_state, effects))
}

fn apply_drop_masking_policy(
    state: State,
    tenant_id: TenantId,
    name: String,
    mut effects: Vec<Effect>,
) -> Result<(State, Vec<Effect>), KernelError> {
    if !state.masking_policy_exists(tenant_id, &name) {
        return Err(KernelError::MaskingPolicyNotFound {
            tenant_id,
            name,
        });
    }
    // Precondition: no column attachments reference this policy.
    // Mirroring PostgreSQL — the caller must detach first so a dropped
    // policy can never silently leave a column unmasked mid-flight.
    if state.masking_policy_has_attachments(tenant_id, &name) {
        return Err(KernelError::MaskingPolicyStillAttached { tenant_id, name });
    }

    effects.push(Effect::MaskingPolicyDrop {
        tenant_id,
        name: name.clone(),
    });
    effects.push(Effect::AuditLogAppend(AuditAction::MaskingPolicyDropped {
        tenant_id,
        policy_name: name.clone(),
    }));
    debug_assert_eq!(effects.len(), 2);

    let new_state = state.without_masking_policy(tenant_id, &name);

    assert!(
        !new_state.masking_policy_exists(tenant_id, &name),
        "postcondition: policy `{name}` must be gone after DropMaskingPolicy",
    );

    Ok((new_state, effects))
}

/// AUDIT-2026-04 H-5 helper: extract the tenant_id from a
/// mutating command, or `None` if the command is tenant-agnostic
/// (CreateStream variants, AppendBatch — these don't carry a tenant
/// in their payload at this layer) or a lifecycle command that
/// shouldn't itself be gated by sealing.
///
/// Seal/Unseal themselves intentionally return None — we want
/// `UnsealTenant` to succeed against a sealed tenant (it's the only
/// way *out* of a seal), and `SealTenant` against an already-sealed
/// tenant is handled separately via `TenantAlreadySealed`.
fn mutating_tenant_id(cmd: &Command) -> Option<TenantId> {
    match cmd {
        Command::CreateTable { tenant_id, .. }
        | Command::DropTable { tenant_id, .. }
        | Command::AlterTableAddColumn { tenant_id, .. }
        | Command::AlterTableDropColumn { tenant_id, .. }
        | Command::CreateIndex { tenant_id, .. }
        | Command::Insert { tenant_id, .. }
        | Command::Update { tenant_id, .. }
        | Command::Delete { tenant_id, .. }
        | Command::CreateMaskingPolicy { tenant_id, .. }
        | Command::AttachMaskingPolicy { tenant_id, .. }
        | Command::DetachMaskingPolicy { tenant_id, .. }
        | Command::DropMaskingPolicy { tenant_id, .. }
        | Command::Upsert { tenant_id, .. } => Some(*tenant_id),
        Command::CreateStream { .. }
        | Command::CreateStreamWithAutoId { .. }
        | Command::AppendBatch { .. }
        | Command::SealTenant { .. }
        | Command::UnsealTenant { .. } => None,
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

    /// `ALTER TABLE ADD COLUMN` against a table that already has this name.
    ///
    /// Name comparison is case-sensitive to mirror the SQL catalog's
    /// sensitivity; the parser normalises unquoted identifiers to a
    /// canonical case before they reach the kernel.
    #[error("column '{column_name}' already exists on table {table_id}")]
    ColumnAlreadyExists {
        table_id: TableId,
        column_name: String,
    },

    /// `ALTER TABLE DROP COLUMN` against a non-existent column.
    #[error("column '{column_name}' not found on table {table_id}")]
    ColumnNotFound {
        table_id: TableId,
        column_name: String,
    },

    /// `ALTER TABLE DROP COLUMN` against a primary-key column.
    ///
    /// Dropping a primary-key column would invalidate every persisted
    /// row key. Structurally rejected so the SQL layer surfaces a clear
    /// error instead of a later corruption panic in the storage layer.
    #[error(
        "cannot drop primary-key column '{column_name}' from table {table_id}; \
         drop or recreate the table instead"
    )]
    CannotDropPrimaryKeyColumn {
        table_id: TableId,
        column_name: String,
    },

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

    // ------------------------------------------------------------------------
    // Tenant sealing errors (AUDIT-2026-04 H-5)
    // ------------------------------------------------------------------------
    /// Command rejected because the tenant is sealed.
    ///
    /// Sealed tenants reject every mutating command (DDL, DML,
    /// CreateStream, AppendBatch). Reads are unaffected. This is the
    /// error surfaced to callers so a forensic tool can render
    /// "operation blocked — tenant sealed for X" rather than a
    /// misleading "table not found" or a silent data mutation.
    #[error("tenant {tenant_id} is sealed; mutating commands are rejected")]
    TenantSealed { tenant_id: TenantId },

    /// Attempted to seal a tenant that is already sealed.
    #[error("tenant {tenant_id} is already sealed")]
    TenantAlreadySealed { tenant_id: TenantId },

    /// Attempted to unseal a tenant that is not sealed.
    #[error("tenant {tenant_id} is not sealed; cannot unseal")]
    TenantNotSealed { tenant_id: TenantId },

    // ------------------------------------------------------------------------
    // Masking policy errors (v0.6.0 Tier 2 #7)
    // ------------------------------------------------------------------------
    /// `CREATE MASKING POLICY` with an empty name.
    #[error("masking policy name must not be empty")]
    MaskingPolicyNameEmpty,

    /// `CREATE MASKING POLICY` with a name that already exists for this tenant.
    #[error("masking policy `{name}` already exists for tenant {tenant_id}")]
    MaskingPolicyAlreadyExists { tenant_id: TenantId, name: String },

    /// Lookup failed — no policy with this name in the tenant's catalogue.
    #[error("masking policy `{name}` not found for tenant {tenant_id}")]
    MaskingPolicyNotFound { tenant_id: TenantId, name: String },

    /// `DROP MASKING POLICY` attempted against a policy still referenced
    /// by a column attachment. Detach first.
    #[error(
        "masking policy `{name}` still attached to one or more columns in tenant {tenant_id}; \
         detach from every column before dropping"
    )]
    MaskingPolicyStillAttached { tenant_id: TenantId, name: String },

    /// `ALTER TABLE … SET MASKING POLICY` attempted on a column that
    /// already has a policy attached. Explicit detach required before
    /// a re-attach; silent overwrites are rejected.
    #[error("column `{column_name}` on table {table_id} already has a masking policy")]
    MaskingPolicyAlreadyAttached {
        table_id: TableId,
        column_name: String,
    },

    /// `ALTER TABLE … DROP MASKING POLICY` on a column that has none.
    #[error("column `{column_name}` on table {table_id} has no masking policy")]
    MaskingPolicyNotAttached {
        table_id: TableId,
        column_name: String,
    },

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
