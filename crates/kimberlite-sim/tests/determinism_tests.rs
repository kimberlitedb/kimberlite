//! Tests for determinism validation and state hash stability.
//!
//! These tests verify that kernel state hashing is deterministic and
//! sensitive to changes.

use bytes::Bytes;
use kimberlite_kernel::{Command, State, apply_committed};
use kimberlite_types::{DataClass, Offset, Placement, Region, StreamId, StreamName};

#[test]
fn test_empty_state_hash_is_stable() {
    // Empty states should always produce the same hash
    let state1 = State::new();
    let state2 = State::new();

    let hash1 = state1.compute_state_hash();
    let hash2 = state2.compute_state_hash();

    assert_eq!(hash1, hash2);
}

#[test]
fn test_state_hash_is_repeatable() {
    // Same state hashed multiple times should produce identical results
    let state = State::new();
    let (state, _) = apply_committed(
        state,
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("test"),
            data_class: DataClass::Public,
            placement: Placement::Global,
        },
    )
    .unwrap();

    let hash1 = state.compute_state_hash();
    let hash2 = state.compute_state_hash();
    let hash3 = state.compute_state_hash();

    assert_eq!(hash1, hash2);
    assert_eq!(hash2, hash3);
}

#[test]
fn test_equivalent_states_have_same_hash() {
    // Two states with the same content should hash identically
    let (state1, _) = apply_committed(
        State::new(),
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("stream1"),
            data_class: DataClass::Public,
            placement: Placement::Region(Region::USEast1),
        },
    )
    .unwrap();

    let (state2, _) = apply_committed(
        State::new(),
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("stream1"),
            data_class: DataClass::Public,
            placement: Placement::Region(Region::USEast1),
        },
    )
    .unwrap();

    assert_eq!(state1.compute_state_hash(), state2.compute_state_hash());
}

#[test]
fn test_different_stream_names_produce_different_hashes() {
    let (state1, _) = apply_committed(
        State::new(),
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("alice"),
            data_class: DataClass::Public,
            placement: Placement::Global,
        },
    )
    .unwrap();

    let (state2, _) = apply_committed(
        State::new(),
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("bob"),
            data_class: DataClass::Public,
            placement: Placement::Global,
        },
    )
    .unwrap();

    assert_ne!(state1.compute_state_hash(), state2.compute_state_hash());
}

#[test]
fn test_different_placements_produce_different_hashes() {
    let (state1, _) = apply_committed(
        State::new(),
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("test"),
            data_class: DataClass::Public,
            placement: Placement::Global,
        },
    )
    .unwrap();

    let (state2, _) = apply_committed(
        State::new(),
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("test"),
            data_class: DataClass::Public,
            placement: Placement::Region(Region::USEast1),
        },
    )
    .unwrap();

    assert_ne!(state1.compute_state_hash(), state2.compute_state_hash());
}

#[test]
fn test_different_data_classes_produce_different_hashes() {
    let (state1, _) = apply_committed(
        State::new(),
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("test"),
            data_class: DataClass::Public,
            placement: Placement::Global,
        },
    )
    .unwrap();

    let (state2, _) = apply_committed(
        State::new(),
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("test"),
            data_class: DataClass::PHI,
            placement: Placement::Global,
        },
    )
    .unwrap();

    assert_ne!(state1.compute_state_hash(), state2.compute_state_hash());
}

#[test]
fn test_command_sequence_produces_deterministic_hash() {
    // Same sequence of commands should always produce the same final state hash

    fn apply_sequence() -> [u8; 32] {
        let state = State::new();

        // Create stream
        let (state, _) = apply_committed(
            state,
            Command::CreateStream {
                stream_id: StreamId::new(1),
                stream_name: StreamName::new("events"),
                data_class: DataClass::Public,
                placement: Placement::Global,
            },
        )
        .expect("CreateStream failed");

        // Append events
        let (state, _) = apply_committed(
            state,
            Command::AppendBatch {
                stream_id: StreamId::new(1),
                events: vec![
                    Bytes::from("event1"),
                    Bytes::from("event2"),
                    Bytes::from("event3"),
                ],
                expected_offset: Offset::ZERO,
            },
        )
        .expect("AppendBatch failed");

        state.compute_state_hash()
    }

    let hash1 = apply_sequence();
    let hash2 = apply_sequence();
    let hash3 = apply_sequence();

    assert_eq!(hash1, hash2);
    assert_eq!(hash2, hash3);
}

#[test]
fn test_order_of_operations_affects_hash() {
    // Different order of creating streams should produce different hashes
    // (because stream IDs are auto-incremented)

    let state1 = State::new();
    let (state1, _) = apply_committed(
        state1,
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("stream-a"),
            data_class: DataClass::Public,
            placement: Placement::Global,
        },
    )
    .unwrap();
    let (state1, _) = apply_committed(
        state1,
        Command::CreateStream {
            stream_id: StreamId::new(2),
            stream_name: StreamName::new("stream-b"),
            data_class: DataClass::Public,
            placement: Placement::Global,
        },
    )
    .unwrap();

    let state2 = State::new();
    let (state2, _) = apply_committed(
        state2,
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("stream-b"),
            data_class: DataClass::Public,
            placement: Placement::Global,
        },
    )
    .unwrap();
    let (state2, _) = apply_committed(
        state2,
        Command::CreateStream {
            stream_id: StreamId::new(2),
            stream_name: StreamName::new("stream-a"),
            data_class: DataClass::Public,
            placement: Placement::Global,
        },
    )
    .unwrap();

    // Hashes should differ because different names are assigned to the same stream IDs
    assert_ne!(state1.compute_state_hash(), state2.compute_state_hash());
}
