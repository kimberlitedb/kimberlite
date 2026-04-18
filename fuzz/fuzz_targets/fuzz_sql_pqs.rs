#![no_main]
//! Pivoted Query Synthesis (PQS) oracle.
//!
//! Classic PQS: pick a random row (the *pivot*) from a seeded table.
//! Synthesize a `WHERE` clause that is true for the pivot by
//! construction — a conjunction of equalities on its column values.
//! Execute `SELECT id FROM t WHERE <pred>`. The result must contain
//! the pivot's id.
//!
//! A miss is a WHERE-evaluator bug (wrong equality, NULL propagation
//! error) or a projection bug (missed row in the output). Any
//! panic during execute is also a bug.

use kimberlite::{Kimberlite, TenantId, Value};
use libfuzzer_sys::fuzz_target;
use tempfile::tempdir;

const SCHEMA_SQL: &str =
    "CREATE TABLE t (id BIGINT PRIMARY KEY, v BIGINT, w BIGINT)";

fuzz_target!(|data: &[u8]| {
    // 1 byte row count + at least enough bytes to seed a row + 1 pivot index
    if data.len() < 8 {
        return;
    }

    let Ok(dir) = tempdir() else { return };
    let Ok(db) = Kimberlite::open(dir.path()) else {
        return;
    };
    let tenant = db.tenant(TenantId::new(1));

    if tenant.execute(SCHEMA_SQL, &[]).is_err() {
        return;
    }

    let row_count = (data[0] as usize % 21) + 10;
    let mut rows: Vec<(i64, Option<i64>, Option<i64>)> = Vec::with_capacity(row_count);
    for i in 0..row_count {
        let idx = 1 + (i * 2) % data.len().saturating_sub(2);
        if idx + 1 >= data.len() {
            break;
        }
        // No nullable columns in the pivot by construction — the
        // pivot predicate needs to use `=` which the planner would
        // evaluate to NULL against a NULL cell. We still seed some
        // rows with nulls to stress the negative (non-matching) path.
        let v_byte = data[idx];
        let w_byte = data[idx + 1];
        let v = if v_byte.is_multiple_of(5) {
            None
        } else {
            Some(i64::from(v_byte as i8))
        };
        let w = if w_byte.is_multiple_of(7) {
            None
        } else {
            Some(i64::from(w_byte as i8))
        };
        let v_val = v.map_or(Value::Null, Value::BigInt);
        let w_val = w.map_or(Value::Null, Value::BigInt);
        if tenant
            .execute(
                "INSERT INTO t (id, v, w) VALUES ($1, $2, $3)",
                &[Value::BigInt(i as i64), v_val, w_val],
            )
            .is_ok()
        {
            rows.push((i as i64, v, w));
        }
    }

    if rows.is_empty() {
        return;
    }

    // Select a pivot — prefer rows with fully non-null values so the
    // equality predicate is well-defined.
    let pivot_idx = (data[data.len() - 1] as usize) % rows.len();
    let (pivot_id, pivot_v, pivot_w) = rows[pivot_idx];
    let (Some(v), Some(w)) = (pivot_v, pivot_w) else {
        return;
    };

    let pred = format!("id = {pivot_id} AND v = {v} AND w = {w}");
    let sql = format!("SELECT id FROM t WHERE {pred}");

    let Ok(result) = tenant.query(&sql, &[]) else {
        return;
    };

    let found = result.rows.iter().any(|row| {
        matches!(row.first(), Some(Value::BigInt(n)) if *n == pivot_id)
    });

    assert!(
        found,
        "PQS miss: pivot id={pivot_id} (v={v}, w={w}) not returned by {sql:?} \
         — result rows: {:?}",
        result.rows
    );
});
