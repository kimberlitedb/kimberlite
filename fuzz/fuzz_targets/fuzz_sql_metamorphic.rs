#![no_main]

// Metamorphic SQL fuzz target (SQLancer TLP, internalised).
//
// Ternary Logic Partitioning: for a predicate `p`, the three partitions
//   - p is TRUE
//   - p is FALSE
//   - p is NULL
// must collectively cover the original table exactly once. For a query
//
//   Q  = SELECT count(*) FROM t
//   Qt = SELECT count(*) FROM t WHERE p
//   Qf = SELECT count(*) FROM t WHERE NOT (p)
//   Qn = SELECT count(*) FROM t WHERE (p) IS NULL
//
// the invariant is: Q == Qt + Qf + Qn.
//
// Any disagreement points at either a NULL-handling bug, a boolean logic
// bug (AND/OR precedence), or a type-coercion bug in the WHERE evaluator.
// No external reference DB is required — we run all four queries through
// the same Kimberlite instance, so there are no cross-engine semantic gaps
// to produce false positives.
//
// Persistent mode: one `Kimberlite` instance is opened once per process
// and reset between iterations. See `Kimberlite::reset_state` (gated on
// the `fuzz-reset` feature) for the drops-everything-in-place contract.

use kimberlite::{Kimberlite, TenantId, Value};
use libfuzzer_sys::fuzz_target;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use tempfile::TempDir;

static DB: Lazy<Mutex<(TempDir, Kimberlite)>> = Lazy::new(|| {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open");
    Mutex::new((dir, db))
});

fuzz_target!(|data: &[u8]| {
    // Need at least: row count byte + one row worth of fuzz bytes.
    if data.len() < 8 {
        return;
    }

    let guard = DB.lock().expect("db mutex poisoned");
    let (_tmp, db) = &*guard;
    db.reset_state().expect("reset_state");
    let tenant = db.tenant(TenantId::new(1));

    // Schema: `fuzz_t(id BIGINT PRIMARY KEY, v BIGINT)` — v is nullable,
    // which is what makes the NULL partition non-trivial.
    if tenant
        .execute(
            "CREATE TABLE fuzz_t (id BIGINT PRIMARY KEY, v BIGINT)",
            &[],
        )
        .is_err()
    {
        return;
    }

    // Load 4–20 rows derived from fuzz bytes. Values include nulls so all
    // three TLP partitions get hit. Every 5th slot gets NULL.
    let row_count = (data[0] as usize % 16) + 4;
    for i in 0..row_count {
        let idx = 1 + (i * 2) % data.len().saturating_sub(1);
        if idx + 1 >= data.len() {
            break;
        }
        let v_byte = data[idx + 1];
        let value = if data[idx].is_multiple_of(5) {
            Value::Null
        } else {
            Value::BigInt(i64::from(v_byte))
        };
        if tenant
            .execute(
                "INSERT INTO fuzz_t (id, v) VALUES ($1, $2)",
                &[Value::BigInt(i as i64), value],
            )
            .is_err()
        {
            return;
        }
    }

    // Predicate: v = <threshold>. Cheap to express all three TLP partitions
    // against; NULL cases arise naturally because v is nullable.
    let threshold = i64::from(data[1]);

    let count = |sql: &str| -> Option<usize> {
        tenant.query(sql, &[]).ok().map(|r| {
            // The count(*) aggregate returns a single row with a single column.
            // Fall back to row count if aggregate shape changes in a future revision.
            if let Some(first_row) = r.rows.first()
                && let Some(Value::BigInt(n)) = first_row.first()
            {
                return *n as usize;
            }
            r.rows.len()
        })
    };

    let q_all = "SELECT COUNT(*) FROM fuzz_t".to_string();
    let q_true = format!("SELECT COUNT(*) FROM fuzz_t WHERE v = {threshold}");
    let q_false = format!("SELECT COUNT(*) FROM fuzz_t WHERE NOT (v = {threshold})");
    let q_null = format!("SELECT COUNT(*) FROM fuzz_t WHERE (v = {threshold}) IS NULL");

    let (Some(n_all), Some(n_t), Some(n_f), Some(n_n)) = (
        count(&q_all),
        count(&q_true),
        count(&q_false),
        count(&q_null),
    ) else {
        return;
    };

    assert_eq!(
        n_all,
        n_t + n_f + n_n,
        "TLP violation: Q={n_all} but partitions sum to {n_t}+{n_f}+{n_n}={}",
        n_t + n_f + n_n
    );
});
