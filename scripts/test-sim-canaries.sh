#!/bin/bash
# Test script for simulator-level canaries
#
# This script verifies that all sim canaries are properly integrated and detected.
# It runs each canary feature and checks that the integration tests pass.
#
# Expected: All canaries should be detectable through their integration tests.

set -e  # Exit on error

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

echo "========================================"
echo "Testing Simulator-Level Canaries"
echo "========================================"
echo ""

# ANSI color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

CANARIES=(
    "sim-canary-partition-leak"
    "sim-canary-time-leak"
    "sim-canary-drop-disabled"
    "sim-canary-fsync-lies"
    "sim-canary-rng-unseeded"
)

PASSED=0
FAILED=0

for canary in "${CANARIES[@]}"; do
    echo "----------------------------------------"
    echo "Testing: $canary"
    echo "----------------------------------------"

    # Run integration test with the canary feature enabled
    # These tests verify that the canary changes behavior as expected
    if cargo test -p kimberlite-sim --test sim_canary_integration \
        --features "$canary" -- --nocapture; then
        echo -e "${GREEN}✓ $canary integration test PASSED${NC}"
        ((PASSED++))
    else
        echo -e "${RED}✗ $canary integration test FAILED${NC}"
        ((FAILED++))
    fi

    echo ""
done

echo "========================================"
echo "Summary"
echo "========================================"
echo -e "Passed: ${GREEN}$PASSED${NC}"
echo -e "Failed: ${RED}$FAILED${NC}"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}All simulator canaries are working correctly!${NC}"
    exit 0
else
    echo -e "${RED}Some simulator canaries failed. See output above.${NC}"
    exit 1
fi
