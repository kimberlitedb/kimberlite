//! vdb-vsr: Viewstamped Replication for VerityDB
//!
//! This crate implements the VSR consensus protocol for quorum-based
//! high availability. Commands are proposed to VSR groups and committed
//! once a quorum of replicas has acknowledged.
//!
//! Initial implementation: SingleNodeGroupReplicator (dev mode)
//! Future: Full VSR with prepare/commit phases, view changes, snapshotting

// TODO: Implement GroupReplicator trait and implementations
