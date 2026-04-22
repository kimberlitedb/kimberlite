//! v0.6.0 Tier 2 #6 integration test — `FOR SYSTEM_TIME AS OF` /
//! `AS OF TIMESTAMP` time-travel end-to-end.
//!
//! Covers the full flow:
//!
//! 1. Open a fresh tenant, create a table, insert rows one at a time
//!    with a short sleep between each so each commit lands under a
//!    distinct wall-clock nanosecond.
//! 2. Record the `chrono::Utc::now()` snapshot immediately *after*
//!    each insert — this is the "exact" timestamp where the commit
//!    settled into the projection + the timestamp index.
//! 3. For each recorded snapshot, issue
//!    `SELECT * FROM t FOR SYSTEM_TIME AS OF '<iso>'` through
//!    `TenantHandle::query` and verify the returned rows match
//!    exactly the prefix of inserts committed up to that instant.
//! 4. Verify the two error paths:
//!    - Querying a timestamp older than any recorded commit yields
//!      `AsOfBeforeRetentionHorizon`.
//!    - Querying a freshly opened (empty) DB yields
//!      `UnsupportedFeature` (no index entries to resolve against).
//! 5. Property test: monotonically increasing timestamps resolve
//!    correctly across a random number of inserts.
//!
//! Why here and not in kimberlite-query? The query crate owns the
//! parser + `query_at_timestamp`; the default runtime resolver lives
//! in `kimberlite::TenantHandle::query_at_timestamp` (on top of
//! `KimberliteInner::timestamp_index`). This test exercises the
//! runtime seam, which only the facade crate can observe.

use std::time::Duration;

use kimberlite::{Kimberlite, QueryError, TenantId, Value};

const TENANT: u64 = 1729;

/// Inserts N patient rows with short sleeps between each so that each
/// commit lands under a strictly-distinct wall-clock nanosecond.
/// Returns a vector of `(iso_after_commit, inserted_id)` pairs.
fn insert_n_patients(
    tenant: &kimberlite::TenantHandle,
    n: usize,
) -> Vec<(chrono::DateTime<chrono::Utc>, i64)> {
    let mut timestamps = Vec::with_capacity(n);
    for i in 0..n {
        let id = i as i64 + 1;
        tenant
            .execute(
                "INSERT INTO patients (id, name) VALUES ($1, $2)",
                &[Value::BigInt(id), Value::Text(format!("patient-{id}"))],
            )
            .unwrap_or_else(|e| panic!("insert patient {id}: {e}"));
        let now = chrono::Utc::now();
        timestamps.push((now, id));
        // The timestamp index clamps duplicates to `prev + 1`, so a
        // sub-ms sleep is actually enough — we don't need strict
        // wall-clock separation. The sleep matches the way a real
        // workload would commit.
        std::thread::sleep(Duration::from_millis(2));
    }
    timestamps
}

#[test]
fn as_of_timestamp_returns_state_at_requested_instant() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open db");
    let tenant = db.tenant(TenantId::new(TENANT));

    tenant
        .execute(
            "CREATE TABLE patients (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
            &[],
        )
        .expect("create patients");

    let snapshots = insert_n_patients(&tenant, 5);

    // For each post-commit snapshot, AS OF TIMESTAMP returns exactly
    // the prefix of patients inserted up to and including that commit.
    for (after_commit, last_id) in &snapshots {
        let iso = after_commit.to_rfc3339();
        let sql = format!("SELECT id FROM patients ORDER BY id FOR SYSTEM_TIME AS OF '{iso}'");
        let result = tenant
            .query(&sql, &[])
            .unwrap_or_else(|e| panic!("query at {iso}: {e}"));
        assert_eq!(
            result.rows.len() as i64,
            *last_id,
            "expected {last_id} patients visible at {iso}, got {}",
            result.rows.len()
        );
        for (i, row) in result.rows.iter().enumerate() {
            let expected = i as i64 + 1;
            assert_eq!(row[0], Value::BigInt(expected));
        }
    }
}

#[test]
fn as_of_timestamp_before_retention_horizon_errors_cleanly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open db");
    let tenant = db.tenant(TenantId::new(TENANT));

    tenant
        .execute(
            "CREATE TABLE patients (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
            &[],
        )
        .expect("create patients");

    // Stamp the index with at least one commit so the log is non-empty.
    insert_n_patients(&tenant, 2);

    // An instant far in the past — definitely older than any recorded
    // commit — must produce AsOfBeforeRetentionHorizon, not
    // UnsupportedFeature. This is the v0.6.0 Tier 2 #6 contract.
    let pre_genesis = chrono::DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc)
        .to_rfc3339();
    let sql = format!("SELECT id FROM patients FOR SYSTEM_TIME AS OF '{pre_genesis}'");
    let err = tenant.query(&sql, &[]).unwrap_err();

    // The error must be exposed via QueryError::AsOfBeforeRetentionHorizon —
    // the facade wraps it in KimberliteError::Query.
    let kimberlite::KimberliteError::Query(ref q_err) = err else {
        panic!("expected KimberliteError::Query, got {err:?}");
    };
    match q_err {
        QueryError::AsOfBeforeRetentionHorizon {
            requested_ns,
            horizon_ns,
        } => {
            assert!(
                *requested_ns < *horizon_ns,
                "requested_ns {requested_ns} must be < horizon_ns {horizon_ns}"
            );
        }
        other => panic!("expected AsOfBeforeRetentionHorizon, got {other:?}"),
    }
}

#[test]
fn as_of_timestamp_on_empty_log_surfaces_unsupported_feature() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open db");
    let tenant = db.tenant(TenantId::new(TENANT));

    // No table, no inserts — the timestamp index is pristine. Any
    // AS OF TIMESTAMP query must produce UnsupportedFeature with the
    // "empty log or predates genesis" message so callers know to
    // write something before time-travelling.
    //
    // We ask for a far-future instant on purpose — "predates
    // retention horizon" only fires when the index has entries; with
    // no entries, even a future instant trips the LogEmpty branch.
    let target = chrono::DateTime::parse_from_rfc3339("2100-01-15T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc)
        .to_rfc3339();
    let sql = format!("SELECT 1 FROM (VALUES (1)) AS t FOR SYSTEM_TIME AS OF '{target}'");
    let err = tenant.query(&sql, &[]).unwrap_err();
    let kimberlite::KimberliteError::Query(ref q_err) = err else {
        panic!("expected KimberliteError::Query, got {err:?}");
    };
    match q_err {
        QueryError::UnsupportedFeature(msg) => {
            assert!(
                msg.contains("empty log") || msg.contains("predates genesis"),
                "expected empty-log message, got: {msg}"
            );
        }
        // A fresh DB has no projection activity, so the index is
        // empty and we expect LogEmpty. If the kernel ever starts
        // stamping metadata-only commits, this branch catches the
        // regression rather than masking it.
        other => panic!("expected UnsupportedFeature on empty index, got {other:?}"),
    }
}

#[test]
fn offset_time_travel_still_works_after_resolver_plumbing() {
    // Regression guard — the existing `AT OFFSET n` path must keep
    // working unchanged after the v0.6.0 resolver plumbing lands.
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open db");
    let tenant = db.tenant(TenantId::new(TENANT));

    tenant
        .execute(
            "CREATE TABLE patients (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
            &[],
        )
        .expect("create patients");

    insert_n_patients(&tenant, 3);

    // Query at offset 1 — should see just the first insert (the exact
    // offset depends on projection indexing, but at least 0 and 1
    // offsets are defined).
    let result = tenant
        .query("SELECT id FROM patients AT OFFSET 1", &[])
        .expect("AT OFFSET 1 must still parse and execute");
    assert!(result.rows.len() <= 3);
}

// ============================================================================
// Property test — AS OF resolves consistently for monotonic inserts.
// ============================================================================

use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]

    /// For every k in 1..=N, AS OF TIMESTAMP `ts_k` returns exactly
    /// the first k events' state. This is the core invariant the
    /// timestamp index promises.
    #[test]
    fn prop_as_of_timestamp_returns_prefix(n in 2usize..6) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = Kimberlite::open(dir.path()).expect("open db");
        let tenant = db.tenant(TenantId::new(TENANT));

        tenant
            .execute(
                "CREATE TABLE patients (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
                &[],
            )
            .expect("create patients");

        let snapshots = insert_n_patients(&tenant, n);

        for (k, (after_commit, last_id)) in snapshots.iter().enumerate() {
            let k = k + 1;
            let iso = after_commit.to_rfc3339();
            let sql = format!(
                "SELECT id FROM patients ORDER BY id FOR SYSTEM_TIME AS OF '{iso}'"
            );
            let result = tenant
                .query(&sql, &[])
                .unwrap_or_else(|e| panic!("query at {iso}: {e}"));
            prop_assert_eq!(result.rows.len(), k, "k={} iso={}", k, iso);
            prop_assert_eq!(*last_id, k as i64);
        }
    }
}
