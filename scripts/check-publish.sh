#!/usr/bin/env bash
set -euo pipefail

echo "🔍 Pre-publish validation checks"

# Check workspace is clean
if [[ -n $(git status --porcelain) ]]; then
  echo "❌ Working directory is not clean"
  exit 1
fi
echo "✅ Working directory clean"

# Check version tag exists
VERSION=$(cargo metadata --format-version 1 | jq -r '.packages[] | select(.name == "kimberlite") | .version')
if ! git tag | grep -q "^v$VERSION$"; then
  echo "❌ Version tag v$VERSION does not exist"
  exit 1
fi
echo "✅ Version tag v$VERSION exists"

# Check CHANGELOG updated
if ! grep -q "## \[$VERSION\]" CHANGELOG.md; then
  echo "❌ CHANGELOG.md does not have entry for $VERSION"
  exit 1
fi
echo "✅ CHANGELOG.md updated"

# Dry-run publish
echo ""
echo "🧪 Running dry-run publish..."
DRY_RUN=true ./scripts/publish-crates.sh

echo ""
echo "✅ All validation checks passed!"
