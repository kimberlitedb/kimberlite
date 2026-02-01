//! Configuration management for Kimberlite
//!
//! Provides hierarchical configuration loading from multiple sources:
//! 1. CLI arguments (highest precedence)
//! 2. Environment variables (KMB_* prefix)
//! 3. kimberlite.local.toml (gitignored, local overrides)
//! 4. kimberlite.toml (git-tracked, project config)
//! 5. ~/.config/kimberlite/config.toml (user defaults)
//! 6. Built-in defaults (lowest precedence)

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod error;
mod loader;
mod paths;

pub use error::ConfigError;
pub use loader::ConfigLoader;
pub use paths::Paths;

/// Main Kimberlite configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct KimberliteConfig {
    pub project: ProjectConfig,
    pub database: DatabaseConfig,
    pub development: DevelopmentConfig,
    pub replication: ReplicationConfig,
    pub cluster: ClusterConfig,
    pub migrations: MigrationConfig,
    pub studio: StudioConfig,
    pub tenants: TenantConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub name: String,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: "kimberlite-project".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    pub data_dir: PathBuf,
    pub bind_address: String,
    pub max_connections: u32,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from(".kimberlite/data"),
            bind_address: "127.0.0.1:5432".to_string(),
            max_connections: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DevelopmentConfig {
    pub studio: bool,
    pub studio_port: u16,
    pub auto_migrate: bool,
    pub watch: bool,
}

impl Default for DevelopmentConfig {
    fn default() -> Self {
        Self {
            studio: true,
            studio_port: 5555,
            auto_migrate: true,
            watch: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplicationConfig {
    pub mode: ReplicationMode,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            mode: ReplicationMode::SingleNode,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReplicationMode {
    None,
    SingleNode,
    Cluster,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClusterConfig {
    pub nodes: u32,
    pub base_port: u16,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            nodes: 3,
            base_port: 5432,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MigrationConfig {
    pub directory: PathBuf,
    pub auto_timestamp: bool,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("migrations"),
            auto_timestamp: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StudioConfig {
    pub default_tenant: Option<u64>,
    pub time_travel: bool,
}

impl Default for StudioConfig {
    fn default() -> Self {
        Self {
            default_tenant: Some(1),
            time_travel: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TenantConfig {
    pub mode: TenantMode,
    pub allow_dynamic_create: bool,
    pub require_confirmation: bool,
}

impl Default for TenantConfig {
    fn default() -> Self {
        Self {
            mode: TenantMode::Explicit,
            allow_dynamic_create: true,
            require_confirmation: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TenantMode {
    Explicit,
    AutoCreate,
}

/// Tenant definition from config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantDefinition {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub protected: bool,
}

impl KimberliteConfig {
    /// Load configuration from default locations
    pub fn load() -> Result<Self> {
        ConfigLoader::new().load()
    }

    /// Load configuration from specific project directory
    pub fn load_from_dir(project_dir: impl AsRef<Path>) -> Result<Self> {
        ConfigLoader::new().with_project_dir(project_dir).load()
    }

    /// Create a development configuration
    pub fn development() -> Self {
        Self {
            development: DevelopmentConfig {
                studio: true,
                auto_migrate: true,
                ..Default::default()
            },
            replication: ReplicationConfig {
                mode: ReplicationMode::None,
            },
            ..Default::default()
        }
    }

    /// Create a production configuration
    pub fn production() -> Self {
        Self {
            development: DevelopmentConfig {
                studio: false,
                auto_migrate: false,
                watch: false,
                ..Default::default()
            },
            replication: ReplicationConfig {
                mode: ReplicationMode::Cluster,
            },
            ..Default::default()
        }
    }

    /// Resolve relative paths to absolute
    pub fn resolve_paths(&mut self, base_dir: impl AsRef<Path>) {
        let base = base_dir.as_ref();

        if self.database.data_dir.is_relative() {
            self.database.data_dir = base.join(&self.database.data_dir);
        }

        if self.migrations.directory.is_relative() {
            self.migrations.directory = base.join(&self.migrations.directory);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = KimberliteConfig::default();
        assert_eq!(config.database.bind_address, "127.0.0.1:5432");
        assert_eq!(config.cluster.nodes, 3);
        assert!(config.development.studio);
        assert_eq!(config.tenants.mode, TenantMode::Explicit);
    }

    #[test]
    fn test_development_config() {
        let config = KimberliteConfig::development();
        assert!(config.development.studio);
        assert!(config.development.auto_migrate);
        assert_eq!(config.replication.mode, ReplicationMode::None);
    }

    #[test]
    fn test_production_config() {
        let config = KimberliteConfig::production();
        assert!(!config.development.studio);
        assert!(!config.development.auto_migrate);
        assert_eq!(config.replication.mode, ReplicationMode::Cluster);
    }

    #[test]
    fn test_path_resolution() {
        let mut config = KimberliteConfig::default();
        config.resolve_paths("/home/user/project");

        assert_eq!(
            config.database.data_dir,
            PathBuf::from("/home/user/project/.kimberlite/data")
        );
        assert_eq!(
            config.migrations.directory,
            PathBuf::from("/home/user/project/migrations")
        );
    }
}
