#!/usr/bin/env bash
set -euo pipefail

NEW_VERSION=$1
if [[ -z "$NEW_VERSION" ]]; then
  echo "Usage: $0 <new-version>"
  echo "Example: $0 0.5.0"
  exit 1
fi

echo "📝 Updating version to $NEW_VERSION"

# Update workspace version
sed -i.bak "s/^version = \".*\"/version = \"$NEW_VERSION\"/" Cargo.toml

# Update lock file
cargo check --workspace

# Add CHANGELOG entry
DATE=$(date +%Y-%m-%d)
TEMP=$(mktemp)
{
  head -n 4 CHANGELOG.md
  echo ""
  echo "## [$NEW_VERSION] - $DATE"
  echo ""
  echo "### Added"
  echo ""
  echo "### Changed"
  echo ""
  echo "### Fixed"
  echo ""
  echo "---"
  echo ""
  tail -n +5 CHANGELOG.md
} > "$TEMP"
mv "$TEMP" CHANGELOG.md

echo "✅ Updated version to $NEW_VERSION"
echo ""
echo "Next steps:"
echo "  1. Review: git diff"
echo "  2. Update CHANGELOG.md with actual release notes"
echo "  3. Commit: git commit -am 'chore: Bump version to $NEW_VERSION'"
echo "  4. Tag: git tag -a v$NEW_VERSION -m 'Release v$NEW_VERSION'"
echo "  5. Push: git push origin main --tags"
echo "  6. Publish: ./scripts/publish-crates.sh"
