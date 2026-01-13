//! ChangeEvent - the core event type captured from SQLite mutations.
//!
//! These events form the write-ahead log for VerityDB projections. Every
//! INSERT, UPDATE, DELETE, and schema change is captured and persisted
//! before the SQLite write completes, enabling point-in-time recovery
//! and full audit trails.

use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{ProjectionError, SqlValue};

/// Prefixes that identify internal/system tables to be excluded from event capture.
///
/// These tables are either SQLite system tables or VerityDB metadata tables.
/// Changes to these tables are not captured in the event log since they are
/// infrastructure rather than user data.
///
/// # Example
///
/// ```
/// use vdb_projections::InternalTablePrefix;
///
/// // Check if a table name matches any internal prefix
/// assert!(InternalTablePrefix::Sqlite.matches("sqlite_master"));
/// assert!(InternalTablePrefix::Vdb.matches("_vdb_checkpoints"));
/// assert!(!InternalTablePrefix::Vdb.matches("users"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalTablePrefix {
    /// SQLite system tables (sqlite_master, sqlite_sequence, etc.)
    Sqlite,
    /// VerityDB metadata tables (_vdb_checkpoints, _vdb_schema, etc.)
    Vdb,
}

impl InternalTablePrefix {
    /// Returns the string prefix for this internal table type.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite_",
            Self::Vdb => "_vdb_",
        }
    }

    /// Returns all internal prefixes for iteration.
    pub const fn all() -> &'static [Self] {
        &[Self::Sqlite, Self::Vdb]
    }

    /// Checks if the given table name starts with this prefix.
    pub fn matches(&self, table_name: &str) -> bool {
        table_name.starts_with(self.as_str())
    }

    /// Checks if the given table name matches any internal prefix.
    pub fn is_internal(table_name: &str) -> bool {
        Self::all().iter().any(|prefix| prefix.matches(table_name))
    }
}

/// A database mutation event captured via SQLite's preupdate_hook.
///
/// Each variant contains all information needed to replay the mutation,
/// enabling recovery from the event log. Events are serialized and appended
/// to the durable log before the corresponding SQLite write completes.
///
/// # Variants
///
/// - [`Insert`](ChangeEvent::Insert) - New row added to a table
/// - [`Update`](ChangeEvent::Update) - Existing row modified (captures before/after)
/// - [`Delete`](ChangeEvent::Delete) - Row removed (captures deleted data)
/// - [`SchemaChange`](ChangeEvent::SchemaChange) - DDL statement (CREATE, ALTER, DROP)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChangeEvent {
    /// A new row was inserted into a table.
    Insert {
        table_name: TableName,
        row_id: RowId,
        /// Column names in insertion order.
        column_names: Vec<ColumnName>,
        /// Values in the same order as column_names.
        values: Vec<SqlValue>,
    },
    /// An existing row was updated.
    /// Captures both old and new values for audit compliance.
    Update {
        table_name: TableName,
        row_id: RowId,
        /// Column values before the update.
        old_values: Vec<(ColumnName, SqlValue)>,
        /// Column values after the update.
        new_values: Vec<(ColumnName, SqlValue)>,
    },
    /// A row was deleted from a table.
    /// Captures the deleted data for audit compliance.
    Delete {
        table_name: TableName,
        row_id: RowId,
        /// The values that were in the deleted row.
        deleted_values: Vec<SqlValue>,
    },
    /// A schema change (DDL) was executed.
    /// Captured during migrations to enable replay from scratch.
    SchemaChange { sql_statement: SqlStatement },
}

/// A SQLite table name.
///
/// Lightweight newtype for type safety. Values come from SQLite's preupdate_hook
/// and are trusted (SQLite has already validated them as real table names).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Eq, Hash)]
pub struct TableName(String);

impl TableName {
    /// Creates a TableName from a value provided by SQLite.
    /// Use this for values from preupdate_hook (trusted source).
    pub fn from_sqlite(name: &str) -> Self {
        Self(name.to_owned())
    }

    /// Returns true if this is an internal table that should be skipped.
    /// Internal tables include SQLite system tables (`sqlite_*`) and
    /// VerityDB metadata tables (`_vdb_*`).
    ///
    /// See [`InternalTablePrefix`] for the list of recognized prefixes.
    pub fn is_internal(&self) -> bool {
        InternalTablePrefix::is_internal(&self.0)
    }

    /// Returns the table name for use in SQL identifiers.
    pub fn as_identifier(&self) -> &str {
        &self.0
    }
}

impl Display for TableName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for TableName {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<TableName> for String {
    fn from(table_name: TableName) -> Self {
        table_name.0
    }
}

/// SQLite's internal row identifier.
///
/// Every SQLite table has a 64-bit signed integer rowid (unless it's a WITHOUT ROWID table).
/// This uniquely identifies a row within a table and is stable across the row's lifetime.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RowId(i64);

impl Display for RowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for RowId {
    fn from(value: i64) -> Self {
        debug_assert!(value >= 0, "RowId cannot be negative");
        Self(value)
    }
}

impl From<RowId> for i64 {
    fn from(row_id: RowId) -> Self {
        row_id.0
    }
}

/// A SQLite column name.
///
/// Lightweight newtype for type safety. Values come from SQLite's preupdate_hook
/// and are trusted (SQLite has already validated them as real column names).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnName(String);

impl Display for ColumnName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ColumnName {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<ColumnName> for String {
    fn from(column_name: ColumnName) -> Self {
        column_name.0
    }
}

/// A validated DDL (Data Definition Language) SQL statement.
///
/// Only schema-modifying statements (CREATE, ALTER, DROP) are allowed.
/// This ensures that [`ChangeEvent::SchemaChange`] only contains DDL,
/// while DML (INSERT, UPDATE, DELETE) is captured via the other variants.
///
/// # Example
///
/// ```
/// use vdb_projections::SqlStatement;
///
/// let stmt = SqlStatement::from_ddl("CREATE TABLE users (id INTEGER PRIMARY KEY)").unwrap();
/// assert_eq!(stmt.as_str(), "CREATE TABLE users (id INTEGER PRIMARY KEY)");
///
/// // DML is rejected
/// assert!(SqlStatement::from_ddl("INSERT INTO users VALUES (1)").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SqlStatement(String);

impl SqlStatement {
    /// Creates a SqlStatement from trusted migration code.
    ///
    /// Validates that the statement is DDL (CREATE, ALTER, DROP).
    /// Returns an error if the statement appears to be DML.
    pub fn from_ddl(sql: impl Into<String>) -> Result<Self, ProjectionError> {
        let sql = sql.into();
        let normalized = sql.trim().to_uppercase();

        // Only allow DDL statements
        const DDL_PREFIXES: &[&str] = &[
            "CREATE ",
            "ALTER ",
            "DROP ",
            "CREATE INDEX",
            "DROP INDEX",
            "CREATE TRIGGER",
            "DROP TRIGGER",
        ];

        if !DDL_PREFIXES.iter().any(|p| normalized.starts_with(p)) {
            return Err(ProjectionError::InvalidDdlStatement { statement: sql });
        }

        Ok(Self(sql))
    }

    /// Returns the SQL statement as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod internal_table_prefix {
        use super::*;

        #[test]
        fn sqlite_prefix_is_correct() {
            assert_eq!(InternalTablePrefix::Sqlite.as_str(), "sqlite_");
        }

        #[test]
        fn vdb_prefix_is_correct() {
            assert_eq!(InternalTablePrefix::Vdb.as_str(), "_vdb_");
        }

        #[test]
        fn all_returns_all_variants() {
            let all = InternalTablePrefix::all();
            assert_eq!(all.len(), 2);
            assert!(all.contains(&InternalTablePrefix::Sqlite));
            assert!(all.contains(&InternalTablePrefix::Vdb));
        }

        #[test]
        fn matches_sqlite_tables() {
            assert!(InternalTablePrefix::Sqlite.matches("sqlite_master"));
            assert!(InternalTablePrefix::Sqlite.matches("sqlite_sequence"));
            assert!(!InternalTablePrefix::Sqlite.matches("users"));
            assert!(!InternalTablePrefix::Sqlite.matches("_vdb_checkpoints"));
        }

        #[test]
        fn matches_vdb_tables() {
            assert!(InternalTablePrefix::Vdb.matches("_vdb_checkpoints"));
            assert!(InternalTablePrefix::Vdb.matches("_vdb_metadata"));
            assert!(!InternalTablePrefix::Vdb.matches("users"));
            assert!(!InternalTablePrefix::Vdb.matches("sqlite_master"));
        }

        #[test]
        fn is_internal_checks_all_prefixes() {
            // SQLite tables
            assert!(InternalTablePrefix::is_internal("sqlite_master"));
            assert!(InternalTablePrefix::is_internal("sqlite_sequence"));

            // VDB tables
            assert!(InternalTablePrefix::is_internal("_vdb_checkpoints"));
            assert!(InternalTablePrefix::is_internal("_vdb_schema"));

            // User tables
            assert!(!InternalTablePrefix::is_internal("users"));
            assert!(!InternalTablePrefix::is_internal("patients"));
            assert!(!InternalTablePrefix::is_internal("my_sqlite_backup")); // contains but doesn't start with
        }

        #[test]
        fn enum_is_copy() {
            let prefix = InternalTablePrefix::Sqlite;
            let copy = prefix; // This compiles because Copy is derived
            assert_eq!(prefix, copy);
        }
    }

    mod table_name {
        use super::*;

        #[test]
        fn from_sqlite_creates_table_name() {
            let name = TableName::from_sqlite("users");
            assert_eq!(name.as_identifier(), "users");
        }

        #[test]
        fn is_internal_detects_sqlite_tables() {
            assert!(TableName::from_sqlite("sqlite_master").is_internal());
            assert!(TableName::from_sqlite("sqlite_sequence").is_internal());
            assert!(TableName::from_sqlite("sqlite_stat1").is_internal());
        }

        #[test]
        fn is_internal_detects_vdb_tables() {
            assert!(TableName::from_sqlite("_vdb_checkpoints").is_internal());
            assert!(TableName::from_sqlite("_vdb_metadata").is_internal());
            assert!(TableName::from_sqlite("_vdb_schema").is_internal());
        }

        #[test]
        fn is_internal_allows_user_tables() {
            assert!(!TableName::from_sqlite("users").is_internal());
            assert!(!TableName::from_sqlite("patients").is_internal());
            assert!(!TableName::from_sqlite("appointments").is_internal());
            assert!(!TableName::from_sqlite("my_sqlite_backup").is_internal()); // contains but doesn't start with
        }

        #[test]
        fn display_shows_name() {
            let name = TableName::from_sqlite("users");
            assert_eq!(format!("{}", name), "users");
        }

        #[test]
        fn from_string_conversion() {
            let name: TableName = "patients".to_string().into();
            assert_eq!(name.as_identifier(), "patients");

            let back: String = name.into();
            assert_eq!(back, "patients");
        }

        #[test]
        fn serialization_roundtrip() {
            let name = TableName::from_sqlite("users");
            let json = serde_json::to_string(&name).unwrap();
            let restored: TableName = serde_json::from_str(&json).unwrap();
            assert_eq!(name, restored);
        }
    }

    mod row_id {
        use super::*;

        #[test]
        fn from_i64_positive() {
            let id = RowId::from(42i64);
            assert_eq!(i64::from(id), 42);
        }

        #[test]
        fn from_i64_zero() {
            let id = RowId::from(0i64);
            assert_eq!(i64::from(id), 0);
        }

        #[test]
        fn display_shows_id() {
            let id = RowId::from(123i64);
            assert_eq!(format!("{}", id), "123");
        }

        #[test]
        fn ordering_works() {
            let id1 = RowId::from(1i64);
            let id2 = RowId::from(2i64);
            let id3 = RowId::from(3i64);

            assert!(id1 < id2);
            assert!(id2 < id3);
            assert!(id1 < id3);
        }

        #[test]
        fn serialization_roundtrip() {
            let id = RowId::from(999i64);
            let json = serde_json::to_string(&id).unwrap();
            let restored: RowId = serde_json::from_str(&json).unwrap();
            assert_eq!(id, restored);
        }
    }

    mod column_name {
        use super::*;

        #[test]
        fn from_string_conversion() {
            let name: ColumnName = "email".to_string().into();
            let back: String = name.into();
            assert_eq!(back, "email");
        }

        #[test]
        fn display_shows_name() {
            let name: ColumnName = "created_at".to_string().into();
            assert_eq!(format!("{}", name), "created_at");
        }

        #[test]
        fn serialization_roundtrip() {
            let name: ColumnName = "patient_id".to_string().into();
            let json = serde_json::to_string(&name).unwrap();
            let restored: ColumnName = serde_json::from_str(&json).unwrap();
            assert_eq!(name, restored);
        }
    }

    mod sql_statement {
        use super::*;

        // Valid DDL statements
        #[test]
        fn accepts_create_table() {
            let stmt = SqlStatement::from_ddl("CREATE TABLE users (id INTEGER PRIMARY KEY)");
            assert!(stmt.is_ok());
            assert_eq!(
                stmt.unwrap().as_str(),
                "CREATE TABLE users (id INTEGER PRIMARY KEY)"
            );
        }

        #[test]
        fn accepts_create_table_if_not_exists() {
            let stmt =
                SqlStatement::from_ddl("CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY)");
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_alter_table() {
            let stmt = SqlStatement::from_ddl("ALTER TABLE users ADD COLUMN email TEXT");
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_drop_table() {
            let stmt = SqlStatement::from_ddl("DROP TABLE users");
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_drop_table_if_exists() {
            let stmt = SqlStatement::from_ddl("DROP TABLE IF EXISTS users");
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_create_index() {
            let stmt = SqlStatement::from_ddl("CREATE INDEX idx_users_email ON users(email)");
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_create_unique_index() {
            let stmt =
                SqlStatement::from_ddl("CREATE UNIQUE INDEX idx_users_email ON users(email)");
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_drop_index() {
            let stmt = SqlStatement::from_ddl("DROP INDEX idx_users_email");
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_create_trigger() {
            let stmt = SqlStatement::from_ddl(
                "CREATE TRIGGER update_timestamp AFTER UPDATE ON users BEGIN SELECT 1; END",
            );
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_drop_trigger() {
            let stmt = SqlStatement::from_ddl("DROP TRIGGER update_timestamp");
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_lowercase() {
            let stmt = SqlStatement::from_ddl("create table users (id integer primary key)");
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_mixed_case() {
            let stmt = SqlStatement::from_ddl("Create Table users (id Integer Primary Key)");
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_with_leading_whitespace() {
            let stmt = SqlStatement::from_ddl("  CREATE TABLE users (id INTEGER)");
            assert!(stmt.is_ok());
        }

        #[test]
        fn accepts_with_leading_newline() {
            let stmt = SqlStatement::from_ddl("\nCREATE TABLE users (id INTEGER)");
            assert!(stmt.is_ok());
        }

        // Invalid DML statements
        #[test]
        fn rejects_insert() {
            let stmt = SqlStatement::from_ddl("INSERT INTO users (name) VALUES ('test')");
            assert!(stmt.is_err());
            match stmt {
                Err(ProjectionError::InvalidDdlStatement { statement }) => {
                    assert!(statement.contains("INSERT"));
                }
                _ => panic!("Expected InvalidDdlStatement error"),
            }
        }

        #[test]
        fn rejects_update() {
            let stmt = SqlStatement::from_ddl("UPDATE users SET name = 'test' WHERE id = 1");
            assert!(stmt.is_err());
        }

        #[test]
        fn rejects_delete() {
            let stmt = SqlStatement::from_ddl("DELETE FROM users WHERE id = 1");
            assert!(stmt.is_err());
        }

        #[test]
        fn rejects_select() {
            let stmt = SqlStatement::from_ddl("SELECT * FROM users");
            assert!(stmt.is_err());
        }

        #[test]
        fn rejects_empty_string() {
            let stmt = SqlStatement::from_ddl("");
            assert!(stmt.is_err());
        }

        #[test]
        fn rejects_whitespace_only() {
            let stmt = SqlStatement::from_ddl("   ");
            assert!(stmt.is_err());
        }

        #[test]
        fn preserves_original_case() {
            let original = "CREATE TABLE Users (Id INTEGER PRIMARY KEY, Name TEXT)";
            let stmt = SqlStatement::from_ddl(original).unwrap();
            assert_eq!(stmt.as_str(), original);
        }

        #[test]
        fn serialization_roundtrip() {
            let stmt = SqlStatement::from_ddl("CREATE TABLE test (id INTEGER)").unwrap();
            let json = serde_json::to_string(&stmt).unwrap();
            let restored: SqlStatement = serde_json::from_str(&json).unwrap();
            assert_eq!(stmt, restored);
        }
    }

    mod change_event {
        use super::*;

        fn sample_insert() -> ChangeEvent {
            ChangeEvent::Insert {
                table_name: TableName::from_sqlite("users"),
                row_id: RowId::from(1i64),
                column_names: vec![
                    ColumnName::from("id".to_string()),
                    ColumnName::from("name".to_string()),
                ],
                values: vec![SqlValue::Integer(1), SqlValue::Text("Alice".to_string())],
            }
        }

        fn sample_update() -> ChangeEvent {
            ChangeEvent::Update {
                table_name: TableName::from_sqlite("users"),
                row_id: RowId::from(1i64),
                old_values: vec![(
                    ColumnName::from("name".to_string()),
                    SqlValue::Text("Alice".to_string()),
                )],
                new_values: vec![(
                    ColumnName::from("name".to_string()),
                    SqlValue::Text("Alicia".to_string()),
                )],
            }
        }

        fn sample_delete() -> ChangeEvent {
            ChangeEvent::Delete {
                table_name: TableName::from_sqlite("users"),
                row_id: RowId::from(1i64),
                deleted_values: vec![SqlValue::Integer(1), SqlValue::Text("Alice".to_string())],
            }
        }

        fn sample_schema_change() -> ChangeEvent {
            ChangeEvent::SchemaChange {
                sql_statement: SqlStatement::from_ddl("CREATE TABLE users (id INTEGER)").unwrap(),
            }
        }

        #[test]
        fn insert_serialization_roundtrip() {
            let event = sample_insert();
            let json = serde_json::to_string(&event).unwrap();
            let restored: ChangeEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event, restored);
        }

        #[test]
        fn update_serialization_roundtrip() {
            let event = sample_update();
            let json = serde_json::to_string(&event).unwrap();
            let restored: ChangeEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event, restored);
        }

        #[test]
        fn delete_serialization_roundtrip() {
            let event = sample_delete();
            let json = serde_json::to_string(&event).unwrap();
            let restored: ChangeEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event, restored);
        }

        #[test]
        fn schema_change_serialization_roundtrip() {
            let event = sample_schema_change();
            let json = serde_json::to_string(&event).unwrap();
            let restored: ChangeEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event, restored);
        }

        #[test]
        fn clone_works() {
            let event = sample_insert();
            let cloned = event.clone();
            assert_eq!(event, cloned);
        }

        #[test]
        fn debug_works() {
            let event = sample_insert();
            let debug = format!("{:?}", event);
            assert!(debug.contains("Insert"));
            assert!(debug.contains("users"));
        }
    }
}
