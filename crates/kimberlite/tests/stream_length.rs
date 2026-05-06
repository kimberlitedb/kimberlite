//! Integration tests for the v0.8.0 `stream_length` primitive.
//!
//! `TenantHandle::stream_length` reads `StreamMetadata.current_offset`
//! directly off kernel state — O(1), no full-stream walk. These tests
//! pin the contract:
//! - empty stream → `0`
//! - after N appends → `N`
//! - unknown stream → `StreamNotFound`

use kimberlite::{Kimberlite, TenantId};
use kimberlite_types::{DataClass, Offset};

fn open() -> (tempfile::TempDir, Kimberlite, kimberlite::TenantHandle) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open db");
    let tenant = db.tenant(TenantId::new(0x00C0_FFEE));
    (dir, db, tenant)
}

#[test]
fn stream_length_returns_zero_for_empty_stream() {
    let (_dir, _db, tenant) = open();
    let stream_id = tenant
        .create_stream("empty", DataClass::Public)
        .expect("create_stream");

    assert_eq!(
        tenant.stream_length(stream_id).expect("stream_length"),
        0,
        "freshly-created stream must report length 0"
    );
}

#[test]
fn stream_length_matches_event_count_after_appends() {
    let (_dir, _db, tenant) = open();
    let stream_id = tenant
        .create_stream("events", DataClass::Public)
        .expect("create_stream");

    // Append five single-event batches; current_offset advances by one
    // per event.
    for i in 0..5_u64 {
        tenant
            .append(
                stream_id,
                vec![format!("event-{i}").into_bytes()],
                Offset::new(i),
            )
            .expect("append");
    }

    assert_eq!(
        tenant.stream_length(stream_id).expect("stream_length"),
        5,
        "five appends → length 5"
    );

    // Append a batch of three events at once. current_offset should
    // advance by the batch size, not by one.
    tenant
        .append(
            stream_id,
            vec![
                b"batch-a".to_vec(),
                b"batch-b".to_vec(),
                b"batch-c".to_vec(),
            ],
            Offset::new(5),
        )
        .expect("append batch");

    assert_eq!(
        tenant.stream_length(stream_id).expect("stream_length"),
        8,
        "five-singles plus three-batch → length 8"
    );
}

#[test]
fn stream_length_unknown_stream_errors() {
    let (_dir, _db, tenant) = open();
    let phantom = kimberlite_types::StreamId::from_tenant_and_local(tenant.tenant_id(), 9999);
    let err = tenant
        .stream_length(phantom)
        .expect_err("unknown stream must error");
    assert!(
        matches!(err, kimberlite::KimberliteError::StreamNotFound(_)),
        "expected StreamNotFound, got {err:?}"
    );
}
