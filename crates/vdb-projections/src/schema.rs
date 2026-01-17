//! Schema cache for mapping table names to column names.
//!
//! The preupdate_hook provides column values by index, not by name. This cache
//! maintains the mapping from table names to ordered column names, enabling
//! the hook to construct properly labeled [`ChangeEvent`](crate::ChangeEvent)s.
//!
//! The cache is populated at startup from existing tables and updated during
//! migrations when new tables are created.

use std::{collections::HashMap, sync::RwLock};

use sqlx::SqlitePool;

use crate::{ColumnName, ProjectionError, TableName};

/// Thread-safe cache mapping table names to their column names.
///
/// Used by the preupdate_hook to look up column names by index.
/// The cache must be populated before any DML operations occur.
///
/// # Example
///
/// ```ignore
/// use vdb_projections::schema::SchemaCache;
/// use std::sync::Arc;
///
/// let cache = Arc::new(SchemaCache::new());
/// cache.populate_from_db(&pool).await?;
///
/// // Later, in the hook:
/// let columns = cache.get_columns(&table_name).unwrap();
/// ```
#[derive(Debug)]
pub struct SchemaCache {
    tables: RwLock<HashMap<TableName, Vec<ColumnName>>>,
}

impl Default for SchemaCache {
    fn default() -> Self {
        Self {
            tables: RwLock::new(HashMap::default()),
        }
    }
}

impl SchemaCache {
    /// Populates the cache from all existing tables in the database.
    ///
    /// Call this at startup before installing the preupdate hook.
    /// Queries `sqlite_master` for all user tables and fetches their
    /// column information via `PRAGMA table_info`.
    pub async fn from_db(pool: &SqlitePool) -> Result<Self, ProjectionError> {
        let cache = Self::default();
        let tables: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE
  'sqlite_%'",
        )
        .fetch_all(pool)
        .await?;

        for (table_name,) in tables {
            let table_name = TableName::from(table_name);
            let columns = cache.query_table_columns(pool, &table_name).await?;
            cache.tables.write().unwrap().insert(table_name, columns);
        }

        Ok(cache)
    }

    async fn query_table_columns(
        &self,
        pool: &SqlitePool,
        table: &TableName,
    ) -> Result<Vec<ColumnName>, ProjectionError> {
        use sqlx::Row;

        use sqlx::AssertSqlSafe;

        // PRAGMA table_info returns: cid, name, type, notnull, dflt_value, pk
        // We only need column name at index 1
        let query = format!("PRAGMA table_info(\"{}\")", table.as_identifier());
        // Safe: table name comes from sqlite_master, not user input
        let rows = sqlx::raw_sql(AssertSqlSafe(query)).fetch_all(pool).await?;

        let columns = rows
            .into_iter()
            .map(|row| {
                let name: String = row.get(1);
                ColumnName::from(name)
            })
            .collect();

        Ok(columns)
    }

    /// Register a new table during migrations
    pub fn register_table(&self, table: TableName, columns: Vec<ColumnName>) {
        self.tables.write().unwrap().insert(table, columns);
    }

    /// Get columns for a table (called from hook)
    pub fn get_columns(&self, table: &TableName) -> Option<Vec<ColumnName>> {
        self.tables.read().unwrap().get(table).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_columns(names: &[&str]) -> Vec<ColumnName> {
        names
            .iter()
            .map(|n| ColumnName::from(n.to_string()))
            .collect()
    }

    #[test]
    fn new_creates_empty_cache() {
        let cache = SchemaCache::default();
        let table = TableName::from_sqlite("users");
        assert!(cache.get_columns(&table).is_none());
    }

    #[test]
    fn default_creates_empty_cache() {
        let cache = SchemaCache::default();
        let table = TableName::from_sqlite("users");
        assert!(cache.get_columns(&table).is_none());
    }

    #[test]
    fn register_and_get_table() {
        let cache = SchemaCache::default();
        let table = TableName::from_sqlite("users");
        let columns = make_columns(&["id", "name", "email"]);

        cache.register_table(table.clone(), columns.clone());

        let retrieved = cache.get_columns(&table);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), columns);
    }

    #[test]
    fn get_missing_table_returns_none() {
        let cache = SchemaCache::default();

        // Register one table
        cache.register_table(
            TableName::from_sqlite("users"),
            make_columns(&["id", "name"]),
        );

        // Query a different table
        let other = TableName::from_sqlite("patients");
        assert!(cache.get_columns(&other).is_none());
    }

    #[test]
    fn register_overwrites_existing() {
        let cache = SchemaCache::default();
        let table = TableName::from_sqlite("users");

        // Register initial columns
        cache.register_table(table.clone(), make_columns(&["id", "name"]));

        // Overwrite with new columns (e.g., after ALTER TABLE)
        let new_columns = make_columns(&["id", "name", "email", "created_at"]);
        cache.register_table(table.clone(), new_columns.clone());

        let retrieved = cache.get_columns(&table).unwrap();
        assert_eq!(retrieved, new_columns);
    }

    #[test]
    fn multiple_tables() {
        let cache = SchemaCache::default();

        let users = TableName::from_sqlite("users");
        let patients = TableName::from_sqlite("patients");
        let appointments = TableName::from_sqlite("appointments");

        cache.register_table(users.clone(), make_columns(&["id", "name"]));
        cache.register_table(patients.clone(), make_columns(&["id", "dob", "mrn"]));
        cache.register_table(
            appointments.clone(),
            make_columns(&["id", "patient_id", "scheduled_at"]),
        );

        assert_eq!(
            cache.get_columns(&users).unwrap(),
            make_columns(&["id", "name"])
        );
        assert_eq!(
            cache.get_columns(&patients).unwrap(),
            make_columns(&["id", "dob", "mrn"])
        );
        assert_eq!(
            cache.get_columns(&appointments).unwrap(),
            make_columns(&["id", "patient_id", "scheduled_at"])
        );
    }

    #[test]
    fn get_columns_returns_clone() {
        let cache = SchemaCache::default();
        let table = TableName::from_sqlite("users");
        let columns = make_columns(&["id", "name"]);

        cache.register_table(table.clone(), columns.clone());

        // Get columns twice - should be independent clones
        let first = cache.get_columns(&table).unwrap();
        let second = cache.get_columns(&table).unwrap();

        assert_eq!(first, second);
        assert_eq!(first, columns);
    }

    #[test]
    fn empty_columns_list() {
        let cache = SchemaCache::default();
        let table = TableName::from_sqlite("empty_table");

        cache.register_table(table.clone(), vec![]);

        let retrieved = cache.get_columns(&table).unwrap();
        assert!(retrieved.is_empty());
    }

    #[test]
    fn thread_safety_concurrent_reads() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(SchemaCache::default());
        let table = TableName::from_sqlite("users");
        let columns = make_columns(&["id", "name", "email"]);

        cache.register_table(table.clone(), columns.clone());

        let mut handles = vec![];

        // Spawn multiple reader threads
        for _ in 0..10 {
            let cache = Arc::clone(&cache);
            let table = table.clone();
            let expected = columns.clone();

            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    let retrieved = cache.get_columns(&table).unwrap();
                    assert_eq!(retrieved, expected);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn thread_safety_concurrent_writes() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(SchemaCache::default());
        let mut handles = vec![];

        // Spawn multiple writer threads, each writing different tables
        for i in 0..10 {
            let cache = Arc::clone(&cache);

            handles.push(thread::spawn(move || {
                let table = TableName::from_sqlite(&format!("table_{}", i));
                let columns = make_columns(&["id", "value"]);
                cache.register_table(table, columns);
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all tables were registered
        for i in 0..10 {
            let table = TableName::from_sqlite(&format!("table_{}", i));
            assert!(cache.get_columns(&table).is_some());
        }
    }
}
