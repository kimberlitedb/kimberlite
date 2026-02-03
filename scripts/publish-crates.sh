#!/usr/bin/env bash
set -euo pipefail

CRATES_TO_PUBLISH=(
  # Layer 0
  "kimberlite-types"
  "kimberlite-agent-protocol"

  # Layer 1
  "kimberlite-crypto"
  "kimberlite-directory"

  # Layer 2
  "kimberlite-storage"
  "kimberlite-kernel"
  "kimberlite-wire"

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

DRY_RUN=${DRY_RUN:-false}
PUBLISH_DELAY=${PUBLISH_DELAY:-30}

echo "🚀 Publishing ${#CRATES_TO_PUBLISH[@]} crates to crates.io"
echo "Dry run: $DRY_RUN"
echo ""

for crate in "${CRATES_TO_PUBLISH[@]}"; do
  echo "📦 Publishing $crate..."

  if [[ "$DRY_RUN" == "true" ]]; then
    cargo publish --dry-run -p "$crate"
  else
    cargo publish -p "$crate"

    if [[ "$crate" != "${CRATES_TO_PUBLISH[-1]}" ]]; then
      echo "⏳ Waiting ${PUBLISH_DELAY}s for crates.io propagation..."
      sleep "$PUBLISH_DELAY"
    fi
  fi

  echo "✅ $crate"
  echo ""
done

echo "🎉 All crates published successfully!"
