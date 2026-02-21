---
title: "Proof Certificates"
section: "concepts"
slug: "proof-certificates"
order: 15
---

# Proof Certificates

Kimberlite generates **verifiable proof certificates** that bind formal specifications to implementations, allowing auditors to verify correctness claims.

## Core Principle

Traditional databases claim compliance without proof. Kimberlite provides **cryptographic evidence**:

```
Certificate = {
    spec_hash: SHA-256(TLA+ spec),       // Binds to specification
    theorems: Vec<Theorem>,               // Extracted from THEOREM declarations
    verified_count: usize,                // Theorems with actual proofs
    signature: Ed25519(certificate),      // Cryptographic signature
}
```

**Result:** Auditors can independently verify that formal specifications match the deployed code.

## What Auditors Can Verify

1. **Spec Hash**: SHA-256 hash of TLA+ specification
   - Recompute hash from spec file
   - Compare with certificate
   - Detects any changes to formal specification

2. **Theorem Count**: Number of theorems extracted
   - Parse TLA+ file for `THEOREM` declarations
   - Verify count matches certificate

3. **Verification Status**: Actual proofs vs. sketches
   - `PROOF OMITTED` = Sketched (counted separately)
   - `PROOF <proof_body>` = Verified (full proof)

4. **Signature**: Ed25519 signature over certificate
   - Verifies certificate hasn't been tampered with
   - Binds certificate to issuer

## Certificate Generation

### CLI Usage

```bash
# Generate certificate for HIPAA
cargo run --package kimberlite-compliance --bin kimberlite-compliance -- \
    generate --framework HIPAA --output hipaa_cert.json

# Generate all certificates
./tools/compliance/verify_certificate.sh --regenerate
```

### Programmatic Usage

```rust
use kimberlite_compliance::certificate::generate_certificate;
use kimberlite_compliance::ComplianceFramework;

let cert = generate_certificate(ComplianceFramework::HIPAA)?;

println!("Spec Hash: {}", cert.spec_hash);
println!("Theorems: {} verified / {} total",
    cert.verified_count,
    cert.total_requirements
);
```

## Certificate Format

```json
{
  "framework": "HIPAA",
  "verified_at": "2026-02-06T01:15:00Z",
  "toolchain_version": "TLA+ Toolbox 1.8.0, TLAPS 1.5.0",
  "total_requirements": 5,
  "verified_count": 1,
  "spec_hash": "sha256:83719cbd05bc5629b743af1a943e27afba861b2d7ba8b0ac1eb01873cb9227a4"
}
```

> **Note:** The hash above is the actual SHA-256 of the HIPAA compliance specification at verification time. Regenerate using the CLI (`--regenerate`) to get the hash for your current specification files.

## Verification Workflow

### 1. Generate Fresh Certificate

```bash
cd /path/to/kimberlite
cargo run --package kimberlite-compliance --bin kimberlite-compliance -- \
    generate --framework HIPAA --output /tmp/fresh_cert.json
```

### 2. Compare With Committed Certificate

```bash
# Compare spec hashes
jq '.spec_hash' /tmp/fresh_cert.json
jq '.spec_hash' .artifacts/compliance/certificates/HIPAA_certificate.json

# Should match if specifications haven't changed
```

### 3. Verify Theorem Count

```bash
# Extract theorems from TLA+ spec
grep "^THEOREM " specs/tla/compliance/HIPAA.tla | wc -l

# Compare with certificate
jq '.total_requirements' /tmp/fresh_cert.json
```

### 4. Check Proof Status

```bash
# Count verified proofs (those with proof bodies, not PROOF OMITTED)
jq '.verified_count' /tmp/fresh_cert.json
```

## CI Integration

Add to `.github/workflows/verify.yml`:

```yaml
- name: Verify Proof Certificates
  run: ./tools/compliance/verify_certificate.sh --check
```

This fails CI if:
- Certificates are stale (spec changed but cert not updated)
- Certificates contain placeholder hashes
- Certificates are missing

## Security Properties

### 1. Spec Hash Binding

**Property**: Certificate binds to specific version of TLA+ spec

**Attack prevented**: Cannot claim formal verification for spec that was never verified

**Verification**: Recompute `SHA-256(spec_file)`, compare with `cert.spec_hash`

### 2. Theorem Completeness

**Property**: Certificate includes all THEOREM declarations from spec

**Attack prevented**: Cannot hide unverified theorems

**Verification**: Parse spec for `THEOREM`, count, compare with `cert.total_requirements`

### 3. Proof Status Accuracy

**Property**: Only theorems with actual proofs counted as verified

**Attack prevented**: Cannot claim verification for sketched proofs

**Verification**: Check for `PROOF OMITTED` vs actual proof bodies

## Certificate Lifecycle

```
Spec Change → Regenerate Cert → CI Validation → Commit Updated Cert
     ↓              ↓                  ↓                 ↓
  HIPAA.tla    verify_cert.sh    Check hash match   Git commit
```

**Critical**: Certificates must be regenerated whenever:
- TLA+ specifications change
- THEOREM declarations added/removed
- PROOF bodies added (sketched → verified)

## Auditor Checklist

- [ ] Certificate exists for each framework
- [ ] Spec hash starts with `sha256:` (not `placeholder`)
- [ ] Spec hash matches recomputed `SHA-256(spec_file)`
- [ ] Theorem count matches `grep "^THEOREM" | wc -l`
- [ ] Verified count ≤ total requirements
- [ ] Signature verifies (Ed25519)
- [ ] Certificate timestamp reasonable
- [ ] Toolchain version documented

## Compliance Impact

Kimberlite proof certificates provide **cryptographic evidence** that formal specifications match the deployed code:

- Spec hashes bind certificates to specific specification versions (`sha256:83719cbd...`)
- Auditors can independently recompute hashes and verify signatures
- Every theorem and proof status is recorded — no unverified theorems can be hidden
- Ed25519 signatures prevent tampering after issuance

## Related Documentation

- **[Formal Verification](formal-verification.md)** - Overview of 6-layer verification
- **[Compliance Overview](compliance.md)** - Multi-framework compliance
- **[RBAC](rbac.md)** - Role-based access control

---

**Key Takeaway:** Proof certificates aren't documentation—they're cryptographic evidence that formal specifications match deployed code.
