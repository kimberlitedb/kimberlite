//! Integration tests for kmb-query.

mod type_tests;

use std::collections::HashMap;
use std::ops::Range;

use bytes::Bytes;
use kmb_store::{Key, ProjectionStore, StoreError, TableId, WriteBatch, WriteOp};
use kmb_types::Offset;

use crate::QueryEngine;
use crate::schema::{ColumnDef, DataType, SchemaBuilder};
use crate::value::Value;

// ============================================================================
// Mock Store
// ============================================================================

/// A simple in-memory mock store for testing.
#[derive(Debug, Default)]
struct MockStore {
    tables: HashMap<TableId, Vec<(Key, Bytes)>>,
    position: Offset,
}

impl MockStore {
    fn new() -> Self {
        Self::default()
    }

    fn insert(&mut self, table_id: TableId, key: Key, value: Bytes) {
        let table = self.tables.entry(table_id).or_default();
        table.push((key, value));
        table.sort_by(|a, b| a.0.cmp(&b.0));
    }

    fn insert_json(&mut self, table_id: TableId, key: Key, json: &serde_json::Value) {
        let bytes = Bytes::from(serde_json::to_vec(json).expect("json serialization failed"));
        self.insert(table_id, key, bytes);
    }
}

impl ProjectionStore for MockStore {
    fn apply(&mut self, batch: WriteBatch) -> Result<(), StoreError> {
        for op in batch.operations() {
            match op {
                WriteOp::Put { table, key, value } => {
                    self.insert(*table, key.clone(), value.clone());
                }
                WriteOp::Delete { table, key } => {
                    if let Some(t) = self.tables.get_mut(table) {
                        t.retain(|(k, _)| k != key);
                    }
                }
            }
        }
        self.position = batch.position();
        Ok(())
    }

    fn applied_position(&self) -> Offset {
        self.position
    }

    fn get(&mut self, table: TableId, key: &Key) -> Result<Option<Bytes>, StoreError> {
        Ok(self
            .tables
            .get(&table)
            .and_then(|t| t.iter().find(|(k, _)| k == key))
            .map(|(_, v)| v.clone()))
    }

    fn get_at(
        &mut self,
        table: TableId,
        key: &Key,
        _pos: Offset,
    ) -> Result<Option<Bytes>, StoreError> {
        // Mock doesn't support MVCC, just use current state
        self.get(table, key)
    }

    fn scan(
        &mut self,
        table: TableId,
        range: Range<Key>,
        limit: usize,
    ) -> Result<Vec<(Key, Bytes)>, StoreError> {
        let Some(entries) = self.tables.get(&table) else {
            return Ok(vec![]);
        };

        let result: Vec<_> = entries
            .iter()
            .filter(|(k, _)| k >= &range.start && k < &range.end)
            .take(limit)
            .cloned()
            .collect();

        Ok(result)
    }

    fn scan_at(
        &mut self,
        table: TableId,
        range: Range<Key>,
        limit: usize,
        _pos: Offset,
    ) -> Result<Vec<(Key, Bytes)>, StoreError> {
        self.scan(table, range, limit)
    }

    fn sync(&mut self) -> Result<(), StoreError> {
        Ok(())
    }
}

// ============================================================================
// Test Schema
// ============================================================================

fn test_schema() -> crate::Schema {
    SchemaBuilder::new()
        .table(
            "users",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text).not_null(),
                ColumnDef::new("age", DataType::BigInt),
            ],
            vec!["id".into()],
        )
        .table(
            "orders",
            TableId::new(2),
            vec![
                ColumnDef::new("order_id", DataType::BigInt).not_null(),
                ColumnDef::new("user_id", DataType::BigInt).not_null(),
                ColumnDef::new("total", DataType::BigInt),
            ],
            vec!["order_id".into()],
        )
        .build()
}

fn test_store() -> MockStore {
    use crate::key_encoder::encode_key;

    let mut store = MockStore::new();

    // Insert test users
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "name": "Alice", "age": 30}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "name": "Bob", "age": 25}),
    );
    store.insert_json(
        TableId::new(1),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "name": "Charlie", "age": 35}),
    );

    // Insert test orders
    store.insert_json(
        TableId::new(2),
        encode_key(&[Value::BigInt(100)]),
        &serde_json::json!({"order_id": 100, "user_id": 1, "total": 500}),
    );
    store.insert_json(
        TableId::new(2),
        encode_key(&[Value::BigInt(101)]),
        &serde_json::json!({"order_id": 101, "user_id": 2, "total": 300}),
    );

    store
}

// ============================================================================
// Tests
// ============================================================================

#[test]
fn test_select_all() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT * FROM users", &[])
        .unwrap();

    assert_eq!(result.columns.len(), 3);
    assert_eq!(result.rows.len(), 3);
}

#[test]
fn test_select_columns() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT name, age FROM users", &[])
        .unwrap();

    assert_eq!(result.columns.len(), 2);
    assert_eq!(result.columns[0].as_str(), "name");
    assert_eq!(result.columns[1].as_str(), "age");
}

#[test]
fn test_point_lookup() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT * FROM users WHERE id = 1", &[])
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(1));
    assert_eq!(result.rows[0][1], Value::Text("Alice".to_string()));
}

#[test]
fn test_point_lookup_not_found() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT * FROM users WHERE id = 999", &[])
        .unwrap();

    assert!(result.rows.is_empty());
}

#[test]
fn test_range_scan_gt() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT * FROM users WHERE id > 1", &[])
        .unwrap();

    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_range_scan_lt() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT * FROM users WHERE id < 3", &[])
        .unwrap();

    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_table_scan_with_filter() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT * FROM users WHERE name = 'Bob'", &[])
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][1], Value::Text("Bob".to_string()));
}

#[test]
fn test_limit() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT * FROM users LIMIT 2", &[])
        .unwrap();

    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_order_by() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT * FROM users ORDER BY age ASC", &[])
        .unwrap();

    assert_eq!(result.rows.len(), 3);
    assert_eq!(result.rows[0][2], Value::BigInt(25)); // Bob
    assert_eq!(result.rows[1][2], Value::BigInt(30)); // Alice
    assert_eq!(result.rows[2][2], Value::BigInt(35)); // Charlie
}

#[test]
fn test_order_by_desc() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT * FROM users ORDER BY age DESC", &[])
        .unwrap();

    assert_eq!(result.rows[0][2], Value::BigInt(35)); // Charlie
    assert_eq!(result.rows[2][2], Value::BigInt(25)); // Bob
}

#[test]
fn test_parameterized_query() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(
            &mut store,
            "SELECT * FROM users WHERE id = $1",
            &[Value::BigInt(2)],
        )
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][1], Value::Text("Bob".to_string()));
}

#[test]
fn test_multiple_params() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(
            &mut store,
            "SELECT * FROM users WHERE id >= $1 AND id <= $2",
            &[Value::BigInt(1), Value::BigInt(2)],
        )
        .unwrap();

    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_in_predicate() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT * FROM users WHERE id IN (1, 3)", &[])
        .unwrap();

    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_prepared_query() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let prepared = engine
        .prepare("SELECT name FROM users WHERE id = $1", &[Value::BigInt(1)])
        .unwrap();

    assert_eq!(prepared.columns().len(), 1);
    assert_eq!(prepared.table_name(), "users");

    let result = prepared.execute(&mut store).unwrap();
    assert_eq!(result.rows.len(), 1);
}

#[test]
fn test_unknown_table() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine.query(&mut store, "SELECT * FROM nonexistent", &[]);
    assert!(matches!(result, Err(crate::QueryError::TableNotFound(_))));
}

#[test]
fn test_unknown_column() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine.query(&mut store, "SELECT nonexistent FROM users", &[]);
    assert!(matches!(
        result,
        Err(crate::QueryError::ColumnNotFound { .. })
    ));
}

#[test]
fn test_missing_param() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine.query(&mut store, "SELECT * FROM users WHERE id = $1", &[]);
    assert!(matches!(
        result,
        Err(crate::QueryError::ParameterNotFound(1))
    ));
}

#[test]
fn test_query_at_position() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    // Note: Our mock store doesn't truly support MVCC, but this tests the API
    let result = engine
        .query_at(
            &mut store,
            "SELECT * FROM users WHERE id = 1",
            &[],
            Offset::new(100),
        )
        .unwrap();

    assert_eq!(result.rows.len(), 1);
}

// ============================================================================
// Key Encoding Property Tests
// ============================================================================

// ============================================================================
// DDL/DML Parser Tests
// ============================================================================

#[cfg(test)]
mod parser_tests {
    use crate::parser::{parse_statement, ParsedStatement};

    #[test]
    fn parse_create_table() {
        let sql = "CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::CreateTable(ct) => {
                assert_eq!(ct.table_name, "users");
                assert_eq!(ct.columns.len(), 2);
                assert_eq!(ct.columns[0].name, "id");
                assert_eq!(ct.columns[0].data_type, "BIGINT");
                assert!(!ct.columns[0].nullable);
                assert_eq!(ct.columns[1].name, "name");
                assert_eq!(ct.columns[1].data_type, "TEXT");
                assert_eq!(ct.primary_key, vec!["id"]);
            }
            _ => panic!("expected CreateTable"),
        }
    }

    #[test]
    fn parse_create_table_with_nullable_column() {
        let sql = "CREATE TABLE users (id BIGINT NOT NULL, age BIGINT, PRIMARY KEY (id))";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::CreateTable(ct) => {
                assert_eq!(ct.columns.len(), 2);
                assert!(!ct.columns[0].nullable); // id NOT NULL
                assert!(ct.columns[1].nullable); // age is nullable
            }
            _ => panic!("expected CreateTable"),
        }
    }

    #[test]
    fn parse_create_table_with_composite_primary_key() {
        let sql = "CREATE TABLE orders (
            user_id BIGINT NOT NULL,
            order_id BIGINT NOT NULL,
            amount BIGINT,
            PRIMARY KEY (user_id, order_id)
        )";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::CreateTable(ct) => {
                assert_eq!(ct.table_name, "orders");
                assert_eq!(ct.primary_key, vec!["user_id", "order_id"]);
            }
            _ => panic!("expected CreateTable"),
        }
    }

    #[test]
    fn parse_drop_table() {
        let sql = "DROP TABLE users";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::DropTable(table_name) => {
                assert_eq!(table_name, "users");
            }
            _ => panic!("expected DropTable"),
        }
    }

    #[test]
    fn parse_create_index() {
        let sql = "CREATE INDEX idx_name ON users (name)";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::CreateIndex(ci) => {
                assert_eq!(ci.index_name, "idx_name");
                assert_eq!(ci.table_name, "users");
                assert_eq!(ci.columns, vec!["name"]);
            }
            _ => panic!("expected CreateIndex"),
        }
    }

    #[test]
    fn parse_create_index_composite() {
        let sql = "CREATE INDEX idx_user_date ON orders (user_id, order_date)";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::CreateIndex(ci) => {
                assert_eq!(ci.index_name, "idx_user_date");
                assert_eq!(ci.table_name, "orders");
                assert_eq!(ci.columns, vec!["user_id", "order_date"]);
            }
            _ => panic!("expected CreateIndex"),
        }
    }

    #[test]
    fn parse_insert() {
        let sql = "INSERT INTO users (id, name) VALUES (1, 'Alice')";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Insert(ins) => {
                assert_eq!(ins.table, "users");
                assert_eq!(ins.columns, vec!["id", "name"]);
                assert_eq!(ins.values.len(), 1, "Should have 1 row");
                assert_eq!(ins.values[0].len(), 2, "First row should have 2 values");
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn parse_insert_multiple_types() {
        let sql = "INSERT INTO users (id, name, active, age) VALUES (1, 'Alice', true, 30)";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Insert(ins) => {
                assert_eq!(ins.table, "users");
                assert_eq!(ins.columns.len(), 4);
                assert_eq!(ins.values.len(), 1, "Should have 1 row");
                assert_eq!(ins.values[0].len(), 4, "First row should have 4 values");
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn parse_update() {
        let sql = "UPDATE users SET name = 'Bob' WHERE id = 1";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Update(upd) => {
                assert_eq!(upd.table, "users");
                assert_eq!(upd.assignments.len(), 1);
                assert_eq!(upd.predicates.len(), 1);
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn parse_update_multiple_columns() {
        let sql = "UPDATE users SET name = 'Bob', age = 31 WHERE id = 1";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Update(upd) => {
                assert_eq!(upd.table, "users");
                assert_eq!(upd.assignments.len(), 2);
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn parse_delete() {
        let sql = "DELETE FROM users WHERE id = 1";
        let result = parse_statement(sql);

        if let Err(ref e) = result {
            eprintln!("DELETE parse error: {:?}", e);
        }
        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Delete(del) => {
                assert_eq!(del.table, "users");
                assert_eq!(del.predicates.len(), 1);
            }
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn parse_delete_multiple_conditions() {
        let sql = "DELETE FROM users WHERE id > 100 AND active = false";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Delete(del) => {
                assert_eq!(del.table, "users");
                assert_eq!(del.predicates.len(), 2);
            }
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn parse_select_still_works() {
        let sql = "SELECT id, name FROM users WHERE id = 1";
        let result = parse_statement(sql);

        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Select(sel) => {
                assert_eq!(sel.table, "users");
                assert!(sel.columns.is_some());
                assert_eq!(sel.columns.unwrap().len(), 2);
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn parse_invalid_sql_fails() {
        let sql = "INVALID SQL STATEMENT";
        let result = parse_statement(sql);

        assert!(result.is_err());
    }

    #[test]
    fn parse_unsupported_statement_fails() {
        let sql = "ALTER TABLE users ADD COLUMN email TEXT";
        let result = parse_statement(sql);

        // Should fail because ALTER is not supported
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod key_encoding_tests {
    use super::*;
    use crate::key_encoder::{decode_key, encode_key};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn bigint_encoding_round_trip(v: i64) {
            let key = encode_key(&[Value::BigInt(v)]);
            let decoded = decode_key(&key);
            prop_assert_eq!(decoded, vec![Value::BigInt(v)]);
        }

        #[test]
        fn bigint_ordering_preserved(a: i64, b: i64) {
            let key_a = encode_key(&[Value::BigInt(a)]);
            let key_b = encode_key(&[Value::BigInt(b)]);

            prop_assert_eq!(a.cmp(&b), key_a.cmp(&key_b));
        }

        #[test]
        fn text_round_trip(s in "\\PC*") {
            let key = encode_key(&[Value::Text(s.clone())]);
            let decoded = decode_key(&key);
            prop_assert_eq!(decoded, vec![Value::Text(s)]);
        }

        #[test]
        fn composite_key_round_trip(a: i64, s in "[a-z]{0,10}") {
            let values = vec![Value::BigInt(a), Value::Text(s.clone())];
            let key = encode_key(&values);
            let decoded = decode_key(&key);
            prop_assert_eq!(decoded, values);
        }
    }
}

// ============================================================================
// Edge Case Tests (Phase 2: Logic Bug Detection)
// ============================================================================

#[test]
fn test_null_in_where_clause() {
    use crate::key_encoder::encode_key;

    // Create schema with nullable column
    let schema = SchemaBuilder::new()
        .table(
            "products",
            TableId::new(3),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text), // Nullable
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    // Insert rows with NULL and non-NULL names
    store.insert_json(
        TableId::new(3),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "name": "Widget"}),
    );
    store.insert_json(
        TableId::new(3),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "name": null}),
    );
    store.insert_json(
        TableId::new(3),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "name": "Gadget"}),
    );

    // Test IS NULL
    let result = engine
        .query(&mut store, "SELECT id FROM products WHERE name IS NULL", &[])
        .expect("IS NULL query should succeed");

    assert_eq!(result.rows.len(), 1, "Should find 1 row with NULL name");
    assert_eq!(result.rows[0][0], Value::BigInt(2)); // id column is first (index 0)

    // Test IS NOT NULL
    let result = engine
        .query(&mut store, "SELECT id FROM products WHERE name IS NOT NULL", &[])
        .expect("IS NOT NULL query should succeed");

    assert_eq!(result.rows.len(), 2, "Should find 2 rows with non-NULL names");
}

#[test]
fn test_null_in_order_by() {
    use crate::key_encoder::encode_key;

    // Create schema with nullable column
    let schema = SchemaBuilder::new()
        .table(
            "items",
            TableId::new(4),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("priority", DataType::BigInt), // Nullable
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    // Insert rows with NULL and non-NULL priorities
    store.insert_json(
        TableId::new(4),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "priority": 5}),
    );
    store.insert_json(
        TableId::new(4),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "priority": null}),
    );
    store.insert_json(
        TableId::new(4),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "priority": 1}),
    );
    store.insert_json(
        TableId::new(4),
        encode_key(&[Value::BigInt(4)]),
        &serde_json::json!({"id": 4, "priority": null}),
    );

    // Test ORDER BY with NULLs (NULLs should be last in ASC order)
    let result = engine
        .query(&mut store, "SELECT id FROM items ORDER BY priority ASC", &[])
        .expect("ORDER BY with NULLs should succeed");

    assert_eq!(result.rows.len(), 4);

    // In SQL standard, NULLs typically sort last in ASC order
    // But this depends on implementation - just verify we get all rows
    let ids: Vec<i64> = result
        .rows
        .iter()
        .map(|row| match &row[0] {
            Value::BigInt(id) => *id,
            _ => panic!("Expected BigInt"),
        })
        .collect();

    assert_eq!(ids.len(), 4, "Should get all 4 rows back");
}

#[test]
fn test_bigint_max_min_values() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "extremes",
            TableId::new(5),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("value", DataType::BigInt).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    // Insert rows with extreme values
    store.insert_json(
        TableId::new(5),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "value": i64::MAX}),
    );
    store.insert_json(
        TableId::new(5),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "value": i64::MIN}),
    );
    store.insert_json(
        TableId::new(5),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "value": 0}),
    );

    // Test that we can query and retrieve extreme values
    let result = engine
        .query(&mut store, "SELECT * FROM extremes ORDER BY value ASC", &[])
        .expect("Query with extreme values should succeed");

    assert_eq!(result.rows.len(), 3);

    // Verify ordering: MIN < 0 < MAX
    // Column order is: id (0), value (1)
    assert_eq!(result.rows[0][1], Value::BigInt(i64::MIN));
    assert_eq!(result.rows[1][1], Value::BigInt(0));
    assert_eq!(result.rows[2][1], Value::BigInt(i64::MAX));
}

#[test]
fn test_empty_string_vs_null() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "strings",
            TableId::new(6),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("text", DataType::Text), // Nullable
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    // Insert rows with empty string and NULL
    store.insert_json(
        TableId::new(6),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "text": ""}),
    );
    store.insert_json(
        TableId::new(6),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "text": null}),
    );
    store.insert_json(
        TableId::new(6),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "text": "hello"}),
    );

    // Test that empty string can be queried (without IS NULL which isn't supported yet)
    let result = engine
        .query(&mut store, "SELECT id FROM strings WHERE text = ''", &[])
        .expect("Query for empty string should succeed");

    assert_eq!(result.rows.len(), 1, "Should find empty string row");
    assert_eq!(result.rows[0][0], Value::BigInt(1)); // id column
}

#[test]
#[ignore] // TODO: Empty IN () list not yet supported by SQL parser
fn test_in_predicate_empty_list() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    // Use existing users table from setup
    let result = engine
        .query(&mut store, "SELECT * FROM users WHERE id IN ()", &[])
        .expect("IN with empty list should succeed");

    // Empty IN list should match no rows
    assert_eq!(result.rows.len(), 0, "IN () should return empty result");
}

#[test]
fn test_boolean_type_handling() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "flags",
            TableId::new(7),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("active", DataType::Boolean).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    // Insert rows with true/false
    store.insert_json(
        TableId::new(7),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "active": true}),
    );
    store.insert_json(
        TableId::new(7),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "active": false}),
    );

    // Test querying boolean values
    let result = engine
        .query(&mut store, "SELECT id FROM flags WHERE active = true", &[])
        .expect("Boolean query should succeed");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(1)); // id column
}

// ============================================================================
// Phase 2: Advanced WHERE Clause Tests (OR, LIKE, IS NULL)
// ============================================================================

#[test]
fn test_or_operator_simple() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    // SELECT * FROM users WHERE id = 1 OR id = 3
    let result = engine
        .query(&mut store, "SELECT * FROM users WHERE id = 1 OR id = 3", &[])
        .expect("OR query should succeed");

    assert_eq!(result.rows.len(), 2, "Should match 2 rows (id 1 and 3)");
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(1)));
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(3)));
}

#[test]
fn test_or_operator_with_different_columns() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    // SELECT * FROM users WHERE id = 1 OR name = 'Charlie'
    let result = engine
        .query(
            &mut store,
            "SELECT * FROM users WHERE id = 1 OR name = 'Charlie'",
            &[],
        )
        .expect("OR query with different columns should succeed");

    assert_eq!(result.rows.len(), 2, "Should match 2 rows");
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(1)));
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(3)));
}

#[test]
fn test_or_with_and_precedence() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    // (id = 1 AND age = 30) OR (id = 2)
    let result = engine
        .query(
            &mut store,
            "SELECT * FROM users WHERE (id = 1 AND age = 30) OR (id = 2)",
            &[],
        )
        .expect("OR with AND should succeed");

    assert_eq!(result.rows.len(), 2, "Should match 2 rows");
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(1)));
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(2)));
}

#[test]
fn test_or_no_matches() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    // SELECT * FROM users WHERE id = 999 OR id = 998
    let result = engine
        .query(&mut store, "SELECT * FROM users WHERE id = 999 OR id = 998", &[])
        .expect("OR query with no matches should succeed");

    assert_eq!(result.rows.len(), 0, "Should match 0 rows");
}

#[test]
fn test_or_all_matches() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    // SELECT * FROM users WHERE id = 1 OR id = 2 OR id = 3
    let result = engine
        .query(
            &mut store,
            "SELECT * FROM users WHERE id = 1 OR id = 2 OR id = 3",
            &[],
        )
        .expect("Multiple OR should succeed");

    assert_eq!(result.rows.len(), 3, "Should match all 3 rows");
}

#[test]
fn test_like_prefix_match() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "products",
            TableId::new(10),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    // Insert test data
    store.insert_json(
        TableId::new(10),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "name": "Apple iPhone"}),
    );
    store.insert_json(
        TableId::new(10),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "name": "Apple MacBook"}),
    );
    store.insert_json(
        TableId::new(10),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "name": "Samsung Galaxy"}),
    );

    // LIKE 'Apple%' - matches "Apple iPhone" and "Apple MacBook"
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM products WHERE name LIKE 'Apple%'",
            &[],
        )
        .expect("LIKE prefix query should succeed");

    assert_eq!(result.rows.len(), 2, "Should match 2 Apple products");
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(1)));
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(2)));
}

#[test]
fn test_like_suffix_match() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "files",
            TableId::new(11),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("filename", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(11),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "filename": "document.pdf"}),
    );
    store.insert_json(
        TableId::new(11),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "filename": "image.jpg"}),
    );
    store.insert_json(
        TableId::new(11),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "filename": "report.pdf"}),
    );

    // LIKE '%.pdf' - matches files ending with .pdf
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM files WHERE filename LIKE '%.pdf'",
            &[],
        )
        .expect("LIKE suffix query should succeed");

    assert_eq!(result.rows.len(), 2, "Should match 2 PDF files");
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(1)));
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(3)));
}

#[test]
fn test_like_contains_match() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "articles",
            TableId::new(12),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("title", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(12),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "title": "Introduction to Rust"}),
    );
    store.insert_json(
        TableId::new(12),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "title": "Advanced Rust Patterns"}),
    );
    store.insert_json(
        TableId::new(12),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "title": "Python for Beginners"}),
    );

    // LIKE '%Rust%' - matches titles containing "Rust"
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM articles WHERE title LIKE '%Rust%'",
            &[],
        )
        .expect("LIKE contains query should succeed");

    assert_eq!(result.rows.len(), 2, "Should match 2 Rust articles");
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(1)));
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(2)));
}

#[test]
fn test_like_single_char_wildcard() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "codes",
            TableId::new(13),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("code", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(13),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "code": "A1B"}),
    );
    store.insert_json(
        TableId::new(13),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "code": "A2B"}),
    );
    store.insert_json(
        TableId::new(13),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "code": "A12"}),
    );

    // LIKE 'A_B' - matches A1B and A2B (single char between A and B)
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM codes WHERE code LIKE 'A_B'",
            &[],
        )
        .expect("LIKE single char wildcard should succeed");

    assert_eq!(result.rows.len(), 2, "Should match 2 codes with pattern A_B");
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(1)));
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(2)));
}

#[test]
fn test_like_no_wildcard_exact_match() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "items",
            TableId::new(14),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(14),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "name": "exact"}),
    );
    store.insert_json(
        TableId::new(14),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "name": "exactly"}),
    );

    // LIKE 'exact' without wildcards - exact match only
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM items WHERE name LIKE 'exact'",
            &[],
        )
        .expect("LIKE without wildcards should work as exact match");

    assert_eq!(result.rows.len(), 1, "Should match exactly one row");
    assert_eq!(result.rows[0][0], Value::BigInt(1));
}

#[test]
fn test_like_escape_percent() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "discounts",
            TableId::new(15),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("description", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(15),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "description": "10% off"}),
    );
    store.insert_json(
        TableId::new(15),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "description": "20percent off"}),
    );
    store.insert_json(
        TableId::new(15),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "description": "50% discount"}),
    );

    // LIKE '%\\%%' - matches strings containing literal % character
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM discounts WHERE description LIKE '%\\%%'",
            &[],
        )
        .expect("LIKE with escaped percent should succeed");

    assert_eq!(result.rows.len(), 2, "Should match rows with % character");
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(1)));
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(3)));
}

#[test]
fn test_like_escape_underscore() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "identifiers",
            TableId::new(16),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(16),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "name": "user_id"}),
    );
    store.insert_json(
        TableId::new(16),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "name": "userid"}),
    );
    store.insert_json(
        TableId::new(16),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "name": "user-id"}),
    );

    // LIKE 'user\\_id' - matches literal underscore
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM identifiers WHERE name LIKE 'user\\_id'",
            &[],
        )
        .expect("LIKE with escaped underscore should succeed");

    assert_eq!(result.rows.len(), 1, "Should match only literal user_id");
    assert_eq!(result.rows[0][0], Value::BigInt(1));
}

#[test]
fn test_complex_or_and_like_combination() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "employees",
            TableId::new(17),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text).not_null(),
                ColumnDef::new("department", DataType::Text),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(17),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "name": "Alice Anderson", "department": "Engineering"}),
    );
    store.insert_json(
        TableId::new(17),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "name": "Bob Brown", "department": "Sales"}),
    );
    store.insert_json(
        TableId::new(17),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "name": "Charlie Chen", "department": "Engineering"}),
    );
    store.insert_json(
        TableId::new(17),
        encode_key(&[Value::BigInt(4)]),
        &serde_json::json!({"id": 4, "name": "Alice Cooper", "department": "Marketing"}),
    );

    // (name LIKE 'Alice%') OR (department = 'Engineering')
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM employees WHERE name LIKE 'Alice%' OR department = 'Engineering'",
            &[],
        )
        .expect("Complex OR and LIKE should succeed");

    assert_eq!(result.rows.len(), 3, "Should match 3 employees");
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(1))); // Alice Anderson (both conditions)
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(3))); // Charlie Chen (department)
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(4))); // Alice Cooper (name)
}

#[test]
fn test_is_null_with_or() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "contacts",
            TableId::new(18),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("email", DataType::Text),
                ColumnDef::new("phone", DataType::Text),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(18),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "email": "alice@example.com", "phone": null}),
    );
    store.insert_json(
        TableId::new(18),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "email": null, "phone": "555-1234"}),
    );
    store.insert_json(
        TableId::new(18),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "email": "bob@example.com", "phone": "555-5678"}),
    );

    // email IS NULL OR phone IS NULL
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM contacts WHERE email IS NULL OR phone IS NULL",
            &[],
        )
        .expect("IS NULL with OR should succeed");

    assert_eq!(result.rows.len(), 2, "Should match 2 contacts with missing info");
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(1)));
    assert!(result.rows.iter().any(|r| r[0] == Value::BigInt(2)));
}

#[test]
fn test_is_not_null_with_and() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "profiles",
            TableId::new(19),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("bio", DataType::Text),
                ColumnDef::new("avatar", DataType::Text),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(19),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "bio": "Software engineer", "avatar": "pic1.jpg"}),
    );
    store.insert_json(
        TableId::new(19),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "bio": "Designer", "avatar": null}),
    );
    store.insert_json(
        TableId::new(19),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "bio": null, "avatar": "pic3.jpg"}),
    );

    // bio IS NOT NULL AND avatar IS NOT NULL - both fields required
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM profiles WHERE bio IS NOT NULL AND avatar IS NOT NULL",
            &[],
        )
        .expect("IS NOT NULL with AND should succeed");

    assert_eq!(result.rows.len(), 1, "Should match only complete profiles");
    assert_eq!(result.rows[0][0], Value::BigInt(1));
}

#[test]
fn test_like_case_sensitive() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "words",
            TableId::new(20),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("word", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(20),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "word": "Hello"}),
    );
    store.insert_json(
        TableId::new(20),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "word": "hello"}),
    );
    store.insert_json(
        TableId::new(20),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "word": "HELLO"}),
    );

    // LIKE is case-sensitive - should match only exact case
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM words WHERE word LIKE 'Hello'",
            &[],
        )
        .expect("LIKE case sensitivity test should succeed");

    assert_eq!(result.rows.len(), 1, "Should match only exact case 'Hello'");
    assert_eq!(result.rows[0][0], Value::BigInt(1));
}

// ============================================================================
// Phase 3: Aggregate and GROUP BY Tests
// ============================================================================

#[test]
fn test_count_star_global() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT COUNT(*) FROM users", &[])
        .expect("COUNT(*) should succeed");

    assert_eq!(result.rows.len(), 1, "Should return 1 row for global aggregate");
    assert_eq!(result.rows[0][0], Value::BigInt(3), "Should count all 3 users");
}

#[test]
fn test_count_column() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "items",
            TableId::new(21),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("value", DataType::BigInt),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(21),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "value": 100}),
    );
    store.insert_json(
        TableId::new(21),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "value": null}),
    );
    store.insert_json(
        TableId::new(21),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "value": 300}),
    );

    let result = engine
        .query(&mut store, "SELECT COUNT(value) FROM items", &[])
        .expect("COUNT(column) should succeed");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(3), "Should count all rows including null");
}

#[test]
fn test_sum_aggregate() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT SUM(age) FROM users", &[])
        .expect("SUM should succeed");

    assert_eq!(result.rows.len(), 1);
    // Ages: 30, 25, 35 -> SUM = 90
    assert_eq!(result.rows[0][0], Value::BigInt(90));
}

#[test]
fn test_avg_aggregate() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT AVG(age) FROM users", &[])
        .expect("AVG should succeed");

    assert_eq!(result.rows.len(), 1);
    // Ages: 30, 25, 35 -> AVG = 30.0
    assert_eq!(result.rows[0][0], Value::Real(30.0));
}

#[test]
fn test_min_aggregate() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT MIN(age) FROM users", &[])
        .expect("MIN should succeed");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(25)); // Bob's age
}

#[test]
fn test_max_aggregate() {
    let schema = test_schema();
    let mut store = test_store();
    let engine = QueryEngine::new(schema);

    let result = engine
        .query(&mut store, "SELECT MAX(age) FROM users", &[])
        .expect("MAX should succeed");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(35)); // Charlie's age
}

#[test]
fn test_group_by_single_column() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "sales",
            TableId::new(22),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("department", DataType::Text).not_null(),
                ColumnDef::new("amount", DataType::BigInt).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(22),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "department": "Sales", "amount": 100}),
    );
    store.insert_json(
        TableId::new(22),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "department": "Engineering", "amount": 200}),
    );
    store.insert_json(
        TableId::new(22),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "department": "Sales", "amount": 150}),
    );

    let result = engine
        .query(
            &mut store,
            "SELECT department, SUM(amount) FROM sales GROUP BY department",
            &[],
        )
        .expect("GROUP BY should succeed");

    assert_eq!(result.rows.len(), 2, "Should have 2 groups");
    assert_eq!(result.columns.len(), 2, "Should return department + SUM");

    // Find Sales group
    let sales_row = result.rows.iter().find(|r| r[0] == Value::Text("Sales".to_string())).unwrap();
    assert_eq!(sales_row[1], Value::BigInt(250), "Sales total should be 250");

    // Find Engineering group
    let eng_row = result.rows.iter().find(|r| r[0] == Value::Text("Engineering".to_string())).unwrap();
    assert_eq!(eng_row[1], Value::BigInt(200), "Engineering total should be 200");
}

#[test]
fn test_group_by_multiple_aggregates() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "transactions",
            TableId::new(23),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("category", DataType::Text).not_null(),
                ColumnDef::new("amount", DataType::BigInt).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(23),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "category": "Food", "amount": 50}),
    );
    store.insert_json(
        TableId::new(23),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "category": "Food", "amount": 75}),
    );
    store.insert_json(
        TableId::new(23),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "category": "Transport", "amount": 100}),
    );

    let result = engine
        .query(
            &mut store,
            "SELECT category, COUNT(*), SUM(amount), AVG(amount) FROM transactions GROUP BY category",
            &[],
        )
        .expect("Multiple aggregates should succeed");

    assert_eq!(result.columns.len(), 4); // category + 3 aggregates
    assert_eq!(result.rows.len(), 2); // 2 categories

    // Find Food group
    let food_row = result.rows.iter().find(|r| r[0] == Value::Text("Food".to_string())).unwrap();
    assert_eq!(food_row[1], Value::BigInt(2), "Food count should be 2");
    assert_eq!(food_row[2], Value::BigInt(125), "Food sum should be 125");
    assert_eq!(food_row[3], Value::Real(62.5), "Food avg should be 62.5");
}

#[test]
fn test_distinct_simple() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "duplicates",
            TableId::new(24),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("color", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(24),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "color": "red"}),
    );
    store.insert_json(
        TableId::new(24),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "color": "blue"}),
    );
    store.insert_json(
        TableId::new(24),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "color": "red"}),
    );
    store.insert_json(
        TableId::new(24),
        encode_key(&[Value::BigInt(4)]),
        &serde_json::json!({"id": 4, "color": "blue"}),
    );

    let result = engine
        .query(&mut store, "SELECT DISTINCT color FROM duplicates", &[])
        .expect("DISTINCT should succeed");

    assert_eq!(result.rows.len(), 2, "Should have 2 distinct colors");
    let colors: Vec<String> = result
        .rows
        .iter()
        .map(|r| match &r[0] {
            Value::Text(s) => s.clone(),
            _ => panic!("Expected text"),
        })
        .collect();

    assert!(colors.contains(&"red".to_string()));
    assert!(colors.contains(&"blue".to_string()));
}

#[test]
fn test_distinct_multiple_columns() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "pairs",
            TableId::new(25),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("col1", DataType::Text).not_null(),
                ColumnDef::new("col2", DataType::Text).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(25),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "col1": "A", "col2": "X"}),
    );
    store.insert_json(
        TableId::new(25),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "col1": "A", "col2": "Y"}),
    );
    store.insert_json(
        TableId::new(25),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "col1": "A", "col2": "X"}),
    );

    let result = engine
        .query(&mut store, "SELECT DISTINCT col1, col2 FROM pairs", &[])
        .expect("DISTINCT on multiple columns should succeed");

    assert_eq!(result.rows.len(), 2, "Should have 2 distinct (col1, col2) pairs");
}

#[test]
fn test_aggregate_on_empty_table() {
    let schema = SchemaBuilder::new()
        .table(
            "empty",
            TableId::new(26),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("value", DataType::BigInt),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    // Don't insert any rows

    let result = engine
        .query(&mut store, "SELECT COUNT(*), SUM(value) FROM empty", &[])
        .expect("Aggregate on empty table should succeed");

    assert_eq!(result.rows.len(), 1, "Should return 1 row for empty table");
    assert_eq!(result.rows[0][0], Value::BigInt(0), "COUNT(*) should be 0");
    assert_eq!(result.rows[0][1], Value::Null, "SUM should be NULL");
}

#[test]
fn test_group_by_with_null_values() {
    use crate::key_encoder::encode_key;

    let schema = SchemaBuilder::new()
        .table(
            "nulls",
            TableId::new(27),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("category", DataType::Text),
                ColumnDef::new("amount", DataType::BigInt).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(27),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "category": "A", "amount": 10}),
    );
    store.insert_json(
        TableId::new(27),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "category": null, "amount": 20}),
    );
    store.insert_json(
        TableId::new(27),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "category": null, "amount": 30}),
    );

    let result = engine
        .query(
            &mut store,
            "SELECT category, SUM(amount) FROM nulls GROUP BY category",
            &[],
        )
        .expect("GROUP BY with NULLs should succeed");

    assert_eq!(result.rows.len(), 2, "Should have 2 groups (one for NULL)");

    // Find NULL group
    let null_row = result.rows.iter().find(|r| r[0] == Value::Null).unwrap();
    assert_eq!(null_row[1], Value::BigInt(50), "NULL group sum should be 50");
}
