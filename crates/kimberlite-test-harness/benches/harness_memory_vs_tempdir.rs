//! v0.6.0 Tier 1 #1 — Criterion bench comparing `Backend::TempDir`
//! (on-disk `Storage`) vs `Backend::InMemory` (v0.6.0 `MemoryStorage`).
//!
//! Three measurements per backend:
//!
//!   * `*_cold_spin_up`        — `TestKimberlite::builder().build()`
//!     all the way to a ready-to-serve socket.
//!   * `*_startup_1k_inserts_1k_selects` — end-to-end latency for
//!     the v0.6.0 acceptance workload: cold build → `CREATE TABLE`
//!     → 1,000 `INSERT`s → 1,000 `SELECT`s. Target for InMemory:
//!     < 200ms on local hardware.
//!   * `*_1k_inserts`          — 1,000 `INSERT`s against an already-
//!     spun harness. Isolates the hot-path write cost.
//!
//! Also emits a standalone stdout line:
//!
//!   ```text
//!   harness_memory_vs_tempdir  InMemory end-to-end = <ms>ms (target < 200ms)
//!   ```
//!
//! so CI and release engineering can capture the headline number
//! without re-running Criterion. The line is printed from a
//! `one_shot_end_to_end_timings` helper that runs once per bench
//! invocation (before Criterion's warmup phase).

use std::time::{Duration, Instant};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use kimberlite_client::{Client, ClientConfig};
use kimberlite_test_harness::{Backend, TestKimberlite};
use kimberlite_types::TenantId;
use kimberlite_wire::QueryParam;

const TENANT: u64 = 60_601;

/// Spins up a harness on the requested backend and returns a
/// connected client. Kept as a helper so every bench reuses the same
/// wiring — no drift between the "cold" and "hot" benches.
fn setup(backend: Backend) -> (TestKimberlite, Client) {
    let harness = TestKimberlite::builder()
        .tenant(TENANT)
        .backend(backend)
        .build()
        .expect("harness build");
    let tenant = TenantId::new(TENANT);
    let client = Client::connect(harness.addr(), tenant, ClientConfig::default()).expect("connect");
    (harness, client)
}

fn create_table(client: &mut Client) {
    client
        .execute(
            "CREATE TABLE bench_tbl (id BIGINT PRIMARY KEY, note TEXT NOT NULL)",
            &[],
        )
        .expect("create table");
}

fn insert_1k(client: &mut Client) {
    for i in 0..1_000i64 {
        client
            .execute(
                "INSERT INTO bench_tbl (id, note) VALUES ($1, $2)",
                &[QueryParam::BigInt(i), QueryParam::Text(format!("n-{i}"))],
            )
            .expect("insert");
    }
}

fn select_1k(client: &mut Client) {
    for i in 0..1_000i64 {
        let _ = client
            .query(
                "SELECT note FROM bench_tbl WHERE id = $1",
                &[QueryParam::BigInt(i)],
            )
            .expect("select");
    }
}

fn bench_cold_spin_up(c: &mut Criterion) {
    for (label, backend) in [
        ("tempdir_cold_spin_up", Backend::TempDir),
        ("memory_cold_spin_up", Backend::InMemory),
    ] {
        c.bench_function(label, |b| {
            b.iter(|| {
                let h = TestKimberlite::builder()
                    .backend(backend)
                    .build()
                    .expect("build");
                drop(h);
            });
        });
    }
}

fn bench_end_to_end(c: &mut Criterion) {
    for (label, backend) in [
        ("tempdir_startup_1k_inserts_1k_selects", Backend::TempDir),
        ("memory_startup_1k_inserts_1k_selects", Backend::InMemory),
    ] {
        c.bench_function(label, |b| {
            b.iter(|| {
                let (harness, mut client) = setup(backend);
                create_table(&mut client);
                insert_1k(&mut client);
                select_1k(&mut client);
                drop(harness);
            });
        });
    }
}

fn bench_1k_inserts(c: &mut Criterion) {
    for (label, backend) in [
        ("tempdir_1k_inserts", Backend::TempDir),
        ("memory_1k_inserts", Backend::InMemory),
    ] {
        c.bench_function(label, |b| {
            b.iter_batched(
                || {
                    let (harness, mut client) = setup(backend);
                    create_table(&mut client);
                    (harness, client)
                },
                |(harness, mut client)| {
                    insert_1k(&mut client);
                    drop(harness);
                },
                BatchSize::SmallInput,
            );
        });
    }
}

/// Runs the acceptance workload **once** for each backend and prints
/// the wall-clock time so the result is visible in CI logs even when
/// Criterion is invoked without `--bench-args`. Intentionally outside
/// the Criterion measurement path — this is a human-readable smoke
/// check, not a statistical bench.
fn one_shot_end_to_end_timings() {
    for (label, backend) in [
        ("TempDir", Backend::TempDir),
        ("InMemory", Backend::InMemory),
    ] {
        let start = Instant::now();
        let (harness, mut client) = setup(backend);
        create_table(&mut client);
        insert_1k(&mut client);
        select_1k(&mut client);
        drop(harness);
        let elapsed = start.elapsed();
        // v0.6.0 acceptance: InMemory end-to-end < 200ms.
        println!(
            "harness_memory_vs_tempdir  {label} end-to-end = {:.1}ms (target < 200ms for InMemory)",
            elapsed.as_secs_f64() * 1000.0,
        );
        // Keep the timing visible even under `cargo bench`'s stdout
        // capture — `eprintln!` is unfiltered.
        eprintln!(
            "harness_memory_vs_tempdir  {label} end-to-end = {:.1}ms (target < 200ms for InMemory)",
            elapsed.as_secs_f64() * 1000.0,
        );
        // Small pause between backends so the port-allocator can
        // reclaim the previous address cleanly on slow CI boxes.
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn bench_with_preamble(c: &mut Criterion) {
    one_shot_end_to_end_timings();
    bench_cold_spin_up(c);
    bench_end_to_end(c);
    bench_1k_inserts(c);
}

criterion_group! {
    name = harness_bench;
    config = Criterion::default().sample_size(10).measurement_time(Duration::from_secs(10));
    targets = bench_with_preamble
}
criterion_main!(harness_bench);
