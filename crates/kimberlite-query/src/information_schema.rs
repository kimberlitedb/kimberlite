//! `information_schema` virtual tables.
//!
//! AUDIT-2026-04 S3.4 — SQL-level schema introspection. Compliance
//! developers writing audit reports need to enumerate tables and
//! columns without calling the admin API separately. This module
//! synthesises results for a handful of `information_schema.*`
//! queries by reading the live [`crate::schema::Schema`].
//!
//! # Supported queries
//!
//! - `SELECT * FROM information_schema.tables` — one row per
//!   registered table, columns `(table_name, column_count,
//!   primary_key_count)`.
//! - `SELECT * FROM information_schema.columns` — one row per
//!   `(table, column)` pair with columns
//!   `(table_name, column_name, data_type, ordinal_position)`.
//!
//! # What's NOT supported
//!
//! WHERE / ORDER BY / LIMIT on info_schema queries are ignored
//! — the full row set is returned. Filtering happens client-side
//! if the caller needs it. Future enhancement would push
//! predicate-aware evaluation through the planner.

use crate::executor::QueryResult;
use crate::schema::Schema;
use crate::value::Value;

const INFO_SCHEMA_PREFIX: &str = "information_schema.";

/// If `sql` targets a supported `information_schema.*` table,
/// synthesise and return the result. Otherwise return `None` so
/// the caller falls back to the normal planner/executor path.
///
/// Matches only on the FROM-clause table name; caveats:
/// - Case-insensitive on the schema prefix.
/// - Only exact matches for `tables` and `columns`.
/// - Ignores WHERE / ORDER BY / LIMIT for now.
pub fn maybe_answer(sql: &str, schema: &Schema) -> Option<QueryResult> {
    let target = detect_info_schema_target(sql)?;
    match target {
        InfoSchemaTarget::Tables => Some(render_tables(schema)),
        InfoSchemaTarget::Columns => Some(render_columns(schema)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfoSchemaTarget {
    Tables,
    Columns,
}

/// Parse the SQL just enough to identify an `information_schema.*`
/// FROM target. Case-insensitive.
fn detect_info_schema_target(sql: &str) -> Option<InfoSchemaTarget> {
    // Normalise whitespace to single spaces and uppercase so we
    // can look for `FROM INFORMATION_SCHEMA.TABLES` without
    // reinventing SQL tokenisation.
    let upper: String = sql
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    let prefix = format!("FROM {}", INFO_SCHEMA_PREFIX.to_ascii_uppercase());
    let idx = upper.find(&prefix)?;
    let after = &upper[idx + prefix.len()..];
    // Take the first token after the prefix (table name).
    let name_end = after
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(after.len());
    let name = &after[..name_end];
    match name {
        "TABLES" => Some(InfoSchemaTarget::Tables),
        "COLUMNS" => Some(InfoSchemaTarget::Columns),
        _ => None,
    }
}

fn render_tables(schema: &Schema) -> QueryResult {
    let columns: Vec<crate::schema::ColumnName> = vec![
        "table_name".into(),
        "column_count".into(),
        "primary_key_count".into(),
    ];
    let rows: Vec<Vec<Value>> = schema
        .all_tables()
        .map(|(name, def)| {
            vec![
                Value::Text(name.as_str().to_string()),
                Value::BigInt(def.columns.len() as i64),
                Value::BigInt(def.primary_key.len() as i64),
            ]
        })
        .collect();
    QueryResult { columns, rows }
}

fn render_columns(schema: &Schema) -> QueryResult {
    let columns: Vec<crate::schema::ColumnName> = vec![
        "table_name".into(),
        "column_name".into(),
        "data_type".into(),
        "ordinal_position".into(),
    ];
    let mut rows: Vec<Vec<Value>> = Vec::new();
    for (table_name, def) in schema.all_tables() {
        for (ordinal, col) in def.columns.iter().enumerate() {
            rows.push(vec![
                Value::Text(table_name.as_str().to_string()),
                Value::Text(col.name.as_str().to_string()),
                Value::Text(format!("{:?}", col.data_type)),
                Value::BigInt((ordinal + 1) as i64),
            ]);
        }
    }
    QueryResult { columns, rows }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_tables_target() {
        assert_eq!(
            detect_info_schema_target("SELECT * FROM information_schema.tables"),
            Some(InfoSchemaTarget::Tables),
        );
    }

    #[test]
    fn detect_columns_target() {
        assert_eq!(
            detect_info_schema_target(
                "SELECT table_name, column_name FROM information_schema.columns"
            ),
            Some(InfoSchemaTarget::Columns),
        );
    }

    #[test]
    fn detect_is_case_insensitive() {
        assert_eq!(
            detect_info_schema_target("select * from INFORMATION_SCHEMA.TABLES"),
            Some(InfoSchemaTarget::Tables),
        );
        assert_eq!(
            detect_info_schema_target("SELECT * FROM information_schema.TaBlEs"),
            Some(InfoSchemaTarget::Tables),
        );
    }

    #[test]
    fn detect_handles_extra_whitespace() {
        assert_eq!(
            detect_info_schema_target(
                "SELECT   *\n FROM   information_schema.tables   WHERE x = 1"
            ),
            Some(InfoSchemaTarget::Tables),
        );
    }

    #[test]
    fn detect_unknown_info_table_returns_none() {
        assert!(detect_info_schema_target("SELECT * FROM information_schema.foobar").is_none());
    }

    #[test]
    fn detect_user_table_returns_none() {
        assert!(detect_info_schema_target("SELECT * FROM users").is_none());
    }

    #[test]
    fn detect_similarly_named_prefix_does_not_match() {
        // `not_information_schema.tables` shouldn't trigger.
        assert!(
            detect_info_schema_target("SELECT * FROM not_information_schema.tables").is_none(),
        );
    }
}
