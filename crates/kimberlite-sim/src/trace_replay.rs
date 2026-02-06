//! Trace replay for deterministic simulation reproduction.
//!
//! This module enables replaying a simulation from a recorded trace file
//! instead of just from a seed. This allows perfect reproduction of complex
//! scenarios and bisection of failures to specific decisions.
//!
//! ## Design
//!
//! Traces record:
//! - Scheduler decisions (which node, which event)
//! - Time advances
//! - Timer fires
//! - Fault injection events
//! - Invariant checks
//!
//! Replay mode:
//! - Reads trace from file
//! - Replays decisions in exact order
//! - Verifies outcomes match expected values
//! - Reports any divergence from original run
//!
//! ## Usage
//!
//! ```ignore
//! // Record a trace
//! let mut recorder = TraceRecorder::new();
//! recorder.record_scheduler_decision(0, vec![0, 1, 2]);
//! recorder.save_to_file("trace.bin")?;
//!
//! // Replay the trace
//! let replayer = TraceReplayer::from_file("trace.bin")?;
//! let result = replayer.replay()?;
//! ```

use crate::event_log::{Decision, EventLog, LoggedEvent};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

// ============================================================================
// Trace Replay
// ============================================================================

/// Replays a simulation from a recorded trace.
#[derive(Debug)]
pub struct TraceReplayer {
    /// Recorded events to replay.
    events: Vec<LoggedEvent>,

    /// Current replay position.
    position: usize,

    /// Verification mode (strict vs lenient).
    verification_mode: VerificationMode,
}

/// How strictly to verify replay matches original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationMode {
    /// Strict: every decision must match exactly.
    Strict,

    /// Lenient: allow minor timing variations but verify key decisions.
    Lenient,

    /// NoVerification: just replay, don't verify.
    NoVerification,
}

impl TraceReplayer {
    /// Creates a new trace replayer from events.
    pub fn new(events: Vec<LoggedEvent>) -> Self {
        Self {
            events,
            position: 0,
            verification_mode: VerificationMode::Strict,
        }
    }

    /// Loads a trace from a file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        // Read entire file into buffer
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer)?;

        // Deserialize using postcard
        let events: Vec<LoggedEvent> = postcard::from_bytes(&buffer)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(Self::new(events))
    }

    /// Loads a trace from an EventLog.
    pub fn from_event_log(event_log: &EventLog) -> Self {
        let events: Vec<LoggedEvent> = event_log.iter().cloned().collect();
        Self::new(events)
    }

    /// Sets the verification mode.
    pub fn set_verification_mode(&mut self, mode: VerificationMode) {
        self.verification_mode = mode;
    }

    /// Gets the next decision to replay.
    pub fn next_decision(&mut self) -> Option<&LoggedEvent> {
        if self.position < self.events.len() {
            let event = &self.events[self.position];
            self.position += 1;
            Some(event)
        } else {
            None
        }
    }

    /// Verifies that a decision matches what was recorded.
    ///
    /// Returns Ok(()) if the decision matches, Err with description if not.
    pub fn verify_decision(&self, actual: &Decision, expected: &Decision) -> Result<(), String> {
        match self.verification_mode {
            VerificationMode::NoVerification => Ok(()),
            VerificationMode::Strict => {
                if !decisions_match_strict(actual, expected) {
                    Err(format!(
                        "Decision mismatch:\n  Expected: {:?}\n  Actual: {:?}",
                        expected, actual
                    ))
                } else {
                    Ok(())
                }
            }
            VerificationMode::Lenient => {
                if !decisions_match_lenient(actual, expected) {
                    Err(format!(
                        "Decision mismatch (lenient):\n  Expected: {:?}\n  Actual: {:?}",
                        expected, actual
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Returns the total number of events in the trace.
    pub fn total_events(&self) -> usize {
        self.events.len()
    }

    /// Returns the current replay position.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Returns true if replay is complete.
    pub fn is_complete(&self) -> bool {
        self.position >= self.events.len()
    }

    /// Resets the replay to the beginning.
    pub fn reset(&mut self) {
        self.position = 0;
    }

    /// Returns a summary of the trace.
    pub fn summary(&self) -> TraceSummary {
        let mut summary = TraceSummary::default();

        for event in &self.events {
            summary.total_events += 1;

            match &event.decision {
                Decision::SchedulerNodeSelected { .. } => summary.scheduler_decisions += 1,
                Decision::SchedulerEventDequeued { .. } => summary.event_dequeues += 1,
                Decision::TimeAdvance { .. } => summary.time_advances += 1,
                Decision::TimerFired { .. } => summary.timers_fired += 1,
                Decision::NetworkDelay { .. } => summary.network_delays += 1,
                Decision::NetworkDrop { .. } => summary.network_drops += 1,
                Decision::NodeCrash { .. } => summary.node_crashes += 1,
                Decision::NodeRestart { .. } => summary.node_restarts += 1,
                Decision::InvariantCheck { .. } => summary.invariant_checks += 1,
                _ => summary.other_events += 1,
            }
        }

        summary
    }
}

/// Summary statistics for a trace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceSummary {
    /// Total events in trace.
    pub total_events: usize,

    /// Number of scheduler decisions.
    pub scheduler_decisions: usize,

    /// Number of event dequeues.
    pub event_dequeues: usize,

    /// Number of time advances.
    pub time_advances: usize,

    /// Number of timers fired.
    pub timers_fired: usize,

    /// Number of network delays.
    pub network_delays: usize,

    /// Number of network drops.
    pub network_drops: usize,

    /// Number of node crashes.
    pub node_crashes: usize,

    /// Number of node restarts.
    pub node_restarts: usize,

    /// Number of invariant checks.
    pub invariant_checks: usize,

    /// Other events.
    pub other_events: usize,
}

impl TraceSummary {
    /// Returns a human-readable summary.
    pub fn display(&self) -> String {
        format!(
            "Trace Summary:\n\
             Total events: {}\n\
             Scheduler decisions: {}\n\
             Event dequeues: {}\n\
             Time advances: {}\n\
             Timers fired: {}\n\
             Network delays: {}\n\
             Network drops: {}\n\
             Node crashes: {}\n\
             Node restarts: {}\n\
             Invariant checks: {}\n\
             Other events: {}",
            self.total_events,
            self.scheduler_decisions,
            self.event_dequeues,
            self.time_advances,
            self.timers_fired,
            self.network_delays,
            self.network_drops,
            self.node_crashes,
            self.node_restarts,
            self.invariant_checks,
            self.other_events
        )
    }
}

// ============================================================================
// Trace Recording
// ============================================================================

/// Records a trace for later replay.
#[derive(Debug)]
pub struct TraceRecorder {
    /// Event log being recorded.
    event_log: EventLog,
}

impl TraceRecorder {
    /// Creates a new trace recorder.
    pub fn new() -> Self {
        Self {
            event_log: EventLog::new(),
        }
    }

    /// Records a decision.
    pub fn record(&mut self, time_ns: u64, decision: Decision) {
        self.event_log.log(time_ns, decision);
    }

    /// Saves the recorded trace to a file.
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // Collect all events
        let events: Vec<LoggedEvent> = self.event_log.iter().cloned().collect();

        // Serialize using postcard
        let bytes = postcard::to_allocvec(&events)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        writer.write_all(&bytes)?;
        writer.flush()?;

        Ok(())
    }

    /// Returns the event log.
    pub fn event_log(&self) -> &EventLog {
        &self.event_log
    }

    /// Returns the number of recorded events.
    pub fn len(&self) -> usize {
        self.event_log.len()
    }

    /// Returns true if no events have been recorded.
    pub fn is_empty(&self) -> bool {
        self.event_log.is_empty()
    }
}

impl Default for TraceRecorder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Decision Matching
// ============================================================================

/// Checks if two decisions match strictly (all fields equal).
fn decisions_match_strict(a: &Decision, b: &Decision) -> bool {
    // For simplicity, use Debug formatting comparison
    // In production, you'd want field-by-field comparison
    format!("{:?}", a) == format!("{:?}", b)
}

/// Checks if two decisions match leniently (key fields equal, timing can vary).
fn decisions_match_lenient(a: &Decision, b: &Decision) -> bool {
    match (a, b) {
        (
            Decision::SchedulerNodeSelected {
                node_id: node_a,
                runnable_count: _,
            },
            Decision::SchedulerNodeSelected {
                node_id: node_b,
                runnable_count: _,
            },
        ) => node_a == node_b, // Ignore runnable count in lenient mode

        (
            Decision::NetworkDelay {
                message_id: id_a, ..
            },
            Decision::NetworkDelay {
                message_id: id_b, ..
            },
        ) => id_a == id_b, // Ignore delay_ns in lenient mode

        (
            Decision::NetworkDrop { message_id: id_a },
            Decision::NetworkDrop { message_id: id_b },
        ) => id_a == id_b,

        (Decision::NodeCrash { node_id: id_a }, Decision::NodeCrash { node_id: id_b }) => {
            id_a == id_b
        }

        (Decision::NodeRestart { node_id: id_a }, Decision::NodeRestart { node_id: id_b }) => {
            id_a == id_b
        }

        // For all other cases, fall back to strict matching
        _ => decisions_match_strict(a, b),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_recorder_basic() {
        let mut recorder = TraceRecorder::new();

        recorder.record(
            1000,
            Decision::SchedulerNodeSelected {
                node_id: 0,
                runnable_count: 3,
            },
        );
        recorder.record(
            2000,
            Decision::TimeAdvance {
                from_ns: 1000,
                to_ns: 2000,
                delta_ns: 1000,
            },
        );

        assert_eq!(recorder.len(), 2);

        let replayer = TraceReplayer::from_event_log(recorder.event_log());
        assert_eq!(replayer.total_events(), 2);
    }

    #[test]
    fn test_trace_replay_basic() {
        let mut recorder = TraceRecorder::new();

        recorder.record(
            1000,
            Decision::SchedulerNodeSelected {
                node_id: 0,
                runnable_count: 3,
            },
        );
        recorder.record(
            2000,
            Decision::SchedulerNodeSelected {
                node_id: 1,
                runnable_count: 3,
            },
        );

        let mut replayer = TraceReplayer::from_event_log(recorder.event_log());

        // Replay first decision
        let event1 = replayer.next_decision().unwrap();
        assert_eq!(event1.time_ns, 1000);
        match &event1.decision {
            Decision::SchedulerNodeSelected { node_id, .. } => assert_eq!(*node_id, 0),
            _ => panic!("Wrong decision type"),
        }

        // Replay second decision
        let event2 = replayer.next_decision().unwrap();
        assert_eq!(event2.time_ns, 2000);
        match &event2.decision {
            Decision::SchedulerNodeSelected { node_id, .. } => assert_eq!(*node_id, 1),
            _ => panic!("Wrong decision type"),
        }

        // No more decisions
        assert!(replayer.next_decision().is_none());
        assert!(replayer.is_complete());
    }

    #[test]
    fn test_trace_summary() {
        let mut recorder = TraceRecorder::new();

        recorder.record(
            1000,
            Decision::SchedulerNodeSelected {
                node_id: 0,
                runnable_count: 3,
            },
        );
        recorder.record(
            2000,
            Decision::TimeAdvance {
                from_ns: 1000,
                to_ns: 2000,
                delta_ns: 1000,
            },
        );
        recorder.record(3000, Decision::NetworkDrop { message_id: 42 });

        let replayer = TraceReplayer::from_event_log(recorder.event_log());
        let summary = replayer.summary();

        assert_eq!(summary.total_events, 3);
        assert_eq!(summary.scheduler_decisions, 1);
        assert_eq!(summary.time_advances, 1);
        assert_eq!(summary.network_drops, 1);

        println!("{}", summary.display());
    }

    #[test]
    fn test_strict_verification() {
        let decision1 = Decision::SchedulerNodeSelected {
            node_id: 0,
            runnable_count: 3,
        };
        let decision2 = Decision::SchedulerNodeSelected {
            node_id: 0,
            runnable_count: 2, // Different runnable count
        };

        let replayer = TraceReplayer::new(vec![]);

        // Strict mode should fail
        let result = replayer.verify_decision(&decision1, &decision2);
        assert!(result.is_err());
    }

    #[test]
    fn test_lenient_verification() {
        let decision1 = Decision::SchedulerNodeSelected {
            node_id: 0,
            runnable_count: 3,
        };
        let decision2 = Decision::SchedulerNodeSelected {
            node_id: 0,
            runnable_count: 2, // Different runnable count
        };

        let mut replayer = TraceReplayer::new(vec![]);
        replayer.set_verification_mode(VerificationMode::Lenient);

        // Lenient mode should pass (same node_id)
        let result = replayer.verify_decision(&decision1, &decision2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_reset_replay() {
        let mut recorder = TraceRecorder::new();

        for i in 0..5 {
            recorder.record(
                i * 1000,
                Decision::SchedulerNodeSelected {
                    node_id: i,
                    runnable_count: 3,
                },
            );
        }

        let mut replayer = TraceReplayer::from_event_log(recorder.event_log());

        // Replay all
        while replayer.next_decision().is_some() {}
        assert!(replayer.is_complete());

        // Reset and replay again
        replayer.reset();
        assert_eq!(replayer.position(), 0);
        assert!(!replayer.is_complete());

        let event = replayer.next_decision().unwrap();
        assert_eq!(event.event_id, 0);
    }
}
