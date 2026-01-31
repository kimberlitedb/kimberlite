//! Automated failure diagnosis and debugging aids.
//!
//! This module provides tools for analyzing simulation failures and
//! generating minimal reproduction cases.

use serde::{Deserialize, Serialize};

use crate::trace::{TraceEvent, TraceEventType};

// ============================================================================
// Failure Report
// ============================================================================

/// Comprehensive report of a simulation failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureReport {
    /// Seed that triggered the failure.
    pub seed: u64,
    /// Invariant that was violated.
    pub invariant: String,
    /// Violation message.
    pub message: String,
    /// Time of violation (nanoseconds).
    pub violation_time_ns: u64,
    /// Event number when violation occurred.
    pub violation_event: u64,
    /// Total events before failure.
    pub total_events: u64,
    /// Classification of the failure.
    pub classification: FailureClassification,
    /// Events leading up to the failure.
    pub context_events: Vec<ContextEvent>,
    /// Relevant state at time of failure.
    pub state_snapshot: StateSnapshot,
    /// Minimal reproduction steps (if computed).
    pub reproduction: Option<MinimalReproduction>,
    /// Suggested diagnosis.
    pub diagnosis: Option<String>,
}

/// Classification of failure types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FailureClassification {
    /// Data corruption or incorrect value.
    DataCorruption,
    /// Replica divergence (consistency issue).
    ReplicaDivergence,
    /// Linearizability violation.
    LinearizabilityViolation,
    /// Operation ordering violation.
    OrderingViolation,
    /// Hash chain integrity failure.
    HashChainFailure,
    /// Client session violation.
    SessionViolation,
    /// Storage determinism issue.
    StorageDeterminismFailure,
    /// Other or unknown.
    Unknown,
}

impl FailureClassification {
    /// Classifies a failure based on the invariant name.
    pub fn from_invariant(invariant: &str) -> Self {
        match invariant {
            "model_verification" => Self::DataCorruption,
            "replica_consistency" => Self::ReplicaDivergence,
            "linearizability" => Self::LinearizabilityViolation,
            "commit_history_monotonic" | "commit_history_starts_at_zero" => {
                Self::OrderingViolation
            }
            "hash_chain_linkage" | "hash_chain_genesis" | "hash_chain_offset_monotonic" => {
                Self::HashChainFailure
            }
            "client_session_monotonic"
            | "client_session_idempotent"
            | "client_session_no_gaps" => Self::SessionViolation,
            "storage_determinism" => Self::StorageDeterminismFailure,
            _ => Self::Unknown,
        }
    }

    /// Returns a human-readable description.
    pub fn description(&self) -> &'static str {
        match self {
            Self::DataCorruption => {
                "Data read does not match expected value (model mismatch)"
            }
            Self::ReplicaDivergence => "Replicas at same position have different content",
            Self::LinearizabilityViolation => "Operation history cannot be linearized",
            Self::OrderingViolation => "Operations committed out of order or with gaps",
            Self::HashChainFailure => "Hash chain integrity compromised",
            Self::SessionViolation => "Client session semantics violated",
            Self::StorageDeterminismFailure => "Storage state is non-deterministic",
            Self::Unknown => "Unclassified failure",
        }
    }

    /// Suggests possible root causes.
    pub fn possible_causes(&self) -> Vec<&'static str> {
        match self {
            Self::DataCorruption => vec![
                "Storage corruption not detected",
                "Write/read race condition",
                "Partial write accepted as complete",
                "Incorrect model state tracking",
            ],
            Self::ReplicaDivergence => vec![
                "Non-deterministic storage operations",
                "Replication bug (missing/reordered entries)",
                "Storage corruption on one replica",
                "Byzantine failure injection bug",
            ],
            Self::LinearizabilityViolation => vec![
                "Concurrent operations mishandled",
                "Read seeing stale data after write",
                "Operation ordering bug",
                "Happens-before relationship violated",
            ],
            Self::OrderingViolation => vec![
                "Commit tracking bug",
                "Skipped operation number",
                "Duplicate commit detection failed",
                "State machine transition bug",
            ],
            Self::HashChainFailure => vec![
                "Hash chain computation bug",
                "Offset tracking error",
                "Genesis record malformed",
                "Hash linkage corrupted",
            ],
            Self::SessionViolation => vec![
                "Request number tracking bug",
                "Retry handling incorrect",
                "Session state not persisted",
                "Idempotency logic broken",
            ],
            Self::StorageDeterminismFailure => vec![
                "Non-deterministic compaction",
                "LSM tree merge ordering",
                "Timestamp usage in storage",
                "Hash algorithm inconsistency",
            ],
            Self::Unknown => vec!["Unknown root cause - needs investigation"],
        }
    }
}

/// Simplified event for context in failure reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEvent {
    /// Sequence number.
    pub seq: u64,
    /// Simulation time.
    pub time_ns: u64,
    /// Event description.
    pub description: String,
    /// Whether this event seems relevant to the failure.
    pub is_relevant: bool,
}

/// Snapshot of relevant state at failure time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Number of replicas tracked.
    pub replica_count: u64,
    /// Operations completed.
    pub operations_completed: u64,
    /// Last few operations.
    pub recent_operations: Vec<String>,
    /// Network statistics.
    pub network_stats: NetworkStateSnapshot,
    /// Storage statistics.
    pub storage_stats: StorageStateSnapshot,
}

/// Network state snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStateSnapshot {
    /// Messages sent.
    pub messages_sent: u64,
    /// Messages delivered.
    pub messages_delivered: u64,
    /// Messages dropped.
    pub messages_dropped: u64,
    /// Active partitions.
    pub active_partitions: u64,
}

/// Storage state snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStateSnapshot {
    /// Writes attempted.
    pub writes: u64,
    /// Writes succeeded.
    pub writes_successful: u64,
    /// Reads attempted.
    pub reads: u64,
    /// Reads succeeded.
    pub reads_successful: u64,
    /// Fsyncs performed.
    pub fsyncs: u64,
}

/// Minimal reproduction case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinimalReproduction {
    /// Seed to reproduce.
    pub seed: u64,
    /// Minimum number of events needed.
    pub min_events: u64,
    /// Simplified event sequence.
    pub event_sequence: Vec<String>,
    /// Commands to reproduce.
    pub commands: Vec<String>,
}

// ============================================================================
// Failure Analyzer
// ============================================================================

/// Analyzes simulation failures and generates reports.
pub struct FailureAnalyzer;

impl FailureAnalyzer {
    /// Analyzes a failure and generates a comprehensive report.
    pub fn analyze_failure(
        seed: u64,
        events: &[TraceEvent],
        total_events: u64,
    ) -> FailureReport {
        // Find the violation event
        let violation = events
            .iter()
            .find(|e| matches!(e.event_type, TraceEventType::InvariantViolation { .. }));

        let (invariant, message, violation_time_ns, violation_event) = if let Some(v) = violation {
            if let TraceEventType::InvariantViolation {
                invariant,
                message,
                ..
            } = &v.event_type
            {
                (
                    invariant.clone(),
                    message.clone(),
                    v.time_ns,
                    v.seq,
                )
            } else {
                (
                    "unknown".to_string(),
                    "unknown".to_string(),
                    0,
                    0,
                )
            }
        } else {
            (
                "unknown".to_string(),
                "no violation found".to_string(),
                0,
                0,
            )
        };

        let classification = FailureClassification::from_invariant(&invariant);

        // Extract context events (last N events before violation)
        let context_events = Self::extract_context_events(events, &classification);

        // Build state snapshot
        let state_snapshot = Self::build_state_snapshot(events);

        // Generate diagnosis
        let diagnosis = Self::generate_diagnosis(&classification, &context_events, events);

        // Generate minimal reproduction
        let reproduction = Self::generate_reproduction(seed, &context_events, total_events);

        FailureReport {
            seed,
            invariant,
            message,
            violation_time_ns,
            violation_event,
            total_events,
            classification,
            context_events,
            state_snapshot,
            reproduction: Some(reproduction),
            diagnosis: Some(diagnosis),
        }
    }

    /// Extracts relevant context events before the failure.
    fn extract_context_events(
        events: &[TraceEvent],
        classification: &FailureClassification,
    ) -> Vec<ContextEvent> {
        let lookback = 20.min(events.len());
        let start_idx = events.len().saturating_sub(lookback);

        events[start_idx..]
            .iter()
            .map(|e| {
                let description = Self::describe_event(&e.event_type);
                let is_relevant = Self::is_event_relevant(&e.event_type, classification);

                ContextEvent {
                    seq: e.seq,
                    time_ns: e.time_ns,
                    description,
                    is_relevant,
                }
            })
            .collect()
    }

    /// Describes an event in human-readable form.
    fn describe_event(event_type: &TraceEventType) -> String {
        match event_type {
            TraceEventType::Write { key, value, success, .. } => {
                format!("Write key={key} value={value} success={success}")
            }
            TraceEventType::Read { key, value, success } => {
                format!("Read key={key} value={value:?} success={success}")
            }
            TraceEventType::ReadModifyWrite { key, old_value, new_value, success } => {
                format!("RMW key={key} old={old_value:?} new={new_value} success={success}")
            }
            TraceEventType::Scan { start_key, end_key, count, success } => {
                format!("Scan [{start_key}, {end_key}) count={count} success={success}")
            }
            TraceEventType::ReplicaUpdate { replica_id, view, op, log_length } => {
                format!("Replica {replica_id} update view={view} op={op} log_length={log_length}")
            }
            TraceEventType::InvariantViolation { invariant, message, .. } => {
                format!("VIOLATION: {invariant} - {message}")
            }
            TraceEventType::CheckpointCreate { checkpoint_id, blocks, size_bytes } => {
                format!("Checkpoint {checkpoint_id} created ({blocks} blocks, {size_bytes} bytes)")
            }
            TraceEventType::Fsync { success, latency_ns } => {
                format!("Fsync success={success} latency={latency_ns}ns")
            }
            _ => format!("{:?}", event_type),
        }
    }

    /// Determines if an event is relevant to the failure classification.
    fn is_event_relevant(event_type: &TraceEventType, classification: &FailureClassification) -> bool {
        match classification {
            FailureClassification::DataCorruption => matches!(
                event_type,
                TraceEventType::Write { .. } | TraceEventType::Read { .. }
            ),
            FailureClassification::ReplicaDivergence => {
                matches!(event_type, TraceEventType::ReplicaUpdate { .. })
            }
            FailureClassification::LinearizabilityViolation => matches!(
                event_type,
                TraceEventType::Write { .. }
                    | TraceEventType::Read { .. }
                    | TraceEventType::ReadModifyWrite { .. }
            ),
            FailureClassification::OrderingViolation => {
                matches!(event_type, TraceEventType::ReplicaUpdate { .. })
            }
            _ => false,
        }
    }

    /// Builds a snapshot of state at failure time.
    fn build_state_snapshot(events: &[TraceEvent]) -> StateSnapshot {
        let mut replica_ids = std::collections::HashSet::new();
        let mut operations = Vec::new();
        let mut network_stats = NetworkStateSnapshot {
            messages_sent: 0,
            messages_delivered: 0,
            messages_dropped: 0,
            active_partitions: 0,
        };
        let mut storage_stats = StorageStateSnapshot {
            writes: 0,
            writes_successful: 0,
            reads: 0,
            reads_successful: 0,
            fsyncs: 0,
        };

        for event in events {
            match &event.event_type {
                TraceEventType::ReplicaUpdate { replica_id, .. } => {
                    replica_ids.insert(*replica_id);
                }
                TraceEventType::Write { success, .. } => {
                    storage_stats.writes += 1;
                    if *success {
                        storage_stats.writes_successful += 1;
                    }
                    operations.push(format!("Write @ {}", event.time_ns));
                }
                TraceEventType::Read { success, .. } => {
                    storage_stats.reads += 1;
                    if *success {
                        storage_stats.reads_successful += 1;
                    }
                    operations.push(format!("Read @ {}", event.time_ns));
                }
                TraceEventType::Fsync { .. } => {
                    storage_stats.fsyncs += 1;
                }
                TraceEventType::NetworkSend { .. } => {
                    network_stats.messages_sent += 1;
                }
                TraceEventType::NetworkDeliver { .. } => {
                    network_stats.messages_delivered += 1;
                }
                TraceEventType::NetworkDrop { .. } => {
                    network_stats.messages_dropped += 1;
                }
                TraceEventType::NetworkPartition { .. } => {
                    network_stats.active_partitions += 1;
                }
                _ => {}
            }
        }

        // Keep only last 10 operations
        let recent_operations = operations
            .into_iter()
            .rev()
            .take(10)
            .rev()
            .collect();

        StateSnapshot {
            replica_count: replica_ids.len() as u64,
            operations_completed: storage_stats.writes_successful + storage_stats.reads_successful,
            recent_operations,
            network_stats,
            storage_stats,
        }
    }

    /// Generates a diagnosis based on the failure classification and context.
    fn generate_diagnosis(
        classification: &FailureClassification,
        context_events: &[ContextEvent],
        _all_events: &[TraceEvent],
    ) -> String {
        let mut diagnosis = String::new();

        diagnosis.push_str(&format!(
            "Failure Classification: {:?}\n\n",
            classification
        ));
        diagnosis.push_str(&format!("Description: {}\n\n", classification.description()));

        diagnosis.push_str("Possible Root Causes:\n");
        for (i, cause) in classification.possible_causes().iter().enumerate() {
            diagnosis.push_str(&format!("  {}. {}\n", i + 1, cause));
        }

        diagnosis.push_str("\nRelevant Events Before Failure:\n");
        let relevant: Vec<_> = context_events
            .iter()
            .filter(|e| e.is_relevant)
            .collect();

        if relevant.is_empty() {
            diagnosis.push_str("  (No particularly relevant events found)\n");
        } else {
            for event in relevant.iter().take(5) {
                diagnosis.push_str(&format!(
                    "  [{}] @ {}ns: {}\n",
                    event.seq, event.time_ns, event.description
                ));
            }
        }

        diagnosis
    }

    /// Generates a minimal reproduction case.
    fn generate_reproduction(
        seed: u64,
        context_events: &[ContextEvent],
        total_events: u64,
    ) -> MinimalReproduction {
        // Simplify event sequence
        let event_sequence: Vec<String> = context_events
            .iter()
            .filter(|e| e.is_relevant)
            .map(|e| e.description.clone())
            .collect();

        let commands = vec![
            format!("vopr --seed {seed} -v"),
            format!("vopr --seed {seed} --max-events {total_events}"),
        ];

        MinimalReproduction {
            seed,
            min_events: total_events,
            event_sequence,
            commands,
        }
    }

    /// Formats a failure report as human-readable text.
    pub fn format_report(report: &FailureReport) -> String {
        let mut output = String::new();

        output.push_str("═══════════════════════════════════════════════════════\n");
        output.push_str("           SIMULATION FAILURE REPORT\n");
        output.push_str("═══════════════════════════════════════════════════════\n\n");

        output.push_str(&format!("Seed: {}\n", report.seed));
        output.push_str(&format!("Invariant: {}\n", report.invariant));
        output.push_str(&format!("Message: {}\n", report.message));
        output.push_str(&format!(
            "Time: {}ns ({}ms)\n",
            report.violation_time_ns,
            report.violation_time_ns / 1_000_000
        ));
        output.push_str(&format!(
            "Event: {} / {}\n\n",
            report.violation_event, report.total_events
        ));

        output.push_str("───────────────────────────────────────────────────────\n");
        output.push_str("Classification\n");
        output.push_str("───────────────────────────────────────────────────────\n");
        output.push_str(&format!("Type: {:?}\n", report.classification));
        output.push_str(&format!("Description: {}\n\n", report.classification.description()));

        if let Some(diagnosis) = &report.diagnosis {
            output.push_str("───────────────────────────────────────────────────────\n");
            output.push_str("Diagnosis\n");
            output.push_str("───────────────────────────────────────────────────────\n");
            output.push_str(diagnosis);
            output.push('\n');
        }

        output.push_str("───────────────────────────────────────────────────────\n");
        output.push_str("State Snapshot\n");
        output.push_str("───────────────────────────────────────────────────────\n");
        output.push_str(&format!("Replicas: {}\n", report.state_snapshot.replica_count));
        output.push_str(&format!(
            "Operations completed: {}\n",
            report.state_snapshot.operations_completed
        ));
        output.push_str(&format!(
            "Network: {} sent, {} delivered, {} dropped\n",
            report.state_snapshot.network_stats.messages_sent,
            report.state_snapshot.network_stats.messages_delivered,
            report.state_snapshot.network_stats.messages_dropped
        ));
        output.push_str(&format!(
            "Storage: {}/{} writes, {}/{} reads\n\n",
            report.state_snapshot.storage_stats.writes_successful,
            report.state_snapshot.storage_stats.writes,
            report.state_snapshot.storage_stats.reads_successful,
            report.state_snapshot.storage_stats.reads
        ));

        if let Some(repro) = &report.reproduction {
            output.push_str("───────────────────────────────────────────────────────\n");
            output.push_str("Reproduction\n");
            output.push_str("───────────────────────────────────────────────────────\n");
            output.push_str(&format!("Seed: {}\n", repro.seed));
            output.push_str(&format!("Min events: {}\n\n", repro.min_events));
            output.push_str("Commands:\n");
            for cmd in &repro.commands {
                output.push_str(&format!("  {}\n", cmd));
            }
            output.push('\n');
        }

        output.push_str("═══════════════════════════════════════════════════════\n");

        output
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::TraceCollector;

    #[test]
    fn failure_classification_from_invariant() {
        assert_eq!(
            FailureClassification::from_invariant("model_verification"),
            FailureClassification::DataCorruption
        );
        assert_eq!(
            FailureClassification::from_invariant("replica_consistency"),
            FailureClassification::ReplicaDivergence
        );
        assert_eq!(
            FailureClassification::from_invariant("linearizability"),
            FailureClassification::LinearizabilityViolation
        );
        assert_eq!(
            FailureClassification::from_invariant("unknown"),
            FailureClassification::Unknown
        );
    }

    #[test]
    fn failure_analyzer_generates_report() {
        let mut collector = TraceCollector::default_config();

        collector.record(1000, TraceEventType::SimulationStart { seed: 42 });
        collector.record(
            2000,
            TraceEventType::Write {
                key: 1,
                value: 100,
                success: true,
                bytes_written: Some(8),
            },
        );
        collector.record(
            3000,
            TraceEventType::InvariantViolation {
                invariant: "model_verification".to_string(),
                message: "data mismatch".to_string(),
                context: vec![],
            },
        );

        let events: Vec<_> = collector.events().iter().cloned().collect();
        let report = FailureAnalyzer::analyze_failure(42, &events, 3);

        assert_eq!(report.seed, 42);
        assert_eq!(report.invariant, "model_verification");
        assert_eq!(
            report.classification,
            FailureClassification::DataCorruption
        );
        assert!(report.diagnosis.is_some());
        assert!(report.reproduction.is_some());
    }

    #[test]
    fn failure_report_formatting() {
        let report = FailureReport {
            seed: 12345,
            invariant: "test".to_string(),
            message: "test violation".to_string(),
            violation_time_ns: 5_000_000_000,
            violation_event: 100,
            total_events: 150,
            classification: FailureClassification::DataCorruption,
            context_events: vec![],
            state_snapshot: StateSnapshot {
                replica_count: 3,
                operations_completed: 50,
                recent_operations: vec![],
                network_stats: NetworkStateSnapshot {
                    messages_sent: 10,
                    messages_delivered: 8,
                    messages_dropped: 2,
                    active_partitions: 0,
                },
                storage_stats: StorageStateSnapshot {
                    writes: 30,
                    writes_successful: 28,
                    reads: 25,
                    reads_successful: 24,
                    fsyncs: 5,
                },
            },
            reproduction: None,
            diagnosis: None,
        };

        let formatted = FailureAnalyzer::format_report(&report);
        assert!(formatted.contains("12345"));
        assert!(formatted.contains("test violation"));
        assert!(formatted.contains("DataCorruption"));
    }
}
