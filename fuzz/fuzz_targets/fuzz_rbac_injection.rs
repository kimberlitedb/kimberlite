#![no_main]

// End-to-end RBAC injection oracle.
//
// Exercises the full pipeline: SQL parse → RbacFilter rewrite → execute on a
// real Kimberlite instance with data. Unlike fuzz_rbac_rewrite (which stops at
// AST-level invariants) and fuzz_rbac_bypass (which stops at policy-level
// invariants), this target runs the rewritten query end-to-end and asserts
// that denied columns never surface in the result rows.
//
// Oracles:
//   1. `query_with_policy` never panics for any SQL + policy combination.
//   2. If the call succeeds, the returned `columns` must not contain any
//      column name that the policy explicitly denies — policy is enforced on
//      the actual query result, not just the rewritten AST.
//
// Persistent mode — see fuzz_sql_metamorphic for the same pattern.

use kimberlite::{Kimberlite, TenantId, Value};
use kimberlite_rbac::policy::AccessPolicy;
use kimberlite_rbac::roles::Role;
use libfuzzer_sys::fuzz_target;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use tempfile::TempDir;

// Fixed schema — a realistic mix of sensitive and non-sensitive columns so
// the denial list below has something to hide.
const SCHEMA: &str =
    "CREATE TABLE records (id BIGINT PRIMARY KEY, ssn BIGINT, password BIGINT, name BIGINT, department BIGINT)";
const SENSITIVE: &[&str] = &["ssn", "password"];

static DB: Lazy<Mutex<(TempDir, Kimberlite)>> = Lazy::new(|| {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open");
    Mutex::new((dir, db))
});

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }

    let guard = DB.lock().expect("db mutex poisoned");
    let (_tmp, db) = &*guard;
    db.reset_state().expect("reset_state");
    let tenant = db.tenant(TenantId::new(1));

    if tenant.execute(SCHEMA, &[]).is_err() {
        return;
    }

    // Seed a handful of rows — the values don't matter for the projection check.
    for i in 0..8_i64 {
        let _ = tenant.execute(
            "INSERT INTO records (id, ssn, password, name, department) VALUES ($1, $2, $3, $4, $5)",
            &[
                Value::BigInt(i),
                Value::BigInt(i * 111),
                Value::BigInt(i * 222),
                Value::BigInt(i * 333),
                Value::BigInt(i * 444),
            ],
        );
    }

    // Build a policy that allows the table but denies the sensitive columns.
    let mut policy = AccessPolicy::new(Role::Analyst).allow_stream("records");
    for col in SENSITIVE {
        policy = policy.deny_column(*col);
    }
    // Allow explicitly the columns we expect to show up.
    for col in ["id", "name", "department"] {
        policy = policy.allow_column(col);
    }

    // Queries to try — a mix of safe SELECT and fuzz-byte-driven choices.
    // Each query's projection is either an explicit column list or picks from
    // a fixed menu so sqlparser always accepts it.
    let choice = data[0] % 6;
    let sql = match choice {
        0 => "SELECT id, name, department FROM records".to_string(),
        1 => "SELECT ssn FROM records".to_string(),
        2 => "SELECT password FROM records".to_string(),
        3 => "SELECT id, ssn, password, name, department FROM records".to_string(),
        4 => format!(
            "SELECT id, name FROM records WHERE id = {}",
            i64::from(data[1])
        ),
        _ => "SELECT name, department, ssn FROM records".to_string(),
    };

    // Must not panic for any SQL + policy combo.
    match tenant.query_with_policy(&sql, &[], &policy) {
        Ok(result) => {
            // Invariant: no denied column may appear in the projected columns.
            for col in &result.columns {
                let col_lower = col.as_str().to_lowercase();
                for denied in SENSITIVE {
                    assert!(
                        col_lower != *denied,
                        "RBAC INJECTION BYPASS: denied column {denied:?} appeared in result of \
                         query {sql:?} — columns returned: {:?}",
                        result.columns
                    );
                }
            }
        }
        Err(_) => {
            // Errors are acceptable — the RBAC layer correctly rejected the query.
        }
    }
});
