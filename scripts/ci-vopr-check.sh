#!/bin/bash
# Simulates the VOPR CI checks locally
# Run this before pushing to verify determinism and coverage

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "================================================"
echo "VOPR CI Simulation - Local Determinism Check"
echo "================================================"
echo ""

# Build VOPR
echo -e "${YELLOW}Building VOPR...${NC}"
cargo build --release -p kimberlite-sim --bin vopr
echo -e "${GREEN}✓ Build complete${NC}"
echo ""

# Check 1: Baseline scenario
echo -e "${YELLOW}Check 1: Baseline scenario (100 iterations)${NC}"
./target/release/vopr \
  --scenario baseline \
  --iterations 100 \
  --check-determinism \
  --seed 12345

if [ $? -eq 0 ]; then
  echo -e "${GREEN}✓ Baseline scenario passed${NC}"
else
  echo -e "${RED}✗ Baseline scenario failed${NC}"
  exit 1
fi
echo ""

# Check 2: Combined scenario
echo -e "${YELLOW}Check 2: Combined scenario (50 iterations)${NC}"
./target/release/vopr \
  --scenario combined \
  --iterations 50 \
  --check-determinism \
  --seed 54321

if [ $? -eq 0 ]; then
  echo -e "${GREEN}✓ Combined scenario passed${NC}"
else
  echo -e "${RED}✗ Combined scenario failed${NC}"
  exit 1
fi
echo ""

# Check 3: Multi-tenant isolation
echo -e "${YELLOW}Check 3: Multi-tenant isolation (50 iterations)${NC}"
./target/release/vopr \
  --scenario multi_tenant_isolation \
  --iterations 50 \
  --check-determinism \
  --seed 99999

if [ $? -eq 0 ]; then
  echo -e "${GREEN}✓ Multi-tenant scenario passed${NC}"
else
  echo -e "${RED}✗ Multi-tenant scenario failed${NC}"
  exit 1
fi
echo ""

# Check 4: Coverage enforcement
echo -e "${YELLOW}Check 4: Coverage enforcement (200 iterations)${NC}"
./target/release/vopr \
  --iterations 200 \
  --min-fault-coverage 80.0 \
  --min-invariant-coverage 100.0 \
  --require-all-invariants \
  --check-determinism

if [ $? -eq 0 ]; then
  echo -e "${GREEN}✓ Coverage enforcement passed${NC}"
else
  echo -e "${RED}✗ Coverage enforcement failed${NC}"
  exit 1
fi
echo ""

# Check 5: VSR Invariants
echo "================================================"
echo -e "${YELLOW}Check 5: VSR Invariants (100 iterations)${NC}"
echo "================================================"
./target/release/vopr \
  --scenario baseline \
  --iterations 100 \
  --seed 42001 \
  --enable-vsr-invariants \
  --check-determinism

if [ $? -eq 0 ]; then
  echo -e "${GREEN}✓ VSR invariants passed${NC}"
else
  echo -e "${RED}✗ VSR invariants failed${NC}"
  exit 1
fi
echo ""

# Check 6: Projection Invariants
echo "================================================"
echo -e "${YELLOW}Check 6: Projection Invariants (100 iterations)${NC}"
echo "================================================"
./target/release/vopr \
  --scenario baseline \
  --iterations 100 \
  --seed 42002 \
  --enable-projection-invariants \
  --check-determinism

if [ $? -eq 0 ]; then
  echo -e "${GREEN}✓ Projection invariants passed${NC}"
else
  echo -e "${RED}✗ Projection invariants failed${NC}"
  exit 1
fi
echo ""

# Check 7: Query Invariants
echo "================================================"
echo -e "${YELLOW}Check 7: Query Invariants (100 iterations)${NC}"
echo "================================================"
./target/release/vopr \
  --scenario baseline \
  --iterations 100 \
  --seed 42003 \
  --enable-query-invariants \
  --check-determinism

if [ $? -eq 0 ]; then
  echo -e "${GREEN}✓ Query invariants passed${NC}"
else
  echo -e "${RED}✗ Query invariants failed${NC}"
  exit 1
fi
echo ""

# All checks passed
echo "================================================"
echo -e "${GREEN}✓ All VOPR CI checks passed!${NC}"
echo "================================================"
echo ""
echo "Your changes are ready for CI."
echo "Determinism validated, coverage thresholds met."
