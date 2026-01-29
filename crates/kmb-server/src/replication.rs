//! VSR replication integration for the server.
//!
//! This module provides a unified interface for command submission through
//! VSR replication. It supports both single-node and cluster modes.
//!
//! # Replication Modes
//!
//! - **None**: Direct kernel apply without VSR (legacy mode, no durability guarantees)
//! - **SingleNode**: Single-node VSR with file-based superblock (durable, for development)
//! - **Cluster**: Multi-node VSR with full consensus (production-grade)

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Arc, RwLock};

use tracing::{debug, info};
use kimberlite::Kimberlite;
use kmb_kernel::Command;
use kmb_types::IdempotencyId;
use kmb_vsr::{
    ClusterAddresses, ClusterConfig, MultiNodeConfig, MultiNodeReplicator, Replicator,
    SingleNodeReplicator,
};

use crate::config::ReplicationMode;
use crate::error::{ServerError, ServerResult};

/// A command submitter that routes commands through the appropriate replication layer.
///
/// This abstraction allows the server to use either direct kernel apply (legacy mode)
/// or VSR replication (single-node or cluster mode) transparently.
pub enum CommandSubmitter {
    /// Direct mode - applies commands directly to Kimberlite without VSR.
    Direct { db: Kimberlite },

    /// Single-node VSR mode - uses `SingleNodeReplicator` with file-based superblock.
    SingleNode {
        replicator: Arc<RwLock<SingleNodeReplicator<File>>>,
        db: Kimberlite,
    },

    /// Cluster mode - uses `MultiNodeReplicator` with full VSR consensus.
    Cluster {
        replicator: Arc<RwLock<MultiNodeReplicator>>,
        db: Kimberlite,
    },
}

impl CommandSubmitter {
    /// Creates a new command submitter based on the replication mode.
    pub fn new(mode: &ReplicationMode, db: Kimberlite, data_dir: &Path) -> ServerResult<Self> {
        match mode {
            ReplicationMode::None => {
                info!("starting in direct mode (no VSR replication)");
                Ok(Self::Direct { db })
            }

            ReplicationMode::SingleNode { replica_id } => {
                info!(replica_id = replica_id.as_u8(), "starting single-node VSR replication");

                let config = ClusterConfig::single_node(*replica_id);

                // Use file-based superblock for durability
                let superblock_path =
                    data_dir.join(format!("superblock-single-{}.vsr", replica_id.as_u8()));

                // Check if superblock exists BEFORE opening/creating
                let exists = superblock_path.exists();

                let replicator = if exists {
                    // Open existing replicator with persisted state
                    debug!(path = %superblock_path.display(), "opening existing single-node replicator");
                    let superblock = Self::open_superblock(&superblock_path)?;
                    SingleNodeReplicator::open(config, superblock)
                        .map_err(|e| ServerError::Replication(e.to_string()))?
                } else {
                    // Create new replicator with fresh superblock
                    debug!(path = %superblock_path.display(), "creating new single-node replicator");
                    let superblock = Self::create_superblock(&superblock_path, *replica_id)?;
                    SingleNodeReplicator::create(config, superblock)
                        .map_err(|e| ServerError::Replication(e.to_string()))?
                };

                Ok(Self::SingleNode {
                    replicator: Arc::new(RwLock::new(replicator)),
                    db,
                })
            }

            ReplicationMode::Cluster { replica_id, peers } => {
                info!(
                    replica_id = replica_id.as_u8(),
                    peer_count = peers.len(),
                    "starting cluster VSR replication"
                );

                // Build cluster addresses
                let addresses = ClusterAddresses::from_pairs(peers.iter().copied());

                // Create superblock path
                let superblock_path =
                    data_dir.join(format!("superblock-{}.vsr", replica_id.as_u8()));

                // Create multi-node config
                let config = MultiNodeConfig::new(*replica_id, addresses, superblock_path);

                // Start the multi-node replicator
                let replicator = MultiNodeReplicator::start(config)
                    .map_err(|e| ServerError::Replication(e.to_string()))?;

                Ok(Self::Cluster {
                    replicator: Arc::new(RwLock::new(replicator)),
                    db,
                })
            }
        }
    }

    /// Opens an existing superblock file for reading and writing.
    fn open_superblock(path: &Path) -> ServerResult<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| ServerError::Replication(format!("failed to open superblock: {e}")))
    }

    /// Creates a new superblock file and ensures parent directories exist.
    fn create_superblock(path: &Path, _replica_id: kmb_vsr::ReplicaId) -> ServerResult<File> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ServerError::Replication(format!("failed to create data directory: {e}"))
            })?;
        }
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|e| ServerError::Replication(format!("failed to create superblock: {e}")))
    }

    /// Submits a command for processing.
    ///
    /// In direct mode, this applies the command directly to the kernel.
    /// In VSR mode, this routes through the replicator for durable processing.
    pub fn submit(&self, command: Command) -> ServerResult<SubmissionResult> {
        self.submit_with_idempotency(command, None)
    }

    /// Submits a command with an optional idempotency ID.
    ///
    /// The idempotency ID enables duplicate detection for retried requests.
    pub fn submit_with_idempotency(
        &self,
        command: Command,
        idempotency_id: Option<IdempotencyId>,
    ) -> ServerResult<SubmissionResult> {
        match self {
            Self::Direct { db } => {
                // Direct mode: apply to Kimberlite
                db.submit(command.clone())?;

                // Direct mode doesn't track operation numbers
                Ok(SubmissionResult {
                    was_duplicate: false,
                    effects_applied: true,
                })
            }

            Self::SingleNode { replicator, db } => {
                let mut repl = replicator
                    .write()
                    .map_err(|_| ServerError::Replication("lock poisoned".to_string()))?;

                // Submit to replicator
                let result = repl
                    .submit(command.clone(), idempotency_id)
                    .map_err(|e| ServerError::Replication(e.to_string()))?;

                // If not a duplicate, apply to Kimberlite for projection updates
                // The replicator handles durability, but Kimberlite manages projections
                if !result.was_duplicate {
                    db.submit(command)?;
                }

                Ok(SubmissionResult {
                    was_duplicate: result.was_duplicate,
                    effects_applied: true,
                })
            }

            Self::Cluster { replicator, db } => {
                let mut repl = replicator
                    .write()
                    .map_err(|_| ServerError::Replication("lock poisoned".to_string()))?;

                // Check if we're the leader before submitting
                if !repl.is_leader() {
                    // Get the current view for the error response
                    let view = repl.view().as_u64();

                    // Get the leader's address for client redirection
                    let leader_hint = repl.leader_address();

                    return Err(ServerError::NotLeader { view, leader_hint });
                }

                // Submit to replicator (blocks until committed or error)
                let result = repl.submit(command.clone(), idempotency_id).map_err(|e| {
                    // Check if the error is a NotLeader error from VSR
                    let msg = e.to_string();
                    if msg.contains("not the leader") || msg.contains("NotLeader") {
                        ServerError::NotLeader {
                            view: repl.view().as_u64(),
                            leader_hint: repl.leader_address(),
                        }
                    } else {
                        ServerError::Replication(msg)
                    }
                })?;

                // If not a duplicate, apply to Kimberlite for projection updates
                if !result.was_duplicate {
                    db.submit(command)?;
                }

                Ok(SubmissionResult {
                    was_duplicate: result.was_duplicate,
                    effects_applied: true,
                })
            }
        }
    }

    /// Returns a reference to the underlying Kimberlite instance.
    pub fn kimberlite(&self) -> &Kimberlite {
        match self {
            Self::Direct { db } | Self::SingleNode { db, .. } | Self::Cluster { db, .. } => db,
        }
    }

    /// Returns true if VSR replication is enabled.
    pub fn is_replicated(&self) -> bool {
        !matches!(self, Self::Direct { .. })
    }

    /// Returns the current replication status (for health checks).
    pub fn status(&self) -> ReplicationStatus {
        match self {
            Self::Direct { .. } => ReplicationStatus {
                mode: "direct",
                is_leader: true,
                replica_id: None,
                commit_number: None,
                view: None,
                leader_id: None,
                connected_peers: None,
                bootstrap_complete: None,
            },
            Self::SingleNode { replicator, .. } => {
                let repl = replicator.read().ok();
                ReplicationStatus {
                    mode: "single-node",
                    is_leader: true, // Single-node is always leader
                    replica_id: repl
                        .as_ref()
                        .and_then(|r| r.config().replicas().next().map(|id| id.as_u8())),
                    commit_number: repl.as_ref().map(|r| r.commit_number().as_u64()),
                    view: Some(0), // Single-node is always view 0
                    leader_id: repl
                        .as_ref()
                        .and_then(|r| r.config().replicas().next().map(|id| id.as_u8())),
                    connected_peers: Some(0), // No peers in single-node
                    bootstrap_complete: Some(true), // Always complete
                }
            }
            Self::Cluster { replicator, .. } => {
                let repl = replicator.read().ok();
                ReplicationStatus {
                    mode: "cluster",
                    is_leader: repl.as_ref().is_some_and(|r| r.is_leader()),
                    replica_id: repl
                        .as_ref()
                        .and_then(|r| r.config().replicas().next().map(|id| id.as_u8())),
                    commit_number: repl.as_ref().map(|r| r.commit_number().as_u64()),
                    view: repl.as_ref().map(|r| r.view().as_u64()),
                    leader_id: repl.as_ref().and_then(|r| r.leader_id().map(|id| id.as_u8())),
                    connected_peers: repl.as_ref().map(|r| {
                        // Get connected peers from the shared state
                        let state = r.cluster_config();
                        state.cluster_size().saturating_sub(1) // Approximate: cluster_size - 1
                    }),
                    bootstrap_complete: repl.as_ref().map(|r| r.is_bootstrap_complete()),
                }
            }
        }
    }

    /// Returns true if this node is the leader (for cluster mode).
    pub fn is_leader(&self) -> bool {
        match self {
            Self::Direct { .. } | Self::SingleNode { .. } => true, // Single-node is always leader
            Self::Cluster { replicator, .. } => replicator.read().is_ok_and(|r| r.is_leader()),
        }
    }
}

/// Result of command submission.
#[derive(Debug, Clone)]
pub struct SubmissionResult {
    /// Whether this was a duplicate request (idempotency hit).
    pub was_duplicate: bool,
    /// Whether effects were successfully applied.
    pub effects_applied: bool,
}

/// Replication status for health/metrics.
#[derive(Debug, Clone)]
pub struct ReplicationStatus {
    /// Replication mode name.
    pub mode: &'static str,
    /// Whether this node is the leader.
    pub is_leader: bool,
    /// Replica ID (if replicated).
    pub replica_id: Option<u8>,
    /// Commit number (if replicated).
    pub commit_number: Option<u64>,
    /// Current view number (cluster mode only).
    pub view: Option<u64>,
    /// Current leader's replica ID (cluster mode only).
    pub leader_id: Option<u8>,
    /// Number of connected peers (cluster mode only).
    pub connected_peers: Option<usize>,
    /// Whether bootstrap phase is complete (cluster mode only).
    pub bootstrap_complete: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use kmb_types::{DataClass, Placement, StreamId, StreamName};

    #[test]
    fn test_direct_mode_submit() {
        let temp_dir = TempDir::new().unwrap();
        let db = Kimberlite::open(temp_dir.path()).unwrap();
        let submitter = CommandSubmitter::new(&ReplicationMode::None, db, temp_dir.path()).unwrap();

        assert!(!submitter.is_replicated());

        let cmd = Command::create_stream(
            StreamId::new(1),
            StreamName::new("test"),
            DataClass::NonPHI,
            Placement::Global,
        );

        let result = submitter.submit(cmd).unwrap();
        assert!(!result.was_duplicate);
        assert!(result.effects_applied);
    }

    #[test]
    fn test_single_node_mode_submit() {
        let temp_dir = TempDir::new().unwrap();
        let db = Kimberlite::open(temp_dir.path()).unwrap();
        let submitter =
            CommandSubmitter::new(&ReplicationMode::single_node(), db, temp_dir.path()).unwrap();

        assert!(submitter.is_replicated());

        let cmd = Command::create_stream(
            StreamId::new(1),
            StreamName::new("test"),
            DataClass::NonPHI,
            Placement::Global,
        );

        let result = submitter.submit(cmd).unwrap();
        assert!(!result.was_duplicate);
        assert!(result.effects_applied);

        // Check status
        let status = submitter.status();
        assert_eq!(status.mode, "single-node");
        assert!(status.is_leader);
        assert!(status.replica_id.is_some());
    }

    #[test]
    fn test_idempotency_detection() {
        let temp_dir = TempDir::new().unwrap();
        let db = Kimberlite::open(temp_dir.path()).unwrap();
        let submitter =
            CommandSubmitter::new(&ReplicationMode::single_node(), db, temp_dir.path()).unwrap();

        // Create idempotency ID
        let idem_id = IdempotencyId::generate();

        // First submission
        let cmd = Command::create_stream(
            StreamId::new(1),
            StreamName::new("test"),
            DataClass::NonPHI,
            Placement::Global,
        );
        let result = submitter
            .submit_with_idempotency(cmd.clone(), Some(idem_id))
            .unwrap();
        assert!(!result.was_duplicate);

        // Second submission with same ID should be detected as duplicate
        // Note: The replicator should detect this, but we need different commands
        // for the same idempotency_id to trigger duplicate detection.
        // For now, this test verifies the plumbing works.
    }
}
