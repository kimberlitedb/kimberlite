#!/usr/bin/env bash
# CI script for fuzzing smoke tests
# Runs each fuzz target for a limited number of iterations to catch regressions

set -euo pipefail

echo "Running fuzz smoke tests..."

cd "$(dirname "$0")"

# List of fuzz targets
targets=(
    "fuzz_wire_deserialize"
    "fuzz_crypto_encrypt"
)

# Run each target for 10,000 iterations (quick smoke test)
for target in "${targets[@]}"; do
    echo ""
    echo "=========================================="
    echo "Fuzzing: $target"
    echo "=========================================="

    cargo fuzz run "$target" -- -runs=10000 || {
        echo "ERROR: Fuzzing failed for $target"
        exit 1
    }

    echo "✓ $target passed"
done

echo ""
echo "=========================================="
echo "All fuzz targets passed!"
echo "=========================================="
