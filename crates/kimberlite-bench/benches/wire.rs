//! Wire protocol serialization benchmarks.
//!
//! Benchmarks encoding and decoding of protocol messages.

use bytes::{Bytes, BytesMut};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use kimberlite_types::{DataClass, Offset, Placement, StreamId, TenantId};
use kimberlite_wire::{
    AppendEventsRequest, CreateStreamRequest, Frame, QueryParam, QueryRequest, ReadEventsRequest,
    Request, RequestId, RequestPayload,
};

// ============================================================================
// Frame Encoding/Decoding Benchmarks
// ============================================================================

fn bench_frame_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_encode");

    for size in [64, 256, 1024, 4096, 16384] {
        group.throughput(Throughput::Bytes(size as u64));
        let payload = Bytes::from(vec![0u8; size]);

        group.bench_with_input(BenchmarkId::from_parameter(size), &payload, |b, payload| {
            b.iter(|| {
                let frame = Frame::new(black_box(payload.clone()));
                let mut buf = BytesMut::new();
                frame.encode(black_box(&mut buf));
                black_box(buf);
            });
        });
    }

    group.finish();
}

fn bench_frame_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_decode");

    for size in [64, 256, 1024, 4096, 16384] {
        group.throughput(Throughput::Bytes(size as u64));
        let payload = Bytes::from(vec![0u8; size]);
        let frame = Frame::new(payload);
        let encoded = frame.encode_to_bytes();

        group.bench_with_input(BenchmarkId::from_parameter(size), &encoded, |b, encoded| {
            b.iter(|| {
                let mut buf = BytesMut::from(&encoded[..]);
                let result = Frame::decode(black_box(&mut buf));
                let _ = black_box(result);
            });
        });
    }

    group.finish();
}

// ============================================================================
// Request Serialization Benchmarks
// ============================================================================

fn bench_request_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("request_serialize");

    // CreateStream request
    group.bench_function("create_stream", |b| {
        let request = Request::new(
            RequestId::new(1),
            TenantId::new(1),
            RequestPayload::CreateStream(CreateStreamRequest {
                name: "test_stream".to_string(),
                data_class: DataClass::NonPHI,
                placement: Placement::Global,
            }),
        );

        b.iter(|| {
            let result = request.to_frame();
            let _ = black_box(result);
        });
    });

    // AppendEvents request with varying event counts
    for event_count in [1, 10, 50, 100] {
        group.throughput(Throughput::Elements(event_count as u64));

        group.bench_with_input(
            BenchmarkId::new("append_events", event_count),
            &event_count,
            |b, &event_count| {
                let events: Vec<_> = (0..event_count).map(|_| vec![0u8; 256]).collect();
                let request = Request::new(
                    RequestId::new(1),
                    TenantId::new(1),
                    RequestPayload::AppendEvents(AppendEventsRequest {
                        stream_id: StreamId::new(1),
                        events,
                    }),
                );

                b.iter(|| {
                    let result = request.to_frame();
                    let _ = black_box(result);
                });
            },
        );
    }

    // Query request
    group.bench_function("query", |b| {
        let request = Request::new(
            RequestId::new(1),
            TenantId::new(1),
            RequestPayload::Query(QueryRequest {
                sql: "SELECT * FROM test_table WHERE id = ?".to_string(),
                params: vec![QueryParam::BigInt(42)],
            }),
        );

        b.iter(|| {
            let result = request.to_frame();
            let _ = black_box(result);
        });
    });

    // ReadEvents request
    group.bench_function("read_events", |b| {
        let request = Request::new(
            RequestId::new(1),
            TenantId::new(1),
            RequestPayload::ReadEvents(ReadEventsRequest {
                stream_id: StreamId::new(1),
                from_offset: Offset::ZERO,
                max_bytes: 1024 * 1024,
            }),
        );

        b.iter(|| {
            let result = request.to_frame();
            let _ = black_box(result);
        });
    });

    group.finish();
}

fn bench_request_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("request_deserialize");

    // CreateStream request
    group.bench_function("create_stream", |b| {
        let request = Request::new(
            RequestId::new(1),
            TenantId::new(1),
            RequestPayload::CreateStream(CreateStreamRequest {
                name: "test_stream".to_string(),
                data_class: DataClass::NonPHI,
                placement: Placement::Global,
            }),
        );
        let frame = request.to_frame().unwrap();

        b.iter(|| {
            let result = Request::from_frame(black_box(&frame));
            let _ = black_box(result);
        });
    });

    // AppendEvents request
    for event_count in [1, 10, 50, 100] {
        group.throughput(Throughput::Elements(event_count as u64));

        group.bench_with_input(
            BenchmarkId::new("append_events", event_count),
            &event_count,
            |b, &event_count| {
                let events: Vec<_> = (0..event_count).map(|_| vec![0u8; 256]).collect();
                let request = Request::new(
                    RequestId::new(1),
                    TenantId::new(1),
                    RequestPayload::AppendEvents(AppendEventsRequest {
                        stream_id: StreamId::new(1),
                        events,
                    }),
                );
                let frame = request.to_frame().unwrap();

                b.iter(|| {
                    let result = Request::from_frame(black_box(&frame));
                    let _ = black_box(result);
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Round-Trip Benchmarks
// ============================================================================

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    for event_count in [1, 10, 50] {
        group.throughput(Throughput::Elements(event_count as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(event_count),
            &event_count,
            |b, &event_count| {
                let events: Vec<_> = (0..event_count).map(|_| vec![0u8; 256]).collect();

                b.iter(|| {
                    // Encode
                    let request = Request::new(
                        RequestId::new(1),
                        TenantId::new(1),
                        RequestPayload::AppendEvents(AppendEventsRequest {
                            stream_id: StreamId::new(1),
                            events: black_box(events.clone()),
                        }),
                    );
                    let frame = request.to_frame().unwrap();

                    // Decode
                    let decoded = Request::from_frame(&frame).unwrap();
                    black_box(decoded);
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    wire_benches,
    bench_frame_encode,
    bench_frame_decode,
    bench_request_serialize,
    bench_request_deserialize,
    bench_roundtrip
);

criterion_main!(wire_benches);
