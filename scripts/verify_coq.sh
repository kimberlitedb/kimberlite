#!/usr/bin/env bash
#
# Verify Coq specifications (Phase 2)
#
# This script verifies all Coq files in specs/coq/ using Docker.
# It checks syntax, type-checks, and verifies all proofs.
#
# Usage:
#   ./scripts/verify_coq.sh           # Verify all files
#   ./scripts/verify_coq.sh SHA256.v  # Verify specific file
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SPECS_DIR="$PROJECT_ROOT/specs/coq"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Docker image
COQIMAGE="coqorg/coq:8.18"

echo -e "${BLUE}=== Coq Verification (Phase 2) ===${NC}"
echo

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo -e "${RED}Error: Docker is not running${NC}"
    echo "Please start Docker and try again"
    exit 1
fi

# Pull Docker image if not present
if ! docker image inspect "$COQIMAGE" > /dev/null 2>&1; then
    echo -e "${YELLOW}Pulling Coq Docker image: $COQIMAGE${NC}"
    docker pull "$COQIMAGE"
    echo
fi

# Verification order (dependencies first)
FILES=(
    "Common.v"
    "SHA256.v"
    "BLAKE3.v"
    # "AES_GCM.v"     # TODO: Phase 2.3
    # "Ed25519.v"     # TODO: Phase 2.4
    # "KeyHierarchy.v" # TODO: Phase 2.5
    # "Extract.v"     # TODO: Phase 2.6
)

# If specific file provided, verify only that
if [ $# -gt 0 ]; then
    FILES=("$@")
fi

# Clean up any previous compilation artifacts
echo -e "${BLUE}Cleaning previous artifacts...${NC}"
rm -f "$SPECS_DIR"/*.vo "$SPECS_DIR"/*.vok "$SPECS_DIR"/*.vos "$SPECS_DIR"/*.glob
echo

# Verify each file
FAILED=0
PASSED=0

for file in "${FILES[@]}"; do
    if [ ! -f "$SPECS_DIR/$file" ]; then
        echo -e "${YELLOW}⚠️  Skipping $file (file not found)${NC}"
        continue
    fi

    echo -e "${BLUE}Verifying $file...${NC}"

    # Run coqc in Docker (keep .vo files for dependencies)
    if docker run --rm \
        -v "$SPECS_DIR:/workspace" \
        -w /workspace \
        "$COQIMAGE" \
        coqc -Q . Kimberlite "$file" 2>&1 | tee /tmp/coq_output.txt; then

        echo -e "${GREEN}✅ $file verified successfully${NC}"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}❌ $file verification failed${NC}"
        echo -e "${YELLOW}Error output:${NC}"
        cat /tmp/coq_output.txt
        FAILED=$((FAILED + 1))
    fi
    echo
done

# Summary
echo -e "${BLUE}=== Verification Summary ===${NC}"
echo -e "${GREEN}Passed: $PASSED${NC}"
if [ $FAILED -gt 0 ]; then
    echo -e "${RED}Failed: $FAILED${NC}"
    exit 1
else
    echo -e "${GREEN}All files verified! ✅${NC}"
fi

# Clean up generated files
echo
echo -e "${BLUE}Cleaning up generated files...${NC}"
rm -f "$SPECS_DIR"/*.vo "$SPECS_DIR"/*.vok "$SPECS_DIR"/*.vos "$SPECS_DIR"/*.glob

echo -e "${GREEN}Done!${NC}"
