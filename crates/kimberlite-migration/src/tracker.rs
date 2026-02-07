//! Migration tracking system.

use crate::{Error, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A migration that has been applied to the database.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppliedMigration {
    /// Migration ID
    pub id: u32,

    /// Migration name
    pub name: String,

    /// SHA-256 checksum
    pub checksum: String,

    /// When it was applied
    pub applied_at: DateTime<Utc>,

    /// Who applied it (optional)
    pub applied_by: Option<String>,
}

/// Tracks applied migrations using a simple JSON file.
///
/// In a production implementation, this would be stored in the database itself
/// as a projection. For now, we use a file-based tracker for simplicity.
pub struct MigrationTracker {
    state_file: PathBuf,
}

impl MigrationTracker {
    /// Creates a new migration tracker.
    pub fn new(state_dir: impl Into<PathBuf>) -> Result<Self> {
        let state_dir = state_dir.into();
        fs::create_dir_all(&state_dir)?;

        let state_file = state_dir.join("applied.toml");

        Ok(Self { state_file })
    }

    /// Records a migration as applied.
    pub fn record_applied(
        &self,
        id: u32,
        name: String,
        checksum: String,
    ) -> Result<AppliedMigration> {
        let mut applied = self.load_state()?;

        // Check if already applied
        if applied.iter().any(|m| m.id == id) {
            return Err(Error::AlreadyApplied(id));
        }

        let record = AppliedMigration {
            id,
            name,
            checksum,
            applied_at: Utc::now(),
            applied_by: None, // TODO: Get from environment
        };

        applied.push(record.clone());
        self.save_state(&applied)?;

        Ok(record)
    }

    /// Lists all applied migrations.
    pub fn list_applied(&self) -> Result<Vec<AppliedMigration>> {
        self.load_state()
    }

    /// Checks if a migration has been applied.
    pub fn is_applied(&self, id: u32) -> Result<bool> {
        let applied = self.load_state()?;
        Ok(applied.iter().any(|m| m.id == id))
    }

    /// Removes a migration record (for rollback).
    pub fn remove_applied(&self, id: u32) -> Result<()> {
        let mut applied = self.load_state()?;
        let initial_len = applied.len();
        applied.retain(|m| m.id != id);

        if applied.len() == initial_len {
            // Migration wasn't in the list — not an error for idempotency
            return Ok(());
        }

        self.save_state(&applied)?;
        Ok(())
    }

    /// Gets the last applied migration ID.
    pub fn last_applied_id(&self) -> Result<Option<u32>> {
        let applied = self.load_state()?;
        Ok(applied.iter().map(|m| m.id).max())
    }

    /// Loads state from file.
    fn load_state(&self) -> Result<Vec<AppliedMigration>> {
        if !self.state_file.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&self.state_file)?;

        if content.trim().is_empty() {
            return Ok(Vec::new());
        }

        let state: MigrationState = toml::from_str(&content)?;

        Ok(state.migrations)
    }

    /// Saves state to file.
    fn save_state(&self, migrations: &[AppliedMigration]) -> Result<()> {
        let state = MigrationState {
            migrations: migrations.to_vec(),
        };

        let content = toml::to_string_pretty(&state)?;
        fs::write(&self.state_file, content)?;

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MigrationState {
    migrations: Vec<AppliedMigration>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_tracker_creation() {
        let temp = TempDir::new().unwrap();
        let tracker = MigrationTracker::new(temp.path().to_path_buf()).unwrap();

        // State file is created on demand, not immediately
        assert!(tracker.state_file.parent().unwrap().exists());
        assert_eq!(tracker.list_applied().unwrap().len(), 0);
    }

    #[test]
    fn test_record_and_list_applied() {
        let temp = TempDir::new().unwrap();
        let tracker = MigrationTracker::new(temp.path().to_path_buf()).unwrap();

        tracker
            .record_applied(1, "first".to_string(), "abc123".to_string())
            .unwrap();
        tracker
            .record_applied(2, "second".to_string(), "def456".to_string())
            .unwrap();

        let applied = tracker.list_applied().unwrap();

        assert_eq!(applied.len(), 2);
        assert_eq!(applied[0].id, 1);
        assert_eq!(applied[0].name, "first");
        assert_eq!(applied[1].id, 2);
    }

    #[test]
    fn test_is_applied() {
        let temp = TempDir::new().unwrap();
        let tracker = MigrationTracker::new(temp.path().to_path_buf()).unwrap();

        tracker
            .record_applied(1, "first".to_string(), "abc123".to_string())
            .unwrap();

        assert!(tracker.is_applied(1).unwrap());
        assert!(!tracker.is_applied(2).unwrap());
    }

    #[test]
    fn test_already_applied_error() {
        let temp = TempDir::new().unwrap();
        let tracker = MigrationTracker::new(temp.path().to_path_buf()).unwrap();

        tracker
            .record_applied(1, "first".to_string(), "abc123".to_string())
            .unwrap();

        let result = tracker.record_applied(1, "first".to_string(), "abc123".to_string());

        assert!(matches!(result, Err(Error::AlreadyApplied(1))));
    }

    #[test]
    fn test_last_applied_id() {
        let temp = TempDir::new().unwrap();
        let tracker = MigrationTracker::new(temp.path().to_path_buf()).unwrap();

        assert_eq!(tracker.last_applied_id().unwrap(), None);

        tracker
            .record_applied(1, "first".to_string(), "abc123".to_string())
            .unwrap();
        assert_eq!(tracker.last_applied_id().unwrap(), Some(1));

        tracker
            .record_applied(2, "second".to_string(), "def456".to_string())
            .unwrap();
        assert_eq!(tracker.last_applied_id().unwrap(), Some(2));
    }

    #[test]
    fn test_persistence() {
        let temp = TempDir::new().unwrap();

        {
            let tracker = MigrationTracker::new(temp.path().to_path_buf()).unwrap();
            tracker
                .record_applied(1, "first".to_string(), "abc123".to_string())
                .unwrap();
        }

        // Create new tracker instance, should load existing state
        let tracker = MigrationTracker::new(temp.path().to_path_buf()).unwrap();
        let applied = tracker.list_applied().unwrap();

        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].id, 1);
    }
}
