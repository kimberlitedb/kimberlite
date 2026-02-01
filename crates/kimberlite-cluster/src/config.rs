//! Cluster configuration management.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Configuration for a Kimberlite cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Number of nodes in the cluster.
    pub node_count: usize,

    /// Base port number (node N uses base_port + N).
    pub base_port: u16,

    /// Root data directory for cluster.
    pub data_dir: PathBuf,

    /// Cluster topology (peers, leaders, etc.).
    pub topology: ClusterTopology,
}

/// Cluster topology configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterTopology {
    /// Node configurations.
    pub nodes: Vec<NodeConfig>,
}

/// Configuration for a single node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Node ID (0-indexed).
    pub id: usize,

    /// Port number.
    pub port: u16,

    /// Address to bind to.
    pub bind_address: String,

    /// Data directory for this node.
    pub data_dir: PathBuf,

    /// Peer addresses (for replication).
    pub peers: Vec<String>,
}

impl ClusterConfig {
    /// Creates a new cluster configuration.
    pub fn new(data_dir: PathBuf, node_count: usize, base_port: u16) -> Self {
        if node_count == 0 {
            panic!("Node count must be >= 1");
        }

        // Generate node configs
        let mut nodes = Vec::with_capacity(node_count);
        for id in 0..node_count {
            let port = base_port + id as u16;
            let node_data_dir = data_dir.join("cluster").join(format!("node-{}", id));

            // Build peer list (all other nodes)
            let peers: Vec<String> = (0..node_count)
                .filter(|&peer_id| peer_id != id)
                .map(|peer_id| format!("127.0.0.1:{}", base_port + peer_id as u16))
                .collect();

            nodes.push(NodeConfig {
                id,
                port,
                bind_address: "127.0.0.1".to_string(),
                data_dir: node_data_dir,
                peers,
            });
        }

        Self {
            node_count,
            base_port,
            data_dir: data_dir.clone(),
            topology: ClusterTopology { nodes },
        }
    }

    /// Loads cluster configuration from disk.
    pub fn load(data_dir: &Path) -> Result<Self> {
        let config_path = data_dir.join("cluster").join("cluster.toml");

        if !config_path.exists() {
            return Err(Error::NotInitialized(data_dir.to_path_buf()));
        }

        let content = fs::read_to_string(&config_path)?;
        let config: Self = toml::from_str(&content)?;

        Ok(config)
    }

    /// Saves cluster configuration to disk.
    pub fn save(&self) -> Result<()> {
        let cluster_dir = self.data_dir.join("cluster");
        fs::create_dir_all(&cluster_dir)?;

        let config_path = cluster_dir.join("cluster.toml");
        let content = toml::to_string_pretty(self)?;
        fs::write(config_path, content)?;

        Ok(())
    }

    /// Creates directory structure for all nodes.
    pub fn create_directories(&self) -> Result<()> {
        for node in &self.topology.nodes {
            fs::create_dir_all(&node.data_dir)?;
        }
        Ok(())
    }

    /// Returns the configuration for a specific node.
    pub fn get_node(&self, id: usize) -> Option<&NodeConfig> {
        self.topology.nodes.get(id)
    }

    /// Returns the cluster directory path.
    pub fn cluster_dir(&self) -> PathBuf {
        self.data_dir.join("cluster")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cluster_config_creation() {
        let temp = TempDir::new().unwrap();
        let config = ClusterConfig::new(temp.path().to_path_buf(), 3, 5432);

        assert_eq!(config.node_count, 3);
        assert_eq!(config.base_port, 5432);
        assert_eq!(config.topology.nodes.len(), 3);

        // Check node 0
        let node0 = &config.topology.nodes[0];
        assert_eq!(node0.id, 0);
        assert_eq!(node0.port, 5432);
        assert_eq!(node0.peers.len(), 2); // 2 other nodes

        // Check node 1
        let node1 = &config.topology.nodes[1];
        assert_eq!(node1.id, 1);
        assert_eq!(node1.port, 5433);
    }

    #[test]
    fn test_save_and_load() {
        let temp = TempDir::new().unwrap();
        let config = ClusterConfig::new(temp.path().to_path_buf(), 3, 5432);

        config.save().unwrap();

        let loaded = ClusterConfig::load(temp.path()).unwrap();
        assert_eq!(loaded.node_count, 3);
        assert_eq!(loaded.base_port, 5432);
        assert_eq!(loaded.topology.nodes.len(), 3);
    }

    #[test]
    fn test_create_directories() {
        let temp = TempDir::new().unwrap();
        let config = ClusterConfig::new(temp.path().to_path_buf(), 3, 5432);

        config.create_directories().unwrap();

        for i in 0..3 {
            let node_dir = temp.path().join("cluster").join(format!("node-{}", i));
            assert!(node_dir.exists());
        }
    }

    #[test]
    fn test_get_node() {
        let temp = TempDir::new().unwrap();
        let config = ClusterConfig::new(temp.path().to_path_buf(), 3, 5432);

        let node = config.get_node(1).unwrap();
        assert_eq!(node.id, 1);
        assert_eq!(node.port, 5433);

        assert!(config.get_node(10).is_none());
    }

    #[test]
    fn test_peer_list_excludes_self() {
        let temp = TempDir::new().unwrap();
        let config = ClusterConfig::new(temp.path().to_path_buf(), 3, 5432);

        let node0 = &config.topology.nodes[0];
        assert_eq!(node0.peers.len(), 2);
        assert!(!node0.peers.iter().any(|p| p.contains("5432")));
        assert!(node0.peers.iter().any(|p| p.contains("5433")));
        assert!(node0.peers.iter().any(|p| p.contains("5434")));
    }
}
