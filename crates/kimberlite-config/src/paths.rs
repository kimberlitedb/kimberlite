//! Path utilities and XDG directory discovery

use crate::ConfigError;
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

/// XDG-compliant paths for Kimberlite
pub struct Paths {
    project_dirs: Option<ProjectDirs>,
}

impl Paths {
    /// Create a new Paths instance with XDG discovery
    pub fn new() -> Self {
        Self {
            project_dirs: ProjectDirs::from("com", "Kimberlite", "kimberlite"),
        }
    }

    /// Get user config directory (~/.config/kimberlite/)
    pub fn user_config_dir(&self) -> Result<PathBuf, ConfigError> {
        self.project_dirs
            .as_ref()
            .map(|p| p.config_dir().to_path_buf())
            .ok_or_else(|| {
                ConfigError::XdgError("Failed to determine user config directory".to_string())
            })
    }

    /// Get user cache directory (~/.cache/kimberlite/)
    pub fn user_cache_dir(&self) -> Result<PathBuf, ConfigError> {
        self.project_dirs
            .as_ref()
            .map(|p| p.cache_dir().to_path_buf())
            .ok_or_else(|| {
                ConfigError::XdgError("Failed to determine user cache directory".to_string())
            })
    }

    /// Get user config file path (~/.config/kimberlite/config.toml)
    pub fn user_config_file(&self) -> Result<PathBuf, ConfigError> {
        Ok(self.user_config_dir()?.join("config.toml"))
    }

    /// Get project config file path (kimberlite.toml)
    pub fn project_config_file(project_dir: impl AsRef<Path>) -> PathBuf {
        project_dir.as_ref().join("kimberlite.toml")
    }

    /// Get local config file path (kimberlite.local.toml, gitignored)
    pub fn local_config_file(project_dir: impl AsRef<Path>) -> PathBuf {
        project_dir.as_ref().join("kimberlite.local.toml")
    }

    /// Get .kimberlite state directory
    pub fn state_dir(project_dir: impl AsRef<Path>) -> PathBuf {
        project_dir.as_ref().join(".kimberlite")
    }

    /// Get migrations directory
    pub fn migrations_dir(project_dir: impl AsRef<Path>) -> PathBuf {
        project_dir.as_ref().join("migrations")
    }

    /// Check if a project is initialized (has kimberlite.toml)
    pub fn is_initialized(project_dir: impl AsRef<Path>) -> bool {
        Self::project_config_file(project_dir).exists()
    }
}

impl Default for Paths {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_xdg_paths() {
        let paths = Paths::new();

        // These should not panic (though paths may vary by platform)
        if let Ok(config_dir) = paths.user_config_dir() {
            assert!(config_dir.to_string_lossy().contains("kimberlite"));
        }

        if let Ok(cache_dir) = paths.user_cache_dir() {
            assert!(cache_dir.to_string_lossy().contains("kimberlite"));
        }
    }

    #[test]
    fn test_project_paths() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let project_dir = temp_dir.path();

        let config_file = Paths::project_config_file(project_dir);
        assert_eq!(config_file, project_dir.join("kimberlite.toml"));

        let local_file = Paths::local_config_file(project_dir);
        assert_eq!(local_file, project_dir.join("kimberlite.local.toml"));

        let state_dir = Paths::state_dir(project_dir);
        assert_eq!(state_dir, project_dir.join(".kimberlite"));

        assert!(!Paths::is_initialized(project_dir));

        // Create config file
        std::fs::write(&config_file, "[project]\nname = \"test\"\n").unwrap();
        assert!(Paths::is_initialized(project_dir));
    }
}
