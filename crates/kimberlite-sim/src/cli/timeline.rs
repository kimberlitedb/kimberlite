//! Timeline command for visualizing simulation execution as ASCII Gantt chart.

use super::{Command, CommandError, validate_bundle_path};
use crate::event_log::ReproBundle;
use crate::timeline::{GanttRenderer, TimelineCollector, TimelineConfig, TimelineKind};
use std::path::PathBuf;

// ============================================================================
// Timeline Command
// ============================================================================

/// Displays timeline visualization of a failure bundle.
#[derive(Debug, Clone)]
pub struct TimelineCommand {
    /// Path to the .kmb bundle file.
    pub bundle_path: PathBuf,

    /// Terminal width for rendering.
    pub width: usize,

    /// Time range filter (min_ns, max_ns).
    pub time_range: Option<(u64, u64)>,

    /// Node filter (only show specific nodes).
    pub node_filter: Option<Vec<u64>>,

    /// Show legend.
    pub show_legend: bool,
}

impl TimelineCommand {
    /// Creates a new timeline command.
    pub fn new(bundle_path: PathBuf) -> Self {
        Self {
            bundle_path,
            width: 120,
            time_range: None,
            node_filter: None,
            show_legend: true,
        }
    }

    /// Sets terminal width.
    pub fn with_width(mut self, width: usize) -> Self {
        self.width = width;
        self
    }

    /// Sets time range filter.
    pub fn with_time_range(mut self, min_ns: u64, max_ns: u64) -> Self {
        self.time_range = Some((min_ns, max_ns));
        self
    }

    /// Sets node filter.
    pub fn with_node_filter(mut self, nodes: Vec<u64>) -> Self {
        self.node_filter = Some(nodes);
        self
    }
}

impl Command for TimelineCommand {
    fn execute(&self) -> Result<(), CommandError> {
        // Validate bundle path
        validate_bundle_path(&self.bundle_path)?;

        // Load bundle
        let bundle = ReproBundle::load_from_file(&self.bundle_path)
            .map_err(|e| CommandError::InvalidBundle(e.to_string()))?;

        println!("═══════════════════════════════════════════════════════");
        println!("VOPR Timeline Visualization");
        println!("═══════════════════════════════════════════════════════");
        println!("Bundle: {}", self.bundle_path.display());
        println!("Seed: {}", bundle.seed);
        println!("Scenario: {}", bundle.scenario);
        println!("═══════════════════════════════════════════════════════\n");

        // Collect timeline from event log
        let timeline = self.collect_timeline(&bundle)?;

        if timeline.is_empty() {
            println!("No timeline events found in bundle.");
            return Ok(());
        }

        println!("Collected {} timeline events\n", timeline.len());

        // Render timeline
        let renderer = GanttRenderer::new(self.width);
        let output = renderer.render(&timeline);

        println!("{}", output);

        Ok(())
    }
}

impl TimelineCommand {
    /// Collects timeline from replay bundle event log.
    ///
    /// This reconstructs the timeline by replaying the event log decisions.
    /// Since we don't have actual simulation execution here, we reconstruct
    /// key events from the logged decisions.
    fn collect_timeline(&self, bundle: &ReproBundle) -> Result<TimelineCollector, CommandError> {
        let mut timeline = TimelineCollector::new(TimelineConfig::default());

        if let Some(ref events) = bundle.event_log {
            for event in events {
                // Convert logged events to timeline entries
                let kind = self.convert_event_to_timeline_kind(event);

                if let Some(timeline_kind) = kind {
                    timeline.record(event.time_ns, timeline_kind);
                }
            }
        }

        // Apply filters if specified
        let filtered_timeline = self.apply_filters(timeline)?;

        Ok(filtered_timeline)
    }

    fn convert_event_to_timeline_kind(
        &self,
        event: &crate::event_log::LoggedEvent,
    ) -> Option<TimelineKind> {
        use crate::event_log::Decision;

        match &event.decision {
            Decision::NetworkDelay {
                message_id,
                delay_ns: _,
            } => Some(TimelineKind::MessageSend {
                from: 0,
                to: 1,
                msg_type: format!("msg#{}", message_id),
            }),
            Decision::NetworkDrop { message_id } => Some(TimelineKind::MessageDrop {
                from: 0,
                to: 1,
                reason: format!("msg#{} dropped", message_id),
            }),
            Decision::StorageComplete { success, .. } => {
                if *success {
                    Some(TimelineKind::WriteComplete {
                        address: 0,
                        success: true,
                    })
                } else {
                    Some(TimelineKind::WriteComplete {
                        address: 0,
                        success: false,
                    })
                }
            }
            Decision::NodeCrash { node_id } => Some(TimelineKind::NodeCrash { node_id: *node_id }),
            Decision::NodeRestart { node_id } => {
                Some(TimelineKind::NodeRestart { node_id: *node_id })
            }
            Decision::ByzantineAttack {
                attack_type,
                target,
            } => Some(TimelineKind::Custom {
                label: "Byzantine".to_string(),
                data: format!("{}: {}", attack_type, target),
            }),
            Decision::EventScheduled { event_type, .. } => Some(TimelineKind::Custom {
                label: "Event".to_string(),
                data: event_type.clone(),
            }),
            Decision::SchedulerNodeSelected {
                node_id,
                runnable_count,
            } => Some(TimelineKind::Custom {
                label: "Scheduler".to_string(),
                data: format!("Node {} selected ({} runnable)", node_id, runnable_count),
            }),
            Decision::SchedulerEventDequeued {
                event_type,
                queue_depth,
            } => Some(TimelineKind::Custom {
                label: "Dequeue".to_string(),
                data: format!("{} (queue: {})", event_type, queue_depth),
            }),
            Decision::TimeAdvance {
                from_ns,
                to_ns,
                delta_ns,
            } => Some(TimelineKind::Custom {
                label: "Time".to_string(),
                data: format!("{}ns → {}ns (+{}ns)", from_ns, to_ns, delta_ns),
            }),
            Decision::TimerFired {
                timer_id,
                scheduled_for_ns,
                actual_fire_ns,
            } => Some(TimelineKind::Custom {
                label: "Timer".to_string(),
                data: format!(
                    "Timer {} fired (scheduled: {}, actual: {})",
                    timer_id, scheduled_for_ns, actual_fire_ns
                ),
            }),
            Decision::InvariantCheck {
                invariant_name,
                passed,
            } => Some(TimelineKind::Custom {
                label: if *passed {
                    "Invariant✓"
                } else {
                    "Invariant✗"
                }
                .to_string(),
                data: invariant_name.clone(),
            }),
            Decision::RngValue { .. } => None,
        }
    }

    #[allow(clippy::unnecessary_wraps)] // Result for future error handling in CLI
    fn apply_filters(
        &self,
        mut timeline: TimelineCollector,
    ) -> Result<TimelineCollector, CommandError> {
        // Apply time range filter
        if let Some((min_ns, max_ns)) = self.time_range {
            let filtered_entries = timeline.filter_by_time(min_ns, max_ns);

            let mut new_timeline = TimelineCollector::new(TimelineConfig::default());
            for entry in filtered_entries {
                new_timeline.record(entry.time_ns, entry.kind);
            }
            timeline = new_timeline;
        }

        // Apply node filter
        if let Some(ref nodes) = self.node_filter {
            let mut new_timeline = TimelineCollector::new(TimelineConfig::default());

            for node_id in nodes {
                let filtered_entries = timeline.filter_by_node(*node_id);
                for entry in filtered_entries {
                    new_timeline.record(entry.time_ns, entry.kind);
                }
            }
            timeline = new_timeline;
        }

        Ok(timeline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_command_creation() {
        let cmd = TimelineCommand::new(PathBuf::from("test.kmb"));
        assert_eq!(cmd.width, 120);
        assert!(cmd.show_legend);
    }

    #[test]
    fn timeline_command_with_width() {
        let cmd = TimelineCommand::new(PathBuf::from("test.kmb")).with_width(80);
        assert_eq!(cmd.width, 80);
    }

    #[test]
    fn timeline_command_with_filters() {
        let cmd = TimelineCommand::new(PathBuf::from("test.kmb"))
            .with_time_range(1000, 5000)
            .with_node_filter(vec![0, 1]);

        assert_eq!(cmd.time_range, Some((1000, 5000)));
        assert_eq!(cmd.node_filter, Some(vec![0, 1]));
    }
}
