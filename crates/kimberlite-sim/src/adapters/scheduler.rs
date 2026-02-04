//! Scheduler adapter trait for simulation event ordering.
//!
//! This module provides a trait-based abstraction for event scheduling:
//! - **Deterministic simulation**: Use `EventQueue` with discrete time
//! - **Production use**: Could use tokio timers or other async schedulers
//!
//! # Performance
//!
//! The `Scheduler` trait can be used with either generics (zero-cost) or
//! trait objects (cold path acceptable). Event scheduling is not typically
//! on the hot path.

// Re-export types from parent module
pub use crate::event::{Event, EventId, EventKind, EventQueue};

/// Trait for event scheduling (simulation or production).
///
/// Implementations manage a time-ordered queue of events.
pub trait Scheduler {
    /// Schedules an event at the specified time.
    ///
    /// # Arguments
    ///
    /// * `time_ns` - Time in nanoseconds when event should fire
    /// * `event` - The event to schedule
    ///
    /// # Returns
    ///
    /// Event ID that can be used to cancel the event (if needed).
    fn schedule(&mut self, time_ns: u64, event: EventKind) -> EventId;

    /// Removes and returns the next event to process.
    ///
    /// Returns `None` if the queue is empty.
    fn pop(&mut self) -> Option<Event>;

    /// Returns the time of the next event, if any.
    fn next_time(&self) -> Option<u64>;

    /// Returns true if there are no pending events.
    fn is_empty(&self) -> bool;

    /// Returns the number of pending events.
    fn len(&self) -> usize;

    /// Clears all pending events.
    fn clear(&mut self);
}

// ============================================================================
// Simulation Implementation
// ============================================================================

impl Scheduler for EventQueue {
    fn schedule(&mut self, time_ns: u64, event: EventKind) -> EventId {
        EventQueue::schedule(self, time_ns, event)
    }

    fn pop(&mut self) -> Option<Event> {
        EventQueue::pop(self)
    }

    fn next_time(&self) -> Option<u64> {
        EventQueue::next_time(self)
    }

    fn is_empty(&self) -> bool {
        EventQueue::is_empty(self)
    }

    fn len(&self) -> usize {
        EventQueue::len(self)
    }

    fn clear(&mut self) {
        EventQueue::clear(self);
    }
}

// ============================================================================
// Production Implementation (Sketch)
// ============================================================================

/// Tokio-based scheduler for production use (sketch).
///
/// **Note**: This is a sketch for architectural demonstration.
/// Full implementation would use tokio::time and futures.
#[cfg(not(test))]
pub struct TokioScheduler {
    // Would contain tokio timer handles, channels, etc.
    _placeholder: (),
}

#[cfg(not(test))]
impl TokioScheduler {
    /// Creates a new Tokio-based scheduler.
    pub fn new() -> Self {
        Self { _placeholder: () }
    }
}

#[cfg(not(test))]
impl Scheduler for TokioScheduler {
    fn schedule(&mut self, _time_ns: u64, _event: EventKind) -> EventId {
        // Would schedule via tokio::time::sleep_until
        EventId::from_raw(0)
    }

    fn pop(&mut self) -> Option<Event> {
        // Would poll tokio channels
        None
    }

    fn next_time(&self) -> Option<u64> {
        // Would query tokio timer state
        None
    }

    fn is_empty(&self) -> bool {
        true
    }

    fn len(&self) -> usize {
        0
    }

    fn clear(&mut self) {
        // Would cancel all tokio timers
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_queue_trait_impl() {
        let mut scheduler: Box<dyn Scheduler> = Box::new(EventQueue::new());

        // Schedule events
        let id1 = scheduler.schedule(1000, EventKind::Custom(1));
        let id2 = scheduler.schedule(500, EventKind::Custom(2));
        let id3 = scheduler.schedule(1500, EventKind::Custom(3));

        // Verify ordering
        assert_eq!(scheduler.len(), 3);
        assert_eq!(scheduler.next_time(), Some(500));

        // Pop events in time order
        let event1 = scheduler.pop().unwrap();
        assert_eq!(event1.id, id2); // 500ns event first
        assert_eq!(event1.time_ns, 500);

        let event2 = scheduler.pop().unwrap();
        assert_eq!(event2.id, id1); // 1000ns event second
        assert_eq!(event2.time_ns, 1000);

        let event3 = scheduler.pop().unwrap();
        assert_eq!(event3.id, id3); // 1500ns event third
        assert_eq!(event3.time_ns, 1500);

        assert!(scheduler.is_empty());
    }

    #[test]
    fn event_queue_fifo_at_same_time() {
        let mut scheduler: Box<dyn Scheduler> = Box::new(EventQueue::new());

        // Schedule multiple events at the same time
        let id1 = scheduler.schedule(1000, EventKind::Custom(1));
        let id2 = scheduler.schedule(1000, EventKind::Custom(2));
        let id3 = scheduler.schedule(1000, EventKind::Custom(3));

        // Should be FIFO order
        assert_eq!(scheduler.pop().unwrap().id, id1);
        assert_eq!(scheduler.pop().unwrap().id, id2);
        assert_eq!(scheduler.pop().unwrap().id, id3);
    }

    #[test]
    fn event_queue_clear() {
        let mut scheduler: Box<dyn Scheduler> = Box::new(EventQueue::new());

        scheduler.schedule(1000, EventKind::Custom(1));
        scheduler.schedule(2000, EventKind::Custom(2));

        assert_eq!(scheduler.len(), 2);

        scheduler.clear();

        assert_eq!(scheduler.len(), 0);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn event_queue_generic_usage() {
        fn use_scheduler<S: Scheduler>(scheduler: &mut S) {
            scheduler.schedule(1000, EventKind::Custom(1));
            scheduler.schedule(500, EventKind::Custom(2));
        }

        let mut scheduler = EventQueue::new();
        use_scheduler(&mut scheduler);

        assert_eq!(scheduler.len(), 2);
        assert_eq!(scheduler.next_time(), Some(500));
    }

    #[test]
    fn event_queue_empty_operations() {
        let mut scheduler: Box<dyn Scheduler> = Box::new(EventQueue::new());

        assert!(scheduler.is_empty());
        assert_eq!(scheduler.len(), 0);
        assert_eq!(scheduler.next_time(), None);
        assert!(scheduler.pop().is_none());
    }
}
