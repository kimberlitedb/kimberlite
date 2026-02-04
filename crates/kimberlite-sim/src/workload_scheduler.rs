//! Continuous workload scheduler for marathon stress tests.
//!
//! This module provides a stateful scheduler that generates operations
//! throughout a simulation run, enabling stress tests that reach max_events
//! or max_time limits rather than stopping when initial work drains.
//!
//! **Always Enabled**: The workload scheduler is always active in VOPR
//! simulations. All simulations use continuous workload generation.
//!
//! # Architecture
//!
//! The scheduler uses an event-based approach:
//! 1. Initial `WorkloadTick` event is scheduled at simulation start
//! 2. When processed, it generates a batch of operations (e.g., 5 ops)
//! 3. Each operation is scheduled as a Custom or VsrClientRequest event
//! 4. The scheduler reschedules itself for the next tick
//! 5. Process repeats until max_scheduled_ops, max_events, or max_time reached
//!
//! # Determinism
//!
//! All RNG calls happen in strict event time order, maintaining determinism.
//! Same seed produces identical operation sequences regardless of queue depth.

use crate::{EventKind, SimRng};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for continuous workload generation.
#[derive(Debug, Clone)]
pub struct WorkloadSchedulerConfig {
    /// Operations to schedule per tick.
    pub ops_per_tick: usize,

    /// Interval between ticks (nanoseconds).
    pub tick_interval_ns: u64,

    /// Whether to use enhanced workloads (RMW, scans).
    pub enhanced_workloads: bool,

    /// Stop generating when this many events have been scheduled.
    /// None = no limit (will respect sim max_events).
    pub max_scheduled_ops: Option<usize>,

    /// For VSR mode: schedule client requests instead of Custom events.
    pub vsr_mode: bool,

    /// Simulation-level event limit (passed from SimulationConfig).
    /// Used to prevent infinite rescheduling when max_scheduled_ops is None.
    pub sim_max_events: Option<u64>,

    /// Simulation-level time limit in nanoseconds (passed from SimulationConfig).
    /// Used to prevent scheduling beyond simulation time limit.
    pub sim_max_time_ns: Option<u64>,
}

impl Default for WorkloadSchedulerConfig {
    fn default() -> Self {
        Self {
            ops_per_tick: 5,                    // 5 ops per batch
            tick_interval_ns: 10_000_000,       // 10ms between batches = 500 ops/sec
            enhanced_workloads: true,
            max_scheduled_ops: None,
            vsr_mode: false,
            sim_max_events: None,
            sim_max_time_ns: None,
        }
    }
}

// ============================================================================
// Workload Scheduler
// ============================================================================

/// Stateful workload scheduler that generates operations throughout simulation.
pub struct WorkloadScheduler {
    config: WorkloadSchedulerConfig,
    ops_scheduled: usize,
    enabled: bool,

    /// Estimate of events processed (updated via update_sim_context).
    events_processed_estimate: u64,

    /// Current simulation time in nanoseconds (updated via update_sim_context).
    current_sim_time_ns: u64,
}

impl WorkloadScheduler {
    /// Creates a new workload scheduler with the given configuration.
    pub fn new(config: WorkloadSchedulerConfig) -> Self {
        Self {
            config,
            ops_scheduled: 0,
            enabled: true,
            events_processed_estimate: 0,
            current_sim_time_ns: 0,
        }
    }

    /// Returns true if the scheduler should generate more operations.
    fn should_continue(&self) -> bool {
        if !self.enabled {
            return false;
        }

        // Check workload-specific operation limit
        if let Some(max) = self.config.max_scheduled_ops {
            if self.ops_scheduled >= max {
                return false;
            }
        }

        // Check simulation event limit (with 100-event buffer for safety)
        // This prevents infinite rescheduling when max_scheduled_ops is None
        if let Some(max_events) = self.config.sim_max_events {
            if self.events_processed_estimate + 100 >= max_events {
                return false;
            }
        }

        // Check simulation time limit (with 100ms buffer for safety)
        // This prevents scheduling beyond the simulation time horizon
        if let Some(max_time) = self.config.sim_max_time_ns {
            if self.current_sim_time_ns + 100_000_000 >= max_time {
                return false;
            }
        }

        true
    }

    /// Schedules the initial workload tick event.
    ///
    /// Call this during simulation initialization to seed the first tick.
    pub fn schedule_initial_tick(&self, events: &mut Vec<(u64, EventKind)>, current_time_ns: u64) {
        if self.enabled {
            events.push((
                current_time_ns + self.config.tick_interval_ns,
                EventKind::WorkloadTick,
            ));
        }
    }

    /// Updates the simulation context for limit-aware termination.
    ///
    /// This should be called before `handle_tick()` to ensure the scheduler
    /// has current information about simulation progress.
    ///
    /// # Arguments
    ///
    /// * `events_processed` - Total events processed by the simulation
    /// * `current_time_ns` - Current simulation time in nanoseconds
    pub fn update_sim_context(&mut self, events_processed: u64, current_time_ns: u64) {
        self.events_processed_estimate = events_processed;
        self.current_sim_time_ns = current_time_ns;
    }

    /// Handles a WorkloadTick event by scheduling a batch of operations.
    ///
    /// Returns a list of (time, event) tuples to schedule, plus an optional
    /// next WorkloadTick event.
    ///
    /// # Arguments
    ///
    /// * `current_time_ns` - Current simulation time
    /// * `events_processed` - Total events processed (for limit awareness)
    /// * `rng` - Deterministic random number generator
    ///
    /// # Returns
    ///
    /// Vector of (time_ns, EventKind) pairs to schedule.
    pub fn handle_tick(
        &mut self,
        current_time_ns: u64,
        events_processed: u64,
        rng: &mut SimRng,
    ) -> Vec<(u64, EventKind)> {
        // Update simulation context first
        self.update_sim_context(events_processed, current_time_ns);

        if !self.should_continue() {
            return Vec::new();
        }

        let mut scheduled = Vec::new();

        // Calculate batch size (respect max_scheduled_ops if set)
        let remaining = self.config.max_scheduled_ops.map(|max| max.saturating_sub(self.ops_scheduled));
        let batch_size = remaining
            .map(|r| r.min(self.config.ops_per_tick))
            .unwrap_or(self.config.ops_per_tick);

        // Schedule operations
        let op_count = if self.config.enhanced_workloads { 6 } else { 4 };

        for _ in 0..batch_size {
            let delay = rng.delay_ns(1_000_000, 10_000_000); // 1-10ms

            let event = if self.config.vsr_mode {
                // VSR mode: schedule client request
                EventKind::VsrClientRequest {
                    replica_id: 0, // Leader
                    command_bytes: vec![],
                    idempotency_id: None,
                }
            } else {
                // Simplified mode: schedule custom event with operation type
                let op_type = rng.next_u64() % op_count;
                EventKind::Custom(op_type)
            };

            scheduled.push((current_time_ns + delay, event));
            self.ops_scheduled += 1;
        }

        // Schedule next tick if we should continue
        if self.should_continue() {
            scheduled.push((
                current_time_ns + self.config.tick_interval_ns,
                EventKind::WorkloadTick,
            ));
        }

        scheduled
    }

    /// Returns the number of operations scheduled so far.
    pub fn ops_scheduled(&self) -> usize {
        self.ops_scheduled
    }

    /// Disables future tick scheduling (for graceful shutdown).
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Returns true if the scheduler is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_respects_max_scheduled_ops() {
        let config = WorkloadSchedulerConfig {
            ops_per_tick: 5,
            tick_interval_ns: 10_000_000,
            enhanced_workloads: false,
            max_scheduled_ops: Some(20),
            vsr_mode: false,
            sim_max_events: None,
            sim_max_time_ns: None,
        };

        let mut scheduler = WorkloadScheduler::new(config);
        let mut rng = SimRng::new(12345);

        // Process 4 ticks: 5 ops × 4 = 20 ops
        for i in 0..4 {
            let current_time = i * 10_000_000;
            let events_processed = i * 10;
            let events = scheduler.handle_tick(current_time, events_processed, &mut rng);
            assert!(!events.is_empty(), "Tick {} should generate events", i);
        }

        assert_eq!(scheduler.ops_scheduled(), 20);

        // 5th tick should generate nothing (hit limit)
        let events = scheduler.handle_tick(40_000_000, 40, &mut rng);
        assert!(events.is_empty(), "Should stop at max_scheduled_ops");
        assert_eq!(scheduler.ops_scheduled(), 20);
    }

    #[test]
    fn test_scheduler_determinism() {
        let config = WorkloadSchedulerConfig {
            ops_per_tick: 5,
            tick_interval_ns: 10_000_000,
            enhanced_workloads: true,
            max_scheduled_ops: Some(10),
            vsr_mode: false,
            sim_max_events: None,
            sim_max_time_ns: None,
        };

        let mut scheduler1 = WorkloadScheduler::new(config.clone());
        let mut scheduler2 = WorkloadScheduler::new(config);

        let mut rng1 = SimRng::new(12345);
        let mut rng2 = SimRng::new(12345);

        // Process 2 ticks with both schedulers
        for i in 0..2 {
            let time = i * 10_000_000;
            let events_processed = i * 10;
            let events1 = scheduler1.handle_tick(time, events_processed, &mut rng1);
            let events2 = scheduler2.handle_tick(time, events_processed, &mut rng2);

            assert_eq!(events1.len(), events2.len(), "Same number of events");

            for (e1, e2) in events1.iter().zip(events2.iter()) {
                assert_eq!(e1.0, e2.0, "Same event times");
                // Event types should match (both Custom with same op_type)
                match (&e1.1, &e2.1) {
                    (EventKind::Custom(op1), EventKind::Custom(op2)) => {
                        assert_eq!(op1, op2, "Same operation types");
                    }
                    (EventKind::WorkloadTick, EventKind::WorkloadTick) => {
                        // OK
                    }
                    _ => panic!("Event types don't match"),
                }
            }
        }
    }

    #[test]
    fn test_vsr_mode_schedules_client_requests() {
        let config = WorkloadSchedulerConfig {
            ops_per_tick: 3,
            tick_interval_ns: 10_000_000,
            enhanced_workloads: false,
            max_scheduled_ops: Some(3),
            vsr_mode: true,
            sim_max_events: None,
            sim_max_time_ns: None,
        };

        let mut scheduler = WorkloadScheduler::new(config);
        let mut rng = SimRng::new(12345);

        let events = scheduler.handle_tick(0, 0, &mut rng);

        // Should have 3 VsrClientRequest events
        let client_requests: Vec<_> = events
            .iter()
            .filter(|(_, kind)| matches!(kind, EventKind::VsrClientRequest { .. }))
            .collect();

        assert_eq!(client_requests.len(), 3, "Should schedule 3 client requests");
    }

    #[test]
    fn test_ops_per_tick_batching() {
        let config = WorkloadSchedulerConfig {
            ops_per_tick: 10,
            tick_interval_ns: 10_000_000,
            enhanced_workloads: false,
            max_scheduled_ops: None,
            vsr_mode: false,
            sim_max_events: None,
            sim_max_time_ns: None,
        };

        let mut scheduler = WorkloadScheduler::new(config);
        let mut rng = SimRng::new(12345);

        let events = scheduler.handle_tick(0, 0, &mut rng);

        // Should have 10 Custom events + 1 WorkloadTick
        let custom_events: Vec<_> = events
            .iter()
            .filter(|(_, kind)| matches!(kind, EventKind::Custom(_)))
            .collect();

        assert_eq!(custom_events.len(), 10, "Should schedule 10 operations");
        assert_eq!(scheduler.ops_scheduled(), 10);
    }

    #[test]
    fn test_disable_stops_generation() {
        let config = WorkloadSchedulerConfig::default();
        let mut scheduler = WorkloadScheduler::new(config);
        let mut rng = SimRng::new(12345);

        // First tick should generate events
        let events1 = scheduler.handle_tick(0, 0, &mut rng);
        assert!(!events1.is_empty());

        // Disable scheduler
        scheduler.disable();
        assert!(!scheduler.is_enabled());

        // Second tick should generate nothing
        let events2 = scheduler.handle_tick(10_000_000, 10, &mut rng);
        assert!(events2.is_empty(), "Disabled scheduler should not generate events");
    }

    #[test]
    fn test_respects_sim_event_limit() {
        let config = WorkloadSchedulerConfig {
            ops_per_tick: 10,
            tick_interval_ns: 10_000_000,
            enhanced_workloads: false,
            max_scheduled_ops: None, // No workload limit
            vsr_mode: false,
            sim_max_events: Some(100), // But sim has limit
            sim_max_time_ns: None,
        };

        let mut scheduler = WorkloadScheduler::new(config);
        let mut rng = SimRng::new(12345);

        // Simulate approaching the event limit
        // When we're 95 events in, scheduler should stop (buffer = 100)
        let events = scheduler.handle_tick(0, 95, &mut rng);

        // Should not schedule next tick (would exceed limit)
        let has_tick = events.iter().any(|(_, kind)| matches!(kind, EventKind::WorkloadTick));
        assert!(!has_tick, "Should not schedule next tick when approaching event limit");
    }

    #[test]
    fn test_respects_sim_time_limit() {
        let config = WorkloadSchedulerConfig {
            ops_per_tick: 10,
            tick_interval_ns: 10_000_000,
            enhanced_workloads: false,
            max_scheduled_ops: None,
            vsr_mode: false,
            sim_max_events: None,
            sim_max_time_ns: Some(10_000_000_000), // 10 seconds
        };

        let mut scheduler = WorkloadScheduler::new(config);
        let mut rng = SimRng::new(12345);

        // Simulate approaching the time limit
        // 9.95 seconds (buffer = 100ms)
        let events = scheduler.handle_tick(9_950_000_000, 1000, &mut rng);

        // Should not schedule next tick (would exceed time limit)
        let has_tick = events.iter().any(|(_, kind)| matches!(kind, EventKind::WorkloadTick));
        assert!(!has_tick, "Should not schedule next tick when approaching time limit");
    }

    #[test]
    fn test_partial_batch_at_limit() {
        let config = WorkloadSchedulerConfig {
            ops_per_tick: 10,
            tick_interval_ns: 10_000_000,
            enhanced_workloads: false,
            max_scheduled_ops: Some(25),
            vsr_mode: false,
            sim_max_events: None,
            sim_max_time_ns: None,
        };

        let mut scheduler = WorkloadScheduler::new(config);
        let mut rng = SimRng::new(12345);

        // Tick 1: 10 ops
        scheduler.handle_tick(0, 0, &mut rng);
        assert_eq!(scheduler.ops_scheduled(), 10);

        // Tick 2: 10 ops (total 20)
        scheduler.handle_tick(10_000_000, 10, &mut rng);
        assert_eq!(scheduler.ops_scheduled(), 20);

        // Tick 3: Only 5 ops (reaches 25 limit)
        let events = scheduler.handle_tick(20_000_000, 20, &mut rng);
        let custom_count = events
            .iter()
            .filter(|(_, kind)| matches!(kind, EventKind::Custom(_)))
            .count();

        assert_eq!(custom_count, 5, "Should only schedule 5 ops to reach limit");
        assert_eq!(scheduler.ops_scheduled(), 25);
    }
}
