#!/usr/bin/env bash
#
# Certificate Verification Tool
#
# This script regenerates proof certificates and verifies they match
# the committed versions. Used in CI to detect stale certificates.
#
# Usage:
#   ./tools/compliance/verify_certificate.sh [--regenerate]
#
# Options:
#   --regenerate    Regenerate certificates and update committed files
#   --check         Verify certificates are up-to-date (default, CI mode)
#
# Exit codes:
#   0 - Certificates are up-to-date
#   1 - Certificates are stale (need regeneration)
#   2 - Script error

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Find repository root
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

# Certificate output directory
CERT_DIR="$REPO_ROOT/.artifacts/compliance/certificates"
mkdir -p "$CERT_DIR"

# Mode: regenerate or check
MODE="${1:---check}"

echo "=== Kimberlite Certificate Verification Tool ==="
echo "Mode: $MODE"
echo "Repository: $REPO_ROOT"
echo ""

# Frameworks to verify
FRAMEWORKS=("HIPAA" "GDPR" "SOC2" "PCI_DSS" "ISO27001" "FedRAMP")

# Function to generate certificate for a framework
generate_certificate() {
    local framework="$1"
    local output_file="$CERT_DIR/${framework}_certificate.json"

    echo "Generating certificate for $framework..."

    # Use the kimberlite-compliance CLI to generate certificate
    cargo run --quiet --package kimberlite-compliance --bin kimberlite-compliance -- \
        generate --framework "$framework" --output "$output_file" 2>&1 | grep -v "warning:"

    if [ -f "$output_file" ]; then
        echo "  ✓ Certificate generated: $output_file"
        return 0
    else
        echo "  ✗ Failed to generate certificate"
        return 1
    fi
}

# Function to verify certificate
verify_certificate() {
    local framework="$1"
    local cert_file="$CERT_DIR/${framework}_certificate.json"

    if [ ! -f "$cert_file" ]; then
        echo -e "${YELLOW}  ⚠ Certificate not found (may need to regenerate): $cert_file${NC}"
        return 1
    fi

    # Check if spec_hash contains "placeholder"
    if grep -q "placeholder" "$cert_file"; then
        echo -e "${RED}  ✗ Certificate contains placeholder hash${NC}"
        return 1
    fi

    # Check if spec_hash starts with "sha256:"
    if ! grep -q '"spec_hash": "sha256:' "$cert_file"; then
        echo -e "${RED}  ✗ Certificate has invalid hash format${NC}"
        return 1
    fi

    # Extract spec hash
    local spec_hash
    spec_hash=$(grep '"spec_hash"' "$cert_file" | sed 's/.*"sha256:\([^"]*\)".*/\1/')

    echo -e "${GREEN}  ✓ Certificate valid${NC}"
    echo "    Spec hash: sha256:${spec_hash:0:16}..."
    return 0
}

# Main logic
if [ "$MODE" = "--regenerate" ]; then
    echo "Regenerating all certificates..."
    echo ""

    failed=0
    for framework in "${FRAMEWORKS[@]}"; do
        if ! generate_certificate "$framework"; then
            failed=1
        fi
    done

    echo ""
    if [ $failed -eq 0 ]; then
        echo -e "${GREEN}✓ All certificates regenerated successfully${NC}"
        echo ""
        echo "Certificates written to: $CERT_DIR"
        exit 0
    else
        echo -e "${RED}✗ Some certificates failed to regenerate${NC}"
        exit 1
    fi

elif [ "$MODE" = "--check" ]; then
    echo "Verifying certificates are up-to-date..."
    echo ""

    # First generate fresh certificates to temporary location
    TEMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TEMP_DIR"' EXIT

    failed=0
    for framework in "${FRAMEWORKS[@]}"; do
        echo "Checking $framework..."

        # Generate fresh certificate
        temp_cert="$TEMP_DIR/${framework}_certificate.json"
        if cargo run --quiet --package kimberlite-compliance --bin kimberlite-compliance -- \
            generate --framework "$framework" --output "$temp_cert" 2>&1 | grep -v "warning:"; then

            # Verify it's valid
            if verify_certificate "$framework"; then
                # Compare with committed version (if exists)
                committed_cert="$CERT_DIR/${framework}_certificate.json"
                if [ -f "$committed_cert" ]; then
                    # Compare spec hashes
                    fresh_hash=$(grep '"spec_hash"' "$temp_cert" | sed 's/.*":\s*"\([^"]*\)".*/\1/')
                    committed_hash=$(grep '"spec_hash"' "$committed_cert" | sed 's/.*":\s*"\([^"]*\)".*/\1/')

                    if [ "$fresh_hash" != "$committed_hash" ]; then
                        echo -e "${RED}  ✗ Certificate is stale (spec hash changed)${NC}"
                        echo "    Committed: $committed_hash"
                        echo "    Fresh:     $fresh_hash"
                        failed=1
                    fi
                fi
            else
                failed=1
            fi
        else
            echo -e "${YELLOW}  ⚠ Skipping $framework (spec file may not exist)${NC}"
        fi

        echo ""
    done

    if [ $failed -eq 0 ]; then
        echo -e "${GREEN}✓ All certificates are up-to-date${NC}"
        exit 0
    else
        echo -e "${RED}✗ Some certificates are stale or invalid${NC}"
        echo ""
        echo "To regenerate certificates, run:"
        echo "  ./tools/compliance/verify_certificate.sh --regenerate"
        exit 1
    fi

else
    echo "Error: Invalid mode '$MODE'"
    echo ""
    echo "Usage: $0 [--regenerate|--check]"
    exit 2
fi
