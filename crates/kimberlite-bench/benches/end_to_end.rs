//! End-to-end system throughput benchmarks.
//!
//! Benchmarks full operations from kernel to storage including all layers.

#![allow(clippy::cast_sign_loss)] // Benchmark code uses many numeric conversions
#![allow(clippy::cast_possible_truncation)] // Benchmark conversions between numeric types

use std::hint::black_box;

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use kimberlite_bench::LatencyTracker;
use kimberlite_kernel::{Command, Effect, State, apply_committed};
use kimberlite_storage::Storage;
use kimberlite_types::{DataClass, Offset, Placement, StreamId, StreamName};
use std::time::Instant;
use tempfile::TempDir;

// ============================================================================
// Full Write Path Benchmark
// ============================================================================

fn bench_full_write_path(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_write_path");

    for batch_size in [1, 10, 50] {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch_size,
            |b, &batch_size| {
                b.iter_batched(
                    || {
                        // Setup: create state and storage
                        let temp_dir = TempDir::new().unwrap();
                        let storage = Storage::new(temp_dir.path());
                        let state = State::new();

                        // Create stream
                        let cmd = Command::CreateStream {
                            stream_id: StreamId::new(1),
                            stream_name: StreamName::new("test_stream"),
                            data_class: DataClass::Public,
                            placement: Placement::Global,
                        };
                        let (state, effects) = apply_committed(state, cmd).unwrap();

                        // Execute effects
                        for effect in effects {
                            if let Effect::StreamMetadataWrite(_) = effect {
                                // Would write metadata in real system
                            }
                        }

                        let events: Vec<Bytes> = (0..batch_size)
                            .map(|_| Bytes::from(vec![0u8; 256]))
                            .collect();
                        (state, storage, temp_dir, events)
                    },
                    |(state, mut storage, _temp_dir, events)| {
                        // Apply command to kernel
                        let cmd = Command::AppendBatch {
                            stream_id: StreamId::new(1),
                            events: black_box(events),
                            expected_offset: Offset::ZERO,
                        };
                        let (new_state, effects) =
                            apply_committed(black_box(state), black_box(cmd)).unwrap();
                        black_box(new_state);

                        // Execute storage effects
                        for effect in effects {
                            if let Effect::StorageAppend {
                                stream_id,
                                base_offset,
                                events,
                            } = effect
                            {
                                storage
                                    .append_batch(stream_id, events, base_offset, None, false)
                                    .ok();
                            }
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

// ============================================================================
// Latency Distribution Benchmark
// ============================================================================

fn bench_write_latency_distribution(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_latency_distribution");

    group.bench_function("1000_writes", |b| {
        b.iter_custom(|iters| {
            let temp_dir = TempDir::new().unwrap();
            let mut storage = Storage::new(temp_dir.path());
            let mut state = State::new();

            // Create stream
            let cmd = Command::CreateStream {
                stream_id: StreamId::new(1),
                stream_name: StreamName::new("test_stream"),
                data_class: DataClass::Public,
                placement: Placement::Global,
            };
            let (new_state, _effects) = apply_committed(state, cmd).unwrap();
            state = new_state;

            let mut tracker = LatencyTracker::new();
            let mut total_duration = std::time::Duration::ZERO;

            for i in 0..iters {
                let event = Bytes::from(vec![0u8; 256]);
                let start = Instant::now();

                // Kernel: apply command
                let cmd = Command::AppendBatch {
                    stream_id: StreamId::new(1),
                    events: vec![event],
                    expected_offset: Offset::from(i),
                };
                let (new_state, effects) = apply_committed(state, cmd).unwrap();
                state = new_state;

                // Storage: execute effects
                for effect in effects {
                    if let Effect::StorageAppend {
                        stream_id,
                        base_offset,
                        events,
                    } = effect
                    {
                        storage
                            .append_batch(stream_id, events, base_offset, None, false)
                            .ok();
                    }
                }

                let elapsed = start.elapsed();
                total_duration += elapsed;
                tracker.record(elapsed.as_nanos() as u64);
            }

            // Print latency statistics after benchmark
            if iters >= 1000 {
                eprintln!("\n");
                tracker.print_summary("Write");
            }

            total_duration
        });
    });

    group.finish();
}

// ============================================================================
// Throughput Benchmark
// ============================================================================

fn bench_sustained_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("sustained_throughput");
    group.sample_size(10); // Fewer samples for long-running benchmark

    group.bench_function("10000_writes", |b| {
        b.iter_custom(|_iters| {
            let temp_dir = TempDir::new().unwrap();
            let mut storage = Storage::new(temp_dir.path());
            let mut state = State::new();

            // Create stream
            let cmd = Command::CreateStream {
                stream_id: StreamId::new(1),
                stream_name: StreamName::new("test_stream"),
                data_class: DataClass::Public,
                placement: Placement::Global,
            };
            let (new_state, _effects) = apply_committed(state, cmd).unwrap();
            state = new_state;

            let start = Instant::now();
            let iterations = 10_000;

            for i in 0..iterations {
                let event = Bytes::from(vec![0u8; 256]);
                let should_fsync = i % 100 == 0;

                // Kernel
                let cmd = Command::AppendBatch {
                    stream_id: StreamId::new(1),
                    events: vec![event],
                    expected_offset: Offset::from(i as u64),
                };
                let (new_state, effects) = apply_committed(state, cmd).unwrap();
                state = new_state;

                // Storage (fsync every 100 writes)
                for effect in effects {
                    if let Effect::StorageAppend {
                        stream_id,
                        base_offset,
                        events,
                    } = effect
                    {
                        storage
                            .append_batch(stream_id, events, base_offset, None, should_fsync)
                            .ok();
                    }
                }
            }

            let elapsed = start.elapsed();
            let ops_per_sec = f64::from(iterations) / elapsed.as_secs_f64();
            eprintln!("\nThroughput: {ops_per_sec:.0} ops/sec");

            elapsed
        });
    });

    group.finish();
}

// ============================================================================
// Little's Law Validation Benchmark
// ============================================================================

/// Validates Little's Law: L = λ × W
///
/// Measures throughput (λ) and latency (W) under sustained load, computes
/// the implied concurrency (L), and reports whether it fits within the
/// VSR event loop channel bounds (typically 1000 in production).
///
/// This helps ensure queue sizes are correctly dimensioned: if L > `channel_bound`,
/// requests will be dropped or blocked under sustained load.
fn bench_littles_law_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("littles_law");
    group.sample_size(10);

    group.bench_function("concurrency_validation", |b| {
        b.iter_custom(|_iters| {
            let temp_dir = TempDir::new().unwrap();
            let mut storage = Storage::new(temp_dir.path());
            let mut state = State::new();

            // Create stream
            let cmd = Command::CreateStream {
                stream_id: StreamId::new(1),
                stream_name: StreamName::new("littles_law_stream"),
                data_class: DataClass::Public,
                placement: Placement::Global,
            };
            let (new_state, _effects) = apply_committed(state, cmd).unwrap();
            state = new_state;

            let iterations: u64 = 5_000;
            let mut tracker = LatencyTracker::new();

            let wall_start = Instant::now();

            for i in 0..iterations {
                let event = Bytes::from(vec![0u8; 256]);
                let op_start = Instant::now();

                let cmd = Command::AppendBatch {
                    stream_id: StreamId::new(1),
                    events: vec![event],
                    expected_offset: Offset::from(i),
                };
                let (new_state, effects) = apply_committed(state, cmd).unwrap();
                state = new_state;

                for effect in effects {
                    if let Effect::StorageAppend {
                        stream_id,
                        base_offset,
                        events,
                    } = effect
                    {
                        storage
                            .append_batch(stream_id, events, base_offset, None, false)
                            .ok();
                    }
                }

                tracker.record(op_start.elapsed().as_nanos() as u64);
            }

            let wall_elapsed = wall_start.elapsed();
            let throughput = iterations as f64 / wall_elapsed.as_secs_f64(); // λ (ops/sec)
            let mean_latency_sec = tracker.mean() / 1_000_000_000.0; // W (seconds)
            let implied_concurrency = throughput * mean_latency_sec; // L = λ × W

            // VSR event loop channel bound (production default)
            let channel_bound = 1000;

            eprintln!("\n--- Little's Law Validation ---");
            eprintln!("  Throughput (λ):          {throughput:.0} ops/sec");
            eprintln!("  Mean latency (W):        {:.2} μs", tracker.mean() / 1000.0);
            eprintln!("  p99 latency:             {:.2} μs", tracker.p99() as f64 / 1000.0);
            eprintln!("  Implied concurrency (L): {implied_concurrency:.2}");
            eprintln!("  Channel bound:           {channel_bound}");
            eprintln!(
                "  Headroom:                {:.1}x",
                f64::from(channel_bound) / implied_concurrency.max(1.0)
            );

            // For single-threaded synchronous operations, L should be ~1.0
            // If L approaches the channel bound, queue sizing needs adjustment
            assert!(
                implied_concurrency < f64::from(channel_bound),
                "implied concurrency ({implied_concurrency:.1}) exceeds channel bound ({channel_bound})"
            );

            wall_elapsed
        });
    });

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    end_to_end_benches,
    bench_full_write_path,
    bench_write_latency_distribution,
    bench_sustained_throughput,
    bench_littles_law_validation
);

criterion_main!(end_to_end_benches);
