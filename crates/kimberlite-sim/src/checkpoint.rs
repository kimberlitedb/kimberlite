//! Simulation checkpointing for fast-forward replay.
//!
//! This module provides snapshot and restore capabilities for VOPR simulations,
//! enabling efficient binary search through execution history.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::SimRng;

// ============================================================================
// RNG State
// ============================================================================

/// RNG state for deterministic resumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RngCheckpoint {
    /// Original seed.
    pub seed: u64,
    /// Number of random values generated so far.
    pub step_count: u64,
}

impl RngCheckpoint {
    /// Creates a checkpoint of current RNG state.
    pub fn from_rng(rng: &SimRng, seed: u64) -> Self {
        Self {
            seed,
            step_count: rng.step_count(),
        }
    }

    /// Restores RNG to this checkpoint.
    pub fn restore(&self) -> SimRng {
        // PRECONDITION: step_count must be reasonable
        const MAX_RNG_STEPS: u64 = 10_000_000; // 10M steps max
        assert!(
            self.step_count <= MAX_RNG_STEPS,
            "RNG step count {} exceeds maximum {}",
            self.step_count,
            MAX_RNG_STEPS
        );

        let mut rng = SimRng::new(self.seed);
        // Fast-forward to the checkpointed step count (now bounded)
        for _ in 0..self.step_count {
            let _ = rng.next_u64();
        }
        rng
    }
}

// ============================================================================
// Simulation Checkpoint
// ============================================================================

/// Full simulation state snapshot for fast-forward replay.
///
/// Note: This is a lightweight checkpoint focusing on RNG state and event count.
/// Full state restoration (including event queue and storage) would require
/// Event/EventKind to implement Serialize, which we can add if needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationCheckpoint {
    /// Event count at checkpoint.
    pub event_count: u64,
    /// Simulation time (ns).
    pub time_ns: u64,
    /// RNG state.
    pub rng_state: RngCheckpoint,
    /// Generic state data (for extensibility).
    pub state_data: HashMap<String, Vec<u8>>,
}

impl SimulationCheckpoint {
    /// Creates a new empty checkpoint.
    pub fn new(event_count: u64, time_ns: u64, rng_state: RngCheckpoint) -> Self {
        Self {
            event_count,
            time_ns,
            rng_state,
            state_data: HashMap::new(),
        }
    }

    /// Adds state data to the checkpoint.
    pub fn with_state_data(mut self, key: String, data: Vec<u8>) -> Self {
        self.state_data.insert(key, data);
        self
    }

    /// Serializes checkpoint to bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        Ok(postcard::to_allocvec(self)?)
    }

    /// Deserializes checkpoint from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(postcard::from_bytes(bytes)?)
    }

    /// Estimates memory usage in bytes.
    pub fn size_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .state_data
                .values()
                .map(|v| v.len())
                .sum::<usize>()
    }
}

// ============================================================================
// Checkpoint Manager
// ============================================================================

/// Manages simulation checkpoints for bisection.
pub struct CheckpointManager {
    /// Checkpoints indexed by event count.
    checkpoints: std::collections::BTreeMap<u64, SimulationCheckpoint>,
    /// Maximum checkpoints to keep.
    max_checkpoints: usize,
}

impl CheckpointManager {
    /// Creates a new checkpoint manager.
    pub fn new(max_checkpoints: usize) -> Self {
        Self {
            checkpoints: std::collections::BTreeMap::new(),
            max_checkpoints,
        }
    }

    /// Saves a checkpoint.
    pub fn save(&mut self, checkpoint: SimulationCheckpoint) {
        // PRECONDITION: event_count must be non-negative (u64 guarantees this)
        // Allow 0 for initial state checkpoints

        // Check monotonicity: new checkpoint should be after the last one
        if let Some(last_key) = self.checkpoints.keys().next_back() {
            if checkpoint.event_count <= *last_key {
                // Allow same event_count (replacement), but warn on going backwards
                debug_assert!(
                    checkpoint.event_count >= *last_key,
                    "checkpoint event_count not monotonic: last={}, new={}",
                    last_key,
                    checkpoint.event_count
                );
            }
        }

        let event_count = checkpoint.event_count;
        self.checkpoints.insert(event_count, checkpoint);

        // Evict oldest checkpoints if over limit
        while self.checkpoints.len() > self.max_checkpoints {
            if let Some(first_key) = self.checkpoints.keys().next().copied() {
                let evicted = self.checkpoints.remove(&first_key);

                // POSTCONDITION: evicted checkpoint should be oldest
                if let Some(evicted) = evicted {
                    if let Some(new_first) = self.checkpoints.keys().next() {
                        assert!(
                            evicted.event_count < *new_first,
                            "evicted checkpoint not oldest: evicted={}, first={}",
                            evicted.event_count,
                            new_first
                        );
                    }
                }
            }
        }

        // POSTCONDITION: checkpoints remain sorted (BTreeMap guarantees this)
        debug_assert!(
            self.checkpoints.len() <= self.max_checkpoints,
            "checkpoint count {} exceeds maximum {}",
            self.checkpoints.len(),
            self.max_checkpoints
        );
    }

    /// Finds the closest checkpoint before or at the given event count.
    pub fn find_closest(&self, max_event_count: u64) -> Option<&SimulationCheckpoint> {
        self.checkpoints
            .range(..=max_event_count)
            .next_back()
            .map(|(_, checkpoint)| checkpoint)
    }

    /// Returns the number of checkpoints stored.
    pub fn len(&self) -> usize {
        self.checkpoints.len()
    }

    /// Returns true if no checkpoints are stored.
    pub fn is_empty(&self) -> bool {
        self.checkpoints.is_empty()
    }

    /// Clears all checkpoints.
    pub fn clear(&mut self) {
        self.checkpoints.clear();
    }

    /// Returns total memory usage of all checkpoints.
    pub fn total_size_bytes(&self) -> usize {
        self.checkpoints
            .values()
            .map(|c| c.size_bytes())
            .sum()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_checkpoint_restore() {
        let mut rng1 = SimRng::new(42);

        // Generate some random numbers
        let _v1 = rng1.next_u64();
        let _v2 = rng1.next_u64();
        let _v3 = rng1.next_u64();

        // Checkpoint
        let checkpoint = RngCheckpoint::from_rng(&rng1, 42);

        // Continue generating
        let v4 = rng1.next_u64();
        let v5 = rng1.next_u64();

        // Restore from checkpoint
        let mut rng2 = checkpoint.restore();

        // Should generate same values
        assert_eq!(rng2.next_u64(), v4);
        assert_eq!(rng2.next_u64(), v5);
    }

    #[test]
    fn simulation_checkpoint_serialization() {
        let checkpoint = SimulationCheckpoint::new(
            100,
            1_000_000,
            RngCheckpoint { seed: 42, step_count: 10 },
        );

        let bytes = checkpoint.to_bytes().unwrap();
        let restored = SimulationCheckpoint::from_bytes(&bytes).unwrap();

        assert_eq!(restored.event_count, 100);
        assert_eq!(restored.time_ns, 1_000_000);
        assert_eq!(restored.rng_state.seed, 42);
        assert_eq!(restored.rng_state.step_count, 10);
    }

    #[test]
    fn checkpoint_manager_eviction() {
        let mut manager = CheckpointManager::new(3);

        // Add 5 checkpoints
        for i in 0..5 {
            let checkpoint = SimulationCheckpoint::new(
                i * 1000,
                i * 1_000_000,
                RngCheckpoint { seed: 42, step_count: i },
            );
            manager.save(checkpoint);
        }

        // Should keep only last 3
        assert_eq!(manager.len(), 3);

        // Should have checkpoints for events 2000, 3000, 4000
        assert!(manager.find_closest(2000).is_some());
        assert!(manager.find_closest(3000).is_some());
        assert!(manager.find_closest(4000).is_some());
        assert!(manager.find_closest(1000).is_none());
    }

    #[test]
    fn checkpoint_manager_find_closest() {
        let mut manager = CheckpointManager::new(10);

        manager.save(SimulationCheckpoint::new(
            1000,
            0,
            RngCheckpoint { seed: 42, step_count: 0 },
        ));
        manager.save(SimulationCheckpoint::new(
            3000,
            0,
            RngCheckpoint { seed: 42, step_count: 0 },
        ));
        manager.save(SimulationCheckpoint::new(
            5000,
            0,
            RngCheckpoint { seed: 42, step_count: 0 },
        ));

        // Query for event 3500 should return checkpoint at 3000
        let closest = manager.find_closest(3500).unwrap();
        assert_eq!(closest.event_count, 3000);

        // Query for event 500 should return None
        assert!(manager.find_closest(500).is_none());

        // Query for exact match should return that checkpoint
        let exact = manager.find_closest(3000).unwrap();
        assert_eq!(exact.event_count, 3000);
    }
}
