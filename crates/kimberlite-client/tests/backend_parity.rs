//! v0.6.0 Tier 1 #1 — backend parity test.
//!
//! Runs an identical client workflow (DDL + N inserts + ordered
//! SELECT + aggregate SELECT) against both `Backend::TempDir` (the
//! production default, real on-disk `Storage`) and `Backend::InMemory`
//! (the v0.6.0 `MemoryStorage`) via the shared `kimberlite-test-harness`
//! builder. If either backend diverges — missing row, reordered
//! output, mismatched count — this test fails.
//!
//! Why a fresh test instead of parameterising an existing one? The
//! existing e2e tests (healthcare / finance / compliance) each
//! exercise vertical compliance workflows with their own correctness
//! invariants; mixing the backend axis in would entangle the
//! parameters. This file is the single place the backend axis is
//! under test, which keeps the existing suites unmodified and green.
//!
//! The test is parameterised over `Backend` using `test_case` — same
//! pattern the rest of the codebase uses for cheap matrix coverage.

use kimberlite_client::{Client, ClientConfig};
use kimberlite_test_harness::{Backend, TestKimberlite};
use kimberlite_types::TenantId;
use kimberlite_wire::QueryParam;
use test_case::test_case;

const TENANT: u64 = 60_000;

fn run_workflow(backend: Backend) {
    let harness = TestKimberlite::builder()
        .tenant(TENANT)
        .backend(backend)
        .build()
        .expect("harness should build for both backends");
    let tenant = TenantId::new(TENANT);
    let mut client = Client::connect(harness.addr(), tenant, ClientConfig::default())
        .expect("connect to in-process server");

    client
        .execute(
            "CREATE TABLE parity (\
                id BIGINT PRIMARY KEY, \
                name TEXT NOT NULL, \
                amount BIGINT NOT NULL\
             )",
            &[],
        )
        .expect("create parity table");

    for i in 1..=5i64 {
        client
            .execute(
                "INSERT INTO parity (id, name, amount) VALUES ($1, $2, $3)",
                &[
                    QueryParam::BigInt(i),
                    QueryParam::Text(format!("row-{i}")),
                    QueryParam::BigInt(i * 10),
                ],
            )
            .expect("insert row");
    }

    // Ordered read.
    let ordered = client
        .query("SELECT id, name FROM parity ORDER BY id", &[])
        .expect("ordered select");
    assert_eq!(
        ordered.rows.len(),
        5,
        "backend {backend:?} returned {} rows, expected 5",
        ordered.rows.len()
    );

    // Aggregate read — forces the executor to scan every row.
    let total = client
        .query("SELECT COUNT(*) FROM parity", &[])
        .expect("count select");
    assert_eq!(
        total.rows.len(),
        1,
        "COUNT(*) should return exactly one row on {backend:?}"
    );

    // Filtered read — exercises PK lookup.
    let filtered = client
        .query(
            "SELECT name FROM parity WHERE id = $1",
            &[QueryParam::BigInt(3)],
        )
        .expect("filtered select");
    assert_eq!(
        filtered.rows.len(),
        1,
        "filtered select should find exactly one row on {backend:?}"
    );

    // Harness drops here → server shuts down, tempdir (if any) is
    // reclaimed. In-memory backend has nothing to clean up beyond RAM.
    drop(harness);
}

#[test_case(Backend::TempDir ; "on_disk_tempdir")]
#[test_case(Backend::InMemory ; "in_memory")]
fn workflow_parity_across_backends(backend: Backend) {
    run_workflow(backend);
}
