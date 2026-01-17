//! SQL parsing utilities for DDL extraction.
//!
//! Uses sqlparser to robustly extract table names from DDL statements,
//! handling edge cases like quoted identifiers and IF NOT EXISTS clauses.

use sqlparser::ast::{
    AlterTable, AlterTableOperation, CreateIndex, CreateTable, ObjectType, Statement,
};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;

use crate::{ProjectionError, TableName};

/// Extracts the table name from a DDL statement.
///
/// Supports:
/// - `CREATE TABLE [IF NOT EXISTS] table_name (...)`
/// - `ALTER TABLE table_name ...`
/// - `DROP TABLE [IF EXISTS] table_name`
/// - `CREATE INDEX ... ON table_name (...)`
/// - `DROP INDEX ...` (returns None - no table name)
///
/// # Errors
///
/// Returns an error if:
/// - SQL cannot be parsed
/// - Statement is empty
/// - Statement type is not supported DDL
/// - Table name cannot be extracted from the AST
///
/// # Example
///
/// ```
/// use vdb_projections::parse::extract_table_name;
///
/// let table = extract_table_name("CREATE TABLE patients (id INTEGER PRIMARY KEY)").unwrap();
/// assert_eq!(table.unwrap().as_identifier(), "patients");
///
/// let table = extract_table_name("CREATE TABLE IF NOT EXISTS users (id INTEGER)").unwrap();
/// assert_eq!(table.unwrap().as_identifier(), "users");
/// ```
pub fn extract_table_name(sql: &str) -> Result<Option<TableName>, ProjectionError> {
    let dialect = SQLiteDialect {};
    let statements =
        Parser::parse_sql(&dialect, sql).map_err(|e| ProjectionError::SqlParseError {
            message: e.to_string(),
        })?;

    let statement = statements
        .into_iter()
        .next()
        .ok_or(ProjectionError::EmptyStatement)?;

    extract_table_name_from_statement(&statement)
}

/// Extracts the table name from a parsed statement.
fn extract_table_name_from_statement(
    statement: &Statement,
) -> Result<Option<TableName>, ProjectionError> {
    match statement {
        // CREATE TABLE [IF NOT EXISTS] table_name (...)
        Statement::CreateTable(CreateTable { name, .. }) => {
            let table_name = extract_from_object_name(name)?;
            Ok(Some(table_name))
        }

        // ALTER TABLE table_name ...
        Statement::AlterTable(AlterTable {
            name, operations, ..
        }) => {
            // For RENAME TABLE, we return the original name
            // The caller should handle tracking the new name if needed
            if let Some(AlterTableOperation::RenameTable { .. }) = operations.first() {
                let table_name = extract_from_object_name(name)?;
                return Ok(Some(table_name));
            }
            let table_name = extract_from_object_name(name)?;
            Ok(Some(table_name))
        }

        // DROP TABLE [IF EXISTS] table_name [, ...]
        Statement::Drop {
            object_type: ObjectType::Table,
            names,
            ..
        } => {
            // DROP can have multiple tables, we return the first one
            // For schema cache purposes, caller should handle multiple drops
            let first_name = names
                .first()
                .ok_or(ProjectionError::TableNameExtractionFailed)?;
            let table_name = extract_from_object_name(first_name)?;
            Ok(Some(table_name))
        }

        // CREATE [UNIQUE] INDEX ... ON table_name (...)
        Statement::CreateIndex(CreateIndex { table_name, .. }) => {
            let table_name = extract_from_object_name(table_name)?;
            Ok(Some(table_name))
        }

        // DROP INDEX - no table name to extract
        Statement::Drop {
            object_type: ObjectType::Index,
            ..
        } => Ok(None),

        // CREATE TRIGGER - could add if needed
        // DROP TRIGGER - could add if needed
        _ => Err(ProjectionError::UnsupportedDdlStatement {
            statement_type: format!("{:?}", std::mem::discriminant(statement)),
        }),
    }
}

/// Extracts TableName from an ObjectName (handles qualified names like schema.table).
fn extract_from_object_name(
    name: &sqlparser::ast::ObjectName,
) -> Result<TableName, ProjectionError> {
    // Table name is always the last part of a qualified name
    // e.g., "main.users" -> ["main", "users"] -> "users"
    let last_part = name
        .0
        .last()
        .ok_or(ProjectionError::TableNameExtractionFailed)?;

    let ident = last_part
        .as_ident()
        .ok_or(ProjectionError::TableNameExtractionFailed)?;

    Ok(TableName::from_sqlite(&ident.value))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod create_table {
        use super::*;

        #[test]
        fn simple_create_table() {
            let result = extract_table_name("CREATE TABLE patients (id INTEGER PRIMARY KEY)");
            assert_eq!(result.unwrap().unwrap().as_identifier(), "patients");
        }

        #[test]
        fn create_table_if_not_exists() {
            let result =
                extract_table_name("CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY)");
            assert_eq!(result.unwrap().unwrap().as_identifier(), "users");
        }

        #[test]
        fn create_table_with_schema() {
            let result = extract_table_name("CREATE TABLE main.appointments (id INTEGER)");
            assert_eq!(result.unwrap().unwrap().as_identifier(), "appointments");
        }

        #[test]
        fn create_table_quoted_identifier() {
            let result = extract_table_name("CREATE TABLE \"user-data\" (id INTEGER)");
            assert_eq!(result.unwrap().unwrap().as_identifier(), "user-data");
        }

        #[test]
        fn create_table_with_columns() {
            let sql = r#"
                CREATE TABLE patients (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    dob TEXT,
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let result = extract_table_name(sql);
            assert_eq!(result.unwrap().unwrap().as_identifier(), "patients");
        }
    }

    mod alter_table {
        use super::*;

        #[test]
        fn alter_table_add_column() {
            let result = extract_table_name("ALTER TABLE users ADD COLUMN email TEXT");
            assert_eq!(result.unwrap().unwrap().as_identifier(), "users");
        }

        #[test]
        fn alter_table_rename() {
            let result = extract_table_name("ALTER TABLE old_name RENAME TO new_name");
            // Returns the original name
            assert_eq!(result.unwrap().unwrap().as_identifier(), "old_name");
        }
    }

    mod drop_table {
        use super::*;

        #[test]
        fn drop_table() {
            let result = extract_table_name("DROP TABLE users");
            assert_eq!(result.unwrap().unwrap().as_identifier(), "users");
        }

        #[test]
        fn drop_table_if_exists() {
            let result = extract_table_name("DROP TABLE IF EXISTS old_data");
            assert_eq!(result.unwrap().unwrap().as_identifier(), "old_data");
        }
    }

    mod create_index {
        use super::*;

        #[test]
        fn create_index() {
            let result = extract_table_name("CREATE INDEX idx_users_email ON users(email)");
            assert_eq!(result.unwrap().unwrap().as_identifier(), "users");
        }

        #[test]
        fn create_unique_index() {
            let result = extract_table_name("CREATE UNIQUE INDEX idx_users_email ON users(email)");
            assert_eq!(result.unwrap().unwrap().as_identifier(), "users");
        }
    }

    mod drop_index {
        use super::*;

        #[test]
        fn drop_index_returns_none() {
            let result = extract_table_name("DROP INDEX idx_users_email");
            assert!(result.unwrap().is_none());
        }
    }

    mod errors {
        use super::*;

        #[test]
        fn invalid_sql_returns_parse_error() {
            let result = extract_table_name("NOT VALID SQL AT ALL");
            assert!(matches!(result, Err(ProjectionError::SqlParseError { .. })));
        }

        #[test]
        fn empty_string_returns_error() {
            let result = extract_table_name("");
            assert!(matches!(result, Err(ProjectionError::SqlParseError { .. })));
        }

        #[test]
        fn select_statement_returns_unsupported() {
            let result = extract_table_name("SELECT * FROM users");
            assert!(matches!(
                result,
                Err(ProjectionError::UnsupportedDdlStatement { .. })
            ));
        }

        #[test]
        fn insert_statement_returns_unsupported() {
            let result = extract_table_name("INSERT INTO users (name) VALUES ('test')");
            assert!(matches!(
                result,
                Err(ProjectionError::UnsupportedDdlStatement { .. })
            ));
        }
    }
}
