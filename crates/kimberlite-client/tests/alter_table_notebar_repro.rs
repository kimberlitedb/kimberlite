//! Notebar error-report reproduction — ALTER TABLE ADD COLUMN.
//!
//! Context: Notebar (a downstream SDK consumer) reported:
//!
//!   > "ALTER TABLE not yet implemented - requires kernel support"
//!
//! ALTER TABLE ADD/DROP COLUMN shipped end-to-end in v0.5.0. Either Notebar
//! is pinned to a stale SDK, or there is a subtler bug in projection
//! materialisation for rows persisted before an ADD COLUMN (rows missing
//! the new column must surface as `NULL`, not drop out of the result set
//! or error).
//!
//! This test drives the *exact* Notebar pattern end-to-end over the wire:
//!
//!   1. CREATE TABLE with an initial schema.
//!   2. INSERT a few rows under the initial schema.
//!   3. ALTER TABLE ADD COLUMN.
//!   4. SELECT * and verify:
//!      - the new column is present in the result metadata,
//!      - every row predating the ALTER materialises the new column as
//!        `NULL` (i.e. `QueryValue::Null`).
//!
//! Outcome matrix:
//!   * Passes here -> Notebar is stale-pinned, no kernel fix needed.
//!   * Fails here  -> real projection gap, diagnose and patch.
//!
//! Rust test lives alongside the existing e2e-client tests so it runs in
//! CI (`cargo test -p kimberlite-client`).

use std::time::Duration;

use kimberlite_client::{Client, ClientConfig};
use kimberlite_test_harness::TestKimberlite;
use kimberlite_types::TenantId;
use kimberlite_wire::{QueryParam, QueryValue};

const NOTEBAR_TENANT: u64 = 42_042;

#[test]
fn notebar_alter_table_add_column_select_star_materialises_null() {
    // -------- Harness up ----------------------------------------------------
    let harness = TestKimberlite::builder()
        .tenant(NOTEBAR_TENANT)
        .build()
        .expect("spin up test kimberlite");
    // Small grace period so the polling thread is accepting connections.
    std::thread::sleep(Duration::from_millis(50));

    let tenant = TenantId::new(NOTEBAR_TENANT);
    let mut client = Client::connect(harness.addr(), tenant, ClientConfig::default())
        .expect("sync client connect");
    client
        .tenant_create(tenant, Some("notebar-repro".into()))
        .expect("tenant_create");

    // -------- Step 1: CREATE TABLE -----------------------------------------
    client
        .execute(
            "CREATE TABLE notes (\
                id BIGINT PRIMARY KEY, \
                body TEXT NOT NULL\
             )",
            &[],
        )
        .expect("CREATE TABLE notes");

    // -------- Step 2: INSERT rows under the initial schema -----------------
    for i in 0..3i64 {
        client
            .execute(
                "INSERT INTO notes (id, body) VALUES ($1, $2)",
                &[QueryParam::BigInt(i), QueryParam::Text(format!("body-{i}"))],
            )
            .expect("INSERT pre-ALTER row");
    }

    // Sanity: SELECT * returns 3 rows with 2 columns.
    let pre = client
        .query("SELECT id, body FROM notes ORDER BY id", &[])
        .expect("SELECT pre-ALTER");
    assert_eq!(pre.rows.len(), 3, "three rows inserted pre-ALTER");

    // -------- Step 3: ALTER TABLE ADD COLUMN -------------------------------
    // Nullable-by-default. The kernel rejects NOT NULL without a default
    // (see `ddl.md`), which matches Postgres semantics.
    client
        .execute("ALTER TABLE notes ADD COLUMN author TEXT", &[])
        .expect("ALTER TABLE ADD COLUMN must succeed post-v0.5.0");

    // -------- Step 4: SELECT * post-ALTER ----------------------------------
    // The projection must materialise `NULL` for `author` on every row
    // written pre-ALTER. This is the Notebar-suspected gap.
    let post = client
        .query("SELECT id, body, author FROM notes ORDER BY id", &[])
        .expect("SELECT id, body, author post-ALTER");

    assert_eq!(
        post.rows.len(),
        3,
        "ADD COLUMN must not drop pre-existing rows from SELECT",
    );
    assert_eq!(
        post.columns.len(),
        3,
        "ADD COLUMN must surface the new column in result metadata (got columns={:?})",
        post.columns,
    );
    assert!(
        post.columns.iter().any(|c| c == "author"),
        "`author` column must be in result columns (got {:?})",
        post.columns,
    );

    // Every pre-ALTER row has author = NULL.
    for (idx, row) in post.rows.iter().enumerate() {
        assert_eq!(row.len(), 3, "row {idx} has wrong column count: {row:?}");
        // Column order matches the SELECT list: id, body, author.
        let author_cell = &row[2];
        assert!(
            matches!(author_cell, QueryValue::Null),
            "pre-ALTER row {idx} must have NULL author, got {author_cell:?}",
        );
    }

    // -------- Step 5: post-ALTER INSERT sees the new column ----------------
    client
        .execute(
            "INSERT INTO notes (id, body, author) VALUES ($1, $2, $3)",
            &[
                QueryParam::BigInt(99),
                QueryParam::Text("post-alter body".into()),
                QueryParam::Text("ada".into()),
            ],
        )
        .expect("INSERT post-ALTER row with author");

    let post_insert = client
        .query(
            "SELECT id, author FROM notes WHERE id = 99 ORDER BY id",
            &[],
        )
        .expect("SELECT post-ALTER-insert");
    assert_eq!(post_insert.rows.len(), 1);
    assert!(
        matches!(&post_insert.rows[0][1], QueryValue::Text(s) if s == "ada"),
        "post-ALTER insert must preserve author value, got {:?}",
        post_insert.rows[0][1],
    );
}

/// Secondary Notebar pattern: SELECT * (unprojected) post-ALTER.
/// Notebar's UI issues `SELECT * FROM t` after schema changes. If the
/// projection materialiser is broken, the new column drops out entirely.
#[test]
fn notebar_select_star_post_alter_includes_new_column() {
    let harness = TestKimberlite::builder()
        .tenant(NOTEBAR_TENANT + 1)
        .build()
        .expect("harness");
    std::thread::sleep(Duration::from_millis(50));

    let tenant = TenantId::new(NOTEBAR_TENANT + 1);
    let mut client =
        Client::connect(harness.addr(), tenant, ClientConfig::default()).expect("connect");
    client.tenant_create(tenant, None).expect("tenant_create");

    client
        .execute(
            "CREATE TABLE t (id BIGINT PRIMARY KEY, v TEXT NOT NULL)",
            &[],
        )
        .expect("CREATE");
    client
        .execute(
            "INSERT INTO t (id, v) VALUES ($1, $2)",
            &[QueryParam::BigInt(1), QueryParam::Text("one".into())],
        )
        .expect("INSERT pre-ALTER");
    client
        .execute("ALTER TABLE t ADD COLUMN extra BIGINT", &[])
        .expect("ALTER");

    let rs = client
        .query("SELECT * FROM t ORDER BY id", &[])
        .expect("SELECT *");
    assert_eq!(rs.rows.len(), 1);
    assert!(
        rs.columns.iter().any(|c| c == "extra"),
        "SELECT * must surface the new column (columns={:?})",
        rs.columns,
    );
    // Find the `extra` column by name — robust against the projector
    // reordering columns alphabetically or by schema_version.
    let extra_idx = rs
        .columns
        .iter()
        .position(|c| c == "extra")
        .expect("extra column present");
    assert!(
        matches!(&rs.rows[0][extra_idx], QueryValue::Null),
        "pre-ALTER row must have NULL for the new column, got {:?}",
        rs.rows[0][extra_idx],
    );
}
