# Formal Verification Tools

This directory contains tools for formal verification of Kimberlite.

## Tools

### Alloy (alloy/)
- **Purpose:** Structural property verification (hash chains, quorums)
- **Installation:** JAR included (`alloy-6.2.0.jar`)
- **Usage:** `just verify-alloy` or `java -jar alloy/alloy-6.2.0.jar <spec-file>`

### Docker (docker/)
- **Purpose:** Containerized TLAPS and Ivy verification
- **Setup:** Automatically pulled when running `just verify-tlaps` or `just verify-ivy`

## Quick Start

```bash
# Run all verification locally
just verify-local

# Individual tools
just verify-tla       # TLA+ model checking
just verify-tlaps     # TLAPS proofs
just verify-ivy       # Ivy Byzantine model
just verify-alloy     # Alloy structural models
just verify-coq       # Coq crypto proofs
just verify-kani      # Kani code verification
```

See `justfile` for all available verification commands.
