# Attribute-Based Access Control (ABAC)

Kimberlite provides **attribute-based access control** that extends RBAC with context-aware, fine-grained access decisions:

- **12 condition types** — user, resource, and environment attributes
- **3 pre-built compliance policies** — HIPAA, FedRAMP, PCI DSS
- **Priority-based rule evaluation** — highest priority rule wins, deterministic decisions
- **Logical combinators** — And, Or, Not for arbitrarily complex policies
- **JSON-serializable policies** — store, transmit, and audit policies as structured data
- **Two-layer enforcement** — RBAC (coarse-grained) then ABAC (fine-grained)

---

## Why ABAC Matters

**RBAC assigns roles at login. ABAC evaluates context at request time.**

Static roles cannot express policies like:

- "PHI access is allowed only during business hours" (time-based)
- "FedRAMP data must only be accessed from within the US" (location-based)
- "PCI data is only accessible from server devices" (device-based)
- "Only users with clearance level 2+ can access Sensitive data" (attribute-based)

**Regulatory drivers:**

| Framework | Requirement | What ABAC Enables |
|-----------|-------------|-------------------|
| **GDPR** | Article 25 — Privacy by design | Context-aware access minimization |
| **HIPAA** | § 164.312(a)(1) — Access control | Time-of-day and clearance-based PHI access |
| **FedRAMP** | AC-3 — Access enforcement | Location-based access restrictions |
| **PCI DSS** | Requirement 7 — Restrict access | Device and clearance-based card data access |
| **ISO 27001** | A.5.15 — Access control | Multi-attribute access policies |

---

## Architecture

ABAC operates as a **second layer** after RBAC. Both layers must allow access for the request to proceed:

```
┌─────────────────────────────────────────────┐
│  Access Request                              │
│  (User + Resource + Environment Attributes)  │
└─────────────────┬───────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────┐
│  Layer 1: RBAC (Coarse-Grained)              │
│  ├─ Role-based column filtering              │
│  ├─ Row-level security (tenant isolation)    │
│  └─ Permission checks (read/write/delete)    │
│                                              │
│  If RBAC denies → Access Denied              │
└─────────────────┬───────────────────────────┘
                  │ RBAC allows
                  ▼
┌─────────────────────────────────────────────┐
│  Layer 2: ABAC (Fine-Grained)                │
│  ├─ Evaluate rules by priority               │
│  ├─ Match conditions against attributes      │
│  └─ Return Allow/Deny decision               │
│                                              │
│  If ABAC denies → Access Denied              │
└─────────────────┬───────────────────────────┘
                  │ ABAC allows
                  ▼
┌─────────────────────────────────────────────┐
│  Decision                                    │
│  - Effect (Allow/Deny)                       │
│  - Matched rule name                         │
│  - Human-readable reason                     │
└─────────────────────────────────────────────┘
```

**Why two layers?** RBAC is fast and handles 90% of access decisions. ABAC adds context-awareness for the remaining 10% that need environmental factors. This avoids the performance cost of evaluating complex ABAC policies for every request.

---

## Attribute Categories

ABAC evaluates three categories of attributes:

### User Attributes

Populated from the authentication/identity provider at the start of each request.

| Attribute | Type | Description |
|-----------|------|-------------|
| `role` | String | User's RBAC role (e.g., "admin", "analyst") |
| `department` | String | Organizational department (e.g., "engineering") |
| `clearance_level` | u8 (0-3) | Security clearance: 0=public, 1=confidential, 2=secret, 3=top secret |
| `ip_address` | Option\<String\> | Request origin IP address |
| `device_type` | DeviceType | Desktop, Mobile, Server, or Unknown |
| `tenant_id` | Option\<u64\> | Tenant the user belongs to |

```rust
use kimberlite_abac::attributes::{UserAttributes, DeviceType};

let user = UserAttributes::new("analyst", "engineering", 2)
    .with_ip("10.0.1.50")
    .with_device(DeviceType::Desktop)
    .with_tenant(42);
```

### Resource Attributes

Populated from stream metadata and the data catalog at query time.

| Attribute | Type | Description |
|-----------|------|-------------|
| `data_class` | DataClass | Classification level (Public through PHI) |
| `owner_tenant` | u64 | Tenant that owns this resource |
| `stream_name` | String | Name of the stream being accessed |

```rust
use kimberlite_abac::attributes::ResourceAttributes;
use kimberlite_types::DataClass;

let resource = ResourceAttributes::new(DataClass::PHI, 1, "patient_records");
```

### Environment Attributes

Computed at request time from system state. Not user-controlled, making them harder to forge.

| Attribute | Type | Description |
|-----------|------|-------------|
| `timestamp` | DateTime\<Utc\> | When the request was made |
| `is_business_hours` | bool | 09:00-17:00 UTC, weekdays (Mon-Fri) |
| `source_country` | String | ISO 3166-1 alpha-2 country code |

```rust
use kimberlite_abac::attributes::EnvironmentAttributes;
use chrono::Utc;

// Auto-compute business hours from timestamp
let env = EnvironmentAttributes::from_timestamp(Utc::now(), "US");

// Or set explicitly
let env = EnvironmentAttributes::new(Utc::now(), true, "US");
```

---

## Conditions

Conditions are the building blocks of ABAC rules. Kimberlite provides 12 condition types:

### User Attribute Conditions

| Condition | Description |
|-----------|-------------|
| `RoleEquals(role)` | User's role must equal the specified value |
| `ClearanceLevelAtLeast(level)` | User's clearance must be >= the specified level |
| `DepartmentEquals(dept)` | User's department must equal the specified value |
| `TenantEquals(id)` | User's tenant must equal the specified ID |

### Resource Attribute Conditions

| Condition | Description |
|-----------|-------------|
| `DataClassAtMost(class)` | Resource classification must be at or below the specified level |
| `StreamNameMatches(pattern)` | Stream name must match the glob pattern (`*` and `?` wildcards) |

### Environment Conditions

| Condition | Description |
|-----------|-------------|
| `BusinessHoursOnly` | Request must be during business hours (09:00-17:00 UTC, weekdays) |
| `CountryIn(list)` | Source country must be in the specified list |
| `CountryNotIn(list)` | Source country must NOT be in the specified list |

### Logical Combinators

| Condition | Description |
|-----------|-------------|
| `And(conditions)` | All sub-conditions must be true |
| `Or(conditions)` | At least one sub-condition must be true |
| `Not(condition)` | The sub-condition must be false |

**Example:** PHI access requires clearance 2+ AND business hours AND US location:

```rust
use kimberlite_abac::policy::Condition;

let condition = Condition::And(vec![
    Condition::ClearanceLevelAtLeast(2),
    Condition::BusinessHoursOnly,
    Condition::CountryIn(vec!["US".to_string()]),
]);
```

---

## Policies and Rules

### Policy Structure

A policy contains ordered rules and a default effect:

```rust
use kimberlite_abac::policy::{AbacPolicy, Rule, Condition, Effect};

let policy = AbacPolicy::new(Effect::Deny)  // Default: deny if no rule matches
    .with_rule(Rule {
        name: "allow-analysts-business-hours".to_string(),
        effect: Effect::Allow,
        conditions: vec![
            Condition::RoleEquals("analyst".to_string()),
            Condition::BusinessHoursOnly,
        ],
        priority: 10,
    })
    .with_rule(Rule {
        name: "allow-admins-always".to_string(),
        effect: Effect::Allow,
        conditions: vec![
            Condition::RoleEquals("admin".to_string()),
        ],
        priority: 20,  // Higher priority — evaluated first
    });
```

### Rule Evaluation

Rules are evaluated by **priority** (highest first). The first rule whose conditions **all match** determines the outcome. If no rule matches, the policy's `default_effect` applies.

```
Rules sorted by priority (descending):
  1. priority=20: allow-admins-always   → conditions match? → Allow
  2. priority=10: allow-analysts-biz    → conditions match? → Allow
  3. (no more rules)                    → default_effect    → Deny
```

### Default Effect

The default effect is applied when no rule matches. **Always default to Deny** — this is the safe choice:

```rust
// Safe: deny unless explicitly allowed
let policy = AbacPolicy::new(Effect::Deny);

// Dangerous: allow unless explicitly denied (not recommended)
let policy = AbacPolicy::new(Effect::Allow);
```

---

## Pre-Built Compliance Policies

### HIPAA Policy

PHI access requires clearance >= 2 AND business hours. Non-PHI data (Confidential and below) is accessible to everyone.

```rust
use kimberlite_abac::policy::AbacPolicy;

let policy = AbacPolicy::hipaa_policy();

// Rule 1 (priority 10): PHI access = clearance >= 2 + business hours
// Rule 2 (priority 5):  Non-PHI = DataClass <= Confidential → Allow
// Default: Deny
```

| Request | Clearance | Time | Data Class | Decision |
|---------|-----------|------|------------|----------|
| Doctor queries patient records | 2 | 10:00 UTC Wed | PHI | **Allow** |
| Doctor queries patient records | 2 | 22:00 UTC Wed | PHI | **Deny** (after hours) |
| Nurse queries patient records | 1 | 10:00 UTC Wed | PHI | **Deny** (low clearance) |
| Analyst queries metrics | 0 | 22:00 UTC Sat | Confidential | **Allow** (non-PHI rule) |

### FedRAMP Policy

All access denied from outside the United States.

```rust
let policy = AbacPolicy::fedramp_policy();

// Rule 1 (priority 100): CountryNotIn(["US"]) → Deny
// Rule 2 (priority 50):  CountryIn(["US"])     → Allow
// Default: Deny
```

| Request Origin | Decision |
|----------------|----------|
| US | **Allow** |
| DE (Germany) | **Deny** |
| CN (China) | **Deny** |

### PCI DSS Policy

PCI data accessible only from Server devices with clearance >= 2. Non-PCI data is open.

```rust
let policy = AbacPolicy::pci_policy();

// Rule 1 (priority 10): clearance >= 2 + Server device → Allow
// Rule 2 (priority 5):  DataClass <= Confidential       → Allow
// Default: Deny
```

---

## Usage

### Evaluate an Access Request

```rust
use kimberlite_abac::evaluator;
use kimberlite_abac::policy::{AbacPolicy, Effect};
use kimberlite_abac::attributes::{
    UserAttributes, ResourceAttributes, EnvironmentAttributes,
};
use kimberlite_types::DataClass;
use chrono::Utc;

let policy = AbacPolicy::hipaa_policy();

let user = UserAttributes::new("doctor", "medicine", 2);
let resource = ResourceAttributes::new(DataClass::PHI, 1, "patient_records");
let env = EnvironmentAttributes::from_timestamp(Utc::now(), "US");

let decision = evaluator::evaluate(&policy, &user, &resource, &env);

match decision.effect {
    Effect::Allow => println!("Access granted: {}", decision.reason),
    Effect::Deny => println!("Access denied: {}", decision.reason),
}

// decision.matched_rule = Some("hipaa-phi-access")
// decision.reason = "Matched rule 'hipaa-phi-access' (priority 10)"
```

### Custom Policies

Build policies for your specific compliance requirements:

```rust
use kimberlite_abac::policy::{AbacPolicy, Rule, Condition, Effect};

// Only the compliance department can access audit streams during business hours
let policy = AbacPolicy::new(Effect::Deny)
    .with_rule(Rule {
        name: "compliance-audit-access".to_string(),
        effect: Effect::Allow,
        conditions: vec![Condition::And(vec![
            Condition::DepartmentEquals("compliance".to_string()),
            Condition::StreamNameMatches("audit_*".to_string()),
            Condition::BusinessHoursOnly,
        ])],
        priority: 10,
    });
```

### Serialize Policies as JSON

Policies are fully serializable for storage, transmission, and auditing:

```rust
let policy = AbacPolicy::hipaa_policy();

// Serialize to JSON
let json = serde_json::to_string_pretty(&policy).unwrap();

// Deserialize from JSON
let restored: AbacPolicy = serde_json::from_str(&json).unwrap();
```

---

## Data Classification Ordering

ABAC uses the same 8-level classification ordering as the rest of Kimberlite:

```
Public (0) < Deidentified (1) < Confidential (2) < Financial (3) <
PII (4) < PCI (5) < Sensitive (6) < PHI (7)
```

The `DataClassAtMost` condition uses this ordering. For example, `DataClassAtMost("Confidential")` allows access to Public, Deidentified, and Confidential data, but denies access to Financial, PII, PCI, Sensitive, and PHI data.

---

## Security Model

### Default Deny

All pre-built policies and the `AbacPolicy::default()` use `Effect::Deny` as the default. If no rule matches, access is denied. This ensures that misconfigured or incomplete policies fail safely.

### Clearance Levels

Clearance levels provide a coarse-grained sensitivity hierarchy:

| Level | Name | Typical Access |
|-------|------|----------------|
| 0 | Public | Public and deidentified data |
| 1 | Confidential | Internal business data |
| 2 | Secret | PII, PHI, PCI data |
| 3 | Top Secret | Full system access |

### Environment Attributes Are System-Controlled

Environment attributes (timestamp, business hours, source country) are computed by the server, not provided by the client. This makes them harder to forge — an attacker cannot claim to be in the US when their IP resolves to Germany.

---

## Formal Verification

### Kani Bounded Model Checking

**File:** `crates/kimberlite-abac/src/kani_proofs.rs`

| Proof | Property |
|-------|----------|
| `verify_policy_evaluation` | Policy evaluation correctness |
| `verify_conflict_resolution` | Highest priority rule wins |
| `verify_attribute_extraction` | No data loss from request context |

### TLA+ Specification

**File:** `specs/tla/compliance/RBAC.tla`

**Properties:**
- `PolicyCompleteness` — every request gets a deterministic decision
- `NoConflictingRules` — priority ordering resolves all conflicts
- `DeterministicDecision` — same inputs always produce same output

### Production Assertions

Every evaluation enforces:

- `assert!(clearance_level <= 3)` — clearance level must be 0-3
- Decision always returned (never panics on valid input)
- Priority-sorted evaluation (highest first)

---

## Best Practices

### 1. Layer ABAC on Top of RBAC

Don't replace RBAC with ABAC — use both. RBAC handles 90% of access decisions with minimal overhead. ABAC adds context-awareness for the remaining 10%.

### 2. Default to Deny

Always use `Effect::Deny` as the default effect. Misconfigured policies should deny access, not grant it.

### 3. Use High Priorities for Deny Rules

Deny rules should have higher priority than allow rules. This ensures that explicit denials cannot be overridden by lower-priority allow rules.

```rust
// Deny rule: priority 100 (evaluated first)
Rule { effect: Effect::Deny, priority: 100, .. }

// Allow rule: priority 10 (evaluated after denies)
Rule { effect: Effect::Allow, priority: 10, .. }
```

### 4. Audit Every Decision

Log every ABAC decision with the matched rule name and reason. This is required for SOC 2 CC7.2 and ISO 27001 A.12.4.1 audit trail requirements.

### 5. Test with All Compliance Policies

Run integration tests with `hipaa_policy()`, `fedramp_policy()`, and `pci_policy()` to ensure your data access patterns comply with all applicable frameworks.

---

## See Also

- [Role-Based Access Control](rbac.md) — RBAC layer (evaluated before ABAC)
- [Data Classification](data-classification.md) — 8-level classification used by `DataClassAtMost`
- [Field-Level Masking](field-masking.md) — Post-query data masking
- [Compliance Overview](compliance.md) — Multi-framework compliance architecture
- [Access Control Guide](../coding/guides/access-control.md) — Implementation patterns

---

**Key Takeaway:** ABAC extends RBAC with context-aware access decisions — time-of-day, location, device type, and clearance level. Combined with RBAC, it provides two layers of access control that satisfy HIPAA, GDPR, FedRAMP, and PCI DSS requirements.
