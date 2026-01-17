//! SQLite connection pools with optimized settings for projections.
//!
//! Separate read/write pools following SQLite best practices:
//! - Write pool: Single connection (serialized writes, no SQLITE_BUSY)
//! - Read pool: Multiple connections (parallel reads)
//! - WAL mode: Readers don't block writers
//! - SQLCipher: Optional encryption at rest
//!
//! Performance tuning based on:
//! https://fractaledmind.github.io/2024/04/15/sqlite-on-rails-the-how-and-why-of-optimal-performance/

use serde::{Deserialize, Serialize};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::str::FromStr;
use std::time::Duration;

/// Configuration for projection database pools.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PoolConfig {
    /// Maximum read connections (default: 8)
    pub read_max_connections: u32,
    /// Maximum write connections (MUST be 1 for correctness)
    pub write_max_connections: u32,
    /// Cache size in KiB (default: 64MB)
    pub cache_size_kib: i32,
    /// Memory-mapped I/O size in bytes (default: 256MB)
    pub mmap_size: u64,
    /// Busy timeout in milliseconds (default: 5000)
    pub busy_timeout_ms: u64,
    /// Connection acquire timeout in seconds (default: 30)
    pub acquire_timeout_secs: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            read_max_connections: 8,
            write_max_connections: 1, // MUST be 1 to serialize writes
            cache_size_kib: -64000,   // Negative = KiB, so 64MB
            mmap_size: 268_435_456,   // 256MB
            busy_timeout_ms: 5000,
            acquire_timeout_secs: 30,
        }
    }
}

/// SQLite database with separate read and write pools.
///
/// Write pool is limited to 1 connection to serialize writes.
/// Read pool allows concurrent queries via WAL mode.
#[derive(Clone, Debug)]
pub struct ProjectionDb {
    pub read_pool: SqlitePool,
    pub write_pool: SqlitePool,
}

impl ProjectionDb {
    /// Opens a projection database with optional SQLCipher encryption.
    ///
    /// # Arguments
    /// * `path` - Path to SQLite database file
    /// * `key` - Optional SQLCipher encryption key
    /// * `config` - Pool configuration
    pub async fn open(
        path: &str,
        key: Option<&str>,
        config: PoolConfig,
    ) -> Result<Self, sqlx::Error> {
        let write_pool = Self::create_write_pool(path, key, &config).await?;
        let read_pool = Self::create_read_pool(path, key, &config).await?;

        tracing::info!(
            path = path,
            encrypted = key.is_some(),
            read_connections = config.read_max_connections,
            "Projection database opened"
        );

        Ok(Self {
            read_pool,
            write_pool,
        })
    }

    /// Opens an encrypted projection database with default config.
    pub async fn open_encrypted(path: &str, key: &str) -> Result<Self, sqlx::Error> {
        Self::open(path, Some(key), PoolConfig::default()).await
    }

    /// Opens an unencrypted projection database with default config.
    pub async fn open_unencrypted(path: &str) -> Result<Self, sqlx::Error> {
        Self::open(path, None, PoolConfig::default()).await
    }

    async fn create_write_pool(
        path: &str,
        key: Option<&str>,
        config: &PoolConfig,
    ) -> Result<SqlitePool, sqlx::Error> {
        let mut options = SqliteConnectOptions::from_str(path)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_millis(config.busy_timeout_ms));

        // SQLCipher key MUST be set first
        if let Some(k) = key {
            options = options.pragma("key", format!("\"{}\"", k));
        }

        // Performance pragmas
        options = options
            .pragma("cache_size", config.cache_size_kib.to_string())
            .pragma("temp_store", "MEMORY")
            .pragma("mmap_size", config.mmap_size.to_string())
            .pragma("foreign_keys", "ON");

        let pool = SqlitePoolOptions::new()
            .max_connections(config.write_max_connections)
            .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
            .connect_with(options)
            .await?;

        tracing::debug!(
            max_connections = config.write_max_connections,
            "Write pool created"
        );

        Ok(pool)
    }

    async fn create_read_pool(
        path: &str,
        key: Option<&str>,
        config: &PoolConfig,
    ) -> Result<SqlitePool, sqlx::Error> {
        let mut options = SqliteConnectOptions::from_str(path)?
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_millis(config.busy_timeout_ms))
            .read_only(true);

        // SQLCipher key MUST be set first
        if let Some(k) = key {
            options = options.pragma("key", format!("\"{}\"", k));
        }

        // Performance pragmas
        options = options
            .pragma("cache_size", config.cache_size_kib.to_string())
            .pragma("temp_store", "MEMORY")
            .pragma("mmap_size", config.mmap_size.to_string());

        let pool = SqlitePoolOptions::new()
            .max_connections(config.read_max_connections)
            .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
            .connect_with(options)
            .await?;

        tracing::debug!(
            max_connections = config.read_max_connections,
            "Read pool created"
        );

        Ok(pool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_open_unencrypted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.db");
        let path_str = path.to_str().unwrap();

        let db = ProjectionDb::open_unencrypted(path_str).await.unwrap();

        // Verify we can execute queries
        sqlx::query("SELECT 1")
            .execute(&db.read_pool)
            .await
            .unwrap();

        sqlx::query("CREATE TABLE test (id INTEGER PRIMARY KEY)")
            .execute(&db.write_pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_open_encrypted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("encrypted.db");
        let path_str = path.to_str().unwrap();

        let db = ProjectionDb::open_encrypted(path_str, "test-secret-key")
            .await
            .unwrap();

        // Verify we can execute queries
        sqlx::query("CREATE TABLE secrets (id INTEGER PRIMARY KEY, data TEXT)")
            .execute(&db.write_pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO secrets (data) VALUES ('sensitive')")
            .execute(&db.write_pool)
            .await
            .unwrap();
    }
}
