//! Unit tests for kmb-kernel
//!
//! The kernel is pure (no IO), making it ideal for unit testing.
//! Every code path can be tested without mocks.

use bytes::Bytes;
use kimberlite_types::{
    AuditAction, DataClass, Offset, Placement, Region, StreamId, StreamMetadata, StreamName,
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
fn create_table_with_duplicate_name_fails() {
    let state = State::new();
    let cmd = create_test_table_cmd();

    // Create first table
    let (state, _) = apply_committed(state, cmd).expect("first create should succeed");

    // Try to create another table with same name but different ID
    let cmd2 = Command::CreateTable {
        table_id: TableId::new(2),
        table_name: "users".to_string(), // Same name
        columns: test_column_defs(),
        primary_key: vec!["id".to_string()],
    };

    let result = apply_committed(state, cmd2);
    assert!(matches!(
        result,
        Err(KernelError::TableNameUniqueConstraint(_))
    ));
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
            .any(|e| matches!(e, Effect::TableMetadataDrop(_)))
    );
}

#[test]
fn drop_nonexistent_table_fails() {
    let state = State::new();

    let result = apply_committed(
        state,
        Command::DropTable {
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
            table_id: test_table_id(),
            row_data: Bytes::from(r#"{"id":1,"name":"Alice"}"#),
        },
    )
    .unwrap();

    // Now update
    let row_data = Bytes::from(r#"{"id":1,"name":"Alice Updated"}"#);
    let cmd = Command::Update {
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
            table_id: test_table_id(),
            row_data: Bytes::from(r#"{"id":1,"name":"Alice"}"#),
        },
    )
    .unwrap();

    // Now delete
    let row_data = Bytes::from(r#"{"id":1}"#);
    let cmd = Command::Delete {
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
            table_id: test_table_id(),
            row_data: Bytes::from(r#"{"id":1,"name":"Alice"}"#),
        },
    )
    .unwrap();

    let (state, _) = apply_committed(
        state,
        Command::Insert {
            table_id: test_table_id(),
            row_data: Bytes::from(r#"{"id":2,"name":"Bob"}"#),
        },
    )
    .unwrap();

    let (state, _) = apply_committed(
        state,
        Command::Insert {
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
                    DataClass::NonPHI,
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
                DataClass::NonPHI,
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
                    DataClass::NonPHI,
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
                    DataClass::NonPHI,
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
        DataClass::NonPHI,
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
            DataClass::NonPHI,
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
    let cmd = Command::DropTable { table_id };
    let (state, _) = apply_committed(state, cmd).expect("drop table should succeed");

    assert!(
        !state.table_exists(&table_id),
        "Table should not exist after drop"
    );

    // Recreate with same ID should succeed (new lifecycle)
    let cmd = Command::CreateTable {
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

    // Try to create another table with same name (different ID)
    let cmd = Command::CreateTable {
        table_id: TableId::new(2),
        table_name: "users".to_string(), // Same name!
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
            KernelError::TableNameUniqueConstraint(_)
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
