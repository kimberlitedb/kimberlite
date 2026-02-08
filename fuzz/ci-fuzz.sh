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
    "fuzz_sql_parser"
    "fuzz_storage_record"
    "fuzz_kernel_command"
    "fuzz_rbac_rewrite"
)

# Run each target for 50,000 iterations (quick smoke test, still <2min per target on CI)
for target in "${targets[@]}"; do
    echo ""
    echo "=========================================="
    echo "Fuzzing: $target"
    echo "=========================================="

    cargo fuzz run "$target" -- -runs=50000 || {
        echo "ERROR: Fuzzing failed for $target"
        exit 1
    }

    echo "✓ $target passed"
done

echo ""
echo "=========================================="
echo "All fuzz targets passed!"
echo "=========================================="
