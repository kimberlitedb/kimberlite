//! ROADMAP v0.5.0 item B integration test:
//!
//! Drives `ALTER TABLE ... ADD COLUMN` and `ALTER TABLE ... DROP COLUMN`
//! through the full SQL parser → kernel command → state machine pipeline
//! and verifies the query engine's projected schema picks up the change
//! on the very next `SELECT` — i.e. that `rebuild_query_engine_schema`
//! fires via the `TableMetadataWrite` effect handler.
//!
//! This is the end-to-end complement to the unit tests in
//! `crates/kimberlite-kernel/src/tests.rs` (`alter_table_*`). Those cover
//! the kernel's schema_version / column_count invariants directly against
//! `apply_committed`; this file exercises the SQL surface Notebar and the
//! vertical example apps actually call.

use kimberlite::{Kimberlite, TenantId, Value};

fn open_with_patients() -> (tempfile::TempDir, Kimberlite, kimberlite::TenantHandle) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open db");
    let tenant = db.tenant(TenantId::new(42));
    tenant
        .execute(
            "CREATE TABLE patients (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
            &[],
        )
        .expect("create patients");
    (dir, db, tenant)
}

#[test]
fn alter_table_add_column_makes_new_column_queryable() {
    let (_dir, _db, tenant) = open_with_patients();

    // Insert a row under the original 2-column schema.
    tenant
        .execute(
            "INSERT INTO patients (id, name) VALUES ($1, $2)",
            &[Value::BigInt(1), Value::Text("Alice".into())],
        )
        .expect("insert pre-alter row");

    // ALTER TABLE ADD COLUMN — this was the Notebar-surfaced gap.
    tenant
        .execute("ALTER TABLE patients ADD COLUMN email TEXT", &[])
        .expect("ALTER TABLE ADD COLUMN must succeed post-v0.5.0");

    // Post-alter: inserting with the new column round-trips end-to-end.
    tenant
        .execute(
            "INSERT INTO patients (id, name, email) VALUES ($1, $2, $3)",
            &[
                Value::BigInt(2),
                Value::Text("Bob".into()),
                Value::Text("bob@example.com".into()),
            ],
        )
        .expect("post-alter insert with new column");

    let result = tenant
        .query("SELECT id, name, email FROM patients ORDER BY id", &[])
        .expect("post-alter select");

    assert_eq!(result.rows.len(), 2, "both rows must be visible");

    // Bob's email round-trips intact.
    let bob_email = &result.rows[1][2];
    assert_eq!(*bob_email, Value::Text("bob@example.com".into()));

    // Alice was inserted before the column existed — her email must be
    // NULL (no backfill; the log is immutable, rows on disk carry the
    // pre-alter shape; the planner materialises NULL on read).
    let alice_email = &result.rows[0][2];
    assert!(
        matches!(alice_email, Value::Null),
        "pre-alter rows must project NULL for the new column, got {alice_email:?}",
    );
}

#[test]
fn alter_table_drop_column_removes_it_from_queries() {
    let (_dir, _db, tenant) = open_with_patients();

    // Add a column, use it, then drop it.
    tenant
        .execute("ALTER TABLE patients ADD COLUMN tmp TEXT", &[])
        .expect("ADD COLUMN");
    tenant
        .execute(
            "INSERT INTO patients (id, name, tmp) VALUES ($1, $2, $3)",
            &[
                Value::BigInt(1),
                Value::Text("Alice".into()),
                Value::Text("will-be-dropped".into()),
            ],
        )
        .expect("insert with tmp column");

    tenant
        .execute("ALTER TABLE patients DROP COLUMN tmp", &[])
        .expect("DROP COLUMN must succeed");

    // The original 2 columns still answer queries cleanly.
    let result = tenant
        .query("SELECT id, name FROM patients", &[])
        .expect("post-drop select");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(1));
    assert_eq!(result.rows[0][1], Value::Text("Alice".into()));

    // Querying the dropped column surfaces a clear error.
    let err = tenant
        .query("SELECT tmp FROM patients", &[])
        .expect_err("dropped column must not be queryable");
    let err_str = format!("{err}");
    assert!(
        err_str.contains("tmp") || err_str.to_lowercase().contains("column"),
        "error should mention the missing column, got: {err_str}",
    );
}

#[test]
fn alter_table_cannot_drop_primary_key_column() {
    let (_dir, _db, tenant) = open_with_patients();

    // Dropping the primary-key column must surface an error — dropping
    // would invalidate every persisted row key.
    let err = tenant
        .execute("ALTER TABLE patients DROP COLUMN id", &[])
        .expect_err("dropping a PK column must be rejected");
    let err_str = format!("{err}");
    assert!(
        err_str.to_lowercase().contains("primary") || err_str.contains("id"),
        "error must mention primary key or column name, got: {err_str}",
    );
}

#[test]
fn alter_table_add_duplicate_column_is_rejected() {
    let (_dir, _db, tenant) = open_with_patients();

    let err = tenant
        .execute("ALTER TABLE patients ADD COLUMN name TEXT", &[])
        .expect_err("duplicate ADD COLUMN must be rejected");
    let err_str = format!("{err}");
    assert!(
        err_str.to_lowercase().contains("already") || err_str.contains("name"),
        "error must mention duplicate / column name, got: {err_str}",
    );
}
