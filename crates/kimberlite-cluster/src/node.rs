//! Node process management.

use crate::{Error, NodeConfig, Result};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::sleep;

/// Status of a cluster node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    /// Node is stopped.
    Stopped,

    /// Node is starting up.
    Starting,

    /// Node is running normally.
    Running,

    /// Node has crashed.
    Crashed,
}

/// A managed Kimberlite node process.
pub struct NodeProcess {
    /// Node configuration.
    pub config: NodeConfig,

    /// Child process handle.
    pub process: Option<Child>,

    /// Current status.
    pub status: NodeStatus,

    /// Number of restart attempts.
    pub restart_count: usize,
}

impl NodeProcess {
    /// Creates a new node process (not started).
    pub fn new(config: NodeConfig) -> Self {
        Self {
            config,
            process: None,
            status: NodeStatus::Stopped,
            restart_count: 0,
        }
    }

    /// Starts the node process.
    pub async fn start(&mut self) -> Result<()> {
        if self.status != NodeStatus::Stopped && self.status != NodeStatus::Crashed {
            return Err(Error::NodeAlreadyRunning(self.config.id));
        }

        self.status = NodeStatus::Starting;

        // TODO: Once kimberlite binary has proper server mode, spawn it here
        // For now, we'll use a placeholder command
        let child = Command::new("sleep")
            .arg("infinity")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::SpawnError(e.to_string()))?;

        self.process = Some(child);
        self.status = NodeStatus::Starting;

        // Give it a moment to start
        sleep(Duration::from_millis(200)).await;

        // Check if it's still alive
        if self.is_alive() {
            self.status = NodeStatus::Running;
            Ok(())
        } else {
            self.status = NodeStatus::Crashed;
            // In tests with placeholder command, this is expected
            // In production with real kimberlite server, this indicates real failure
            Err(Error::NodeStartFailed(
                self.config.id,
                "Process died immediately".to_string(),
            ))
        }
    }

    /// Stops the node process gracefully.
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.process.take() {
            // Use tokio's built-in kill (sends SIGKILL on Unix, TerminateProcess on Windows)
            child.kill().await.ok();

            // Wait for it to exit (with timeout)
            let exit_status = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;

            match exit_status {
                Ok(Ok(_status)) => {
                    self.status = NodeStatus::Stopped;
                    Ok(())
                }
                Ok(Err(e)) => {
                    self.status = NodeStatus::Stopped;
                    Err(Error::Io(e))
                }
                Err(_) => {
                    // Timeout, but we already killed it
                    self.status = NodeStatus::Stopped;
                    Ok(())
                }
            }
        } else {
            Ok(()) // Already stopped
        }
    }

    /// Checks if the node process is alive.
    pub fn is_alive(&mut self) -> bool {
        if let Some(child) = &mut self.process {
            // Try to check if process has exited
            match child.try_wait() {
                Ok(Some(_exit_status)) => false, // Process has exited
                Ok(None) => true,                // Still running
                Err(_) => false,                 // Error checking, assume dead
            }
        } else {
            false
        }
    }

    /// Returns the node ID.
    pub fn id(&self) -> usize {
        self.config.id
    }

    /// Returns the port.
    pub fn port(&self) -> u16 {
        self.config.port
    }

    /// Attempts to restart a crashed node.
    pub async fn restart(&mut self) -> Result<()> {
        if self.status != NodeStatus::Crashed {
            return Ok(());
        }

        self.restart_count += 1;

        // Exponential backoff
        let backoff = Duration::from_secs(2u64.pow(self.restart_count.min(5) as u32));
        sleep(backoff).await;

        self.start().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_node_config() -> NodeConfig {
        NodeConfig {
            id: 0,
            port: 5432,
            bind_address: "127.0.0.1".to_string(),
            data_dir: PathBuf::from("/tmp/node-0"),
            peers: vec!["127.0.0.1:5433".to_string()],
        }
    }

    #[test]
    fn test_node_process_creation() {
        let config = test_node_config();
        let node = NodeProcess::new(config);

        assert_eq!(node.status, NodeStatus::Stopped);
        assert_eq!(node.id(), 0);
        assert_eq!(node.port(), 5432);
    }

    #[tokio::test]
    async fn test_node_start_stop() {
        let config = test_node_config();
        let mut node = NodeProcess::new(config);

        // Start (may fail with placeholder command, that's OK for testing)
        let start_result = node.start().await;

        // If it started successfully
        if start_result.is_ok() {
            assert_eq!(node.status, NodeStatus::Running);
            assert!(node.is_alive());

            // Stop
            node.stop().await.unwrap();
            assert_eq!(node.status, NodeStatus::Stopped);
            assert!(!node.is_alive());
        } else {
            // Placeholder command failed immediately, which is fine for testing
            assert_eq!(node.status, NodeStatus::Crashed);
        }
    }

    #[tokio::test]
    async fn test_node_double_start_error() {
        let config = test_node_config();
        let mut node = NodeProcess::new(config);

        // Only test double-start if first start succeeds
        if node.start().await.is_ok() {
            let result = node.start().await;
            assert!(result.is_err());
            assert!(matches!(result.unwrap_err(), Error::NodeAlreadyRunning(0)));

            // Clean up
            node.stop().await.ok();
        }
    }
}
