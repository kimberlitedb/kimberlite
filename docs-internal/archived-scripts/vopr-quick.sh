#!/bin/bash
# Quick VOPR test runner - Simple wrapper for common use cases

set -euo pipefail

# Build if needed
if [[ ! -f ./target/release/vopr ]]; then
    echo "Building VOPR (release mode)..."
    just build-release
fi

# Parse command line
ITERATIONS="${1:-1000}"
SEED="${2:-$(date +%s)}"

echo "╔════════════════════════════════════════════════════╗"
echo "║              VOPR Quick Test                        ║"
echo "╚════════════════════════════════════════════════════╝"
echo
echo "Iterations: ${ITERATIONS}"
echo "Seed:       ${SEED}"
echo

# Run VOPR
./target/release/vopr \
    --seed "${SEED}" \
    --iterations "${ITERATIONS}" \
    --verbose

echo
echo "Test complete!"
