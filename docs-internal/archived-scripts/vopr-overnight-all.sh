#!/bin/bash
# VOPR Comprehensive Overnight Testing Script
#
# Runs all 27 VOPR scenarios sequentially with configurable iterations per scenario.
# Results are saved to separate directories for each scenario.

set -euo pipefail

# ============================================================================
# Configuration
# ============================================================================

# Iterations per scenario (adjust based on available time)
# For 8 hours with 27 scenarios: ~1.3M iterations/scenario @ ~85k sims/sec
ITERATIONS_PER_SCENARIO="${VOPR_ITERATIONS:-1000000}"

# Starting seed (use $(date +%s) for random, or fixed value for reproducibility)
BASE_SEED="${VOPR_SEED:-$(date +%s)}"

# Output directory for all results
OUTPUT_BASE_DIR="${VOPR_OUTPUT_DIR:-./vopr-results/comprehensive-$(date +%Y%m%d-%H%M%S)}"

# VOPR binary path
VOPR_BIN="${VOPR_BIN:-./target/release/vopr}"

# Additional VOPR options
VOPR_OPTS="${VOPR_OPTS:-}"

# ============================================================================
# Scenario List (All 27 Scenarios)
# ============================================================================

# Core scenarios
SCENARIOS=(
    "baseline"
    "swizzle"
    "gray"
    "multi-tenant"
    "time-compression"
    "combined"
)

# Byzantine attack scenarios
BYZANTINE_SCENARIOS=(
    "view-change-merge"
    "commit-desync"
    "inflated-commit"
    "invalid-metadata"
    "malicious-view-change"
    "leader-race"
    "dvc-tail-mismatch"
    "dvc-identical-claims"
    "oversized-start-view"
    "invalid-repair-range"
    "invalid-kernel-command"
)

# Corruption detection scenarios
CORRUPTION_SCENARIOS=(
    "bit-flip"
    "checksum-validation"
    "silent-disk-failure"
)

# Crash and recovery scenarios
CRASH_RECOVERY_SCENARIOS=(
    "crash-commit"
    "crash-view-change"
    "recovery-corrupt"
)

# Gray failure variants
GRAY_VARIANTS=(
    "slow-disk"
    "intermittent-network"
)

# Race condition scenarios
RACE_SCENARIOS=(
    "race-view-changes"
    "race-commit-dvc"
)

# Combine all scenarios
ALL_SCENARIOS=(
    "${SCENARIOS[@]}"
    "${BYZANTINE_SCENARIOS[@]}"
    "${CORRUPTION_SCENARIOS[@]}"
    "${CRASH_RECOVERY_SCENARIOS[@]}"
    "${GRAY_VARIANTS[@]}"
    "${RACE_SCENARIOS[@]}"
)

# ============================================================================
# Invariant Configuration
# ============================================================================

INVARIANT_FLAGS=""

# Core-only mode
if [[ "${VOPR_CORE_ONLY:-0}" == "1" ]]; then
    INVARIANT_FLAGS="--core-invariants-only"

# Specific invariant list
elif [[ -n "${VOPR_INVARIANTS:-}" ]]; then
    IFS=',' read -ra INVS <<< "$VOPR_INVARIANTS"
    for inv in "${INVS[@]}"; do
        INVARIANT_FLAGS="${INVARIANT_FLAGS} --enable-invariant ${inv}"
    done

# Group enablement (defaults to all enabled except SQL oracles)
else
    [[ "${VOPR_ENABLE_VSR_INVARIANTS:-1}" == "1" ]] || INVARIANT_FLAGS="${INVARIANT_FLAGS} --disable-vsr-invariants"
    [[ "${VOPR_ENABLE_PROJECTION_INVARIANTS:-1}" == "1" ]] || INVARIANT_FLAGS="${INVARIANT_FLAGS} --disable-projection-invariants"
    [[ "${VOPR_ENABLE_QUERY_INVARIANTS:-1}" == "1" ]] || INVARIANT_FLAGS="${INVARIANT_FLAGS} --disable-query-invariants"
    [[ "${VOPR_ENABLE_SQL_ORACLES:-0}" == "1" ]] && INVARIANT_FLAGS="${INVARIANT_FLAGS} --enable-sql-oracles"
fi

# ============================================================================
# Setup
# ============================================================================

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║   VOPR Comprehensive Testing - $(date +'%Y-%m-%d %H:%M:%S')    ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo
echo "Running ${#ALL_SCENARIOS[@]} scenarios with ${ITERATIONS_PER_SCENARIO} iterations each"
echo "Total expected iterations: $((${#ALL_SCENARIOS[@]} * ITERATIONS_PER_SCENARIO))"
echo

# Create output directory
mkdir -p "${OUTPUT_BASE_DIR}"

# Check if VOPR binary exists
if [[ ! -f "${VOPR_BIN}" ]]; then
    echo "ERROR: VOPR binary not found at ${VOPR_BIN}"
    echo "Build it with: just build-release"
    exit 1
fi

# Save master configuration
cat > "${OUTPUT_BASE_DIR}/config.txt" << EOF
VOPR Comprehensive Test Configuration
======================================
Start Time:              $(date)
Total Scenarios:         ${#ALL_SCENARIOS[@]}
Iterations Per Scenario: ${ITERATIONS_PER_SCENARIO}
Base Seed:               ${BASE_SEED}
Output Directory:        ${OUTPUT_BASE_DIR}
VOPR Binary:             ${VOPR_BIN}
VOPR Options:            ${VOPR_OPTS}
Invariant Flags:         ${INVARIANT_FLAGS}

Host Information:
-----------------
Hostname:      $(hostname)
OS:            $(uname -s) $(uname -r)
CPU:           $(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "Unknown")
Memory:        $(sysctl -n hw.memsize 2>/dev/null | awk '{print $1/1024/1024/1024 " GB"}' || echo "Unknown")

Scenarios to Run:
-----------------
EOF

for scenario in "${ALL_SCENARIOS[@]}"; do
    echo "  - ${scenario}" >> "${OUTPUT_BASE_DIR}/config.txt"
done

echo >> "${OUTPUT_BASE_DIR}/config.txt"

# ============================================================================
# Results Tracking
# ============================================================================

TOTAL_SUCCESSES=0
TOTAL_FAILURES=0
FAILED_SCENARIOS=()
SCENARIO_RESULTS_FILE=$(mktemp)

# ============================================================================
# Main Execution Loop
# ============================================================================

SCENARIO_COUNT=0
START_TIME=$(date +%s)

for scenario in "${ALL_SCENARIOS[@]}"; do
    SCENARIO_COUNT=$((SCENARIO_COUNT + 1))

    echo
    echo "╔════════════════════════════════════════════════════════════════╗"
    printf "║  [%2d/%2d] Running: %-40s ║\n" "${SCENARIO_COUNT}" "${#ALL_SCENARIOS[@]}" "${scenario}"
    echo "╚════════════════════════════════════════════════════════════════╝"
    echo

    # Create scenario-specific output directory
    SCENARIO_DIR="${OUTPUT_BASE_DIR}/${scenario}"
    mkdir -p "${SCENARIO_DIR}"

    # Generate seed for this scenario (deterministic based on base seed)
    SCENARIO_SEED=$((BASE_SEED + SCENARIO_COUNT))

    # Run VOPR for this scenario
    SCENARIO_LOG="${SCENARIO_DIR}/vopr.log"
    CHECKPOINT_FILE="${SCENARIO_DIR}/checkpoint.json"

    echo "Scenario:    ${scenario}"
    echo "Seed:        ${SCENARIO_SEED}"
    echo "Iterations:  ${ITERATIONS_PER_SCENARIO}"
    echo "Output:      ${SCENARIO_DIR}"
    echo

    # Run the test (capture exit code)
    set +e
    "${VOPR_BIN}" \
        --scenario "${scenario}" \
        --seed "${SCENARIO_SEED}" \
        --iterations "${ITERATIONS_PER_SCENARIO}" \
        --checkpoint-file "${CHECKPOINT_FILE}" \
        ${INVARIANT_FLAGS} \
        ${VOPR_OPTS} \
        2>&1 | tee "${SCENARIO_LOG}"

    EXIT_CODE=$?
    set -e

    # Parse results
    SUCCESS_COUNT=$(grep -o "Successes: [0-9]*" "${SCENARIO_LOG}" 2>/dev/null | tail -1 | awk '{print $2}' || echo "0")
    FAILURE_COUNT=$(grep -o "Failures: [0-9]*" "${SCENARIO_LOG}" 2>/dev/null | tail -1 | awk '{print $2}' || echo "0")

    TOTAL_SUCCESSES=$((TOTAL_SUCCESSES + SUCCESS_COUNT))
    TOTAL_FAILURES=$((TOTAL_FAILURES + FAILURE_COUNT))

    # Save scenario result
    if [[ ${FAILURE_COUNT} -gt 0 ]] || [[ ${EXIT_CODE} -ne 0 ]]; then
        FAILED_SCENARIOS+=("${scenario}")
        echo "${scenario}|FAILED (${FAILURE_COUNT} failures, exit code: ${EXIT_CODE})" >> "${SCENARIO_RESULTS_FILE}"
        echo "❌ SCENARIO FAILED: ${scenario} (${FAILURE_COUNT} failures)"
    else
        echo "${scenario}|PASSED (${SUCCESS_COUNT} iterations)" >> "${SCENARIO_RESULTS_FILE}"
        echo "✅ SCENARIO PASSED: ${scenario}"
    fi

    echo
    echo "Progress: ${SCENARIO_COUNT}/${#ALL_SCENARIOS[@]} scenarios completed"

    # Estimate remaining time
    ELAPSED=$(($(date +%s) - START_TIME))
    if [[ ${SCENARIO_COUNT} -gt 0 ]]; then
        AVG_TIME_PER_SCENARIO=$((ELAPSED / SCENARIO_COUNT))
        REMAINING_SCENARIOS=$((${#ALL_SCENARIOS[@]} - SCENARIO_COUNT))
        ESTIMATED_REMAINING=$((AVG_TIME_PER_SCENARIO * REMAINING_SCENARIOS))

        echo "Elapsed time: $((ELAPSED / 60))m $((ELAPSED % 60))s"
        echo "Estimated remaining: $((ESTIMATED_REMAINING / 60))m $((ESTIMATED_REMAINING % 60))s"
    fi
done

END_TIME=$(date +%s)
TOTAL_ELAPSED=$((END_TIME - START_TIME))

# ============================================================================
# Final Summary
# ============================================================================

echo
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║              Comprehensive Test Suite Complete                 ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo

# Generate summary report
SUMMARY_FILE="${OUTPUT_BASE_DIR}/SUMMARY.txt"

cat > "${SUMMARY_FILE}" << EOF
╔════════════════════════════════════════════════════════════════╗
║            VOPR Comprehensive Test Suite Summary               ║
╚════════════════════════════════════════════════════════════════╝

Test Configuration:
-------------------
Start Time:              $(date -r ${START_TIME})
End Time:                $(date -r ${END_TIME})
Total Duration:          $((TOTAL_ELAPSED / 3600))h $(((TOTAL_ELAPSED % 3600) / 60))m $((TOTAL_ELAPSED % 60))s
Scenarios Run:           ${#ALL_SCENARIOS[@]}
Iterations Per Scenario: ${ITERATIONS_PER_SCENARIO}
Total Iterations:        $((${#ALL_SCENARIOS[@]} * ITERATIONS_PER_SCENARIO))

Overall Results:
----------------
Total Successes:         ${TOTAL_SUCCESSES}
Total Failures:          ${TOTAL_FAILURES}
Success Rate:            $(awk "BEGIN {if (${TOTAL_SUCCESSES}+${TOTAL_FAILURES} > 0) print (${TOTAL_SUCCESSES}*100.0)/(${TOTAL_SUCCESSES}+${TOTAL_FAILURES}); else print 0}")%
Failed Scenarios:        ${#FAILED_SCENARIOS[@]}
Passed Scenarios:        $((${#ALL_SCENARIOS[@]} - ${#FAILED_SCENARIOS[@]}))

Scenario Results:
-----------------
EOF

# Print each scenario result
while IFS='|' read -r scenario result; do
    printf "%-30s %s\n" "${scenario}:" "${result}" >> "${SUMMARY_FILE}"
done < "${SCENARIO_RESULTS_FILE}"

if [[ ${#FAILED_SCENARIOS[@]} -gt 0 ]]; then
    cat >> "${SUMMARY_FILE}" << EOF

Failed Scenarios Details:
--------------------------
EOF
    for failed in "${FAILED_SCENARIOS[@]}"; do
        echo "  - ${failed}" >> "${SUMMARY_FILE}"
        echo "    Log: ${OUTPUT_BASE_DIR}/${failed}/vopr.log" >> "${SUMMARY_FILE}"
    done
fi

cat >> "${SUMMARY_FILE}" << EOF

Performance Metrics:
--------------------
Average Time Per Scenario:     $((TOTAL_ELAPSED / ${#ALL_SCENARIOS[@]}))s
Simulations Per Second:        $(awk "BEGIN {print int((${TOTAL_SUCCESSES}+${TOTAL_FAILURES})/${TOTAL_ELAPSED})}")

Output Directory:
-----------------
${OUTPUT_BASE_DIR}

EOF

# Display summary
cat "${SUMMARY_FILE}"

# Final status
if [[ ${#FAILED_SCENARIOS[@]} -eq 0 ]]; then
    echo "✅ All ${#ALL_SCENARIOS[@]} scenarios passed!"
    rm -f "${SCENARIO_RESULTS_FILE}"
    exit 0
else
    echo "⚠️  ${#FAILED_SCENARIOS[@]} of ${#ALL_SCENARIOS[@]} scenarios failed"
    echo "   See ${SUMMARY_FILE} for details"
    rm -f "${SCENARIO_RESULTS_FILE}"
    exit 1
fi
