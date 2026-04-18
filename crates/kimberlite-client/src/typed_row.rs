//! Typed row mapping for SQL query results.
//!
//! The default wire representation of a query result is a `Vec<Vec<QueryValue>>`
//! — convenient for dynamic access but awkward for typed code. This module
//! lets callers deserialize each row into any `T: serde::de::DeserializeOwned`
//! that matches the query's column layout.
//!
//! # Example
//!
//! ```ignore
//! use kimberlite_client::{Client, FromRow};
//! use serde::Deserialize;
//!
//! #[derive(Debug, Deserialize)]
//! struct User {
//!     id: i64,
//!     name: String,
//!     active: bool,
//! }
//!
//! let users: Vec<User> = client.query_typed::<User>(
//!     "SELECT id, name, active FROM users WHERE tenant = $1",
//!     &[QueryParam::BigInt(42)],
//! )?;
//! ```
//!
//! Rows are deserialized from a serde map of column-name → value, so `T` can
//! be any struct whose fields match the selected columns (order-independent).

use std::collections::BTreeMap;

use kimberlite_wire::{QueryParam, QueryResponse, QueryValue};
use serde::de::{self, DeserializeOwned, MapAccess, Visitor};
use serde::Deserializer;

use crate::client::Client;
use crate::error::{ClientError, ClientResult};

impl Client {
    /// Executes a SELECT query and deserialises every row into `T`.
    ///
    /// The query's columns are matched to `T`'s fields by name. Missing fields
    /// deserialise as `None`/default if the struct permits it, otherwise the
    /// call returns a deserialisation error.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Server`] on a server-side query error, and
    /// [`ClientError::Server`] with [`kimberlite_wire::ErrorCode::InternalError`]
    /// if any row fails to deserialise into `T`.
    pub fn query_typed<T>(&mut self, sql: &str, params: &[QueryParam]) -> ClientResult<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let response = self.query(sql, params)?;
        map_rows::<T>(&response)
    }

    /// Executes a time-travel SELECT query and deserialises every row into `T`.
    ///
    /// See [`Client::query_typed`] for the mapping rules.
    pub fn query_typed_at<T>(
        &mut self,
        sql: &str,
        params: &[QueryParam],
        position: kimberlite_types::Offset,
    ) -> ClientResult<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let response = self.query_at(sql, params, position)?;
        map_rows::<T>(&response)
    }
}

/// Deserialises every row of a `QueryResponse` into `T`.
///
/// Exposed for callers who already have a `QueryResponse` (e.g. cached or
/// obtained via `Client::query` for inspection).
pub fn map_rows<T>(response: &QueryResponse) -> ClientResult<Vec<T>>
where
    T: DeserializeOwned,
{
    let mut out = Vec::with_capacity(response.rows.len());
    for (idx, row) in response.rows.iter().enumerate() {
        if row.len() != response.columns.len() {
            return Err(ClientError::server(
                kimberlite_wire::ErrorCode::InternalError,
                format!(
                    "row {idx} has {actual} values but response has {expected} columns",
                    actual = row.len(),
                    expected = response.columns.len(),
                ),
            ));
        }
        let deserializer = RowDeserializer::new(&response.columns, row);
        let value = T::deserialize(deserializer).map_err(|e| {
            ClientError::server(
                kimberlite_wire::ErrorCode::InternalError,
                format!("row {idx}: {e}"),
            )
        })?;
        out.push(value);
    }
    Ok(out)
}

/// Error produced while deserialising a row.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct RowDeserializeError(String);

impl de::Error for RowDeserializeError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Self(msg.to_string())
    }
}

struct RowDeserializer<'a> {
    columns: &'a [String],
    values: &'a [QueryValue],
}

impl<'a> RowDeserializer<'a> {
    fn new(columns: &'a [String], values: &'a [QueryValue]) -> Self {
        Self { columns, values }
    }
}

impl<'de> Deserializer<'de> for RowDeserializer<'_> {
    type Error = RowDeserializeError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(RowMapAccess {
            columns: self.columns,
            values: self.values,
            idx: 0,
        })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct RowMapAccess<'a> {
    columns: &'a [String],
    values: &'a [QueryValue],
    idx: usize,
}

impl<'de> MapAccess<'de> for RowMapAccess<'_> {
    type Error = RowDeserializeError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        if self.idx >= self.columns.len() {
            return Ok(None);
        }
        let key = seed.deserialize(KeyDeserializer(self.columns[self.idx].as_str()))?;
        Ok(Some(key))
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        let value = &self.values[self.idx];
        self.idx += 1;
        seed.deserialize(ValueDeserializer(value))
    }
}

struct KeyDeserializer<'a>(&'a str);

impl<'de> Deserializer<'de> for KeyDeserializer<'_> {
    type Error = RowDeserializeError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_str(self.0)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct ValueDeserializer<'a>(&'a QueryValue);

impl<'de> Deserializer<'de> for ValueDeserializer<'_> {
    type Error = RowDeserializeError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.0 {
            QueryValue::Null => visitor.visit_unit(),
            QueryValue::BigInt(i) => visitor.visit_i64(*i),
            QueryValue::Text(s) => visitor.visit_str(s),
            QueryValue::Boolean(b) => visitor.visit_bool(*b),
            QueryValue::Timestamp(t) => visitor.visit_i64(*t),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.0 {
            QueryValue::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.0 {
            QueryValue::Null => visitor.visit_unit(),
            _ => Err(de::Error::custom("expected null")),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

/// Convenience trait for callers that prefer an explicit `FromRow` style.
///
/// The blanket impl provides a concise way to convert a `(columns, values)`
/// pair into any `T: DeserializeOwned`. Most users should prefer
/// [`Client::query_typed`] directly.
pub trait FromRow: Sized {
    fn from_row(columns: &[String], values: &[QueryValue]) -> ClientResult<Self>;
}

impl<T> FromRow for T
where
    T: DeserializeOwned,
{
    fn from_row(columns: &[String], values: &[QueryValue]) -> ClientResult<Self> {
        if columns.len() != values.len() {
            return Err(ClientError::server(
                kimberlite_wire::ErrorCode::InternalError,
                format!(
                    "column/value length mismatch: {} columns vs {} values",
                    columns.len(),
                    values.len()
                ),
            ));
        }
        let deserializer = RowDeserializer::new(columns, values);
        T::deserialize(deserializer).map_err(|e| {
            ClientError::server(kimberlite_wire::ErrorCode::InternalError, e.to_string())
        })
    }
}

/// Collects a `QueryResponse` into a `Vec<BTreeMap<String, QueryValue>>`.
///
/// Useful for dynamic callers that want name-keyed access without a typed
/// struct.
pub fn rows_as_maps(response: &QueryResponse) -> Vec<BTreeMap<String, QueryValue>> {
    response
        .rows
        .iter()
        .map(|row| {
            response
                .columns
                .iter()
                .zip(row.iter())
                .map(|(col, val)| (col.clone(), val.clone()))
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct User {
        id: i64,
        name: String,
        active: bool,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct UserWithOptional {
        id: i64,
        email: Option<String>,
    }

    fn make_response(columns: Vec<&str>, rows: Vec<Vec<QueryValue>>) -> QueryResponse {
        QueryResponse {
            columns: columns.into_iter().map(String::from).collect(),
            rows,
        }
    }

    #[test]
    fn deserialises_simple_struct() {
        let response = make_response(
            vec!["id", "name", "active"],
            vec![
                vec![
                    QueryValue::BigInt(1),
                    QueryValue::Text("alice".into()),
                    QueryValue::Boolean(true),
                ],
                vec![
                    QueryValue::BigInt(2),
                    QueryValue::Text("bob".into()),
                    QueryValue::Boolean(false),
                ],
            ],
        );

        let users: Vec<User> = map_rows(&response).unwrap();
        assert_eq!(
            users,
            vec![
                User {
                    id: 1,
                    name: "alice".into(),
                    active: true
                },
                User {
                    id: 2,
                    name: "bob".into(),
                    active: false
                },
            ]
        );
    }

    #[test]
    fn null_becomes_none_for_optional_fields() {
        let response = make_response(
            vec!["id", "email"],
            vec![
                vec![QueryValue::BigInt(1), QueryValue::Null],
                vec![
                    QueryValue::BigInt(2),
                    QueryValue::Text("bob@example.com".into()),
                ],
            ],
        );

        let users: Vec<UserWithOptional> = map_rows(&response).unwrap();
        assert_eq!(
            users,
            vec![
                UserWithOptional {
                    id: 1,
                    email: None
                },
                UserWithOptional {
                    id: 2,
                    email: Some("bob@example.com".into())
                },
            ]
        );
    }

    #[test]
    fn row_length_mismatch_errors() {
        let response = make_response(
            vec!["id", "name"],
            vec![vec![QueryValue::BigInt(1)]], // Only one value for two columns.
        );

        let result: ClientResult<Vec<User>> = map_rows(&response);
        assert!(result.is_err());
    }

    #[test]
    fn empty_rows_returns_empty_vec() {
        let response = make_response(vec!["id", "name", "active"], vec![]);
        let users: Vec<User> = map_rows(&response).unwrap();
        assert!(users.is_empty());
    }

    #[test]
    fn from_row_trait_matches_map_rows() {
        let columns: Vec<String> = vec!["id".into(), "name".into(), "active".into()];
        let values = vec![
            QueryValue::BigInt(42),
            QueryValue::Text("carol".into()),
            QueryValue::Boolean(true),
        ];

        let user = User::from_row(&columns, &values).unwrap();
        assert_eq!(
            user,
            User {
                id: 42,
                name: "carol".into(),
                active: true
            }
        );
    }

    #[test]
    fn rows_as_maps_preserves_column_names() {
        let response = make_response(
            vec!["id", "name"],
            vec![vec![QueryValue::BigInt(1), QueryValue::Text("alice".into())]],
        );
        let maps = rows_as_maps(&response);
        assert_eq!(maps.len(), 1);
        assert!(matches!(maps[0].get("id"), Some(QueryValue::BigInt(1))));
        assert!(
            matches!(maps[0].get("name"), Some(QueryValue::Text(s)) if s == "alice"),
        );
    }
}
