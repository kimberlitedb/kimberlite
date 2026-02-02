#!/usr/bin/env bash
#
# Byzantine Attack Orchestration Script
#
# Runs all 6 Byzantine attack scenarios targeting VSR consensus vulnerabilities.
# Each scenario targets a specific bug with high bounty potential ($3k-$20k).
#
# Usage:
#   ./byzantine-attack.sh all 1000           # Run all attacks with 1000 iterations each
#   ./byzantine-attack.sh view_change 5000   # Run specific attack with 5000 iterations
#   ./byzantine-attack.sh list               # List available attacks
#

# Script directory (set these before set -u)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RESULTS_DIR="$PROJECT_ROOT/results/byzantine"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Attack scenarios (bash 3.2 compatible)
ATTACK_KEYS=(
    "view_change_merge"
    "commit_desync"
    "inflated_commit"
    "invalid_metadata"
    "malicious_view_change"
    "leader_race"
)

# Helper functions to get attack metadata
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

get_bounty_value() {
    case "$1" in
        view_change_merge) echo "\$20,000" ;;
        commit_desync) echo "\$18,000" ;;
        inflated_commit) echo "\$10,000" ;;
        invalid_metadata) echo "\$3,000" ;;
        malicious_view_change) echo "\$10,000" ;;
        leader_race) echo "\$5,000" ;;
        *) echo "\$0" ;;
    esac
}

get_expected_violation() {
    case "$1" in
        view_change_merge) echo "vsr_agreement" ;;
        commit_desync) echo "vsr_prefix_property" ;;
        inflated_commit) echo "vsr_durability" ;;
        invalid_metadata) echo "vsr_agreement" ;;
        malicious_view_change) echo "vsr_view_change_safety" ;;
        leader_race) echo "vsr_agreement" ;;
        *) echo "" ;;
    esac
}

# Enable strict mode
set -euo pipefail

# ============================================================================
# Helper Functions
# ============================================================================

log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $*"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $*"
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
# Attack Functions
# ============================================================================

run_attack() {
    local attack_key=$1
    local scenario_type=$2
    local iterations=$3
    local output_file="$RESULTS_DIR/${attack_key}_$(date +%Y%m%d_%H%M%S).json"
    local bounty=$(get_bounty_value "$attack_key")
    local violation=$(get_expected_violation "$attack_key")

    print_banner "Attack: $attack_key | Bounty: $bounty | Target: $violation"

    log_info "Scenario: $scenario_type"
    log_info "Iterations: $iterations"
    log_info "Output: $output_file"
    echo ""

    # Create results directory
    mkdir -p "$RESULTS_DIR"

    # Run VOPR with the scenario
    log_info "Launching Byzantine attack..."

    if cargo run --release -p kimberlite-sim --bin vopr -- --scenario "$scenario_type" -n "$iterations" --json > "$output_file" 2>&1; then
        log_success "Attack completed successfully"
    else
        local exit_code=$?
        if [ $exit_code -eq 1 ]; then
            log_warning "Attack completed with violations detected (exit code 1)"
        else
            log_error "Attack failed with exit code $exit_code"
            return $exit_code
        fi
    fi

    # Analyze results
    if [ -f "$output_file" ]; then
        local violations=$(jq -r '.violations // [] | length' "$output_file" 2>/dev/null || echo "0")
        local seeds=$(jq -r '.violations // [] | .[].seed' "$output_file" 2>/dev/null || echo "")

        if [ "$violations" -gt 0 ]; then
            log_success "Found $violations violation(s)!"
            echo ""
            echo "Violation seeds:"
            echo "$seeds" | while read -r seed; do
                echo "  - $seed"
            done
            echo ""
            log_info "Run './reproduce-bug.sh $attack_key <seed>' to reproduce"
        else
            log_warning "No violations detected in $iterations iterations"
        fi

        # Print summary
        echo ""
        echo "Results saved to: $output_file"
        echo ""
    else
        log_error "Output file not found: $output_file"
        return 1
    fi
}

run_all_attacks() {
    local iterations=$1
    local total_bounty=0
    local violations_found=0

    print_banner "Byzantine Attack Campaign - All 6 Scenarios"

    log_info "Iterations per attack: $iterations"
    log_info "Total iterations: $((iterations * 6))"
    log_info "Estimated time: ~$((iterations * 6 / 100)) minutes (at 100 iter/min)"
    echo ""

    for attack_key in "${ATTACK_KEYS[@]}"; do
        local scenario_type=$(get_scenario_type "$attack_key")
        if run_attack "$attack_key" "$scenario_type" "$iterations"; then
            violations_found=$((violations_found + 1))
        fi
        echo ""
    done

    print_banner "Campaign Complete"
    log_success "Tested all 6 attack vectors"
    log_info "Attacks with violations: $violations_found / 6"
    echo ""
    log_info "Results directory: $RESULTS_DIR"
    echo ""
}

list_attacks() {
    print_banner "Available Byzantine Attacks"

    echo "Attack scenarios targeting VSR consensus bugs:"
    echo ""

    for attack_key in "${ATTACK_KEYS[@]}"; do
        local bounty=$(get_bounty_value "$attack_key")
        local violation=$(get_expected_violation "$attack_key")
        printf "  %-25s | Bounty: %-10s | Targets: %s\n" \
            "$attack_key" \
            "$bounty" \
            "$violation"
    done

    echo ""
    echo "Usage:"
    echo "  ./byzantine-attack.sh <attack_key> <iterations>"
    echo "  ./byzantine-attack.sh all <iterations>"
    echo ""
}

# ============================================================================
# Main
# ============================================================================

main() {
    cd "$PROJECT_ROOT"

    if [ $# -eq 0 ]; then
        log_error "Missing arguments"
        echo ""
        echo "Usage:"
        echo "  $0 all <iterations>           # Run all attacks"
        echo "  $0 <attack_key> <iterations>  # Run specific attack"
        echo "  $0 list                       # List available attacks"
        echo ""
        exit 1
    fi

    local command=$1

    case "$command" in
        list)
            list_attacks
            ;;
        all)
            local iterations=${2:-1000}
            run_all_attacks "$iterations"
            ;;
        *)
            local scenario_type=$(get_scenario_type "$command")
            if [ -z "$scenario_type" ]; then
                log_error "Unknown attack: $command"
                echo ""
                echo "Available attacks:"
                for key in "${ATTACK_KEYS[@]}"; do
                    echo "  - $key"
                done
                echo ""
                echo "Run '$0 list' for more details"
                exit 1
            fi

            local iterations=${2:-1000}
            run_attack "$command" "$scenario_type" "$iterations"
            ;;
    esac
}

main "$@"
