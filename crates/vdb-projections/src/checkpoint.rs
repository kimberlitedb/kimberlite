use sqlx::{SqlitePool, prelude::FromRow};
use time::OffsetDateTime;
use vdb_types::Offset;

use crate::ProjectionError;

#[derive(Debug, FromRow)]
pub struct Checkpoint {
    pub name: String,
    pub last_offset: Offset,
    pub checksum: i64,
    pub updated_at: OffsetDateTime,
}

impl Checkpoint {
    /// Creates the checkpoint table if it doesn't exist
    pub async fn ensure_table(pool: &SqlitePool) -> Result<(), ProjectionError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS _vdb_checkpoints (
                name TEXT PRIMARY KEY,
                last_offset INTEGER NOT NULL,
                checksum INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Loads checkpoint, returns None if not found
    pub async fn load(pool: &SqlitePool, name: &str) -> Result<Option<Self>, ProjectionError> {
        let result = sqlx::query_as(
            r#"
            SELECT name, last_offset, checksum, updated_at
            FROM _vdb_checkpoints
            WHERE name = ?
            "#,
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        Ok(result)
    }

    /// Upserts checkpoint
    pub async fn save(&self, pool: &SqlitePool) -> Result<(), ProjectionError> {
        sqlx::query(
            r#"
                INSERT INTO _vdb_checkpoints (name, last_offset, checksum, updated_at)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT (name) DO UPDATE SET
                    last_offset = excluded.last_offset,
                    checksum    = excluded.checksum,
                    updated_at  = excluded.updated_at
            "#,
        )
        .bind(&self.name)
        .bind(self.last_offset)
        .bind(self.checksum)
        .bind(self.updated_at)
        .execute(pool)
        .await?;

        Ok(())
    }
}
