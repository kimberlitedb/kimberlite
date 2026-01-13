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

impl SchemaCache {
    /// Creates an empty schema cache.
    pub fn new() -> Self {
        Self {
            tables: RwLock::new(HashMap::new()),
        }
    }

    /// Populates the cache from all existing tables in the database.
    ///
    /// Call this at startup before installing the preupdate hook.
    /// Queries `sqlite_master` for all user tables and fetches their
    /// column information via `PRAGMA table_info`.
    pub async fn populate_from_db(&self, pool: &SqlitePool) -> Result<(), ProjectionError> {
        let tables: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE
  'sqlite_%'",
        )
        .fetch_all(pool)
        .await?;

        for (table_name,) in tables {
            let table_name = TableName::from(table_name);
            let columns = self.query_table_columns(pool, &table_name).await?;
            self.tables.write().unwrap().insert(table_name, columns);
        }
        Ok(())
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

impl Default for SchemaCache {
    fn default() -> Self {
        Self::new()
    }
}
