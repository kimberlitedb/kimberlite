//! Phase tracking for event-triggered assertions.
//!
//! Tracks when critical system phases occur (e.g., view changes, commits)
//! to enable assertions that trigger after specific events.

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static PHASE_TRACKER: RefCell<PhaseTracker> = RefCell::new(PhaseTracker::new());
}

/// Event representing a system phase.
#[derive(Debug, Clone)]
pub struct PhaseEvent {
    pub category: String,
    pub event: String,
    pub context: String,
    pub step: u64,
}

/// Tracker for system phases.
#[derive(Debug)]
pub struct PhaseTracker {
    /// All recorded phase events
    events: Vec<PhaseEvent>,

    /// Count of each phase type (category:event)
    phase_counts: HashMap<String, u64>,

    /// Current step counter
    current_step: u64,
}

// Custom Clone implementation that skips the events vector to avoid
// cloning potentially millions of events when only counts are needed
impl Clone for PhaseTracker {
    fn clone(&self) -> Self {
        Self {
            // Don't clone events - they're only used for tests and can grow unbounded
            events: Vec::new(),
            phase_counts: self.phase_counts.clone(),
            current_step: self.current_step,
        }
    }
}

impl PhaseTracker {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            phase_counts: HashMap::new(),
            current_step: 0,
        }
    }

    /// Record a phase event.
    fn record(&mut self, category: &str, event: &str, context: String) {
        let phase_key = format!("{}:{}", category, event);
        *self.phase_counts.entry(phase_key).or_insert(0) += 1;

        self.events.push(PhaseEvent {
            category: category.to_string(),
            event: event.to_string(),
            context,
            step: self.current_step,
        });
    }

    /// Get the count for a specific phase.
    pub fn get_phase_count(&self, category: &str, event: &str) -> u64 {
        let key = format!("{}:{}", category, event);
        self.phase_counts.get(&key).copied().unwrap_or(0)
    }

    /// Get all phase events.
    pub fn all_events(&self) -> &[PhaseEvent] {
        &self.events
    }

    /// Get all phase counts.
    pub fn all_phase_counts(&self) -> &HashMap<String, u64> {
        &self.phase_counts
    }

    /// Set the current step (for timestamping).
    pub fn set_step(&mut self, step: u64) {
        self.current_step = step;
    }

    /// Reset the tracker.
    pub fn reset(&mut self) {
        self.events.clear();
        self.phase_counts.clear();
        self.current_step = 0;
    }
}

impl Default for PhaseTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Record a phase event (called by macros).
pub fn record_phase(category: &str, event: &str, context: String) {
    PHASE_TRACKER.with(|tracker| {
        tracker.borrow_mut().record(category, event, context);
    });

    // Trigger any deferred assertions waiting for this phase
    use super::deferred_assertions;
    let _triggered = deferred_assertions::trigger_phase_event(category, event);
    // TODO: Execute the triggered assertions
}

/// Set the current step (for synchronization with simulation).
pub fn set_phase_step(step: u64) {
    PHASE_TRACKER.with(|tracker| {
        tracker.borrow_mut().set_step(step);
    });
}

/// Get a snapshot of the current phase tracker.
pub fn get_phase_tracker() -> PhaseTracker {
    PHASE_TRACKER.with(|tracker| tracker.borrow().clone())
}

/// Reset the global phase tracker.
pub fn reset_phase_tracker() {
    PHASE_TRACKER.with(|tracker| tracker.borrow_mut().reset());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_tracking() {
        let mut tracker = PhaseTracker::new();

        tracker.record("vsr", "prepare_sent", "view=1, op=42".to_string());
        tracker.record("vsr", "prepare_sent", "view=1, op=43".to_string());
        tracker.record("vsr", "commit_broadcast", "view=1, op=42".to_string());

        assert_eq!(tracker.get_phase_count("vsr", "prepare_sent"), 2);
        assert_eq!(tracker.get_phase_count("vsr", "commit_broadcast"), 1);
        assert_eq!(tracker.get_phase_count("vsr", "unknown"), 0);
    }

    #[test]
    fn test_phase_events() {
        let mut tracker = PhaseTracker::new();
        tracker.set_step(100);

        tracker.record("vsr", "prepare_sent", "view=1".to_string());
        tracker.set_step(150);
        tracker.record("vsr", "commit_broadcast", "view=1".to_string());

        let events = tracker.all_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].step, 100);
        assert_eq!(events[1].step, 150);
    }

    #[test]
    fn test_phase_reset() {
        let mut tracker = PhaseTracker::new();

        tracker.record("vsr", "prepare_sent", "view=1".to_string());
        assert_eq!(tracker.all_events().len(), 1);

        tracker.reset();
        assert_eq!(tracker.all_events().len(), 0);
        assert_eq!(tracker.get_phase_count("vsr", "prepare_sent"), 0);
    }
}
