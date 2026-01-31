//! Error path tests.
//!
//! Tests error handling for invalid queries, missing tables/columns, type mismatches, etc.

use crate::QueryEngine;
use crate::QueryError;
use crate::key_encoder::encode_key;
use crate::schema::{ColumnDef, DataType, SchemaBuilder};
use crate::tests::MockStore;
use crate::value::Value;
use kmb_store::TableId;

#[test]
fn test_type_mismatch_in_where() {
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

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // WHERE id = 'text' - type mismatch between BigInt and Text
    // The query may succeed but return no results (no match due to type difference)
    let result = engine.query(&mut store, "SELECT * FROM users WHERE id = 'Alice'", &[]);

    // Query succeeds but returns no rows (type mismatch means no match)
    assert!(result.is_ok());
    assert_eq!(result.unwrap().rows.len(), 0);
}

#[test]
fn test_column_not_found() {
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

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // SELECT nonexistent_column FROM users
    let result = engine.query(&mut store, "SELECT nonexistent_column FROM users", &[]);

    assert!(result.is_err());
    match result.unwrap_err() {
        QueryError::ColumnNotFound { table, column } => {
            assert_eq!(table, "users");
            assert_eq!(column, "nonexistent_column");
        }
        other => panic!("Expected ColumnNotFound, got {:?}", other),
    }
}

#[test]
fn test_column_not_found_in_where() {
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

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // WHERE clause with nonexistent column
    let result = engine.query(
        &mut store,
        "SELECT * FROM users WHERE nonexistent_column = 'test'",
        &[],
    );

    assert!(result.is_err());
    match result.unwrap_err() {
        QueryError::ColumnNotFound { table, column } => {
            assert_eq!(table, "users");
            assert_eq!(column, "nonexistent_column");
        }
        other => panic!("Expected ColumnNotFound, got {:?}", other),
    }
}

#[test]
fn test_table_not_found() {
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

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // SELECT * FROM nonexistent_table
    let result = engine.query(&mut store, "SELECT * FROM nonexistent_table", &[]);

    assert!(result.is_err());
    match result.unwrap_err() {
        QueryError::TableNotFound(name) => {
            assert_eq!(name, "nonexistent_table");
        }
        other => panic!("Expected TableNotFound, got {:?}", other),
    }
}

#[test]
fn test_parameter_out_of_bounds() {
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

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // Query with $5 but only 3 params provided
    let result = engine.query(
        &mut store,
        "SELECT * FROM users WHERE id = $5",
        &[Value::BigInt(1), Value::BigInt(2), Value::BigInt(3)],
    );

    assert!(result.is_err());
    match result.unwrap_err() {
        QueryError::ParameterNotFound(index) => {
            assert_eq!(index, 5);
        }
        other => panic!("Expected ParameterNotFound, got {:?}", other),
    }
}

#[test]
fn test_invalid_sql_syntax() {
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

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // Invalid SQL syntax
    let result = engine.query(&mut store, "SELECT * FROM", &[]);

    assert!(result.is_err());
    match result.unwrap_err() {
        QueryError::ParseError(_) => {
            // Expected
        }
        other => panic!("Expected ParseError, got {:?}", other),
    }
}

#[test]
fn test_aggregate_without_group_by_with_non_aggregate() {
    let schema = SchemaBuilder::new()
        .table(
            "users",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text).not_null(),
                ColumnDef::new("age", DataType::BigInt).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // SELECT name, COUNT(*) without GROUP BY
    // Some SQL engines treat this as an error, others allow it and return arbitrary values for name
    // Our engine currently allows it
    let result = engine.query(&mut store, "SELECT name, COUNT(*) FROM users", &[]);

    // Query succeeds (semantic validation may be lenient)
    assert!(result.is_ok());
}

#[test]
fn test_order_by_column_not_in_select() {
    let schema = SchemaBuilder::new()
        .table(
            "users",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text).not_null(),
                ColumnDef::new("age", DataType::BigInt).not_null(),
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
        &serde_json::json!({"id": 1, "name": "Alice", "age": 30}),
    );

    // ORDER BY column not in SELECT list should work (SQL standard allows this)
    let result = engine.query(&mut store, "SELECT name FROM users ORDER BY age", &[]);

    // This should succeed
    assert!(result.is_ok());
}

#[test]
fn test_empty_in_list() {
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

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // WHERE id IN () - empty IN list
    // SQL standard: empty IN list is always false
    let result = engine.query(&mut store, "SELECT * FROM users WHERE id IN ()", &[]);

    // Parser may reject this, or it should return 0 rows
    if result.is_err() {
        // Parser error is acceptable
        assert!(matches!(result.unwrap_err(), QueryError::ParseError(_)));
    } else {
        // Or it should return empty result
        assert_eq!(result.unwrap().rows.len(), 0);
    }
}

#[test]
fn test_limit_zero() {
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

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // Insert test data
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "name": "Alice"}),
    );

    // LIMIT 0 should return 0 rows
    let result = engine
        .query(&mut store, "SELECT * FROM users LIMIT 0", &[])
        .unwrap();

    assert_eq!(result.rows.len(), 0);
}

#[test]
fn test_negative_limit() {
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

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // LIMIT with negative value should be a parse error
    let result = engine.query(&mut store, "SELECT * FROM users LIMIT -1", &[]);

    assert!(result.is_err());
}

#[test]
fn test_duplicate_column_in_select() {
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

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // INSERT test data
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "name": "Alice"}),
    );

    // SELECT id, id FROM users - duplicate columns should work
    let result = engine.query(&mut store, "SELECT id, id FROM users", &[]);

    assert!(result.is_ok());
    let result = result.unwrap();
    assert_eq!(result.rows[0].len(), 2); // Two columns
    assert_eq!(result.rows[0][0], result.rows[0][1]); // Same values
}

#[test]
fn test_select_star_with_explicit_columns() {
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

    let engine = QueryEngine::new(schema);
    let mut store = MockStore::new();

    // INSERT test data
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "name": "Alice"}),
    );

    // SELECT *, id FROM users
    let result = engine.query(&mut store, "SELECT *, id FROM users", &[]);

    assert!(result.is_ok());
    let result = result.unwrap();
    // The parser may deduplicate columns or expand * to all columns
    // Either 2 or 3 columns is acceptable
    assert!(result.rows[0].len() == 2 || result.rows[0].len() == 3);
}
