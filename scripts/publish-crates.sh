#!/usr/bin/env bash
set -euo pipefail

CRATES_TO_PUBLISH=(
    # Already published (v0.4.0):
    # - kimberlite-types
    # - kimberlite-agent-protocol
    # - kimberlite-crypto
    # - kimberlite-directory
    # - kimberlite-storage
    # - kimberlite-kernel
    # - kimberlite-wire

    # Layer 3
    "kimberlite-vsr"
    "kimberlite-store"
    "kimberlite-query"

    # Layer 4
    "kimberlite-client"

    # Layer 5
    "kimberlite-server"
    "kimberlite-config"
    "kimberlite-migration"

    # Layer 6 (facade)
    "kimberlite"

    # Layer 7 (extensions)
    "kimberlite-sharing"
    "kimberlite-mcp"
)

# Crates with unpublished dev-dependencies (kimberlite-sim) that need special handling
CRATES_WITH_SIM_DEVDEP=(
    "kimberlite-vsr"
    "kimberlite-query"
)

DRY_RUN=${DRY_RUN:-false}
PUBLISH_DELAY=${PUBLISH_DELAY:-30}

# Function to temporarily remove kimberlite-sim dev-dependency
remove_sim_devdep() {
    local crate=$1
    local cargo_toml="crates/$crate/Cargo.toml"

    # Create backup
    cp "$cargo_toml" "$cargo_toml.backup"

    # Remove the kimberlite-sim dev-dependency line
    sed -i.tmp '/^kimberlite-sim\.workspace = true$/d' "$cargo_toml"
    rm -f "$cargo_toml.tmp"

    echo "  Temporarily removed kimberlite-sim dev-dependency"
}

# Function to restore original Cargo.toml
restore_cargo_toml() {
    local crate=$1
    local cargo_toml="crates/$crate/Cargo.toml"

    if [[ -f "$cargo_toml.backup" ]]; then
        mv "$cargo_toml.backup" "$cargo_toml"
        echo "  Restored original Cargo.toml"
    fi
}

# Cleanup function to restore all backups on error
cleanup() {
    echo ""
    echo "🔄 Cleaning up backups..."
    for crate in "${CRATES_WITH_SIM_DEVDEP[@]}"; do
        restore_cargo_toml "$crate"
    done
}

# Set trap to cleanup on exit
trap cleanup EXIT

echo "🚀 Publishing ${#CRATES_TO_PUBLISH[@]} crates to crates.io"
echo "Dry run: $DRY_RUN"
echo ""

TOTAL_CRATES=${#CRATES_TO_PUBLISH[@]}
CURRENT_INDEX=0

for crate in "${CRATES_TO_PUBLISH[@]}"; do
    CURRENT_INDEX=$((CURRENT_INDEX + 1))
    echo "📦 Publishing $crate ($CURRENT_INDEX/$TOTAL_CRATES)..."

    # Check if crate has unpublished dev-dependencies
    NEEDS_CLEANUP=false
    for sim_crate in "${CRATES_WITH_SIM_DEVDEP[@]}"; do
        if [[ "$crate" == "$sim_crate" ]]; then
            remove_sim_devdep "$crate"
            NEEDS_CLEANUP=true
            break
        fi
    done

    # Add --allow-dirty if we modified the Cargo.toml
    ALLOW_DIRTY_FLAG=""
    if [[ "$NEEDS_CLEANUP" == "true" ]]; then
        ALLOW_DIRTY_FLAG="--allow-dirty"
    fi

    if [[ "$DRY_RUN" == "true" ]]; then
        cargo publish --dry-run $ALLOW_DIRTY_FLAG -p "$crate"
    else
        cargo publish $ALLOW_DIRTY_FLAG -p "$crate"

        # Wait for crates.io propagation (except for last crate)
        if [[ $CURRENT_INDEX -lt $TOTAL_CRATES ]]; then
            echo "⏳ Waiting ${PUBLISH_DELAY}s for crates.io propagation..."
            sleep "$PUBLISH_DELAY"
        fi
    fi

    # Restore Cargo.toml if we modified it
    if [[ "$NEEDS_CLEANUP" == "true" ]]; then
        restore_cargo_toml "$crate"
    fi

    echo "✅ $crate"
    echo ""
done

echo "🎉 All crates published successfully!"
