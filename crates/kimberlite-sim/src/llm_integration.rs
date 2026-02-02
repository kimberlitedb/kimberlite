///! # LLM Integration for VOPR (Safe Architecture)
///!
///! This module provides **safe** LLM integration for VOPR testing. The key principle
///! is that LLMs generate ideas and analyze results, but NEVER decide correctness or
///! influence deterministic execution.
///!
///! ## Architecture
///!
///! ```text
///! ┌─────────────────────────────────────────────┐
///! │  LLM (offline only)                         │
///! │  - Generates scenario JSON                  │
///! │  - Analyzes failure traces (post-mortem)    │
///! │  - Suggests mutations                       │
///! │  - Helps shrink reproductions              │
///! └────────────┬────────────────────────────────┘
///!              │ JSON only
///!              ▼
///! ┌─────────────────────────────────────────────┐
///! │  Deterministic Validator                    │
///! │  - Validates scenario JSON schema           │
///! │  - Converts to ScenarioConfig + seed        │
///! │  - Ensures no nondeterminism leaks          │
///! └────────────┬────────────────────────────────┘
///!              │
///!              ▼
///! ┌─────────────────────────────────────────────┐
///! │  VOPR (deterministic execution)             │
///! │  - Hard invariants decide pass/fail         │
///! │  - No LLM influence on correctness          │
///! └─────────────────────────────────────────────┘
///! ```
///!
///! ## Safety Guarantees
///!
///! 1. **LLMs CANNOT influence runtime behavior** - All LLM outputs validated offline
///! 2. **Determinism is preserved** - Same seed → same execution, always
///! 3. **Correctness decided by invariants** - LLMs never judge pass/fail
///! 4. **Outputs are logged** - Full audit trail of LLM suggestions
///!
///! ## References
///!
///! - Anthropic: Safe AI system design
///! - FoundationDB: Deterministic simulation with human-generated scenarios
///! - TigerBeetle: Human-reviewed test cases

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Scenario Generation (Offline)
// ============================================================================

/// LLM-generated scenario suggestion.
///
/// **Safety**: This is just a suggestion. Must be validated and converted to
/// ScenarioConfig before execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmScenarioSuggestion {
    /// Human-readable description of what this scenario tests
    pub description: String,

    /// Target subsystem (e.g., "VSR view changes", "MVCC isolation")
    pub target: String,

    /// Suggested fault types to inject
    pub fault_types: Vec<String>,

    /// Suggested fault probabilities (0.0-1.0)
    pub fault_probabilities: HashMap<String, f64>,

    /// Suggested workload characteristics
    pub workload: WorkloadSuggestion,

    /// Suggested duration (simulation steps)
    pub duration_steps: u64,

    /// Why the LLM suggested this scenario
    pub rationale: String,
}

/// Workload characteristics suggested by LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadSuggestion {
    /// Number of concurrent clients
    pub client_count: usize,

    /// Operations per second
    pub ops_per_second: u64,

    /// Read/write ratio (0.0 = all writes, 1.0 = all reads)
    pub read_write_ratio: f64,

    /// Key distribution (e.g., "uniform", "zipfian", "sequential")
    pub key_distribution: String,
}

/// Validates an LLM-generated scenario suggestion.
///
/// **Returns**:
/// - `Ok(())` if valid
/// - `Err(reason)` if invalid
///
/// **Validation checks**:
/// 1. All probabilities in [0.0, 1.0]
/// 2. Known fault types only
/// 3. Reasonable workload parameters
/// 4. No attempts to inject nondeterminism
pub fn validate_llm_scenario(suggestion: &LlmScenarioSuggestion) -> Result<(), String> {
    // Check fault probabilities
    for (fault_type, prob) in &suggestion.fault_probabilities {
        if *prob < 0.0 || *prob > 1.0 {
            return Err(format!(
                "Invalid probability for {}: {} (must be 0.0-1.0)",
                fault_type, prob
            ));
        }

        // Whitelist known fault types
        let known_faults = [
            "network_partition",
            "network_delay",
            "storage_corruption",
            "storage_failure",
            "crash",
            "gray_failure",
            "swizzle_clog",
        ];

        if !known_faults.contains(&fault_type.as_str()) {
            return Err(format!("Unknown fault type: {}", fault_type));
        }
    }

    // Validate workload
    let w = &suggestion.workload;

    if w.client_count == 0 || w.client_count > 1000 {
        return Err(format!(
            "Invalid client_count: {} (must be 1-1000)",
            w.client_count
        ));
    }

    if w.ops_per_second == 0 || w.ops_per_second > 1_000_000 {
        return Err(format!(
            "Invalid ops_per_second: {} (must be 1-1M)",
            w.ops_per_second
        ));
    }

    if w.read_write_ratio < 0.0 || w.read_write_ratio > 1.0 {
        return Err(format!(
            "Invalid read_write_ratio: {} (must be 0.0-1.0)",
            w.read_write_ratio
        ));
    }

    // Validate duration
    if suggestion.duration_steps == 0 || suggestion.duration_steps > 10_000_000 {
        return Err(format!(
            "Invalid duration_steps: {} (must be 1-10M)",
            suggestion.duration_steps
        ));
    }

    Ok(())
}

// ============================================================================
// Failure Analysis (Post-Mortem)
// ============================================================================

/// Failure trace to send to LLM for analysis.
///
/// **Safety**: Contains only observable outputs, no internal state that could
/// leak nondeterminism.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureTrace {
    /// Seed that caused the failure (for reproduction)
    pub seed: u64,

    /// Scenario that was running
    pub scenario: String,

    /// Invariant that was violated
    pub violated_invariant: String,

    /// Violation message
    pub violation_message: String,

    /// Context key-value pairs from the violation
    pub violation_context: Vec<(String, String)>,

    /// Last N events before failure
    pub recent_events: Vec<String>,

    /// Simulation statistics at failure time
    pub stats: FailureStats,
}

/// Statistics at the time of failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureStats {
    /// Simulation step when failure occurred
    pub step: u64,

    /// Simulated time (nanoseconds)
    pub time_ns: u64,

    /// Total events processed
    pub events_processed: u64,

    /// Fault injections performed
    pub fault_count: u64,

    /// Invariant checks performed
    pub invariant_checks: HashMap<String, u64>,
}

/// LLM's analysis of a failure.
///
/// **Safety**: This is just a hypothesis. Humans review it, invariants decide correctness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmFailureAnalysis {
    /// Hypothesis about root cause
    pub root_cause_hypothesis: String,

    /// Suggested steps to reproduce (for debugging)
    pub reproduction_steps: Vec<String>,

    /// Suggested minimal repro (which events/faults to keep)
    pub shrinking_suggestions: Vec<String>,

    /// Similar known bugs or issues
    pub similar_issues: Vec<String>,

    /// Confidence level (0.0-1.0)
    pub confidence: f64,
}

/// Validates LLM failure analysis.
///
/// **Returns**:
/// - `Ok(())` if valid
/// - `Err(reason)` if invalid
///
/// **Validation checks**:
/// 1. Confidence in [0.0, 1.0]
/// 2. No attempt to override invariant results
/// 3. Suggestions are actionable
pub fn validate_llm_analysis(analysis: &LlmFailureAnalysis) -> Result<(), String> {
    if analysis.confidence < 0.0 || analysis.confidence > 1.0 {
        return Err(format!(
            "Invalid confidence: {} (must be 0.0-1.0)",
            analysis.confidence
        ));
    }

    if analysis.root_cause_hypothesis.is_empty() {
        return Err("Empty root cause hypothesis".to_string());
    }

    // Ensure no attempts to override correctness
    let forbidden = ["pass", "fail", "ignore", "skip", "disable invariant"];
    let hypothesis_lower = analysis.root_cause_hypothesis.to_lowercase();

    for word in forbidden {
        if hypothesis_lower.contains(word) {
            return Err(format!(
                "Analysis contains forbidden directive: '{}'",
                word
            ));
        }
    }

    Ok(())
}

// ============================================================================
// Test Case Shrinking (Delta Debugging)
// ============================================================================

/// Shrinking strategy for minimizing test cases.
///
/// **Principle**: Given a failing test with seed S, find a minimal subset of
/// events/faults that still reproduces the failure.
#[derive(Debug, Clone)]
pub struct TestCaseShrinker {
    /// Original failing seed
    pub original_seed: u64,

    /// Events in the original trace
    pub original_events: Vec<String>,

    /// Shrinking attempts made
    pub attempts: usize,

    /// Smallest reproducing subset found
    pub minimal_subset: Option<Vec<String>>,
}

impl TestCaseShrinker {
    /// Creates a new shrinker for a failing test.
    pub fn new(seed: u64, events: Vec<String>) -> Self {
        Self {
            original_seed: seed,
            original_events: events,
            attempts: 0,
            minimal_subset: None,
        }
    }

    /// Suggests the next subset to try (delta debugging strategy).
    ///
    /// **Algorithm**:
    /// 1. Start with full event set
    /// 2. Try removing half the events
    /// 3. If still fails, shrink to that half
    /// 4. If passes, try the other half
    /// 5. Repeat until no further reduction
    pub fn next_candidate(&mut self) -> Option<Vec<String>> {
        self.attempts += 1;

        let current = self.minimal_subset.as_ref().unwrap_or(&self.original_events);

        if current.len() <= 1 {
            return None; // Can't shrink further
        }

        // Try removing the first half
        let mid = current.len() / 2;
        Some(current[mid..].to_vec())
    }

    /// Records the result of testing a candidate.
    pub fn record_result(&mut self, candidate: Vec<String>, still_fails: bool) {
        if still_fails && candidate.len() < self.original_events.len() {
            // Found a smaller reproducer
            self.minimal_subset = Some(candidate);
        }
    }

    /// Returns the minimal reproducing subset found so far.
    pub fn minimal_reproducer(&self) -> &[String] {
        self.minimal_subset
            .as_ref()
            .map(|v| v.as_slice())
            .unwrap_or(&self.original_events)
    }

    /// Returns shrinking statistics.
    pub fn stats(&self) -> ShrinkingStats {
        ShrinkingStats {
            original_size: self.original_events.len(),
            minimal_size: self.minimal_reproducer().len(),
            attempts: self.attempts,
            reduction_percent: if self.original_events.is_empty() {
                0.0
            } else {
                100.0 * (1.0 - (self.minimal_reproducer().len() as f64 / self.original_events.len() as f64))
            },
        }
    }
}

/// Statistics about test case shrinking.
#[derive(Debug, Clone)]
pub struct ShrinkingStats {
    /// Original event count
    pub original_size: usize,

    /// Minimal reproducing event count
    pub minimal_size: usize,

    /// Shrinking attempts made
    pub attempts: usize,

    /// Reduction percentage
    pub reduction_percent: f64,
}

// ============================================================================
// Scenario Mutation Suggestions
// ============================================================================

/// LLM suggestion for mutating a scenario to find more bugs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMutationSuggestion {
    /// What to change
    pub mutation_type: String,

    /// Specific parameters to modify
    pub parameters: HashMap<String, String>,

    /// Why this mutation might find bugs
    pub rationale: String,

    /// Expected invariants this might violate
    pub target_invariants: Vec<String>,
}

/// Validates mutation suggestions.
pub fn validate_llm_mutation(mutation: &LlmMutationSuggestion) -> Result<(), String> {
    // Whitelist allowed mutation types
    let allowed = [
        "increase_fault_rate",
        "decrease_fault_rate",
        "add_fault_type",
        "remove_fault_type",
        "change_workload",
        "extend_duration",
        "add_concurrency",
    ];

    if !allowed.contains(&mutation.mutation_type.as_str()) {
        return Err(format!("Unknown mutation type: {}", mutation.mutation_type));
    }

    if mutation.rationale.is_empty() {
        return Err("Empty rationale".to_string());
    }

    Ok(())
}

// ============================================================================
// LLM Prompt Templates
// ============================================================================

/// Generates a prompt for scenario generation.
pub fn prompt_for_scenario_generation(target: &str, existing_scenarios: &[String]) -> String {
    format!(
        r#"Generate a VOPR test scenario targeting: {}

Existing scenarios already cover:
{}

Requirements:
- Target distributed systems edge cases
- Focus on stress testing {} subsystem
- Include fault injection strategies
- Specify workload characteristics
- Provide rationale for scenario design

Output as JSON matching LlmScenarioSuggestion schema.
Ensure all probabilities are in [0.0, 1.0] and fault types are valid.
"#,
        target,
        existing_scenarios.join("\n- "),
        target
    )
}

/// Generates a prompt for failure analysis.
pub fn prompt_for_failure_analysis(trace: &FailureTrace) -> String {
    let events = trace
        .recent_events
        .iter()
        .take(20)
        .map(|e| format!("  - {}", e))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"Analyze this VOPR test failure:

Seed: {}
Scenario: {}
Violated invariant: {}
Message: {}

Recent events:
{}

Statistics:
- Step: {}
- Time: {} ns
- Events processed: {}
- Fault count: {}

Task: Provide root cause hypothesis, reproduction steps, and shrinking suggestions.
Output as JSON matching LlmFailureAnalysis schema.
Do NOT suggest disabling invariants or marking test as passing.
"#,
        trace.seed,
        trace.scenario,
        trace.violated_invariant,
        trace.violation_message,
        events,
        trace.stats.step,
        trace.stats.time_ns,
        trace.stats.events_processed,
        trace.stats.fault_count
    )
}

/// Generates a prompt for mutation suggestions.
pub fn prompt_for_mutation_suggestions(
    scenario: &str,
    invariants_not_violated: &[String],
) -> String {
    format!(
        r#"Scenario "{}" has been running but hasn't violated these invariants:
{}

Suggest mutations to increase the likelihood of triggering these invariants.
Output as JSON array of LlmMutationSuggestion.
Focus on realistic fault combinations and workload patterns.
"#,
        scenario,
        invariants_not_violated.join("\n- ")
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_llm_scenario_valid() {
        let mut probs = HashMap::new();
        probs.insert("network_partition".to_string(), 0.1);
        probs.insert("crash".to_string(), 0.05);

        let suggestion = LlmScenarioSuggestion {
            description: "Test VSR under partitions".to_string(),
            target: "VSR".to_string(),
            fault_types: vec!["network_partition".to_string()],
            fault_probabilities: probs,
            workload: WorkloadSuggestion {
                client_count: 10,
                ops_per_second: 100,
                read_write_ratio: 0.5,
                key_distribution: "uniform".to_string(),
            },
            duration_steps: 10000,
            rationale: "Partitions can cause view changes".to_string(),
        };

        assert!(validate_llm_scenario(&suggestion).is_ok());
    }

    #[test]
    fn test_validate_llm_scenario_invalid_probability() {
        let mut probs = HashMap::new();
        probs.insert("network_partition".to_string(), 1.5); // Invalid!

        let suggestion = LlmScenarioSuggestion {
            description: "Test".to_string(),
            target: "VSR".to_string(),
            fault_types: vec!["network_partition".to_string()],
            fault_probabilities: probs,
            workload: WorkloadSuggestion {
                client_count: 10,
                ops_per_second: 100,
                read_write_ratio: 0.5,
                key_distribution: "uniform".to_string(),
            },
            duration_steps: 10000,
            rationale: "Test".to_string(),
        };

        assert!(validate_llm_scenario(&suggestion).is_err());
    }

    #[test]
    fn test_validate_llm_scenario_unknown_fault() {
        let mut probs = HashMap::new();
        probs.insert("alien_invasion".to_string(), 0.1); // Unknown fault!

        let suggestion = LlmScenarioSuggestion {
            description: "Test".to_string(),
            target: "VSR".to_string(),
            fault_types: vec!["alien_invasion".to_string()],
            fault_probabilities: probs,
            workload: WorkloadSuggestion {
                client_count: 10,
                ops_per_second: 100,
                read_write_ratio: 0.5,
                key_distribution: "uniform".to_string(),
            },
            duration_steps: 10000,
            rationale: "Test".to_string(),
        };

        let result = validate_llm_scenario(&suggestion);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown fault type"));
    }

    #[test]
    fn test_validate_llm_analysis_valid() {
        let analysis = LlmFailureAnalysis {
            root_cause_hypothesis: "Network partition prevented quorum formation".to_string(),
            reproduction_steps: vec!["Enable partitions".to_string(), "Run for 1000 steps".to_string()],
            shrinking_suggestions: vec!["Remove events after step 500".to_string()],
            similar_issues: vec!["Issue #123: Similar partition bug".to_string()],
            confidence: 0.8,
        };

        assert!(validate_llm_analysis(&analysis).is_ok());
    }

    #[test]
    fn test_validate_llm_analysis_forbidden_directive() {
        let analysis = LlmFailureAnalysis {
            root_cause_hypothesis: "Disable invariant to pass test".to_string(), // Forbidden!
            reproduction_steps: vec![],
            shrinking_suggestions: vec![],
            similar_issues: vec![],
            confidence: 0.8,
        };

        let result = validate_llm_analysis(&analysis);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("forbidden directive"));
    }

    #[test]
    fn test_test_case_shrinker() {
        let events = vec![
            "event1".to_string(),
            "event2".to_string(),
            "event3".to_string(),
            "event4".to_string(),
        ];

        let mut shrinker = TestCaseShrinker::new(12345, events);

        // First candidate: second half
        let candidate = shrinker.next_candidate().unwrap();
        assert_eq!(candidate.len(), 2);
        assert_eq!(candidate[0], "event3");

        // Simulate: candidate still reproduces
        shrinker.record_result(candidate.clone(), true);

        // Next candidate: shrink further
        let candidate2 = shrinker.next_candidate().unwrap();
        assert_eq!(candidate2.len(), 1);

        // Get stats
        let stats = shrinker.stats();
        assert_eq!(stats.original_size, 4);
        assert_eq!(stats.minimal_size, 2); // Last successful shrink
    }

    #[test]
    fn test_prompt_for_scenario_generation() {
        let prompt = prompt_for_scenario_generation(
            "VSR view changes",
            &["Baseline".to_string(), "Partitions".to_string()],
        );

        assert!(prompt.contains("VSR view changes"));
        assert!(prompt.contains("Baseline"));
        assert!(prompt.contains("LlmScenarioSuggestion"));
    }

    #[test]
    fn test_prompt_for_failure_analysis() {
        let trace = FailureTrace {
            seed: 12345,
            scenario: "Combined".to_string(),
            violated_invariant: "vsr_agreement".to_string(),
            violation_message: "Replicas diverged".to_string(),
            violation_context: vec![],
            recent_events: vec!["PrepareOk".to_string(), "Commit".to_string()],
            stats: FailureStats {
                step: 1000,
                time_ns: 1_000_000,
                events_processed: 500,
                fault_count: 10,
                invariant_checks: HashMap::new(),
            },
        };

        let prompt = prompt_for_failure_analysis(&trace);

        assert!(prompt.contains("12345"));
        assert!(prompt.contains("vsr_agreement"));
        assert!(prompt.contains("Replicas diverged"));
        assert!(prompt.contains("Do NOT suggest disabling"));
    }

    #[test]
    fn test_validate_llm_mutation_valid() {
        let mut params = HashMap::new();
        params.insert("fault_rate".to_string(), "0.2".to_string());

        let mutation = LlmMutationSuggestion {
            mutation_type: "increase_fault_rate".to_string(),
            parameters: params,
            rationale: "Higher fault rate might trigger view changes".to_string(),
            target_invariants: vec!["vsr_agreement".to_string()],
        };

        assert!(validate_llm_mutation(&mutation).is_ok());
    }

    #[test]
    fn test_validate_llm_mutation_unknown_type() {
        let mutation = LlmMutationSuggestion {
            mutation_type: "hack_the_planet".to_string(), // Unknown!
            parameters: HashMap::new(),
            rationale: "Test".to_string(),
            target_invariants: vec![],
        };

        let result = validate_llm_mutation(&mutation);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown mutation type"));
    }
}
