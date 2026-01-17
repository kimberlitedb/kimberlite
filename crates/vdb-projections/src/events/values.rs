//! SQLite value types for event serialization.
//!
//! This module provides owned representations of SQLite values that can be
//! serialized to the event log. Unlike sqlx's borrowed value types, these
//! can outlive the database connection and be sent across threads.

use serde::{Deserialize, Serialize};
use sqlx::{Decode, Sqlite, TypeInfo, ValueRef, sqlite::SqliteValueRef};

use crate::ProjectionError;

/// An owned SQLite value extracted from a preupdate_hook callback.
///
/// Maps directly to SQLite's five storage classes. Used in [`ChangeEvent`](crate::ChangeEvent)
/// to capture row data for the event log.
///
/// # Example
///
/// ```ignore
/// use vdb_projections::SqlValue;
///
/// let values = vec![
///     SqlValue::Integer(42),
///     SqlValue::Text("hello".to_string()),
///     SqlValue::Null,
/// ];
/// ```
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum SqlValue {
    /// SQL NULL value.
    Null,
    /// 64-bit signed integer (SQLite INTEGER).
    Integer(i64),
    /// 64-bit IEEE floating point (SQLite REAL).
    Real(f64),
    /// UTF-8 string (SQLite TEXT).
    Text(String),
    /// Raw bytes (SQLite BLOB).
    Blob(Vec<u8>),
}

/// Converts a borrowed SQLite value reference into an owned [`SqlValue`].
///
/// This conversion copies the underlying data, allowing the value to outlive
/// the database connection. Used internally by the preupdate_hook to capture
/// row data before it's modified.
impl<'r> TryFrom<SqliteValueRef<'r>> for SqlValue {
    type Error = ProjectionError;

    fn try_from(value: SqliteValueRef<'r>) -> Result<Self, Self::Error> {
        match value.type_info().name() {
            "NULL" => Ok(SqlValue::Null),
            "INTEGER" => Ok(SqlValue::Integer(Decode::<Sqlite>::decode(value).map_err(
                |e| ProjectionError::DecodeError {
                    type_name: "INTEGER",
                    source: e,
                },
            )?)),
            "REAL" => Ok(SqlValue::Real(Decode::<Sqlite>::decode(value).map_err(
                |e| ProjectionError::DecodeError {
                    type_name: "REAL",
                    source: e,
                },
            )?)),
            "TEXT" => Ok(SqlValue::Text(Decode::<Sqlite>::decode(value).map_err(
                |e| ProjectionError::DecodeError {
                    type_name: "TEXT",
                    source: e,
                },
            )?)),
            "BLOB" => Ok(SqlValue::Blob(Decode::<Sqlite>::decode(value).map_err(
                |e| ProjectionError::DecodeError {
                    type_name: "BLOB",
                    source: e,
                },
            )?)),
            other => Err(ProjectionError::UnsupportedSqliteType {
                type_name: other.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod sql_value {
        use super::*;

        #[test]
        fn null_serialization_roundtrip() {
            let value = SqlValue::Null;
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn integer_serialization_roundtrip() {
            let value = SqlValue::Integer(42);
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn integer_negative_serialization_roundtrip() {
            let value = SqlValue::Integer(-123);
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn integer_max_value() {
            let value = SqlValue::Integer(i64::MAX);
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn integer_min_value() {
            let value = SqlValue::Integer(i64::MIN);
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn real_serialization_roundtrip() {
            let value = SqlValue::Real(3.35488);
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn real_negative_serialization_roundtrip() {
            let value = SqlValue::Real(-273.15);
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn text_serialization_roundtrip() {
            let value = SqlValue::Text("Hello, World!".to_string());
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn text_empty_serialization_roundtrip() {
            let value = SqlValue::Text("".to_string());
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn text_unicode_serialization_roundtrip() {
            let value = SqlValue::Text("こんにちは 🎉".to_string());
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn text_with_special_chars() {
            let value = SqlValue::Text("Line1\nLine2\tTabbed\"Quoted\"".to_string());
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn blob_serialization_roundtrip() {
            let value = SqlValue::Blob(vec![0x00, 0x01, 0x02, 0xFF]);
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn blob_empty_serialization_roundtrip() {
            let value = SqlValue::Blob(vec![]);
            let json = serde_json::to_string(&value).unwrap();
            let restored: SqlValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, restored);
        }

        #[test]
        fn clone_works() {
            let value = SqlValue::Text("test".to_string());
            let cloned = value.clone();
            assert_eq!(value, cloned);
        }

        #[test]
        fn debug_works() {
            let value = SqlValue::Integer(42);
            let debug = format!("{:?}", value);
            assert!(debug.contains("Integer"));
            assert!(debug.contains("42"));
        }

        #[test]
        fn equality_same_variant() {
            assert_eq!(SqlValue::Null, SqlValue::Null);
            assert_eq!(SqlValue::Integer(1), SqlValue::Integer(1));
            assert_eq!(SqlValue::Real(1.0), SqlValue::Real(1.0));
            assert_eq!(
                SqlValue::Text("a".to_string()),
                SqlValue::Text("a".to_string())
            );
            assert_eq!(SqlValue::Blob(vec![1]), SqlValue::Blob(vec![1]));
        }

        #[test]
        fn inequality_different_values() {
            assert_ne!(SqlValue::Integer(1), SqlValue::Integer(2));
            assert_ne!(SqlValue::Real(1.0), SqlValue::Real(2.0));
            assert_ne!(
                SqlValue::Text("a".to_string()),
                SqlValue::Text("b".to_string())
            );
        }

        #[test]
        fn inequality_different_variants() {
            assert_ne!(SqlValue::Null, SqlValue::Integer(0));
            assert_ne!(SqlValue::Integer(1), SqlValue::Real(1.0));
            assert_ne!(SqlValue::Text("1".to_string()), SqlValue::Integer(1));
        }
    }
}
