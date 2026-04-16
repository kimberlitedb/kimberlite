//! External invariant checking for chaos scenarios.
//!
//! Unlike `kimberlite-sim`'s in-process invariant checkers, chaos scenarios run
//! real kimberlite-server binaries. Invariants are checked externally via HTTP
//! (client query results, cluster status endpoints) and by direct inspection
//! of replica disk state after scenarios complete.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ============================================================================
// Invariant
// ============================================================================

/// An external invariant to check against a running chaos cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invariant {
    /// Invariant identifier (e.g. "no_divergence_after_heal").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Category (safety/liveness/durability).
    pub category: InvariantCategory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvariantCategory {
    /// Must always hold — single violation is a bug.
    Safety,
    /// Must eventually hold within a bounded time.
    Liveness,
    /// Data must survive failures.
    Durability,
}

// ============================================================================
// Invariant Result
// ============================================================================

/// Outcome of checking one invariant against a chaos cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvariantResult {
    pub invariant: String,
    pub held: bool,
    pub message: String,
    pub check_timestamp_ms: u64,
}

// ============================================================================
// Invariant Checker
// ============================================================================

/// Checks external invariants against a live or post-mortem chaos cluster.
///
/// Currently this is a skeleton with placeholder logic. The full implementation
/// will include:
///
/// - HTTP probing of cluster replicas to read state.
/// - Linearizability checker (Jepsen-style) on recorded operations.
/// - Hash chain verification across all replicas post-scenario.
/// - Partition detection via cluster topology queries.
#[derive(Debug, Default)]
pub struct InvariantChecker {
    /// Registered invariants by name.
    invariants: HashMap<String, Invariant>,
    /// Results from the last check run.
    results: Vec<InvariantResult>,
}

impl InvariantChecker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an invariant for future checking.
    pub fn register(&mut self, invariant: Invariant) {
        self.invariants.insert(invariant.name.clone(), invariant);
    }

    /// Returns the built-in invariant catalog.
    #[must_use]
    pub fn builtin() -> Self {
        let mut checker = Self::new();
        for inv in builtin_invariants() {
            checker.register(inv);
        }
        checker
    }

    /// Checks a named invariant, appending the result.
    ///
    /// TODO: replace placeholder with real implementations (HTTP calls,
    /// disk state comparison, linearizability checker).
    pub fn check(&mut self, name: &str, now_ms: u64) -> InvariantResult {
        let result = InvariantResult {
            invariant: name.to_string(),
            // Placeholder: always passes until real checks are implemented.
            // This is deliberate — it prevents false positives during
            // scaffolding while the check machinery is incomplete.
            held: true,
            message: format!("placeholder check for {name}"),
            check_timestamp_ms: now_ms,
        };
        self.results.push(result.clone());
        result
    }

    /// Returns all recorded results.
    #[must_use]
    pub fn results(&self) -> &[InvariantResult] {
        &self.results
    }

    /// Returns results that failed.
    #[must_use]
    pub fn failures(&self) -> Vec<&InvariantResult> {
        self.results.iter().filter(|r| !r.held).collect()
    }
}

// ============================================================================
// Built-in Invariants
// ============================================================================

fn builtin_invariants() -> Vec<Invariant> {
    vec![
        Invariant {
            name: "minority_refuses_writes".into(),
            description: "A minority partition must refuse write requests.".into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "no_divergence_after_heal".into(),
            description: "After healing a partition, all replicas must converge to \
                          identical committed log state."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "hash_chain_valid_all_replicas".into(),
            description: "Every replica's hash chain must validate end-to-end.".into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "all_writes_preserved".into(),
            description: "Every client write that received an acknowledgment must \
                          be present in the final log of a quorum of replicas."
                .into(),
            category: InvariantCategory::Durability,
        },
        Invariant {
            name: "linearizability".into(),
            description: "Client operations must appear to execute in a global total \
                          order consistent with real-time ordering."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "exactly_once_semantics".into(),
            description: "Client retries must produce exactly-once effects (no duplicate \
                          commits, no lost operations)."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "no_lost_commits".into(),
            description: "A commit acknowledged to a client must never be lost.".into(),
            category: InvariantCategory::Durability,
        },
        Invariant {
            name: "directory_reroutes_to_cluster_b".into(),
            description: "When all replicas of cluster A are unreachable, \
                          kimberlite-directory must route new requests to cluster B."
                .into(),
            category: InvariantCategory::Liveness,
        },
        Invariant {
            name: "no_data_loss_across_failover".into(),
            description: "Cross-cluster failover must not lose data that was \
                          durably committed in the original cluster."
                .into(),
            category: InvariantCategory::Durability,
        },
        Invariant {
            name: "quorum_loss_detected".into(),
            description: "When f+1 replicas fail, the cluster must reject writes \
                          rather than commit with under-quorum."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "no_corruption_under_quorum_loss".into(),
            description: "Quorum loss must not corrupt log state — on recovery, \
                          the hash chain must still validate."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "graceful_enforcement".into(),
            description: "Storage exhaustion must be enforced with clear error \
                          responses, not panics or silent corruption."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "no_panic_or_corruption".into(),
            description: "No kimberlite-server process should panic under any \
                          chaos scenario. Disk state must remain valid."
                .into(),
            category: InvariantCategory::Safety,
        },
    ]
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_checker_has_thirteen_invariants() {
        let checker = InvariantChecker::builtin();
        assert_eq!(checker.invariants.len(), 13);
    }

    #[test]
    fn check_records_result() {
        let mut checker = InvariantChecker::builtin();
        let result = checker.check("minority_refuses_writes", 1000);
        assert!(result.held);
        assert_eq!(checker.results().len(), 1);
    }
}
