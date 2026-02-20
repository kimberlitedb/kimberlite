---
title: "Field-Level Data Masking"
section: "concepts"
slug: "field-masking"
order: 11
---

# Field-Level Data Masking

Kimberlite provides **field-level data masking** to enforce the HIPAA "minimum necessary" principle and GDPR data minimization at the column level:

- **5 masking strategies** (Redact, Hash, Tokenize, Truncate, Null)
- **Role-based application** — different roles see different views of the same data
- **Admin exemption** — privileged users see raw data when necessary
- **Deterministic output** — preserves referential integrity for JOINs (Hash, Tokenize)
- **Formal verification** — Kani proofs for masking correctness

---

## Why Field Masking Matters

**Column filtering alone isn't enough.** RBAC can hide entire columns, but regulations often require *partial* visibility:

- A billing analyst needs to *verify* an SSN exists but should only see `***-**-1234`
- An auditor must confirm a patient record has a phone number, but shouldn't see the full number
- A support agent needs to identify a credit card ending in `3456` without seeing all 16 digits

**Regulatory requirements:**

| Framework | Requirement | What It Means |
|-----------|-------------|---------------|
| **HIPAA** | § 164.312(a)(1) | Minimum necessary access to PHI |
| **GDPR** | Article 5(1)(c) | Data minimization — limit exposure |
| **PCI DSS** | Requirement 3.4 | Mask PAN when displayed (first 6/last 4) |
| **SOC 2** | CC6.1 | Logical access controls for sensitive data |

---

## Masking Strategies

### 1. Redact

**Pattern-aware partial redaction** that preserves structure while hiding sensitive content.

| Pattern | Input | Output |
|---------|-------|--------|
| `Ssn` | `123-45-6789` | `***-**-6789` |
| `Phone` | `555-867-5309` | `***-***-5309` |
| `Email` | `alice@example.com` | `a***@example.com` |
| `CreditCard` | `4111-1111-1111-1234` | `****-****-****-1234` |
| `Custom` | *(any)* | *(configured replacement)* |

**Use when:** The format of the value matters (e.g., confirming an SSN ends in a known suffix).

### 2. Hash

**SHA-256 one-way hash**, hex-encoded (64 characters).

| Input | Output |
|-------|--------|
| `alice@example.com` | `2c740c48e7f0a9ab3c38... (64 chars)` |

**Use when:** You need referential integrity (JOINs across masked datasets) without revealing the original value. Same input always produces the same hash.

### 3. Tokenize

**Deterministic BLAKE3 token** prefixed with `tok_` (20 characters total).

| Input | Output |
|-------|--------|
| `4111-1111-1111-1234` | `tok_a1b2c3d4e5f6g7h8` |

**Use when:** You need a reversible-by-Admin token for payment processing workflows. Deterministic output preserves JOIN capability.

### 4. Truncate

**Keep first N characters**, replace the rest with `"..."`.

| Input | max_chars | Output |
|-------|-----------|--------|
| `John Smith` | 4 | `John...` |
| `alice@example.com` | 5 | `alice...` |

**Use when:** Partial visibility is sufficient (e.g., patient name initials for scheduling).

### 5. Null

**Replace with empty value** (zero bytes).

| Input | Output |
|-------|--------|
| *(any)* | *(empty)* |

**Use when:** The value must be completely hidden but the column must remain in results (schema stability).

---

## Architecture

Masking is applied as a **post-processing step** after RBAC column filtering and query execution:

```
┌─────────────────────────────────────┐
│  Original Query                      │
│  SELECT name, ssn FROM patients      │
└───────────────┬─────────────────────┘
                │
                ▼
┌─────────────────────────────────────┐
│  1. RBAC Column Filtering            │
│  Remove unauthorized columns         │
└───────────────┬─────────────────────┘
                │
                ▼
┌─────────────────────────────────────┐
│  2. Query Execution                  │
│  Execute against projection store    │
└───────────────┬─────────────────────┘
                │
                ▼
┌─────────────────────────────────────┐
│  3. Field Masking                    │
│  Apply masks to result set           │
│  SSN: 123-45-6789 → ***-**-6789     │
└───────────────┬─────────────────────┘
                │
                ▼
┌─────────────────────────────────────┐
│  Masked Result                       │
│  name: "Alice", ssn: "***-**-6789"  │
└─────────────────────────────────────┘
```

**Two-stage enforcement:** Column filtering removes unauthorized columns. Field masking transforms *allowed* columns based on role. This provides defense in depth.

---

## Usage

### Define a Masking Policy

```rust
use kimberlite_rbac::masking::{
    FieldMask, MaskingPolicy, MaskingStrategy, RedactPattern,
};
use kimberlite_rbac::roles::Role;

let policy = MaskingPolicy::new()
    .with_mask(
        FieldMask::new("ssn", MaskingStrategy::Redact(RedactPattern::Ssn))
            .applies_to(vec![Role::Analyst, Role::User, Role::Auditor])
            .exempt(vec![Role::Admin]),
    )
    .with_mask(
        FieldMask::new("email", MaskingStrategy::Hash)
            .applies_to(vec![Role::Auditor])
            .exempt(vec![Role::Admin, Role::Analyst, Role::User]),
    )
    .with_mask(
        FieldMask::new("credit_card", MaskingStrategy::Redact(RedactPattern::CreditCard))
            .applies_to(vec![Role::Analyst, Role::User, Role::Auditor])
            .exempt(vec![Role::Admin]),
    );
```

### Apply Masks to a Row

```rust
use kimberlite_rbac::masking::apply_masks_to_row;

let columns = vec!["name".into(), "ssn".into(), "email".into()];
let row = vec![
    b"Alice".to_vec(),
    b"123-45-6789".to_vec(),
    b"alice@example.com".to_vec(),
];

// Analyst sees: name=Alice, ssn=***-**-6789, email=alice@example.com
let masked = apply_masks_to_row(&row, &columns, &policy, &Role::Analyst)?;

// Admin sees: name=Alice, ssn=123-45-6789, email=alice@example.com (no masking)
let unmasked = apply_masks_to_row(&row, &columns, &policy, &Role::Admin)?;
```

---

## Role-to-Masking Defaults

Recommended masking configuration for regulated environments:

| Column Type | Admin | Analyst | User | Auditor |
|-------------|-------|---------|------|---------|
| **SSN** | Raw | Redacted (`***-**-6789`) | Redacted | Null |
| **Credit Card** | Raw | Redacted (`****-****-****-1234`) | Redacted | Null |
| **Email** | Raw | Raw | Raw (own data) | Hashed |
| **Phone** | Raw | Redacted (`***-***-5309`) | Raw (own data) | Null |
| **Name** | Raw | Raw | Raw (own data) | Truncated (`J...`) |

---

## Formal Verification

### Kani Bounded Model Checking

**File:** `crates/kimberlite-rbac/src/kani_proofs.rs`

| Proof | Property |
|-------|----------|
| `verify_mask_application` | Masked value differs from original for non-exempt roles |
| `verify_masking_policy_consistency` | Policy applies consistently across invocations |

### Production Assertions

Every masking operation enforces:

- `assert!(masked_value != original_value)` for restricted roles on masked columns
- `assert_eq!(output_row.len(), input_row.len())` — row length preserved
- `debug_assert!(strategy_deterministic)` — same input produces same output

---

## Performance

Masking is applied at the **result set level** (not in SQL), so overhead scales with result size, not query complexity:

| Operation | Overhead |
|-----------|----------|
| Redact (SSN/Phone/Email) | ~100ns per value |
| Hash (SHA-256) | ~200ns per value |
| Tokenize (BLAKE3) | ~150ns per value |
| Truncate | ~50ns per value |
| Null | ~10ns per value |

For a 1,000-row result with 3 masked columns: **< 1ms total overhead**.

---

## Best Practices

### 1. Mask at the Outermost Boundary

Apply masking as late as possible — at query result time, not in storage. This preserves the ability to run aggregations and JOINs on raw data internally.

### 2. Use Hash for JOIN Columns

If analysts need to correlate records across tables by email, use `Hash` strategy. The SHA-256 output is deterministic, so `JOIN ON hash(a.email) = hash(b.email)` works correctly.

### 3. Audit All Masking Operations

Log which fields were masked, for which role, and which strategy was applied. This is required for HIPAA § 164.312(b) audit controls.

### 4. Start with Strict Masking

Default to `Null` for all PII/PHI columns, then relax to `Redact` only when business need is documented and approved.

---

## See Also

- [Role-Based Access Control](rbac.md) — Column filtering and row-level security
- [Data Classification](data-classification.md) — 8-level classification system
- [Compliance Overview](compliance.md) — Multi-framework compliance architecture
- [Access Control Guide](/docs/coding/guides/access-control) — Implementation patterns

---

**Key Takeaway:** Field masking enforces the principle of **minimum necessary access** — users see exactly what they need, nothing more. Combined with RBAC column filtering and row-level security, Kimberlite provides three layers of data access control.
