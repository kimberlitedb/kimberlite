//! Trace event collection for post-mortem analysis.
//!
//! The trace system captures all significant events during simulation,
//! enabling detailed debugging and analysis of failures.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

// ============================================================================
// Trace Event Types
// ============================================================================

/// A traced event in the simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    /// Sequential event number.
    pub seq: u64,
    /// Simulation time when event occurred (nanoseconds).
    pub time_ns: u64,
    /// Wall clock timestamp when event was recorded.
    pub wall_clock_ms: u64,
    /// Type of event.
    pub event_type: TraceEventType,
    /// Optional metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TraceMetadata>,
}

/// Types of events that can be traced.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TraceEventType {
    /// Simulation started.
    SimulationStart { seed: u64 },
    /// Simulation ended.
    SimulationEnd { events_processed: u64 },
    /// Write operation.
    Write {
        key: u64,
        value: u64,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        bytes_written: Option<usize>,
    },
    /// Read operation.
    Read {
        key: u64,
        value: Option<u64>,
        success: bool,
    },
    /// Read-modify-write operation.
    ReadModifyWrite {
        key: u64,
        old_value: Option<u64>,
        new_value: u64,
        success: bool,
    },
    /// Scan operation.
    Scan {
        start_key: u64,
        end_key: u64,
        count: usize,
        success: bool,
    },
    /// Network message sent.
    NetworkSend {
        from: u64,
        to: u64,
        message_id: u64,
        size_bytes: usize,
    },
    /// Network message delivered.
    NetworkDeliver {
        from: u64,
        to: u64,
        message_id: u64,
        delay_ns: u64,
    },
    /// Network message dropped.
    NetworkDrop {
        from: u64,
        to: u64,
        message_id: u64,
        reason: String,
    },
    /// Storage fsync operation.
    Fsync { success: bool, latency_ns: u64 },
    /// Replica state update.
    ReplicaUpdate {
        replica_id: u64,
        view: u32,
        op: u64,
        log_length: u64,
    },
    /// Checkpoint created.
    CheckpointCreate {
        checkpoint_id: u64,
        blocks: usize,
        size_bytes: u64,
    },
    /// Checkpoint recovered.
    CheckpointRecover {
        checkpoint_id: u64,
        success: bool,
    },
    /// Invariant check.
    InvariantCheck { invariant: String, passed: bool },
    /// Invariant violation detected.
    InvariantViolation {
        invariant: String,
        message: String,
        context: Vec<(String, String)>,
    },
    /// Node crash.
    NodeCrash { node_id: u64 },
    /// Node restart.
    NodeRestart { node_id: u64 },
    /// Network partition.
    NetworkPartition {
        partition_id: u64,
        affected_nodes: Vec<u64>,
    },
    /// Network heal.
    NetworkHeal { partition_id: u64 },
    /// Storage fault injected.
    StorageFault { fault_type: String, target: String },
    /// Custom event.
    Custom { name: String, data: String },
}

/// Optional metadata for trace events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMetadata {
    /// Thread/replica ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replica_id: Option<u64>,
    /// Client ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<u64>,
    /// Operation ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<u64>,
    /// Additional key-value pairs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Vec<(String, String)>>,
}

// ============================================================================
// Trace Collector
// ============================================================================

/// Configuration for trace collection.
#[derive(Debug, Clone)]
pub struct TraceConfig {
    /// Maximum number of events to keep in memory.
    pub max_events: usize,
    /// Whether to include successful operations (vs failures only).
    pub include_success: bool,
    /// Whether to include read operations.
    pub include_reads: bool,
    /// Whether to include network events.
    pub include_network: bool,
    /// Whether to collect wall clock timestamps.
    pub include_wall_clock: bool,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            max_events: 10_000,
            include_success: true,
            include_reads: true,
            include_network: true,
            include_wall_clock: true,
        }
    }
}

impl TraceConfig {
    /// Creates a config that only traces failures.
    pub fn failures_only() -> Self {
        Self {
            include_success: false,
            include_reads: false,
            include_network: false,
            ..Self::default()
        }
    }

    /// Creates a config optimized for minimal overhead.
    pub fn minimal() -> Self {
        Self {
            max_events: 1_000,
            include_success: false,
            include_reads: false,
            include_network: false,
            include_wall_clock: false,
        }
    }

    /// Creates a config that captures everything.
    pub fn verbose() -> Self {
        Self {
            max_events: 100_000,
            include_success: true,
            include_reads: true,
            include_network: true,
            include_wall_clock: true,
        }
    }
}

/// Collects trace events during simulation.
///
/// Uses a circular buffer to limit memory usage.
#[derive(Debug)]
pub struct TraceCollector {
    /// Configuration.
    config: TraceConfig,
    /// Buffered events (circular buffer).
    events: VecDeque<TraceEvent>,
    /// Next sequence number.
    next_seq: u64,
    /// Start time for wall clock offsets.
    start_time_ms: u64,
    /// Statistics.
    stats: TraceStats,
}

/// Statistics about trace collection.
#[derive(Debug, Clone, Default)]
pub struct TraceStats {
    /// Total events captured.
    pub total_events: u64,
    /// Events dropped due to buffer limit.
    pub events_dropped: u64,
    /// Events filtered out by config.
    pub events_filtered: u64,
}

impl TraceCollector {
    /// Creates a new trace collector with the given configuration.
    pub fn new(config: TraceConfig) -> Self {
        Self {
            events: VecDeque::with_capacity(config.max_events),
            config,
            next_seq: 0,
            start_time_ms: Self::wall_clock_ms(),
            stats: TraceStats::default(),
        }
    }

    /// Creates a collector with default configuration.
    pub fn default_config() -> Self {
        Self::new(TraceConfig::default())
    }

    /// Records a trace event.
    pub fn record(&mut self, time_ns: u64, event_type: TraceEventType) {
        self.record_with_metadata(time_ns, event_type, None);
    }

    /// Records a trace event with metadata.
    pub fn record_with_metadata(
        &mut self,
        time_ns: u64,
        event_type: TraceEventType,
        metadata: Option<TraceMetadata>,
    ) {
        // Apply filters
        if self.should_filter(&event_type) {
            self.stats.events_filtered += 1;
            return;
        }

        let wall_clock_ms = if self.config.include_wall_clock {
            Self::wall_clock_ms() - self.start_time_ms
        } else {
            0
        };

        let event = TraceEvent {
            seq: self.next_seq,
            time_ns,
            wall_clock_ms,
            event_type,
            metadata,
        };

        self.next_seq += 1;
        self.stats.total_events += 1;

        // Manage circular buffer
        if self.events.len() >= self.config.max_events {
            self.events.pop_front();
            self.stats.events_dropped += 1;
        }

        self.events.push_back(event);
    }

    /// Checks if an event should be filtered out.
    fn should_filter(&self, event_type: &TraceEventType) -> bool {
        match event_type {
            TraceEventType::Read { success, .. } => {
                !self.config.include_reads || (!self.config.include_success && *success)
            }
            TraceEventType::Write { success, .. }
            | TraceEventType::ReadModifyWrite { success, .. }
            | TraceEventType::Scan { success, .. } => !self.config.include_success && *success,
            TraceEventType::NetworkSend { .. }
            | TraceEventType::NetworkDeliver { .. }
            | TraceEventType::NetworkDrop { .. } => !self.config.include_network,
            TraceEventType::InvariantCheck { passed, .. } => {
                !self.config.include_success && *passed
            }
            _ => false,
        }
    }

    /// Returns all collected events.
    pub fn events(&self) -> &VecDeque<TraceEvent> {
        &self.events
    }

    /// Returns statistics about trace collection.
    pub fn stats(&self) -> &TraceStats {
        &self.stats
    }

    /// Clears all collected events.
    pub fn clear(&mut self) {
        self.events.clear();
        self.next_seq = 0;
        self.stats = TraceStats::default();
    }

    /// Exports events as JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.events)
    }

    /// Exports events as newline-delimited JSON.
    pub fn to_ndjson(&self) -> Result<String, serde_json::Error> {
        let mut output = String::new();
        for event in &self.events {
            let line = serde_json::to_string(event)?;
            output.push_str(&line);
            output.push('\n');
        }
        Ok(output)
    }

    /// Filters events by time range.
    pub fn filter_by_time(&self, start_ns: u64, end_ns: u64) -> Vec<&TraceEvent> {
        self.events
            .iter()
            .filter(|e| e.time_ns >= start_ns && e.time_ns <= end_ns)
            .collect()
    }

    /// Filters events by type.
    pub fn filter_by_type<F>(&self, predicate: F) -> Vec<&TraceEvent>
    where
        F: Fn(&TraceEventType) -> bool,
    {
        self.events
            .iter()
            .filter(|e| predicate(&e.event_type))
            .collect()
    }

    /// Gets events before an invariant violation.
    pub fn events_before_violation(&self, lookback_count: usize) -> Vec<&TraceEvent> {
        if let Some(violation_idx) = self
            .events
            .iter()
            .position(|e| matches!(e.event_type, TraceEventType::InvariantViolation { .. }))
        {
            let start_idx = violation_idx.saturating_sub(lookback_count);
            self.events
                .range(start_idx..=violation_idx)
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Gets the current wall clock time in milliseconds.
    fn wall_clock_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_millis() as u64
    }
}

impl Default for TraceCollector {
    fn default() -> Self {
        Self::default_config()
    }
}

// ============================================================================
// Trace Analysis Helpers
// ============================================================================

/// Analyzes a trace to find patterns.
pub struct TraceAnalyzer<'a> {
    events: &'a VecDeque<TraceEvent>,
}

impl<'a> TraceAnalyzer<'a> {
    /// Creates a new analyzer for the given events.
    pub fn new(events: &'a VecDeque<TraceEvent>) -> Self {
        Self { events }
    }

    /// Finds all invariant violations.
    pub fn find_violations(&self) -> Vec<&TraceEvent> {
        self.events
            .iter()
            .filter(|e| matches!(e.event_type, TraceEventType::InvariantViolation { .. }))
            .collect()
    }

    /// Finds all failed operations.
    pub fn find_failures(&self) -> Vec<&TraceEvent> {
        self.events
            .iter()
            .filter(|e| match &e.event_type {
                TraceEventType::Write { success, .. }
                | TraceEventType::Read { success, .. }
                | TraceEventType::ReadModifyWrite { success, .. }
                | TraceEventType::Scan { success, .. } => !success,
                TraceEventType::NetworkDrop { .. } => true,
                TraceEventType::CheckpointRecover { success, .. } => !success,
                _ => false,
            })
            .collect()
    }

    /// Computes operation statistics.
    pub fn operation_stats(&self) -> OperationStats {
        let mut stats = OperationStats::default();

        for event in self.events {
            match &event.event_type {
                TraceEventType::Write { success, .. } => {
                    stats.write_count += 1;
                    if *success {
                        stats.write_success += 1;
                    }
                }
                TraceEventType::Read { success, .. } => {
                    stats.read_count += 1;
                    if *success {
                        stats.read_success += 1;
                    }
                }
                TraceEventType::ReadModifyWrite { success, .. } => {
                    stats.rmw_count += 1;
                    if *success {
                        stats.rmw_success += 1;
                    }
                }
                TraceEventType::Scan { success, .. } => {
                    stats.scan_count += 1;
                    if *success {
                        stats.scan_success += 1;
                    }
                }
                _ => {}
            }
        }

        stats
    }

    /// Gets the time range of events.
    pub fn time_range(&self) -> Option<(u64, u64)> {
        if self.events.is_empty() {
            None
        } else {
            let min = self.events.iter().map(|e| e.time_ns).min().unwrap();
            let max = self.events.iter().map(|e| e.time_ns).max().unwrap();
            Some((min, max))
        }
    }
}

/// Statistics about operations in a trace.
#[derive(Debug, Clone, Default)]
pub struct OperationStats {
    pub write_count: u64,
    pub write_success: u64,
    pub read_count: u64,
    pub read_success: u64,
    pub rmw_count: u64,
    pub rmw_success: u64,
    pub scan_count: u64,
    pub scan_success: u64,
}

impl OperationStats {
    /// Computes write success rate.
    pub fn write_success_rate(&self) -> f64 {
        if self.write_count == 0 {
            0.0
        } else {
            self.write_success as f64 / self.write_count as f64
        }
    }

    /// Computes read success rate.
    pub fn read_success_rate(&self) -> f64 {
        if self.read_count == 0 {
            0.0
        } else {
            self.read_success as f64 / self.read_count as f64
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
    fn trace_collector_basic() {
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

        assert_eq!(collector.events().len(), 2);
        assert_eq!(collector.stats().total_events, 2);
    }

    #[test]
    fn trace_collector_circular_buffer() {
        let config = TraceConfig {
            max_events: 3,
            ..TraceConfig::default()
        };
        let mut collector = TraceCollector::new(config);

        for i in 0..5 {
            collector.record(
                i * 1000,
                TraceEventType::Write {
                    key: i,
                    value: i,
                    success: true,
                    bytes_written: Some(8),
                },
            );
        }

        // Should only keep last 3
        assert_eq!(collector.events().len(), 3);
        assert_eq!(collector.stats().total_events, 5);
        assert_eq!(collector.stats().events_dropped, 2);

        // Check that we kept the latest
        assert_eq!(collector.events()[0].seq, 2);
        assert_eq!(collector.events()[2].seq, 4);
    }

    #[test]
    fn trace_collector_filter_success() {
        let config = TraceConfig {
            include_success: false,
            ..TraceConfig::default()
        };
        let mut collector = TraceCollector::new(config);

        collector.record(
            1000,
            TraceEventType::Write {
                key: 1,
                value: 100,
                success: true,
                bytes_written: Some(8),
            },
        );
        collector.record(
            2000,
            TraceEventType::Write {
                key: 2,
                value: 200,
                success: false,
                bytes_written: None,
            },
        );

        // Only failure should be recorded
        assert_eq!(collector.events().len(), 1);
        assert_eq!(collector.stats().events_filtered, 1);
    }

    #[test]
    fn trace_analyzer_finds_violations() {
        let mut collector = TraceCollector::default_config();

        collector.record(
            1000,
            TraceEventType::Write {
                key: 1,
                value: 100,
                success: true,
                bytes_written: Some(8),
            },
        );
        collector.record(
            2000,
            TraceEventType::InvariantViolation {
                invariant: "test".to_string(),
                message: "test violation".to_string(),
                context: vec![],
            },
        );

        let analyzer = TraceAnalyzer::new(collector.events());
        let violations = analyzer.find_violations();

        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn trace_analyzer_operation_stats() {
        let mut collector = TraceCollector::default_config();

        collector.record(
            1000,
            TraceEventType::Write {
                key: 1,
                value: 100,
                success: true,
                bytes_written: Some(8),
            },
        );
        collector.record(
            2000,
            TraceEventType::Write {
                key: 2,
                value: 200,
                success: false,
                bytes_written: None,
            },
        );
        collector.record(
            3000,
            TraceEventType::Read {
                key: 1,
                value: Some(100),
                success: true,
            },
        );

        let analyzer = TraceAnalyzer::new(collector.events());
        let stats = analyzer.operation_stats();

        assert_eq!(stats.write_count, 2);
        assert_eq!(stats.write_success, 1);
        assert_eq!(stats.read_count, 1);
        assert_eq!(stats.read_success, 1);
        assert_eq!(stats.write_success_rate(), 0.5);
    }

    #[test]
    fn trace_filter_by_time() {
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
            TraceEventType::Read {
                key: 1,
                value: Some(100),
                success: true,
            },
        );

        let filtered = collector.filter_by_time(1500, 2500);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].time_ns, 2000);
    }

    #[test]
    fn trace_to_json() {
        let mut collector = TraceCollector::default_config();

        collector.record(1000, TraceEventType::SimulationStart { seed: 42 });

        let json = collector.to_json().expect("should serialize");
        assert!(json.contains("SimulationStart"));
        assert!(json.contains("\"seed\": 42"));
    }
}
