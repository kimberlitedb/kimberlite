//! ROADMAP v0.5.1 — smoke bench for `kimberlite-test-harness`.
//!
//! Three measurements:
//!
//! * `harness_cold_spin_up` — time from `TestKimberlite::builder()`
//!   to a ready-to-serve `SocketAddr`. Acceptance target: < 50ms on
//!   CI (Linux tmpfs).
//! * `harness_1k_inserts` — 1,000 `INSERT` statements against a
//!   freshly built harness. Captures the common warm-up cost.
//! * `harness_1k_selects` — 1,000 `SELECT` queries against the
//!   same table after those inserts.
//!
//! Run: `cargo bench -p kimberlite-bench --bench harness_smoke`.
//!
//! Phase-2 (v0.6.0) adds an in-memory backend; this bench stays the
//! same so we can cite a single before/after number.

use criterion::{Criterion, criterion_group, criterion_main};
use kimberlite_client::Client;
use kimberlite_test_harness::TestKimberlite;
use kimberlite_wire::QueryParam;

fn bench_cold_spin_up(c: &mut Criterion) {
    c.bench_function("harness_cold_spin_up", |b| {
        b.iter(|| {
            let h = TestKimberlite::builder().build().expect("build");
            // Drop inside the timed region so shutdown is amortised too.
            drop(h);
        });
    });
}

fn bench_1k_inserts(c: &mut Criterion) {
    c.bench_function("harness_1k_inserts", |b| {
        b.iter_batched(
            setup,
            |(harness, mut client)| {
                for i in 0..1_000i64 {
                    client
                        .execute(
                            "INSERT INTO bench_tbl (id, note) VALUES ($1, $2)",
                            &[QueryParam::BigInt(i), QueryParam::Text(format!("n-{i}"))],
                        )
                        .expect("insert");
                }
                drop(harness);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_1k_selects(c: &mut Criterion) {
    c.bench_function("harness_1k_selects", |b| {
        b.iter_batched(
            || {
                let (harness, mut client) = setup();
                for i in 0..1_000i64 {
                    client
                        .execute(
                            "INSERT INTO bench_tbl (id, note) VALUES ($1, $2)",
                            &[QueryParam::BigInt(i), QueryParam::Text(format!("n-{i}"))],
                        )
                        .expect("insert");
                }
                (harness, client)
            },
            |(harness, mut client)| {
                for i in 0..1_000i64 {
                    let _ = client
                        .query(
                            "SELECT note FROM bench_tbl WHERE id = $1",
                            &[QueryParam::BigInt(i)],
                        )
                        .expect("select");
                }
                drop(harness);
            },
            criterion::BatchSize::LargeInput,
        );
    });
}

fn setup() -> (TestKimberlite, Client) {
    let harness = TestKimberlite::builder().build().expect("build");
    let mut client = harness.client();
    client
        .execute(
            "CREATE TABLE bench_tbl (id BIGINT PRIMARY KEY, note TEXT NOT NULL)",
            &[],
        )
        .expect("create");
    (harness, client)
}

criterion_group!(
    harness_smoke,
    bench_cold_spin_up,
    bench_1k_inserts,
    bench_1k_selects
);
criterion_main!(harness_smoke);
