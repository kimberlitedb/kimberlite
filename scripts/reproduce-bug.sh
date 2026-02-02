#!/usr/bin/env bash
#
# Bug Reproducibility Harness
#
# Takes a VOPR seed and reproduces a Byzantine attack to verify determinism.
# Required for bug bounty submissions - must be 100/100 reproducible.
#
# Usage:
#   ./reproduce-bug.sh view_change_merge 42 100   # Reproduce seed 42, 100 times
#   ./reproduce-bug.sh commit_desync 1337          # Reproduce seed 1337, 10 times (default)
#

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RESULTS_DIR="$PROJECT_ROOT/results/reproductions"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Attack scenarios (bash 3.2 compatible)
ATTACK_KEYS=(
    "view_change_merge"
    "commit_desync"
    "inflated_commit"
    "invalid_metadata"
    "malicious_view_change"
    "leader_race"
)

get_scenario_type() {
    case "$1" in
        view_change_merge) echo "byzantine_view_change_merge" ;;
        commit_desync) echo "byzantine_commit_desync" ;;
        inflated_commit) echo "byzantine_inflated_commit" ;;
        invalid_metadata) echo "byzantine_invalid_metadata" ;;
        malicious_view_change) echo "byzantine_malicious_view_change" ;;
        leader_race) echo "byzantine_leader_race" ;;
        *) echo "" ;;
    esac
}

set -euo pipefail

log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

print_banner() {
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  $*"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
}

# ============================================================================
# Reproducibility Testing
# ============================================================================

reproduce_seed() {
    local attack_key=$1
    local scenario_type=$2
    local seed=$3
    local num_runs=$4
    local output_dir="$RESULTS_DIR/${attack_key}_seed_${seed}"

    print_banner "Reproducibility Test: $attack_key (Seed: $seed)"

    log_info "Scenario: $scenario_type"
    log_info "Seed: $seed"
    log_info "Runs: $num_runs"
    echo ""

    mkdir -p "$output_dir"

    # Track results
    local successful_reproductions=0
    local failed_reproductions=0
    local first_hash=""
    local first_events=""
    local first_violation=""

    log_info "Running $num_runs reproductions..."
    echo ""

    for ((i=1; i<=num_runs; i++)); do
        local run_output="$output_dir/run_${i}.json"

        # Run with specific seed
        if ! cargo run --release -p kimberlite-sim --bin vopr -- --scenario "$scenario_type" -n 1 --seed "$seed" --json > "$run_output" 2>&1; then
            # Exit code 1 means violation detected (expected)
            if [ $? -ne 1 ]; then
                log_error "Run $i failed unexpectedly"
                failed_reproductions=$((failed_reproductions + 1))
                continue
            fi
        fi

        # Extract key metrics for comparison
        local current_hash=$(jq -r '.final_storage_hash // "none"' "$run_output" 2>/dev/null || echo "none")
        local current_events=$(jq -r '.total_events // 0' "$run_output" 2>/dev/null || echo "0")
        local current_violation=$(jq -r '.violations[0].invariant // "none"' "$run_output" 2>/dev/null || echo "none")

        # First run establishes baseline
        if [ -z "$first_hash" ]; then
            first_hash="$current_hash"
            first_events="$current_events"
            first_violation="$current_violation"
            log_success "Run $i: Baseline established"
        else
            # Compare with baseline
            if [ "$current_hash" = "$first_hash" ] && \
               [ "$current_events" = "$first_events" ] && \
               [ "$current_violation" = "$first_violation" ]; then
                successful_reproductions=$((successful_reproductions + 1))
                printf "${GREEN}✓${NC} Run %3d: Match\n" "$i"
            else
                failed_reproductions=$((failed_reproductions + 1))
                printf "${RED}✗${NC} Run %3d: Mismatch (hash: %s, events: %s, violation: %s)\n" \
                    "$i" "$current_hash" "$current_events" "$current_violation"
            fi
        fi
    done

    echo ""
    print_banner "Reproducibility Results"

    # Calculate percentage (subtract 1 from total for baseline)
    local total_comparisons=$((num_runs - 1))
    local success_rate=0
    if [ $total_comparisons -gt 0 ]; then
        success_rate=$((successful_reproductions * 100 / total_comparisons))
    fi

    echo "Seed: $seed"
    echo "Total runs: $num_runs"
    echo "Successful reproductions: $successful_reproductions / $total_comparisons"
    echo "Success rate: $success_rate%"
    echo ""

    if [ "$first_violation" != "none" ]; then
        log_info "Violation detected: $first_violation"
    else
        log_error "No violation detected - seed may be invalid"
    fi

    echo ""
    echo "Baseline metrics:"
    echo "  - Storage hash: $first_hash"
    echo "  - Total events: $first_events"
    echo "  - Violation: $first_violation"
    echo ""

    if [ $success_rate -eq 100 ]; then
        log_success "✓ 100% reproducible - ready for bounty submission!"
        echo ""
        log_info "Next steps:"
        echo "  1. Run './generate-bounty-submission.sh $attack_key $seed'"
        echo "  2. Review generated submission package"
        echo "  3. Submit to security@kimberlite.dev"
        return 0
    elif [ $success_rate -ge 95 ]; then
        log_success "✓ Highly reproducible ($success_rate%)"
        echo ""
        log_info "Consider running more iterations to verify 100% reproducibility"
        return 0
    else
        log_error "✗ Not consistently reproducible ($success_rate%)"
        echo ""
        log_error "This seed may not be suitable for bounty submission"
        return 1
    fi
}

# ============================================================================
# Main
# ============================================================================

main() {
    cd "$PROJECT_ROOT"

    if [ $# -lt 2 ]; then
        log_error "Missing arguments"
        echo ""
        echo "Usage:"
        echo "  $0 <attack_key> <seed> [num_runs]"
        echo ""
        echo "Examples:"
        echo "  $0 view_change_merge 42 100   # Reproduce seed 42, 100 times"
        echo "  $0 commit_desync 1337          # Reproduce seed 1337, 10 times"
        echo ""
        echo "Available attacks:"
        for key in "${ATTACK_KEYS[@]}"; do
            echo "  - $key"
        done
        echo ""
        exit 1
    fi

    local attack_key=$1
    local seed=$2
    local num_runs=${3:-10}
    local scenario_type=$(get_scenario_type "$attack_key")

    if [ -z "$scenario_type" ]; then
        log_error "Unknown attack: $attack_key"
        echo ""
        echo "Available attacks:"
        for key in "${ATTACK_KEYS[@]}"; do
            echo "  - $key"
        done
        exit 1
    fi

    reproduce_seed "$attack_key" "$scenario_type" "$seed" "$num_runs"
}

main "$@"
