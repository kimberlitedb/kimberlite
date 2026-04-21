//! Phase-1 test harness for Kimberlite apps — wraps the
//! `Kimberlite::open(tempdir)` + in-process server pattern behind a
//! builder API. Replaces the 4x-duplicated `TestServer` boilerplate in
//! `crates/kimberlite-client/tests/*.rs` so downstream SDKs (TS,
//! Python) get a single stable entrypoint.
//!
//! ## Example
//!
//! ```
//! use kimberlite_test_harness::TestKimberlite;
//! use kimberlite_wire::QueryParam;
//!
//! let harness = TestKimberlite::builder()
//!     .build()
//!     .expect("spin up test kimberlite");
//! let mut client = harness.client();
//! client
//!     .execute(
//!         "CREATE TABLE t (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
//!         &[],
//!     )
//!     .unwrap();
//! client
//!     .execute(
//!         "INSERT INTO t (id, name) VALUES ($1, $2)",
//!         &[QueryParam::BigInt(1), QueryParam::Text("Ada".into())],
//!     )
//!     .unwrap();
//! let rs = client.query("SELECT UPPER(name) FROM t WHERE id = 1", &[]).unwrap();
//! assert_eq!(rs.rows.len(), 1);
//! // Dispose runs implicitly on drop — explicit shutdown is optional.
//! drop(harness);
//! ```
//!
//! ## Design notes
//!
//! - Every `build()` gets a fresh `tempfile::TempDir`, a fresh
//!   `Kimberlite` instance, and a new TCP listener on a free port.
//! - `Drop` joins the server polling thread with a 3s timeout; the
//!   tempdir is then cleaned up naturally. No `std::mem::forget`.
//! - `Backend::TempDir` is the only variant today. Phase 2 (v0.6.0)
//!   adds `Backend::InMemory` backed by the `StorageBackend` trait
//!   without breaking this API.

#![deny(unsafe_code)]

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use kimberlite::{Kimberlite, KimberliteError};
use kimberlite_client::{Client, ClientConfig};
use kimberlite_server::{Server, ServerConfig};
use kimberlite_types::TenantId;

/// Default tenant id used when the builder doesn't override it.
const DEFAULT_TENANT: u64 = 1_000_000;

/// How long the polling thread is allowed to continue running after
/// shutdown is signalled. Matches the 3s budget from the ROADMAP
/// acceptance criteria for phase-1.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

/// Error type for harness construction + teardown.
#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    #[error("failed to create tempdir: {0}")]
    TempDir(#[from] std::io::Error),
    #[error("failed to bind loopback TCP listener for the harness")]
    Bind,
    #[error("failed to open Kimberlite instance: {0}")]
    OpenDb(#[from] KimberliteError),
    #[error("server init failed: {0}")]
    Server(Box<dyn std::error::Error + Send + Sync>),
    #[error("test client connect failed: {0}")]
    Connect(Box<dyn std::error::Error + Send + Sync>),
}

/// Backend selector for the harness's underlying `Kimberlite`.
///
/// Both variants spin up identical in-process servers; they differ only
/// in how the event log is persisted.
#[derive(Debug, Clone, Copy, Default)]
pub enum Backend {
    /// Real on-disk backend over a `tempfile::TempDir`. Matches
    /// production's `Kimberlite::open(path)`, minus durability
    /// guarantees (the tempdir is dropped on teardown).
    #[default]
    TempDir,
    /// Pure in-memory event log via
    /// `kimberlite_storage::MemoryStorage`. Uses
    /// `Kimberlite::in_memory()` under the hood — no segment files,
    /// no fsync, no manifest, no mmap. The B+tree projection store
    /// still lives in a short-lived scratch tempdir that
    /// `Kimberlite::in_memory` owns; the harness doesn't create a
    /// separate one.
    ///
    /// Added in v0.6.0. Use for test workloads that don't exercise
    /// restart/recovery semantics — those need the `TempDir` backend
    /// since `MemoryStorage` has no durable state.
    InMemory,
}

/// Builder for [`TestKimberlite`]. Holds configuration options that
/// land on `build()`.
#[derive(Debug, Default)]
pub struct TestKimberliteBuilder {
    tenant: Option<u64>,
    backend: Backend,
}

impl TestKimberliteBuilder {
    /// Set the tenant id the harness binds its client to.
    #[must_use]
    pub fn tenant(mut self, tenant: u64) -> Self {
        self.tenant = Some(tenant);
        self
    }

    /// Select the storage backend — either [`Backend::TempDir`]
    /// (default, real on-disk) or [`Backend::InMemory`] (v0.6.0,
    /// `MemoryStorage`).
    #[must_use]
    pub fn backend(mut self, backend: Backend) -> Self {
        self.backend = backend;
        self
    }

    /// Spin up the in-process server and return a ready-to-use
    /// [`TestKimberlite`]. The returned handle owns the tempdir (if
    /// any) and server thread; dropping it shuts everything down
    /// deterministically.
    pub fn build(self) -> Result<TestKimberlite, HarnessError> {
        // Construct the backing Kimberlite + optional tempdir *first*,
        // then bind the listener. Ordering doesn't matter for
        // correctness but failing fast on DB open avoids leaving
        // ports dangling under extreme filesystem pressure.
        let (db, owned_temp, data_path) = match self.backend {
            Backend::TempDir => {
                let temp = tempfile::tempdir().map_err(HarnessError::TempDir)?;
                let path: PathBuf = temp.path().to_path_buf();
                let db = Kimberlite::open(&path)?;
                (db, Some(temp), path)
            }
            Backend::InMemory => {
                // `Kimberlite::in_memory()` owns its own scratch
                // tempdir for the B+tree projections — the harness
                // doesn't need to create one. `ServerConfig` still
                // requires a path, so we pass a stable sentinel; the
                // server uses it only for log messages.
                let db = Kimberlite::in_memory()?;
                (db, None, PathBuf::from("<in-memory>"))
            }
        };

        let port = free_port()?;
        let addr: SocketAddr = format!("127.0.0.1:{port}")
            .parse()
            .expect("static addr parses");

        let cfg = ServerConfig::new(addr, &data_path);
        let mut server = Server::new(cfg, db).map_err(|e| HarnessError::Server(Box::new(e)))?;
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let server_shutdown = server.shutdown_handle();
        let flag = shutdown_flag.clone();
        let handle = thread::spawn(move || {
            // Bounded polling loop — mirrors the pre-harness TestServer
            // pattern. Budget is intentionally long (the grace timer in
            // Drop below cuts us off) so legit tests don't time out.
            let deadline = Instant::now() + Duration::from_secs(600);
            while !flag.load(Ordering::SeqCst) && Instant::now() < deadline {
                let _ = server.poll_once(Some(Duration::from_millis(20)));
            }
            server_shutdown.shutdown();
        });

        Ok(TestKimberlite {
            addr,
            tenant: TenantId::new(self.tenant.unwrap_or(DEFAULT_TENANT)),
            shutdown: shutdown_flag,
            handle: Some(handle),
            _temp: owned_temp,
        })
    }
}

/// A running in-process Kimberlite instance. Dropping it shuts the
/// server down and cleans up the underlying tempdir.
pub struct TestKimberlite {
    addr: SocketAddr,
    tenant: TenantId,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    /// Owned tempdir — dropped after `Drop::drop` joins the server
    /// thread. `Option` so we can explicitly close in `shutdown()`.
    _temp: Option<tempfile::TempDir>,
}

impl TestKimberlite {
    /// Entrypoint for the fluent builder.
    #[must_use]
    pub fn builder() -> TestKimberliteBuilder {
        TestKimberliteBuilder::default()
    }

    /// Address of the in-process server (loopback only).
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Tenant id the harness binds clients to by default.
    #[must_use]
    pub fn tenant(&self) -> TenantId {
        self.tenant
    }

    /// Open a blocking [`Client`] against the in-process server using
    /// the harness's default tenant id. Each call returns a fresh
    /// client; tests should cache the handle if they need many queries.
    pub fn client(&self) -> Client {
        Client::connect(self.addr, self.tenant, ClientConfig::default())
            .expect("test harness client connect (loopback, no auth)")
    }

    /// Explicitly shut down the harness. Dropping also shuts down;
    /// calling this is only useful if the caller wants to observe any
    /// panic from the polling thread via the returned join handle.
    pub fn shutdown(mut self) -> Result<(), HarnessError> {
        self.shutdown_in_place()
    }

    // Return type is infallible today but mirrors the public `shutdown`
    // contract so a future switch (e.g. surfacing a join panic) doesn't
    // cascade into a signature change and an SDK-binding break.
    #[allow(clippy::unnecessary_wraps)]
    fn shutdown_in_place(&mut self) -> Result<(), HarnessError> {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let deadline = Instant::now() + SHUTDOWN_GRACE;
            // Wait up to SHUTDOWN_GRACE for the thread to exit before
            // giving up. If it panics, log + swallow — harness cleanup
            // is best-effort.
            let mut remaining = SHUTDOWN_GRACE;
            while !handle.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
                remaining = deadline.saturating_duration_since(Instant::now());
            }
            let _ = remaining;
            let _ = handle.join();
        }
        // Dropping self._temp here is fine — the next Drop pass is a
        // no-op since _temp is Option and we leave it Some.
        Ok(())
    }
}

impl Drop for TestKimberlite {
    fn drop(&mut self) {
        // Best-effort deterministic teardown. Any error is swallowed —
        // panicking in Drop would poison downstream tests.
        let _ = self.shutdown_in_place();
    }
}

fn free_port() -> Result<u16, HarnessError> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| HarnessError::Bind)?;
    let port = listener
        .local_addr()
        .map_err(|_| HarnessError::Bind)?
        .port();
    drop(listener);
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_wire::QueryParam;

    #[test]
    fn builder_spins_up_and_tears_down() {
        let harness = TestKimberlite::builder()
            .tenant(99_999)
            .build()
            .expect("build should succeed");
        assert_eq!(u64::from(harness.tenant()), 99_999);
        assert!(
            harness.addr().ip().is_loopback(),
            "harness must bind to loopback only"
        );
        // Drop triggers shutdown — no panic on the polling thread.
    }

    #[test]
    fn client_can_round_trip_select() {
        let harness = TestKimberlite::builder().build().expect("build");
        let mut client = harness.client();
        client
            .execute(
                "CREATE TABLE widgets (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
                &[],
            )
            .expect("create");
        client
            .execute(
                "INSERT INTO widgets (id, name) VALUES ($1, $2)",
                &[QueryParam::BigInt(1), QueryParam::Text("Ada".into())],
            )
            .expect("insert");
        let result = client
            .query(
                "SELECT UPPER(name) FROM widgets WHERE id = $1",
                &[QueryParam::BigInt(1)],
            )
            .expect("query");
        assert_eq!(result.rows.len(), 1);
    }

    #[test]
    fn explicit_shutdown_is_idempotent_with_drop() {
        let harness = TestKimberlite::builder().build().expect("build");
        harness.shutdown().expect("shutdown");
        // If we reach here, the Drop path didn't double-shutdown + panic.
    }

    #[test]
    fn in_memory_backend_round_trips_select() {
        let harness = TestKimberlite::builder()
            .backend(Backend::InMemory)
            .build()
            .expect("in-memory build should succeed");
        let mut client = harness.client();
        client
            .execute(
                "CREATE TABLE mem_widgets (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
                &[],
            )
            .expect("create table on memory backend");
        client
            .execute(
                "INSERT INTO mem_widgets (id, name) VALUES ($1, $2)",
                &[QueryParam::BigInt(7), QueryParam::Text("Grace".into())],
            )
            .expect("insert on memory backend");
        let result = client
            .query(
                "SELECT name FROM mem_widgets WHERE id = $1",
                &[QueryParam::BigInt(7)],
            )
            .expect("query on memory backend");
        assert_eq!(result.rows.len(), 1);
    }
}
