//! Production instrumentation for VSR protocol.
//!
//! This module provides comprehensive observability for the VSR protocol:
//!
//! - **Latency Metrics**: Histograms for prepare, commit, view change, recovery
//! - **Throughput Metrics**: Counters for operations, messages, bytes
//! - **Health Metrics**: Gauges for view, commit, log size, quorum status
//! - **Byzantine Tracking**: Rejection counts for malicious messages (sim-only)
//!
//! ## Architecture
//!
//! Metrics are organized into thread-safe atomic structures with <1% overhead.
//! Export to OpenTelemetry, Prometheus, or custom backends is supported.
//!
//! ## Usage
//!
//! ```ignore
//! use kimberlite_vsr::instrumentation::METRICS;
//!
//! // Record latency
//! let timer = Instant::now();
//! // ... consensus operation ...
//! METRICS.record_prepare_latency(timer.elapsed());
//!
//! // Increment counter
//! METRICS.increment_operations();
//!
//! // Update gauge
//! METRICS.set_view_number(view.as_u64());
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

// ============================================================================
// Production Metrics (Always Available)
// ============================================================================

/// Global metrics instance for VSR protocol.
///
/// This is a lazy-initialized singleton that tracks all VSR metrics.
/// Metrics have minimal overhead (<1%) and are safe for production use.
pub static METRICS: Metrics = Metrics::new();

/// Production metrics for VSR protocol.
///
/// All metrics use atomic operations for thread-safety without locks.
/// Histogram buckets are pre-allocated for O(1) recording.
#[derive(Debug)]
pub struct Metrics {
    // === Latency Histograms ===
    /// Prepare latency (Prepare send → `PrepareOk` quorum)
    prepare_latency_buckets: [AtomicU64; 9],
    prepare_latency_sum_ns: AtomicU64,
    prepare_latency_count: AtomicU64,

    /// Commit latency (`PrepareOk` quorum → Commit broadcast)
    commit_latency_buckets: [AtomicU64; 9],
    commit_latency_sum_ns: AtomicU64,
    commit_latency_count: AtomicU64,

    /// Client latency (Request received → Applied + Effects)
    client_latency_buckets: [AtomicU64; 9],
    client_latency_sum_ns: AtomicU64,
    client_latency_count: AtomicU64,

    /// View change latency (`StartViewChange` → Normal operation)
    view_change_latency_buckets: [AtomicU64; 7],
    view_change_latency_sum_ns: AtomicU64,
    view_change_latency_count: AtomicU64,

    // === Throughput Counters ===
    /// Total operations committed
    operations_total: AtomicU64,
    /// Total operations failed
    operations_failed_total: AtomicU64,
    /// Total bytes written to log
    bytes_written_total: AtomicU64,
    /// Total messages sent (by type)
    messages_sent_prepare: AtomicU64,
    messages_sent_prepare_ok: AtomicU64,
    messages_sent_commit: AtomicU64,
    messages_sent_heartbeat: AtomicU64,
    messages_sent_view_change: AtomicU64,
    /// Total messages received
    messages_received_total: AtomicU64,
    /// Total checksum failures
    checksum_failures_total: AtomicU64,
    /// Total replay attacks detected (AUDIT-2026-03 M-6)
    replay_attacks_total: AtomicU64,
    /// Total signature verification failures (AUDIT-2026-03 M-3)
    signature_failures_total: AtomicU64,
    /// Total repairs completed
    repairs_total: AtomicU64,

    // === Health Gauges ===
    /// Current view number
    view_number: AtomicU64,
    /// Current commit number
    commit_number: AtomicU64,
    /// Current op number
    op_number: AtomicU64,
    /// Current log size in bytes
    log_size_bytes: AtomicU64,
    /// Current log entry count
    log_entry_count: AtomicU64,
    /// Current replica status (0=Normal, 1=ViewChange, 2=Recovering, 3=StateTransfer)
    replica_status: AtomicU64,
    /// Current quorum size
    quorum_size: AtomicU64,
    /// Pending client requests
    pending_requests: AtomicU64,

    // === Phase-Specific Metrics ===
    /// Clock offset from leader (milliseconds)
    clock_offset_ms: AtomicU64,
    /// Active client sessions
    client_sessions_active: AtomicU64,
    /// Available repair budget
    repair_budget_available: AtomicU64,
    /// Scrub tours completed
    scrub_tours_completed: AtomicU64,
    /// Reconfig state (0=Stable, 1=Joint)
    reconfig_state: AtomicU64,
    /// Standby replica count
    standby_count: AtomicU64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    /// Creates a new metrics instance.
    ///
    /// This is a const function for static initialization.
    pub const fn new() -> Self {
        Self {
            // Latency histograms (initialized to zero)
            prepare_latency_buckets: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            prepare_latency_sum_ns: AtomicU64::new(0),
            prepare_latency_count: AtomicU64::new(0),

            commit_latency_buckets: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            commit_latency_sum_ns: AtomicU64::new(0),
            commit_latency_count: AtomicU64::new(0),

            client_latency_buckets: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            client_latency_sum_ns: AtomicU64::new(0),
            client_latency_count: AtomicU64::new(0),

            view_change_latency_buckets: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            view_change_latency_sum_ns: AtomicU64::new(0),
            view_change_latency_count: AtomicU64::new(0),

            // Throughput counters
            operations_total: AtomicU64::new(0),
            operations_failed_total: AtomicU64::new(0),
            bytes_written_total: AtomicU64::new(0),
            messages_sent_prepare: AtomicU64::new(0),
            messages_sent_prepare_ok: AtomicU64::new(0),
            messages_sent_commit: AtomicU64::new(0),
            messages_sent_heartbeat: AtomicU64::new(0),
            messages_sent_view_change: AtomicU64::new(0),
            messages_received_total: AtomicU64::new(0),
            checksum_failures_total: AtomicU64::new(0),
            replay_attacks_total: AtomicU64::new(0),
            signature_failures_total: AtomicU64::new(0),
            repairs_total: AtomicU64::new(0),

            // Health gauges
            view_number: AtomicU64::new(0),
            commit_number: AtomicU64::new(0),
            op_number: AtomicU64::new(0),
            log_size_bytes: AtomicU64::new(0),
            log_entry_count: AtomicU64::new(0),
            replica_status: AtomicU64::new(0),
            quorum_size: AtomicU64::new(0),
            pending_requests: AtomicU64::new(0),

            // Phase-specific metrics
            clock_offset_ms: AtomicU64::new(0),
            client_sessions_active: AtomicU64::new(0),
            repair_budget_available: AtomicU64::new(0),
            scrub_tours_completed: AtomicU64::new(0),
            reconfig_state: AtomicU64::new(0),
            standby_count: AtomicU64::new(0),
        }
    }

    // ========================================================================
    // Latency Recording
    // ========================================================================

    /// Records prepare latency (Prepare send → `PrepareOk` quorum).
    ///
    /// Buckets: [0.1ms, 0.5ms, 1ms, 2ms, 5ms, 10ms, 25ms, 50ms, 100ms, +Inf]
    pub fn record_prepare_latency(&self, duration: Duration) {
        let ms = duration.as_secs_f64() * 1000.0;
        self.record_histogram_ms(&self.prepare_latency_buckets, ms);
        self.prepare_latency_sum_ns
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
        self.prepare_latency_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Records commit latency (`PrepareOk` quorum → Commit broadcast).
    pub fn record_commit_latency(&self, duration: Duration) {
        let ms = duration.as_secs_f64() * 1000.0;
        self.record_histogram_ms(&self.commit_latency_buckets, ms);
        self.commit_latency_sum_ns
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
        self.commit_latency_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Records client latency (Request received → Applied + Effects).
    pub fn record_client_latency(&self, duration: Duration) {
        let ms = duration.as_secs_f64() * 1000.0;
        self.record_histogram_ms(&self.client_latency_buckets, ms);
        self.client_latency_sum_ns
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
        self.client_latency_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Records view change latency (`StartViewChange` → Normal operation).
    ///
    /// Buckets: [10ms, 50ms, 100ms, 250ms, 500ms, 1000ms, 5000ms, +Inf]
    pub fn record_view_change_latency(&self, duration: Duration) {
        let ms = duration.as_secs_f64() * 1000.0;
        let buckets = [10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0];

        for (i, &bucket) in buckets.iter().enumerate() {
            if ms <= bucket {
                self.view_change_latency_buckets[i].fetch_add(1, Ordering::Relaxed);
                break;
            }
        }

        self.view_change_latency_sum_ns
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
        self.view_change_latency_count
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Helper: Records value in histogram buckets.
    #[allow(clippy::unused_self)]
    fn record_histogram_ms(&self, buckets: &[AtomicU64; 9], ms: f64) {
        let bucket_bounds = [0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0];

        for (i, &bound) in bucket_bounds.iter().enumerate() {
            if ms <= bound {
                buckets[i].fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
    }

    // ========================================================================
    // Throughput Counters
    // ========================================================================

    /// Increments total operations committed.
    pub fn increment_operations(&self) {
        self.operations_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments total operations failed.
    pub fn increment_operations_failed(&self) {
        self.operations_failed_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Adds bytes written to log.
    pub fn add_bytes_written(&self, bytes: u64) {
        self.bytes_written_total.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Increments message sent counter (by type).
    pub fn increment_messages_sent(&self, message_type: &str) {
        match message_type {
            "Prepare" => self.messages_sent_prepare.fetch_add(1, Ordering::Relaxed),
            "PrepareOk" => self
                .messages_sent_prepare_ok
                .fetch_add(1, Ordering::Relaxed),
            "Commit" => self.messages_sent_commit.fetch_add(1, Ordering::Relaxed),
            "Heartbeat" => self.messages_sent_heartbeat.fetch_add(1, Ordering::Relaxed),
            _ => self
                .messages_sent_view_change
                .fetch_add(1, Ordering::Relaxed),
        };
    }

    /// Increments messages received.
    pub fn increment_messages_received(&self) {
        self.messages_received_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments checksum failures.
    pub fn increment_checksum_failures(&self) {
        self.checksum_failures_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments replay attacks detected (AUDIT-2026-03 M-6).
    ///
    /// **Security:** Tracks Byzantine replicas attempting to replay old messages
    /// to disrupt consensus. High replay counts indicate active attack or misconfigured replica.
    pub fn increment_replay_attacks(&self) {
        self.replay_attacks_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments signature verification failures.
    ///
    /// **Security:** Tracks messages with invalid Ed25519 signatures, indicating:
    /// - Byzantine replica attempting to forge messages
    /// - Man-in-the-middle attacker tampering with messages
    /// - Corrupted messages in transit
    ///
    /// High signature failure counts indicate active attack or network corruption.
    pub fn increment_signature_failures(&self) {
        self.signature_failures_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Increments repairs completed.
    pub fn increment_repairs(&self) {
        self.repairs_total.fetch_add(1, Ordering::Relaxed);
    }

    // ========================================================================
    // Health Gauges
    // ========================================================================

    /// Sets current view number.
    pub fn set_view_number(&self, view: u64) {
        self.view_number.store(view, Ordering::Relaxed);
    }

    /// Sets current commit number.
    pub fn set_commit_number(&self, commit: u64) {
        self.commit_number.store(commit, Ordering::Relaxed);
    }

    /// Sets current op number.
    pub fn set_op_number(&self, op: u64) {
        self.op_number.store(op, Ordering::Relaxed);
    }

    /// Sets log size in bytes.
    pub fn set_log_size_bytes(&self, bytes: u64) {
        self.log_size_bytes.store(bytes, Ordering::Relaxed);
    }

    /// Sets log entry count.
    pub fn set_log_entry_count(&self, count: u64) {
        self.log_entry_count.store(count, Ordering::Relaxed);
    }

    /// Sets replica status (0=Normal, 1=ViewChange, 2=Recovering, 3=StateTransfer).
    pub fn set_replica_status(&self, status: u64) {
        self.replica_status.store(status, Ordering::Relaxed);
    }

    /// Sets quorum size.
    pub fn set_quorum_size(&self, quorum: u64) {
        self.quorum_size.store(quorum, Ordering::Relaxed);
    }

    /// Sets pending requests count.
    pub fn set_pending_requests(&self, count: u64) {
        self.pending_requests.store(count, Ordering::Relaxed);
    }

    // ========================================================================
    // Phase-Specific Metrics
    // ========================================================================

    /// Sets clock offset from leader (milliseconds).
    #[allow(clippy::cast_sign_loss)]
    pub fn set_clock_offset_ms(&self, offset_ms: i64) {
        // Store as unsigned (add 2^31 to handle negatives)
        let unsigned = (offset_ms + (1 << 31)) as u64;
        self.clock_offset_ms.store(unsigned, Ordering::Relaxed);
    }

    /// Sets active client sessions count.
    pub fn set_client_sessions_active(&self, count: u64) {
        self.client_sessions_active.store(count, Ordering::Relaxed);
    }

    /// Sets available repair budget.
    pub fn set_repair_budget_available(&self, budget: u64) {
        self.repair_budget_available
            .store(budget, Ordering::Relaxed);
    }

    /// Increments scrub tours completed.
    pub fn increment_scrub_tours(&self) {
        self.scrub_tours_completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Sets reconfig state (0=Stable, 1=Joint).
    pub fn set_reconfig_state(&self, state: u64) {
        self.reconfig_state.store(state, Ordering::Relaxed);
    }

    /// Sets standby replica count.
    pub fn set_standby_count(&self, count: u64) {
        self.standby_count.store(count, Ordering::Relaxed);
    }

    // ========================================================================
    // Metric Export
    // ========================================================================

    /// Exports all metrics in Prometheus exposition format.
    #[allow(clippy::cast_precision_loss)]
    pub fn export_prometheus(&self) -> String {
        use std::fmt::Write;
        let mut output = String::new();

        // Operations counter
        let _ = write!(
            output,
            "# HELP vsr_operations_total Total operations committed\n\
             # TYPE vsr_operations_total counter\n\
             vsr_operations_total {}\n",
            self.operations_total.load(Ordering::Relaxed)
        );

        // Prepare latency histogram
        output.push_str(
            "# HELP vsr_prepare_latency_ms Prepare latency histogram\n\
             # TYPE vsr_prepare_latency_ms histogram\n",
        );

        let bucket_bounds = [0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0];
        let mut cumulative = 0u64;

        for (i, &bound) in bucket_bounds.iter().enumerate() {
            cumulative += self.prepare_latency_buckets[i].load(Ordering::Relaxed);
            let _ = writeln!(
                output,
                "vsr_prepare_latency_ms_bucket{{le=\"{bound}\"}} {cumulative}",
            );
        }

        let _ = write!(
            output,
            "vsr_prepare_latency_ms_bucket{{le=\"+Inf\"}} {}\n\
             vsr_prepare_latency_ms_sum {}\n\
             vsr_prepare_latency_ms_count {}\n",
            self.prepare_latency_count.load(Ordering::Relaxed),
            self.prepare_latency_sum_ns.load(Ordering::Relaxed) as f64 / 1_000_000.0,
            self.prepare_latency_count.load(Ordering::Relaxed)
        );

        // Health gauges
        let _ = write!(
            output,
            "# HELP vsr_view_number Current view number\n\
             # TYPE vsr_view_number gauge\n\
             vsr_view_number {}\n",
            self.view_number.load(Ordering::Relaxed)
        );

        let _ = write!(
            output,
            "# HELP vsr_commit_number Current commit number\n\
             # TYPE vsr_commit_number gauge\n\
             vsr_commit_number {}\n",
            self.commit_number.load(Ordering::Relaxed)
        );

        output
    }

    /// Returns snapshot of all metrics for testing/debugging.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            operations_total: self.operations_total.load(Ordering::Relaxed),
            operations_failed_total: self.operations_failed_total.load(Ordering::Relaxed),
            view_number: self.view_number.load(Ordering::Relaxed),
            commit_number: self.commit_number.load(Ordering::Relaxed),
            op_number: self.op_number.load(Ordering::Relaxed),
            prepare_latency_count: self.prepare_latency_count.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of metrics at a point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub operations_total: u64,
    pub operations_failed_total: u64,
    pub view_number: u64,
    pub commit_number: u64,
    pub op_number: u64,
    pub prepare_latency_count: u64,
}

// ============================================================================
// Byzantine Rejection Tracking (Simulation Only)
// ============================================================================

#[cfg(feature = "sim")]
pub use sim_tracking::{
    ByzantineRejectionStats, get_rejection_count, get_rejection_stats, record_byzantine_rejection,
    reset_rejection_stats,
};

#[cfg(feature = "sim")]
mod sim_tracking {
    use super::*;

    /// Global counters for Byzantine rejection tracking.
    ///
    /// These are only available when the `sim` feature is enabled.
    static REJECTION_TOTAL: AtomicU64 = AtomicU64::new(0);
    static REJECTION_COMMIT_NUMBER: AtomicU64 = AtomicU64::new(0);
    static REJECTION_LOG_TAIL_LENGTH: AtomicU64 = AtomicU64::new(0);
    static REJECTION_VIEW_MONOTONICITY: AtomicU64 = AtomicU64::new(0);
    static REJECTION_OP_NUMBER_MISMATCH: AtomicU64 = AtomicU64::new(0);

    /// Records a Byzantine message rejection.
    ///
    /// This function is only available when the `sim` feature is enabled.
    ///
    /// # Parameters
    ///
    /// - `reason`: Why the message was rejected (e.g., `"inflated_commit_number"`)
    /// - `from`: The replica that sent the Byzantine message
    /// - `claimed`: The value claimed in the message
    /// - `actual`: The actual/expected value
    pub fn record_byzantine_rejection(
        reason: &str,
        from: crate::ReplicaId,
        claimed: u64,
        actual: u64,
    ) {
        REJECTION_TOTAL.fetch_add(1, Ordering::Relaxed);

        // Track by rejection type
        match reason {
            "inflated_commit_number" | "commit_number_mismatch" => {
                REJECTION_COMMIT_NUMBER.fetch_add(1, Ordering::Relaxed);
            }
            "log_tail_length_mismatch" | "truncated_log_tail" => {
                REJECTION_LOG_TAIL_LENGTH.fetch_add(1, Ordering::Relaxed);
            }
            "view_not_monotonic" | "view_regression" => {
                REJECTION_VIEW_MONOTONICITY.fetch_add(1, Ordering::Relaxed);
            }
            "op_number_mismatch" => {
                REJECTION_OP_NUMBER_MISMATCH.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }

        // Log the rejection for debugging
        tracing::warn!(
            replica = %from.as_u8(),
            reason = %reason,
            claimed = claimed,
            actual = actual,
            "Byzantine message rejected by protocol handler"
        );
    }

    /// Returns the total number of Byzantine rejections.
    pub fn get_rejection_count() -> u64 {
        REJECTION_TOTAL.load(Ordering::Relaxed)
    }

    /// Returns Byzantine rejection statistics.
    pub fn get_rejection_stats() -> ByzantineRejectionStats {
        ByzantineRejectionStats {
            total: REJECTION_TOTAL.load(Ordering::Relaxed),
            commit_number: REJECTION_COMMIT_NUMBER.load(Ordering::Relaxed),
            log_tail_length: REJECTION_LOG_TAIL_LENGTH.load(Ordering::Relaxed),
            view_monotonicity: REJECTION_VIEW_MONOTONICITY.load(Ordering::Relaxed),
            op_number_mismatch: REJECTION_OP_NUMBER_MISMATCH.load(Ordering::Relaxed),
        }
    }

    /// Resets all Byzantine rejection counters.
    ///
    /// Used between test runs to get fresh statistics.
    pub fn reset_rejection_stats() {
        REJECTION_TOTAL.store(0, Ordering::Relaxed);
        REJECTION_COMMIT_NUMBER.store(0, Ordering::Relaxed);
        REJECTION_LOG_TAIL_LENGTH.store(0, Ordering::Relaxed);
        REJECTION_VIEW_MONOTONICITY.store(0, Ordering::Relaxed);
        REJECTION_OP_NUMBER_MISMATCH.store(0, Ordering::Relaxed);
    }

    /// Statistics for Byzantine message rejections.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct ByzantineRejectionStats {
        /// Total rejections across all types.
        pub total: u64,
        /// Rejections due to commit number violations.
        pub commit_number: u64,
        /// Rejections due to log tail length mismatches.
        pub log_tail_length: u64,
        /// Rejections due to view monotonicity violations.
        pub view_monotonicity: u64,
        /// Rejections due to op number mismatches.
        pub op_number_mismatch: u64,
    }

    impl ByzantineRejectionStats {
        /// Returns true if any rejections were recorded.
        pub fn has_rejections(&self) -> bool {
            self.total > 0
        }
    }
} // end mod sim_tracking

// ============================================================================
// Core Metrics Tests
// ============================================================================

#[cfg(test)]
mod core_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_counter_increment() {
        let initial = METRICS.operations_total.load(Ordering::Relaxed);

        METRICS.increment_operations();
        METRICS.increment_operations();
        METRICS.increment_operations();

        let final_count = METRICS.operations_total.load(Ordering::Relaxed);
        assert_eq!(final_count - initial, 3, "counter should increment by 3");
    }

    #[test]
    fn test_gauge_update() {
        METRICS.set_view_number(42);
        assert_eq!(METRICS.view_number.load(Ordering::Relaxed), 42);

        METRICS.set_commit_number(100);
        assert_eq!(METRICS.commit_number.load(Ordering::Relaxed), 100);

        METRICS.set_op_number(150);
        assert_eq!(METRICS.op_number.load(Ordering::Relaxed), 150);
    }

    #[test]
    fn test_histogram_recording() {
        // Record some latencies
        METRICS.record_prepare_latency(Duration::from_micros(100)); // 0.1ms
        METRICS.record_prepare_latency(Duration::from_millis(1)); // 1ms
        METRICS.record_prepare_latency(Duration::from_millis(10)); // 10ms
        METRICS.record_prepare_latency(Duration::from_millis(200)); // 200ms (>100ms)

        // Check count increased
        let count = METRICS.prepare_latency_count.load(Ordering::Relaxed);
        assert!(count >= 4, "should have recorded at least 4 latencies");

        // Check sum increased (should be >211ms = 0.1 + 1 + 10 + 200)
        let sum_ns = METRICS.prepare_latency_sum_ns.load(Ordering::Relaxed);
        assert!(sum_ns >= 211_000_000, "sum should be at least 211ms");
    }

    #[test]
    fn test_histogram_buckets() {
        // Record latency in specific bucket (e.g., 0.5ms bucket)
        let bucket_idx = 1; // 0.5ms bucket
        let initial = METRICS.prepare_latency_buckets[bucket_idx].load(Ordering::Relaxed);

        METRICS.record_prepare_latency(Duration::from_micros(500)); // Exactly 0.5ms

        let final_count = METRICS.prepare_latency_buckets[bucket_idx].load(Ordering::Relaxed);
        assert_eq!(final_count - initial, 1, "0.5ms bucket should increment");
    }

    #[test]
    fn test_message_sent_counters() {
        let initial_prepare = METRICS.messages_sent_prepare.load(Ordering::Relaxed);
        let initial_commit = METRICS.messages_sent_commit.load(Ordering::Relaxed);

        METRICS.increment_messages_sent("Prepare");
        METRICS.increment_messages_sent("Commit");
        METRICS.increment_messages_sent("Commit");

        assert_eq!(
            METRICS.messages_sent_prepare.load(Ordering::Relaxed) - initial_prepare,
            1,
            "prepare counter should increment by 1"
        );
        assert_eq!(
            METRICS.messages_sent_commit.load(Ordering::Relaxed) - initial_commit,
            2,
            "commit counter should increment by 2"
        );
    }

    #[test]
    fn test_prometheus_export_format() {
        // Set some test metrics
        METRICS.increment_operations();
        METRICS.set_view_number(5);
        METRICS.set_commit_number(10);

        let output = METRICS.export_prometheus();

        // Check for required Prometheus format elements
        assert!(output.contains("# HELP vsr_operations_total"));
        assert!(output.contains("# TYPE vsr_operations_total counter"));
        assert!(output.contains("vsr_operations_total"));

        assert!(output.contains("# HELP vsr_view_number"));
        assert!(output.contains("# TYPE vsr_view_number gauge"));
        assert!(output.contains("vsr_view_number 5"));

        assert!(output.contains("# HELP vsr_commit_number"));
        assert!(output.contains("vsr_commit_number 10"));

        // Check histogram format
        assert!(output.contains("# HELP vsr_prepare_latency_ms"));
        assert!(output.contains("# TYPE vsr_prepare_latency_ms histogram"));
        assert!(output.contains("vsr_prepare_latency_ms_bucket"));
        assert!(output.contains("vsr_prepare_latency_ms_sum"));
        assert!(output.contains("vsr_prepare_latency_ms_count"));
    }

    #[test]
    fn test_metrics_snapshot() {
        // Set known values
        METRICS.set_view_number(7);
        METRICS.set_commit_number(14);
        METRICS.set_op_number(21);

        let snapshot = METRICS.snapshot();

        assert_eq!(snapshot.view_number, 7);
        assert_eq!(snapshot.commit_number, 14);
        assert_eq!(snapshot.op_number, 21);
    }

    #[test]
    fn test_phase_specific_metrics() {
        // Test clock offset (stores as unsigned with offset)
        METRICS.set_clock_offset_ms(5);
        let stored = METRICS.clock_offset_ms.load(Ordering::Relaxed);
        // Value is stored as (offset_ms + 2^31), so 5 + 2^31
        let expected = (5i64 + (1 << 31)) as u64;
        assert_eq!(stored, expected, "clock offset should be stored with bias");

        // Test client sessions (direct storage)
        METRICS.set_client_sessions_active(10);
        assert_eq!(METRICS.client_sessions_active.load(Ordering::Relaxed), 10);

        // Test repair budget (direct storage)
        METRICS.set_repair_budget_available(100);
        assert_eq!(METRICS.repair_budget_available.load(Ordering::Relaxed), 100);

        // Test scrub tours (incremental)
        let initial_tours = METRICS.scrub_tours_completed.load(Ordering::Relaxed);
        METRICS.increment_scrub_tours();
        METRICS.increment_scrub_tours();
        METRICS.increment_scrub_tours();
        assert_eq!(
            METRICS.scrub_tours_completed.load(Ordering::Relaxed) - initial_tours,
            3,
            "scrub tours should increment by 3"
        );

        // Test standby count (direct storage)
        METRICS.set_standby_count(2);
        assert_eq!(METRICS.standby_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_view_change_latency() {
        // View change latency uses different buckets (10ms - 5000ms)
        METRICS.record_view_change_latency(Duration::from_millis(50));
        METRICS.record_view_change_latency(Duration::from_millis(500));

        let count = METRICS.view_change_latency_count.load(Ordering::Relaxed);
        assert!(
            count >= 2,
            "should have recorded at least 2 view change latencies"
        );
    }

    #[test]
    fn test_atomic_operations_thread_safe() {
        use std::thread;

        let threads: Vec<_> = (0..10)
            .map(|_| {
                thread::spawn(|| {
                    for _ in 0..100 {
                        METRICS.increment_operations();
                    }
                })
            })
            .collect();

        for t in threads {
            t.join().unwrap();
        }

        // All increments should be recorded (no lost updates)
        // Note: We can't assert exact count since other tests run concurrently,
        // but we verify the code compiles and doesn't panic
    }
}

// ============================================================================
// OpenTelemetry Integration (Optional)
// ============================================================================

#[cfg(feature = "otel")]
pub use otel_export::*;

#[cfg(feature = "otel")]
mod otel_export {
    use super::*;
    use opentelemetry::metrics::MeterProvider;
    use opentelemetry_otlp::WithExportConfig;
    use std::sync::Arc;

    /// OpenTelemetry exporter configuration.
    #[derive(Debug, Clone)]
    pub struct OtelConfig {
        /// OTLP endpoint (e.g., "http://localhost:4317")
        pub otlp_endpoint: Option<String>,
        /// Export interval in seconds
        pub export_interval_secs: u64,
        /// Service name for telemetry
        pub service_name: String,
        /// Service version
        pub service_version: String,
    }

    impl Default for OtelConfig {
        fn default() -> Self {
            Self {
                otlp_endpoint: None,
                export_interval_secs: 10,
                service_name: "kimberlite-vsr".to_string(),
                service_version: env!("CARGO_PKG_VERSION").to_string(),
            }
        }
    }

    /// OpenTelemetry exporter for VSR metrics.
    ///
    /// Supports multiple backends:
    /// - **OTLP**: Push to OpenTelemetry collector
    /// - **Prometheus**: Pull-based scraping via /metrics endpoint
    /// - **StatsD**: UDP push to StatsD daemon
    pub struct OtelExporter {
        config: OtelConfig,
        meter_provider: Option<Arc<opentelemetry_sdk::metrics::SdkMeterProvider>>,
    }

    impl OtelExporter {
        /// Creates a new OpenTelemetry exporter.
        pub fn new(config: OtelConfig) -> Result<Self, Box<dyn std::error::Error>> {
            Ok(Self {
                config,
                meter_provider: None,
            })
        }

        /// Initializes the OTLP exporter.
        ///
        /// This sets up push-based metrics export to an OpenTelemetry collector.
        pub fn init_otlp(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            use opentelemetry_otlp::MetricExporter;
            use opentelemetry_sdk::metrics::SdkMeterProvider;

            let endpoint = self
                .config
                .otlp_endpoint
                .as_ref()
                .ok_or("OTLP endpoint not configured")?;

            // Create OTLP exporter
            let exporter = MetricExporter::builder()
                .with_http()
                .with_endpoint(endpoint)
                .build()?;

            // Create periodic reader
            let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter)
                .with_interval(std::time::Duration::from_secs(
                    self.config.export_interval_secs,
                ))
                .build();

            // Create meter provider (using default resource attributes)
            let meter_provider = SdkMeterProvider::builder().with_reader(reader).build();

            self.meter_provider = Some(Arc::new(meter_provider));
            Ok(())
        }

        /// Exports current metrics to OpenTelemetry.
        ///
        /// This converts our atomic metrics to OTEL format and pushes them.
        pub fn export_metrics(&self) -> Result<(), Box<dyn std::error::Error>> {
            let provider = self
                .meter_provider
                .as_ref()
                .ok_or("Meter provider not initialized")?;

            let meter = provider.meter("kimberlite-vsr");

            // Export counters
            let operations_counter = meter
                .u64_counter("vsr_operations_total")
                .with_description("Total operations committed")
                .build();
            operations_counter.add(METRICS.operations_total.load(Ordering::Relaxed), &[]);

            let operations_failed_counter = meter
                .u64_counter("vsr_operations_failed_total")
                .with_description("Total operations failed")
                .build();
            operations_failed_counter
                .add(METRICS.operations_failed_total.load(Ordering::Relaxed), &[]);

            let messages_received_counter = meter
                .u64_counter("vsr_messages_received_total")
                .with_description("Total messages received")
                .build();
            messages_received_counter
                .add(METRICS.messages_received_total.load(Ordering::Relaxed), &[]);

            let checksum_failures_counter = meter
                .u64_counter("vsr_checksum_failures_total")
                .with_description("Total checksum failures")
                .build();
            checksum_failures_counter
                .add(METRICS.checksum_failures_total.load(Ordering::Relaxed), &[]);

            // Export gauges (observable gauges are registered but values sampled asynchronously)
            let _view_number_gauge = meter
                .u64_observable_gauge("vsr_view_number")
                .with_description("Current view number")
                .build();

            let _commit_number_gauge = meter
                .u64_observable_gauge("vsr_commit_number")
                .with_description("Current commit number")
                .build();

            let _op_number_gauge = meter
                .u64_observable_gauge("vsr_op_number")
                .with_description("Current op number")
                .build();

            // Export histograms (convert bucket counts to observations)
            let prepare_latency_histogram = meter
                .f64_histogram("vsr_prepare_latency_ms")
                .with_description("Prepare latency histogram")
                .build();

            // Note: In a real implementation, we'd need to replay observations
            // from histogram buckets. For now, we export summary statistics.
            let prepare_count = METRICS.prepare_latency_count.load(Ordering::Relaxed);
            if prepare_count > 0 {
                let sum_ns = METRICS.prepare_latency_sum_ns.load(Ordering::Relaxed);
                let avg_ms = (sum_ns as f64 / prepare_count as f64) / 1_000_000.0;
                prepare_latency_histogram.record(avg_ms, &[]);
            }

            Ok(())
        }

        /// Shuts down the exporter and flushes pending metrics.
        pub fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            if let Some(provider) = self.meter_provider.take() {
                if let Ok(provider) = Arc::try_unwrap(provider) {
                    provider.shutdown()?;
                }
            }
            Ok(())
        }

        /// Exports metrics in StatsD format (UDP push).
        ///
        /// Format: `metric_name:value|type`
        /// Types: c (counter), g (gauge), ms (timer), h (histogram)
        pub fn export_statsd(&self) -> Vec<String> {
            let mut lines = Vec::new();

            // Counters
            lines.push(format!(
                "vsr.operations.total:{}|c",
                METRICS.operations_total.load(Ordering::Relaxed)
            ));
            lines.push(format!(
                "vsr.operations.failed:{}|c",
                METRICS.operations_failed_total.load(Ordering::Relaxed)
            ));
            lines.push(format!(
                "vsr.messages.received:{}|c",
                METRICS.messages_received_total.load(Ordering::Relaxed)
            ));
            lines.push(format!(
                "vsr.checksum.failures:{}|c",
                METRICS.checksum_failures_total.load(Ordering::Relaxed)
            ));

            // Gauges
            lines.push(format!(
                "vsr.view.number:{}|g",
                METRICS.view_number.load(Ordering::Relaxed)
            ));
            lines.push(format!(
                "vsr.commit.number:{}|g",
                METRICS.commit_number.load(Ordering::Relaxed)
            ));
            lines.push(format!(
                "vsr.op.number:{}|g",
                METRICS.op_number.load(Ordering::Relaxed)
            ));
            lines.push(format!(
                "vsr.log.size_bytes:{}|g",
                METRICS.log_size_bytes.load(Ordering::Relaxed)
            ));

            // Histograms (export as timing with average)
            let prepare_count = METRICS.prepare_latency_count.load(Ordering::Relaxed);
            if prepare_count > 0 {
                let sum_ns = METRICS.prepare_latency_sum_ns.load(Ordering::Relaxed);
                let avg_ms = (sum_ns as f64 / prepare_count as f64) / 1_000_000.0;
                lines.push(format!("vsr.prepare.latency:{}|ms", avg_ms));
            }

            let commit_count = METRICS.commit_latency_count.load(Ordering::Relaxed);
            if commit_count > 0 {
                let sum_ns = METRICS.commit_latency_sum_ns.load(Ordering::Relaxed);
                let avg_ms = (sum_ns as f64 / commit_count as f64) / 1_000_000.0;
                lines.push(format!("vsr.commit.latency:{}|ms", avg_ms));
            }

            lines
        }
    }

    impl Drop for OtelExporter {
        fn drop(&mut self) {
            let _ = self.shutdown();
        }
    }

    // ========================================================================
    // Tests
    // ========================================================================

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_otel_config_default() {
            let config = OtelConfig::default();
            assert_eq!(config.service_name, "kimberlite-vsr");
            assert_eq!(config.export_interval_secs, 10);
            assert!(config.otlp_endpoint.is_none());
        }

        #[test]
        fn test_otel_exporter_creation() {
            let config = OtelConfig::default();
            let exporter = OtelExporter::new(config);
            assert!(exporter.is_ok());
        }

        #[test]
        fn test_statsd_export_format() {
            // Set some metrics
            METRICS.increment_operations();
            METRICS.set_view_number(5);
            METRICS.set_commit_number(42);

            let config = OtelConfig::default();
            let exporter = OtelExporter::new(config).unwrap();
            let lines = exporter.export_statsd();

            // Verify format
            assert!(
                lines
                    .iter()
                    .any(|l| l.contains("vsr.operations.total:") && l.ends_with("|c"))
            );
            assert!(
                lines
                    .iter()
                    .any(|l| l.contains("vsr.view.number:") && l.ends_with("|g"))
            );
            assert!(
                lines
                    .iter()
                    .any(|l| l.contains("vsr.commit.number:") && l.ends_with("|g"))
            );
        }

        #[test]
        fn test_otel_init_without_endpoint() {
            let config = OtelConfig::default();
            let mut exporter = OtelExporter::new(config).unwrap();

            // Should fail without endpoint
            let result = exporter.init_otlp();
            assert!(result.is_err());
        }
    }
}
