//! VSR event scheduler for integration with VOPR simulation loop.
//!
//! This module provides functions to schedule VSR-specific events in the
//! simulation event queue. It bridges VsrSimulation with the VOPR event loop.

use kimberlite_vsr::Message;

use crate::{EventKind, EventQueue, SimRng, vsr_message_to_bytes};

// ============================================================================
// Event Scheduling
// ============================================================================

/// Schedules a client request to be processed by a VSR replica.
///
/// # Parameters
///
/// - `queue`: The simulation event queue
/// - `current_time_ns`: Current simulation time
/// - `delay_ns`: Delay before the event fires
/// - `replica_id`: Which replica should process the request
///
/// # Returns
///
/// The event ID of the scheduled event.
pub fn schedule_client_request(
    queue: &mut EventQueue,
    current_time_ns: u64,
    delay_ns: u64,
    replica_id: u8,
) -> crate::EventId {
    queue.schedule(
        current_time_ns + delay_ns,
        EventKind::VsrClientRequest {
            replica_id,
            command_bytes: vec![], // Filled in by event handler
            idempotency_id: None,
        },
    )
}

/// Schedules VSR message deliveries from a list of messages.
///
/// This takes messages output by a replica and schedules their delivery
/// through the network layer.
///
/// # Parameters
///
/// - `queue`: The simulation event queue
/// - `current_time_ns`: Current simulation time
/// - `messages`: VSR messages to deliver
/// - `rng`: Random number generator for network latency
/// - `min_delay_ns`: Minimum network delay
/// - `max_delay_ns`: Maximum network delay
///
/// # Returns
///
/// Number of messages scheduled.
pub fn schedule_vsr_messages(
    queue: &mut EventQueue,
    current_time_ns: u64,
    messages: &[Message],
    rng: &mut SimRng,
    min_delay_ns: u64,
    max_delay_ns: u64,
) -> usize {
    let mut count = 0;

    for msg in messages {
        // Determine destination(s)
        if let Some(to) = msg.to {
            // Unicast message
            let delay = rng.delay_ns(min_delay_ns, max_delay_ns);
            let message_bytes = vsr_message_to_bytes(msg);

            queue.schedule(
                current_time_ns + delay,
                EventKind::VsrMessage {
                    to_replica: to.as_u8(),
                    message_bytes,
                },
            );
            count += 1;
        } else {
            // Broadcast message - send to all replicas except sender
            for replica_id in 0..3u8 {
                if replica_id != msg.from.as_u8() {
                    let delay = rng.delay_ns(min_delay_ns, max_delay_ns);
                    let message_bytes = vsr_message_to_bytes(msg);

                    queue.schedule(
                        current_time_ns + delay,
                        EventKind::VsrMessage {
                            to_replica: replica_id,
                            message_bytes,
                        },
                    );
                    count += 1;
                }
            }
        }
    }

    count
}

/// Schedules a VSR timeout event for a replica.
///
/// # Parameters
///
/// - `queue`: The simulation event queue
/// - `current_time_ns`: Current simulation time
/// - `timeout_delay_ns`: When the timeout should fire
/// - `replica_id`: Which replica times out
/// - `timeout_kind`: Type of timeout (0=Heartbeat, 1=Prepare, 2=ViewChange)
pub fn schedule_vsr_timeout(
    queue: &mut EventQueue,
    current_time_ns: u64,
    timeout_delay_ns: u64,
    replica_id: u8,
    timeout_kind: u8,
) -> crate::EventId {
    queue.schedule(
        current_time_ns + timeout_delay_ns,
        EventKind::VsrTimeout {
            replica_id,
            timeout_kind,
        },
    )
}

/// Schedules periodic tick events for all replicas.
///
/// Tick events drive housekeeping tasks like sending heartbeats.
///
/// # Parameters
///
/// - `queue`: The simulation event queue
/// - `current_time_ns`: Current simulation time
/// - `tick_interval_ns`: How often ticks fire
///
/// # Returns
///
/// Number of tick events scheduled (one per replica).
pub fn schedule_vsr_ticks(
    queue: &mut EventQueue,
    current_time_ns: u64,
    tick_interval_ns: u64,
) -> usize {
    for replica_id in 0..3u8 {
        queue.schedule(
            current_time_ns + tick_interval_ns,
            EventKind::VsrTick { replica_id },
        );
    }
    3 // Scheduled one tick per replica
}

/// Schedules a replica crash event.
///
/// # Parameters
///
/// - `queue`: The simulation event queue
/// - `time_ns`: When the crash occurs
/// - `replica_id`: Which replica crashes
pub fn schedule_vsr_crash(
    queue: &mut EventQueue,
    time_ns: u64,
    replica_id: u8,
) -> crate::EventId {
    queue.schedule(time_ns, EventKind::VsrCrash { replica_id })
}

/// Schedules a replica recovery event.
///
/// # Parameters
///
/// - `queue`: The simulation event queue
/// - `time_ns`: When the recovery completes
/// - `replica_id`: Which replica recovers
pub fn schedule_vsr_recover(
    queue: &mut EventQueue,
    time_ns: u64,
    replica_id: u8,
) -> crate::EventId {
    queue.schedule(time_ns, EventKind::VsrRecover { replica_id })
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Computes a random delay for client request arrival.
///
/// Models exponential inter-arrival times for a Poisson process.
///
/// # Parameters
///
/// - `rng`: Random number generator
/// - `mean_interval_ns`: Mean time between requests
///
/// # Returns
///
/// Delay in nanoseconds until next request.
pub fn random_client_request_delay(rng: &mut SimRng, mean_interval_ns: u64) -> u64 {
    // Simple approximation: uniform between 0 and 2*mean
    rng.delay_ns(0, mean_interval_ns * 2)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_vsr::{MessagePayload, ReplicaId, StartViewChange, ViewNumber};

    #[test]
    fn test_schedule_client_request() {
        let mut queue = EventQueue::new();
        let id = schedule_client_request(&mut queue, 1000, 500, 0);

        let event = queue.pop().expect("should have event");
        assert_eq!(event.id, id);
        assert_eq!(event.time_ns, 1500);
        assert!(matches!(event.kind, EventKind::VsrClientRequest { .. }));
    }

    #[test]
    fn test_schedule_unicast_message() {
        let mut queue = EventQueue::new();
        let mut rng = SimRng::new(42);

        let msg = Message::targeted(
            ReplicaId::new(0),
            ReplicaId::new(1),
            MessagePayload::StartViewChange(StartViewChange {
                view: ViewNumber::from(1),
                replica: ReplicaId::new(0),
            }),
        );

        let count = schedule_vsr_messages(&mut queue, 1000, &[msg], &mut rng, 100, 200);

        assert_eq!(count, 1);
        assert_eq!(queue.len(), 1);

        let event = queue.pop().expect("should have event");
        assert!(matches!(event.kind, EventKind::VsrMessage { .. }));
    }

    #[test]
    fn test_schedule_broadcast_message() {
        let mut queue = EventQueue::new();
        let mut rng = SimRng::new(42);

        let msg = Message::broadcast(
            ReplicaId::new(0),
            MessagePayload::StartViewChange(StartViewChange {
                view: ViewNumber::from(1),
                replica: ReplicaId::new(0),
            }),
        );

        let count = schedule_vsr_messages(&mut queue, 1000, &[msg], &mut rng, 100, 200);

        // Broadcast to 2 other replicas (not sender)
        assert_eq!(count, 2);
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn test_schedule_timeout() {
        let mut queue = EventQueue::new();
        let id = schedule_vsr_timeout(&mut queue, 1000, 500, 0, 0);

        let event = queue.pop().expect("should have event");
        assert_eq!(event.id, id);
        assert_eq!(event.time_ns, 1500);
        assert!(matches!(event.kind, EventKind::VsrTimeout { .. }));
    }

    #[test]
    fn test_schedule_ticks() {
        let mut queue = EventQueue::new();
        let count = schedule_vsr_ticks(&mut queue, 1000, 100);

        assert_eq!(count, 3); // One per replica
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn test_schedule_crash_recover() {
        let mut queue = EventQueue::new();

        schedule_vsr_crash(&mut queue, 1000, 1);
        schedule_vsr_recover(&mut queue, 2000, 1);

        assert_eq!(queue.len(), 2);

        let crash_event = queue.pop().expect("should have crash");
        assert!(matches!(crash_event.kind, EventKind::VsrCrash { .. }));

        let recover_event = queue.pop().expect("should have recover");
        assert!(matches!(recover_event.kind, EventKind::VsrRecover { .. }));
    }

    #[test]
    fn test_random_client_request_delay() {
        let mut rng = SimRng::new(42);
        let mean = 1_000_000; // 1ms

        let delay = random_client_request_delay(&mut rng, mean);

        // Should be between 0 and 2*mean
        assert!(delay <= mean * 2);
    }
}
