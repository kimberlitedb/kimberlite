//! AUDIT-2026-04 S3.2 — end-to-end window-function tests.
//!
//! Drives `SELECT ... <fn>() OVER (PARTITION BY ... ORDER BY ...)`
//! through the full parse → plan → execute → window-pass pipeline.
//!
//! PostgreSQL parity is the contract — reference behaviour was
//! verified manually with `psql` and the matching results are
//! encoded in the assertions below.

use crate::QueryEngine;
use crate::key_encoder::encode_key;
use crate::schema::{ColumnDef, DataType, SchemaBuilder};
use crate::tests::MockStore;
use crate::value::Value;
use kimberlite_store::TableId;

fn employees_schema_and_store() -> (crate::Schema, MockStore) {
    let schema = SchemaBuilder::new()
        .table(
            "employees",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("dept", DataType::Text).not_null(),
                ColumnDef::new("salary", DataType::BigInt).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let rows = vec![
        (1i64, "eng", 100i64),
        (2, "eng", 120),
        (3, "eng", 120),
        (4, "sales", 80),
        (5, "sales", 90),
        (6, "sales", 110),
    ];
    for (id, dept, salary) in rows {
        store.insert_json(
            TableId::new(1),
            encode_key(&[Value::BigInt(id)]),
            &serde_json::json!({"id": id, "dept": dept, "salary": salary}),
        );
    }
    (schema, store)
}

#[test]
fn row_number_partition_by_orders_within_groups() {
    let (schema, mut store) = employees_schema_and_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(
            &mut store,
            "SELECT id, dept, salary, ROW_NUMBER() OVER (PARTITION BY dept ORDER BY salary) AS rn FROM employees",
            &[],
        )
        .unwrap();

    assert_eq!(result.columns.len(), 4, "id + dept + salary + rn");
    assert_eq!(result.rows.len(), 6);

    // Build (id → row_number) map; ordering of input rows is preserved
    // so we look up by id rather than relying on a specific order.
    let by_id: std::collections::HashMap<i64, i64> = result
        .rows
        .iter()
        .map(|r| {
            let id = match &r[0] {
                Value::BigInt(i) => *i,
                _ => panic!(),
            };
            let rn = match &r[3] {
                Value::BigInt(i) => *i,
                _ => panic!(),
            };
            (id, rn)
        })
        .collect();

    // engineering: salaries 100, 120, 120 → rn 1, 2, 3 (stable)
    assert_eq!(by_id[&1], 1, "id=1 (eng, salary=100) is first in dept");
    assert!(by_id[&2] == 2 || by_id[&2] == 3);
    assert!(by_id[&3] == 2 || by_id[&3] == 3);
    assert_ne!(by_id[&2], by_id[&3], "rn must distinguish ties");

    // sales: salaries 80, 90, 110 → rn 1, 2, 3
    assert_eq!(by_id[&4], 1);
    assert_eq!(by_id[&5], 2);
    assert_eq!(by_id[&6], 3);
}

#[test]
fn rank_skips_after_ties_dense_rank_does_not() {
    let (schema, mut store) = employees_schema_and_store();
    let engine = QueryEngine::new(schema);

    // Window functions look up referenced columns by name in the
    // base SELECT projection — the planner does not yet auto-inject
    // them. Project the columns the OVER clause needs explicitly.
    let result = engine
        .query(
            &mut store,
            "SELECT id, dept, salary, \
                    RANK() OVER (PARTITION BY dept ORDER BY salary) AS r, \
                    DENSE_RANK() OVER (PARTITION BY dept ORDER BY salary) AS dr \
             FROM employees",
            &[],
        )
        .unwrap();

    assert_eq!(result.columns.len(), 5, "id + dept + salary + r + dr");
    let by_id: std::collections::HashMap<i64, (i64, i64)> = result
        .rows
        .iter()
        .map(|row| {
            let id = match &row[0] {
                Value::BigInt(i) => *i,
                _ => panic!(),
            };
            let r = match &row[3] {
                Value::BigInt(i) => *i,
                _ => panic!(),
            };
            let dr = match &row[4] {
                Value::BigInt(i) => *i,
                _ => panic!(),
            };
            (id, (r, dr))
        })
        .collect();

    // engineering: id=1 salary=100 → r=1, dr=1
    // id=2 and id=3 (both salary=120) → r=2, dr=2
    // (RANK leaves a gap if ties exist; here ties are at the
    // start of the new salary tier so r=2 for both.)
    assert_eq!(by_id[&1], (1, 1));
    assert_eq!(by_id[&2], (2, 2));
    assert_eq!(by_id[&3], (2, 2));
    // sales: 80, 90, 110 — no ties, r and dr both 1, 2, 3.
    assert_eq!(by_id[&4], (1, 1));
    assert_eq!(by_id[&5], (2, 2));
    assert_eq!(by_id[&6], (3, 3));
}

#[test]
fn lag_returns_null_at_partition_start_and_value_otherwise() {
    let (schema, mut store) = employees_schema_and_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(
            &mut store,
            "SELECT id, dept, salary, \
                    LAG(salary) OVER (PARTITION BY dept ORDER BY salary) AS prev \
             FROM employees",
            &[],
        )
        .unwrap();

    let by_id: std::collections::HashMap<i64, Value> = result
        .rows
        .iter()
        .map(|r| {
            let id = match &r[0] {
                Value::BigInt(i) => *i,
                _ => panic!(),
            };
            (id, r[3].clone())
        })
        .collect();

    // engineering: lowest salary (id=1) → NULL; mid (id=2 or 3) →
    // 100; the other tied row → 120 (the partition's previous row
    // by sorted salary).
    assert_eq!(by_id[&1], Value::Null);
    // sales: id=4 → NULL, id=5 → 80, id=6 → 90.
    assert_eq!(by_id[&4], Value::Null);
    assert_eq!(by_id[&5], Value::BigInt(80));
    assert_eq!(by_id[&6], Value::BigInt(90));
}

#[test]
fn first_value_returns_partition_minimum_under_order_by_salary() {
    let (schema, mut store) = employees_schema_and_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(
            &mut store,
            "SELECT dept, salary, \
                    FIRST_VALUE(salary) OVER (PARTITION BY dept ORDER BY salary) AS lowest \
             FROM employees",
            &[],
        )
        .unwrap();

    for row in &result.rows {
        let dept = match &row[0] {
            Value::Text(s) => s.clone(),
            _ => panic!(),
        };
        let lowest = match &row[2] {
            Value::BigInt(i) => *i,
            _ => panic!(),
        };
        match dept.as_str() {
            "eng" => assert_eq!(lowest, 100),
            "sales" => assert_eq!(lowest, 80),
            other => panic!("unexpected dept: {other}"),
        }
    }
}

#[test]
fn explicit_window_frame_clause_is_rejected_with_clear_error() {
    let (schema, mut store) = employees_schema_and_store();
    let engine = QueryEngine::new(schema);

    // The implementation deliberately rejects ROWS/RANGE BETWEEN
    // clauses since the default frame semantics differ between
    // ranking and value functions and we don't want to silently
    // diverge from PostgreSQL.
    let err = engine
        .query(
            &mut store,
            "SELECT ROW_NUMBER() OVER (\
                PARTITION BY dept ORDER BY salary \
                ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW\
             ) FROM employees",
            &[],
        )
        .expect_err("explicit frame must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("frame") || msg.contains("FRAME"),
        "error must explain the frame issue; got: {msg}"
    );
}
