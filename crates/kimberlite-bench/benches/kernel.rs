//! Kernel state machine benchmarks.
//!
//! Benchmarks state transitions for the pure functional kernel.

#![allow(clippy::cast_sign_loss)] // Benchmark code uses many numeric conversions

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use kimberlite_kernel::command::{ColumnDefinition, TableId};
use kimberlite_kernel::{Command, State, apply_committed};
use kimberlite_types::{DataClass, Offset, Placement, StreamId, StreamName};

// ============================================================================
// Stream Command Benchmarks
// ============================================================================

fn bench_create_stream(c: &mut Criterion) {
    let mut group = c.benchmark_group("kernel_create_stream");

    group.bench_function("create_stream", |b| {
        b.iter(|| {
            let state = State::new();
            let cmd = Command::CreateStream {
                stream_id: StreamId::new(1),
                stream_name: StreamName::new("test_stream"),
                data_class: DataClass::Public,
                placement: Placement::Global,
            };

            let result = apply_committed(black_box(state), black_box(cmd));
            let _ = black_box(result);
        });
    });

    group.finish();
}

fn bench_append_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("kernel_append_batch");

    for batch_size in [1, 10, 50, 100] {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch_size,
            |b, &batch_size| {
                b.iter_batched(
                    || {
                        // Setup: create stream with state
                        let state = State::new();
                        let cmd = Command::CreateStream {
                            stream_id: StreamId::new(1),
                            stream_name: StreamName::new("test_stream"),
                            data_class: DataClass::Public,
                            placement: Placement::Global,
                        };
                        let (state, _effects) = apply_committed(state, cmd).unwrap();

                        let events: Vec<Bytes> = (0..batch_size)
                            .map(|_| Bytes::from(vec![0u8; 256]))
                            .collect();
                        (state, events)
                    },
                    |(state, events)| {
                        let cmd = Command::AppendBatch {
                            stream_id: StreamId::new(1),
                            events: black_box(events),
                            expected_offset: Offset::ZERO,
                        };

                        let result = apply_committed(black_box(state), black_box(cmd));
                        let _ = black_box(result);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

// ============================================================================
// Table Command Benchmarks
// ============================================================================

fn bench_create_table(c: &mut Criterion) {
    let mut group = c.benchmark_group("kernel_create_table");

    group.bench_function("create_table", |b| {
        b.iter(|| {
            let state = State::new();
            let cmd = Command::CreateTable {
                table_id: TableId::new(1),
                table_name: "test_table".to_string(),
                columns: vec![
                    ColumnDefinition {
                        name: "id".to_string(),
                        data_type: "BIGINT".to_string(),
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

            let result = apply_committed(black_box(state), black_box(cmd));
            let _ = black_box(result);
        });
    });

    group.finish();
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("kernel_insert");

    group.bench_function("insert_row", |b| {
        b.iter_batched(
            || {
                // Setup: create table
                let state = State::new();
                let cmd = Command::CreateTable {
                    table_id: TableId::new(1),
                    table_name: "test_table".to_string(),
                    columns: vec![
                        ColumnDefinition {
                            name: "id".to_string(),
                            data_type: "BIGINT".to_string(),
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
                let (state, _effects) = apply_committed(state, cmd).unwrap();
                state
            },
            |state| {
                let cmd = Command::Insert {
                    table_id: TableId::new(1),
                    row_data: Bytes::from(vec![1, 2, 3, 4]),
                };

                let result = apply_committed(black_box(state), black_box(cmd));
                let _ = black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================================
// State Cloning Benchmarks
// ============================================================================

fn bench_state_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("kernel_state");

    // Create a state with some streams
    let mut state = State::new();
    for i in 0..10 {
        let cmd = Command::CreateStream {
            stream_id: StreamId::new(i),
            stream_name: StreamName::new(format!("stream_{i}")),
            data_class: DataClass::Public,
            placement: Placement::Global,
        };
        let (new_state, _) = apply_committed(state, cmd).unwrap();
        state = new_state;
    }

    group.bench_function("state_clone", |b| {
        b.iter(|| {
            let cloned = black_box(state.clone());
            black_box(cloned);
        });
    });

    group.bench_function("state_query_stream", |b| {
        b.iter(|| {
            let stream = state.get_stream(black_box(&StreamId::new(5)));
            black_box(stream);
        });
    });

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    kernel_benches,
    bench_create_stream,
    bench_append_batch,
    bench_create_table,
    bench_insert,
    bench_state_operations
);

criterion_main!(kernel_benches);
