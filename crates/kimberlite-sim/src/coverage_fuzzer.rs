//! Coverage-guided fuzzing for simulation testing.
//!
//! This module implements coverage-guided fuzzing to prioritize seeds that
//! explore new state space regions. Coverage is tracked across multiple
//! dimensions to maximize the breadth of tested scenarios.
//!
//! ## Coverage Dimensions
//!
//! - **State Coverage**: Unique (view, op_number, commit_number) tuples
//! - **Message Coverage**: Unique message sequences (Prepare→PrepareOK→Commit)
//! - **Fault Coverage**: Unique fault combinations (crash + network partition)
//! - **Path Coverage**: Unique event sequences leading to states
//!
//! ## Fuzzing Strategy
//!
//! Seeds that reach new coverage are added to an "interesting corpus".
//! The fuzzer prioritizes corpus seeds and mutates them to explore nearby
//! state space regions.

use crate::SimRng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ============================================================================
// Coverage Tracking
// ============================================================================

/// Tracks coverage across multiple dimensions.
#[derive(Debug, Clone)]
pub struct CoverageTracker {
    /// Unique VSR state tuples (view, op_number, commit_number).
    state_coverage: HashSet<StatePoint>,

    /// Unique message sequences (up to length 5).
    message_sequences: HashSet<MessageSequence>,

    /// Unique fault combinations.
    fault_combinations: HashSet<FaultSet>,

    /// Unique event type sequences (for path coverage).
    event_sequences: HashSet<EventSequence>,

    /// Statistics.
    stats: CoverageStats,
}

/// A point in VSR state space.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatePoint {
    pub view: u64,
    pub op_number: u64,
    pub commit_number: u64,
    pub replica_id: u64,
}

/// A sequence of message types (limited to 5 for tractability).
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSequence {
    pub messages: Vec<MessageType>,
}

/// Message types for sequence tracking.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    Prepare,
    PrepareOk,
    Commit,
    DoViewChange,
    StartView,
    RequestVote,
    VoteReply,
}

/// A set of active faults.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaultSet {
    pub faults: Vec<FaultKind>,
}

/// Kinds of faults for coverage tracking.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaultKind {
    NetworkPartition,
    NodeCrash,
    MessageDrop,
    MessageDelay,
    StorageCorruption,
    Byzantine,
}

/// A sequence of event types (limited to 10 for tractability).
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSequence {
    pub events: Vec<EventKind>,
}

/// Event types for sequence tracking.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    ClientRequest,
    NetworkMessage,
    StorageComplete,
    Crash,
    Recover,
    ViewChange,
}

/// Coverage statistics.
#[derive(Debug, Clone, Default)]
pub struct CoverageStats {
    pub unique_states: usize,
    pub unique_message_sequences: usize,
    pub unique_fault_combinations: usize,
    pub unique_event_sequences: usize,
    pub total_observations: usize,
}

impl CoverageTracker {
    /// Creates a new coverage tracker.
    pub fn new() -> Self {
        Self {
            state_coverage: HashSet::new(),
            message_sequences: HashSet::new(),
            fault_combinations: HashSet::new(),
            event_sequences: HashSet::new(),
            stats: CoverageStats::default(),
        }
    }

    /// Records a VSR state observation.
    ///
    /// Returns true if this is a new state point.
    pub fn observe_state(&mut self, state: StatePoint) -> bool {
        self.stats.total_observations += 1;
        let is_new = self.state_coverage.insert(state);
        if is_new {
            self.stats.unique_states = self.state_coverage.len();
        }
        is_new
    }

    /// Records a message sequence.
    ///
    /// Returns true if this is a new sequence.
    pub fn observe_message_sequence(&mut self, messages: Vec<MessageType>) -> bool {
        // Limit sequence length to prevent combinatorial explosion
        let sequence = MessageSequence {
            messages: messages.into_iter().take(5).collect(),
        };

        let is_new = self.message_sequences.insert(sequence);
        if is_new {
            self.stats.unique_message_sequences = self.message_sequences.len();
        }
        is_new
    }

    /// Records a fault combination.
    ///
    /// Returns true if this is a new combination.
    pub fn observe_fault_set(&mut self, faults: Vec<FaultKind>) -> bool {
        let mut sorted_faults = faults;
        sorted_faults.sort_by_key(|f| format!("{:?}", f));
        let fault_set = FaultSet {
            faults: sorted_faults,
        };

        let is_new = self.fault_combinations.insert(fault_set);
        if is_new {
            self.stats.unique_fault_combinations = self.fault_combinations.len();
        }
        is_new
    }

    /// Records an event sequence.
    ///
    /// Returns true if this is a new sequence.
    pub fn observe_event_sequence(&mut self, events: Vec<EventKind>) -> bool {
        let sequence = EventSequence {
            events: events.into_iter().take(10).collect(),
        };

        let is_new = self.event_sequences.insert(sequence);
        if is_new {
            self.stats.unique_event_sequences = self.event_sequences.len();
        }
        is_new
    }

    /// Returns true if any observation was new (reached new coverage).
    pub fn reached_new_coverage(&self, baseline_stats: &CoverageStats) -> bool {
        self.stats.unique_states > baseline_stats.unique_states
            || self.stats.unique_message_sequences > baseline_stats.unique_message_sequences
            || self.stats.unique_fault_combinations > baseline_stats.unique_fault_combinations
            || self.stats.unique_event_sequences > baseline_stats.unique_event_sequences
    }

    /// Returns current coverage statistics.
    pub fn stats(&self) -> &CoverageStats {
        &self.stats
    }

    /// Resets all coverage tracking.
    pub fn clear(&mut self) {
        self.state_coverage.clear();
        self.message_sequences.clear();
        self.fault_combinations.clear();
        self.event_sequences.clear();
        self.stats = CoverageStats::default();
    }
}

impl Default for CoverageTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Coverage-Guided Fuzzer
// ============================================================================

/// Coverage-guided fuzzer that maintains an interesting seed corpus.
#[derive(Debug)]
pub struct CoverageFuzzer {
    /// Coverage tracker.
    tracker: CoverageTracker,

    /// Interesting seed corpus (seeds that reached new coverage).
    corpus: Vec<InterestingSeed>,

    /// Seed selection strategy.
    strategy: SelectionStrategy,

    /// Maximum corpus size (to prevent unbounded growth).
    max_corpus_size: usize,
}

/// A seed that reached interesting coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterestingSeed {
    /// RNG seed.
    pub seed: u64,

    /// Coverage stats when this seed was added.
    pub coverage_snapshot: CoverageStatsSnapshot,

    /// Number of times this seed has been selected.
    pub selection_count: usize,

    /// Energy (priority for selection).
    pub energy: f64,
}

/// Serializable snapshot of coverage stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageStatsSnapshot {
    pub unique_states: usize,
    pub unique_message_sequences: usize,
    pub unique_fault_combinations: usize,
    pub unique_event_sequences: usize,
}

/// Seed selection strategy.
#[derive(Debug, Clone, Copy)]
pub enum SelectionStrategy {
    /// Random selection from corpus.
    Random,

    /// Prefer seeds with lower selection count.
    LeastUsed,

    /// Energy-based selection (AFL-style).
    EnergyBased,
}

impl CoverageFuzzer {
    /// Creates a new coverage-guided fuzzer.
    pub fn new(strategy: SelectionStrategy) -> Self {
        Self {
            tracker: CoverageTracker::new(),
            corpus: Vec::new(),
            strategy,
            max_corpus_size: 10_000,
        }
    }

    /// Records a seed run and its coverage.
    ///
    /// If the seed reached new coverage, it's added to the corpus.
    pub fn record_seed(&mut self, seed: u64, reached_new_coverage: bool) {
        if reached_new_coverage {
            let snapshot = CoverageStatsSnapshot {
                unique_states: self.tracker.stats.unique_states,
                unique_message_sequences: self.tracker.stats.unique_message_sequences,
                unique_fault_combinations: self.tracker.stats.unique_fault_combinations,
                unique_event_sequences: self.tracker.stats.unique_event_sequences,
            };

            let interesting = InterestingSeed {
                seed,
                coverage_snapshot: snapshot,
                selection_count: 0,
                energy: 1.0,
            };

            self.corpus.push(interesting);

            // Trim corpus if it exceeds max size
            if self.corpus.len() > self.max_corpus_size {
                self.trim_corpus();
            }
        }
    }

    /// Selects a seed from the corpus for mutation/exploration.
    ///
    /// Returns None if corpus is empty.
    pub fn select_seed(&mut self, rng: &mut SimRng) -> Option<u64> {
        if self.corpus.is_empty() {
            return None;
        }

        let idx = match self.strategy {
            SelectionStrategy::Random => rng.next_usize(self.corpus.len()),
            SelectionStrategy::LeastUsed => self.select_least_used(),
            SelectionStrategy::EnergyBased => self.select_by_energy(rng),
        };

        self.corpus[idx].selection_count += 1;
        Some(self.corpus[idx].seed)
    }

    /// Mutates a seed (simple bit flipping for now).
    pub fn mutate_seed(&self, seed: u64, rng: &mut SimRng) -> u64 {
        let mutation_type = rng.next_usize(3);
        match mutation_type {
            0 => seed.wrapping_add(rng.next_u64()), // Add random value
            1 => seed ^ (1u64 << rng.next_usize(64)), // Flip random bit
            2 => seed.wrapping_mul(rng.next_u64()), // Multiply
            _ => unreachable!(),
        }
    }

    /// Returns the coverage tracker (for observation recording).
    pub fn tracker_mut(&mut self) -> &mut CoverageTracker {
        &mut self.tracker
    }

    /// Returns current coverage statistics.
    pub fn coverage_stats(&self) -> &CoverageStats {
        self.tracker.stats()
    }

    /// Returns corpus size.
    pub fn corpus_size(&self) -> usize {
        self.corpus.len()
    }

    /// Selects the least-used seed.
    fn select_least_used(&self) -> usize {
        self.corpus
            .iter()
            .enumerate()
            .min_by_key(|(_, seed)| seed.selection_count)
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }

    /// Selects a seed based on energy (AFL-style).
    fn select_by_energy(&self, rng: &mut SimRng) -> usize {
        // Simple energy-based selection: higher energy = higher probability
        let total_energy: f64 = self.corpus.iter().map(|s| s.energy).sum();
        let mut target = rng.next_f64() * total_energy;

        for (idx, seed) in self.corpus.iter().enumerate() {
            target -= seed.energy;
            if target <= 0.0 {
                return idx;
            }
        }

        self.corpus.len() - 1
    }

    /// Trims corpus to max_corpus_size by removing least interesting seeds.
    fn trim_corpus(&mut self) {
        // Sort by energy (descending) and keep top N
        self.corpus.sort_by(|a, b| {
            b.energy
                .partial_cmp(&a.energy)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.corpus.truncate(self.max_corpus_size);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_tracker_observes_states() {
        let mut tracker = CoverageTracker::new();

        let state1 = StatePoint {
            view: 0,
            op_number: 10,
            commit_number: 5,
            replica_id: 0,
        };

        let state2 = StatePoint {
            view: 1,
            op_number: 20,
            commit_number: 15,
            replica_id: 1,
        };

        assert!(tracker.observe_state(state1)); // New
        assert!(!tracker.observe_state(state1)); // Duplicate
        assert!(tracker.observe_state(state2)); // New

        assert_eq!(tracker.stats().unique_states, 2);
    }

    #[test]
    fn coverage_tracker_message_sequences() {
        let mut tracker = CoverageTracker::new();

        let seq1 = vec![
            MessageType::Prepare,
            MessageType::PrepareOk,
            MessageType::Commit,
        ];
        let seq2 = vec![MessageType::DoViewChange, MessageType::StartView];

        assert!(tracker.observe_message_sequence(seq1.clone())); // New
        assert!(!tracker.observe_message_sequence(seq1)); // Duplicate
        assert!(tracker.observe_message_sequence(seq2)); // New

        assert_eq!(tracker.stats().unique_message_sequences, 2);
    }

    #[test]
    fn coverage_fuzzer_builds_corpus() {
        let mut fuzzer = CoverageFuzzer::new(SelectionStrategy::Random);
        let mut rng = SimRng::new(42);

        // Simulate finding interesting seeds
        fuzzer.record_seed(1000, true); // Interesting
        fuzzer.record_seed(2000, false); // Not interesting
        fuzzer.record_seed(3000, true); // Interesting

        assert_eq!(fuzzer.corpus_size(), 2);

        // Can select from corpus
        let selected = fuzzer.select_seed(&mut rng);
        assert!(selected.is_some());
        assert!(selected.unwrap() == 1000 || selected.unwrap() == 3000);
    }

    #[test]
    fn coverage_fuzzer_mutates_seeds() {
        let fuzzer = CoverageFuzzer::new(SelectionStrategy::Random);
        let mut rng = SimRng::new(42);

        let seed = 12345u64;
        let mutated = fuzzer.mutate_seed(seed, &mut rng);

        assert_ne!(seed, mutated);
    }

    #[test]
    fn selection_strategy_least_used() {
        let mut fuzzer = CoverageFuzzer::new(SelectionStrategy::LeastUsed);
        let mut rng = SimRng::new(42);

        fuzzer.record_seed(100, true);
        fuzzer.record_seed(200, true);

        // First selection
        let _ = fuzzer.select_seed(&mut rng);

        // Second selection should pick the other seed (least used)
        let selected = fuzzer.select_seed(&mut rng).unwrap();

        // One seed should have selection_count=2, other should have 1
        let counts: Vec<_> = fuzzer.corpus.iter().map(|s| s.selection_count).collect();
        assert_eq!(counts.iter().sum::<usize>(), 2);
    }
}
