//! Event dependency analysis for delta debugging.
//!
//! This module analyzes dependencies between logged events to enable
//! safe minimization while preserving causal relationships.

use std::collections::{HashMap, HashSet};

use crate::event_log::{Decision, LoggedEvent};

// ============================================================================
// Dependency Graph
// ============================================================================

/// Analyzes dependencies between simulation events.
pub struct DependencyAnalyzer {
    /// All events being analyzed.
    events: Vec<LoggedEvent>,
    /// Dependencies: event_id → set of events it depends on.
    dependencies: HashMap<u64, HashSet<u64>>,
}

impl DependencyAnalyzer {
    /// Analyzes event dependencies.
    pub fn analyze(events: &[LoggedEvent]) -> Self {
        let mut analyzer = Self {
            events: events.to_vec(),
            dependencies: HashMap::new(),
        };

        analyzer.build_dependency_graph();
        analyzer
    }

    /// Builds the dependency graph between events.
    fn build_dependency_graph(&mut self) {
        // Track state for dependency tracking
        let mut last_network_send: HashMap<u64, u64> = HashMap::new();
        let mut last_storage_op: HashMap<u64, u64> = HashMap::new();

        for event in &self.events {
            let event_id = event.event_id;
            let mut deps = HashSet::new();

            match &event.decision {
                Decision::NetworkDelay { message_id, .. } => {
                    // Record this send for later delivery
                    last_network_send.insert(*message_id, event_id);
                }

                Decision::NetworkDrop { message_id } => {
                    // Dropping depends on the message being sent
                    if let Some(&send_event) = last_network_send.get(message_id) {
                        deps.insert(send_event);
                    }
                }

                Decision::StorageComplete { operation_id, .. } => {
                    // Storage completion depends on the operation being started
                    if let Some(&op_event) = last_storage_op.get(operation_id) {
                        deps.insert(op_event);
                    }
                    last_storage_op.insert(*operation_id, event_id);
                }

                Decision::NodeRestart { .. } => {
                    // Node restart might depend on previous crash
                    // (simplified - real implementation would track node states)
                }

                Decision::ByzantineAttack { .. } => {
                    // Byzantine attacks may depend on network messages
                    // (simplified - would need detailed target tracking)
                }

                _ => {
                    // Other events have no special dependencies
                }
            }

            self.dependencies.insert(event_id, deps);
        }
    }

    /// Returns all events that the given event depends on (direct dependencies only).
    pub fn direct_dependencies(&self, event_id: u64) -> HashSet<u64> {
        self.dependencies
            .get(&event_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Returns the transitive closure of dependencies for an event.
    pub fn transitive_dependencies(&self, event_id: u64) -> HashSet<u64> {
        let mut closure = HashSet::new();
        let mut queue = vec![event_id];

        while let Some(id) = queue.pop() {
            if closure.insert(id) {
                if let Some(deps) = self.dependencies.get(&id) {
                    queue.extend(deps.iter().copied());
                }
            }
        }

        // Remove the event itself from its dependencies
        closure.remove(&event_id);
        closure
    }

    /// Checks if an event can be safely removed.
    ///
    /// An event is removable if:
    /// 1. It's not in the required set
    /// 2. No required event depends on it
    pub fn is_removable(&self, event_id: u64, required_events: &HashSet<u64>) -> bool {
        if required_events.contains(&event_id) {
            return false;
        }

        // Check if any required event depends on this one
        for &required_id in required_events {
            let deps = self.transitive_dependencies(required_id);
            if deps.contains(&event_id) {
                return false;
            }
        }

        true
    }

    /// Returns all events.
    pub fn events(&self) -> &[LoggedEvent] {
        &self.events
    }

    /// Returns the number of events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns true if there are no events.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_event(id: u64, decision: Decision) -> LoggedEvent {
        LoggedEvent {
            event_id: id,
            time_ns: id * 1000,
            decision,
        }
    }

    #[test]
    fn dependency_analyzer_basic() {
        let events = vec![
            create_test_event(0, Decision::RngValue { value: 42 }),
            create_test_event(
                1,
                Decision::NetworkDelay {
                    message_id: 100,
                    delay_ns: 1000,
                },
            ),
            create_test_event(2, Decision::NetworkDrop { message_id: 100 }),
        ];

        let analyzer = DependencyAnalyzer::analyze(&events);

        // Event 2 (drop) should depend on event 1 (send)
        let deps = analyzer.direct_dependencies(2);
        assert!(deps.contains(&1));
    }

    #[test]
    fn dependency_analyzer_transitive() {
        let events = vec![
            create_test_event(0, Decision::RngValue { value: 1 }),
            create_test_event(
                1,
                Decision::StorageComplete {
                    operation_id: 1,
                    success: true,
                    latency_ns: 100,
                },
            ),
            create_test_event(2, Decision::RngValue { value: 2 }),
        ];

        let analyzer = DependencyAnalyzer::analyze(&events);

        // Should have minimal dependencies in this simple case
        let deps = analyzer.transitive_dependencies(2);
        assert!(deps.is_empty()); // Event 2 doesn't depend on anything
    }

    #[test]
    fn dependency_analyzer_is_removable() {
        let events = vec![
            create_test_event(0, Decision::RngValue { value: 1 }),
            create_test_event(1, Decision::RngValue { value: 2 }),
            create_test_event(2, Decision::RngValue { value: 3 }),
        ];

        let analyzer = DependencyAnalyzer::analyze(&events);

        let mut required = HashSet::new();
        required.insert(2);

        // Events 0 and 1 should be removable if 2 is the only required event
        // (assuming no dependencies)
        assert!(analyzer.is_removable(0, &required));
        assert!(analyzer.is_removable(1, &required));
        assert!(!analyzer.is_removable(2, &required)); // Required events can't be removed
    }

    #[test]
    fn dependency_analyzer_network_dependency() {
        let events = vec![
            create_test_event(
                0,
                Decision::NetworkDelay {
                    message_id: 100,
                    delay_ns: 1000,
                },
            ),
            create_test_event(1, Decision::RngValue { value: 1 }),
            create_test_event(2, Decision::NetworkDrop { message_id: 100 }),
        ];

        let analyzer = DependencyAnalyzer::analyze(&events);

        let mut required = HashSet::new();
        required.insert(2); // Event 2 (drop) is required

        // Event 0 (send) should NOT be removable because event 2 depends on it
        assert!(!analyzer.is_removable(0, &required));

        // Event 1 should be removable
        assert!(analyzer.is_removable(1, &required));
    }
}
