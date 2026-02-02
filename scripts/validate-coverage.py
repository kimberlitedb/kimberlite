#!/usr/bin/env python3
"""
Validates VOPR coverage against nightly thresholds.

Reads VOPR JSON output files and checks:
- Fault point coverage >= 90%
- All critical invariants executed
- View changes >= 5
- Phase events met minimum counts

Exit code 0: all thresholds met
Exit code 1: one or more thresholds violated
"""

import json
import sys
from pathlib import Path
from typing import Dict, List, Any

# Nightly thresholds (matches CoverageThresholds::nightly() in Rust)
NIGHTLY_THRESHOLDS = {
    "fault_point_coverage_min": 0.9,
    "critical_fault_points": [
        "sim.storage.fsync",
        "sim.storage.write",
        "sim.storage.read",
        "sim.storage.crash",
        "sim.network.send",
        "sim.network.deliver",
        "sim.network.partition",
    ],
    "required_invariants": [
        "linearizability",
        "hash_chain",
        "replica_consistency",
        "storage_determinism",
        "vsr_agreement",
        "vsr_prefix_property",
        "vsr_view_change_safety",
        "vsr_recovery_safety",
        "projection_applied_position_monotonic",
        "projection_mvcc_visibility",
        "projection_applied_index_integrity",
        "sql_tlp_partitioning",
        "sql_norec_equivalence",
    ],
    "min_view_changes": 5,
    "min_repairs": 2,
    "min_unique_query_plans": 20,
    "min_phase_events": {
        "vsr.prepare_sent": 100,
        "vsr.commit_broadcast": 50,
        "vsr.view_change_complete": 5,
        "storage.fsync_complete": 100,
        "storage.repair_complete": 2,
    },
}


def load_vopr_result(filepath: Path) -> Dict[str, Any]:
    """Load VOPR JSON result file."""
    with open(filepath, 'r') as f:
        return json.load(f)


def validate_result(result: Dict[str, Any], filepath: Path) -> List[str]:
    """Validate a single VOPR result against nightly thresholds."""
    violations = []

    # Extract coverage data (may be in different locations depending on VOPR version)
    coverage = result.get("coverage", {})

    # Check 1: Fault point coverage
    total_fault_points = coverage.get("total_fault_points", 0)
    fault_points_hit = coverage.get("fault_points_hit", 0)
    if total_fault_points > 0:
        fault_coverage = fault_points_hit / total_fault_points
        if fault_coverage < NIGHTLY_THRESHOLDS["fault_point_coverage_min"]:
            violations.append(
                f"{filepath.name}: Fault point coverage too low: {fault_coverage:.1%} "
                f"(expected >= {NIGHTLY_THRESHOLDS['fault_point_coverage_min']:.1%})"
            )

    # Check 2: Critical fault points
    hit_fault_points = set(coverage.get("hit_fault_points", []))
    for critical_fp in NIGHTLY_THRESHOLDS["critical_fault_points"]:
        if critical_fp not in hit_fault_points:
            violations.append(
                f"{filepath.name}: Critical fault point '{critical_fp}' never hit"
            )

    # Check 3: Required invariants
    invariant_execs = coverage.get("invariant_executions", {})
    for required_inv in NIGHTLY_THRESHOLDS["required_invariants"]:
        exec_count = invariant_execs.get(required_inv, 0)
        if exec_count == 0:
            violations.append(
                f"{filepath.name}: Required invariant '{required_inv}' never executed"
            )

    # Check 4: View changes
    view_changes = coverage.get("view_changes", 0)
    if view_changes < NIGHTLY_THRESHOLDS["min_view_changes"]:
        violations.append(
            f"{filepath.name}: Insufficient view changes: {view_changes} "
            f"(expected >= {NIGHTLY_THRESHOLDS['min_view_changes']})"
        )

    # Check 5: Repairs
    repairs = coverage.get("repairs", 0)
    if repairs < NIGHTLY_THRESHOLDS["min_repairs"]:
        violations.append(
            f"{filepath.name}: Insufficient repairs: {repairs} "
            f"(expected >= {NIGHTLY_THRESHOLDS['min_repairs']})"
        )

    # Check 6: Unique query plans
    unique_query_plans = coverage.get("unique_query_plans", 0)
    if unique_query_plans < NIGHTLY_THRESHOLDS["min_unique_query_plans"]:
        violations.append(
            f"{filepath.name}: Insufficient unique query plans: {unique_query_plans} "
            f"(expected >= {NIGHTLY_THRESHOLDS['min_unique_query_plans']})"
        )

    # Check 7: Phase events
    phase_events = coverage.get("phase_events", {})
    for phase, min_count in NIGHTLY_THRESHOLDS["min_phase_events"].items():
        actual_count = phase_events.get(phase, 0)
        if actual_count < min_count:
            violations.append(
                f"{filepath.name}: Phase event '{phase}' too few: {actual_count} "
                f"(expected >= {min_count})"
            )

    return violations


def main():
    if len(sys.argv) < 2:
        print("Usage: validate-coverage.py <vopr-result.json> [...]", file=sys.stderr)
        sys.exit(1)

    all_violations = []

    for filepath_str in sys.argv[1:]:
        filepath = Path(filepath_str)
        if not filepath.exists():
            print(f"Warning: {filepath} does not exist, skipping", file=sys.stderr)
            continue

        try:
            result = load_vopr_result(filepath)
            violations = validate_result(result, filepath)
            all_violations.extend(violations)
        except Exception as e:
            print(f"Error processing {filepath}: {e}", file=sys.stderr)
            all_violations.append(f"{filepath.name}: Failed to parse: {e}")

    if all_violations:
        print("❌ Coverage validation FAILED\n")
        for violation in all_violations:
            print(f"  - {violation}")
        print(f"\nTotal violations: {len(all_violations)}")
        sys.exit(1)
    else:
        print("✅ Coverage validation PASSED")
        sys.exit(0)


if __name__ == "__main__":
    main()
