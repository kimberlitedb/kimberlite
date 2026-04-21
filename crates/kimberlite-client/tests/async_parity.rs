//! AUDIT-2026-04 S2.1 — parity test for the async (tokio) client.
//!
//! Spawns an in-process Kimberlite server on a kernel-assigned port,
//! issues the same workload through the sync `Client` and the async
//! `AsyncClient`, and asserts the responses agree.
//!
//! Why duplicate the workload? Because the async client is not just
//! a `tokio::task::spawn_blocking` wrapper around the sync client —
//! it has its own reader/writer task split, request_id correlation,
//! and push routing. The parity check is what guarantees the two
//! clients are indistinguishable from a server's perspective.

use std::net::SocketAddr;
use std::time::Duration;

use kimberlite_client::{AsyncClient, AsyncClientConfig, Client, ClientConfig};
use kimberlite_test_harness::TestKimberlite;
use kimberlite_types::TenantId;
use kimberlite_wire::QueryParam;

/// ROADMAP v0.5.1 — thin shim over `kimberlite-test-harness`. Keeps
/// `server.addr` accessors intact while the harness owns the tempdir,
/// polling thread, and shutdown.
struct TestServer {
    addr: SocketAddr,
    _harness: TestKimberlite,
}

impl TestServer {
    fn start() -> Self {
        let harness = TestKimberlite::builder()
            .tenant(42)
            .build()
            .expect("harness build");
        Self {
            addr: harness.addr(),
            _harness: harness,
        }
    }
}

fn tenant() -> TenantId {
    TenantId::new(42)
}

#[tokio::test]
async fn async_query_matches_sync_query() {
    let server = TestServer::start();
    // Tiny grace period so the server has the socket bound.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create the table via the sync client (also exercises that
    // sync writes are visible to async readers).
    let mut sync_client =
        Client::connect(server.addr, tenant(), ClientConfig::default()).expect("sync connect");
    sync_client
        .execute(
            "CREATE TABLE async_parity (id BIGINT PRIMARY KEY, name TEXT)",
            &[],
        )
        .expect("create table");
    sync_client
        .execute(
            "INSERT INTO async_parity (id, name) VALUES ($1, $2)",
            &[QueryParam::BigInt(1), QueryParam::Text("alice".into())],
        )
        .expect("insert sync");

    let async_client = AsyncClient::connect(server.addr, tenant(), AsyncClientConfig::default())
        .await
        .expect("async connect");

    // 1. Async query reads the row written by sync client.
    let response = async_client
        .query(
            "SELECT id, name FROM async_parity WHERE id = $1",
            &[QueryParam::BigInt(1)],
        )
        .await
        .expect("async query");
    assert_eq!(response.rows.len(), 1, "async query must see sync write");

    // 2. Async insert is visible to sync client.
    async_client
        .execute(
            "INSERT INTO async_parity (id, name) VALUES ($1, $2)",
            &[QueryParam::BigInt(2), QueryParam::Text("bob".into())],
        )
        .await
        .expect("async insert");
    let post = sync_client
        .query("SELECT id FROM async_parity ORDER BY id", &[])
        .expect("sync query post");
    assert_eq!(
        post.rows.len(),
        2,
        "sync client must observe async insert; got {} rows",
        post.rows.len(),
    );
}

#[tokio::test]
async fn async_concurrent_requests_complete() {
    // Issue 32 queries concurrently through a single shared
    // AsyncClient; they share one socket but each call must get its
    // own response. Catches bugs in request_id correlation.
    let server = TestServer::start();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut sync_client =
        Client::connect(server.addr, tenant(), ClientConfig::default()).expect("sync connect");
    sync_client
        .execute("CREATE TABLE async_concurrent (id BIGINT PRIMARY KEY)", &[])
        .expect("create");

    let async_client = AsyncClient::connect(server.addr, tenant(), AsyncClientConfig::default())
        .await
        .expect("async connect");

    let mut handles = Vec::new();
    for i in 0..32i64 {
        let c = async_client.clone();
        handles.push(tokio::spawn(async move {
            c.execute(
                "INSERT INTO async_concurrent (id) VALUES ($1)",
                &[QueryParam::BigInt(i)],
            )
            .await
        }));
    }
    for (i, h) in handles.into_iter().enumerate() {
        h.await
            .expect("task join")
            .unwrap_or_else(|e| panic!("insert {i} failed: {e}"));
    }

    let total = async_client
        .query("SELECT id FROM async_concurrent", &[])
        .await
        .expect("count");
    assert_eq!(
        total.rows.len(),
        32,
        "all 32 concurrent inserts must succeed and be visible"
    );
}
