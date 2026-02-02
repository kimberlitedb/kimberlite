//! Deferred and event-triggered assertions.
//!
//! This module provides infrastructure for assertions that fire based on
//! system phases or after a certain number of steps.

use std::cell::RefCell;
use std::collections::VecDeque;

thread_local! {
    static DEFERRED_ASSERTIONS: RefCell<DeferredAssertionQueue> =
        RefCell::new(DeferredAssertionQueue::new());
}

/// A deferred assertion that will fire at a specific step or after a trigger.
#[derive(Debug, Clone)]
pub struct DeferredAssertion {
    /// Unique identifier for this assertion
    pub id: u64,
    /// The step at which this assertion should fire
    pub fire_at_step: u64,
    /// Optional trigger event (category:event)
    pub trigger: Option<String>,
    /// The assertion key (for tracking)
    pub key: String,
    /// Human-readable description
    pub description: String,
}

/// Queue of deferred assertions waiting to fire.
#[derive(Debug)]
pub struct DeferredAssertionQueue {
    /// Assertions waiting to fire
    assertions: VecDeque<DeferredAssertion>,
    /// Current simulation step
    current_step: u64,
    /// Next assertion ID
    next_id: u64,
    /// Fired assertion IDs (for tracking)
    fired: Vec<u64>,
}

impl DeferredAssertionQueue {
    /// Create a new empty queue.
    pub fn new() -> Self {
        Self {
            assertions: VecDeque::new(),
            current_step: 0,
            next_id: 0,
            fired: Vec::new(),
        }
    }

    /// Register a new deferred assertion.
    pub fn register(
        &mut self,
        fire_at_step: u64,
        trigger: Option<String>,
        key: String,
        description: String,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.assertions.push_back(DeferredAssertion {
            id,
            fire_at_step,
            trigger,
            key,
            description,
        });

        id
    }

    /// Set the current step.
    pub fn set_step(&mut self, step: u64) {
        self.current_step = step;
    }

    /// Get all assertions ready to fire at the current step.
    pub fn get_ready(&mut self) -> Vec<DeferredAssertion> {
        let mut ready = Vec::new();
        let current = self.current_step;

        // Keep only assertions not yet ready
        self.assertions.retain(|assertion| {
            if assertion.fire_at_step <= current {
                ready.push(assertion.clone());
                self.fired.push(assertion.id);
                false
            } else {
                true
            }
        });

        // Sort by fire step for deterministic ordering
        ready.sort_by_key(|a| (a.fire_at_step, a.id));
        ready
    }

    /// Trigger assertions waiting for a specific event.
    pub fn trigger_event(&mut self, category: &str, event: &str) -> Vec<DeferredAssertion> {
        let trigger_key = format!("{category}:{event}");
        let mut triggered = Vec::new();

        self.assertions.retain(|assertion| {
            if let Some(ref trigger) = assertion.trigger {
                if trigger == &trigger_key {
                    triggered.push(assertion.clone());
                    self.fired.push(assertion.id);
                    return false;
                }
            }
            true
        });

        triggered
    }

    /// Get the number of pending assertions.
    pub fn pending_count(&self) -> usize {
        self.assertions.len()
    }

    /// Get the number of fired assertions.
    pub fn fired_count(&self) -> usize {
        self.fired.len()
    }

    /// Reset the queue.
    pub fn reset(&mut self) {
        self.assertions.clear();
        self.fired.clear();
        self.current_step = 0;
    }
}

impl Default for DeferredAssertionQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Register a deferred assertion (called by macros).
pub fn register_deferred_assertion(
    fire_at_step: u64,
    trigger: Option<String>,
    key: String,
    description: String,
) -> u64 {
    DEFERRED_ASSERTIONS.with(|queue| {
        queue
            .borrow_mut()
            .register(fire_at_step, trigger, key, description)
    })
}

/// Set the current simulation step.
pub fn set_deferred_step(step: u64) {
    DEFERRED_ASSERTIONS.with(|queue| {
        queue.borrow_mut().set_step(step);
    });
}

/// Get assertions ready to fire at the current step.
pub fn get_ready_assertions() -> Vec<DeferredAssertion> {
    DEFERRED_ASSERTIONS.with(|queue| queue.borrow_mut().get_ready())
}

/// Trigger assertions waiting for a specific event.
pub fn trigger_phase_event(category: &str, event: &str) -> Vec<DeferredAssertion> {
    DEFERRED_ASSERTIONS.with(|queue| queue.borrow_mut().trigger_event(category, event))
}

/// Get a snapshot of the current queue.
pub fn get_deferred_queue_stats() -> (usize, usize) {
    DEFERRED_ASSERTIONS.with(|queue| {
        let q = queue.borrow();
        (q.pending_count(), q.fired_count())
    })
}

/// Reset the global deferred assertion queue.
pub fn reset_deferred_assertions() {
    DEFERRED_ASSERTIONS.with(|queue| {
        queue.borrow_mut().reset();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deferred_assertion_basic() {
        let mut queue = DeferredAssertionQueue::new();

        queue.register(
            100,
            None,
            "test_assertion".to_string(),
            "Test assertion".to_string(),
        );

        // Not ready yet
        queue.set_step(50);
        let ready = queue.get_ready();
        assert_eq!(ready.len(), 0);
        assert_eq!(queue.pending_count(), 1);

        // Now ready
        queue.set_step(100);
        let ready = queue.get_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(queue.pending_count(), 0);
        assert_eq!(queue.fired_count(), 1);
    }

    #[test]
    fn test_triggered_assertions() {
        let mut queue = DeferredAssertionQueue::new();

        queue.register(
            1000,
            Some("vsr:prepare_sent".to_string()),
            "test_after_prepare".to_string(),
            "Test after prepare".to_string(),
        );

        // Trigger the event
        let triggered = queue.trigger_event("vsr", "prepare_sent");
        assert_eq!(triggered.len(), 1);
        assert_eq!(queue.pending_count(), 0);
        assert_eq!(queue.fired_count(), 1);
    }

    #[test]
    fn test_multiple_assertions() {
        let mut queue = DeferredAssertionQueue::new();

        queue.register(100, None, "first".to_string(), "First".to_string());
        queue.register(200, None, "second".to_string(), "Second".to_string());
        queue.register(150, None, "third".to_string(), "Third".to_string());

        // Fire first batch
        queue.set_step(150);
        let ready = queue.get_ready();
        assert_eq!(ready.len(), 2); // first and third
        assert_eq!(ready[0].key, "first");
        assert_eq!(ready[1].key, "third");

        // Fire second batch
        queue.set_step(200);
        let ready = queue.get_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].key, "second");
    }

    #[test]
    fn test_reset() {
        let mut queue = DeferredAssertionQueue::new();

        queue.register(100, None, "test".to_string(), "Test".to_string());
        queue.set_step(100);
        let _ = queue.get_ready();

        assert_eq!(queue.fired_count(), 1);

        queue.reset();
        assert_eq!(queue.pending_count(), 0);
        assert_eq!(queue.fired_count(), 0);
    }
}
