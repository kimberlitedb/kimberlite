//! Typed SQL values.

#![allow(clippy::match_same_arms)]

use std::cmp::Ordering;
use std::fmt::{self, Display};

use bytes::Bytes;
use kimberlite_types::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::{QueryError, Result};
use crate::schema::DataType;

/// A typed SQL value.
///
/// Represents values that can appear in query parameters, row data,
/// and comparison predicates.
///
/// Note: Real and Decimal types use total ordering (NaN < -Inf < values < Inf)
/// for comparisons to enable use in B+tree indexes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    /// SQL NULL.
    #[default]
    Null,

    // ===== Integer Types =====
    /// 8-bit signed integer (-128 to 127).
    TinyInt(i8),
    /// 16-bit signed integer (-32,768 to 32,767).
    SmallInt(i16),
    /// 32-bit signed integer (-2^31 to 2^31-1).
    Integer(i32),
    /// 64-bit signed integer (-2^63 to 2^63-1).
    BigInt(i64),

    // ===== Numeric Types =====
    /// 64-bit floating point (IEEE 754 double precision).
    Real(f64),
    /// Fixed-precision decimal (value in smallest units, scale).
    ///
    /// Stored as (i128, u8) where the second field is the scale.
    /// Example: Decimal(12345, 2) represents 123.45
    #[serde(skip)] // Complex serialization, handled separately
    Decimal(i128, u8),

    // ===== String Types =====
    /// UTF-8 text string.
    Text(String),

    // ===== Binary Types =====
    /// Raw bytes (base64 encoded in JSON).
    #[serde(with = "bytes_base64")]
    Bytes(Bytes),

    // ===== Boolean Type =====
    /// Boolean value.
    Boolean(bool),

    // ===== Date/Time Types =====
    /// Date (days since Unix epoch).
    Date(i32),
    /// Time of day (nanoseconds within day).
    Time(i64),
    /// Timestamp (nanoseconds since Unix epoch).
    Timestamp(Timestamp),

    // ===== Structured Types =====
    /// UUID (RFC 4122, 128-bit).
    Uuid([u8; 16]),
    /// JSON document (validated).
    Json(serde_json::Value),

    /// Parameter placeholder ($1, $2, etc.) - 1-indexed.
    /// This is an intermediate representation used during parsing,
    /// and should be bound to actual values before execution.
    #[serde(skip)]
    Placeholder(usize),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::TinyInt(a), Value::TinyInt(b)) => a == b,
            (Value::SmallInt(a), Value::SmallInt(b)) => a == b,
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::BigInt(a), Value::BigInt(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => {
                // Use total ordering for floats: NaN == NaN
                a.to_bits() == b.to_bits()
            }
            (Value::Decimal(a_val, a_scale), Value::Decimal(b_val, b_scale)) => {
                a_val == b_val && a_scale == b_scale
            }
            (Value::Text(a), Value::Text(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Date(a), Value::Date(b)) => a == b,
            (Value::Time(a), Value::Time(b)) => a == b,
            (Value::Timestamp(a), Value::Timestamp(b)) => a == b,
            (Value::Uuid(a), Value::Uuid(b)) => a == b,
            (Value::Json(a), Value::Json(b)) => a == b,
            (Value::Placeholder(a), Value::Placeholder(b)) => a == b,
            _ => false, // Different types are not equal
        }
    }
}

impl Eq for Value {}

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash the discriminant first
        std::mem::discriminant(self).hash(state);

        // Hash the value based on type
        match self {
            Value::Null => {}
            Value::TinyInt(v) => v.hash(state),
            Value::SmallInt(v) => v.hash(state),
            Value::Integer(v) => v.hash(state),
            Value::BigInt(v) => v.hash(state),
            Value::Real(v) => v.to_bits().hash(state), // Use total ordering
            Value::Decimal(val, scale) => {
                val.hash(state);
                scale.hash(state);
            }
            Value::Text(v) => v.hash(state),
            Value::Bytes(v) => v.hash(state),
            Value::Boolean(v) => v.hash(state),
            Value::Date(v) => v.hash(state),
            Value::Time(v) => v.hash(state),
            Value::Timestamp(v) => v.hash(state),
            Value::Uuid(v) => v.hash(state),
            Value::Json(v) => v.to_string().hash(state), // Hash JSON string representation
            Value::Placeholder(v) => v.hash(state),
        }
    }
}

/// Parses a decimal string like "123.45" with a given scale.
fn parse_decimal_string(s: &str, scale: u8) -> Result<i128> {
    let parts: Vec<&str> = s.split('.').collect();
    match parts.as_slice() {
        [int_part] => {
            // No decimal point: "123" -> 12300 (with scale=2)
            let int_val: i128 = int_part.parse().map_err(|_| QueryError::TypeMismatch {
                expected: format!("decimal with scale {scale}"),
                actual: s.to_string(),
            })?;
            Ok(int_val * 10_i128.pow(u32::from(scale)))
        }
        [int_part, frac_part] => {
            // With decimal point: "123.45" -> 12345 (with scale=2)
            let int_val: i128 = int_part.parse().map_err(|_| QueryError::TypeMismatch {
                expected: format!("decimal with scale {scale}"),
                actual: s.to_string(),
            })?;

            // Pad or truncate fractional part to match scale
            let mut frac_str = (*frac_part).to_string();
            if frac_str.len() > scale as usize {
                frac_str.truncate(scale as usize);
            } else {
                frac_str.push_str(&"0".repeat(scale as usize - frac_str.len()));
            }

            let frac_val: i128 = frac_str.parse().map_err(|_| QueryError::TypeMismatch {
                expected: format!("decimal with scale {scale}"),
                actual: s.to_string(),
            })?;

            let multiplier = 10_i128.pow(u32::from(scale));
            // For negative decimals like "-1234.56", the fractional part must be
            // subtracted (not added) to extend the magnitude away from zero.
            // Use the original string's sign to handle "-0.xx" correctly.
            let is_negative = s.starts_with('-');
            if is_negative {
                Ok(int_val * multiplier - frac_val)
            } else {
                Ok(int_val * multiplier + frac_val)
            }
        }
        _ => Err(QueryError::TypeMismatch {
            expected: format!("decimal with scale {scale}"),
            actual: s.to_string(),
        }),
    }
}

/// Parses a UUID string (hyphenated or raw hex).
fn parse_uuid_string(s: &str) -> Result<[u8; 16]> {
    // Remove hyphens if present
    let hex_str = s.replace('-', "");

    if hex_str.len() != 32 {
        return Err(QueryError::TypeMismatch {
            expected: "UUID (32 hex digits)".to_string(),
            actual: s.to_string(),
        });
    }

    let mut bytes = [0u8; 16];
    for (i, chunk) in hex_str.as_bytes().chunks(2).enumerate() {
        let hex_byte = std::str::from_utf8(chunk).map_err(|_| QueryError::TypeMismatch {
            expected: "UUID (valid hex)".to_string(),
            actual: s.to_string(),
        })?;

        bytes[i] = u8::from_str_radix(hex_byte, 16).map_err(|_| QueryError::TypeMismatch {
            expected: "UUID (valid hex)".to_string(),
            actual: s.to_string(),
        })?;
    }

    Ok(bytes)
}

/// Total ordering for f64 values.
///
/// NaN < -Inf < negative values < -0.0 < +0.0 < positive values < +Inf
///
/// This enables f64 values to be used as B+tree keys.
fn total_cmp_f64(a: f64, b: f64) -> Ordering {
    // Use bit representation for total ordering
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();

    // Flip sign bit for negatives to get correct ordering
    let a_key = if a.is_sign_negative() {
        !a_bits
    } else {
        a_bits ^ (1u64 << 63)
    };

    let b_key = if b.is_sign_negative() {
        !b_bits
    } else {
        b_bits ^ (1u64 << 63)
    };

    a_key.cmp(&b_key)
}

impl Value {
    /// Returns the data type of this value.
    ///
    /// Returns `None` for `Null` and `Placeholder` since they have no concrete type.
    pub fn data_type(&self) -> Option<DataType> {
        match self {
            Value::Null | Value::Placeholder(_) => None,
            Value::TinyInt(_) => Some(DataType::TinyInt),
            Value::SmallInt(_) => Some(DataType::SmallInt),
            Value::Integer(_) => Some(DataType::Integer),
            Value::BigInt(_) => Some(DataType::BigInt),
            Value::Real(_) => Some(DataType::Real),
            Value::Decimal(_, scale) => Some(DataType::Decimal {
                precision: 38, // Max precision for i128
                scale: *scale,
            }),
            Value::Text(_) => Some(DataType::Text),
            Value::Bytes(_) => Some(DataType::Bytes),
            Value::Boolean(_) => Some(DataType::Boolean),
            Value::Date(_) => Some(DataType::Date),
            Value::Time(_) => Some(DataType::Time),
            Value::Timestamp(_) => Some(DataType::Timestamp),
            Value::Uuid(_) => Some(DataType::Uuid),
            Value::Json(_) => Some(DataType::Json),
        }
    }

    /// Returns true if this value is NULL.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Returns the value as an i64, if it is a `BigInt`.
    pub fn as_bigint(&self) -> Option<i64> {
        match self {
            Value::BigInt(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the value as a string slice, if it is Text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the value as a bool, if it is Boolean.
    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            Value::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns the value as a Timestamp, if it is Timestamp.
    pub fn as_timestamp(&self) -> Option<Timestamp> {
        match self {
            Value::Timestamp(ts) => Some(*ts),
            _ => None,
        }
    }

    /// Returns the value as bytes, if it is Bytes.
    pub fn as_bytes(&self) -> Option<&Bytes> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    /// Returns the value as an i8, if it is a `TinyInt`.
    pub fn as_tinyint(&self) -> Option<i8> {
        match self {
            Value::TinyInt(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the value as an i16, if it is a `SmallInt`.
    pub fn as_smallint(&self) -> Option<i16> {
        match self {
            Value::SmallInt(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the value as an i32, if it is an `Integer`.
    pub fn as_integer(&self) -> Option<i32> {
        match self {
            Value::Integer(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the value as an f64, if it is a `Real`.
    pub fn as_real(&self) -> Option<f64> {
        match self {
            Value::Real(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the value as a `Decimal` (value, scale), if it is a `Decimal`.
    pub fn as_decimal(&self) -> Option<(i128, u8)> {
        match self {
            Value::Decimal(v, s) => Some((*v, *s)),
            _ => None,
        }
    }

    /// Returns the value as a `Uuid`, if it is a `Uuid`.
    pub fn as_uuid(&self) -> Option<&[u8; 16]> {
        match self {
            Value::Uuid(u) => Some(u),
            _ => None,
        }
    }

    /// Returns the value as a `Json`, if it is a `Json`.
    pub fn as_json(&self) -> Option<&serde_json::Value> {
        match self {
            Value::Json(j) => Some(j),
            _ => None,
        }
    }

    /// Returns the value as a `Date`, if it is a `Date`.
    pub fn as_date(&self) -> Option<i32> {
        match self {
            Value::Date(d) => Some(*d),
            _ => None,
        }
    }

    /// Returns the value as a `Time`, if it is a `Time`.
    pub fn as_time(&self) -> Option<i64> {
        match self {
            Value::Time(t) => Some(*t),
            _ => None,
        }
    }

    /// Compares two values for ordering.
    ///
    /// NULL values are considered less than all non-NULL values.
    /// Values of different types return None (incomparable).
    ///
    /// For `Real` values, uses total ordering: NaN < -Inf < values < Inf.
    pub fn compare(&self, other: &Value) -> Option<Ordering> {
        match (self, other) {
            (Value::Null, Value::Null) => Some(Ordering::Equal),
            (Value::Null, _) => Some(Ordering::Less),
            (_, Value::Null) => Some(Ordering::Greater),
            (Value::TinyInt(a), Value::TinyInt(b)) => Some(a.cmp(b)),
            (Value::SmallInt(a), Value::SmallInt(b)) => Some(a.cmp(b)),
            (Value::Integer(a), Value::Integer(b)) => Some(a.cmp(b)),
            (Value::BigInt(a), Value::BigInt(b)) => Some(a.cmp(b)),
            (Value::Real(a), Value::Real(b)) => Some(total_cmp_f64(*a, *b)),
            (Value::Decimal(a_val, a_scale), Value::Decimal(b_val, b_scale)) => {
                // Only compare if same scale
                if a_scale == b_scale {
                    Some(a_val.cmp(b_val))
                } else {
                    None
                }
            }
            (Value::Text(a), Value::Text(b)) => Some(a.cmp(b)),
            (Value::Bytes(a), Value::Bytes(b)) => Some(a.as_ref().cmp(b.as_ref())),
            (Value::Boolean(a), Value::Boolean(b)) => Some(a.cmp(b)),
            (Value::Date(a), Value::Date(b)) => Some(a.cmp(b)),
            (Value::Time(a), Value::Time(b)) => Some(a.cmp(b)),
            (Value::Timestamp(a), Value::Timestamp(b)) => Some(a.cmp(b)),
            (Value::Uuid(a), Value::Uuid(b)) => Some(a.cmp(b)),
            (Value::Json(a), Value::Json(b)) => {
                // JSON comparison is complex, use string representation
                Some(a.to_string().cmp(&b.to_string()))
            }
            _ => None, // Different types are incomparable
        }
    }

    /// Checks if this value can be assigned to a column of the given type.
    pub fn is_compatible_with(&self, data_type: DataType) -> bool {
        match self {
            Value::Null => true,           // NULL is compatible with any type
            Value::Placeholder(_) => true, // Placeholder will be bound to actual value
            Value::TinyInt(_) => data_type == DataType::TinyInt,
            Value::SmallInt(_) => data_type == DataType::SmallInt,
            Value::Integer(_) => data_type == DataType::Integer,
            Value::BigInt(_) => data_type == DataType::BigInt,
            Value::Real(_) => data_type == DataType::Real,
            Value::Decimal(_, scale) => {
                matches!(data_type, DataType::Decimal { scale: s, .. } if s == *scale)
            }
            Value::Text(_) => data_type == DataType::Text,
            Value::Bytes(_) => data_type == DataType::Bytes,
            Value::Boolean(_) => data_type == DataType::Boolean,
            Value::Date(_) => data_type == DataType::Date,
            Value::Time(_) => data_type == DataType::Time,
            Value::Timestamp(_) => data_type == DataType::Timestamp,
            Value::Uuid(_) => data_type == DataType::Uuid,
            Value::Json(_) => data_type == DataType::Json,
        }
    }

    /// Converts this value to JSON.
    ///
    /// # Panics
    ///
    /// Panics if the value is a `Placeholder` (should be bound before conversion).
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::Null => serde_json::Value::Null,
            Value::TinyInt(v) => serde_json::Value::Number((*v).into()),
            Value::SmallInt(v) => serde_json::Value::Number((*v).into()),
            Value::Integer(v) => serde_json::Value::Number((*v).into()),
            Value::BigInt(v) => serde_json::Value::Number((*v).into()),
            Value::Real(v) => {
                serde_json::Number::from_f64(*v)
                    .map_or(serde_json::Value::Null, serde_json::Value::Number) // NaN/Inf become null
            }
            Value::Decimal(val, scale) => {
                // Convert to string representation
                let divisor = 10_i128.pow(u32::from(*scale));
                let int_part = val / divisor;
                let frac_part = (val % divisor).abs();
                let s = format!("{int_part}.{frac_part:0width$}", width = *scale as usize);
                serde_json::Value::String(s)
            }
            Value::Text(s) => serde_json::Value::String(s.clone()),
            Value::Bytes(b) => {
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD.encode(b);
                serde_json::Value::String(encoded)
            }
            Value::Boolean(b) => serde_json::Value::Bool(*b),
            Value::Date(d) => serde_json::Value::Number((*d).into()),
            Value::Time(t) => serde_json::Value::Number((*t).into()),
            Value::Timestamp(ts) => serde_json::Value::Number(ts.as_nanos().into()),
            Value::Uuid(u) => {
                // Format as RFC 4122 hyphenated string
                let s = format!(
                    "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                    u[0],
                    u[1],
                    u[2],
                    u[3],
                    u[4],
                    u[5],
                    u[6],
                    u[7],
                    u[8],
                    u[9],
                    u[10],
                    u[11],
                    u[12],
                    u[13],
                    u[14],
                    u[15]
                );
                serde_json::Value::String(s)
            }
            Value::Json(j) => j.clone(),
            Value::Placeholder(idx) => {
                panic!("Cannot convert unbound placeholder ${idx} to JSON - bind parameters first")
            }
        }
    }

    /// Parses a value from JSON with an expected data type.
    pub fn from_json(json: &serde_json::Value, data_type: DataType) -> Result<Self> {
        match (json, data_type) {
            (serde_json::Value::Null, _) => Ok(Value::Null),
            (serde_json::Value::Number(n), DataType::TinyInt) => n
                .as_i64()
                .and_then(|v| i8::try_from(v).ok())
                .map(Value::TinyInt)
                .ok_or_else(|| QueryError::TypeMismatch {
                    expected: "tinyint (-128 to 127)".to_string(),
                    actual: format!("number {n}"),
                }),
            (serde_json::Value::Number(n), DataType::SmallInt) => n
                .as_i64()
                .and_then(|v| i16::try_from(v).ok())
                .map(Value::SmallInt)
                .ok_or_else(|| QueryError::TypeMismatch {
                    expected: "smallint (-32768 to 32767)".to_string(),
                    actual: format!("number {n}"),
                }),
            (serde_json::Value::Number(n), DataType::Integer) => n
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .map(Value::Integer)
                .ok_or_else(|| QueryError::TypeMismatch {
                    expected: "integer (-2^31 to 2^31-1)".to_string(),
                    actual: format!("number {n}"),
                }),
            (serde_json::Value::Number(n), DataType::BigInt) => n
                .as_i64()
                .map(Value::BigInt)
                .ok_or_else(|| QueryError::TypeMismatch {
                    expected: "bigint".to_string(),
                    actual: format!("number {n}"),
                }),
            (serde_json::Value::Number(n), DataType::Real) => n
                .as_f64()
                .map(Value::Real)
                .ok_or_else(|| QueryError::TypeMismatch {
                    expected: "real (f64)".to_string(),
                    actual: format!("number {n}"),
                }),
            (
                serde_json::Value::String(s),
                DataType::Decimal {
                    precision: _,
                    scale,
                },
            ) => {
                // Parse decimal string like "123.45"
                parse_decimal_string(s, scale).map(|val| Value::Decimal(val, scale))
            }
            (serde_json::Value::String(s), DataType::Text) => Ok(Value::Text(s.clone())),
            (serde_json::Value::String(s), DataType::Bytes) => {
                use base64::Engine;
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(s)
                    .map_err(|e| QueryError::TypeMismatch {
                        expected: "base64 bytes".to_string(),
                        actual: e.to_string(),
                    })?;
                Ok(Value::Bytes(Bytes::from(decoded)))
            }
            (serde_json::Value::Bool(b), DataType::Boolean) => Ok(Value::Boolean(*b)),
            (serde_json::Value::Number(n), DataType::Date) => n
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .map(Value::Date)
                .ok_or_else(|| QueryError::TypeMismatch {
                    expected: "date (i32 days)".to_string(),
                    actual: format!("number {n}"),
                }),
            (serde_json::Value::Number(n), DataType::Time) => n
                .as_i64()
                .map(Value::Time)
                .ok_or_else(|| QueryError::TypeMismatch {
                    expected: "time (i64 nanos)".to_string(),
                    actual: format!("number {n}"),
                }),
            (serde_json::Value::Number(n), DataType::Timestamp) => n
                .as_u64()
                .map(|nanos| Value::Timestamp(Timestamp::from_nanos(nanos)))
                .ok_or_else(|| QueryError::TypeMismatch {
                    expected: "timestamp".to_string(),
                    actual: format!("number {n}"),
                }),
            (serde_json::Value::String(s), DataType::Uuid) => parse_uuid_string(s).map(Value::Uuid),
            (
                json @ (serde_json::Value::Object(_)
                | serde_json::Value::Array(_)
                | serde_json::Value::String(_)
                | serde_json::Value::Number(_)
                | serde_json::Value::Bool(_)),
                DataType::Json,
            ) => Ok(Value::Json(json.clone())),
            (json, dt) => Err(QueryError::TypeMismatch {
                expected: format!("{dt:?}"),
                actual: format!("{json:?}"),
            }),
        }
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::TinyInt(v) => write!(f, "{v}"),
            Value::SmallInt(v) => write!(f, "{v}"),
            Value::Integer(v) => write!(f, "{v}"),
            Value::BigInt(v) => write!(f, "{v}"),
            Value::Real(v) => write!(f, "{v}"),
            Value::Decimal(val, scale) => {
                let divisor = 10_i128.pow(u32::from(*scale));
                let int_part = val / divisor;
                let frac_part = (val % divisor).abs();
                write!(f, "{int_part}.{frac_part:0width$}", width = *scale as usize)
            }
            Value::Text(s) => write!(f, "'{s}'"),
            Value::Bytes(b) => write!(f, "<{} bytes>", b.len()),
            Value::Boolean(b) => write!(f, "{b}"),
            Value::Date(d) => write!(f, "DATE({d})"),
            Value::Time(t) => write!(f, "TIME({t})"),
            Value::Timestamp(ts) => write!(f, "{ts}"),
            Value::Uuid(u) => {
                write!(
                    f,
                    "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                    u[0],
                    u[1],
                    u[2],
                    u[3],
                    u[4],
                    u[5],
                    u[6],
                    u[7],
                    u[8],
                    u[9],
                    u[10],
                    u[11],
                    u[12],
                    u[13],
                    u[14],
                    u[15]
                )
            }
            Value::Json(j) => write!(f, "{j}"),
            Value::Placeholder(idx) => write!(f, "${idx}"),
        }
    }
}

impl From<i8> for Value {
    fn from(v: i8) -> Self {
        Value::TinyInt(v)
    }
}

impl From<i16> for Value {
    fn from(v: i16) -> Self {
        Value::SmallInt(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Integer(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::BigInt(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Real(v)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Text(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Text(s.to_string())
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Boolean(b)
    }
}

impl From<Timestamp> for Value {
    fn from(ts: Timestamp) -> Self {
        Value::Timestamp(ts)
    }
}

impl From<Bytes> for Value {
    fn from(b: Bytes) -> Self {
        Value::Bytes(b)
    }
}

impl From<[u8; 16]> for Value {
    fn from(u: [u8; 16]) -> Self {
        Value::Uuid(u)
    }
}

impl From<serde_json::Value> for Value {
    fn from(j: serde_json::Value) -> Self {
        Value::Json(j)
    }
}

/// Serde module for base64 encoding of bytes.
mod bytes_base64 {
    use base64::Engine;
    use bytes::Bytes;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Bytes, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Bytes, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&s)
            .map_err(serde::de::Error::custom)?;
        Ok(Bytes::from(decoded))
    }
}
