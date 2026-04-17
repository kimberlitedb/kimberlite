#!/usr/bin/env bash
# CI script for fuzzing smoke tests.
# Runs each fuzz target for a limited iteration count to catch regressions
# introduced into deserialization, parsing, crypto, kernel state machine,
# and RBAC/ABAC policy enforcement.

set -euo pipefail

echo "Running fuzz smoke tests..."

cd "$(dirname "$0")"

# Tier 1 targets: panic + structural / round-trip / model invariants.
# Every listed target has a real oracle beyond "no panic" — see the header
# comment in each harness file for the specific invariants checked.
targets=(
    "fuzz_wire_deserialize"
    "fuzz_wire_vsr"
    "fuzz_crypto_encrypt"
    "fuzz_sql_parser"
    "fuzz_storage_record"
    "fuzz_storage_decompress"
    "fuzz_superblock"
    "fuzz_kernel_command"
    "fuzz_rbac_rewrite"
    "fuzz_rbac_bypass"
    "fuzz_abac_evaluator"
    "fuzz_auth_token"
)

# Some targets ship a curated seed corpus alongside the libFuzzer-grown one.
# cargo-fuzz picks up extra seed directories positionally after the corpus dir.
# Format: "target_name:extra_corpus_dir" (relative to this directory).
extra_seeds=(
    "fuzz_sql_parser:corpus/fuzz_sql_parser_adversarial"
)

lookup_extra_seed() {
    local target="$1"
    for entry in "${extra_seeds[@]}"; do
        if [[ "${entry%%:*}" == "$target" ]]; then
            echo "${entry#*:}"
            return 0
        fi
    done
    return 1
}

for target in "${targets[@]}"; do
    echo ""
    echo "=========================================="
    echo "Fuzzing: $target"
    echo "=========================================="

    # Default corpus directory: corpus/<target>/ (created on first run).
    corpus_dir="corpus/${target}"
    mkdir -p "${corpus_dir}"

    # Optional hand-curated adversarial seed directory.
    seed_args=()
    if extra=$(lookup_extra_seed "$target"); then
        if [[ -d "$extra" ]]; then
            seed_args+=("$extra")
            echo "  (additional seed directory: $extra)"
        fi
    fi

    cargo fuzz run "$target" "${corpus_dir}" "${seed_args[@]}" -- -runs=50000 || {
        echo "ERROR: Fuzzing failed for $target"
        exit 1
    }

    echo "✓ $target passed"
done

echo ""
echo "=========================================="
echo "All fuzz targets passed!"
echo "=========================================="
