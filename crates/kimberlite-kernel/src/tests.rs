//! Unit tests for kmb-kernel
//!
//! The kernel is pure (no IO), making it ideal for unit testing.
//! Every code path can be tested without mocks.

use bytes::Bytes;
use kimberlite_types::{
    AuditAction, DataClass, Offset, Placement, Region, SealReason, StreamId, StreamMetadata,
    StreamName, TenantId,
};

use crate::command::{ColumnDefinition, Command, IndexId, TableId};
use crate::effects::Effect;
use crate::kernel::{KernelError, apply_committed};
use crate::state::State;

// ============================================================================
// Test Helpers
// ============================================================================

fn test_stream_id() -> StreamId {
    StreamId::new(1)
}

fn test_stream_name() -> StreamName {
    StreamName::new("test-stream")
}

fn test_placement() -> Placement {
    Placement::Region(Region::APSoutheast2)
}

fn test_data_class() -> DataClass {
    DataClass::PHI
}

fn create_test_stream_cmd() -> Command {
    Command::create_stream(
        test_stream_id(),
        test_stream_name(),
        test_data_class(),
        test_placement(),
    )
}

/// Helper to create a state with a test stream already in it
fn state_with_test_stream() -> State {
    let state = State::new();
    let cmd = create_test_stream_cmd();
    let (state, _) = apply_committed(state, cmd).expect("failed to create test stream");
    state
}

/// Helper to create test events
fn test_events(count: usize) -> Vec<Bytes> {
    (0..count)
        .map(|i| Bytes::from(format!("event-{i}")))
        .collect()
}

// ============================================================================
// CreateStream Tests
// ============================================================================

#[test]
fn create_stream_on_empty_state_succeeds() {
    let state = State::new();
    let cmd = create_test_stream_cmd();
    let (state, effects) = apply_committed(state, cmd).expect("stream should exist");
    let meta = StreamMetadata {
        stream_id: test_stream_id(),
        stream_name: test_stream_name(),
        data_class: test_data_class(),
        placement: test_placement(),
        current_offset: Offset::default(),
    };

    assert!(state.stream_exists(&test_stream_id()));

    assert!(
        effects.contains(&Effect::AuditLogAppend(AuditAction::StreamCreated {
            stream_id: test_stream_id(),
            stream_name: test_stream_name(),
            data_class: test_data_class(),
            placement: test_placement()
        }))
    );

    assert!(effects.contains(&Effect::StreamMetadataWrite(meta)));
}

#[test]
fn create_stream_sets_initial_offset_to_zero() {
    let state = State::new();
    let cmd = create_test_stream_cmd();
    let (state, _) = apply_committed(state, cmd).expect("create should succeed");

    let stream = state
        .get_stream(&test_stream_id())
        .expect("stream should exist");

    assert_eq!(stream.current_offset, Offset::default());
    assert_eq!(stream.current_offset.as_u64(), 0);
}

#[test]
fn create_duplicate_stream_fails() {
    // Create first stream
    let state = state_with_test_stream();

    // Try to create the same stream again
    let cmd = create_test_stream_cmd();
    let result = apply_committed(state, cmd);

    assert!(matches!(
        result,
        Err(KernelError::StreamIdUniqueConstraint(id)) if id == test_stream_id()
    ));
}

#[test]
fn create_stream_produces_correct_effects() {
    let state = State::new();
    let cmd = create_test_stream_cmd();
    let (_, effects) = apply_committed(state, cmd).expect("create should succeed");

    // Should produce exactly 2 effects
    assert_eq!(effects.len(), 2);

    // First effect: StreamMetadataWrite
    let has_metadata_write = effects.iter().any(|e| {
        matches!(e, Effect::StreamMetadataWrite(meta)
            if meta.stream_id == test_stream_id()
            && meta.stream_name == test_stream_name()
            && meta.data_class == test_data_class()
            && meta.placement == test_placement()
        )
    });
    assert!(has_metadata_write, "missing StreamMetadataWrite effect");

    // Second effect: AuditLogAppend
    let has_audit = effects.iter().any(|e| {
        matches!(e, Effect::AuditLogAppend(AuditAction::StreamCreated { stream_id, .. })
            if *stream_id == test_stream_id()
        )
    });
    assert!(has_audit, "missing AuditLogAppend effect");
}

// ============================================================================
// AppendBatch Tests
// ============================================================================

#[test]
fn append_to_existing_stream_succeeds() {
    let state = state_with_test_stream();

    let cmd = Command::append_batch(test_stream_id(), test_events(3), Offset::default());

    let (state, _) = apply_committed(state, cmd).expect("append should succeed");

    let stream = state
        .get_stream(&test_stream_id())
        .expect("stream should exist");

    assert_eq!(stream.current_offset.as_u64(), 3);
}

#[test]
fn append_to_nonexistent_stream_fails() {
    let state = State::new(); // Empty state, no streams

    let cmd = Command::append_batch(
        StreamId::new(999), // Stream doesn't exist
        test_events(1),
        Offset::default(),
    );

    let result = apply_committed(state, cmd);

    assert!(matches!(
        result,
        Err(KernelError::StreamNotFound(id)) if id == StreamId::new(999)
    ));
}

#[test]
fn append_with_wrong_offset_fails() {
    let state = state_with_test_stream(); // Stream at offset 0

    let cmd = Command::append_batch(
        test_stream_id(),
        test_events(1),
        Offset::new(5), // Wrong! Stream is at 0
    );

    let result = apply_committed(state, cmd);

    assert!(matches!(
        result,
        Err(KernelError::UnexpectedStreamOffset {
            stream_id,
            expected,
            actual
        }) if stream_id == test_stream_id()
            && expected.as_u64() == 5
            && actual.as_u64() == 0
    ));
}

#[test]
fn append_updates_stream_offset() {
    let state = state_with_test_stream();

    // Append first batch (3 events)
    let (state, _) = apply_committed(
        state,
        Command::append_batch(test_stream_id(), test_events(3), Offset::new(0)),
    )
    .expect("batch 1 failed");

    let stream = state.get_stream(&test_stream_id()).unwrap();
    assert_eq!(stream.current_offset.as_u64(), 3);

    // Append second batch (2 events) with correct expected offset
    let (state, _) = apply_committed(
        state,
        Command::append_batch(test_stream_id(), test_events(2), Offset::new(3)),
    )
    .expect("batch 2 failed");

    let stream = state.get_stream(&test_stream_id()).unwrap();
    assert_eq!(stream.current_offset.as_u64(), 5);
}

#[test]
fn append_produces_correct_effects() {
    let state = state_with_test_stream();

    let events = test_events(3);
    let (_, effects) = apply_committed(
        state,
        Command::append_batch(test_stream_id(), events.clone(), Offset::default()),
    )
    .expect("append failed");

    // Should produce exactly 3 effects
    assert_eq!(effects.len(), 3);

    // StorageAppend with correct data
    let storage_effect = effects
        .iter()
        .find(|e| matches!(e, Effect::StorageAppend { .. }));
    assert!(storage_effect.is_some(), "missing StorageAppend effect");

    if let Some(Effect::StorageAppend {
        stream_id,
        base_offset,
        events: stored_events,
    }) = storage_effect
    {
        assert_eq!(*stream_id, test_stream_id());
        assert_eq!(base_offset.as_u64(), 0);
        assert_eq!(stored_events.len(), 3);
    }

    // WakeProjection with correct offset range
    let wake_effect = effects
        .iter()
        .find(|e| matches!(e, Effect::WakeProjection { .. }));
    assert!(wake_effect.is_some(), "missing WakeProjection effect");

    if let Some(Effect::WakeProjection {
        stream_id,
        from_offset,
        to_offset,
    }) = wake_effect
    {
        assert_eq!(*stream_id, test_stream_id());
        assert_eq!(from_offset.as_u64(), 0);
        assert_eq!(to_offset.as_u64(), 3);
    }

    // AuditLogAppend with correct count
    let audit_effect = effects.iter().find(|e| {
        matches!(
            e,
            Effect::AuditLogAppend(AuditAction::EventsAppended { .. })
        )
    });
    assert!(audit_effect.is_some(), "missing AuditLogAppend effect");

    if let Some(Effect::AuditLogAppend(AuditAction::EventsAppended {
        stream_id,
        count,
        from_offset,
    })) = audit_effect
    {
        assert_eq!(*stream_id, test_stream_id());
        assert_eq!(*count, 3);
        assert_eq!(from_offset.as_u64(), 0);
    }
}

#[test]
fn append_empty_batch_succeeds() {
    let state = state_with_test_stream();

    let (state, _) = apply_committed(
        state,
        Command::append_batch(
            test_stream_id(),
            vec![], // Empty batch
            Offset::default(),
        ),
    )
    .expect("append failed");

    // Offset should be unchanged
    let stream = state.get_stream(&test_stream_id()).unwrap();
    assert_eq!(stream.current_offset.as_u64(), 0);
}

// ============================================================================
// DDL Tests (CREATE TABLE, DROP TABLE, CREATE INDEX)
// ============================================================================

fn test_table_id() -> TableId {
    TableId::new(1)
}

fn test_tenant_id() -> kimberlite_types::TenantId {
    kimberlite_types::TenantId::new(1)
}

fn test_column_defs() -> Vec<ColumnDefinition> {
    vec![
        ColumnDefinition {
            name: "id".to_string(),
            data_type: "BIGINT".to_string(),
            nullable: false,
        },
        ColumnDefinition {
            name: "name".to_string(),
            data_type: "TEXT".to_string(),
            nullable: false,
        },
        ColumnDefinition {
            name: "age".to_string(),
            data_type: "BIGINT".to_string(),
            nullable: true,
        },
    ]
}

fn create_test_table_cmd() -> Command {
    Command::CreateTable {
        tenant_id: test_tenant_id(),
        table_id: test_table_id(),
        table_name: "users".to_string(),
        columns: test_column_defs(),
        primary_key: vec!["id".to_string()],
    }
}

#[test]
fn create_table_on_empty_state_succeeds() {
    let state = State::new();
    let cmd = create_test_table_cmd();

    let result = apply_committed(state, cmd);
    assert!(result.is_ok());

    let (state, effects) = result.unwrap();

    // Verify table exists in state
    assert!(state.table_exists(&test_table_id()));

    // Verify underlying stream was created
    let table = state.get_table(&test_table_id()).unwrap();
    assert!(state.stream_exists(&table.stream_id));

    // Should produce effects for stream creation and table metadata
    assert!(effects.len() >= 2);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::StreamMetadataWrite(_)))
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::TableMetadataWrite(_)))
    );
}

#[test]
fn create_duplicate_table_fails() {
    let state = State::new();
    let cmd = create_test_table_cmd();

    // Create table first time
    let (state, _) = apply_committed(state, cmd.clone()).expect("first create should succeed");

    // Attempt to create same table again
    let result = apply_committed(state, cmd);

    assert!(matches!(
        result,
        Err(KernelError::TableIdUniqueConstraint(_))
    ));
}

#[test]
fn create_table_duplicate_name_fails_within_same_tenant() {
    let state = State::new();
    let cmd = create_test_table_cmd();

    // Create first table
    let (state, _) = apply_committed(state, cmd).expect("first create should succeed");

    // Try to create another table with same name in the SAME tenant.
    // Must fail — uniqueness is scoped per-tenant, not global.
    let cmd2 = Command::CreateTable {
        tenant_id: test_tenant_id(),
        table_id: TableId::new(2),
        table_name: "users".to_string(),
        columns: test_column_defs(),
        primary_key: vec!["id".to_string()],
    };

    let result = apply_committed(state, cmd2);
    assert!(matches!(
        result,
        Err(KernelError::TableNameUniqueConstraint { .. })
    ));
}

#[test]
fn create_table_same_name_succeeds_across_tenants() {
    // Two different tenants must be able to own a table named "users".
    // The prior behavior — enforcing global table-name uniqueness — was
    // the compliance-grade leak this test protects against.
    let state = State::new();

    let cmd_a = Command::CreateTable {
        tenant_id: kimberlite_types::TenantId::new(1),
        table_id: TableId::new(1),
        table_name: "users".to_string(),
        columns: test_column_defs(),
        primary_key: vec!["id".to_string()],
    };

    let cmd_b = Command::CreateTable {
        tenant_id: kimberlite_types::TenantId::new(2),
        table_id: TableId::new(2),
        table_name: "users".to_string(),
        columns: test_column_defs(),
        primary_key: vec!["id".to_string()],
    };

    let (state, _) = apply_committed(state, cmd_a).expect("tenant 1 CREATE should succeed");
    let (state, _) = apply_committed(state, cmd_b).expect("tenant 2 CREATE should succeed");

    assert!(state.table_exists(&TableId::new(1)));
    assert!(state.table_exists(&TableId::new(2)));

    let tenant_1_users = state
        .table_by_tenant_name(kimberlite_types::TenantId::new(1), "users")
        .expect("tenant 1 owns users");
    let tenant_2_users = state
        .table_by_tenant_name(kimberlite_types::TenantId::new(2), "users")
        .expect("tenant 2 owns users");

    assert_ne!(tenant_1_users.table_id, tenant_2_users.table_id);
    assert_ne!(tenant_1_users.stream_id, tenant_2_users.stream_id);
}

#[test]
#[should_panic(expected = "cross-tenant table access")]
fn cross_tenant_insert_panics_production_assert() {
    // Tenant A creates a table; tenant B attempts an INSERT referencing
    // A's table_id. The kernel must panic (debug-build) with the
    // production-assertion message. In release, the error path returns
    // CrossTenantTableAccess and is captured by the caller.
    let state = State::new();
    let (state, _) = apply_committed(state, create_test_table_cmd())
        .expect("create table should succeed");

    let forged_insert = Command::Insert {
        tenant_id: kimberlite_types::TenantId::new(999),
        table_id: test_table_id(),
        row_data: Bytes::from(r#"{"id":1,"name":"Mallory"}"#),
    };

    // Expected: the debug_assert! in ensure_tenant_owns_table fires.
    let _ = apply_committed(state, forged_insert);
}

#[test]
fn drop_table_removes_table_from_state() {
    let state = State::new();
    let (state, _) = apply_committed(state, create_test_table_cmd()).unwrap();

    // Verify table exists
    assert!(state.table_exists(&test_table_id()));

    // Drop the table
    let (state, effects) = apply_committed(
        state,
        Command::DropTable {
            tenant_id: test_tenant_id(),
            table_id: test_table_id(),
        },
    )
    .expect("drop should succeed");

    // Verify table no longer exists
    assert!(!state.table_exists(&test_table_id()));

    // Should produce TableMetadataDrop effect
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::TableMetadataDrop { .. }))
    );
}

#[test]
fn drop_nonexistent_table_fails() {
    let state = State::new();

    let result = apply_committed(
        state,
        Command::DropTable {
            tenant_id: test_tenant_id(),
            table_id: TableId::new(999),
        },
    );

    assert!(matches!(result, Err(KernelError::TableNotFound(_))));
}

#[test]
fn create_index_on_table_succeeds() {
    let state = State::new();
    let (state, _) = apply_committed(state, create_test_table_cmd()).unwrap();

    let cmd = Command::CreateIndex {
        tenant_id: test_tenant_id(),
        index_id: IndexId::new(1),
        table_id: test_table_id(),
        index_name: "idx_name".to_string(),
        columns: vec!["name".to_string()],
    };

    let (state, effects) = apply_committed(state, cmd).expect("create index should succeed");

    // Verify index exists
    assert!(state.index_exists(&IndexId::new(1)));

    // Should produce IndexMetadataWrite effect
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::IndexMetadataWrite(_)))
    );
}

#[test]
fn create_index_on_nonexistent_table_fails() {
    let state = State::new();

    let cmd = Command::CreateIndex {
        tenant_id: test_tenant_id(),
        index_id: IndexId::new(1),
        table_id: TableId::new(999), // Doesn't exist
        index_name: "idx_name".to_string(),
        columns: vec!["name".to_string()],
    };

    let result = apply_committed(state, cmd);
    assert!(matches!(result, Err(KernelError::TableNotFound(_))));
}

#[test]
fn create_duplicate_index_fails() {
    let state = State::new();
    let (state, _) = apply_committed(state, create_test_table_cmd()).unwrap();

    let cmd = Command::CreateIndex {
        tenant_id: test_tenant_id(),
        index_id: IndexId::new(1),
        table_id: test_table_id(),
        index_name: "idx_name".to_string(),
        columns: vec!["name".to_string()],
    };

    // Create index first time
    let (state, _) = apply_committed(state, cmd.clone()).expect("first create should succeed");

    // Try to create same index again
    let result = apply_committed(state, cmd);
    assert!(matches!(
        result,
        Err(KernelError::IndexIdUniqueConstraint(_))
    ));
}

// ============================================================================
// DML Tests (INSERT, UPDATE, DELETE)
// ============================================================================

fn state_with_test_table() -> State {
    let state = State::new();
    let (state, _) =
        apply_committed(state, create_test_table_cmd()).expect("table creation failed");
    state
}

#[test]
fn insert_into_table_succeeds() {
    let state = state_with_test_table();

    let row_data = Bytes::from(r#"{"id":1,"name":"Alice","age":30}"#);
    let cmd = Command::Insert {
        tenant_id: test_tenant_id(),
        table_id: test_table_id(),
        row_data,
    };

    let (state, effects) = apply_committed(state, cmd).expect("insert should succeed");

    // Verify stream offset was advanced
    let table = state.get_table(&test_table_id()).unwrap();
    let stream = state.get_stream(&table.stream_id).unwrap();
    assert_eq!(stream.current_offset.as_u64(), 1);

    // Should produce effects
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::StorageAppend { .. }))
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::UpdateProjection { .. }))
    );
}

#[test]
fn insert_into_nonexistent_table_fails() {
    let state = State::new();

    let row_data = Bytes::from(r#"{"id":1,"name":"Alice"}"#);
    let cmd = Command::Insert {
        tenant_id: test_tenant_id(),
        table_id: TableId::new(999),
        row_data,
    };

    let result = apply_committed(state, cmd);
    assert!(matches!(result, Err(KernelError::TableNotFound(_))));
}

#[test]
fn update_table_row_succeeds() {
    let state = state_with_test_table();

    // Insert first
    let (state, _) = apply_committed(
        state,
        Command::Insert {
            tenant_id: test_tenant_id(),
            table_id: test_table_id(),
            row_data: Bytes::from(r#"{"id":1,"name":"Alice"}"#),
        },
    )
    .unwrap();

    // Now update
    let row_data = Bytes::from(r#"{"id":1,"name":"Alice Updated"}"#);
    let cmd = Command::Update {
        tenant_id: test_tenant_id(),
        table_id: test_table_id(),
        row_data,
    };

    let (state, effects) = apply_committed(state, cmd).expect("update should succeed");

    // Verify stream offset was advanced
    let table = state.get_table(&test_table_id()).unwrap();
    let stream = state.get_stream(&table.stream_id).unwrap();
    assert_eq!(stream.current_offset.as_u64(), 2); // Insert + Update

    // Should produce UpdateProjection effect
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::UpdateProjection { .. }))
    );
}

#[test]
fn delete_from_table_succeeds() {
    let state = state_with_test_table();

    // Insert first
    let (state, _) = apply_committed(
        state,
        Command::Insert {
            tenant_id: test_tenant_id(),
            table_id: test_table_id(),
            row_data: Bytes::from(r#"{"id":1,"name":"Alice"}"#),
        },
    )
    .unwrap();

    // Now delete
    let row_data = Bytes::from(r#"{"id":1}"#);
    let cmd = Command::Delete {
        tenant_id: test_tenant_id(),
        table_id: test_table_id(),
        row_data,
    };

    let (state, effects) = apply_committed(state, cmd).expect("delete should succeed");

    // Verify stream offset was advanced
    let table = state.get_table(&test_table_id()).unwrap();
    let stream = state.get_stream(&table.stream_id).unwrap();
    assert_eq!(stream.current_offset.as_u64(), 2); // Insert + Delete

    // Should produce UpdateProjection effect
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::UpdateProjection { .. }))
    );
}

#[test]
fn multiple_inserts_advance_offset_correctly() {
    let state = state_with_test_table();

    // Insert 3 rows
    let (state, _) = apply_committed(
        state,
        Command::Insert {
            tenant_id: test_tenant_id(),
            table_id: test_table_id(),
            row_data: Bytes::from(r#"{"id":1,"name":"Alice"}"#),
        },
    )
    .unwrap();

    let (state, _) = apply_committed(
        state,
        Command::Insert {
            tenant_id: test_tenant_id(),
            table_id: test_table_id(),
            row_data: Bytes::from(r#"{"id":2,"name":"Bob"}"#),
        },
    )
    .unwrap();

    let (state, _) = apply_committed(
        state,
        Command::Insert {
            tenant_id: test_tenant_id(),
            table_id: test_table_id(),
            row_data: Bytes::from(r#"{"id":3,"name":"Charlie"}"#),
        },
    )
    .unwrap();

    // Verify offset
    let table = state.get_table(&test_table_id()).unwrap();
    let stream = state.get_stream(&table.stream_id).unwrap();
    assert_eq!(stream.current_offset.as_u64(), 3);
}

// ============================================================================
// Property-Based Tests
// ============================================================================

mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn stream_count_increases_by_one_per_create(stream_ids in prop::collection::vec(0u64..1000, 1..10)) {
            // Ensure unique IDs
            prop_assume!(stream_ids.iter().collect::<std::collections::HashSet<_>>().len() == stream_ids.len());

            let mut state = State::new();

            for (i, id) in stream_ids.iter().enumerate() {
                let cmd = Command::create_stream(
                    StreamId::new(*id),
                    StreamName::new(format!("stream-{id}")),
                    DataClass::Public,
                    Placement::Global,
                );

                let (new_state, _) = apply_committed(state, cmd).expect("create should succeed");
                state = new_state;

                // Stream count should match number of streams created
                prop_assert_eq!(state.stream_count(), i + 1);
            }
        }

        #[test]
        fn offset_equals_total_events_appended(batch_sizes in prop::collection::vec(1usize..100, 1..5)) {
            // Create a stream
            let mut state = State::new();
            let cmd = Command::create_stream(
                StreamId::new(1),
                StreamName::new("test"),
                DataClass::Public,
                Placement::Global,
            );
            let (new_state, _) = apply_committed(state, cmd).expect("create should succeed");
            state = new_state;

            let mut expected_offset: u64 = 0;

            for batch_size in batch_sizes {
                let events: Vec<Bytes> = (0..batch_size)
                    .map(|i| Bytes::from(format!("event-{i}")))
                    .collect();

                let (new_state, _) = apply_committed(state, Command::append_batch(
                    StreamId::new(1),
                    events,
                    Offset::new(expected_offset),
                )).expect("append should succeed");
                state = new_state;

                expected_offset += batch_size as u64;
            }

            // Final offset should equal sum of all batch sizes
            let stream = state.get_stream(&StreamId::new(1)).unwrap();
            prop_assert_eq!(stream.current_offset.as_u64(), expected_offset);
        }

        /// Verifies that applying the same sequence of commands twice produces
        /// byte-identical final states (determinism requirement).
        #[test]
        fn replay_determinism(
            num_streams in 1usize..20,
            appends_per_stream in 1usize..10,
        ) {
            let mut commands = Vec::new();

            // Generate CreateStream commands
            for i in 0..num_streams {
                commands.push(Command::create_stream(
                    StreamId::new(i as u64 + 1),
                    StreamName::new(format!("stream-{i}")),
                    DataClass::Public,
                    Placement::Global,
                ));
            }

            // Generate AppendBatch commands for each stream
            for i in 0..num_streams {
                for j in 0..appends_per_stream {
                    let events = vec![Bytes::from(format!("event-{i}-{j}"))];
                    commands.push(Command::append_batch(
                        StreamId::new(i as u64 + 1),
                        events,
                        Offset::new(j as u64),
                    ));
                }
            }

            // First execution
            let mut state1 = State::new();
            for cmd in commands.iter().cloned() {
                let (new_state, _) = apply_committed(state1, cmd)
                    .expect("command should succeed");
                state1 = new_state;
            }

            // Second execution with same commands
            let mut state2 = State::new();
            for cmd in commands.iter().cloned() {
                let (new_state, _) = apply_committed(state2, cmd)
                    .expect("command should succeed");
                state2 = new_state;
            }

            // States should be byte-identical (determinism)
            prop_assert_eq!(state1.stream_count(), state2.stream_count());

            // Verify each stream has identical state
            for i in 0..num_streams {
                let stream_id = StreamId::new(i as u64 + 1);
                let s1 = state1.get_stream(&stream_id).unwrap();
                let s2 = state2.get_stream(&stream_id).unwrap();

                prop_assert_eq!(s1.stream_id, s2.stream_id);
                prop_assert_eq!(&s1.stream_name, &s2.stream_name);
                prop_assert_eq!(s1.current_offset, s2.current_offset);
                prop_assert_eq!(s1.data_class, s2.data_class);
                prop_assert_eq!(&s1.placement, &s2.placement);
            }
        }

        /// Verifies that state can be reconstructed from empty by replaying
        /// a log of operations (fundamental invariant: State = Apply(∅, Log)).
        #[test]
        fn state_reconstruction_from_empty(
            operations in prop::collection::vec(1usize..50, 5..20),
        ) {
            // Build a log of operations on a single stream
            let stream_id = StreamId::new(1);
            let mut log = vec![
                Command::create_stream(
                    stream_id,
                    StreamName::new("reconstruction-test"),
                    DataClass::Public,
                    Placement::Global,
                ),
            ];

            let mut expected_offset = 0u64;
            for batch_size in operations {
                let events: Vec<Bytes> = (0..batch_size)
                    .map(|i| Bytes::from(format!("event-{expected_offset}-{i}")))
                    .collect();

                log.push(Command::append_batch(
                    stream_id,
                    events,
                    Offset::new(expected_offset),
                ));

                expected_offset += batch_size as u64;
            }

            // Build state incrementally
            let mut incremental_state = State::new();
            for cmd in log.iter().cloned() {
                let (new_state, _) = apply_committed(incremental_state, cmd)
                    .expect("command should succeed");
                incremental_state = new_state;
            }

            // Reconstruct state from empty by replaying entire log
            let mut reconstructed_state = State::new();
            for cmd in log.iter().cloned() {
                let (new_state, _) = apply_committed(reconstructed_state, cmd)
                    .expect("command should succeed");
                reconstructed_state = new_state;
            }

            // States should be identical
            prop_assert_eq!(
                incremental_state.stream_count(),
                reconstructed_state.stream_count()
            );

            let inc_stream = incremental_state.get_stream(&stream_id).unwrap();
            let rec_stream = reconstructed_state.get_stream(&stream_id).unwrap();

            prop_assert_eq!(inc_stream.current_offset, rec_stream.current_offset);
            prop_assert_eq!(inc_stream.current_offset.as_u64(), expected_offset);
        }
    }
}

// ============================================================================
// Edge Case Tests (Phase 2: Logic Bug Detection)
// ============================================================================

#[test]
fn test_offset_gap_detection() {
    // Create a stream with one event at offset 0
    let state = State::new();
    let cmd = Command::create_stream(
        StreamId::new(1),
        StreamName::new("test"),
        DataClass::Public,
        Placement::Global,
    );
    let (state, _) = apply_committed(state, cmd).expect("create should succeed");

    let cmd = Command::append_batch(
        StreamId::new(1),
        vec![Bytes::from("event-0")],
        Offset::new(0),
    );
    let (state, _) = apply_committed(state, cmd).expect("first append should succeed");

    // Try to append at wrong expected offset (gap of 1)
    let cmd = Command::append_batch(
        StreamId::new(1),
        vec![Bytes::from("event-1")],
        Offset::new(0), // Wrong! Should be 1
    );
    let result = apply_committed(state, cmd);

    assert!(
        result.is_err(),
        "Appending with wrong expected_offset should fail"
    );
    assert!(
        matches!(
            result.unwrap_err(),
            KernelError::UnexpectedStreamOffset { .. }
        ),
        "Should return UnexpectedStreamOffset error"
    );
}

#[test]
fn test_multiple_streams_isolated() {
    // Create multiple streams and verify their offsets are independent
    let mut state = State::new();

    // Create 5 streams
    for i in 0..5 {
        let cmd = Command::create_stream(
            StreamId::new(i + 1),
            StreamName::new(format!("stream-{i}")),
            DataClass::Public,
            Placement::Global,
        );
        let (new_state, _) = apply_committed(state, cmd).expect("create should succeed");
        state = new_state;
    }

    // Append different numbers of events to each stream
    for i in 0..5 {
        let event_count = (i + 1) * 2; // 2, 4, 6, 8, 10 events
        for j in 0..event_count {
            let cmd = Command::append_batch(
                StreamId::new(i + 1),
                vec![Bytes::from(format!("stream-{i}-event-{j}"))],
                Offset::new(j),
            );
            let (new_state, _) = apply_committed(state, cmd).expect("append should succeed");
            state = new_state;
        }
    }

    // Verify each stream has correct offset
    for i in 0..5 {
        let stream = state.get_stream(&StreamId::new(i + 1)).unwrap();
        let expected_offset = (i + 1) * 2;
        assert_eq!(
            stream.current_offset.as_u64(),
            expected_offset,
            "Stream {i} should have offset {expected_offset}"
        );
    }
}

#[test]
fn test_invalid_stream_id() {
    let state = State::new();

    // Try to append to non-existent stream
    let cmd = Command::append_batch(
        StreamId::new(999),
        vec![Bytes::from("data")],
        Offset::new(0),
    );
    let result = apply_committed(state, cmd);

    assert!(
        result.is_err(),
        "Appending to non-existent stream should fail"
    );
    assert!(
        matches!(result.unwrap_err(), KernelError::StreamNotFound(_)),
        "Should return StreamNotFound error"
    );
}

// ============================================================================
// Table Lifecycle Tests
// ============================================================================

#[test]
fn test_table_drop_recreate() {
    let state = State::new();

    // Create table
    let table_id = TableId::new(1);
    let cmd = Command::CreateTable {
        tenant_id: test_tenant_id(),
        table_id,
        table_name: "users".to_string(),
        columns: vec![
            ColumnDefinition {
                name: "id".to_string(),
                data_type: "INT".to_string(),
                nullable: false,
            },
            ColumnDefinition {
                name: "name".to_string(),
                data_type: "TEXT".to_string(),
                nullable: true,
            },
        ],
        primary_key: vec!["id".to_string()],
    };
    let (state, _) = apply_committed(state, cmd).expect("create table should succeed");

    assert!(state.table_exists(&table_id), "Table should exist");

    // Drop table
    let cmd = Command::DropTable {
        tenant_id: test_tenant_id(),
        table_id,
    };
    let (state, _) = apply_committed(state, cmd).expect("drop table should succeed");

    assert!(
        !state.table_exists(&table_id),
        "Table should not exist after drop"
    );

    // Recreate with same ID should succeed (new lifecycle)
    let cmd = Command::CreateTable {
        tenant_id: test_tenant_id(),
        table_id,
        table_name: "users_v2".to_string(),
        columns: vec![ColumnDefinition {
            name: "id".to_string(),
            data_type: "INT".to_string(),
            nullable: false,
        }],
        primary_key: vec!["id".to_string()],
    };
    let (state, _) = apply_committed(state, cmd).expect("recreate table should succeed");

    assert!(state.table_exists(&table_id), "Table should exist again");
}

#[test]
fn test_duplicate_table_name_rejected() {
    let state = State::new();

    // Create first table
    let cmd = Command::CreateTable {
        tenant_id: test_tenant_id(),
        table_id: TableId::new(1),
        table_name: "users".to_string(),
        columns: vec![ColumnDefinition {
            name: "id".to_string(),
            data_type: "INT".to_string(),
            nullable: false,
        }],
        primary_key: vec!["id".to_string()],
    };
    let (state, _) = apply_committed(state, cmd).expect("first create should succeed");

    // Try to create another table with same name (different ID) in the
    // SAME tenant. Must fail — per-tenant uniqueness is the invariant.
    let cmd = Command::CreateTable {
        tenant_id: test_tenant_id(),
        table_id: TableId::new(2),
        table_name: "users".to_string(),
        columns: vec![ColumnDefinition {
            name: "id".to_string(),
            data_type: "INT".to_string(),
            nullable: false,
        }],
        primary_key: vec!["id".to_string()],
    };
    let result = apply_committed(state, cmd);

    assert!(
        result.is_err(),
        "Creating table with duplicate name should fail"
    );
    assert!(
        matches!(
            result.unwrap_err(),
            KernelError::TableNameUniqueConstraint { .. }
        ),
        "Should return TableNameUniqueConstraint error"
    );
}

#[test]
fn test_duplicate_table_id_rejected() {
    let state = State::new();

    // Create first table
    let table_id = TableId::new(1);
    let cmd = Command::CreateTable {
        tenant_id: test_tenant_id(),
        table_id,
        table_name: "users".to_string(),
        columns: vec![ColumnDefinition {
            name: "id".to_string(),
            data_type: "INT".to_string(),
            nullable: false,
        }],
        primary_key: vec!["id".to_string()],
    };
    let (state, _) = apply_committed(state, cmd).expect("first create should succeed");

    // Try to create another table with same ID
    let cmd = Command::CreateTable {
        tenant_id: test_tenant_id(),
        table_id, // Same ID!
        table_name: "posts".to_string(),
        columns: vec![ColumnDefinition {
            name: "id".to_string(),
            data_type: "INT".to_string(),
            nullable: false,
        }],
        primary_key: vec!["id".to_string()],
    };
    let result = apply_committed(state, cmd);

    assert!(
        result.is_err(),
        "Creating table with duplicate ID should fail"
    );
    assert!(
        matches!(result.unwrap_err(), KernelError::TableIdUniqueConstraint(_)),
        "Should return TableIdUniqueConstraint error"
    );
}

// ============================================================================
// AUDIT-2026-04 H-5 — Tenant sealing tests
// ============================================================================

fn create_table_for(tenant: TenantId, table_id: TableId) -> Command {
    Command::CreateTable {
        tenant_id: tenant,
        table_id,
        table_name: format!("t_{}", table_id.0),
        columns: vec![ColumnDefinition {
            name: "id".to_string(),
            data_type: "BIGINT".to_string(),
            nullable: false,
        }],
        primary_key: vec!["id".to_string()],
    }
}

#[test]
fn seal_tenant_then_insert_is_rejected() {
    let state = State::new();

    // Create a table for tenant A.
    let (state, _) = apply_committed(state, create_table_for(TenantId::new(1), TableId::new(10)))
        .expect("create table");

    // Seal tenant A.
    let (state, effects) = apply_committed(
        state,
        Command::SealTenant {
            tenant_id: TenantId::new(1),
            reason: SealReason::LegalHold,
            sealed_at_ns: 42,
        },
    )
    .expect("seal");
    assert_eq!(effects.len(), 1);
    assert!(matches!(
        &effects[0],
        Effect::AuditLogAppend(AuditAction::TenantSealed { tenant_id, reason })
            if *tenant_id == TenantId::new(1) && *reason == SealReason::LegalHold,
    ));
    assert!(state.is_tenant_sealed(TenantId::new(1)));

    // Attempt a DML op; must be rejected with TenantSealed, state unchanged.
    let forged = Command::Insert {
        tenant_id: TenantId::new(1),
        table_id: TableId::new(10),
        row_data: Bytes::from_static(b"{\"id\":1}"),
    };
    let result = apply_committed(state.clone(), forged);
    let err = result.unwrap_err();
    assert!(
        matches!(err, KernelError::TenantSealed { tenant_id } if tenant_id == TenantId::new(1)),
        "expected TenantSealed, got {err:?}",
    );
}

#[test]
fn sealed_tenant_rejects_every_mutating_variant() {
    let mut state = State::new();
    state = apply_committed(state, create_table_for(TenantId::new(5), TableId::new(5)))
        .unwrap()
        .0;
    state = apply_committed(
        state,
        Command::SealTenant {
            tenant_id: TenantId::new(5),
            reason: SealReason::AuditInProgress,
            sealed_at_ns: 1,
        },
    )
    .unwrap()
    .0;

    let cases = vec![
        create_table_for(TenantId::new(5), TableId::new(6)),
        Command::DropTable {
            tenant_id: TenantId::new(5),
            table_id: TableId::new(5),
        },
        Command::CreateIndex {
            tenant_id: TenantId::new(5),
            index_id: IndexId::new(1),
            table_id: TableId::new(5),
            index_name: "idx".into(),
            columns: vec!["id".into()],
        },
        Command::Insert {
            tenant_id: TenantId::new(5),
            table_id: TableId::new(5),
            row_data: Bytes::from_static(b"{}"),
        },
        Command::Update {
            tenant_id: TenantId::new(5),
            table_id: TableId::new(5),
            row_data: Bytes::from_static(b"{}"),
        },
        Command::Delete {
            tenant_id: TenantId::new(5),
            table_id: TableId::new(5),
            row_data: Bytes::from_static(b"{}"),
        },
    ];

    for cmd in cases {
        let result = apply_committed(state.clone(), cmd.clone());
        let err = result.expect_err(&format!("expected rejection for {cmd:?}"));
        assert!(
            matches!(err, KernelError::TenantSealed { .. }),
            "expected TenantSealed for {cmd:?}, got {err:?}",
        );
    }
}

#[test]
fn sealed_tenant_a_does_not_affect_tenant_b() {
    let mut state = State::new();
    state = apply_committed(state, create_table_for(TenantId::new(1), TableId::new(1)))
        .unwrap()
        .0;
    state = apply_committed(state, create_table_for(TenantId::new(2), TableId::new(2)))
        .unwrap()
        .0;
    state = apply_committed(
        state,
        Command::SealTenant {
            tenant_id: TenantId::new(1),
            reason: SealReason::ForensicHold,
            sealed_at_ns: 1,
        },
    )
    .unwrap()
    .0;

    // Tenant 2 is not sealed — an insert succeeds.
    let ok = Command::Insert {
        tenant_id: TenantId::new(2),
        table_id: TableId::new(2),
        row_data: Bytes::from_static(b"{\"id\":42}"),
    };
    apply_committed(state.clone(), ok).expect("tenant 2 writes must succeed");

    // Tenant 1 is sealed — an insert fails.
    let blocked = Command::Insert {
        tenant_id: TenantId::new(1),
        table_id: TableId::new(1),
        row_data: Bytes::from_static(b"{\"id\":42}"),
    };
    apply_committed(state, blocked).expect_err("tenant 1 writes must be blocked");
}

#[test]
fn unseal_restores_write_capability() {
    let mut state = State::new();
    state = apply_committed(state, create_table_for(TenantId::new(7), TableId::new(1)))
        .unwrap()
        .0;
    state = apply_committed(
        state,
        Command::SealTenant {
            tenant_id: TenantId::new(7),
            reason: SealReason::LegalHold,
            sealed_at_ns: 1,
        },
    )
    .unwrap()
    .0;
    assert!(state.is_tenant_sealed(TenantId::new(7)));

    let (state, effects) = apply_committed(
        state,
        Command::UnsealTenant {
            tenant_id: TenantId::new(7),
        },
    )
    .expect("unseal");
    assert_eq!(effects.len(), 1);
    assert!(matches!(
        &effects[0],
        Effect::AuditLogAppend(AuditAction::TenantUnsealed { tenant_id })
            if *tenant_id == TenantId::new(7),
    ));
    assert!(!state.is_tenant_sealed(TenantId::new(7)));

    // Writes now succeed.
    let ok = Command::Insert {
        tenant_id: TenantId::new(7),
        table_id: TableId::new(1),
        row_data: Bytes::from_static(b"{\"id\":1}"),
    };
    apply_committed(state, ok).expect("writes resume after unseal");
}

#[test]
fn seal_twice_errors() {
    let state = State::new();
    let (state, _) = apply_committed(
        state,
        Command::SealTenant {
            tenant_id: TenantId::new(9),
            reason: SealReason::ForensicHold,
            sealed_at_ns: 1,
        },
    )
    .unwrap();

    let err = apply_committed(
        state,
        Command::SealTenant {
            tenant_id: TenantId::new(9),
            reason: SealReason::BreachInvestigation,
            sealed_at_ns: 2,
        },
    )
    .unwrap_err();
    assert!(
        matches!(err, KernelError::TenantAlreadySealed { tenant_id } if tenant_id == TenantId::new(9)),
        "expected TenantAlreadySealed, got {err:?}",
    );
}

#[test]
fn unseal_without_seal_errors() {
    let state = State::new();
    let err = apply_committed(
        state,
        Command::UnsealTenant {
            tenant_id: TenantId::new(3),
        },
    )
    .unwrap_err();
    assert!(
        matches!(err, KernelError::TenantNotSealed { tenant_id } if tenant_id == TenantId::new(3)),
        "expected TenantNotSealed, got {err:?}",
    );
}
