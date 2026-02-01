//! Migration lock file for integrity validation.

use crate::{Error, MigrationFile, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Lock file entry for a migration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockEntry {
    /// Migration ID
    pub id: u32,

    /// Migration name
    pub name: String,

    /// SHA-256 checksum
    pub checksum: String,
}

/// Migration lock file for tamper detection.
///
/// Stores checksums of all migrations to detect if they've been modified
/// after being applied. This prevents data integrity issues from migrations
/// being changed retroactively.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockFile {
    /// Version of lock file format
    pub version: u32,

    /// Locked migrations (stored as vec for TOML compatibility)
    #[serde(rename = "migration")]
    pub migrations: Vec<LockEntry>,
}

impl LockFile {
    /// Creates a new empty lock file.
    pub fn new() -> Self {
        Self {
            version: 1,
            migrations: Vec::new(),
        }
    }

    /// Loads lock file from disk.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }

        let content = fs::read_to_string(path)?;

        if content.trim().is_empty() {
            return Ok(Self::new());
        }

        let lock_file: Self = toml::from_str(&content)?;

        Ok(lock_file)
    }

    /// Saves lock file to disk.
    pub fn save(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;

        Ok(())
    }

    /// Adds a migration to the lock file.
    pub fn lock(&mut self, file: &MigrationFile) {
        let entry = LockEntry {
            id: file.migration.id,
            name: file.migration.name.clone(),
            checksum: file.checksum.clone(),
        };

        // Remove existing entry if present
        self.migrations.retain(|e| e.id != file.migration.id);
        self.migrations.push(entry);

        // Keep sorted by ID
        self.migrations.sort_by_key(|e| e.id);
    }

    /// Validates migrations against lock file.
    pub fn validate(&self, files: &[MigrationFile]) -> Result<()> {
        for file in files {
            if let Some(locked) = self.migrations.iter().find(|e| e.id == file.migration.id) {
                // Check if checksum matches
                if locked.checksum != file.checksum {
                    return Err(Error::ChecksumMismatch {
                        id: file.migration.id,
                        expected: locked.checksum.clone(),
                        actual: file.checksum.clone(),
                    });
                }
            }
        }

        Ok(())
    }

    /// Checks if a migration is locked.
    pub fn is_locked(&self, id: u32) -> bool {
        self.migrations.iter().any(|e| e.id == id)
    }

    /// Updates lock file with new migrations.
    pub fn update(&mut self, files: &[MigrationFile]) -> Result<()> {
        // Validate existing locked migrations first
        self.validate(files)?;

        // Add new migrations
        for file in files {
            if !self.is_locked(file.migration.id) {
                self.lock(file);
            }
        }

        Ok(())
    }
}

impl Default for LockFile {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Migration;
    use chrono::Utc;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_migration_file(id: u32, name: &str, sql: &str) -> MigrationFile {
        let migration = Migration {
            id,
            name: name.to_string(),
            sql: sql.to_string(),
            created_at: Utc::now(),
            author: None,
        };

        let checksum = migration.checksum();

        MigrationFile {
            migration,
            path: PathBuf::from(format!("{id:04}_{name}.sql")),
            checksum,
        }
    }

    #[test]
    fn test_lock_file_creation() {
        let lock = LockFile::new();

        assert_eq!(lock.version, 1);
        assert!(lock.migrations.is_empty());
    }

    #[test]
    fn test_lock_migration() {
        let mut lock = LockFile::new();
        let file = create_test_migration_file(1, "test", "SELECT 1;");

        lock.lock(&file);

        assert!(lock.is_locked(1));
        assert_eq!(lock.migrations.iter().find(|e| e.id == 1).unwrap().name, "test");
    }

    #[test]
    fn test_validate_success() {
        let mut lock = LockFile::new();
        let file = create_test_migration_file(1, "test", "SELECT 1;");

        lock.lock(&file);

        // Validate with same file should succeed
        assert!(lock.validate(&[file]).is_ok());
    }

    #[test]
    fn test_validate_checksum_mismatch() {
        let mut lock = LockFile::new();
        let file1 = create_test_migration_file(1, "test", "SELECT 1;");

        lock.lock(&file1);

        // Different SQL = different checksum
        let file2 = create_test_migration_file(1, "test", "SELECT 2;");

        let result = lock.validate(&[file2]);

        assert!(matches!(result, Err(Error::ChecksumMismatch { .. })));
    }

    #[test]
    fn test_update_lock_file() {
        let mut lock = LockFile::new();
        let file1 = create_test_migration_file(1, "first", "SELECT 1;");

        lock.lock(&file1);

        // Add second migration
        let file2 = create_test_migration_file(2, "second", "SELECT 2;");
        lock.update(&[file1.clone(), file2.clone()]).unwrap();

        assert!(lock.is_locked(1));
        assert!(lock.is_locked(2));
    }

    #[test]
    fn test_save_and_load() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(".lock");

        let mut lock = LockFile::new();
        let file = create_test_migration_file(1, "test", "SELECT 1;");
        lock.lock(&file);

        // Save
        lock.save(&path).unwrap();
        assert!(path.exists());

        // Load
        let loaded = LockFile::load(&path).unwrap();

        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.migrations.len(), 1);
        assert!(loaded.is_locked(1));
    }

    #[test]
    fn test_load_nonexistent_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(".lock");

        let lock = LockFile::load(&path).unwrap();

        assert!(lock.migrations.is_empty());
    }
}
