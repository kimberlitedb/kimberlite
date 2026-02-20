---
title: "Safe LLM Integration with VOPR"
section: "internals/design"
slug: "llm-integration"
order: 6
---

# Safe LLM Integration with VOPR

This document explains how Large Language Models (LLMs) are used in Kimberlite's VOPR testing framework **without compromising determinism or correctness**.

---

## Core Principle

**LLMs suggest, validators verify, invariants decide.**

LLMs are **idea generators**, not **judges**.

---

## The Risk: Nondeterminism

LLMs are probabilistic. If you use an LLM **during** a VOPR run to make decisions, you break determinism:

```
❌ BAD: LLM in the loop
┌─────────────┐
│  VOPR run   │
│  (seed=42)  │
└──────┬──────┘
       │
       ▼
┌─────────────┐
│ Should we   │  ← LLM decides
│ inject a    │     (nondeterministic!)
│ fault?      │
└──────┬──────┘
       │
       ▼
Same seed ≠ Same execution  ← BROKEN
```

**Result**: Bugs are irreproducible, VOPR is useless.

---

## The Solution: Offline-Only LLMs

LLMs operate **before** or **after** VOPR runs, never **during**:

```
✅ GOOD: LLM offline

1. GENERATE (offline)
   ┌─────────────┐
   │  LLM        │ → scenario.json (validated)
   └─────────────┘

2. EXECUTE (deterministic)
   ┌─────────────┐
   │  VOPR run   │ → same seed = same execution
   │  (seed=42)  │
   └─────────────┘

3. ANALYZE (offline)
   ┌─────────────┐
   │  LLM        │ → hypothesis + suggestions
   └─────────────┘
```

**Result**: Determinism preserved, LLMs enhance testing.

---

## Architecture

### Strict Separation

```
┌────────────────────────────────────────────────┐
│  LLM Layer (offline)                           │
│  - Generates scenario JSON                     │
│  - Analyzes failure traces                     │
│  - Suggests mutations                          │
│  - Helps shrink test cases                     │
└───────────────┬────────────────────────────────┘
                │ JSON only
                ▼
┌────────────────────────────────────────────────┐
│  Validation Layer (deterministic)              │
│  - Schema validation                           │
│  - Whitelist checks (fault types, mutations)   │
│  - Range checks (probabilities [0.0, 1.0])     │
│  - Forbidden directive scan                    │
└───────────────┬────────────────────────────────┘
                │
                ▼
┌────────────────────────────────────────────────┐
│  VOPR (deterministic execution)                │
│  - Hard invariants decide pass/fail            │
│  - No LLM influence on correctness             │
└────────────────────────────────────────────────┘
```

### Safety Guarantees

**LLMs CANNOT**:
- ❌ Influence deterministic execution
- ❌ Override invariant decisions
- ❌ Inject nondeterminism mid-simulation
- ❌ Skip checks or disable faults
- ❌ Modify seeds or RNG state

**LLMs CAN**:
- ✅ Generate scenario JSON (validated before use)
- ✅ Analyze failure traces (post-mortem only)
- ✅ Suggest code paths to investigate
- ✅ Recommend mutations to try
- ✅ Assist with test case reduction

---

## Use Case 1: Scenario Generation

### Goal

Generate adversarial scenarios to stress-test specific properties (e.g., view changes, MVCC visibility, tenant isolation).

### Workflow

**1. Generate Prompt**

```rust
use kimberlite_sim::llm_integration::prompt_for_scenario_generation;

let prompt = prompt_for_scenario_generation(
    "stress view changes under packet loss",
    &["baseline", "swizzle_clogging"]
);

// Output:
// "You are a distributed systems testing expert. Generate a VOPR scenario
//  to stress-test: view changes under packet loss
//
//  Existing scenarios:
//  - baseline
//  - swizzle_clogging
//
//  Requirements:
//  - Focus on realistic adversarial conditions
//  - Use fault injection types: network_partition, packet_delay, packet_drop,
//    storage_corruption, crash
//  - Keep probabilities low (0.001 - 0.05 range)
//  - Provide clear rationale
//
//  Output valid JSON matching LlmScenarioSuggestion schema."
```

**2. Call LLM (Claude, GPT, etc.)**

```rust
// Using Claude API (example)
let llm_response = call_claude_api(prompt)?;
```

LLM returns JSON:

```json
{
  "description": "High packet loss with delayed view changes",
  "target": "stress view changes under packet loss",
  "fault_types": ["packet_delay", "packet_drop", "network_partition"],
  "fault_probabilities": {
    "packet_delay": 0.02,
    "packet_drop": 0.01,
    "network_partition": 0.005
  },
  "workload": {
    "operations_per_second": 1000,
    "read_ratio": 0.7,
    "write_ratio": 0.3,
    "tenants": 5
  },
  "duration_steps": 500000,
  "rationale": "Packet loss delays view change messages, increasing the
               chance of split-brain or stale view scenarios."
}
```

**3. Validate** (CRITICAL STEP)

```rust
use kimberlite_sim::llm_integration::{LlmScenarioSuggestion, validate_llm_scenario};

let suggestion: LlmScenarioSuggestion = serde_json::from_str(&llm_response)?;

// Validation checks:
// - All probabilities in [0.0, 1.0]
// - Only known fault types (whitelist)
// - Workload parameters reasonable (ops/sec < 100k, tenants < 1000)
// - No attempts to inject nondeterminism
validate_llm_scenario(&suggestion)?;
```

If validation fails:

```
❌ LLM scenario validation failed:
  - Unknown fault type: "nuclear_launch" (allowed: packet_delay, packet_drop, ...)
  - Probability out of range: packet_delay=1.5 (must be in [0.0, 1.0])
```

**4. Convert to VOPR Config**

```rust
let config = VoprConfig::from_llm_suggestion(&suggestion);
let mut runner = VoprRunner::new(config);
runner.run()?;
```

Now VOPR runs deterministically with the LLM-generated scenario.

### Safety Mechanism

Validation is **mandatory defense-in-depth**:
- Whitelist of allowed fault types
- Range checks on all numeric values
- Schema enforcement (JSON structure)
- Forbidden directive scanning

LLMs can't bypass this validation.

---

## Use Case 2: Failure Analysis

### Goal

When VOPR detects an invariant violation, use an LLM to suggest root causes and next steps.

### Workflow

**1. Collect Failure Data**

```rust
use kimberlite_sim::llm_integration::FailureTrace;

let trace = FailureTrace {
    seed: 42,
    scenario: "combined".to_string(),
    violated_invariant: "LinearizabilityChecker".to_string(),
    violation_message: "Read observed stale value".to_string(),
    violation_context: vec![
        ("key".to_string(), "x".to_string()),
        ("expected".to_string(), "1".to_string()),
        ("observed".to_string(), "0".to_string()),
    ],
    recent_events: vec![
        "[1000ms] NetworkPartition applied".to_string(),
        "[1005ms] Client write: key=x, value=1".to_string(),
        "[1010ms] Client read: key=x, observed=0".to_string(),
    ],
    stats: FailureStats {
        events_processed: 5000,
        fault_injections: 12,
        view_changes: 2,
        repairs: 0,
    },
};
```

**2. Generate Analysis Prompt**

```rust
let prompt = prompt_for_failure_analysis(&trace);

// Output:
// "Analyze this VOPR failure:
//
//  Seed: 42
//  Scenario: combined
//  Violated Invariant: LinearizabilityChecker
//  Message: Read observed stale value
//
//  Recent Events:
//  - [1000ms] NetworkPartition applied
//  - [1005ms] Client write: key=x, value=1
//  - [1010ms] Client read: key=x, observed=0 (expected=1)
//
//  Stats:
//  - Events processed: 5000
//  - Fault injections: 12
//
//  Provide:
//  1. Root cause hypothesis
//  2. Related invariants to check
//  3. Suggested mutations to isolate bug
//  4. Relevant code paths"
```

**3. Call LLM**

```rust
let llm_response = call_claude_api(prompt)?;
```

LLM returns JSON:

```json
{
  "hypothesis": "Network partition caused write to commit on majority but not
                 reach the replica serving the read. Read served stale data
                 from minority partition.",
  "related_invariants": [
    "vsr_agreement",
    "replica_consistency",
    "read_your_writes"
  ],
  "suggested_mutations": [
    "Increase partition probability to 0.1",
    "Add repair delay to prevent quick catchup",
    "Run with single-client workload to isolate"
  ],
  "confidence": 0.8,
  "relevant_code_paths": [
    "crates/kimberlite-vsr/src/replica.rs:prepare_phase",
    "crates/kimberlite-vsr/src/replica.rs:commit_phase",
    "crates/kimberlite-sim/src/network.rs:partition_apply"
  ]
}
```

**4. Validate** (CRITICAL STEP)

```rust
use kimberlite_sim::llm_integration::{LlmFailureAnalysis, validate_llm_analysis};

let analysis: LlmFailureAnalysis = serde_json::from_str(&llm_response)?;

// Validation checks:
// - Confidence in [0.0, 1.0]
// - No forbidden directives ("skip_invariant", "override_seed", etc.)
// - Field lengths reasonable (< 10k chars)
validate_llm_analysis(&analysis)?;
```

**5. Human Reviews**

The analysis is presented to a human:

```
Root Cause Hypothesis (confidence: 80%):
  Network partition caused write to commit on majority but not reach the
  replica serving the read. Read served stale data from minority partition.

Related Invariants:
  - vsr_agreement
  - replica_consistency
  - read_your_writes

Suggested Mutations:
  1. Increase partition probability to 0.1
  2. Add repair delay to prevent quick catchup
  3. Run with single-client workload to isolate

Relevant Code Paths:
  - crates/kimberlite-vsr/src/replica.rs:prepare_phase
  - crates/kimberlite-vsr/src/replica.rs:commit_phase
```

Human decides whether to follow the suggestions.

### Safety Mechanism

- LLM **never** decides correctness (invariants do that)
- LLM output is **informational** only
- Human reviews before taking action
- Forbidden directives blocked ("skip this check", "assume this is fine")

---

## Use Case 3: Test Case Shrinking

### Goal

When a failure is found, reduce it to the **minimal reproducing case** (delta debugging).

### Workflow

**1. Start with Full Failure**

```rust
use kimberlite_sim::llm_integration::TestCaseShrinker;

let events = vec![
    "e1".to_string(), "e2".to_string(), "e3".to_string(),
    "e4".to_string(), "e5".to_string(), "e6".to_string(),
    // ... 100 events total
];

let mut shrinker = TestCaseShrinker::new(failing_seed, events);
```

**2. Binary Search for Minimal Subset**

```rust
while let Some(candidate) = shrinker.next_candidate() {
    // Try running VOPR with subset of events
    let still_fails = run_vopr_with_events(failing_seed, &candidate)?;

    // Record result
    shrinker.record_attempt(candidate, still_fails);

    if still_fails && shrinker.minimal_subset.as_ref().unwrap().len() == 1 {
        break; // Found minimal case (single event!)
    }
}

println!("Minimal reproducing case: {:?}", shrinker.minimal_subset);
```

**3. LLM-Assisted Heuristics (Optional)**

Instead of binary search, ask LLM which events to try removing first:

```
Prompt: "Given this failure trace, which events are most likely irrelevant?
         [Event list]
         Focus on events related to: LinearizabilityChecker violation"

LLM: "Events e10, e15, e20 are likely unrelated (they're tenant 2 operations,
      but the failure is in tenant 1). Try removing those first."
```

This is a **heuristic** - the LLM doesn't decide, just guides the search order.

### Safety Mechanism

- Validation always checks if bug still reproduces
- LLM can't force a "minimal case" that doesn't actually fail
- Human verifies final minimal case

---

## Use Case 4: Mutation Suggestions

### Goal

VOPR ran without violations. LLM suggests variations that might trigger dormant bugs.

### Workflow

**1. Identify Invariants That Didn't Trigger**

```rust
let invariants_not_violated = vec![
    "vsr_view_change_safety",
    "projection_mvcc_visibility",
];
```

**2. Generate Mutation Prompt**

```rust
let prompt = prompt_for_mutation_suggestions(
    "combined",
    &invariants_not_violated
);

// Output:
// "Scenario 'combined' ran but did NOT violate these invariants:
//  - vsr_view_change_safety
//  - projection_mvcc_visibility
//
//  Suggest mutations to stress these invariants specifically."
```

**3. Call LLM**

LLM returns suggestions:

```json
{
  "mutation_type": "increase_fault_rate",
  "target": "view_change_safety",
  "parameters": {
    "fault": "network_partition",
    "new_rate": "0.05"
  },
  "expected_invariants": ["vsr_view_change_safety", "vsr_agreement"],
  "rationale": "Higher partition rate increases likelihood of view changes
                during commits, stressing view change safety invariant."
}
```

**4. Validate**

```rust
use kimberlite_sim::llm_integration::{LlmMutationSuggestion, validate_llm_mutation};

let mutation: LlmMutationSuggestion = serde_json::from_str(&llm_response)?;

// Validation checks:
// - Only known mutation types (increase_fault_rate, add_partition, extend_duration)
// - Parameters within bounds
validate_llm_mutation(&mutation)?;
```

**5. Apply Mutation**

```rust
let mut config = VoprConfig::default();
config.network_fault_rate = 0.05; // Increased from 0.01
runner.run_with_config(config)?;
```

### Safety Mechanism

- Whitelist of allowed mutation types
- Parameter bounds enforced
- Mutations don't bypass invariants (they increase stress, not reduce checks)

---

## Validation: Defense-in-Depth

All LLM outputs pass through **mandatory validation**:

### 1. Schema Validation

JSON structure must match expected schema (serde deserialization).

### 2. Whitelist Checks

Only allow known values:
- **Fault types**: `packet_delay`, `packet_drop`, `network_partition`, `storage_corruption`, `crash`
- **Mutation types**: `increase_fault_rate`, `add_partition`, `extend_duration`, `add_workload`, `enable_repair_delay`

Unknown values → rejected.

### 3. Range Checks

Numeric values must be in bounds:
- Probabilities: `[0.0, 1.0]`
- Operations/sec: `< 100,000`
- Tenants: `< 1,000`
- Duration steps: `< 10,000,000`

Out-of-range → rejected.

### 4. Forbidden Directive Scan

Reject outputs containing:
- `"skip_invariant"`
- `"override_seed"`
- `"disable_checks"`
- `"bypass_validation"`
- `"force_pass"`

Case-insensitive substring match.

### 5. Length Limits

Text fields capped:
- Descriptions: < 10,000 chars
- Rationale: < 5,000 chars
- Code paths: < 500 chars each

Prevents prompt injection or exfiltration attempts.

---

## Comparison to Naive LLM Integration

| Approach | Determinism | Safety | Usefulness |
|----------|-------------|--------|------------|
| **LLM decides correctness** | ❌ Broken | ❌ Unsafe | ⚠️ High risk |
| **LLM in VOPR loop** | ❌ Broken | ❌ Unsafe | ⚠️ Nondeterministic |
| **LLM offline (validated)** | ✅ Preserved | ✅ Safe | ✅ High value |

Kimberlite uses **LLM offline (validated)** exclusively.

**Note**: Planned LLM integration enhancements are documented in [ROADMAP.md](../../../ROADMAP.md#llm-integration-enhancements).

---

## Best Practices

### ✅ DO

- Always validate LLM output before using it
- Use LLMs for **idea generation**, not **decision-making**
- Keep LLMs **offline** (before/after VOPR runs, never during)
- Review LLM suggestions before acting
- Track LLM usage in logs (prompt + response for audit)

### ❌ DON'T

- Let LLMs decide invariant pass/fail
- Use LLMs during deterministic execution
- Skip validation ("it's just a suggestion")
- Blindly apply LLM-generated mutations
- Use LLMs for security-critical decisions

---

## Example: End-to-End Workflow

**Goal**: Find bugs in view change logic.

**Step 1: Generate Scenario**

```bash
# Prompt LLM
echo "Generate a VOPR scenario to stress view changes" | \
  llm-cli --model claude-opus-4.5 > scenario-raw.json

# Validate
python3 scripts/validate-llm-scenario.py scenario-raw.json > scenario.json
```

**Step 2: Run VOPR**

```bash
cargo run --release -p kimberlite-sim --bin vopr -- \
  --scenario scenario.json \
  --iterations 100000 \
  --json > results.json
```

**Step 3: Analyze Failure (if any)**

```bash
# Extract failure trace
jq '.failure_trace' results.json > failure.json

# Get LLM analysis
cat failure.json | llm-cli --model claude-opus-4.5 > analysis.json

# Review
cat analysis.json | jq '.hypothesis, .suggested_mutations'
```

**Step 4: Iterate**

- Apply suggested mutations
- Re-run VOPR
- Compare results

**Result**: LLM-guided testing workflow, determinism preserved.

---

## References

- **Implementation**: `/crates/kimberlite-sim/src/llm_integration.rs`
- **Tests**: `/crates/kimberlite-sim/src/llm_integration.rs` (11 tests)
- **Philosophy**: `/docs/TESTING.md` (VOPR section)

---

**Last Updated**: 2026-02-02
**Status**: Phase 9 complete (core functionality), CLI tools planned
