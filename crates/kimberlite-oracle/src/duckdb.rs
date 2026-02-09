//! DuckDB oracle implementation for differential testing.
//!
//! This module provides a wrapper around DuckDB for use as a ground truth
//! oracle in differential testing.

use duckdb::Connection;
use kimberlite_query::{ColumnName, QueryResult, Value};

use crate::{OracleError, OracleRunner};

/// DuckDB oracle for differential testing.
///
/// Uses an in-memory DuckDB instance as the reference implementation.
pub struct DuckDbOracle {
    conn: Connection,
}

impl DuckDbOracle {
    /// Creates a new DuckDB oracle with an in-memory database.
    pub fn new() -> Result<Self, OracleError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| OracleError::Internal(format!("Failed to open DuckDB: {e}")))?;

        Ok(Self { conn })
    }

    /// Converts a DuckDB value to a Kimberlite Value.
    fn convert_value(value: duckdb::types::ValueRef) -> Value {
        use bytes::Bytes;
        use duckdb::types::ValueRef;

        match value {
            ValueRef::Null => Value::Null,
            ValueRef::Boolean(b) => Value::Boolean(b),
            ValueRef::TinyInt(i) => Value::TinyInt(i),
            ValueRef::SmallInt(i) => Value::SmallInt(i),
            ValueRef::Int(i) => Value::Integer(i),
            ValueRef::BigInt(i) => Value::BigInt(i),
            ValueRef::HugeInt(i) => Value::BigInt(i as i64),
            ValueRef::UTinyInt(i) => Value::TinyInt(i as i8),
            ValueRef::USmallInt(i) => Value::SmallInt(i as i16),
            ValueRef::UInt(i) => Value::Integer(i as i32),
            ValueRef::UBigInt(i) => Value::BigInt(i as i64),
            ValueRef::Float(f) => Value::Real(f64::from(f)),
            ValueRef::Double(f) => Value::Real(f),
            ValueRef::Decimal(d) => {
                // Convert decimal to text representation
                Value::Text(format!("{}", d))
            }
            ValueRef::Timestamp(_, _) => {
                // Simplified: convert timestamp to BigInt microseconds
                Value::BigInt(0) // TODO: proper timestamp conversion
            }
            ValueRef::Text(s) => {
                // Convert &[u8] to String (DuckDB stores text as UTF-8 bytes)
                Value::Text(String::from_utf8_lossy(s).to_string())
            }
            ValueRef::Blob(b) => {
                // Convert blob to Bytes
                Value::Bytes(Bytes::from(b.to_vec()))
            }
            ValueRef::Date32(d) => {
                // Convert date to Date type (days since epoch)
                Value::Date(d)
            }
            ValueRef::Time64(_, t) => {
                // Convert time to Time type (nanoseconds within day)
                Value::Time(t)
            }
            _ => Value::Null, // Unsupported types become NULL
        }
    }
}

impl OracleRunner for DuckDbOracle {
    fn execute(&mut self, sql: &str) -> Result<QueryResult, OracleError> {
        // Check if this is a DDL/DML statement (CREATE, INSERT, UPDATE, DELETE, DROP)
        let sql_upper = sql.trim().to_uppercase();
        let is_ddl_dml = sql_upper.starts_with("CREATE")
            || sql_upper.starts_with("INSERT")
            || sql_upper.starts_with("UPDATE")
            || sql_upper.starts_with("DELETE")
            || sql_upper.starts_with("DROP")
            || sql_upper.starts_with("ALTER");

        if is_ddl_dml {
            // Execute DDL/DML statement (no result set)
            self.conn
                .execute(sql, [])
                .map_err(|e| OracleError::RuntimeError(format!("DuckDB error: {e}")))?;

            // Return empty result
            return Ok(QueryResult {
                columns: vec![],
                rows: vec![],
            });
        }

        // For SELECT queries, execute via the connection's query method
        // This is simpler than prepare/query because DuckDB's API requires
        // executing the statement before accessing metadata
        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| OracleError::SyntaxError(format!("DuckDB syntax error: {e}")))?;

        let mut rows_result = stmt
            .query([])
            .map_err(|e| OracleError::RuntimeError(format!("DuckDB runtime error: {e}")))?;

        // Fetch first row to get column metadata
        let first_row = rows_result
            .next()
            .map_err(|e| OracleError::RuntimeError(format!("Failed to fetch first row: {e}")))?;

        let mut columns = Vec::new();
        let mut rows = Vec::new();

        if let Some(row) = first_row {
            // Get column count and names from the first row
            let column_count = row.as_ref().column_count();
            for i in 0..column_count {
                let name = row
                    .as_ref()
                    .column_name(i)
                    .map_err(|e| OracleError::Internal(format!("Failed to get column name: {e}")))?
                    .to_string();
                columns.push(ColumnName::from(name));
            }

            // Process first row
            let mut row_values = Vec::with_capacity(column_count);
            for i in 0..column_count {
                let value_ref = row.get_ref(i).map_err(|e| {
                    OracleError::RuntimeError(format!("Failed to get column {i}: {e}"))
                })?;
                row_values.push(Self::convert_value(value_ref));
            }
            rows.push(row_values);

            // Fetch remaining rows
            while let Some(row) = rows_result
                .next()
                .map_err(|e| OracleError::RuntimeError(format!("Failed to fetch row: {e}")))?
            {
                let mut row_values = Vec::with_capacity(column_count);
                for i in 0..column_count {
                    let value_ref = row.get_ref(i).map_err(|e| {
                        OracleError::RuntimeError(format!("Failed to get column {i}: {e}"))
                    })?;
                    row_values.push(Self::convert_value(value_ref));
                }
                rows.push(row_values);
            }
        }

        Ok(QueryResult { columns, rows })
    }

    fn reset(&mut self) -> Result<(), OracleError> {
        // Get list of all tables
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .map_err(|e| OracleError::Internal(format!("Failed to list tables: {e}")))?;

        let mut rows = stmt
            .query([])
            .map_err(|e| OracleError::Internal(format!("Failed to query tables: {e}")))?;

        let mut table_names = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|e| OracleError::Internal(format!("Failed to fetch table name: {e}")))?
        {
            let name: String = row
                .get(0)
                .map_err(|e| OracleError::Internal(format!("Failed to get table name: {e}")))?;
            table_names.push(name);
        }

        // Drop all tables
        for table_name in table_names {
            let drop_sql = format!("DROP TABLE IF EXISTS {table_name}");
            self.conn.execute(&drop_sql, []).map_err(|e| {
                OracleError::Internal(format!("Failed to drop table {table_name}: {e}"))
            })?;
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "DuckDB"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duckdb_oracle_basic() {
        let mut oracle = DuckDbOracle::new().expect("Failed to create DuckDB oracle");

        // Create a test table
        oracle
            .execute("CREATE TABLE users (id INTEGER, name TEXT)")
            .expect("Failed to create table");

        // Insert test data
        oracle
            .execute("INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob')")
            .expect("Failed to insert data");

        // Query the data
        let result = oracle
            .execute("SELECT * FROM users ORDER BY id")
            .expect("Failed to query data");

        assert_eq!(result.columns.len(), 2);
        assert_eq!(result.columns[0].as_str(), "id");
        assert_eq!(result.columns[1].as_str(), "name");
        assert_eq!(result.rows.len(), 2);
    }

    #[test]
    fn test_duckdb_oracle_reset() {
        let mut oracle = DuckDbOracle::new().expect("Failed to create DuckDB oracle");

        // Create and populate a table
        oracle
            .execute("CREATE TABLE test (id INTEGER)")
            .expect("Failed to create table");
        oracle
            .execute("INSERT INTO test VALUES (1), (2), (3)")
            .expect("Failed to insert data");

        // Reset
        oracle.reset().expect("Failed to reset");

        // Table should no longer exist
        let result = oracle.execute("SELECT * FROM test");
        assert!(result.is_err());
    }

    #[test]
    fn test_duckdb_oracle_null_handling() {
        let mut oracle = DuckDbOracle::new().expect("Failed to create DuckDB oracle");

        oracle
            .execute("CREATE TABLE test (id INTEGER, value INTEGER)")
            .expect("Failed to create table");
        oracle
            .execute("INSERT INTO test VALUES (1, NULL), (2, 42)")
            .expect("Failed to insert data");

        let result = oracle
            .execute("SELECT * FROM test ORDER BY id")
            .expect("Failed to query data");

        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][1], Value::Null);
        assert_eq!(result.rows[1][1], Value::Integer(42));
    }
}
