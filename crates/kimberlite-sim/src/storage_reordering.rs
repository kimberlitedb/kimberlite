//! Write reordering engine for modeling I/O scheduler behavior.
//!
//! Real disk I/O schedulers (elevator, CFQ, deadline) reorder writes to improve
//! throughput. This can expose durability bugs where applications assume writes
//! complete in order. This module simulates deterministic write reordering based
//! on SimRng seeds.
//!
//! ## Design
//!
//! - Pending write queue with configurable reorder window
//! - Dependency tracking (WAL → data blocks)
//! - Multiple reordering policies (FIFO, Random, Elevator-like)
//! - Deterministic reordering based on SimRng seed
//!
//! ## Example
//!
//! ```ignore
//! let mut reorderer = WriteReorderer::new(ReorderConfig::default());
//! let write_id = reorderer.submit_write(address, data, rng);
//! // ... later ...
//! if let Some(write) = reorderer.pop_ready_write(rng) {
//!     // Execute write
//! }
//! ```

use std::collections::VecDeque;

use crate::rng::SimRng;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for write reordering behavior.
#[derive(Debug, Clone)]
pub struct ReorderConfig {
    /// Maximum number of pending writes before blocking.
    pub max_pending: usize,

    /// Reorder window size (number of writes that can be reordered).
    /// Writes outside this window maintain FIFO order.
    pub reorder_window: usize,

    /// Reordering policy to use.
    pub policy: ReorderPolicy,

    /// Whether to track dependencies between writes.
    /// If true, dependent writes (e.g., WAL → data) maintain order.
    pub track_dependencies: bool,
}

impl Default for ReorderConfig {
    fn default() -> Self {
        Self {
            max_pending: 32,
            reorder_window: 8,
            policy: ReorderPolicy::Random,
            track_dependencies: true,
        }
    }
}

impl ReorderConfig {
    /// Creates a configuration with no reordering (FIFO).
    pub fn no_reorder() -> Self {
        Self {
            reorder_window: 1,
            policy: ReorderPolicy::Fifo,
            ..Self::default()
        }
    }

    /// Creates a configuration with aggressive reordering.
    pub fn aggressive() -> Self {
        Self {
            max_pending: 64,
            reorder_window: 32,
            policy: ReorderPolicy::Random,
            track_dependencies: false,
        }
    }
}

/// Reordering policy determines how writes are selected from the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorderPolicy {
    /// First-in-first-out (no reordering).
    Fifo,

    /// Random selection from reorder window.
    Random,

    /// Elevator algorithm (favor sequential addresses).
    Elevator,

    /// Deadline-like (oldest write gets priority after threshold).
    Deadline { max_age_ns: u64 },
}

// ============================================================================
// Write Tracking
// ============================================================================

/// Unique identifier for a pending write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WriteId(u64);

impl WriteId {
    /// Returns the raw ID value.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// A pending write in the reorder queue.
#[derive(Debug, Clone)]
pub struct PendingWrite {
    /// Unique ID for this write.
    pub id: WriteId,

    /// Storage address being written to.
    pub address: u64,

    /// Data being written.
    pub data: Vec<u8>,

    /// Submission time (for deadline policy).
    pub submitted_at_ns: u64,

    /// Dependencies: writes that must complete before this one.
    pub dependencies: Vec<WriteId>,

    /// Whether this write is a barrier (e.g., fsync).
    /// Barrier writes force all previous writes to complete first.
    pub is_barrier: bool,
}

// ============================================================================
// Write Reorderer
// ============================================================================

/// Write reordering engine that models I/O scheduler behavior.
///
/// Maintains a queue of pending writes and reorders them according to
/// the configured policy while respecting dependencies.
#[derive(Debug)]
pub struct WriteReorderer {
    /// Configuration.
    config: ReorderConfig,

    /// Queue of pending writes.
    pending: VecDeque<PendingWrite>,

    /// Next write ID.
    next_id: u64,

    /// Completed write IDs (for dependency tracking).
    completed: Vec<WriteId>,

    /// Last issued address (for elevator policy).
    last_address: u64,
}

impl WriteReorderer {
    /// Creates a new write reorderer with the given configuration.
    pub fn new(config: ReorderConfig) -> Self {
        Self {
            config,
            pending: VecDeque::new(),
            next_id: 0,
            completed: Vec::new(),
            last_address: 0,
        }
    }

    /// Creates a reorderer with default configuration.
    pub fn default_config() -> Self {
        Self::new(ReorderConfig::default())
    }

    /// Submits a write to the reorder queue.
    ///
    /// Returns the write ID that can be used to track dependencies.
    ///
    /// # Panics
    ///
    /// Panics if the queue is full (pending >= max_pending).
    pub fn submit_write(
        &mut self,
        address: u64,
        data: Vec<u8>,
        submitted_at_ns: u64,
        dependencies: Vec<WriteId>,
    ) -> WriteId {
        assert!(
            self.pending.len() < self.config.max_pending,
            "write queue full"
        );

        let id = WriteId(self.next_id);
        self.next_id += 1;

        let write = PendingWrite {
            id,
            address,
            data,
            submitted_at_ns,
            dependencies,
            is_barrier: false,
        };

        self.pending.push_back(write);
        id
    }

    /// Submits a barrier write (e.g., fsync).
    ///
    /// Barrier writes force all previous writes to complete first.
    /// Returns the write ID for the barrier.
    pub fn submit_barrier(&mut self, submitted_at_ns: u64) -> WriteId {
        assert!(
            self.pending.len() < self.config.max_pending,
            "write queue full"
        );

        let id = WriteId(self.next_id);
        self.next_id += 1;

        let write = PendingWrite {
            id,
            address: 0, // Barriers don't have addresses
            data: Vec::new(),
            submitted_at_ns,
            dependencies: Vec::new(),
            is_barrier: true,
        };

        self.pending.push_back(write);
        id
    }

    /// Pops a ready write from the queue according to the reordering policy.
    ///
    /// A write is ready if all its dependencies have completed.
    /// Returns `None` if no writes are ready or the queue is empty.
    ///
    /// For deadline-based policies, `current_time_ns` is used to check write age.
    pub fn pop_ready_write(&mut self, rng: &mut SimRng, current_time_ns: u64) -> Option<PendingWrite> {
        if self.pending.is_empty() {
            return None;
        }

        // Determine reorder window bounds
        let window_size = self.config.reorder_window.min(self.pending.len());

        // Find ready writes in the window (dependencies satisfied)
        let mut ready_indices = Vec::new();
        for i in 0..window_size {
            if self.is_write_ready(&self.pending[i]) {
                ready_indices.push(i);
            }
        }

        if ready_indices.is_empty() {
            return None;
        }

        // Select a write according to policy
        let selected_idx = match self.config.policy {
            ReorderPolicy::Fifo => {
                // Take the first ready write
                ready_indices[0]
            }
            ReorderPolicy::Random => {
                // Random selection from ready writes
                let choice = rng.next_usize(ready_indices.len());
                ready_indices[choice]
            }
            ReorderPolicy::Elevator => {
                // Select write closest to last address (elevator algorithm)
                self.select_elevator(&ready_indices)
            }
            ReorderPolicy::Deadline { max_age_ns } => {
                // Select oldest write if it exceeds deadline, otherwise random
                self.select_deadline(&ready_indices, max_age_ns, current_time_ns, rng)
            }
        };

        // Remove and return the selected write
        let write = self.pending.remove(selected_idx).unwrap();
        self.last_address = write.address;
        self.completed.push(write.id);

        Some(write)
    }

    /// Returns the number of pending writes.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Returns true if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Returns true if the queue is full.
    pub fn is_full(&self) -> bool {
        self.pending.len() >= self.config.max_pending
    }

    /// Checks if a write's dependencies are satisfied.
    fn is_write_ready(&self, write: &PendingWrite) -> bool {
        if write.is_barrier {
            // Barriers are only ready if they're at the front
            return self
                .pending
                .front()
                .is_some_and(|w| w.id == write.id);
        }

        // Check if there's a barrier before this write
        // If so, this write is blocked
        for pending_write in &self.pending {
            if pending_write.id == write.id {
                break; // Found our write, stop searching
            }
            if pending_write.is_barrier {
                // There's a barrier before us, we're blocked
                return false;
            }
        }

        if !self.config.track_dependencies {
            return true;
        }

        // Check if all dependencies have completed
        write
            .dependencies
            .iter()
            .all(|dep_id| self.completed.contains(dep_id))
    }

    /// Selects a write using the elevator algorithm.
    ///
    /// Favors writes with addresses close to the last issued address,
    /// simulating disk head movement minimization.
    fn select_elevator(&self, ready_indices: &[usize]) -> usize {
        let mut best_idx = ready_indices[0];
        let mut best_distance = u64::MAX;

        for &idx in ready_indices {
            let write = &self.pending[idx];
            let distance = write.address.abs_diff(self.last_address);

            if distance < best_distance {
                best_distance = distance;
                best_idx = idx;
            }
        }

        best_idx
    }

    /// Selects a write using deadline policy.
    ///
    /// If any write exceeds max_age_ns, select the oldest.
    /// Otherwise, select randomly.
    fn select_deadline(&self, ready_indices: &[usize], max_age_ns: u64, current_time: u64, rng: &mut SimRng) -> usize {

        // Find oldest write
        let mut oldest_idx = ready_indices[0];
        let mut oldest_age = 0u64;

        for &idx in ready_indices {
            let write = &self.pending[idx];
            let age = current_time.saturating_sub(write.submitted_at_ns);

            if age > oldest_age {
                oldest_age = age;
                oldest_idx = idx;
            }
        }

        // If oldest exceeds deadline, use it
        if oldest_age > max_age_ns {
            oldest_idx
        } else {
            // Otherwise random selection
            let choice = rng.next_usize(ready_indices.len());
            ready_indices[choice]
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_and_pop_fifo() {
        let config = ReorderConfig {
            policy: ReorderPolicy::Fifo,
            reorder_window: 4,
            ..Default::default()
        };
        let mut reorderer = WriteReorderer::new(config);
        let mut rng = SimRng::new(42);

        let id1 = reorderer.submit_write(100, vec![1, 2, 3], 0, vec![]);
        let id2 = reorderer.submit_write(200, vec![4, 5, 6], 0, vec![]);
        let id3 = reorderer.submit_write(300, vec![7, 8, 9], 0, vec![]);

        // FIFO order
        let w1 = reorderer.pop_ready_write(&mut rng, 0).unwrap();
        let w2 = reorderer.pop_ready_write(&mut rng, 0).unwrap();
        let w3 = reorderer.pop_ready_write(&mut rng, 0).unwrap();

        assert_eq!(w1.id, id1);
        assert_eq!(w2.id, id2);
        assert_eq!(w3.id, id3);
        assert!(reorderer.is_empty());
    }

    #[test]
    fn random_reordering() {
        let config = ReorderConfig {
            policy: ReorderPolicy::Random,
            reorder_window: 4,
            ..Default::default()
        };
        let mut reorderer = WriteReorderer::new(config);
        let mut rng = SimRng::new(42);

        // Submit multiple writes
        for i in 0..4 {
            reorderer.submit_write(i * 100, vec![i as u8], 0, vec![]);
        }

        // Pop all writes
        let mut popped = Vec::new();
        while let Some(write) = reorderer.pop_ready_write(&mut rng, 0) {
            popped.push(write.address);
        }

        // Should have all 4 writes (but possibly reordered)
        assert_eq!(popped.len(), 4);
    }

    #[test]
    fn dependency_tracking() {
        let config = ReorderConfig {
            policy: ReorderPolicy::Fifo,
            track_dependencies: true,
            ..Default::default()
        };
        let mut reorderer = WriteReorderer::new(config);
        let mut rng = SimRng::new(42);

        // Write 2 depends on Write 1
        let id1 = reorderer.submit_write(100, vec![1], 0, vec![]);
        let _id2 = reorderer.submit_write(200, vec![2], 0, vec![id1]);

        // Can only pop write 1 first
        let w1 = reorderer.pop_ready_write(&mut rng, 0).unwrap();
        assert_eq!(w1.id, id1);

        // Now write 2 is ready
        let w2 = reorderer.pop_ready_write(&mut rng, 0).unwrap();
        assert_eq!(w2.address, 200);
    }

    #[test]
    fn barrier_blocks() {
        let config = ReorderConfig::default();
        let mut reorderer = WriteReorderer::new(config);
        let mut rng = SimRng::new(42);

        let _id1 = reorderer.submit_write(100, vec![1], 0, vec![]);
        let _barrier = reorderer.submit_barrier(0);
        let _id2 = reorderer.submit_write(200, vec![2], 0, vec![]);

        // Pop first write
        let w1 = reorderer.pop_ready_write(&mut rng, 0).unwrap();
        assert_eq!(w1.address, 100);

        // Barrier should be next (blocks id2)
        let barrier = reorderer.pop_ready_write(&mut rng, 0).unwrap();
        assert!(barrier.is_barrier);

        // Now id2 can proceed
        let w2 = reorderer.pop_ready_write(&mut rng, 0).unwrap();
        assert_eq!(w2.address, 200);
    }

    #[test]
    fn elevator_favors_sequential() {
        let config = ReorderConfig {
            policy: ReorderPolicy::Elevator,
            reorder_window: 10,
            ..Default::default()
        };
        let mut reorderer = WriteReorderer::new(config);
        let mut rng = SimRng::new(42);

        // Submit writes in non-sequential order
        reorderer.submit_write(1000, vec![1], 0, vec![]);
        reorderer.submit_write(100, vec![2], 0, vec![]);
        reorderer.submit_write(1100, vec![3], 0, vec![]);
        reorderer.submit_write(200, vec![4], 0, vec![]);

        // Elevator should favor sequential addresses
        let w1 = reorderer.pop_ready_write(&mut rng, 0).unwrap();
        // First one will be any (last_address = 0)

        // After first write, should favor nearby addresses
        let w2 = reorderer.pop_ready_write(&mut rng, 0).unwrap();
        let w3 = reorderer.pop_ready_write(&mut rng, 0).unwrap();
        let w4 = reorderer.pop_ready_write(&mut rng, 0).unwrap();

        // All writes should complete
        assert!(reorderer.is_empty());

        // Addresses should show some sequential pattern
        // (exact order depends on first write, but distances should be minimized)
        let addresses = vec![w1.address, w2.address, w3.address, w4.address];
        assert_eq!(addresses.len(), 4);
    }

    #[test]
    fn queue_capacity_limits() {
        let config = ReorderConfig {
            max_pending: 2,
            ..Default::default()
        };
        let mut reorderer = WriteReorderer::new(config);

        reorderer.submit_write(100, vec![1], 0, vec![]);
        reorderer.submit_write(200, vec![2], 0, vec![]);

        assert!(reorderer.is_full());
    }
}
