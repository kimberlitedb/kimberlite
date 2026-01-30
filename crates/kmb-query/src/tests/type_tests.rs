//! Tests for the extended type system (14 types total).
//!
//! Covers encoding, decoding, ordering, and validation for all types.

use crate::key_encoder::{
    decode_date, decode_decimal, decode_integer, decode_real, decode_smallint,
    decode_time, decode_tinyint, decode_uuid, encode_date, encode_decimal,
    encode_integer, encode_key, encode_real, encode_smallint, encode_time, encode_tinyint,
    encode_uuid,
};
use crate::value::Value;

#[test]
fn test_tinyint_encoding_preserves_order() {
    let values = [i8::MIN, i8::MIN + 1, -1, 0, 1, i8::MAX - 1, i8::MAX];

    let encoded: Vec<_> = values.iter().map(|&v| encode_tinyint(v)).collect();
    let mut sorted = encoded.clone();
    sorted.sort_unstable();

    assert_eq!(encoded, sorted, "TinyInt encoding should preserve ordering");

    // Verify decode round-trips
    for &v in &values {
        assert_eq!(decode_tinyint(encode_tinyint(v)), v);
    }
}

#[test]
fn test_smallint_encoding_preserves_order() {
    let values = [
        i16::MIN,
        i16::MIN + 1,
        -1000,
        -1,
        0,
        1,
        1000,
        i16::MAX - 1,
        i16::MAX,
    ];

    let encoded: Vec<_> = values.iter().map(|&v| encode_smallint(v)).collect();
    let mut sorted = encoded.clone();
    sorted.sort_unstable();

    assert_eq!(
        encoded, sorted,
        "SmallInt encoding should preserve ordering"
    );

    // Verify decode round-trips
    for &v in &values {
        assert_eq!(decode_smallint(encode_smallint(v)), v);
    }
}

#[test]
fn test_integer_encoding_preserves_order() {
    let values = [
        i32::MIN,
        i32::MIN + 1,
        -1000000,
        -1,
        0,
        1,
        1000000,
        i32::MAX - 1,
        i32::MAX,
    ];

    let encoded: Vec<_> = values.iter().map(|&v| encode_integer(v)).collect();
    let mut sorted = encoded.clone();
    sorted.sort_unstable();

    assert_eq!(encoded, sorted, "Integer encoding should preserve ordering");

    // Verify decode round-trips
    for &v in &values {
        assert_eq!(decode_integer(encode_integer(v)), v);
    }
}

#[test]
fn test_real_total_ordering() {
    // Test that encoding preserves ordering for sortable values
    let values = [
        f64::NEG_INFINITY,
        -1000.0,
        -1.0,
        -0.0,
        0.0,
        1.0,
        1000.0,
        f64::INFINITY,
    ];

    let encoded: Vec<_> = values.iter().map(|&v| encode_real(v)).collect();
    let mut sorted = encoded.clone();
    sorted.sort_unstable();

    assert_eq!(
        encoded, sorted,
        "Real encoding should preserve total ordering for non-NaN values"
    );

    // Verify decode round-trips
    for &v in &values {
        let decoded = decode_real(encode_real(v));
        assert_eq!(decoded, v, "Value should round-trip");
    }

    // Test NaN separately
    let nan = f64::NAN;
    let decoded_nan = decode_real(encode_real(nan));
    assert!(decoded_nan.is_nan(), "NaN should decode to NaN");
}

#[test]
fn test_real_negative_zero_vs_positive_zero() {
    let neg_zero = -0.0f64;
    let pos_zero = 0.0f64;

    let encoded_neg = encode_real(neg_zero);
    let encoded_pos = encode_real(pos_zero);

    // Negative zero should sort before positive zero
    assert!(
        encoded_neg < encoded_pos,
        "Negative zero should sort before positive zero"
    );
}

#[test]
fn test_decimal_encoding_preserves_order() {
    let scale = 2;
    let values = [
        (i128::MIN, scale),
        (-1000000, scale),
        (-100, scale),
        (0, scale),
        (100, scale),
        (1000000, scale),
        (i128::MAX, scale),
    ];

    let encoded: Vec<_> = values
        .iter()
        .map(|&(v, s)| encode_decimal(v, s))
        .collect();
    let mut sorted = encoded.clone();
    sorted.sort_unstable();

    assert_eq!(
        encoded, sorted,
        "Decimal encoding should preserve ordering"
    );

    // Verify decode round-trips
    for &(v, s) in &values {
        assert_eq!(decode_decimal(encode_decimal(v, s)), (v, s));
    }
}

#[test]
fn test_date_encoding_preserves_order() {
    let values = [i32::MIN, -365, -1, 0, 1, 365, 10000, i32::MAX];

    let encoded: Vec<_> = values.iter().map(|&v| encode_date(v)).collect();
    let mut sorted = encoded.clone();
    sorted.sort_unstable();

    assert_eq!(encoded, sorted, "Date encoding should preserve ordering");

    // Verify decode round-trips
    for &v in &values {
        assert_eq!(decode_date(encode_date(v)), v);
    }
}

#[test]
fn test_time_encoding_preserves_order() {
    let values = [
        0i64,
        1_000_000_000,             // 1 second
        3_600_000_000_000,         // 1 hour
        86_400_000_000_000 - 1,    // Last nanosecond of day
    ];

    let encoded: Vec<_> = values.iter().map(|&v| encode_time(v)).collect();
    let mut sorted = encoded.clone();
    sorted.sort_unstable();

    assert_eq!(encoded, sorted, "Time encoding should preserve ordering");

    // Verify decode round-trips
    for &v in &values {
        assert_eq!(decode_time(encode_time(v)), v);
    }
}

#[test]
fn test_uuid_encoding_round_trip() {
    let uuids = [
        [0u8; 16],
        [255u8; 16],
        [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
            0x32, 0x10,
        ],
    ];

    for uuid in &uuids {
        assert_eq!(decode_uuid(encode_uuid(*uuid)), *uuid);
    }
}

#[test]
fn test_composite_key_with_new_types() {
    let values = vec![
        Value::TinyInt(42),
        Value::SmallInt(1000),
        Value::Integer(100000),
        Value::Real(3.14159),
        Value::Decimal(12345, 2), // 123.45
        Value::Uuid([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
        Value::Date(19000), // Days since epoch
        Value::Time(43200_000_000_000), // Noon
    ];

    let key = encode_key(&values);

    // Verify key is non-empty
    assert!(!key.as_bytes().is_empty());

    // Verify key length is reasonable
    // 1 byte tag + data for each: 1+1 + 1+2 + 1+4 + 1+8 + 1+17 + 1+16 + 1+4 + 1+8 = 66 bytes
    // But Text and Bytes also need length prefix, so may vary
    assert!(key.as_bytes().len() >= 64, "Key should be at least 64 bytes");
}

#[test]
fn test_value_compare_tinyint() {
    assert_eq!(
        Value::TinyInt(-1).compare(&Value::TinyInt(0)),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        Value::TinyInt(0).compare(&Value::TinyInt(0)),
        Some(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        Value::TinyInt(1).compare(&Value::TinyInt(0)),
        Some(std::cmp::Ordering::Greater)
    );
}

#[test]
fn test_value_compare_real_with_special_values() {
    let neg_inf = Value::Real(f64::NEG_INFINITY);
    let zero = Value::Real(0.0);
    let inf = Value::Real(f64::INFINITY);

    // -Inf < 0 < Inf
    assert_eq!(neg_inf.compare(&zero), Some(std::cmp::Ordering::Less));
    assert_eq!(zero.compare(&inf), Some(std::cmp::Ordering::Less));
    assert_eq!(neg_inf.compare(&inf), Some(std::cmp::Ordering::Less));

    // Test NaN comparison (NaN uses total ordering in our implementation)
    let nan = Value::Real(f64::NAN);
    assert_eq!(nan.compare(&nan), Some(std::cmp::Ordering::Equal));
}

#[test]
fn test_value_compare_decimal_same_scale() {
    let a = Value::Decimal(12345, 2); // 123.45
    let b = Value::Decimal(12346, 2); // 123.46

    assert_eq!(a.compare(&b), Some(std::cmp::Ordering::Less));
}

#[test]
fn test_value_compare_decimal_different_scale() {
    let a = Value::Decimal(12345, 2); // 123.45
    let b = Value::Decimal(12345, 3); // 12.345

    // Different scales are incomparable
    assert_eq!(a.compare(&b), None);
}

#[test]
fn test_value_compare_different_types() {
    let a = Value::Integer(42);
    let b = Value::BigInt(42);

    // Different types are incomparable (strict typing)
    assert_eq!(a.compare(&b), None);
}

#[test]
fn test_value_to_json_all_types() {
    use serde_json::json;

    assert_eq!(Value::TinyInt(42).to_json(), json!(42));
    assert_eq!(Value::SmallInt(1000).to_json(), json!(1000));
    assert_eq!(Value::Integer(100000).to_json(), json!(100000));
    assert_eq!(Value::Real(3.14).to_json(), json!(3.14));
    assert_eq!(Value::Decimal(12345, 2).to_json(), json!("123.45"));

    let uuid_str = Value::Uuid([
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
        0x32, 0x10,
    ])
    .to_json();
    assert_eq!(uuid_str, json!("01234567-89ab-cdef-fedc-ba9876543210"));

    assert_eq!(Value::Date(19000).to_json(), json!(19000));
    assert_eq!(Value::Time(43200_000_000_000).to_json(), json!(43200000000000i64));

    let json_val = Value::Json(json!({"key": "value"}));
    assert_eq!(json_val.to_json(), json!({"key": "value"}));
}

#[test]
fn test_value_display() {
    assert_eq!(format!("{}", Value::TinyInt(42)), "42");
    assert_eq!(format!("{}", Value::SmallInt(1000)), "1000");
    assert_eq!(format!("{}", Value::Integer(100000)), "100000");
    assert_eq!(format!("{}", Value::Real(3.14)), "3.14");
    assert_eq!(format!("{}", Value::Decimal(12345, 2)), "123.45");
    assert_eq!(
        format!(
            "{}",
            Value::Uuid([
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76,
                0x54, 0x32, 0x10
            ])
        ),
        "01234567-89ab-cdef-fedc-ba9876543210"
    );
    assert_eq!(format!("{}", Value::Date(19000)), "DATE(19000)");
    assert_eq!(
        format!("{}", Value::Time(43200_000_000_000)),
        "TIME(43200000000000)"
    );
}

#[test]
fn test_null_comparison_with_all_types() {
    // NULL < all non-NULL values
    assert_eq!(
        Value::Null.compare(&Value::TinyInt(0)),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        Value::Null.compare(&Value::Real(0.0)),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        Value::Null.compare(&Value::Uuid([0; 16])),
        Some(std::cmp::Ordering::Less)
    );

    // NULL == NULL
    assert_eq!(
        Value::Null.compare(&Value::Null),
        Some(std::cmp::Ordering::Equal)
    );
}

#[test]
fn test_value_equality() {
    // Same type, same value
    assert_eq!(Value::Integer(42), Value::Integer(42));
    assert_ne!(Value::Integer(42), Value::Integer(43));

    // Different types, same underlying value
    assert_ne!(Value::Integer(42), Value::BigInt(42));

    // Float equality with total ordering
    assert_eq!(Value::Real(3.14), Value::Real(3.14));
    assert_eq!(Value::Real(f64::NAN), Value::Real(f64::NAN)); // NaN == NaN with total ordering
}

#[test]
#[should_panic(expected = "JSON values cannot be used in primary keys")]
fn test_json_cannot_be_encoded_as_key() {
    let values = vec![Value::Json(serde_json::json!({"key": "value"}))];
    encode_key(&values);
}

#[test]
fn test_all_integer_types_distinct() {
    // Verify that TinyInt, SmallInt, Integer, BigInt are distinct types
    let tiny = Value::TinyInt(42);
    let small = Value::SmallInt(42);
    let int = Value::Integer(42);
    let big = Value::BigInt(42);

    // All should be incomparable (different types)
    assert_eq!(tiny.compare(&small), None);
    assert_eq!(tiny.compare(&int), None);
    assert_eq!(tiny.compare(&big), None);
    assert_eq!(small.compare(&int), None);
    assert_eq!(small.compare(&big), None);
    assert_eq!(int.compare(&big), None);
}
