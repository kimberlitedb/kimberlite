//! Standby replica mode for disaster recovery and read scaling.
//!
//! Standby replicas are read-only followers that:
//! - Receive and apply committed operations
//! - Do NOT participate in quorum calculations
//! - Can be promoted to active replicas
//! - Useful for disaster recovery, geographic distribution, read scaling
//!
//! # State Transitions
//!
//! ```text
//! ┌─────────┐  activate()   ┌────────┐
//! │ Standby │ ─────────────> │ Active │
//! └─────────┘               └────────┘
//!      ^                        │
//!      │        deactivate()    │
//!      └────────────────────────┘
//! ```
//!
//! # Protocol
//!
//! 1. **Standby Mode**: Replica follows the log via state transfer
//! 2. **Health Monitoring**: Leader tracks standby health via heartbeats
//! 3. **Promotion**: Standby can be promoted to active (joins quorum)
//! 4. **Demotion**: Active replica can be demoted to standby (leaves quorum)
//!
//! # Example
//!
//! ```rust,ignore
//! // Create standby replica
//! let standby = StandbyState::new(replica_id);
//!
//! // Apply committed operations (read-only)
//! standby.apply_commit(op_number, entry);
//!
//! // Promote to active replica
//! let active_replica = standby.activate();
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::{CommitNumber, LogEntry, OpNumber, ReplicaId};

// ============================================================================
// Standby State
// ============================================================================

/// State for a standby (read-only) replica.
///
/// Standby replicas follow the cluster but don't participate in consensus.
/// They can be promoted to active replicas for disaster recovery or scaling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandbyState {
    /// This standby's replica ID.
    pub replica_id: ReplicaId,

    /// Last committed operation number applied.
    pub commit_number: CommitNumber,

    /// Log of applied operations.
    ///
    /// Standby maintains its own log for read queries.
    pub log: Vec<LogEntry>,

    /// Health status: true if receiving heartbeats, false if lagging.
    pub is_healthy: bool,

    /// Number of consecutive heartbeats missed.
    pub missed_heartbeats: u64,

    /// Last heartbeat timestamp (monotonic nanoseconds).
    pub last_heartbeat_ns: u128,
}

impl StandbyState {
    /// Creates a new standby replica.
    pub fn new(replica_id: ReplicaId) -> Self {
        Self {
            replica_id,
            commit_number: CommitNumber::ZERO,
            log: Vec::new(),
            is_healthy: true,
            missed_heartbeats: 0,
            last_heartbeat_ns: 0,
        }
    }

    /// Applies a committed operation to the standby log.
    ///
    /// Returns true if the operation was applied, false if it was a duplicate.
    pub fn apply_commit(&mut self, op_number: OpNumber, entry: LogEntry) -> bool {
        assert_eq!(
            entry.op_number, op_number,
            "entry op_number must match parameter"
        );

        // Skip if already applied
        if op_number <= self.commit_number.as_op_number() {
            return false;
        }

        // Ensure operations are applied in order
        let expected_op = self.commit_number.as_op_number().next();
        if op_number != expected_op {
            tracing::warn!(
                replica = %self.replica_id,
                expected = %expected_op,
                actual = %op_number,
                "standby: out-of-order operation, skipping"
            );
            return false;
        }

        // Apply operation
        self.log.push(entry);
        self.commit_number = CommitNumber::new(op_number);

        true
    }

    /// Records a heartbeat from the leader.
    ///
    /// Resets missed heartbeat counter and updates health status.
    pub fn record_heartbeat(&mut self, timestamp_ns: u128) {
        self.last_heartbeat_ns = timestamp_ns;
        self.missed_heartbeats = 0;
        self.is_healthy = true;
    }

    /// Records a missed heartbeat.
    ///
    /// If too many heartbeats are missed, marks standby as unhealthy.
    pub fn record_missed_heartbeat(&mut self) {
        self.missed_heartbeats += 1;

        // Mark unhealthy after 3 consecutive misses
        if self.missed_heartbeats >= 3 {
            self.is_healthy = false;
        }
    }

    /// Checks if this standby can be promoted to active.
    ///
    /// Returns true if the standby is healthy and caught up.
    pub fn can_promote(&self, cluster_commit: CommitNumber) -> bool {
        self.is_healthy && self.commit_number >= cluster_commit
    }

    /// Returns the lag (in operations) behind the cluster.
    pub fn lag(&self, cluster_commit: CommitNumber) -> u64 {
        if cluster_commit.as_u64() > self.commit_number.as_u64() {
            cluster_commit.as_u64() - self.commit_number.as_u64()
        } else {
            0
        }
    }
}

// ============================================================================
// Standby Manager
// ============================================================================

/// Manages multiple standby replicas for a cluster.
///
/// The leader uses this to track standby health and coordinate promotions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandbyManager {
    /// Map of standby replica ID to state.
    standbys: HashMap<ReplicaId, StandbyState>,

    /// Heartbeat timeout in nanoseconds (3 seconds default).
    heartbeat_timeout_ns: u128,
}

impl StandbyManager {
    /// Creates a new standby manager.
    pub fn new() -> Self {
        Self {
            standbys: HashMap::new(),
            heartbeat_timeout_ns: 3_000_000_000, // 3 seconds
        }
    }

    /// Registers a new standby replica.
    pub fn register_standby(&mut self, replica_id: ReplicaId) {
        self.standbys
            .insert(replica_id, StandbyState::new(replica_id));
    }

    /// Unregisters a standby replica.
    pub fn unregister_standby(&mut self, replica_id: ReplicaId) -> Option<StandbyState> {
        self.standbys.remove(&replica_id)
    }

    /// Records a heartbeat from a standby.
    pub fn record_heartbeat(&mut self, replica_id: ReplicaId, timestamp_ns: u128) {
        if let Some(standby) = self.standbys.get_mut(&replica_id) {
            standby.record_heartbeat(timestamp_ns);
        }
    }

    /// Checks for timed-out standbys and marks them unhealthy.
    pub fn check_timeouts(&mut self, current_time_ns: u128) {
        for standby in self.standbys.values_mut() {
            let elapsed = current_time_ns.saturating_sub(standby.last_heartbeat_ns);
            if elapsed > self.heartbeat_timeout_ns {
                // Immediately mark as unhealthy if timeout exceeded
                standby.is_healthy = false;
                standby.missed_heartbeats += 1;
            }
        }
    }

    /// Returns all healthy standbys.
    pub fn healthy_standbys(&self) -> Vec<ReplicaId> {
        self.standbys
            .iter()
            .filter(|(_, state)| state.is_healthy)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Returns all unhealthy standbys.
    pub fn unhealthy_standbys(&self) -> Vec<ReplicaId> {
        self.standbys
            .iter()
            .filter(|(_, state)| !state.is_healthy)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Returns standbys that can be promoted to active.
    ///
    /// Standbys must be healthy and caught up with cluster commit.
    pub fn promotable_standbys(&self, cluster_commit: CommitNumber) -> Vec<ReplicaId> {
        self.standbys
            .iter()
            .filter(|(_, state)| state.can_promote(cluster_commit))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Returns the state of a specific standby.
    pub fn get_standby(&self, replica_id: ReplicaId) -> Option<&StandbyState> {
        self.standbys.get(&replica_id)
    }

    /// Returns the number of registered standbys.
    pub fn standby_count(&self) -> usize {
        self.standbys.len()
    }

    /// Returns mutable access to a specific standby (for testing).
    #[cfg(test)]
    pub fn get_standby_mut(&mut self, replica_id: ReplicaId) -> Option<&mut StandbyState> {
        self.standbys.get_mut(&replica_id)
    }

    /// Returns statistics about standby health.
    pub fn health_stats(&self) -> StandbyHealthStats {
        let total = self.standbys.len();
        let healthy = self.standbys.values().filter(|s| s.is_healthy).count();
        let unhealthy = total - healthy;

        StandbyHealthStats {
            total,
            healthy,
            unhealthy,
        }
    }
}

impl Default for StandbyManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Health Statistics
// ============================================================================

/// Statistics about standby replica health.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandbyHealthStats {
    /// Total number of standbys.
    pub total: usize,

    /// Number of healthy standbys.
    pub healthy: usize,

    /// Number of unhealthy standbys.
    pub unhealthy: usize,
}

impl StandbyHealthStats {
    /// Returns the health percentage (0.0 to 1.0).
    #[allow(clippy::cast_precision_loss)]
    pub fn health_percentage(&self) -> f64 {
        if self.total == 0 {
            1.0
        } else {
            self.healthy as f64 / self.total as f64
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_kernel::Command;
    use kimberlite_types::{DataClass, Placement};

    fn test_entry(op: u64, view: u64) -> LogEntry {
        LogEntry::new(
            OpNumber::new(op),
            crate::ViewNumber::new(view),
            Command::create_stream_with_auto_id(
                "test".into(),
                DataClass::Public,
                Placement::Global,
            ),
            None,
            None,
            None,
        )
    }

    #[test]
    fn test_standby_apply_commit() {
        let mut standby = StandbyState::new(ReplicaId::new(10));

        // Apply first operation
        let entry1 = test_entry(1, 0);
        assert!(standby.apply_commit(OpNumber::new(1), entry1));
        assert_eq!(standby.commit_number.as_u64(), 1);
        assert_eq!(standby.log.len(), 1);

        // Apply second operation
        let entry2 = test_entry(2, 0);
        assert!(standby.apply_commit(OpNumber::new(2), entry2));
        assert_eq!(standby.commit_number.as_u64(), 2);
        assert_eq!(standby.log.len(), 2);
    }

    #[test]
    fn test_standby_skip_duplicate() {
        let mut standby = StandbyState::new(ReplicaId::new(10));

        let entry = test_entry(1, 0);
        standby.apply_commit(OpNumber::new(1), entry.clone());

        // Try to apply same operation again
        assert!(!standby.apply_commit(OpNumber::new(1), entry));
        assert_eq!(standby.commit_number.as_u64(), 1);
        assert_eq!(standby.log.len(), 1);
    }

    #[test]
    fn test_standby_out_of_order() {
        let mut standby = StandbyState::new(ReplicaId::new(10));

        // Try to apply op 2 before op 1
        let entry2 = test_entry(2, 0);
        assert!(!standby.apply_commit(OpNumber::new(2), entry2));
        assert_eq!(standby.commit_number.as_u64(), 0);
        assert_eq!(standby.log.len(), 0);
    }

    #[test]
    fn test_standby_health_tracking() {
        let mut standby = StandbyState::new(ReplicaId::new(10));

        assert!(standby.is_healthy);

        // Record missed heartbeats
        standby.record_missed_heartbeat();
        assert!(standby.is_healthy); // Still healthy after 1 miss

        standby.record_missed_heartbeat();
        assert!(standby.is_healthy); // Still healthy after 2 misses

        standby.record_missed_heartbeat();
        assert!(!standby.is_healthy); // Unhealthy after 3 misses

        // Receiving heartbeat restores health
        standby.record_heartbeat(1000);
        assert!(standby.is_healthy);
        assert_eq!(standby.missed_heartbeats, 0);
    }

    #[test]
    fn test_standby_can_promote() {
        let mut standby = StandbyState::new(ReplicaId::new(10));

        // Apply operations
        let entry1 = test_entry(1, 0);
        standby.apply_commit(OpNumber::new(1), entry1);
        assert_eq!(standby.commit_number.as_u64(), 1);

        // Can promote if caught up
        assert!(standby.can_promote(CommitNumber::new(OpNumber::new(1))));

        // Cannot promote if behind
        assert!(!standby.can_promote(CommitNumber::new(OpNumber::new(5))));

        // Cannot promote if unhealthy
        standby.record_missed_heartbeat();
        standby.record_missed_heartbeat();
        standby.record_missed_heartbeat();
        assert!(!standby.is_healthy);
        assert!(!standby.can_promote(CommitNumber::new(OpNumber::new(1))));
    }

    #[test]
    fn test_standby_lag() {
        let mut standby = StandbyState::new(ReplicaId::new(10));

        // Lag when behind
        assert_eq!(standby.lag(CommitNumber::new(OpNumber::new(5))), 5);

        // Apply some operations
        for i in 1..=3 {
            let entry = test_entry(i, 0);
            standby.apply_commit(OpNumber::new(i), entry);
        }

        // Lag reduced
        assert_eq!(standby.lag(CommitNumber::new(OpNumber::new(5))), 2);

        // No lag when caught up
        assert_eq!(standby.lag(CommitNumber::new(OpNumber::new(3))), 0);

        // No lag when ahead (shouldn't happen)
        assert_eq!(standby.lag(CommitNumber::new(OpNumber::new(1))), 0);
    }

    #[test]
    fn test_standby_manager() {
        let mut manager = StandbyManager::new();

        // Register standbys
        manager.register_standby(ReplicaId::new(10));
        manager.register_standby(ReplicaId::new(11));
        assert_eq!(manager.standby_count(), 2);

        // All healthy initially
        assert_eq!(manager.healthy_standbys().len(), 2);
        assert_eq!(manager.unhealthy_standbys().len(), 0);

        // Record heartbeat for one at time 3 seconds
        manager.record_heartbeat(ReplicaId::new(10), 3_000_000_000);

        // Check timeouts (simulate 5 seconds absolute time)
        // Standby 10: elapsed = 5 - 3 = 2 seconds (< 3 second timeout, healthy)
        // Standby 11: elapsed = 5 - 0 = 5 seconds (> 3 second timeout, unhealthy)
        manager.check_timeouts(5_000_000_000);

        // One should still be healthy (received recent heartbeat)
        // Other should be unhealthy (timed out)
        let stats = manager.health_stats();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.healthy, 1);
        assert_eq!(stats.unhealthy, 1);
    }

    #[test]
    fn test_standby_manager_promotable() {
        let mut manager = StandbyManager::new();

        manager.register_standby(ReplicaId::new(10));
        manager.register_standby(ReplicaId::new(11));

        // Apply commits to first standby
        if let Some(standby) = manager.standbys.get_mut(&ReplicaId::new(10)) {
            let entry = test_entry(1, 0);
            standby.apply_commit(OpNumber::new(1), entry);
        }

        // Record heartbeat to keep it healthy
        manager.record_heartbeat(ReplicaId::new(10), 1000);

        // Only first standby is promotable (caught up and healthy)
        let promotable = manager.promotable_standbys(CommitNumber::new(OpNumber::new(1)));
        assert_eq!(promotable.len(), 1);
        assert_eq!(promotable[0], ReplicaId::new(10));
    }

    #[test]
    #[allow(clippy::float_cmp)] // Test code - exact float comparison acceptable
    fn test_health_stats_percentage() {
        let stats = StandbyHealthStats {
            total: 4,
            healthy: 3,
            unhealthy: 1,
        };
        assert_eq!(stats.health_percentage(), 0.75);

        let empty_stats = StandbyHealthStats {
            total: 0,
            healthy: 0,
            unhealthy: 0,
        };
        assert_eq!(empty_stats.health_percentage(), 1.0);
    }
}
