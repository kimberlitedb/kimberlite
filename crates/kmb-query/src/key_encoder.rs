//! Lexicographic key encoding for B+tree lookups.
//!
//! This module provides encoding functions that preserve ordering when
//! the encoded bytes are compared lexicographically. This enables efficient
//! range scans on the B+tree store.
//!
//! # Encoding Strategies
//!
//! - **`BigInt`**: Sign-flip encoding (XOR with 0x80 on high byte)
//! - **Text**: UTF-8 bytes as-is (already lexicographic)
//! - **Bytes**: Raw bytes as-is
//! - **Timestamp**: Big-endian u64 (naturally lexicographic)
//! - **Boolean**: 0x00 for false, 0x01 for true

use bytes::Bytes;
use kmb_store::Key;
use kmb_types::Timestamp;

use crate::value::Value;

/// Encodes a `TinyInt` (i8) for lexicographic ordering.
#[allow(clippy::cast_sign_loss)]
pub fn encode_tinyint(value: i8) -> [u8; 1] {
    let unsigned = (value as u8) ^ (1u8 << 7);
    [unsigned]
}

/// Decodes a `TinyInt` from sign-flip encoding.
#[allow(dead_code)]
pub fn decode_tinyint(bytes: [u8; 1]) -> i8 {
    let unsigned = bytes[0];
    (unsigned ^ (1u8 << 7)) as i8
}

/// Encodes a `SmallInt` (i16) for lexicographic ordering.
#[allow(clippy::cast_sign_loss)]
pub fn encode_smallint(value: i16) -> [u8; 2] {
    let unsigned = (value as u16) ^ (1u16 << 15);
    unsigned.to_be_bytes()
}

/// Decodes a `SmallInt` from sign-flip encoding.
#[allow(dead_code)]
pub fn decode_smallint(bytes: [u8; 2]) -> i16 {
    let unsigned = u16::from_be_bytes(bytes);
    (unsigned ^ (1u16 << 15)) as i16
}

/// Encodes an `Integer` (i32) for lexicographic ordering.
#[allow(clippy::cast_sign_loss)]
pub fn encode_integer(value: i32) -> [u8; 4] {
    let unsigned = (value as u32) ^ (1u32 << 31);
    unsigned.to_be_bytes()
}

/// Decodes an `Integer` from sign-flip encoding.
#[allow(dead_code)]
pub fn decode_integer(bytes: [u8; 4]) -> i32 {
    let unsigned = u32::from_be_bytes(bytes);
    (unsigned ^ (1u32 << 31)) as i32
}

/// Encodes a `BigInt` for lexicographic ordering.
///
/// Uses sign-flip encoding: XOR the high byte with 0x80 so that
/// negative numbers sort before positive numbers in byte order.
///
/// ```text
/// i64::MIN (-9223372036854775808) -> 0x00_00_00_00_00_00_00_00
/// -1                              -> 0x7F_FF_FF_FF_FF_FF_FF_FF
///  0                              -> 0x80_00_00_00_00_00_00_00
///  1                              -> 0x80_00_00_00_00_00_00_01
/// i64::MAX (9223372036854775807)  -> 0xFF_FF_FF_FF_FF_FF_FF_FF
/// ```
#[allow(clippy::cast_sign_loss)]
pub fn encode_bigint(value: i64) -> [u8; 8] {
    // Convert to big-endian, then flip the sign bit
    // The cast is intentional for sign-flip encoding.
    let unsigned = (value as u64) ^ (1u64 << 63);
    unsigned.to_be_bytes()
}

/// Decodes a `BigInt` from sign-flip encoding.
#[allow(dead_code)]
pub fn decode_bigint(bytes: [u8; 8]) -> i64 {
    let unsigned = u64::from_be_bytes(bytes);
    (unsigned ^ (1u64 << 63)) as i64
}

/// Encodes a Timestamp for lexicographic ordering.
///
/// Timestamps are u64 nanoseconds, which are naturally ordered
/// when stored as big-endian bytes.
pub fn encode_timestamp(ts: Timestamp) -> [u8; 8] {
    ts.as_nanos().to_be_bytes()
}

/// Decodes a Timestamp from big-endian encoding.
#[allow(dead_code)]
pub fn decode_timestamp(bytes: [u8; 8]) -> Timestamp {
    Timestamp::from_nanos(u64::from_be_bytes(bytes))
}

/// Encodes a `Real` (f64) for lexicographic ordering with total ordering.
///
/// NaN < -Inf < negative values < -0.0 < +0.0 < positive values < +Inf
#[allow(clippy::cast_sign_loss)]
pub fn encode_real(value: f64) -> [u8; 8] {
    let bits = value.to_bits();

    // Sign-flip encoding for total ordering
    let key = if value.is_sign_negative() {
        !bits // Flip all bits for negatives
    } else {
        bits ^ (1u64 << 63) // Flip only sign bit for positives
    };

    key.to_be_bytes()
}

/// Decodes a `Real` from sign-flip encoding.
#[allow(dead_code)]
pub fn decode_real(bytes: [u8; 8]) -> f64 {
    let key = u64::from_be_bytes(bytes);

    // Check if original was negative (MSB is 0 in key)
    let bits = if (key & (1u64 << 63)) == 0 {
        !key // Was negative, flip all bits back
    } else {
        key ^ (1u64 << 63) // Was positive, flip only sign bit
    };

    f64::from_bits(bits)
}

/// Encodes a `Decimal` (i128, u8) for lexicographic ordering.
///
/// Format: [sign-flipped i128 16 bytes][scale 1 byte]
#[allow(clippy::cast_sign_loss)]
pub fn encode_decimal(value: i128, scale: u8) -> [u8; 17] {
    let unsigned = (value as u128) ^ (1u128 << 127);
    let mut bytes = [0u8; 17];
    bytes[0..16].copy_from_slice(&unsigned.to_be_bytes());
    bytes[16] = scale;
    bytes
}

/// Decodes a `Decimal` from sign-flip encoding.
#[allow(dead_code)]
pub fn decode_decimal(bytes: [u8; 17]) -> (i128, u8) {
    let mut value_bytes = [0u8; 16];
    value_bytes.copy_from_slice(&bytes[0..16]);
    let unsigned = u128::from_be_bytes(value_bytes);
    let value = (unsigned ^ (1u128 << 127)) as i128;
    let scale = bytes[16];
    (value, scale)
}

/// Encodes a `Date` (i32) for lexicographic ordering.
#[allow(clippy::cast_sign_loss)]
pub fn encode_date(value: i32) -> [u8; 4] {
    encode_integer(value) // Same as Integer
}

/// Decodes a `Date` from sign-flip encoding.
#[allow(dead_code)]
pub fn decode_date(bytes: [u8; 4]) -> i32 {
    decode_integer(bytes)
}

/// Encodes a `Time` (i64) for lexicographic ordering.
///
/// Time is nanoseconds within a day (always positive), so big-endian is sufficient.
pub fn encode_time(value: i64) -> [u8; 8] {
    value.to_be_bytes()
}

/// Decodes a `Time` from big-endian encoding.
#[allow(dead_code)]
pub fn decode_time(bytes: [u8; 8]) -> i64 {
    i64::from_be_bytes(bytes)
}

/// Encodes a `Uuid` for lexicographic ordering.
///
/// UUIDs are already 16 bytes in a comparable format (RFC 4122).
pub fn encode_uuid(value: [u8; 16]) -> [u8; 16] {
    value
}

/// Decodes a `Uuid` from its encoding.
#[allow(dead_code)]
pub fn decode_uuid(bytes: [u8; 16]) -> [u8; 16] {
    bytes
}

/// Encodes a boolean for lexicographic ordering.
pub fn encode_boolean(value: bool) -> [u8; 1] {
    [u8::from(value)]
}

/// Decodes a boolean from its encoded form.
#[allow(dead_code)]
pub fn decode_boolean(byte: u8) -> bool {
    byte != 0
}

/// Encodes a composite key from multiple values.
///
/// Each value is length-prefixed to enable unambiguous decoding
/// and to handle variable-length types correctly.
///
/// # Encoding Format
///
/// For each value:
/// - 1 byte: Type tag (0x00=Null, 0x01=BigInt, 0x02=Text, 0x03=Boolean, 0x04=Timestamp, 0x05=Bytes,
///                      0x06=Integer, 0x07=SmallInt, 0x08=TinyInt, 0x09=Real, 0x0A=Decimal,
///                      0x0B=Uuid, 0x0C=Json, 0x0D=Date, 0x0E=Time)
/// - Variable: Encoded value
///
/// For variable-length types (Text, Bytes):
/// - 4 bytes: Length (big-endian u32)
/// - N bytes: Data
///
/// # Panics
///
/// Panics if the value is a `Placeholder` or `Json` (JSON is not indexable).
pub fn encode_key(values: &[Value]) -> Key {
    let mut buf = Vec::with_capacity(64);

    for value in values {
        match value {
            Value::Null => {
                buf.push(0x00); // Type tag for NULL
            }
            Value::BigInt(v) => {
                buf.push(0x01); // Type tag for BigInt
                buf.extend_from_slice(&encode_bigint(*v));
            }
            Value::Text(s) => {
                buf.push(0x02); // Type tag for Text
                let bytes = s.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(bytes);
            }
            Value::Boolean(b) => {
                buf.push(0x03); // Type tag for Boolean
                buf.extend_from_slice(&encode_boolean(*b));
            }
            Value::Timestamp(ts) => {
                buf.push(0x04); // Type tag for Timestamp
                buf.extend_from_slice(&encode_timestamp(*ts));
            }
            Value::Bytes(b) => {
                buf.push(0x05); // Type tag for Bytes
                buf.extend_from_slice(&(b.len() as u32).to_be_bytes());
                buf.extend_from_slice(b);
            }
            Value::Integer(v) => {
                buf.push(0x06); // Type tag for Integer
                buf.extend_from_slice(&encode_integer(*v));
            }
            Value::SmallInt(v) => {
                buf.push(0x07); // Type tag for SmallInt
                buf.extend_from_slice(&encode_smallint(*v));
            }
            Value::TinyInt(v) => {
                buf.push(0x08); // Type tag for TinyInt
                buf.extend_from_slice(&encode_tinyint(*v));
            }
            Value::Real(v) => {
                buf.push(0x09); // Type tag for Real
                buf.extend_from_slice(&encode_real(*v));
            }
            Value::Decimal(v, scale) => {
                buf.push(0x0A); // Type tag for Decimal
                buf.extend_from_slice(&encode_decimal(*v, *scale));
            }
            Value::Uuid(u) => {
                buf.push(0x0B); // Type tag for Uuid
                buf.extend_from_slice(&encode_uuid(*u));
            }
            Value::Json(_) => {
                panic!(
                    "JSON values cannot be used in primary keys or indexes - they are not orderable"
                )
            }
            Value::Date(d) => {
                buf.push(0x0D); // Type tag for Date
                buf.extend_from_slice(&encode_date(*d));
            }
            Value::Time(t) => {
                buf.push(0x0E); // Type tag for Time
                buf.extend_from_slice(&encode_time(*t));
            }
            Value::Placeholder(idx) => {
                panic!("Cannot encode unbound placeholder ${idx} - bind parameters first")
            }
        }
    }

    Key::from(buf)
}

/// Decodes a composite key back into values.
///
/// # Panics
///
/// Panics if the encoded data is malformed.
#[allow(dead_code)]
/// Decodes a `BigInt` value from key bytes.
#[inline]
fn decode_bigint_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos + 8 <= bytes.len(),
        "insufficient bytes for BigInt at position {pos}"
    );
    let arr: [u8; 8] = bytes[*pos..*pos + 8]
        .try_into()
        .expect("BigInt decode failed");
    *pos += 8;
    Value::BigInt(decode_bigint(arr))
}

/// Decodes a Text value from key bytes.
#[inline]
fn decode_text_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos + 4 <= bytes.len(),
        "insufficient bytes for Text length at position {pos}"
    );
    let len = u32::from_be_bytes(
        bytes[*pos..*pos + 4]
            .try_into()
            .expect("Text length decode failed"),
    ) as usize;
    *pos += 4;
    debug_assert!(
        *pos + len <= bytes.len(),
        "insufficient bytes for Text data at position {pos}"
    );
    let s =
        std::str::from_utf8(&bytes[*pos..*pos + len]).expect("Text decode failed: invalid UTF-8");
    *pos += len;
    Value::Text(s.to_string())
}

/// Decodes a Boolean value from key bytes.
#[inline]
fn decode_boolean_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos < bytes.len(),
        "insufficient bytes for Boolean at position {pos}"
    );
    let b = decode_boolean(bytes[*pos]);
    *pos += 1;
    Value::Boolean(b)
}

/// Decodes a Timestamp value from key bytes.
#[inline]
fn decode_timestamp_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos + 8 <= bytes.len(),
        "insufficient bytes for Timestamp at position {pos}"
    );
    let arr: [u8; 8] = bytes[*pos..*pos + 8]
        .try_into()
        .expect("Timestamp decode failed");
    *pos += 8;
    Value::Timestamp(decode_timestamp(arr))
}

/// Decodes a Bytes value from key bytes.
#[inline]
fn decode_bytes_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos + 4 <= bytes.len(),
        "insufficient bytes for Bytes length at position {pos}"
    );
    let len = u32::from_be_bytes(
        bytes[*pos..*pos + 4]
            .try_into()
            .expect("Bytes length decode failed"),
    ) as usize;
    *pos += 4;
    debug_assert!(
        *pos + len <= bytes.len(),
        "insufficient bytes for Bytes data at position {pos}"
    );
    let data = Bytes::copy_from_slice(&bytes[*pos..*pos + len]);
    *pos += len;
    Value::Bytes(data)
}

/// Decodes an Integer value from key bytes.
#[inline]
fn decode_integer_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos + 4 <= bytes.len(),
        "insufficient bytes for Integer at position {pos}"
    );
    let arr: [u8; 4] = bytes[*pos..*pos + 4]
        .try_into()
        .expect("Integer decode failed");
    *pos += 4;
    Value::Integer(decode_integer(arr))
}

/// Decodes a `SmallInt` value from key bytes.
#[inline]
fn decode_smallint_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos + 2 <= bytes.len(),
        "insufficient bytes for SmallInt at position {pos}"
    );
    let arr: [u8; 2] = bytes[*pos..*pos + 2]
        .try_into()
        .expect("SmallInt decode failed");
    *pos += 2;
    Value::SmallInt(decode_smallint(arr))
}

/// Decodes a `TinyInt` value from key bytes.
#[inline]
fn decode_tinyint_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos < bytes.len(),
        "insufficient bytes for TinyInt at position {pos}"
    );
    let arr: [u8; 1] = [bytes[*pos]];
    *pos += 1;
    Value::TinyInt(decode_tinyint(arr))
}

/// Decodes a Real value from key bytes.
#[inline]
fn decode_real_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos + 8 <= bytes.len(),
        "insufficient bytes for Real at position {pos}"
    );
    let arr: [u8; 8] = bytes[*pos..*pos + 8]
        .try_into()
        .expect("Real decode failed");
    *pos += 8;
    Value::Real(decode_real(arr))
}

/// Decodes a Decimal value from key bytes.
#[inline]
fn decode_decimal_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos + 17 <= bytes.len(),
        "insufficient bytes for Decimal at position {pos}"
    );
    let arr: [u8; 17] = bytes[*pos..*pos + 17]
        .try_into()
        .expect("Decimal decode failed");
    *pos += 17;
    let (val, scale) = decode_decimal(arr);
    Value::Decimal(val, scale)
}

/// Decodes a Uuid value from key bytes.
#[inline]
fn decode_uuid_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos + 16 <= bytes.len(),
        "insufficient bytes for Uuid at position {pos}"
    );
    let arr: [u8; 16] = bytes[*pos..*pos + 16]
        .try_into()
        .expect("Uuid decode failed");
    *pos += 16;
    Value::Uuid(decode_uuid(arr))
}

/// Decodes a Date value from key bytes.
#[inline]
fn decode_date_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos + 4 <= bytes.len(),
        "insufficient bytes for Date at position {pos}"
    );
    let arr: [u8; 4] = bytes[*pos..*pos + 4]
        .try_into()
        .expect("Date decode failed");
    *pos += 4;
    Value::Date(decode_date(arr))
}

/// Decodes a Time value from key bytes.
#[inline]
fn decode_time_value(bytes: &[u8], pos: &mut usize) -> Value {
    debug_assert!(
        *pos + 8 <= bytes.len(),
        "insufficient bytes for Time at position {pos}"
    );
    let arr: [u8; 8] = bytes[*pos..*pos + 8]
        .try_into()
        .expect("Time decode failed");
    *pos += 8;
    Value::Time(decode_time(arr))
}

pub fn decode_key(key: &Key) -> Vec<Value> {
    let bytes = key.as_bytes();
    let mut values = Vec::new();
    let mut pos = 0;

    while pos < bytes.len() {
        let tag = bytes[pos];
        pos += 1;

        let value = match tag {
            0x00 => Value::Null,
            0x01 => decode_bigint_value(bytes, &mut pos),
            0x02 => decode_text_value(bytes, &mut pos),
            0x03 => decode_boolean_value(bytes, &mut pos),
            0x04 => decode_timestamp_value(bytes, &mut pos),
            0x05 => decode_bytes_value(bytes, &mut pos),
            0x06 => decode_integer_value(bytes, &mut pos),
            0x07 => decode_smallint_value(bytes, &mut pos),
            0x08 => decode_tinyint_value(bytes, &mut pos),
            0x09 => decode_real_value(bytes, &mut pos),
            0x0A => decode_decimal_value(bytes, &mut pos),
            0x0B => decode_uuid_value(bytes, &mut pos),
            0x0C => panic!("JSON values cannot be decoded from keys - they are not indexable"),
            0x0D => decode_date_value(bytes, &mut pos),
            0x0E => decode_time_value(bytes, &mut pos),
            _ => panic!("unknown type tag {tag:#04x} at position {}", pos - 1),
        };

        values.push(value);
    }

    values
}

/// Creates a key that represents the minimum value for a type.
///
/// Useful for constructing range scan bounds.
#[allow(dead_code)]
pub fn min_key_for_type(count: usize) -> Key {
    let values: Vec<Value> = (0..count).map(|_| Value::Null).collect();
    encode_key(&values)
}

/// Creates a key that is greater than any key starting with the given prefix.
///
/// Used for exclusive upper bounds in range scans.
pub fn successor_key(key: &Key) -> Key {
    let bytes = key.as_bytes();
    let mut result = bytes.to_vec();

    // Increment the last byte, carrying as needed
    for i in (0..result.len()).rev() {
        if result[i] < 0xFF {
            result[i] += 1;
            return Key::from(result);
        }
        result[i] = 0x00;
    }

    // All bytes were 0xFF, append 0x00 to make it larger
    result.push(0x00);
    Key::from(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bigint_encoding_preserves_order() {
        let values = [
            i64::MIN,
            i64::MIN + 1,
            -1000,
            -1,
            0,
            1,
            1000,
            i64::MAX - 1,
            i64::MAX,
        ];

        let encoded: Vec<_> = values.iter().map(|&v| encode_bigint(v)).collect();
        let mut sorted = encoded.clone();
        sorted.sort_unstable();

        assert_eq!(encoded, sorted, "BigInt encoding should preserve ordering");

        // Verify decode round-trips
        for &v in &values {
            assert_eq!(decode_bigint(encode_bigint(v)), v);
        }
    }

    #[test]
    fn test_timestamp_encoding_preserves_order() {
        let values = [0u64, 1, 1000, u64::MAX / 2, u64::MAX];

        let encoded: Vec<_> = values
            .iter()
            .map(|&v| encode_timestamp(Timestamp::from_nanos(v)))
            .collect();
        let mut sorted = encoded.clone();
        sorted.sort_unstable();

        assert_eq!(
            encoded, sorted,
            "Timestamp encoding should preserve ordering"
        );

        // Verify decode round-trips
        for &v in &values {
            let ts = Timestamp::from_nanos(v);
            assert_eq!(decode_timestamp(encode_timestamp(ts)), ts);
        }
    }

    #[test]
    fn test_composite_key_round_trip() {
        let values = vec![
            Value::BigInt(42),
            Value::Text("hello".to_string()),
            Value::Boolean(true),
            Value::Timestamp(Timestamp::from_nanos(12345)),
            Value::Bytes(Bytes::from_static(b"data")),
        ];

        let key = encode_key(&values);
        let decoded = decode_key(&key);

        assert_eq!(values, decoded);
    }

    #[test]
    fn test_composite_key_ordering() {
        // Keys with same first value, different second value
        let key1 = encode_key(&[Value::BigInt(1), Value::BigInt(1)]);
        let key2 = encode_key(&[Value::BigInt(1), Value::BigInt(2)]);
        let key3 = encode_key(&[Value::BigInt(2), Value::BigInt(1)]);

        assert!(key1 < key2, "key1 should be less than key2");
        assert!(key2 < key3, "key2 should be less than key3");
    }

    #[test]
    fn test_successor_key() {
        let key = encode_key(&[Value::BigInt(42)]);
        let succ = successor_key(&key);

        assert!(key < succ, "successor should be greater");
    }

    #[test]
    fn test_null_handling() {
        let key = encode_key(&[Value::Null]);
        let decoded = decode_key(&key);
        assert_eq!(decoded, vec![Value::Null]);
    }
}
