#![no_main]
//! NoREC metamorphic oracle (Non-optimizing Reference Engine,
//! internalised).
//!
//! Classic NoREC compares a DBMS against a hand-built reference
//! executor. The internalised variant here generates two queries over
//! the same table that must produce equal counts *by construction* —
//! one query lets the optimizer push the predicate down, the other
//! forces it into an aggregate expression the optimizer cannot rewrite.
//! Disagreement points at either a WHERE-clause evaluator bug or a
//! CASE/aggregate bug.
//!
//!   Q1 = SELECT COUNT(*) FROM t WHERE p
//!   Q2 = SELECT SUM(CASE WHEN p THEN 1 ELSE 0 END) FROM t
//!
//! Both skip NULL-valued predicates, so the invariant is exact modulo
//! NULL propagation — which is precisely the class of bug we want to
//! surface.
//!
//! Persistent mode — see fuzz_sql_metamorphic for the shared pattern.

use kimberlite::{Kimberlite, TenantId, Value};
use kimberlite_sim::sql_grammar::{self, SeedSchema};
use libfuzzer_sys::fuzz_target;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use tempfile::TempDir;

const SCHEMA_SQL: &str =
    "CREATE TABLE t (id BIGINT PRIMARY KEY, v BIGINT, w BIGINT)";

static DB: Lazy<Mutex<(TempDir, Kimberlite)>> = Lazy::new(|| {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open");
    Mutex::new((dir, db))
});

fuzz_target!(|data: &[u8]| {
    // Need at least 9 bytes: one for row count, 8 for the predicate seed.
    if data.len() < 9 {
        return;
    }

    let guard = DB.lock().expect("db mutex poisoned");
    let (_tmp, db) = &*guard;
    db.reset_state().expect("reset_state");
    let tenant = db.tenant(TenantId::new(1));

    if tenant.execute(SCHEMA_SQL, &[]).is_err() {
        return;
    }

    // Seed 10-30 rows with null-aware fuzz bytes.
    let row_count = (data[0] as usize % 21) + 10;
    for i in 0..row_count {
        let idx = 1 + (i * 2) % data.len().saturating_sub(2);
        if idx + 1 >= data.len() {
            break;
        }
        let v = if data[idx].is_multiple_of(7) {
            Value::Null
        } else {
            Value::BigInt(i64::from(data[idx] as i8))
        };
        let w = if data[idx + 1].is_multiple_of(11) {
            Value::Null
        } else {
            Value::BigInt(i64::from(data[idx + 1] as i8))
        };
        let _ = tenant.execute(
            "INSERT INTO t (id, v, w) VALUES ($1, $2, $3)",
            &[Value::BigInt(i as i64), v, w],
        );
    }

    // Derive the predicate seed from the tail of `data`.
    let pred_seed = u64::from_le_bytes(
        data[data.len().saturating_sub(8)..]
            .try_into()
            .unwrap_or([0u8; 8]),
    );
    let schema = SeedSchema::numeric_trio("t");
    let pred = sql_grammar::generate_predicate(pred_seed, &schema);

    let q_opt = format!("SELECT COUNT(*) FROM t WHERE {pred}");
    let q_noopt = format!(
        "SELECT SUM(CASE WHEN {pred} THEN 1 ELSE 0 END) FROM t"
    );

    // Extract a BigInt from a count(*)/sum(...) result — both return
    // exactly one row, exactly one column.
    let extract = |sql: &str| -> Option<i64> {
        let r = tenant.query(sql, &[]).ok()?;
        let row = r.rows.first()?;
        let cell = row.first()?;
        match cell {
            Value::BigInt(n) => Some(*n),
            Value::Null => Some(0),
            _ => None,
        }
    };

    let (Some(n_opt), Some(n_noopt)) = (extract(&q_opt), extract(&q_noopt)) else {
        return;
    };

    assert_eq!(
        n_opt, n_noopt,
        "NoREC divergence: \
         COUNT(*) WHERE p = {n_opt} vs SUM(CASE WHEN p) = {n_noopt} for predicate {pred:?}",
    );
});
