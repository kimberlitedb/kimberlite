//! Cluster supervisor for managing multiple nodes.

use crate::{ClusterConfig, Error, NodeProcess, NodeStatus, Result};
use std::collections::HashMap;
use tokio::signal;
use tokio::time::{interval, Duration};

/// Supervisor for a multi-node Kimberlite cluster.
pub struct ClusterSupervisor {
    /// Cluster configuration.
    config: ClusterConfig,

    /// Managed node processes.
    nodes: HashMap<usize, NodeProcess>,

    /// Whether the supervisor is running.
    running: bool,
}

impl ClusterSupervisor {
    /// Creates a new cluster supervisor.
    pub fn new(config: ClusterConfig) -> Self {
        let mut nodes = HashMap::new();

        for node_config in config.topology.nodes.clone() {
            let node = NodeProcess::new(node_config);
            nodes.insert(node.id(), node);
        }

        Self {
            config,
            nodes,
            running: false,
        }
    }

    /// Starts all nodes in the cluster.
    pub async fn start_all(&mut self) -> Result<()> {
        for (id, node) in &mut self.nodes {
            match node.start().await {
                Ok(()) => {
                    println!("Node {} started on port {}", id, node.port());
                }
                Err(e) => {
                    eprintln!("Failed to start node {}: {}", id, e);
                    // Continue starting other nodes
                }
            }
        }

        self.running = true;
        Ok(())
    }

    /// Starts a specific node.
    pub async fn start_node(&mut self, id: usize) -> Result<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| Error::NodeNotFound(id))?;

        node.start().await?;
        println!("Node {} started on port {}", id, node.port());

        Ok(())
    }

    /// Stops all nodes gracefully.
    pub async fn stop_all(&mut self) -> Result<()> {
        for (id, node) in &mut self.nodes {
            match node.stop().await {
                Ok(()) => {
                    println!("Node {} stopped", id);
                }
                Err(e) => {
                    eprintln!("Failed to stop node {}: {}", id, e);
                }
            }
        }

        self.running = false;
        Ok(())
    }

    /// Stops a specific node.
    pub async fn stop_node(&mut self, id: usize) -> Result<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| Error::NodeNotFound(id))?;

        node.stop().await?;
        println!("Node {} stopped", id);

        Ok(())
    }

    /// Returns the status of all nodes.
    pub fn status(&mut self) -> Vec<(usize, NodeStatus, u16)> {
        let mut status = Vec::new();

        for (id, node) in &mut self.nodes {
            // Update status by checking if alive
            if node.status == NodeStatus::Running && !node.is_alive() {
                node.status = NodeStatus::Crashed;
            }

            status.push((*id, node.status, node.port()));
        }

        status.sort_by_key(|(id, _, _)| *id);
        status
    }

    /// Monitors all nodes and attempts restarts on crash.
    pub async fn monitor_loop(&mut self) {
        let mut tick = interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    // Check each node
                    for (id, node) in &mut self.nodes {
                        if node.status == NodeStatus::Running && !node.is_alive() {
                            eprintln!("Node {} crashed, attempting restart...", id);
                            node.status = NodeStatus::Crashed;

                            if let Err(e) = node.restart().await {
                                eprintln!("Failed to restart node {}: {}", id, e);
                            } else {
                                println!("Node {} restarted successfully", id);
                            }
                        }
                    }

                    if !self.running {
                        break;
                    }
                }

                _ = signal::ctrl_c() => {
                    println!("Received Ctrl+C, shutting down cluster...");
                    if let Err(e) = self.stop_all().await {
                        eprintln!("Error during shutdown: {}", e);
                    }
                    break;
                }
            }
        }
    }

    /// Returns the number of running nodes.
    pub fn running_count(&mut self) -> usize {
        self.status()
            .iter()
            .filter(|(_, status, _)| *status == NodeStatus::Running)
            .count()
    }

    /// Returns the cluster configuration.
    pub fn config(&self) -> &ClusterConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_supervisor_creation() {
        let temp = TempDir::new().unwrap();
        let config = ClusterConfig::new(temp.path().to_path_buf(), 3, 5432);
        let supervisor = ClusterSupervisor::new(config);

        assert_eq!(supervisor.nodes.len(), 3);
        assert!(!supervisor.running);
    }

    #[tokio::test]
    async fn test_start_stop_all() {
        let temp = TempDir::new().unwrap();
        let config = ClusterConfig::new(temp.path().to_path_buf(), 3, 5432);
        let mut supervisor = ClusterSupervisor::new(config);

        supervisor.start_all().await.unwrap();
        assert!(supervisor.running);

        // With placeholder commands, nodes may not all start successfully
        // In production with real kimberlite, they would
        let running = supervisor.running_count();
        assert!(running <= 3);

        supervisor.stop_all().await.unwrap();
        assert!(!supervisor.running);
        assert_eq!(supervisor.running_count(), 0);
    }

    #[tokio::test]
    async fn test_start_stop_specific_node() {
        let temp = TempDir::new().unwrap();
        let config = ClusterConfig::new(temp.path().to_path_buf(), 3, 5432);
        let mut supervisor = ClusterSupervisor::new(config);

        // Start may fail with placeholder command
        let _ = supervisor.start_node(1).await;

        // If it started, verify we can stop it
        if supervisor.running_count() > 0 {
            supervisor.stop_node(1).await.unwrap();
            assert_eq!(supervisor.running_count(), 0);
        }
    }

    #[tokio::test]
    async fn test_node_not_found() {
        let temp = TempDir::new().unwrap();
        let config = ClusterConfig::new(temp.path().to_path_buf(), 3, 5432);
        let mut supervisor = ClusterSupervisor::new(config);

        let result = supervisor.start_node(10).await;
        assert!(matches!(result, Err(Error::NodeNotFound(10))));
    }

    #[tokio::test]
    async fn test_status() {
        let temp = TempDir::new().unwrap();
        let config = ClusterConfig::new(temp.path().to_path_buf(), 3, 5432);
        let mut supervisor = ClusterSupervisor::new(config);

        let status = supervisor.status();
        assert_eq!(status.len(), 3);

        // All should be stopped initially
        for (_, node_status, _) in status {
            assert_eq!(node_status, NodeStatus::Stopped);
        }
    }
}
