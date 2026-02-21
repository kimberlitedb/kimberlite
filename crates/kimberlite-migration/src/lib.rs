//! SQL migration system for Kimberlite database.
//!
//! Provides file-based migration management with:
//! - Auto-numbered SQL migration files
//! - Checksum-based integrity validation
//! - Migration tracking in database
//! - Lock file to prevent tampering

pub mod error;
pub mod file;
pub mod lock;
pub mod tracker;

pub use error::{Error, Result};
pub use file::{Migration, MigrationFile};
pub use lock::LockFile;
pub use tracker::MigrationTracker;

use std::path::PathBuf;

/// Configuration for migration system.
#[derive(Debug, Clone)]
pub struct MigrationConfig {
    /// Directory containing migration files (default: "migrations/")
    pub migrations_dir: PathBuf,

    /// Directory for state files (default: ".kimberlite/migrations/")
    pub state_dir: PathBuf,

    /// Auto-generate timestamps in migration filenames
    pub auto_timestamp: bool,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            migrations_dir: PathBuf::from("migrations"),
            state_dir: PathBuf::from(".kimberlite/migrations"),
            auto_timestamp: true,
        }
    }
}

impl MigrationConfig {
    /// Creates config with custom migrations directory.
    pub fn with_migrations_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            migrations_dir: dir.into(),
            ..Default::default()
        }
    }

    /// Returns path to lock file.
    pub fn lock_file_path(&self) -> PathBuf {
        self.state_dir.join(".lock")
    }
}

/// Migration manager coordinates file operations and tracking.
pub struct MigrationManager {
    config: MigrationConfig,
    tracker: MigrationTracker,
}

impl MigrationManager {
    /// Creates a new migration manager.
    pub fn new(config: MigrationConfig) -> Result<Self> {
        let tracker = MigrationTracker::new(config.state_dir.clone())?;
        Ok(Self { config, tracker })
    }

    /// Lists all migration files in directory.
    pub fn list_files(&self) -> Result<Vec<MigrationFile>> {
        MigrationFile::discover(&self.config.migrations_dir)
    }

    /// Lists pending migrations (not yet applied).
    pub fn list_pending(&self) -> Result<Vec<MigrationFile>> {
        let all_files = self.list_files()?;
        let applied = self.tracker.list_applied()?;

        let applied_ids: std::collections::HashSet<_> = applied.iter().map(|m| m.id).collect();

        Ok(all_files
            .into_iter()
            .filter(|f| !applied_ids.contains(&f.migration.id))
            .collect())
    }

    /// Creates a new migration file.
    pub fn create(&self, name: &str) -> Result<MigrationFile> {
        MigrationFile::create(
            &self.config.migrations_dir,
            name,
            self.config.auto_timestamp,
        )
    }

    /// Records a migration as applied.
    pub fn record_applied(&self, file: &MigrationFile) -> Result<tracker::AppliedMigration> {
        self.tracker.record_applied(
            file.migration.id,
            file.migration.name.clone(),
            file.checksum.clone(),
        )
    }

    /// Removes a migration record (for rollback).
    pub fn remove_applied(&self, id: u32) -> Result<()> {
        self.tracker.remove_applied(id)
    }

    /// Returns the SQL content for the up migration (before "-- Down Migration" marker).
    pub fn up_sql(file: &MigrationFile) -> &str {
        if let Some(idx) = file.migration.sql.find("-- Down Migration") {
            file.migration.sql[..idx].trim_end()
        } else {
            file.migration.sql.trim()
        }
    }

    /// Returns the SQL content for the down migration (after "-- Down Migration" marker).
    pub fn down_sql(file: &MigrationFile) -> Option<&str> {
        file.migration
            .sql
            .find("-- Down Migration")
            .map(|idx| {
                let after = &file.migration.sql[idx..];
                // Skip the marker line itself
                after.find('\n').map_or("", |nl| after[nl + 1..].trim())
            })
            .filter(|s| !s.is_empty())
    }

    /// Validates all migrations (checksums, sequence).
    pub fn validate(&self) -> Result<()> {
        let lock = LockFile::load(&self.config.lock_file_path())?;
        let files = self.list_files()?;

        lock.validate(&files)?;

        // Validate sequence (no gaps)
        for (i, file) in files.iter().enumerate() {
            let expected_id = (i + 1) as u32;
            if file.migration.id != expected_id {
                return Err(Error::InvalidSequence {
                    expected: expected_id,
                    found: file.migration.id,
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_migration_config_default() {
        let config = MigrationConfig::default();
        assert_eq!(config.migrations_dir, PathBuf::from("migrations"));
        assert_eq!(config.state_dir, PathBuf::from(".kimberlite/migrations"));
        assert!(config.auto_timestamp);
    }

    #[test]
    fn test_migration_config_lock_path() {
        let config = MigrationConfig::default();
        assert_eq!(
            config.lock_file_path(),
            PathBuf::from(".kimberlite/migrations/.lock")
        );
    }

    #[test]
    fn test_migration_manager_creation() {
        let temp = TempDir::new().unwrap();
        let config = MigrationConfig {
            migrations_dir: temp.path().join("migrations"),
            state_dir: temp.path().join("state"),
            auto_timestamp: false,
        };

        std::fs::create_dir_all(&config.migrations_dir).unwrap();
        std::fs::create_dir_all(&config.state_dir).unwrap();

        let manager = MigrationManager::new(config);
        assert!(manager.is_ok());
    }
}
