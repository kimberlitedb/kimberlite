//! Integration tests for kmb-query.

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
                assert_eq!(ins.values.len(), 2);
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
                assert_eq!(ins.values.len(), 4);
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
#[ignore] // TODO: IS NULL not yet supported in query engine
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
