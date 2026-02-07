//! Repair budget management for preventing repair storms.
//!
//! This module implements rate limiting for repair requests to prevent
//! repair storms that can overwhelm the cluster's send queues.
//!
//! # Problem: Repair Storms
//!
//! Without rate limiting, a lagging replica can flood the cluster with
//! repair requests. `TigerBeetle`'s send queues are sized to only 4 messages,
//! so unbounded repair requests cause cascading failures.
//!
//! # Solution: Budget-Based Repair
//!
//! - Track per-replica latency using EWMA (Exponentially Weighted Moving Average)
//! - Route repairs to fastest replicas first
//! - Limit inflight repairs (max 2 per replica)
//! - Expire stale requests (500ms timeout)
//! - 10% experiment chance to re-test "slow" replicas
//!
//! # `TigerBeetle` Reference
//!
//! Based on `src/vsr/repair_budget.zig` (~500 LOC):
//! - `RepairBudgetJournal` for log repairs
//! - EWMA with alpha = 0.2 for latency smoothing
//! - Experiment chance = 10% to discover recovered replicas

use std::collections::HashMap;
use std::time::Instant;

use rand::Rng;

use crate::types::{OpNumber, ReplicaId};

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of inflight repair requests per replica.
///
/// Limiting to 2 prevents any single replica from being overwhelmed
/// with repair requests, even if it's the fastest.
const MAX_INFLIGHT_PER_REPLICA: usize = 2;

/// Repair request timeout in milliseconds.
///
/// Requests older than this are considered expired and can be retried.
const REPAIR_TIMEOUT_MS: u64 = 500;

/// EWMA smoothing factor (alpha).
///
/// Higher alpha = more weight on recent samples
/// Lower alpha = more smoothing over time
/// `TigerBeetle` uses 0.2 for good balance
const EWMA_ALPHA: f64 = 0.2;

/// Probability of selecting a "slow" replica for experimentation.
///
/// Even if a replica is marked slow, we give it a 10% chance to be
/// selected to discover if it has recovered.
const EXPERIMENT_CHANCE: f64 = 0.1;

// ============================================================================
// Repair Budget
// ============================================================================

/// Tracks repair budget and routes repairs to optimal replicas.
///
/// Uses EWMA to track per-replica latency and preferentially routes
/// repairs to the fastest replicas. Limits inflight requests and
/// expires stale requests to prevent queue overflow.
#[derive(Debug, Clone)]
pub struct RepairBudget {
    /// Per-replica latency tracking.
    replicas: HashMap<ReplicaId, ReplicaLatency>,

    /// Our own replica ID (we don't send repairs to ourselves).
    /// Retained for future use in repair target selection.
    _self_replica_id: ReplicaId,

    /// Total cluster size.
    cluster_size: usize,
}

impl RepairBudget {
    /// Creates a new repair budget for the given cluster.
    ///
    /// # Parameters
    ///
    /// - `self_replica_id`: This replica's ID
    /// - `cluster_size`: Total number of replicas in the cluster
    pub fn new(self_replica_id: ReplicaId, cluster_size: usize) -> Self {
        let mut replicas = HashMap::new();

        // Initialize all replicas (except ourselves)
        for i in 0..cluster_size {
            let replica_id = ReplicaId::new(i as u8);
            if replica_id != self_replica_id {
                replicas.insert(replica_id, ReplicaLatency::new(replica_id));
            }
        }

        Self {
            replicas,
            _self_replica_id: self_replica_id,
            cluster_size,
        }
    }

    /// Selects the best replica to send a repair request to.
    ///
    /// Uses EWMA latency to prefer fast replicas, but gives a 10% chance
    /// to "slow" replicas to discover if they've recovered.
    ///
    /// # Returns
    ///
    /// `Some(replica_id)` if a suitable replica is available, `None` if
    /// all replicas are at their inflight limit.
    pub fn select_replica(&self, rng: &mut impl rand::Rng) -> Option<ReplicaId> {
        // Filter replicas that are available (not at inflight limit)
        let mut available: Vec<_> = self
            .replicas
            .values()
            .filter(|r| r.inflight_count < MAX_INFLIGHT_PER_REPLICA)
            .collect();

        if available.is_empty() {
            return None;
        }

        // Sort by EWMA latency (ascending - fastest first)
        available.sort_by(|a, b| {
            a.ewma_latency_ns
                .partial_cmp(&b.ewma_latency_ns)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 10% chance to experiment with a random replica
        if Rng::r#gen::<f64>(rng) < EXPERIMENT_CHANCE {
            let idx = Rng::gen_range(rng, 0..available.len());
            return Some(available[idx].replica_id);
        }

        // Otherwise, select the fastest replica
        Some(available[0].replica_id)
    }

    /// Records that a repair request was sent to a replica.
    ///
    /// Increments the inflight count and records the send time for
    /// timeout tracking.
    ///
    /// # Parameters
    ///
    /// - `replica_id`: Destination replica
    /// - `op_range_start`: Start of repair range
    /// - `op_range_end`: End of repair range
    /// - `send_time`: When the request was sent
    pub fn record_repair_sent(
        &mut self,
        replica_id: ReplicaId,
        op_range_start: OpNumber,
        op_range_end: OpNumber,
        send_time: Instant,
    ) {
        if let Some(replica) = self.replicas.get_mut(&replica_id) {
            // PRODUCTION ASSERTION: Inflight limit (TigerBeetle bug fix)
            // Prevents send queue overflow (TigerBeetle queues = 4 messages)
            assert!(
                replica.inflight_count < MAX_INFLIGHT_PER_REPLICA,
                "inflight {} must be < max {} for replica {}",
                replica.inflight_count,
                MAX_INFLIGHT_PER_REPLICA,
                replica_id.as_u8()
            );

            replica.inflight_count += 1;
            replica.inflight_requests.push(InflightRepair::new(
                op_range_start,
                op_range_end,
                send_time,
            ));

            // PRODUCTION ASSERTION: Inflight count matches request tracking
            assert_eq!(
                replica.inflight_count,
                replica.inflight_requests.len(),
                "inflight count mismatch for replica {}",
                replica_id.as_u8()
            );

            tracing::trace!(
                replica = %replica_id.as_u8(),
                inflight = replica.inflight_count,
                "repair sent"
            );
        }
    }

    /// Records that a repair completed successfully.
    ///
    /// Updates the EWMA latency and releases the inflight slot.
    ///
    /// # Parameters
    ///
    /// - `replica_id`: Replica that responded
    /// - `op_range_start`: Start of repair range
    /// - `op_range_end`: End of repair range
    /// - `receive_time`: When the response was received
    pub fn record_repair_completed(
        &mut self,
        replica_id: ReplicaId,
        op_range_start: OpNumber,
        op_range_end: OpNumber,
        receive_time: Instant,
    ) {
        let Some(replica) = self.replicas.get_mut(&replica_id) else {
            return;
        };

        // Find the matching inflight request
        let pos = replica.inflight_requests.iter().position(|req| {
            req.op_range_start == op_range_start && req.op_range_end == op_range_end
        });

        if let Some(idx) = pos {
            let request = replica.inflight_requests.remove(idx);
            let latency_ns = receive_time.duration_since(request.send_time).as_nanos() as u64;

            // Update EWMA
            replica.update_ewma(latency_ns);

            // Decrement inflight count
            replica.inflight_count = replica.inflight_count.saturating_sub(1);

            tracing::trace!(
                replica = %replica_id.as_u8(),
                latency_ms = latency_ns / 1_000_000,
                ewma_ms = replica.ewma_latency_ns / 1_000_000,
                inflight = replica.inflight_count,
                "repair completed"
            );
        }
    }

    /// Records that a repair request expired (timeout).
    ///
    /// Releases the inflight slot without updating EWMA (we don't have
    /// a latency sample).
    ///
    /// # Parameters
    ///
    /// - `replica_id`: Replica that didn't respond
    /// - `op_range_start`: Start of repair range
    /// - `op_range_end`: End of repair range
    pub fn record_repair_expired(
        &mut self,
        replica_id: ReplicaId,
        op_range_start: OpNumber,
        op_range_end: OpNumber,
    ) {
        let Some(replica) = self.replicas.get_mut(&replica_id) else {
            return;
        };

        // Find and remove the expired request
        let pos = replica.inflight_requests.iter().position(|req| {
            req.op_range_start == op_range_start && req.op_range_end == op_range_end
        });

        if let Some(idx) = pos {
            replica.inflight_requests.remove(idx);
            replica.inflight_count = replica.inflight_count.saturating_sub(1);

            // Penalize EWMA for timeout (assume 2x current EWMA as latency)
            let penalty_latency = replica.ewma_latency_ns * 2;
            replica.update_ewma(penalty_latency);

            tracing::debug!(
                replica = %replica_id.as_u8(),
                inflight = replica.inflight_count,
                "repair expired"
            );
        }
    }

    /// Checks for expired repair requests and releases their slots.
    ///
    /// Should be called periodically (e.g., on timeout events).
    ///
    /// # Returns
    ///
    /// List of (`replica_id`, `op_range_start`, `op_range_end`) for expired repairs.
    pub fn expire_stale_requests(&mut self, now: Instant) -> Vec<(ReplicaId, OpNumber, OpNumber)> {
        let mut expired = Vec::new();

        for replica in self.replicas.values_mut() {
            // Find expired requests
            let mut i = 0;
            while i < replica.inflight_requests.len() {
                let elapsed_ms = now
                    .duration_since(replica.inflight_requests[i].send_time)
                    .as_millis() as u64;

                if elapsed_ms >= REPAIR_TIMEOUT_MS {
                    let request = replica.inflight_requests.remove(i);
                    replica.inflight_count = replica.inflight_count.saturating_sub(1);

                    // Penalize EWMA
                    let penalty_latency = replica.ewma_latency_ns * 2;
                    replica.update_ewma(penalty_latency);

                    expired.push((
                        replica.replica_id,
                        request.op_range_start,
                        request.op_range_end,
                    ));

                    tracing::debug!(
                        replica = %replica.replica_id.as_u8(),
                        elapsed_ms,
                        "repair expired by timeout"
                    );
                } else {
                    i += 1;
                }
            }

            // PRODUCTION ASSERTION: Stale request removal verification
            // After expiry pass, no requests should remain that exceed the timeout
            // This prevents resource leaks from stuck requests
            for request in &replica.inflight_requests {
                let elapsed_ms = now.duration_since(request.send_time).as_millis() as u64;
                assert!(
                    elapsed_ms < REPAIR_TIMEOUT_MS,
                    "stale request not removed: replica {} has request with {}ms age (timeout {}ms)",
                    replica.replica_id.as_u8(),
                    elapsed_ms,
                    REPAIR_TIMEOUT_MS
                );
            }
        }

        expired
    }

    /// Returns the number of available repair slots across all replicas.
    ///
    /// This is the total capacity minus inflight requests.
    pub fn available_slots(&self) -> usize {
        let max_total = (self.cluster_size - 1) * MAX_INFLIGHT_PER_REPLICA;
        let current_inflight: usize = self.replicas.values().map(|r| r.inflight_count).sum();
        max_total.saturating_sub(current_inflight)
    }

    /// Returns true if there are available repair slots.
    pub fn has_available_slots(&self) -> bool {
        self.available_slots() > 0
    }

    /// Returns the EWMA latency for a specific replica (for testing/debugging).
    pub fn replica_latency(&self, replica_id: ReplicaId) -> Option<u64> {
        self.replicas.get(&replica_id).map(|r| r.ewma_latency_ns)
    }

    /// Returns the inflight count for a specific replica (for testing/debugging).
    pub fn replica_inflight(&self, replica_id: ReplicaId) -> Option<usize> {
        self.replicas.get(&replica_id).map(|r| r.inflight_count)
    }
}

// ============================================================================
// Per-Replica Latency Tracking
// ============================================================================

/// Tracks latency and inflight requests for a single replica.
#[derive(Debug, Clone)]
struct ReplicaLatency {
    /// Replica ID.
    replica_id: ReplicaId,

    /// EWMA latency in nanoseconds.
    ///
    /// Starts at 1ms (conservative default) and adapts over time.
    ewma_latency_ns: u64,

    /// Number of inflight repair requests to this replica.
    inflight_count: usize,

    /// Active repair requests with send times.
    inflight_requests: Vec<InflightRepair>,
}

impl ReplicaLatency {
    /// Creates a new latency tracker for a replica.
    fn new(replica_id: ReplicaId) -> Self {
        Self {
            replica_id,
            ewma_latency_ns: 1_000_000, // Start at 1ms
            inflight_count: 0,
            inflight_requests: Vec::new(),
        }
    }

    /// Updates the EWMA with a new latency sample.
    ///
    /// Formula: EWMA = alpha * `new_sample` + (1 - alpha) * `old_ewma`
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
    fn update_ewma(&mut self, latency_ns: u64) {
        let new_ewma =
            (EWMA_ALPHA * latency_ns as f64) + ((1.0 - EWMA_ALPHA) * self.ewma_latency_ns as f64);
        self.ewma_latency_ns = new_ewma as u64;

        // PRODUCTION ASSERTION: EWMA reasonable bounds
        // Ensures latency values stay within 0-10s range (prevents overflow/underflow)
        // - Lower bound: EWMA must always be positive (prevents division by zero)
        // - Upper bound: 10s is unreasonable for intra-cluster RPC (indicates failure)
        assert!(
            self.ewma_latency_ns > 0 && self.ewma_latency_ns < 10_000_000_000,
            "EWMA latency {} ns must be in range (0, 10s) for replica {}",
            self.ewma_latency_ns,
            self.replica_id.as_u8()
        );
    }
}

// ============================================================================
// Inflight Repair Tracking
// ============================================================================

/// Tracks a single inflight repair request.
#[derive(Debug, Clone)]
struct InflightRepair {
    /// Start of operation range (inclusive).
    op_range_start: OpNumber,

    /// End of operation range (exclusive).
    op_range_end: OpNumber,

    /// When the request was sent.
    send_time: Instant,
}

impl InflightRepair {
    fn new(op_range_start: OpNumber, op_range_end: OpNumber, send_time: Instant) -> Self {
        Self {
            op_range_start,
            op_range_end,
            send_time,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng as RandRng;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    fn test_rng() -> ChaCha8Rng {
        ChaCha8Rng::seed_from_u64(42)
    }

    #[test]
    fn budget_creation() {
        let budget = RepairBudget::new(ReplicaId::new(0), 3);

        // Should have entries for replicas 1 and 2 (not ourselves)
        assert_eq!(budget.replicas.len(), 2);
        assert!(budget.replicas.contains_key(&ReplicaId::new(1)));
        assert!(budget.replicas.contains_key(&ReplicaId::new(2)));
        assert!(!budget.replicas.contains_key(&ReplicaId::new(0)));
    }

    #[test]
    fn select_replica_prefers_fast() {
        let mut budget = RepairBudget::new(ReplicaId::new(0), 3);
        let mut rng = test_rng();

        // Update EWMA: replica 1 is faster (100µs vs 1000µs)
        budget
            .replicas
            .get_mut(&ReplicaId::new(1))
            .unwrap()
            .ewma_latency_ns = 100_000; // 100µs

        budget
            .replicas
            .get_mut(&ReplicaId::new(2))
            .unwrap()
            .ewma_latency_ns = 1_000_000; // 1ms

        // Should mostly select replica 1 (except for 10% experiment chance)
        let mut selected_1 = 0;
        let mut selected_2 = 0;

        for _ in 0..100 {
            match budget.select_replica(&mut rng) {
                Some(rid) if rid == ReplicaId::new(1) => selected_1 += 1,
                Some(rid) if rid == ReplicaId::new(2) => selected_2 += 1,
                _ => {}
            }
        }

        // Replica 1 should be selected much more often
        assert!(selected_1 > selected_2);
        assert!(selected_1 >= 80); // At least 80% (accounting for experiment chance)
    }

    #[test]
    fn inflight_limit_enforced() {
        let mut budget = RepairBudget::new(ReplicaId::new(0), 3);
        let now = Instant::now();

        // Send 2 repairs to replica 1 (max)
        budget.record_repair_sent(ReplicaId::new(1), OpNumber::new(1), OpNumber::new(5), now);
        budget.record_repair_sent(ReplicaId::new(1), OpNumber::new(5), OpNumber::new(10), now);

        assert_eq!(budget.replica_inflight(ReplicaId::new(1)), Some(2));

        // Replica 1 should not be selectable now
        let mut rng = test_rng();
        for _ in 0..50 {
            let selected = budget.select_replica(&mut rng);
            assert_ne!(selected, Some(ReplicaId::new(1)));
        }
    }

    #[test]
    fn ewma_updates_on_completion() {
        let mut budget = RepairBudget::new(ReplicaId::new(0), 3);
        let send_time = Instant::now();

        // Initial EWMA is 1ms
        let initial_ewma = budget.replica_latency(ReplicaId::new(1)).unwrap();
        assert_eq!(initial_ewma, 1_000_000);

        // Send repair
        budget.record_repair_sent(
            ReplicaId::new(1),
            OpNumber::new(1),
            OpNumber::new(5),
            send_time,
        );

        // Complete with 500µs latency
        let receive_time = send_time + std::time::Duration::from_micros(500);
        budget.record_repair_completed(
            ReplicaId::new(1),
            OpNumber::new(1),
            OpNumber::new(5),
            receive_time,
        );

        // EWMA should decrease (faster than initial)
        let new_ewma = budget.replica_latency(ReplicaId::new(1)).unwrap();
        assert!(new_ewma < initial_ewma);
        assert_eq!(budget.replica_inflight(ReplicaId::new(1)), Some(0));
    }

    #[test]
    fn expired_requests_released() {
        let mut budget = RepairBudget::new(ReplicaId::new(0), 3);
        let send_time = Instant::now().checked_sub(std::time::Duration::from_millis(600)).unwrap(); // 600ms ago

        // Send repair
        budget.record_repair_sent(
            ReplicaId::new(1),
            OpNumber::new(1),
            OpNumber::new(5),
            send_time,
        );
        assert_eq!(budget.replica_inflight(ReplicaId::new(1)), Some(1));

        // Expire stale requests
        let expired = budget.expire_stale_requests(Instant::now());

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, ReplicaId::new(1));
        assert_eq!(budget.replica_inflight(ReplicaId::new(1)), Some(0));
    }

    #[test]
    fn available_slots_calculated_correctly() {
        let mut budget = RepairBudget::new(ReplicaId::new(0), 3);
        let now = Instant::now();

        // Initially, all slots available: 2 replicas * 2 max = 4
        assert_eq!(budget.available_slots(), 4);
        assert!(budget.has_available_slots());

        // Send 1 repair
        budget.record_repair_sent(ReplicaId::new(1), OpNumber::new(1), OpNumber::new(5), now);
        assert_eq!(budget.available_slots(), 3);

        // Send 3 more repairs (fill all slots)
        budget.record_repair_sent(ReplicaId::new(1), OpNumber::new(5), OpNumber::new(10), now);
        budget.record_repair_sent(ReplicaId::new(2), OpNumber::new(1), OpNumber::new(5), now);
        budget.record_repair_sent(ReplicaId::new(2), OpNumber::new(5), OpNumber::new(10), now);

        assert_eq!(budget.available_slots(), 0);
        assert!(!budget.has_available_slots());
    }

    #[test]
    fn timeout_penalty_increases_ewma() {
        let mut budget = RepairBudget::new(ReplicaId::new(0), 3);
        let send_time = Instant::now();

        let initial_ewma = budget.replica_latency(ReplicaId::new(1)).unwrap();

        // Send repair
        budget.record_repair_sent(
            ReplicaId::new(1),
            OpNumber::new(1),
            OpNumber::new(5),
            send_time,
        );

        // Expire (timeout penalty = 2x current EWMA)
        budget.record_repair_expired(ReplicaId::new(1), OpNumber::new(1), OpNumber::new(5));

        let new_ewma = budget.replica_latency(ReplicaId::new(1)).unwrap();

        // EWMA should increase (penalty for timeout)
        assert!(new_ewma > initial_ewma);
        assert_eq!(budget.replica_inflight(ReplicaId::new(1)), Some(0));
    }

    // ========================================================================
    // Property-Based Tests (Phase 2)
    // ========================================================================

    use proptest::prelude::*;

    proptest! {
        /// Property: Budget never exceeds MAX_INFLIGHT_PER_REPLICA per replica.
        #[test]
        fn prop_budget_respects_inflight_limit(
            cluster_size in 2_usize..10,
            repair_count in 0_usize..50,
        ) {
            let mut budget = RepairBudget::new(ReplicaId::new(0), cluster_size);
            let now = Instant::now();
            let mut rng = test_rng();
            let mut sent_count = 0;

            // Send random repairs using select_replica (which respects the budget)
            for i in 0..repair_count {
                if budget.has_available_slots() {
                    if let Some(replica) = budget.select_replica(&mut rng) {
                        let start_op = OpNumber::new(i as u64);
                        let end_op = OpNumber::new((i + 1) as u64);
                        budget.record_repair_sent(replica, start_op, end_op, now);
                        sent_count += 1;
                    }
                }
            }

            // Verify no replica exceeds MAX_INFLIGHT_PER_REPLICA
            for replica_id in 1..cluster_size {
                let replica = ReplicaId::new(replica_id as u8);
                if let Some(inflight) = budget.replica_inflight(replica) {
                    prop_assert!(inflight <= MAX_INFLIGHT_PER_REPLICA,
                        "replica {} has {} inflight (max {})", replica_id, inflight, MAX_INFLIGHT_PER_REPLICA);
                }
            }

            // Total inflight should not exceed total sent
            let total_inflight: usize = (1..cluster_size)
                .filter_map(|i| budget.replica_inflight(ReplicaId::new(i as u8)))
                .sum();
            prop_assert!(total_inflight <= sent_count);
        }

        /// Property: EWMA latency is always positive.
        #[test]
        fn prop_ewma_latency_always_positive(
            cluster_size in 2_usize..10,
            operation_count in 0_usize..100,
        ) {
            let mut budget = RepairBudget::new(ReplicaId::new(0), cluster_size);
            let mut rng = test_rng();
            let base_time = Instant::now();

            // Perform random operations
            for i in 0..operation_count {
                let replica = ReplicaId::new(1 + (RandRng::r#gen::<u8>(&mut rng) % (cluster_size - 1) as u8));
                let start_op = OpNumber::new(i as u64);
                let end_op = OpNumber::new((i + 1) as u64);
                let send_time = base_time + std::time::Duration::from_millis(i as u64);

                if budget.has_available_slots() {
                    budget.record_repair_sent(replica, start_op, end_op, send_time);

                    // Randomly complete or expire
                    if RandRng::r#gen::<bool>(&mut rng) {
                        let receive_time = send_time + std::time::Duration::from_millis(RandRng::r#gen::<u64>(&mut rng) % 100);
                        budget.record_repair_completed(replica, start_op, end_op, receive_time);
                    } else {
                        budget.record_repair_expired(replica, start_op, end_op);
                    }
                }
            }

            // Verify all EWMAs are positive
            for replica_id in 1..cluster_size {
                let replica = ReplicaId::new(replica_id as u8);
                if let Some(latency) = budget.replica_latency(replica) {
                    prop_assert!(latency > 0, "EWMA latency must be positive");
                }
            }
        }

        /// Property: Replica selection always returns available replica or None.
        #[test]
        fn prop_replica_selection_valid(
            cluster_size in 2_usize..10,
            selection_count in 1_usize..50,
        ) {
            let budget = RepairBudget::new(ReplicaId::new(0), cluster_size);
            let mut rng = test_rng();

            for _ in 0..selection_count {
                if let Some(selected) = budget.select_replica(&mut rng) {
                    // Selected replica must be in range
                    prop_assert!(selected.as_u8() > 0);
                    prop_assert!((selected.as_u8() as usize) < cluster_size);
                    // Selected replica must not be self
                    prop_assert_ne!(selected, ReplicaId::new(0));
                }
            }
        }

        /// Property: Available slots never exceeds theoretical maximum.
        #[test]
        fn prop_available_slots_bounded(
            cluster_size in 2_usize..10,
        ) {
            let budget = RepairBudget::new(ReplicaId::new(0), cluster_size);

            let max_slots = (cluster_size - 1) * MAX_INFLIGHT_PER_REPLICA;
            prop_assert!(budget.available_slots() <= max_slots);
            let _ = test_rng(); // Suppress unused warning
        }

        /// Property: Expiry correctly removes stale requests.
        #[test]
        fn prop_expiry_removes_stale_requests(
            cluster_size in 2_usize..10,
            repair_count in 1_usize..20,
        ) {
            let mut budget = RepairBudget::new(ReplicaId::new(0), cluster_size);
            let _rng = test_rng();
            let base_time = Instant::now();
            let max_to_send = repair_count.min(budget.available_slots());

            // Send repairs with staggered send times
            for i in 0..max_to_send {
                let replica = ReplicaId::new(1 + (i % (cluster_size - 1)) as u8);
                let start_op = OpNumber::new(i as u64);
                let end_op = OpNumber::new((i + 1) as u64);
                let send_time = base_time + std::time::Duration::from_millis((i * 100) as u64);

                budget.record_repair_sent(replica, start_op, end_op, send_time);
            }

            let inflight_before: usize = (1..cluster_size)
                .map(|i| budget.replica_inflight(ReplicaId::new(i as u8)).unwrap_or(0))
                .sum();

            // Expire stale requests (>500ms old)
            let now = base_time + std::time::Duration::from_millis(600);
            let expired = budget.expire_stale_requests(now);

            // At least the first request should have expired (sent at t=0)
            if max_to_send > 0 {
                prop_assert!(!expired.is_empty(), "expected some requests to expire");
            }

            // Inflight count should have decreased
            let inflight_after: usize = (1..cluster_size)
                .map(|i| budget.replica_inflight(ReplicaId::new(i as u8)).unwrap_or(0))
                .sum();

            // After expiry, inflight should be less than before
            if max_to_send > 0 {
                prop_assert!(inflight_after < inflight_before,
                    "inflight should decrease after expiry (before: {}, after: {}, expired: {})",
                    inflight_before, inflight_after, expired.len());
            }
        }
    }
}
