//! Migration file format and parsing.

use crate::{Error, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// A SQL migration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Migration {
    /// Sequential ID (1, 2, 3, ...)
    pub id: u32,

    /// Human-readable name (e.g., "add_patients_table")
    pub name: String,

    /// SQL content
    pub sql: String,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Author (optional)
    pub author: Option<String>,
}

impl Migration {
    /// Computes SHA-256 checksum of migration content.
    pub fn checksum(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.sql.as_bytes());
        let result = hasher.finalize();
        // Convert to hex string
        result.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

/// A migration file on disk.
#[derive(Debug, Clone)]
pub struct MigrationFile {
    /// Parsed migration data
    pub migration: Migration,

    /// File path
    pub path: PathBuf,

    /// SHA-256 checksum
    pub checksum: String,
}

impl MigrationFile {
    /// Parses a migration file from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path).map_err(|e| Error::Io(e))?;

        let migration = Self::parse(&content, path)?;
        let checksum = migration.checksum();

        Ok(Self {
            migration,
            path: path.to_path_buf(),
            checksum,
        })
    }

    /// Parses migration from file content.
    fn parse(content: &str, path: &Path) -> Result<Migration> {
        // Extract metadata from comments at top of file
        let mut name: Option<String> = None;
        let mut created_at: Option<DateTime<Utc>> = None;
        let mut author: Option<String> = None;
        let mut sql_lines = Vec::new();
        let mut in_metadata = true;

        for line in content.lines() {
            let trimmed = line.trim();

            // Parse metadata comments
            if in_metadata && trimmed.starts_with("--") {
                let comment = trimmed.trim_start_matches("--").trim();

                if let Some(rest) = comment.strip_prefix("Migration:") {
                    name = Some(rest.trim().to_string());
                } else if let Some(rest) = comment.strip_prefix("Created:") {
                    created_at = DateTime::parse_from_rfc3339(rest.trim())
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc));
                } else if let Some(rest) = comment.strip_prefix("Author:") {
                    author = Some(rest.trim().to_string());
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with("--") {
                // End of metadata section
                in_metadata = false;
                sql_lines.push(line);
            } else if !in_metadata {
                sql_lines.push(line);
            }
        }

        // Extract ID from filename (e.g., "0001_add_users.sql" -> 1)
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::ParseError {
                path: path.to_path_buf(),
                reason: "Invalid filename".to_string(),
            })?;

        let id_str = filename.split('_').next().ok_or_else(|| Error::ParseError {
            path: path.to_path_buf(),
            reason: "Filename must start with numeric ID".to_string(),
        })?;

        let id: u32 = id_str.parse().map_err(|_| Error::ParseError {
            path: path.to_path_buf(),
            reason: format!("Invalid migration ID: {}", id_str),
        })?;

        // If no name in comments, extract from filename
        if name.is_none() {
            let name_part = filename
                .trim_end_matches(".sql")
                .split('_')
                .skip(1)
                .collect::<Vec<_>>()
                .join("_");
            name = Some(name_part);
        }

        let sql = sql_lines.join("\n");

        Ok(Migration {
            id,
            name: name.ok_or_else(|| Error::ParseError {
                path: path.to_path_buf(),
                reason: "Missing migration name".to_string(),
            })?,
            sql,
            created_at: created_at.unwrap_or_else(Utc::now),
            author,
        })
    }

    /// Creates a new migration file.
    pub fn create(
        migrations_dir: &Path,
        name: &str,
        auto_timestamp: bool,
    ) -> Result<Self> {
        // Validate name (alphanumeric + underscores only)
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(Error::InvalidName(name.to_string()));
        }

        // Create migrations directory if it doesn't exist
        fs::create_dir_all(migrations_dir)?;

        // Find next ID
        let next_id = Self::next_id(migrations_dir)?;

        // Generate filename
        let filename = if auto_timestamp {
            format!(
                "{:04}_{}.sql",
                next_id,
                name.replace(' ', "_").to_lowercase()
            )
        } else {
            format!("{:04}_{}.sql", next_id, name.replace(' ', "_").to_lowercase())
        };

        let path = migrations_dir.join(&filename);
        let created_at = Utc::now();

        // Generate file content with metadata
        let content = format!(
            "-- Migration: {}\n\
             -- Created: {}\n\
             -- Author: \n\n\
             -- Up Migration\n\
             -- TODO: Add your SQL here\n\n\
             -- Down Migration (optional)\n\
             -- TODO: Add rollback SQL here\n",
            name,
            created_at.to_rfc3339()
        );

        fs::write(&path, &content)?;

        let migration = Migration {
            id: next_id,
            name: name.to_string(),
            sql: String::new(), // Empty until user fills it in
            created_at,
            author: None,
        };

        let checksum = migration.checksum();

        Ok(Self {
            migration,
            path,
            checksum,
        })
    }

    /// Discovers all migration files in directory.
    pub fn discover(migrations_dir: &Path) -> Result<Vec<Self>> {
        if !migrations_dir.exists() {
            return Ok(Vec::new());
        }

        let mut files = Vec::new();

        for entry in fs::read_dir(migrations_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("sql") {
                files.push(Self::load(&path)?);
            }
        }

        // Sort by ID
        files.sort_by_key(|f| f.migration.id);

        Ok(files)
    }

    /// Finds the next available migration ID.
    fn next_id(migrations_dir: &Path) -> Result<u32> {
        let existing = Self::discover(migrations_dir)?;

        Ok(existing
            .iter()
            .map(|f| f.migration.id)
            .max()
            .unwrap_or(0)
            + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_migration_with_metadata() {
        let content = r#"-- Migration: Add users table
-- Created: 2026-02-01T10:00:00Z
-- Author: alice@example.com

CREATE TABLE users (
    id BIGINT NOT NULL,
    name TEXT NOT NULL
);
"#;

        let temp = TempDir::new().unwrap();
        let path = temp.path().join("0001_add_users.sql");
        fs::write(&path, content).unwrap();

        let file = MigrationFile::load(&path).unwrap();

        assert_eq!(file.migration.id, 1);
        assert_eq!(file.migration.name, "Add users table");
        assert_eq!(file.migration.author, Some("alice@example.com".to_string()));
        assert!(file.migration.sql.contains("CREATE TABLE users"));
    }

    #[test]
    fn test_parse_migration_without_metadata() {
        let content = "CREATE TABLE users (id BIGINT);";

        let temp = TempDir::new().unwrap();
        let path = temp.path().join("0002_create_users.sql");
        fs::write(&path, content).unwrap();

        let file = MigrationFile::load(&path).unwrap();

        assert_eq!(file.migration.id, 2);
        assert_eq!(file.migration.name, "create_users");
        assert_eq!(file.migration.author, None);
    }

    #[test]
    fn test_create_migration() {
        let temp = TempDir::new().unwrap();
        let file = MigrationFile::create(temp.path(), "add_patients", true).unwrap();

        assert_eq!(file.migration.id, 1);
        assert_eq!(file.migration.name, "add_patients");
        assert!(file.path.exists());

        let content = fs::read_to_string(&file.path).unwrap();
        assert!(content.contains("Migration: add_patients"));
    }

    #[test]
    fn test_discover_migrations() {
        let temp = TempDir::new().unwrap();

        MigrationFile::create(temp.path(), "first", false).unwrap();
        MigrationFile::create(temp.path(), "second", false).unwrap();

        let files = MigrationFile::discover(temp.path()).unwrap();

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].migration.id, 1);
        assert_eq!(files[1].migration.id, 2);
    }

    #[test]
    fn test_checksum_consistency() {
        let migration = Migration {
            id: 1,
            name: "test".to_string(),
            sql: "SELECT 1;".to_string(),
            created_at: Utc::now(),
            author: None,
        };

        let checksum1 = migration.checksum();
        let checksum2 = migration.checksum();

        assert_eq!(checksum1, checksum2);
        assert_eq!(checksum1.len(), 64); // SHA-256 hex is 64 chars
    }

    #[test]
    fn test_invalid_migration_name() {
        let temp = TempDir::new().unwrap();
        let result = MigrationFile::create(temp.path(), "invalid/name", false);

        assert!(matches!(result, Err(Error::InvalidName(_))));
    }
}
