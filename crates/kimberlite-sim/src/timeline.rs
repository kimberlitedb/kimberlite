//! Timeline visualization for VOPR simulations.
//!
//! This module provides timeline collection and ASCII Gantt chart rendering
//! for visualizing simulation execution. The timeline captures key events
//! during simulation and can render them as a human-readable timeline.
//!
//! ## Example
//!
//! ```ignore
//! use kimberlite_sim::timeline::{TimelineCollector, TimelineConfig, GanttRenderer};
//!
//! let mut timeline = TimelineCollector::new(TimelineConfig::default());
//! timeline.record(1000, TimelineKind::WriteStart { address: 0, size: 4096 });
//! timeline.record(2000, TimelineKind::WriteComplete { address: 0, success: true });
//!
//! let renderer = GanttRenderer::new(120);
//! let output = renderer.render(&timeline);
//! println!("{}", output);
//! ```

use serde::{Deserialize, Serialize};

// ============================================================================
// Timeline Entry Types
// ============================================================================

/// A single entry in the simulation timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// Simulation time (nanoseconds).
    pub time_ns: u64,
    /// Sequential event ID.
    pub event_id: u64,
    /// Entry type.
    pub kind: TimelineKind,
    /// Associated node ID (if applicable).
    pub node_id: Option<u64>,
    /// Operation duration (0 for instant events).
    pub duration_ns: u64,
}

/// Types of events captured in the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimelineKind {
    // Client operations
    ClientRequest {
        op_type: String,
        key: u64,
    },
    ClientResponse {
        success: bool,
        latency_ns: u64,
    },

    // Storage operations
    WriteStart {
        address: u64,
        size: usize,
    },
    WriteComplete {
        address: u64,
        success: bool,
    },
    FsyncStart,
    FsyncComplete {
        latency_ns: u64,
    },

    // Network operations
    MessageSend {
        from: u64,
        to: u64,
        msg_type: String,
    },
    MessageDeliver {
        from: u64,
        to: u64,
        delay_ns: u64,
    },
    MessageDrop {
        from: u64,
        to: u64,
        reason: String,
    },

    // Replica state changes
    ViewChange {
        old_view: u64,
        new_view: u64,
        replica_id: u64,
    },
    Commit {
        op_number: u64,
        commit_number: u64,
        replica_id: u64,
    },

    // System events
    NodeCrash {
        node_id: u64,
    },
    NodeRestart {
        node_id: u64,
    },
    NetworkPartition {
        affected_nodes: Vec<u64>,
    },
    NetworkHeal,

    // Invariant checks
    InvariantCheck {
        name: String,
        passed: bool,
    },
    InvariantViolation {
        name: String,
        message: String,
    },

    // Generic event for extensibility
    Custom {
        label: String,
        data: String,
    },
}

// ============================================================================
// Timeline Collector
// ============================================================================

/// Configuration for timeline collection.
#[derive(Debug, Clone)]
pub struct TimelineConfig {
    /// Maximum number of entries to store (default: 50,000).
    pub max_entries: usize,
    /// Capture client operations.
    pub capture_client_ops: bool,
    /// Capture storage operations.
    pub capture_storage: bool,
    /// Capture network operations.
    pub capture_network: bool,
    /// Capture invariant checks.
    pub capture_invariants: bool,
}

impl Default for TimelineConfig {
    fn default() -> Self {
        Self {
            max_entries: 50_000,
            capture_client_ops: true,
            capture_storage: true,
            capture_network: true,
            capture_invariants: true,
        }
    }
}

/// Collects timeline events during simulation.
pub struct TimelineCollector {
    /// Collected entries.
    entries: Vec<TimelineEntry>,
    /// Next event ID.
    next_id: u64,
    /// Configuration.
    config: TimelineConfig,
    /// Pending duration tracking (event_id → start_time).
    pending_durations: std::collections::HashMap<u64, u64>,
}

impl TimelineCollector {
    /// Creates a new timeline collector.
    pub fn new(config: TimelineConfig) -> Self {
        Self {
            entries: Vec::new(),
            next_id: 0,
            config,
            pending_durations: std::collections::HashMap::new(),
        }
    }

    /// Records a timeline event.
    pub fn record(&mut self, time_ns: u64, kind: TimelineKind) -> u64 {
        if !self.should_capture(&kind) {
            return self.next_id;
        }

        // Prevent unbounded growth
        if self.entries.len() >= self.config.max_entries {
            return self.next_id;
        }

        let event_id = self.next_id;
        self.next_id += 1;

        let node_id = Self::extract_node_id(&kind);

        self.entries.push(TimelineEntry {
            time_ns,
            event_id,
            kind,
            node_id,
            duration_ns: 0,
        });

        event_id
    }

    /// Records the start of a duration event and returns the event ID.
    pub fn record_start(&mut self, time_ns: u64, kind: TimelineKind) -> u64 {
        let event_id = self.record(time_ns, kind);
        self.pending_durations.insert(event_id, time_ns);
        event_id
    }

    /// Records the completion of a duration event.
    pub fn record_complete(&mut self, start_event_id: u64, time_ns: u64) {
        if let Some(start_time) = self.pending_durations.remove(&start_event_id) {
            // PRECONDITION: completion time should be after start time
            assert!(
                time_ns >= start_time,
                "completion time {time_ns} before start time {start_time} for event {start_event_id}"
            );

            let duration_ns = time_ns.saturating_sub(start_time);

            // PRECONDITION: duration should be reasonable (< 60 seconds)
            const MAX_DURATION_NS: u64 = 60_000_000_000; // 60 seconds
            assert!(
                duration_ns <= MAX_DURATION_NS,
                "duration {duration_ns}ns exceeds maximum {MAX_DURATION_NS}ns for event {start_event_id}"
            );

            // Find and update the entry
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|e| e.event_id == start_event_id)
            {
                entry.duration_ns = duration_ns;

                // POSTCONDITION: entry was updated
                assert_eq!(
                    entry.duration_ns, duration_ns,
                    "duration not set correctly for event {start_event_id}"
                );
            }
        }
    }

    /// Returns all collected entries.
    pub fn entries(&self) -> &[TimelineEntry] {
        &self.entries
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if no entries have been collected.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clears all collected entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.next_id = 0;
        self.pending_durations.clear();
    }

    /// Filters entries by time range.
    pub fn filter_by_time(&self, min_ns: u64, max_ns: u64) -> Vec<TimelineEntry> {
        self.entries
            .iter()
            .filter(|e| e.time_ns >= min_ns && e.time_ns <= max_ns)
            .cloned()
            .collect()
    }

    /// Filters entries by node ID.
    pub fn filter_by_node(&self, node_id: u64) -> Vec<TimelineEntry> {
        self.entries
            .iter()
            .filter(|e| e.node_id == Some(node_id))
            .cloned()
            .collect()
    }

    fn should_capture(&self, kind: &TimelineKind) -> bool {
        match kind {
            TimelineKind::ClientRequest { .. } | TimelineKind::ClientResponse { .. } => {
                self.config.capture_client_ops
            }
            TimelineKind::WriteStart { .. }
            | TimelineKind::WriteComplete { .. }
            | TimelineKind::FsyncStart
            | TimelineKind::FsyncComplete { .. } => self.config.capture_storage,
            TimelineKind::MessageSend { .. }
            | TimelineKind::MessageDeliver { .. }
            | TimelineKind::MessageDrop { .. } => self.config.capture_network,
            TimelineKind::InvariantCheck { .. } | TimelineKind::InvariantViolation { .. } => {
                self.config.capture_invariants
            }
            _ => true,
        }
    }

    fn extract_node_id(kind: &TimelineKind) -> Option<u64> {
        match kind {
            TimelineKind::MessageSend { from, .. } => Some(*from),
            TimelineKind::MessageDeliver { to, .. } => Some(*to),
            TimelineKind::ViewChange { replica_id, .. } => Some(*replica_id),
            TimelineKind::Commit { replica_id, .. } => Some(*replica_id),
            TimelineKind::NodeCrash { node_id } => Some(*node_id),
            TimelineKind::NodeRestart { node_id } => Some(*node_id),
            _ => None,
        }
    }
}

// ============================================================================
// ASCII Gantt Renderer
// ============================================================================

/// Renders timeline as ASCII Gantt chart.
pub struct GanttRenderer {
    /// Terminal width in characters.
    width: usize,
    /// Show separate lane per node.
    show_node_lanes: bool,
}

impl GanttRenderer {
    /// Creates a new Gantt renderer with the specified width.
    pub fn new(width: usize) -> Self {
        Self {
            width,
            show_node_lanes: true,
        }
    }

    /// Renders the timeline as an ASCII Gantt chart.
    pub fn render(&self, timeline: &TimelineCollector) -> String {
        if timeline.is_empty() {
            return "No timeline events to display.".to_string();
        }

        let entries = timeline.entries();

        // Calculate time range
        let min_time = entries.iter().map(|e| e.time_ns).min().unwrap_or(0);
        let max_time = entries.iter().map(|e| e.time_ns).max().unwrap_or(0);

        if min_time == max_time {
            return format!(
                "All events at time {} ns\n{} events total",
                min_time,
                entries.len()
            );
        }

        let mut output = String::new();

        // Render header with time markers
        output.push_str(&self.render_header(min_time, max_time));
        output.push('\n');

        // Render lanes
        if self.show_node_lanes {
            // Group by node
            let mut node_entries: std::collections::HashMap<u64, Vec<&TimelineEntry>> =
                std::collections::HashMap::new();

            for entry in entries {
                if let Some(node_id) = entry.node_id {
                    node_entries.entry(node_id).or_default().push(entry);
                }
            }

            // BOUND: limit number of timeline nodes to prevent unbounded collection
            const MAX_TIMELINE_NODES: usize = 10_000;
            assert!(
                node_entries.len() <= MAX_TIMELINE_NODES,
                "Timeline node count {} exceeds maximum {}",
                node_entries.len(),
                MAX_TIMELINE_NODES
            );

            let mut nodes: Vec<_> = node_entries
                .keys()
                .copied()
                .take(MAX_TIMELINE_NODES)
                .collect();
            nodes.sort_unstable();

            for node_id in nodes {
                if let Some(node_events) = node_entries.get(&node_id) {
                    output.push_str(&self.render_lane(node_id, node_events, min_time, max_time));
                    output.push('\n');
                }
            }
        } else {
            // Single lane for all events
            let all_entries: Vec<_> = entries.iter().collect();
            output.push_str(&self.render_lane(0, &all_entries, min_time, max_time));
        }

        // Render legend
        output.push('\n');
        output.push_str(&self.render_legend());

        output
    }

    fn render_header(&self, min_time_ns: u64, max_time_ns: u64) -> String {
        let time_range_us = (max_time_ns - min_time_ns) / 1_000;
        let usable_width = self.width.saturating_sub(15); // Reserve space for labels

        // Create time markers every ~20 characters
        let num_markers = usable_width / 20;
        let time_step_us = if num_markers > 0 {
            time_range_us / num_markers as u64
        } else {
            time_range_us
        };

        let mut header = format!("{:12}", "Time (μs):");

        for i in 0..=num_markers {
            let time_us = (min_time_ns / 1_000) + (i as u64 * time_step_us);
            let marker = format!("{:>6}", time_us);
            header.push_str(&marker);
            header.push_str(&" ".repeat(14)); // Spacing
        }

        header
    }

    fn render_lane(
        &self,
        node_id: u64,
        entries: &[&TimelineEntry],
        min_time: u64,
        max_time: u64,
    ) -> String {
        let usable_width = self.width.saturating_sub(15);
        let time_range = max_time - min_time;

        let label = format!("Node {:>2}:    ", node_id);
        let mut output = label;

        // Create a blank timeline visualization
        let mut timeline_chars: Vec<char> = vec![' '; usable_width];

        for entry in entries {
            let pos = if time_range > 0 {
                ((entry.time_ns - min_time) as f64 / time_range as f64 * usable_width as f64)
                    as usize
            } else {
                0
            };

            if pos < usable_width {
                timeline_chars[pos] = self.entry_symbol(&entry.kind);
            }
        }

        output.push_str(&timeline_chars.iter().collect::<String>());
        output
    }

    fn entry_symbol(&self, kind: &TimelineKind) -> char {
        match kind {
            TimelineKind::WriteStart { .. } => 'W',
            TimelineKind::WriteComplete { success: true, .. } => 'w',
            TimelineKind::WriteComplete { success: false, .. } => 'X',
            TimelineKind::MessageSend { .. } => 'M',
            TimelineKind::MessageDeliver { .. } => 'm',
            TimelineKind::ViewChange { .. } => 'V',
            TimelineKind::Commit { .. } => 'C',
            TimelineKind::NodeCrash { .. } => '✗',
            TimelineKind::NodeRestart { .. } => '↑',
            TimelineKind::InvariantViolation { .. } => '!',
            TimelineKind::InvariantCheck { passed: true, .. } => '✓',
            TimelineKind::InvariantCheck { passed: false, .. } => '✗',
            TimelineKind::ClientRequest { .. } => 'R',
            TimelineKind::ClientResponse { success: true, .. } => 'r',
            TimelineKind::ClientResponse { success: false, .. } => 'x',
            _ => '·',
        }
    }

    fn render_legend(&self) -> String {
        let mut legend = String::from("Legend:\n");
        legend
            .push_str("  W/w = Write Start/Complete  M/m = Message Send/Deliver  V = ViewChange\n");
        legend.push_str(
            "  C = Commit  R/r = Request/Response  ✗ = Crash/Failure  ! = Invariant Violation\n",
        );
        legend
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_collector_basic() {
        let mut timeline = TimelineCollector::new(TimelineConfig::default());

        timeline.record(
            1000,
            TimelineKind::WriteStart {
                address: 0,
                size: 4096,
            },
        );
        timeline.record(
            2000,
            TimelineKind::WriteComplete {
                address: 0,
                success: true,
            },
        );

        assert_eq!(timeline.len(), 2);
        assert!(!timeline.is_empty());
    }

    #[test]
    fn timeline_collector_duration_tracking() {
        let mut timeline = TimelineCollector::new(TimelineConfig::default());

        let start_id = timeline.record_start(
            1000,
            TimelineKind::WriteStart {
                address: 0,
                size: 4096,
            },
        );
        timeline.record_complete(start_id, 3000);

        let entries = timeline.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].duration_ns, 2000);
    }

    #[test]
    fn timeline_collector_max_entries() {
        let config = TimelineConfig {
            max_entries: 10,
            ..Default::default()
        };
        let mut timeline = TimelineCollector::new(config);

        // Try to add 20 entries
        for i in 0..20 {
            timeline.record(
                i * 1000,
                TimelineKind::Custom {
                    label: "test".to_string(),
                    data: String::new(),
                },
            );
        }

        // Should be capped at 10
        assert_eq!(timeline.len(), 10);
    }

    #[test]
    fn timeline_filter_by_time() {
        let mut timeline = TimelineCollector::new(TimelineConfig::default());

        timeline.record(1000, TimelineKind::NodeCrash { node_id: 0 });
        timeline.record(2000, TimelineKind::NodeCrash { node_id: 1 });
        timeline.record(3000, TimelineKind::NodeCrash { node_id: 2 });

        let filtered = timeline.filter_by_time(1500, 2500);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].time_ns, 2000);
    }

    #[test]
    fn timeline_filter_by_node() {
        let mut timeline = TimelineCollector::new(TimelineConfig::default());

        timeline.record(1000, TimelineKind::NodeCrash { node_id: 0 });
        timeline.record(2000, TimelineKind::NodeCrash { node_id: 1 });
        timeline.record(3000, TimelineKind::NodeCrash { node_id: 0 });

        let filtered = timeline.filter_by_node(0);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn gantt_renderer_basic() {
        let mut timeline = TimelineCollector::new(TimelineConfig::default());

        timeline.record(
            1000,
            TimelineKind::WriteStart {
                address: 0,
                size: 4096,
            },
        );
        timeline.record(
            2000,
            TimelineKind::MessageSend {
                from: 0,
                to: 1,
                msg_type: "Prepare".to_string(),
            },
        );
        timeline.record(
            3000,
            TimelineKind::ViewChange {
                old_view: 0,
                new_view: 1,
                replica_id: 0,
            },
        );

        let renderer = GanttRenderer::new(120);
        let output = renderer.render(&timeline);

        assert!(output.contains("Time (μs):"));
        assert!(output.contains("Legend:"));
    }

    #[test]
    fn gantt_renderer_empty_timeline() {
        let timeline = TimelineCollector::new(TimelineConfig::default());
        let renderer = GanttRenderer::new(120);
        let output = renderer.render(&timeline);

        assert_eq!(output, "No timeline events to display.");
    }

    #[test]
    fn timeline_capture_filtering() {
        let config = TimelineConfig {
            capture_client_ops: false,
            capture_storage: true,
            ..Default::default()
        };
        let mut timeline = TimelineCollector::new(config);

        timeline.record(
            1000,
            TimelineKind::ClientRequest {
                op_type: "write".to_string(),
                key: 42,
            },
        );
        timeline.record(
            2000,
            TimelineKind::WriteStart {
                address: 0,
                size: 4096,
            },
        );

        // Only storage event should be captured
        assert_eq!(timeline.len(), 1);
    }
}
