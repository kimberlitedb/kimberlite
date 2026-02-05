#!/bin/bash
# VOPR Overnight Testing Script
#
# This script runs VOPR with many iterations to find bugs overnight.
# It saves checkpoints, logs, and failure reports automatically.

set -euo pipefail

# ============================================================================
# Configuration
# ============================================================================

# Test scenario (baseline, swizzle, gray, multi-tenant, time-compression, combined)
SCENARIO="${VOPR_SCENARIO:-combined}"

# Number of iterations
# Performance estimates (M1 MacBook, release build):
#   Baseline:  ~167k sims/sec → 10M takes ~60s
#   Combined:  ~85k sims/sec  → 10M takes ~118s (recommended for overnight)
# For overnight (~8 hours): 85k * 28800 = 2.4 billion iterations
ITERATIONS="${VOPR_ITERATIONS:-10000000}"

# Starting seed (use $(date +%s) for random, or fixed value for reproducibility)
SEED="${VOPR_SEED:-$(date +%s)}"

# Output directory for logs and failure reports
OUTPUT_DIR="${VOPR_OUTPUT_DIR:-./vopr-results/$(date +%Y%m%d-%H%M%S)}"

# Checkpoint file for resume support
CHECKPOINT_FILE="${OUTPUT_DIR}/checkpoint.json"

# Log files
LOG_FILE="${OUTPUT_DIR}/vopr.log"
FAILURE_LOG="${OUTPUT_DIR}/failures.log"
SUMMARY_FILE="${OUTPUT_DIR}/summary.txt"

# VOPR binary path
VOPR_BIN="${VOPR_BIN:-./target/release/vopr}"

# Additional VOPR options (verbose disabled by default for performance)
# Set VOPR_OPTS="--verbose" to enable detailed logging
VOPR_OPTS="${VOPR_OPTS:-}"

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

echo "Invariant Flags: ${INVARIANT_FLAGS}"

# ============================================================================
# Setup
# ============================================================================

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║         VOPR Overnight Testing - $(date +'%Y-%m-%d %H:%M:%S')         ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo

# Create output directory
mkdir -p "${OUTPUT_DIR}"

# Save configuration
cat > "${OUTPUT_DIR}/config.txt" << EOF
VOPR Overnight Test Configuration
==================================
Start Time:    $(date)
Scenario:      ${SCENARIO}
Iterations:    ${ITERATIONS}
Starting Seed: ${SEED}
Output Dir:    ${OUTPUT_DIR}
VOPR Binary:   ${VOPR_BIN}
VOPR Options:  ${VOPR_OPTS}

Host Information:
-----------------
Hostname:      $(hostname)
OS:            $(uname -s) $(uname -r)
CPU:           $(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "Unknown")
Memory:        $(sysctl -n hw.memsize 2>/dev/null | awk '{print $1/1024/1024/1024 " GB"}' || echo "Unknown")
EOF

# Check if VOPR binary exists
if [[ ! -f "${VOPR_BIN}" ]]; then
    echo "ERROR: VOPR binary not found at ${VOPR_BIN}"
    echo "Build it with: just build-release"
    exit 1
fi

echo "Configuration saved to: ${OUTPUT_DIR}/config.txt"
echo "Logs will be written to: ${LOG_FILE}"
echo "Failures will be logged to: ${FAILURE_LOG}"
echo

# ============================================================================
# Trap Handlers for Clean Shutdown
# ============================================================================

cleanup() {
    local exit_code=$?
    echo
    echo "╔════════════════════════════════════════════════════════════════╗"
    echo "║                    Test Interrupted/Completed                   ║"
    echo "╚════════════════════════════════════════════════════════════════╝"
    echo
    echo "Test ended at: $(date)"
    echo "Exit code: ${exit_code}"
    echo
    generate_summary
    exit "${exit_code}"
}

trap cleanup EXIT INT TERM

# ============================================================================
# Main Execution
# ============================================================================

echo "Starting VOPR test..."
echo "Progress will be logged to: ${LOG_FILE}"
echo "Press Ctrl+C to interrupt (checkpoint will be saved)"
echo
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                        Test Running...                          ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo

# Run VOPR with checkpointing
# The --checkpoint-file allows resuming if interrupted
"${VOPR_BIN}" \
    --scenario "${SCENARIO}" \
    --seed "${SEED}" \
    --iterations "${ITERATIONS}" \
    --checkpoint-file "${CHECKPOINT_FILE}" \
    ${INVARIANT_FLAGS} \
    ${VOPR_OPTS} \
    2>&1 | tee "${LOG_FILE}"

# ============================================================================
# Summary Generation
# ============================================================================

generate_summary() {
    echo
    echo "Generating summary..."

    # Extract success and failure counts from VOPR results
    local success_count
    success_count=$(grep "Successes:" "${LOG_FILE}" 2>/dev/null | tail -1 | awk '{print $2}' || echo "0")
    success_count=${success_count:-0}

    local failure_count
    failure_count=$(grep "Failures:" "${LOG_FILE}" 2>/dev/null | tail -1 | awk '{print $2}' || echo "0")
    failure_count=${failure_count:-0}

    # Extract completed iterations from checkpoint
    local completed_iterations="Unknown"
    if [[ -f "${CHECKPOINT_FILE}" ]]; then
        completed_iterations=$(jq -r '.total_iterations // "Unknown"' "${CHECKPOINT_FILE}" 2>/dev/null || echo "Unknown")
    fi

    # Generate summary
    cat > "${SUMMARY_FILE}" << EOF
╔════════════════════════════════════════════════════════════════╗
║                    VOPR Test Summary                            ║
╚════════════════════════════════════════════════════════════════╝

Test Configuration:
-------------------
Scenario:           ${SCENARIO}
Start Time:         $(head -n 1 "${OUTPUT_DIR}/config.txt" | grep -o '[0-9][0-9][0-9][0-9]-.*' || echo "Unknown")
End Time:           $(date)
Total Iterations:   ${ITERATIONS}
Completed:          ${completed_iterations}
Starting Seed:      ${SEED}

Results:
--------
Successes:          ${success_count}
Failures:           ${failure_count}
Success Rate:       $(awk "BEGIN {if (${success_count}+${failure_count} > 0) print (${success_count}*100.0)/(${success_count}+${failure_count}); else print 0}")%

Output Files:
-------------
Full Log:           ${LOG_FILE}
Failures:           ${FAILURE_LOG}
Checkpoint:         ${CHECKPOINT_FILE}
Summary:            ${SUMMARY_FILE}

EOF

    # Add failure details if any
    if [[ ${failure_count} -gt 0 ]]; then
        echo "Failure Details:" >> "${SUMMARY_FILE}"
        echo "----------------" >> "${SUMMARY_FILE}"
        grep "FAIL" "${LOG_FILE}" | head -20 >> "${SUMMARY_FILE}" 2>/dev/null || true

        if [[ ${failure_count} -gt 20 ]]; then
            echo "... (${failure_count} total failures, showing first 20)" >> "${SUMMARY_FILE}"
        fi

        # Extract unique failure types
        echo >> "${SUMMARY_FILE}"
        echo "Unique Failure Types:" >> "${SUMMARY_FILE}"
        grep -o "invariant: [^,]*" "${LOG_FILE}" 2>/dev/null | sort | uniq -c | sort -rn >> "${SUMMARY_FILE}" || true
    fi

    # Display summary
    cat "${SUMMARY_FILE}"
    echo

    # Notification
    if [[ ${failure_count} -gt 0 ]]; then
        echo "⚠️  FAILURES DETECTED! See ${FAILURE_LOG} for details."

        # Save failure details
        grep -A 50 "SIMULATION FAILURE REPORT" "${LOG_FILE}" > "${FAILURE_LOG}" 2>/dev/null || true
    else
        echo "✅ All tests passed!"
    fi

    echo
    echo "Results saved to: ${OUTPUT_DIR}"
}

# ============================================================================
# End
# ============================================================================
