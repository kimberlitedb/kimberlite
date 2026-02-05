#!/usr/bin/env bash
#
# Extract Coq specifications to OCaml (Phase 2.6)
#
# This script runs Coq extraction to generate OCaml code from formal
# specifications. The OCaml output is then manually inspected to create
# Rust trait definitions and wrappers.
#
# Usage:
#   ./scripts/extract_coq.sh
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SPECS_DIR="$PROJECT_ROOT/specs/coq"
OUTPUT_DIR="$SPECS_DIR/extracted"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Docker image
COQIMAGE="coqorg/coq:8.18"

echo -e "${BLUE}=== Coq Extraction (Phase 2.6) ===${NC}"
echo

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo -e "${RED}Error: Docker is not running${NC}"
    echo "Please start Docker and try again"
    exit 1
fi

# Create output directory
mkdir -p "$OUTPUT_DIR"

echo -e "${BLUE}Extracting Coq specifications to OCaml...${NC}"

# Run extraction
if docker run --rm \
    -v "$SPECS_DIR:/workspace" \
    -w /workspace \
    "$COQIMAGE" \
    coqc -Q . Kimberlite Extract.v 2>&1 | tee /tmp/coq_extract_output.txt; then

    echo
    echo -e "${GREEN}✅ Extraction successful${NC}"
    echo

    # Move extracted files to output directory
    if [ -f "$SPECS_DIR/Extract.ml" ]; then
        mv "$SPECS_DIR"/Extract*.ml* "$OUTPUT_DIR/" 2>/dev/null || true
        echo -e "${BLUE}Extracted files:${NC}"
        ls -lh "$OUTPUT_DIR"/Extract*.ml* 2>/dev/null || echo "  (No .ml files found)"
    fi

    # Clean up Coq compilation artifacts
    rm -f "$SPECS_DIR"/*.vo "$SPECS_DIR"/*.vok "$SPECS_DIR"/*.vos "$SPECS_DIR"/*.glob

    echo
    echo -e "${BLUE}=== Next Steps ===${NC}"
    echo
    echo "1. Inspect extracted OCaml code:"
    echo "   cat $OUTPUT_DIR/Extract.mli"
    echo
    echo "2. Manually create Rust trait definitions in:"
    echo "   crates/kimberlite-crypto/src/verified/"
    echo
    echo "3. Implement traits using vetted crypto libraries:"
    echo "   - sha2 for SHA-256"
    echo "   - blake3 for BLAKE3"
    echo "   - aes-gcm for AES-256-GCM"
    echo "   - ed25519-dalek for Ed25519"
    echo
    echo "4. Embed proof certificates from Coq:"
    echo "   pub const THEOREM_CERT: ProofCertificate = ..."
    echo
    echo "5. Run tests:"
    echo "   cargo test -p kimberlite-crypto --features verified-crypto"
    echo

else
    echo -e "${RED}❌ Extraction failed${NC}"
    echo -e "${YELLOW}Error output:${NC}"
    cat /tmp/coq_extract_output.txt
    exit 1
fi

echo -e "${GREEN}Done!${NC}"
