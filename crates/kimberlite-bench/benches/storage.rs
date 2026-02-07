//! Storage layer benchmarks.
//!
//! Benchmarks write, read, and fsync operations for the storage layer.

#![allow(clippy::cast_sign_loss)] // Benchmark code uses many numeric conversions

use std::hint::black_box;

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use kimberlite_storage::Storage;
use kimberlite_types::{Offset, StreamId};
use tempfile::TempDir;

// ============================================================================
// Write Benchmarks
// ============================================================================

fn bench_storage_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_write");

    for size in [64, 256, 1024, 4096, 16384] {
        group.throughput(Throughput::Bytes(size as u64));
        let data = vec![0u8; size];

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter_batched(
                || {
                    let temp_dir = TempDir::new().unwrap();
                    let storage = Storage::new(temp_dir.path());
                    (storage, temp_dir)
                },
                |(mut storage, _temp_dir)| {
                    let stream_id = StreamId::new(1);
                    let result = storage.append_batch(
                        black_box(stream_id),
                        black_box(vec![Bytes::from(data.clone())]),
                        black_box(Offset::ZERO),
                        black_box(None),
                        black_box(false),
                    );
                    let _ = black_box(result);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// ============================================================================
// Read Benchmarks
// ============================================================================

fn bench_storage_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_read");

    for size in [64, 256, 1024, 4096, 16384] {
        group.throughput(Throughput::Bytes(size as u64));
        let data = vec![0u8; size];

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter_batched(
                || {
                    let temp_dir = TempDir::new().unwrap();
                    let mut storage = Storage::new(temp_dir.path());
                    let stream_id = StreamId::new(1);

                    // Write data first
                    storage
                        .append_batch(
                            stream_id,
                            vec![Bytes::from(data.clone())],
                            Offset::ZERO,
                            None,
                            true,
                        )
                        .unwrap();

                    (storage, temp_dir)
                },
                |(mut storage, _temp_dir)| {
                    let stream_id = StreamId::new(1);
                    let result = storage.read_from(
                        black_box(stream_id),
                        black_box(Offset::ZERO),
                        black_box(size as u64),
                    );
                    let _ = black_box(result);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// ============================================================================
// Fsync Benchmarks
// ============================================================================

fn bench_storage_fsync(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_fsync");

    // Benchmark write with fsync vs without fsync
    group.bench_function("write_with_fsync", |b| {
        b.iter_batched(
            || {
                let temp_dir = TempDir::new().unwrap();
                let storage = Storage::new(temp_dir.path());
                (storage, temp_dir)
            },
            |(mut storage, _temp_dir)| {
                let stream_id = StreamId::new(1);
                let data = vec![Bytes::from(vec![0u8; 1024])];
                let result = storage.append_batch(stream_id, data, Offset::ZERO, None, true);
                let _ = black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("write_without_fsync", |b| {
        b.iter_batched(
            || {
                let temp_dir = TempDir::new().unwrap();
                let storage = Storage::new(temp_dir.path());
                (storage, temp_dir)
            },
            |(mut storage, _temp_dir)| {
                let stream_id = StreamId::new(1);
                let data = vec![Bytes::from(vec![0u8; 1024])];
                let result = storage.append_batch(stream_id, data, Offset::ZERO, None, false);
                let _ = black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================================
// Batch Write Benchmarks
// ============================================================================

fn bench_storage_batch_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_batch_write");

    for batch_size in [10, 50, 100, 500] {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch_size,
            |b, &batch_size| {
                b.iter_batched(
                    || {
                        let temp_dir = TempDir::new().unwrap();
                        let storage = Storage::new(temp_dir.path());
                        let events: Vec<Bytes> = (0..batch_size)
                            .map(|_| Bytes::from(vec![0u8; 256]))
                            .collect();
                        (storage, temp_dir, events)
                    },
                    |(mut storage, _temp_dir, events)| {
                        let stream_id = StreamId::new(1);
                        let result = storage.append_batch(
                            black_box(stream_id),
                            black_box(events),
                            black_box(Offset::ZERO),
                            black_box(None),
                            black_box(false),
                        );
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
// Sequential Read Benchmarks
// ============================================================================

fn bench_storage_sequential_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_sequential_read");

    for count in [10, 50, 100, 500] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter_batched(
                || {
                    let temp_dir = TempDir::new().unwrap();
                    let mut storage = Storage::new(temp_dir.path());
                    let stream_id = StreamId::new(1);

                    // Write test data
                    let events: Vec<Bytes> =
                        (0..count).map(|_| Bytes::from(vec![0u8; 256])).collect();
                    storage
                        .append_batch(stream_id, events, Offset::ZERO, None, true)
                        .unwrap();

                    (storage, temp_dir)
                },
                |(mut storage, _temp_dir)| {
                    let stream_id = StreamId::new(1);
                    let max_bytes = (count as u64) * 256 + 1024; // Rough estimate with overhead
                    let result = storage.read_from(
                        black_box(stream_id),
                        black_box(Offset::ZERO),
                        black_box(max_bytes),
                    );
                    let _ = black_box(result);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    storage_benches,
    bench_storage_write,
    bench_storage_read,
    bench_storage_fsync,
    bench_storage_batch_write,
    bench_storage_sequential_read
);

criterion_main!(storage_benches);
