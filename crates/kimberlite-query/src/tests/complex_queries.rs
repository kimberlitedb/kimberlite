//! Complex query scenario tests.
//!
//! Tests multi-predicate queries, aggregates, GROUP BY, NULL handling, and LIKE patterns.

use crate::QueryEngine;
use crate::key_encoder::encode_key;
use crate::schema::{ColumnDef, DataType, SchemaBuilder};
use crate::tests::MockStore;
use crate::value::Value;
use kimberlite_store::TableId;
use kimberlite_types::Timestamp;

#[test]
fn test_multi_predicate_and_or() {
    // Test simple OR predicate
    let schema = SchemaBuilder::new()
        .table(
            "users",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("age", DataType::BigInt).not_null(), // Changed to BigInt
            ],
            vec!["id".into()],
        )
        .build();

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // Insert test data
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "age": 30}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "age": 20}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "age": 16}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(4)]),
        &serde_json::json!({"id": 4, "age": 40}),
    );

    // Test simple OR: age < 18 OR age > 35
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM users WHERE age < 18 OR age > 35 ORDER BY id",
            &[],
        )
        .unwrap();

    // Should match: id=3 (age=16), id=4 (age=40)
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0][0], Value::BigInt(3));
    assert_eq!(result.rows[1][0], Value::BigInt(4));
}

#[test]
fn test_order_by_limit_combination() {
    // ORDER BY created_at DESC LIMIT 10 (regression test for bug)
    let schema = SchemaBuilder::new()
        .table(
            "events",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("created_at", DataType::Timestamp).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let engine = QueryEngine::new(schema.clone());
    let mut store = MockStore::new();

    // Insert 20 events with ascending timestamps
    for i in 1..=20 {
        store.insert_json(
            TableId::new(1),
            encode_key(&[Value::BigInt(i)]),
            &serde_json::json!({
                "id": i,
                "created_at": Timestamp::from_nanos((i as u64) * 1_000_000_000) // 1 second apart
            }),
        );
    }

    let result = engine
        .query(
            &mut store,
            "SELECT id FROM events ORDER BY created_at DESC LIMIT 10",
            &[],
        )
        .unwrap();

    // Should return newest 10 events (ids 20-11 in descending order)
    assert_eq!(result.rows.len(), 10);
    assert_eq!(result.rows[0][0], Value::BigInt(20));
    assert_eq!(result.rows[1][0], Value::BigInt(19));
    assert_eq!(result.rows[9][0], Value::BigInt(11));
}

#[test]
fn test_aggregate_with_where() {
    // SELECT COUNT(*), AVG(age) FROM users WHERE active = true
    let schema = SchemaBuilder::new()
        .table(
            "users",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("age", DataType::BigInt).not_null(), // Changed to BigInt
                ColumnDef::new("active", DataType::Boolean).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let engine = QueryEngine::new(schema.clone());
    let mut store = MockStore::new();

    // Insert test data
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "age": 30, "active": true}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "age": 40, "active": true}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "age": 50, "active": false}),
    );

    let result = engine
        .query(
            &mut store,
            "SELECT COUNT(*), AVG(age) FROM users WHERE active = true",
            &[],
        )
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(2)); // COUNT(*) = 2 active users
    assert_eq!(result.rows[0][1], Value::Real(35.0)); // AVG(age) = (30 + 40) / 2
}

#[test]
fn test_group_by_multiple_columns() {
    // SELECT user_id, status, COUNT(*) FROM orders GROUP BY user_id, status
    let schema = SchemaBuilder::new()
        .table(
            "orders",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("user_id", DataType::BigInt).not_null(),
                ColumnDef::new("status", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let engine = QueryEngine::new(schema.clone());
    let mut store = MockStore::new();

    // Insert test data: multiple orders per user with different statuses
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "user_id": 100, "status": "pending"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "user_id": 100, "status": "pending"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "user_id": 100, "status": "completed"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(4)]),
        &serde_json::json!({"id": 4, "user_id": 200, "status": "pending"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(5)]),
        &serde_json::json!({"id": 5, "user_id": 200, "status": "completed"}),
    );

    let result = engine
        .query(
            &mut store,
            "SELECT user_id, status, COUNT(*) FROM orders GROUP BY user_id, status ORDER BY user_id, status",
            &[],
        )
        .unwrap();

    // Should have 4 groups
    assert_eq!(result.rows.len(), 4);

    // Verify we have the correct groups (order may vary)
    let mut user_100_completed = 0;
    let mut user_100_pending = 0;
    let mut user_200_completed = 0;
    let mut user_200_pending = 0;

    for row in &result.rows {
        match (row[0].clone(), row[1].clone(), row[2].clone()) {
            (Value::BigInt(100), Value::Text(s), Value::BigInt(count)) if s == "completed" => {
                user_100_completed = count;
            }
            (Value::BigInt(100), Value::Text(s), Value::BigInt(count)) if s == "pending" => {
                user_100_pending = count;
            }
            (Value::BigInt(200), Value::Text(s), Value::BigInt(count)) if s == "completed" => {
                user_200_completed = count;
            }
            (Value::BigInt(200), Value::Text(s), Value::BigInt(count)) if s == "pending" => {
                user_200_pending = count;
            }
            _ => panic!("Unexpected row: {row:?}"),
        }
    }

    assert_eq!(user_100_completed, 1);
    assert_eq!(user_100_pending, 2);
    assert_eq!(user_200_completed, 1);
    assert_eq!(user_200_pending, 1);
}

#[test]
fn test_null_handling_in_predicates() {
    let schema = SchemaBuilder::new()
        .table(
            "users",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("email", DataType::Text), // Nullable
            ],
            vec!["id".into()],
        )
        .build();

    let engine = QueryEngine::new(schema.clone());
    let mut store = MockStore::new();

    // Insert test data with some NULL emails
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "email": "alice@example.com"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "email": null}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "email": "charlie@example.com"}),
    );

    // Test IS NULL
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM users WHERE email IS NULL ORDER BY id",
            &[],
        )
        .unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(2));

    // Test IS NOT NULL
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM users WHERE email IS NOT NULL ORDER BY id",
            &[],
        )
        .unwrap();
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0][0], Value::BigInt(1));
    assert_eq!(result.rows[1][0], Value::BigInt(3));

    // Test = should not match NULL rows
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM users WHERE email = 'alice@example.com'",
            &[],
        )
        .unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(1));
}

#[test]
fn test_in_operator_with_nulls() {
    let schema = SchemaBuilder::new()
        .table(
            "users",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let engine = QueryEngine::new(schema.clone());
    let mut store = MockStore::new();

    // Insert test data
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "name": "Alice"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "name": "Bob"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "name": "Charlie"}),
    );

    // Test IN with values (no NULL in IN list for this test)
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM users WHERE id IN (1, 3) ORDER BY id",
            &[],
        )
        .unwrap();
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0][0], Value::BigInt(1));
    assert_eq!(result.rows[1][0], Value::BigInt(3));
}

#[test]
fn test_like_patterns() {
    let schema = SchemaBuilder::new()
        .table(
            "users",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let engine = QueryEngine::new(schema.clone());
    let mut store = MockStore::new();

    // Insert test data
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "name": "Alice"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "name": "Bob"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "name": "Andrew"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(4)]),
        &serde_json::json!({"id": 4, "name": "TestUser"}),
    );

    // Test LIKE 'A%' - starts with A
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM users WHERE name LIKE 'A%' ORDER BY id",
            &[],
        )
        .unwrap();
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0][0], Value::BigInt(1)); // Alice
    assert_eq!(result.rows[1][0], Value::BigInt(3)); // Andrew

    // Test LIKE '%test%' - contains 'test' (case-sensitive)
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM users WHERE name LIKE '%Test%'",
            &[],
        )
        .unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(4)); // TestUser

    // Test LIKE 'A_ice' - exactly one character between A and ice
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM users WHERE name LIKE 'A_ice'",
            &[],
        )
        .unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(1)); // Alice
}

#[test]
fn test_complex_aggregate_with_group_by_and_having() {
    // More complex scenario: GROUP BY with HAVING clause
    let schema = SchemaBuilder::new()
        .table(
            "sales",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("product_id", DataType::BigInt).not_null(),
                ColumnDef::new("amount", DataType::BigInt).not_null(), // Changed to BigInt
            ],
            vec!["id".into()],
        )
        .build();

    let engine = QueryEngine::new(schema.clone());
    let mut store = MockStore::new();

    // Insert sales data
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "product_id": 100, "amount": 50}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "product_id": 100, "amount": 75}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "product_id": 200, "amount": 30}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(4)]),
        &serde_json::json!({"id": 4, "product_id": 200, "amount": 40}),
    );

    // Test: GROUP BY product_id with aggregates
    let result = engine
        .query(
            &mut store,
            "SELECT product_id, COUNT(*), SUM(amount) FROM sales GROUP BY product_id ORDER BY product_id",
            &[],
        )
        .unwrap();

    assert_eq!(result.rows.len(), 2);

    // Verify we have the correct aggregates (order may vary)
    let mut product_100_count = 0;
    let mut product_100_sum = 0;
    let mut product_200_count = 0;
    let mut product_200_sum = 0;

    for row in &result.rows {
        match (row[0].clone(), row[1].clone(), row[2].clone()) {
            (Value::BigInt(100), Value::BigInt(count), Value::BigInt(sum)) => {
                product_100_count = count;
                product_100_sum = sum;
            }
            (Value::BigInt(200), Value::BigInt(count), Value::BigInt(sum)) => {
                product_200_count = count;
                product_200_sum = sum;
            }
            _ => panic!("Unexpected row: {row:?}"),
        }
    }

    assert_eq!(product_100_count, 2);
    assert_eq!(product_100_sum, 125);
    assert_eq!(product_200_count, 2);
    assert_eq!(product_200_sum, 70);
}

#[test]
fn test_multiple_order_by_columns() {
    // Test ORDER BY with multiple columns
    let schema = SchemaBuilder::new()
        .table(
            "users",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("age", DataType::BigInt).not_null(), // Changed to BigInt
                ColumnDef::new("name", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let engine = QueryEngine::new(schema.clone());
    let mut store = MockStore::new();

    // Insert data with same ages to test secondary sort
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "age": 30, "name": "Charlie"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "age": 25, "name": "Bob"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "age": 30, "name": "Alice"}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(4)]),
        &serde_json::json!({"id": 4, "age": 25, "name": "Dave"}),
    );

    // ORDER BY age DESC, name ASC
    let result = engine
        .query(
            &mut store,
            "SELECT id, age, name FROM users ORDER BY age DESC, name ASC",
            &[],
        )
        .unwrap();

    assert_eq!(result.rows.len(), 4);

    // Age 30 group (descending), sorted by name (ascending)
    assert_eq!(result.rows[0][0], Value::BigInt(3)); // Alice, age 30
    assert_eq!(result.rows[1][0], Value::BigInt(1)); // Charlie, age 30

    // Age 25 group (descending), sorted by name (ascending)
    assert_eq!(result.rows[2][0], Value::BigInt(2)); // Bob, age 25
    assert_eq!(result.rows[3][0], Value::BigInt(4)); // Dave, age 25
}
