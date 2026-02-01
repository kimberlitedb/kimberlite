//! Multi-node cluster management for Kimberlite.
//!
//! Provides local cluster orchestration for testing and development:
//! - Process supervision for multiple Kimberlite nodes
//! - Cluster initialization and topology configuration
//! - Health monitoring and failover testing
//! - Single supervisor process managing N nodes

pub mod config;
pub mod error;
pub mod node;
pub mod supervisor;

pub use config::{ClusterConfig, ClusterTopology, NodeConfig};
pub use error::{Error, Result};
pub use node::{NodeProcess, NodeStatus};
pub use supervisor::ClusterSupervisor;

use std::path::PathBuf;

/// Creates a new cluster with the specified number of nodes.
pub fn init_cluster(data_dir: PathBuf, node_count: usize, base_port: u16) -> Result<ClusterConfig> {
    let config = ClusterConfig::new(data_dir, node_count, base_port);
    config.save()?;
    config.create_directories()?;
    Ok(config)
}

/// Starts an existing cluster.
pub async fn start_cluster(data_dir: PathBuf) -> Result<ClusterSupervisor> {
    let config = ClusterConfig::load(&data_dir)?;
    let mut supervisor = ClusterSupervisor::new(config);
    supervisor.start_all().await?;
    Ok(supervisor)
}

/// Stops a running cluster gracefully.
pub async fn stop_cluster(supervisor: &mut ClusterSupervisor) -> Result<()> {
    supervisor.stop_all().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_cluster() {
        let temp = TempDir::new().unwrap();
        let config = init_cluster(temp.path().to_path_buf(), 3, 5432).unwrap();

        assert_eq!(config.node_count, 3);
        assert_eq!(config.base_port, 5432);
        assert!(temp.path().join("cluster").exists());
    }

    #[test]
    fn test_cluster_config_save_load() {
        let temp = TempDir::new().unwrap();
        let config = ClusterConfig::new(temp.path().to_path_buf(), 3, 5432);
        config.save().unwrap();

        let loaded = ClusterConfig::load(temp.path()).unwrap();
        assert_eq!(loaded.node_count, config.node_count);
        assert_eq!(loaded.base_port, config.base_port);
    }
}
