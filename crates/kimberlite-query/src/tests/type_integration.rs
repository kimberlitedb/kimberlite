//! End-to-end integration tests for all 14 data types.
//!
//! Tests CREATE TABLE, INSERT, SELECT, WHERE, and ORDER BY for each type.

use crate::QueryEngine;
use crate::key_encoder::encode_key;
use crate::schema::{ColumnDef, DataType, SchemaBuilder};
use crate::tests::MockStore;
use crate::value::Value;
use bytes::Bytes;
use kimberlite_store::TableId;
use kimberlite_types::Timestamp;

// Helper macro to reduce boilerplate
macro_rules! type_integration_test {
    ($test_name:ident, $type_name:ident, $test_values:expr) => {
        #[test]
        fn $test_name() {
            let schema = SchemaBuilder::new()
                .table(
                    "test_table",
                    TableId::new(200),
                    vec![
                        ColumnDef::new("id", DataType::BigInt).not_null(),
                        ColumnDef::new("value", DataType::$type_name).not_null(),
                    ],
                    vec!["id".into()],
                )
                .build();

            let mut store = MockStore::new();
            let engine = QueryEngine::new(schema);

            let test_values: &[(i64, Value)] = $test_values;

            // Insert test values
            for (id, value) in test_values {
                store.insert_json(
                    TableId::new(200),
                    encode_key(&[Value::BigInt(*id)]),
                    &serde_json::json!({"id": id, "value": value.to_json()}),
                );
            }

            // Test SELECT *
            let result = engine
                .query(&mut store, "SELECT * FROM test_table ORDER BY id", &[])
                .expect("SELECT * should succeed");

            assert_eq!(result.rows.len(), test_values.len());
            for (i, (id, expected_value)) in test_values.iter().enumerate() {
                assert_eq!(
                    result.rows[i][0],
                    Value::BigInt(*id),
                    "Row {} id mismatch",
                    i
                );
                assert_eq!(
                    result.rows[i][1], *expected_value,
                    "Row {} value mismatch",
                    i
                );
            }

            // Test WHERE clause with first value
            let (first_id, first_value) = &test_values[0];
            let result = engine
                .query(
                    &mut store,
                    "SELECT * FROM test_table WHERE value = $1",
                    &[first_value.clone()],
                )
                .expect("WHERE clause should succeed");

            assert_eq!(result.rows.len(), 1);
            assert_eq!(result.rows[0][0], Value::BigInt(*first_id));

            // Test ORDER BY value DESC
            let result = engine
                .query(
                    &mut store,
                    "SELECT id FROM test_table ORDER BY value DESC",
                    &[],
                )
                .expect("ORDER BY should succeed");

            // Verify descending order - should be reverse of input order
            let expected_desc: Vec<_> = test_values.iter().rev().map(|(id, _)| *id).collect();
            for (i, expected_id) in expected_desc.iter().enumerate() {
                assert_eq!(
                    result.rows[i][0],
                    Value::BigInt(*expected_id),
                    "ORDER BY DESC row {} mismatch",
                    i
                );
            }
        }
    };
}

// TinyInt tests
type_integration_test!(
    test_tinyint_integration,
    TinyInt,
    &[
        (1, Value::TinyInt(i8::MIN)),
        (2, Value::TinyInt(0)),
        (3, Value::TinyInt(i8::MAX)),
    ]
);

// SmallInt tests
type_integration_test!(
    test_smallint_integration,
    SmallInt,
    &[
        (1, Value::SmallInt(i16::MIN)),
        (2, Value::SmallInt(0)),
        (3, Value::SmallInt(i16::MAX)),
    ]
);

// Integer tests
type_integration_test!(
    test_integer_integration,
    Integer,
    &[
        (1, Value::Integer(i32::MIN)),
        (2, Value::Integer(0)),
        (3, Value::Integer(i32::MAX)),
    ]
);

// BigInt tests
type_integration_test!(
    test_bigint_integration,
    BigInt,
    &[
        (1, Value::BigInt(i64::MIN)),
        (2, Value::BigInt(0)),
        (3, Value::BigInt(i64::MAX)),
    ]
);

// Real tests
type_integration_test!(
    test_real_integration,
    Real,
    &[
        (1, Value::Real(-1.0e308)),
        (2, Value::Real(0.0)),
        (3, Value::Real(1.0e308)),
    ]
);

// Decimal tests - Note: Using default precision/scale in macro
#[test]
fn test_decimal_integration() {
    let schema = SchemaBuilder::new()
        .table(
            "test_table",
            TableId::new(200),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new(
                    "value",
                    DataType::Decimal {
                        precision: 18,
                        scale: 2,
                    },
                )
                .not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    let test_values: &[(i64, Value)] = &[
        (1, Value::Decimal(-123456, 2)), // -1234.56
        (2, Value::Decimal(0, 2)),       // 0.00
        (3, Value::Decimal(999999, 2)),  // 9999.99
    ];

    // Insert test values - use string representation for decimals
    store.insert_json(
        TableId::new(200),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "value": "-1234.56"}),
    );
    store.insert_json(
        TableId::new(200),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "value": "0.00"}),
    );
    store.insert_json(
        TableId::new(200),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "value": "9999.99"}),
    );

    // Test SELECT *
    let result = engine
        .query(&mut store, "SELECT * FROM test_table ORDER BY id", &[])
        .expect("SELECT * should succeed");

    assert_eq!(result.rows.len(), test_values.len());
    for (i, (id, expected_value)) in test_values.iter().enumerate() {
        assert_eq!(result.rows[i][0], Value::BigInt(*id), "Row {i} id mismatch");
        assert_eq!(result.rows[i][1], *expected_value, "Row {i} value mismatch");
    }

    // Test WHERE clause
    let (first_id, first_value) = &test_values[0];
    let result = engine
        .query(
            &mut store,
            "SELECT * FROM test_table WHERE value = $1",
            &[first_value.clone()],
        )
        .expect("WHERE clause should succeed");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(*first_id));

    // Test ORDER BY value DESC
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM test_table ORDER BY value DESC",
            &[],
        )
        .expect("ORDER BY should succeed");

    let expected_desc: Vec<_> = test_values.iter().rev().map(|(id, _)| *id).collect();
    for (i, expected_id) in expected_desc.iter().enumerate() {
        assert_eq!(
            result.rows[i][0],
            Value::BigInt(*expected_id),
            "ORDER BY DESC row {i} mismatch"
        );
    }
}

// Text tests
type_integration_test!(
    test_text_integration,
    Text,
    &[
        (1, Value::Text(String::new())),
        (2, Value::Text("hello".to_string())),
        (3, Value::Text("zzz".to_string())),
    ]
);

// Bytes tests
type_integration_test!(
    test_bytes_integration,
    Bytes,
    &[
        (1, Value::Bytes(Bytes::from(vec![]))),
        (2, Value::Bytes(Bytes::from(vec![0x00, 0x01, 0x02]))),
        (3, Value::Bytes(Bytes::from(vec![0xFF, 0xFE, 0xFD]))),
    ]
);

// Boolean tests - Note: Only 2 values possible, so we use false and true
#[test]
fn test_boolean_integration() {
    let schema = SchemaBuilder::new()
        .table(
            "test_table",
            TableId::new(200),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("value", DataType::Boolean).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    let test_values: &[(i64, Value)] = &[(1, Value::Boolean(false)), (2, Value::Boolean(true))];

    // Insert test values
    for (id, value) in test_values {
        store.insert_json(
            TableId::new(200),
            encode_key(&[Value::BigInt(*id)]),
            &serde_json::json!({"id": id, "value": value.to_json()}),
        );
    }

    // Test SELECT *
    let result = engine
        .query(&mut store, "SELECT * FROM test_table ORDER BY id", &[])
        .expect("SELECT * should succeed");

    assert_eq!(result.rows.len(), test_values.len());
    for (i, (id, expected_value)) in test_values.iter().enumerate() {
        assert_eq!(result.rows[i][0], Value::BigInt(*id), "Row {i} id mismatch");
        assert_eq!(result.rows[i][1], *expected_value, "Row {i} value mismatch");
    }

    // Test WHERE clause
    let (first_id, first_value) = &test_values[0];
    let result = engine
        .query(
            &mut store,
            "SELECT * FROM test_table WHERE value = $1",
            &[first_value.clone()],
        )
        .expect("WHERE clause should succeed");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(*first_id));

    // Test ORDER BY value DESC
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM test_table ORDER BY value DESC",
            &[],
        )
        .expect("ORDER BY should succeed");

    // Descending order: true (2) then false (1)
    assert_eq!(
        result.rows[0][0],
        Value::BigInt(2),
        "First should be true (id=2)"
    );
    assert_eq!(
        result.rows[1][0],
        Value::BigInt(1),
        "Second should be false (id=1)"
    );
}

// Date tests
type_integration_test!(
    test_date_integration,
    Date,
    &[
        (1, Value::Date(0)),      // Unix epoch
        (2, Value::Date(10000)),  // ~27 years after epoch
        (3, Value::Date(100000)), // ~273 years after epoch
    ]
);

// Time tests
type_integration_test!(
    test_time_integration,
    Time,
    &[
        (1, Value::Time(0)),                 // Midnight
        (2, Value::Time(43200_000_000_000)), // Noon
        (3, Value::Time(86399_999_999_999)), // Last nanosecond before midnight
    ]
);

// Timestamp tests
type_integration_test!(
    test_timestamp_integration,
    Timestamp,
    &[
        (1, Value::Timestamp(Timestamp::from_nanos(0))), // Unix epoch
        (
            2,
            Value::Timestamp(Timestamp::from_nanos(1_000_000_000_000_000_000))
        ), // 2001-09-09
        (
            3,
            Value::Timestamp(Timestamp::from_nanos(2_000_000_000_000_000_000))
        ), // 2033-05-18
    ]
);

// UUID tests
type_integration_test!(
    test_uuid_integration,
    Uuid,
    &[
        (1, Value::Uuid([0u8; 16])),
        (
            2,
            Value::Uuid([
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
                0x32, 0x10,
            ])
        ),
        (3, Value::Uuid([0xFFu8; 16])),
    ]
);

// JSON tests - Note: JSON cannot be used in WHERE or ORDER BY
#[test]
fn test_json_integration() {
    let schema = SchemaBuilder::new()
        .table(
            "test_table",
            TableId::new(201),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("data", DataType::Json).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    // Insert various JSON values
    store.insert_json(
        TableId::new(201),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({"id": 1, "data": {"status": "active"}}),
    );
    store.insert_json(
        TableId::new(201),
        encode_key(&[Value::BigInt(2)]),
        &serde_json::json!({"id": 2, "data": {"key": "value"}}),
    );
    store.insert_json(
        TableId::new(201),
        encode_key(&[Value::BigInt(3)]),
        &serde_json::json!({"id": 3, "data": [1, 2, 3]}),
    );

    // Test SELECT *
    let result = engine
        .query(&mut store, "SELECT * FROM test_table ORDER BY id", &[])
        .expect("SELECT * should succeed");

    assert_eq!(result.rows.len(), 3);
    assert_eq!(
        result.rows[0][1],
        Value::Json(serde_json::json!({"status": "active"})),
        "JSON object 1"
    );
    assert_eq!(
        result.rows[1][1],
        Value::Json(serde_json::json!({"key": "value"})),
        "JSON object 2"
    );
    assert_eq!(
        result.rows[2][1],
        Value::Json(serde_json::json!([1, 2, 3])),
        "JSON array"
    );
}

// Test NULL values for nullable columns
#[test]
fn test_nullable_columns_all_types() {
    let schema = SchemaBuilder::new()
        .table(
            "nullable_test",
            TableId::new(202),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("tiny", DataType::TinyInt), // Nullable
                ColumnDef::new("small", DataType::SmallInt), // Nullable
                ColumnDef::new("int", DataType::Integer),  // Nullable
                ColumnDef::new("big", DataType::BigInt),   // Nullable
                ColumnDef::new("real", DataType::Real),    // Nullable
                ColumnDef::new(
                    "decimal",
                    DataType::Decimal {
                        precision: 18,
                        scale: 2,
                    },
                ), // Nullable
                ColumnDef::new("text", DataType::Text),    // Nullable
                ColumnDef::new("bytes", DataType::Bytes),  // Nullable
                ColumnDef::new("bool", DataType::Boolean), // Nullable
                ColumnDef::new("date", DataType::Date),    // Nullable
                ColumnDef::new("time", DataType::Time),    // Nullable
                ColumnDef::new("timestamp", DataType::Timestamp), // Nullable
                ColumnDef::new("uuid", DataType::Uuid),    // Nullable
                ColumnDef::new("json", DataType::Json),    // Nullable
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    // Insert row with all NULLs
    store.insert_json(
        TableId::new(202),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({
            "id": 1,
            "tiny": null,
            "small": null,
            "int": null,
            "big": null,
            "real": null,
            "decimal": null,
            "text": null,
            "bytes": null,
            "bool": null,
            "date": null,
            "time": null,
            "timestamp": null,
            "uuid": null,
            "json": null,
        }),
    );

    // Test SELECT *
    let result = engine
        .query(&mut store, "SELECT * FROM nullable_test", &[])
        .expect("SELECT should succeed");

    assert_eq!(result.rows.len(), 1);
    // All columns except id should be NULL
    for i in 1..15 {
        assert_eq!(result.rows[0][i], Value::Null, "Column {i} should be NULL");
    }

    // Test WHERE IS NULL
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM nullable_test WHERE tiny IS NULL",
            &[],
        )
        .expect("WHERE IS NULL should succeed");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::BigInt(1));
}

// Test boundary values for all numeric types
#[test]
fn test_numeric_boundary_values() {
    let schema = SchemaBuilder::new()
        .table(
            "boundaries",
            TableId::new(203),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("type_name", DataType::Text).not_null(),
                ColumnDef::new("tiny_min", DataType::TinyInt),
                ColumnDef::new("tiny_max", DataType::TinyInt),
                ColumnDef::new("small_min", DataType::SmallInt),
                ColumnDef::new("small_max", DataType::SmallInt),
                ColumnDef::new("int_min", DataType::Integer),
                ColumnDef::new("int_max", DataType::Integer),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    store.insert_json(
        TableId::new(203),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({
            "id": 1,
            "type_name": "boundaries",
            "tiny_min": i8::MIN,
            "tiny_max": i8::MAX,
            "small_min": i16::MIN,
            "small_max": i16::MAX,
            "int_min": i32::MIN,
            "int_max": i32::MAX,
        }),
    );

    let result = engine
        .query(&mut store, "SELECT * FROM boundaries", &[])
        .expect("SELECT should succeed");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][2], Value::TinyInt(i8::MIN));
    assert_eq!(result.rows[0][3], Value::TinyInt(i8::MAX));
    assert_eq!(result.rows[0][4], Value::SmallInt(i16::MIN));
    assert_eq!(result.rows[0][5], Value::SmallInt(i16::MAX));
    assert_eq!(result.rows[0][6], Value::Integer(i32::MIN));
    assert_eq!(result.rows[0][7], Value::Integer(i32::MAX));
}

// Test large text and bytes values
#[test]
fn test_large_text_and_bytes() {
    use base64::{Engine, engine::general_purpose::STANDARD};

    let schema = SchemaBuilder::new()
        .table(
            "large_data",
            TableId::new(204),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("large_text", DataType::Text).not_null(),
                ColumnDef::new("large_bytes", DataType::Bytes).not_null(),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MockStore::new();
    let engine = QueryEngine::new(schema);

    // Create 1KB text and bytes
    let large_text = "x".repeat(1024);
    let large_bytes: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
    let large_bytes_b64 = STANDARD.encode(&large_bytes);

    store.insert_json(
        TableId::new(204),
        encode_key(&[Value::BigInt(1)]),
        &serde_json::json!({
            "id": 1,
            "large_text": large_text,
            "large_bytes": large_bytes_b64,  // Use base64 encoding
        }),
    );

    let result = engine
        .query(&mut store, "SELECT * FROM large_data", &[])
        .expect("SELECT should succeed");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][1], Value::Text(large_text));
    assert_eq!(result.rows[0][2], Value::Bytes(Bytes::from(large_bytes)));
}
