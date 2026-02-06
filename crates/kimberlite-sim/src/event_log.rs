//! Deterministic event logging for simulation debugging.
//!
//! Records all nondeterministic decisions during simulation execution
//! to enable perfect reproduction of failures.
//!
//! ## Design
//!
//! The event log captures:
//! - RNG seeds and outputs
//! - Event scheduling decisions
//! - Network delays and message drops
//! - Storage operation timing
//!
//! Logs are stored in a compact binary format (~100 bytes/event) and can
//! be serialized to disk for later replay.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

// ============================================================================
// Logged Events
// ============================================================================

/// A logged nondeterministic decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggedEvent {
    /// Simulation time when this decision was made.
    pub time_ns: u64,

    /// Event number (sequential ID).
    pub event_id: u64,

    /// The decision that was made.
    pub decision: Decision,
}

/// Types of nondeterministic decisions to log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Decision {
    /// RNG value generated.
    RngValue { value: u64 },

    /// Event scheduled.
    EventScheduled {
        event_type: String,
        scheduled_at_ns: u64,
    },

    /// Network message delayed.
    NetworkDelay { message_id: u64, delay_ns: u64 },

    /// Network message dropped.
    NetworkDrop { message_id: u64 },

    /// Storage operation completed.
    StorageComplete {
        operation_id: u64,
        success: bool,
        latency_ns: u64,
    },

    /// Node crashed.
    NodeCrash { node_id: u64 },

    /// Node recovered.
    NodeRestart { node_id: u64 },

    /// Byzantine attack applied.
    ByzantineAttack { attack_type: String, target: String },

    /// Scheduler decision: which node was selected to run.
    SchedulerNodeSelected { node_id: u64, runnable_count: u64 },

    /// Scheduler decision: event dequeued from event queue.
    SchedulerEventDequeued {
        event_type: String,
        queue_depth: usize,
    },

    /// Time advanced.
    TimeAdvance {
        from_ns: u64,
        to_ns: u64,
        delta_ns: u64,
    },

    /// Timer fired.
    TimerFired {
        timer_id: u64,
        scheduled_for_ns: u64,
        actual_fire_ns: u64,
    },

    /// Invariant check executed.
    InvariantCheck {
        invariant_name: String,
        passed: bool,
    },
}

// ============================================================================
// Event Log
// ============================================================================

/// Event logger that records simulation decisions.
#[derive(Debug)]
pub struct EventLog {
    /// Logged events.
    events: VecDeque<LoggedEvent>,

    /// Next event ID.
    next_id: u64,

    /// Maximum events to keep in memory (for bounded memory usage).
    max_in_memory: usize,

    /// Whether logging is enabled.
    enabled: bool,
}

impl EventLog {
    /// Creates a new event log.
    pub fn new() -> Self {
        Self {
            events: VecDeque::new(),
            next_id: 0,
            max_in_memory: 100_000,
            enabled: true,
        }
    }

    /// Creates a disabled event log (no-op).
    pub fn disabled() -> Self {
        Self {
            events: VecDeque::new(),
            next_id: 0,
            max_in_memory: 0,
            enabled: false,
        }
    }

    /// Logs a decision.
    pub fn log(&mut self, time_ns: u64, decision: Decision) {
        if !self.enabled {
            return;
        }

        let event = LoggedEvent {
            time_ns,
            event_id: self.next_id,
            decision,
        };

        self.next_id += 1;
        self.events.push_back(event);

        // Evict old events if we exceed max_in_memory
        if self.events.len() > self.max_in_memory {
            self.events.pop_front();
        }
    }

    /// Returns the number of logged events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns true if no events have been logged.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns an iterator over logged events.
    pub fn iter(&self) -> impl Iterator<Item = &LoggedEvent> {
        self.events.iter()
    }

    /// Writes the log to a file.
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // Write header: version + event count
        let version: u32 = 1;
        let version_bytes = postcard::to_allocvec(&version).map_err(io::Error::other)?;
        writer.write_all(&version_bytes)?;

        let count_bytes =
            postcard::to_allocvec(&(self.events.len() as u64)).map_err(io::Error::other)?;
        writer.write_all(&count_bytes)?;

        // Write events
        for event in &self.events {
            let event_bytes = postcard::to_allocvec(event).map_err(io::Error::other)?;
            writer.write_all(&event_bytes)?;
        }

        writer.flush()?;
        Ok(())
    }

    /// Clears all logged events.
    pub fn clear(&mut self) {
        self.events.clear();
        self.next_id = 0;
    }
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Repro Bundle
// ============================================================================

/// Reproduction bundle containing everything needed to reproduce a failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReproBundle {
    /// Initial RNG seed.
    pub seed: u64,

    /// Scenario configuration name.
    pub scenario: String,

    /// VOPR version (for compatibility checking).
    pub vopr_version: String,

    /// Event log (optional - may be omitted for small repros).
    pub event_log: Option<Vec<LoggedEvent>>,

    /// Failure description.
    pub failure: FailureInfo,

    /// Timestamp when bundle was created.
    pub created_at: u64,
}

/// Information about a simulation failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureInfo {
    /// Invariant that was violated.
    pub invariant_name: String,

    /// Failure message.
    pub message: String,

    /// Event number where failure occurred.
    pub failed_at_event: u64,

    /// Simulation time when failure occurred.
    pub failed_at_time_ns: u64,
}

impl ReproBundle {
    /// Creates a new reproduction bundle.
    pub fn new(
        seed: u64,
        scenario: String,
        event_log: Option<Vec<LoggedEvent>>,
        failure: FailureInfo,
    ) -> Self {
        Self {
            seed,
            scenario,
            vopr_version: env!("CARGO_PKG_VERSION").to_string(),
            event_log,
            failure,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Saves the bundle to a .kmb file.
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // Use postcard for compact storage
        let bytes = postcard::to_allocvec(self).map_err(io::Error::other)?;
        writer.write_all(&bytes)?;

        writer.flush()?;
        Ok(())
    }

    /// Loads a bundle from a .kmb file.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        postcard::from_bytes(&bytes).map_err(io::Error::other)
    }

    /// Returns a human-readable summary of this bundle.
    pub fn summary(&self) -> String {
        format!(
            "Seed: {}\nScenario: {}\nInvariant: {}\nMessage: {}\nFailed at event: {}\nTime: {}ns",
            self.seed,
            self.scenario,
            self.failure.invariant_name,
            self.failure.message,
            self.failure.failed_at_event,
            self.failure.failed_at_time_ns
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_log_basic() {
        let mut log = EventLog::new();

        log.log(1000, Decision::RngValue { value: 42 });
        log.log(
            2000,
            Decision::EventScheduled {
                event_type: "test".to_string(),
                scheduled_at_ns: 3000,
            },
        );

        assert_eq!(log.len(), 2);
        assert!(!log.is_empty());
    }

    #[test]
    fn event_log_disabled() {
        let mut log = EventLog::disabled();

        log.log(1000, Decision::RngValue { value: 42 });

        assert_eq!(log.len(), 0);
        assert!(log.is_empty());
    }

    #[test]
    fn event_log_eviction() {
        let mut log = EventLog::new();
        log.max_in_memory = 10;

        for i in 0..20 {
            log.log(i * 1000, Decision::RngValue { value: i });
        }

        assert_eq!(log.len(), 10);
    }

    #[test]
    fn repro_bundle_summary() {
        let bundle = ReproBundle::new(
            12345,
            "test_scenario".to_string(),
            None,
            FailureInfo {
                invariant_name: "test_invariant".to_string(),
                message: "test failure".to_string(),
                failed_at_event: 100,
                failed_at_time_ns: 50000,
            },
        );

        let summary = bundle.summary();
        assert!(summary.contains("12345"));
        assert!(summary.contains("test_scenario"));
        assert!(summary.contains("test_invariant"));
    }

    #[test]
    fn repro_bundle_roundtrip() {
        let bundle = ReproBundle::new(
            12345,
            "test_scenario".to_string(),
            Some(vec![LoggedEvent {
                time_ns: 1000,
                event_id: 0,
                decision: Decision::RngValue { value: 42 },
            }]),
            FailureInfo {
                invariant_name: "test_invariant".to_string(),
                message: "test failure".to_string(),
                failed_at_event: 100,
                failed_at_time_ns: 50000,
            },
        );

        // Serialize to bytes
        let bytes = postcard::to_allocvec(&bundle).unwrap();

        // Deserialize back
        let loaded: ReproBundle = postcard::from_bytes(&bytes).unwrap();

        assert_eq!(loaded.seed, bundle.seed);
        assert_eq!(loaded.scenario, bundle.scenario);
        assert!(loaded.event_log.is_some());
    }
}
