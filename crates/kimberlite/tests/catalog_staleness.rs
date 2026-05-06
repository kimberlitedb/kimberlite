//! ROADMAP v0.7.0 — catalog-staleness reproducer.
//!
//! AUDIT-2026-05 S3.6 — closes the v0.6.2-deferred bug where
//! recreating a table by the same name within a single connection
//! left stale planner state, surfacing as `QueryParseError: SQL
//! syntax error` on a parameter-bound INSERT into the recreated
//! table.
//!
//! Root cause: `Effect::TableMetadataDrop` did not call
//! `rebuild_query_engine_schema()`, leaving the per-tenant
//! `QueryEngine` cache holding the dropped table's `TableDef`.
//! The next `CREATE TABLE` (which hashes back to the same
//! `TableId` because table_id is `hash(tenant, name)`) re-emitted
//! `TableMetadataWrite` and rebuilt — but only because the create
//! path triggered the rebuild. Any read between the DROP and the
//! CREATE saw stale schema.
//!
//! These tests pin the symmetry: DROP rebuilds, CREATE rebuilds,
//! and the documented reproducer (DROP → CREATE → parameter-bound
//! INSERT) succeeds end-to-end.

use kimberlite::{Kimberlite, TenantId, Value};

fn open() -> (tempfile::TempDir, Kimberlite, kimberlite::TenantHandle) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open db");
    let tenant = db.tenant(TenantId::new(0xCAFE));
    (dir, db, tenant)
}

#[test]
fn drop_create_insert_same_name_succeeds() {
    let (_dir, _db, tenant) = open();

    tenant
        .execute(
            "CREATE TABLE notes (id BIGINT PRIMARY KEY, body TEXT NOT NULL)",
            &[],
        )
        .expect("initial CREATE TABLE");
    tenant
        .execute(
            "INSERT INTO notes (id, body) VALUES ($1, $2)",
            &[Value::BigInt(1), Value::Text("first".into())],
        )
        .expect("insert pre-drop");

    tenant
        .execute("DROP TABLE notes", &[])
        .expect("DROP TABLE notes");

    tenant
        .execute(
            "CREATE TABLE notes (id BIGINT PRIMARY KEY, body TEXT NOT NULL)",
            &[],
        )
        .expect("CREATE TABLE same name post-drop");

    // The exact reproducer from ROADMAP v0.7.0:
    //   DROP TABLE t; CREATE TABLE t (...); INSERT INTO t (...) VALUES ($1, ...)
    // Pre-fix this returned `QueryParseError` via stale planner
    // state — the per-tenant `QueryEngine` cache still pointed
    // at the dropped `TableDef`. Post-fix the parameter-bound
    // INSERT plans against the fresh schema and round-trips.
    //
    // NOTE: we use a fresh PK (id=2) here to keep this test focused
    // on the catalog-cache bug rather than the data-purge path.
    // DROP TABLE now also purges projection rows (v0.8.0); the
    // dedicated regression net for that is
    // `drop_table_purges_projection_rows` below.
    tenant
        .execute(
            "INSERT INTO notes (id, body) VALUES ($1, $2)",
            &[Value::BigInt(2), Value::Text("after-recreate".into())],
        )
        .expect("parameter-bound INSERT into recreated table");

    let rows = tenant
        .query(
            "SELECT id, body FROM notes WHERE id = $1",
            &[Value::BigInt(2)],
        )
        .expect("query post-recreate");
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0][0], Value::BigInt(2));
    assert_eq!(rows.rows[0][1], Value::Text("after-recreate".into()));
}

#[test]
fn drop_table_purges_projection_rows() {
    // v0.8.0: DROP TABLE now emits `Effect::ProjectionRowsPurge`
    // alongside `Effect::TableMetadataDrop`. The recreated table
    // starts empty; the previously-`#[ignore]`d regression net
    // (`drop_does_not_yet_purge_projection_rows`) is now a live
    // correctness assertion.
    let (_dir, _db, tenant) = open();
    tenant
        .execute(
            "CREATE TABLE persists (id BIGINT PRIMARY KEY, n BIGINT)",
            &[],
        )
        .expect("create");
    tenant
        .execute(
            "INSERT INTO persists (id, n) VALUES ($1, $2)",
            &[Value::BigInt(1), Value::BigInt(99)],
        )
        .expect("insert");
    tenant.execute("DROP TABLE persists", &[]).expect("drop");
    tenant
        .execute(
            "CREATE TABLE persists (id BIGINT PRIMARY KEY, n BIGINT)",
            &[],
        )
        .expect("recreate");
    // What the user expects: recreated table is empty.
    let rows = tenant
        .query("SELECT id FROM persists", &[])
        .expect("select");
    assert_eq!(
        rows.rows.len(),
        0,
        "DROP TABLE must purge projection-store rows for the recreated table to start empty"
    );
}

#[test]
fn drop_then_query_old_name_fails_cleanly() {
    // Belt-and-braces: after DROP the table really is gone — no
    // stale `TableDef` lingering in the cache that would let a
    // SELECT briefly succeed against the dropped schema.
    let (_dir, _db, tenant) = open();

    tenant
        .execute("CREATE TABLE temp_t (id BIGINT PRIMARY KEY)", &[])
        .expect("create");
    tenant.execute("DROP TABLE temp_t", &[]).expect("drop");

    let err = tenant
        .query("SELECT id FROM temp_t", &[])
        .expect_err("query against dropped table must fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("temp_t")
            || msg.to_lowercase().contains("not found")
            || msg.to_lowercase().contains("table"),
        "expected TableNotFound-like error, got: {msg}"
    );
}

#[test]
fn drop_create_drop_create_insert_loop_stays_consistent() {
    // Notebar's integration-test harness loops DDL — the schema
    // cache must stay coherent across many cycles, not just one.
    let (_dir, _db, tenant) = open();

    for iteration in 0..5_u64 {
        tenant
            .execute(
                "CREATE TABLE IF NOT EXISTS cycle_t (id BIGINT PRIMARY KEY, n BIGINT)",
                &[],
            )
            .expect("create-if-not-exists");

        tenant
            .execute(
                "INSERT INTO cycle_t (id, n) VALUES ($1, $2)",
                &[
                    Value::BigInt(iteration as i64),
                    Value::BigInt(iteration as i64 * 10),
                ],
            )
            .expect("parameter-bound insert in iteration");

        tenant
            .execute("DROP TABLE cycle_t", &[])
            .expect("drop-end-of-iteration");
    }
}
