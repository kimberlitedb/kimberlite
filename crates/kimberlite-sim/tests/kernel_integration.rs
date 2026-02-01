//! Integration tests for running the kernel under VOPR simulation.
//!
//! These tests demonstrate that the kernel can be executed with simulated
//! Clock, Storage, and Network implementations, enabling deterministic testing.

use bytes::Bytes;
use kimberlite_kernel::traits::{Clock, Storage};
use kimberlite_kernel::{Command, ReplicaId, Runtime, State, apply_committed};
use kimberlite_sim::kernel_adapter::{ClockAdapter, NetworkAdapter, StorageAdapter};
use kimberlite_sim::{NetworkConfig, SimClock, SimNetwork};
use kimberlite_types::{DataClass, Offset, Placement, StreamId, StreamName, TenantId};

#[test]
fn kernel_runs_under_simulation() {
    // Create simulated components
    let clock = ClockAdapter::new(SimClock::new());
    let storage = StorageAdapter::new_reliable();
    let network = NetworkAdapter::new(
        SimNetwork::new(NetworkConfig::reliable()),
        ReplicaId::new(0),
        TenantId::new(1),
    );

    // Create runtime with simulated components
    let mut runtime = Runtime::new(clock, storage, network);

    // Create initial kernel state
    let state = State::new();

    // Issue a command: CreateStream
    let cmd = Command::CreateStream {
        stream_id: StreamId::new(1),
        stream_name: StreamName::new("test-stream"),
        data_class: DataClass::NonPHI,
        placement: Placement::Global,
    };

    // Apply command to get new state and effects
    let (new_state, effects) = apply_committed(state, cmd).expect("command should succeed");

    // Execute effects via runtime
    runtime
        .execute_effects(effects)
        .expect("effects should execute");

    // Verify state was updated
    assert!(new_state.stream_exists(&StreamId::new(1)));
}

#[test]
fn kernel_append_batch_under_simulation() {
    // Setup
    let clock = ClockAdapter::new(SimClock::new());
    let storage = StorageAdapter::new_reliable();
    let network = NetworkAdapter::new(
        SimNetwork::new(NetworkConfig::reliable()),
        ReplicaId::new(0),
        TenantId::new(1),
    );

    let mut runtime = Runtime::new(clock, storage, network);

    // Create stream first
    let state = State::new();
    let (state, effects) = apply_committed(
        state,
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("events"),
            data_class: DataClass::NonPHI,
            placement: Placement::Global,
        },
    )
    .unwrap();

    runtime.execute_effects(effects).unwrap();

    // Append events
    let events = vec![
        Bytes::from("event1"),
        Bytes::from("event2"),
        Bytes::from("event3"),
    ];

    let (new_state, effects) = apply_committed(
        state,
        Command::AppendBatch {
            stream_id: StreamId::new(1),
            events,
            expected_offset: Offset::ZERO,
        },
    )
    .unwrap();

    runtime.execute_effects(effects).unwrap();

    // Verify offset was advanced
    let stream = new_state.get_stream(&StreamId::new(1)).unwrap();
    assert_eq!(stream.current_offset, Offset::from(3u64));

    // Verify storage contains the events
    let stored_events = runtime
        .storage()
        .read(StreamId::new(1), Offset::ZERO, Offset::from(3u64))
        .unwrap();

    assert_eq!(stored_events.len(), 3);
}

#[test]
fn simulated_clock_advances_deterministically() {
    let mut clock = ClockAdapter::new(SimClock::new());

    assert_eq!(clock.now_ns(), 0);

    // Simulate "sleep" - in simulation, this advances the clock
    clock.sleep_ns(1_000_000); // 1ms
    assert_eq!(clock.now_ns(), 1_000_000);

    clock.sleep_ns(2_500_000); // 2.5ms
    assert_eq!(clock.now_ns(), 3_500_000);
}

#[test]
fn multiple_streams_isolated_under_simulation() {
    let clock = ClockAdapter::new(SimClock::new());
    let storage = StorageAdapter::new_reliable();
    let network = NetworkAdapter::new(
        SimNetwork::new(NetworkConfig::reliable()),
        ReplicaId::new(0),
        TenantId::new(1),
    );

    let mut runtime = Runtime::new(clock, storage, network);
    let mut state = State::new();

    // Create two streams
    for stream_id in [1, 2] {
        let (new_state, effects) = apply_committed(
            state,
            Command::CreateStream {
                stream_id: StreamId::new(stream_id),
                stream_name: StreamName::new(format!("stream-{}", stream_id)),
                data_class: DataClass::NonPHI,
                placement: Placement::Global,
            },
        )
        .unwrap();

        runtime.execute_effects(effects).unwrap();
        state = new_state;
    }

    // Append to stream 1
    let (state, effects) = apply_committed(
        state,
        Command::AppendBatch {
            stream_id: StreamId::new(1),
            events: vec![Bytes::from("stream1-event1")],
            expected_offset: Offset::ZERO,
        },
    )
    .unwrap();
    runtime.execute_effects(effects).unwrap();

    // Append to stream 2
    let (state, effects) = apply_committed(
        state,
        Command::AppendBatch {
            stream_id: StreamId::new(2),
            events: vec![Bytes::from("stream2-event1"), Bytes::from("stream2-event2")],
            expected_offset: Offset::ZERO,
        },
    )
    .unwrap();
    runtime.execute_effects(effects).unwrap();

    // Verify streams are independent
    let stream1 = state.get_stream(&StreamId::new(1)).unwrap();
    let stream2 = state.get_stream(&StreamId::new(2)).unwrap();

    assert_eq!(stream1.current_offset, Offset::from(1u64));
    assert_eq!(stream2.current_offset, Offset::from(2u64));

    // Verify storage has correct data
    let events1 = runtime
        .storage()
        .read(StreamId::new(1), Offset::ZERO, Offset::from(1u64))
        .unwrap();
    let events2 = runtime
        .storage()
        .read(StreamId::new(2), Offset::ZERO, Offset::from(2u64))
        .unwrap();

    assert_eq!(events1.len(), 1);
    assert_eq!(events2.len(), 2);
}

#[test]
fn storage_stats_tracked_under_simulation() {
    let clock = ClockAdapter::new(SimClock::new());
    let storage = StorageAdapter::new_reliable();
    let network = NetworkAdapter::new(
        SimNetwork::new(NetworkConfig::reliable()),
        ReplicaId::new(0),
        TenantId::new(1),
    );

    let mut runtime = Runtime::new(clock, storage, network);

    // Create and append to stream
    let state = State::new();
    let (state, effects) = apply_committed(
        state,
        Command::CreateStream {
            stream_id: StreamId::new(1),
            stream_name: StreamName::new("metrics-test"),
            data_class: DataClass::NonPHI,
            placement: Placement::Global,
        },
    )
    .unwrap();
    runtime.execute_effects(effects).unwrap();

    let (_, effects) = apply_committed(
        state,
        Command::AppendBatch {
            stream_id: StreamId::new(1),
            events: vec![Bytes::from("test event data")],
            expected_offset: Offset::ZERO,
        },
    )
    .unwrap();
    runtime.execute_effects(effects).unwrap();

    // Check stats
    let stats = runtime.storage().stats();
    assert_eq!(stats.corruption_errors, 0);
}
