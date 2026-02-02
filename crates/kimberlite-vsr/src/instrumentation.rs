//! Simulation instrumentation for VSR protocol handlers.
//!
//! This module provides instrumentation hooks that are only compiled when
//! the `sim` feature is enabled. It tracks Byzantine message rejections
//! and other protocol validation failures for testing purposes.
//!
//! ## Usage
//!
//! ```ignore
//! #[cfg(feature = "sim")]
//! record_byzantine_rejection(
//!     "inflated_commit_number",
//!     from_replica,
//!     claimed_value,
//!     actual_value,
//! );
//! ```

use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Rejection Tracking
// ============================================================================

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
pub fn record_byzantine_rejection(reason: &str, from: crate::ReplicaId, claimed: u64, actual: u64) {
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
