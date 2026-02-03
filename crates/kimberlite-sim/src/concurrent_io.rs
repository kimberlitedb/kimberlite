//! Concurrent I/O simulator for modeling multiple outstanding operations.
//!
//! Real storage devices handle multiple concurrent operations. This module
//! simulates concurrent I/O with deterministic out-of-order completion,
//! exposing bugs in code that assumes sequential I/O.
//!
//! ## Design
//!
//! - Track in-flight operations (up to max_concurrent per device)
//! - Out-of-order I/O completion based on SimRng
//! - Barrier operations (fsync) block until all prior I/O completes
//! - Deterministic completion ordering
//!
//! ## Example
//!
//! ```ignore
//! let mut io_tracker = ConcurrentIOTracker::new(32); // max 32 ops
//! let op_id = io_tracker.start_operation(OpKind::Write, completion_time_ns);
//! // ... later ...
//! if let Some(completed) = io_tracker.poll_completions(current_time_ns) {
//!     // Handle completed operation
//! }
//! ```

use std::collections::VecDeque;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for concurrent I/O simulation.
#[derive(Debug, Clone)]
pub struct ConcurrentIOConfig {
    /// Maximum number of concurrent operations per device.
    pub max_concurrent: usize,

    /// Whether to allow out-of-order completion.
    /// If false, operations complete in submission order (like O_DIRECT with queue depth 1).
    pub allow_out_of_order: bool,
}

impl Default for ConcurrentIOConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 32,
            allow_out_of_order: true,
        }
    }
}

impl ConcurrentIOConfig {
    /// Creates a configuration that enforces strict ordering (no concurrency).
    pub fn sequential() -> Self {
        Self {
            max_concurrent: 1,
            allow_out_of_order: false,
        }
    }

    /// Creates a configuration with high concurrency.
    pub fn high_concurrency() -> Self {
        Self {
            max_concurrent: 128,
            allow_out_of_order: true,
        }
    }
}

// ============================================================================
// I/O Operations
// ============================================================================

/// Unique identifier for an I/O operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(u64);

impl OperationId {
    /// Returns the raw ID value.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// Kind of I/O operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// Read operation.
    Read,

    /// Write operation.
    Write,

    /// Fsync operation (barrier).
    Fsync,
}

/// State of an in-flight I/O operation.
#[derive(Debug, Clone)]
pub struct InFlightOp {
    /// Unique operation ID.
    pub id: OperationId,

    /// Kind of operation.
    pub kind: OpKind,

    /// Time operation was submitted (ns).
    pub submitted_at_ns: u64,

    /// Time operation will complete (ns).
    pub completes_at_ns: u64,

    /// Storage address (for reads/writes).
    pub address: Option<u64>,

    /// Data size in bytes.
    pub size_bytes: usize,

    /// Whether this is a barrier operation.
    /// Barriers block until all prior operations complete.
    pub is_barrier: bool,
}

/// Result of a completed I/O operation.
#[derive(Debug, Clone)]
pub struct CompletedOp {
    /// The operation that completed.
    pub operation: InFlightOp,

    /// Whether the operation succeeded.
    pub success: bool,

    /// Completion time (ns).
    pub completed_at_ns: u64,
}

// ============================================================================
// Concurrent I/O Tracker
// ============================================================================

/// Tracks concurrent in-flight I/O operations with deterministic completion.
///
/// Models a storage device that can handle multiple concurrent operations,
/// completing them potentially out-of-order based on device characteristics
/// and simulation RNG.
#[derive(Debug)]
pub struct ConcurrentIOTracker {
    /// Configuration.
    config: ConcurrentIOConfig,

    /// In-flight operations, ordered by completion time.
    in_flight: VecDeque<InFlightOp>,

    /// Next operation ID.
    next_id: u64,

    /// Total operations started.
    total_started: u64,

    /// Total operations completed.
    total_completed: u64,
}

impl ConcurrentIOTracker {
    /// Creates a new tracker with the given configuration.
    pub fn new(config: ConcurrentIOConfig) -> Self {
        Self {
            config,
            in_flight: VecDeque::new(),
            next_id: 0,
            total_started: 0,
            total_completed: 0,
        }
    }

    /// Creates a tracker with default configuration.
    pub fn default_config() -> Self {
        Self::new(ConcurrentIOConfig::default())
    }

    /// Starts a new I/O operation.
    ///
    /// Returns the operation ID.
    ///
    /// # Panics
    ///
    /// Panics if the maximum concurrent operations limit is exceeded.
    pub fn start_operation(
        &mut self,
        kind: OpKind,
        submitted_at_ns: u64,
        completes_at_ns: u64,
        address: Option<u64>,
        size_bytes: usize,
    ) -> OperationId {
        assert!(
            self.in_flight.len() < self.config.max_concurrent,
            "exceeded max concurrent I/O operations"
        );

        let id = OperationId(self.next_id);
        self.next_id += 1;
        self.total_started += 1;

        let op = InFlightOp {
            id,
            kind,
            submitted_at_ns,
            completes_at_ns,
            address,
            size_bytes,
            is_barrier: matches!(kind, OpKind::Fsync),
        };

        // Insert maintaining sorted order by completion time
        let insert_pos = self
            .in_flight
            .iter()
            .position(|o| o.completes_at_ns > completes_at_ns)
            .unwrap_or(self.in_flight.len());

        self.in_flight.insert(insert_pos, op);

        id
    }

    /// Starts a barrier operation (fsync).
    ///
    /// Barrier operations complete only after all prior operations complete.
    /// Returns the operation ID.
    pub fn start_barrier(
        &mut self,
        submitted_at_ns: u64,
        completes_at_ns: u64,
    ) -> OperationId {
        // Fsync completes after all currently in-flight ops
        let actual_completion = self
            .in_flight
            .iter()
            .map(|op| op.completes_at_ns)
            .max()
            .unwrap_or(submitted_at_ns)
            .max(completes_at_ns);

        self.start_operation(OpKind::Fsync, submitted_at_ns, actual_completion, None, 0)
    }

    /// Polls for completed operations at the given time.
    ///
    /// Returns all operations that have completed by `current_time_ns`,
    /// potentially out of order if `allow_out_of_order` is enabled.
    pub fn poll_completions(&mut self, current_time_ns: u64) -> Vec<CompletedOp> {
        let mut completed = Vec::new();

        // In out-of-order mode, we can complete any ready operation
        // In ordered mode, we must complete operations in submission order
        if self.config.allow_out_of_order {
            // Complete all operations whose time has arrived
            while let Some(op) = self.in_flight.front() {
                if op.completes_at_ns <= current_time_ns {
                    let op = self.in_flight.pop_front().unwrap();
                    self.total_completed += 1;

                    completed.push(CompletedOp {
                        operation: op,
                        success: true,
                        completed_at_ns: current_time_ns,
                    });
                } else {
                    break;
                }
            }
        } else {
            // Ordered mode: only complete the oldest (smallest ID) operation IF it's ready
            // If the oldest isn't ready, complete nothing
            if let Some((idx, op)) = self
                .in_flight
                .iter()
                .enumerate()
                .min_by_key(|(_, op)| op.id.0)
            {
                if op.completes_at_ns <= current_time_ns {
                    let op = self.in_flight.remove(idx).unwrap();
                    self.total_completed += 1;

                    completed.push(CompletedOp {
                        operation: op,
                        success: true,
                        completed_at_ns: current_time_ns,
                    });
                }
            }
        }

        completed
    }

    /// Returns the number of in-flight operations.
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    /// Returns true if there are no in-flight operations.
    pub fn is_idle(&self) -> bool {
        self.in_flight.is_empty()
    }

    /// Returns true if the device is at max capacity.
    pub fn is_at_capacity(&self) -> bool {
        self.in_flight.len() >= self.config.max_concurrent
    }

    /// Returns the time when the next operation will complete, if any.
    pub fn next_completion_time(&self) -> Option<u64> {
        self.in_flight.front().map(|op| op.completes_at_ns)
    }

    /// Returns statistics about I/O operations.
    pub fn stats(&self) -> IOStats {
        IOStats {
            total_started: self.total_started,
            total_completed: self.total_completed,
            in_flight: self.in_flight.len() as u64,
            max_concurrent: self.config.max_concurrent as u64,
        }
    }

    /// Cancels all in-flight operations (simulates device reset/crash).
    ///
    /// Returns the operations that were in flight.
    pub fn cancel_all(&mut self) -> Vec<InFlightOp> {
        let ops: Vec<_> = self.in_flight.drain(..).collect();
        ops
    }
}

/// Statistics about I/O operations.
#[derive(Debug, Clone)]
pub struct IOStats {
    /// Total operations started.
    pub total_started: u64,

    /// Total operations completed.
    pub total_completed: u64,

    /// Current in-flight operations.
    pub in_flight: u64,

    /// Maximum concurrent operations allowed.
    pub max_concurrent: u64,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_and_complete_operation() {
        let mut tracker = ConcurrentIOTracker::default_config();

        let op_id = tracker.start_operation(OpKind::Write, 0, 1000, Some(100), 4096);

        assert_eq!(tracker.in_flight_count(), 1);
        assert!(!tracker.is_idle());

        // Not ready yet
        let completed = tracker.poll_completions(500);
        assert!(completed.is_empty());
        assert_eq!(tracker.in_flight_count(), 1);

        // Now ready
        let completed = tracker.poll_completions(1000);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].operation.id, op_id);
        assert!(tracker.is_idle());
    }

    #[test]
    fn out_of_order_completion() {
        let config = ConcurrentIOConfig {
            allow_out_of_order: true,
            ..Default::default()
        };
        let mut tracker = ConcurrentIOTracker::new(config);

        // Submit 3 ops with different completion times
        let op1 = tracker.start_operation(OpKind::Write, 0, 3000, Some(100), 4096);
        let op2 = tracker.start_operation(OpKind::Write, 0, 1000, Some(200), 4096);
        let op3 = tracker.start_operation(OpKind::Write, 0, 2000, Some(300), 4096);

        // At time 1500, op2 should complete
        let completed = tracker.poll_completions(1500);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].operation.id, op2);

        // At time 2500, op3 should complete
        let completed = tracker.poll_completions(2500);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].operation.id, op3);

        // At time 3500, op1 should complete
        let completed = tracker.poll_completions(3500);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].operation.id, op1);
    }

    #[test]
    fn ordered_completion() {
        let config = ConcurrentIOConfig {
            allow_out_of_order: false,
            max_concurrent: 10,
        };
        let mut tracker = ConcurrentIOTracker::new(config);

        // Submit 3 ops with different completion times
        let op1 = tracker.start_operation(OpKind::Write, 0, 3000, Some(100), 4096);
        let op2 = tracker.start_operation(OpKind::Write, 0, 1000, Some(200), 4096);
        let _op3 = tracker.start_operation(OpKind::Write, 0, 2000, Some(300), 4096);

        // At time 2000, can't complete op2 or op3 yet because op1 isn't done
        let completed = tracker.poll_completions(2000);
        assert!(completed.is_empty());

        // At time 3000, op1 completes
        let completed = tracker.poll_completions(3000);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].operation.id, op1);

        // Now op2 can complete (even though time has passed)
        let completed = tracker.poll_completions(3001);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].operation.id, op2);
    }

    #[test]
    fn barrier_waits_for_prior_ops() {
        let mut tracker = ConcurrentIOTracker::default_config();

        // Start a write that completes at 1000ns
        tracker.start_operation(OpKind::Write, 0, 1000, Some(100), 4096);

        // Start a barrier at 500ns
        let barrier_id = tracker.start_barrier(500, 500);

        // Barrier should complete after the write (at 1000ns)
        let barrier = tracker
            .in_flight
            .iter()
            .find(|op| op.id == barrier_id)
            .unwrap();
        assert!(barrier.completes_at_ns >= 1000);
    }

    #[test]
    fn capacity_limits() {
        let config = ConcurrentIOConfig {
            max_concurrent: 2,
            ..Default::default()
        };
        let mut tracker = ConcurrentIOTracker::new(config);

        tracker.start_operation(OpKind::Write, 0, 1000, Some(100), 4096);
        tracker.start_operation(OpKind::Write, 0, 2000, Some(200), 4096);

        assert!(tracker.is_at_capacity());
        assert_eq!(tracker.in_flight_count(), 2);
    }

    #[test]
    fn cancel_all_operations() {
        let mut tracker = ConcurrentIOTracker::default_config();

        tracker.start_operation(OpKind::Write, 0, 1000, Some(100), 4096);
        tracker.start_operation(OpKind::Write, 0, 2000, Some(200), 4096);

        assert_eq!(tracker.in_flight_count(), 2);

        let cancelled = tracker.cancel_all();
        assert_eq!(cancelled.len(), 2);
        assert!(tracker.is_idle());
    }

    #[test]
    fn next_completion_time() {
        let mut tracker = ConcurrentIOTracker::default_config();

        assert!(tracker.next_completion_time().is_none());

        tracker.start_operation(OpKind::Write, 0, 3000, Some(100), 4096);
        tracker.start_operation(OpKind::Write, 0, 1000, Some(200), 4096);

        // Should return earliest completion time
        assert_eq!(tracker.next_completion_time(), Some(1000));
    }

    #[test]
    fn stats_tracking() {
        let mut tracker = ConcurrentIOTracker::default_config();

        tracker.start_operation(OpKind::Write, 0, 1000, Some(100), 4096);
        tracker.start_operation(OpKind::Write, 0, 2000, Some(200), 4096);

        let stats = tracker.stats();
        assert_eq!(stats.total_started, 2);
        assert_eq!(stats.total_completed, 0);
        assert_eq!(stats.in_flight, 2);

        tracker.poll_completions(1500);

        let stats = tracker.stats();
        assert_eq!(stats.total_started, 2);
        assert_eq!(stats.total_completed, 1);
        assert_eq!(stats.in_flight, 1);
    }
}
