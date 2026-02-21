---
title: "Multi-tenancy"
section: "concepts"
slug: "multitenancy"
order: 5
---

# Multi-tenancy

Multi-tenancy is a first-class concept in Kimberlite, not an afterthought.

## Why Multi-tenancy Matters

In regulated industries, data isolation isn't optional—it's legally required:

- **HIPAA:** Patient data must be isolated between healthcare providers
- **GDPR:** Customer data must be segregated by organization
- **SOC 2:** Tenant data must be cryptographically separated
- **Industry best practices:** "Defense in depth" requires multiple isolation layers

Traditional databases offer "tenant_id columns" and hope you don't mess up WHERE clauses. Kimberlite makes tenant isolation **structural and cryptographic**.

## Tenant Isolation

Each tenant has:

| Isolation Layer | Description | Benefit |
|----------------|-------------|---------|
| **Separate log partitions** | Tenant data is not interleaved | Physical isolation |
| **Separate projections** | Each tenant has independent B+trees | Query isolation |
| **Separate encryption keys** | Per-tenant envelope encryption | Cryptographic isolation |
| **Separate quotas** | Storage and throughput limits | Resource isolation |

<div class="doc-diagram-wrapper">
<figure class="interactive-section__figure"
        data-signals="{focus: ''}"
        tabindex="0">
  <header class="interactive-section__figure-header">
    <span class="interactive-section__fig-label">Fig. 1</span>
    <span class="interactive-section__fig-caption">Each tenant has its own log, projections, and encryption key. Click a tenant to focus it.</span>
  </header>

  <div class="interactive-section__figure-content tenant-grid">
    <div class="tenant-grid__outer-label">Multi-Tenant Kimberlite</div>
    <div class="tenant-grid__tenants">

      <div class="tenant-grid__tenant"
           role="button"
           tabindex="0"
           data-class:is-focused="$focus === 'a'"
           data-class:is-dimmed="$focus && $focus !== 'a'"
           data-on:click="$focus = $focus === 'a' ? '' : 'a'"
           data-on:keydown="(evt.key === 'Enter' || evt.key === ' ') && ($focus = $focus === 'a' ? '' : 'a')">
        <div class="tenant-grid__tenant-header">Tenant A</div>
        <div class="tenant-grid__box">Log</div>
        <div class="tenant-grid__box">Projections</div>
        <div class="tenant-grid__key">Key: K_A</div>
        <div class="tenant-grid__badge" data-show="$focus === 'a'">ISOLATED</div>
      </div>

      <div class="tenant-grid__tenant"
           role="button"
           tabindex="0"
           data-class:is-focused="$focus === 'b'"
           data-class:is-dimmed="$focus && $focus !== 'b'"
           data-on:click="$focus = $focus === 'b' ? '' : 'b'"
           data-on:keydown="(evt.key === 'Enter' || evt.key === ' ') && ($focus = $focus === 'b' ? '' : 'b')">
        <div class="tenant-grid__tenant-header">Tenant B</div>
        <div class="tenant-grid__box">Log</div>
        <div class="tenant-grid__box">Projections</div>
        <div class="tenant-grid__key">Key: K_B</div>
        <div class="tenant-grid__badge" data-show="$focus === 'b'">ISOLATED</div>
      </div>

      <div class="tenant-grid__tenant"
           role="button"
           tabindex="0"
           data-class:is-focused="$focus === 'c'"
           data-class:is-dimmed="$focus && $focus !== 'c'"
           data-on:click="$focus = $focus === 'c' ? '' : 'c'"
           data-on:keydown="(evt.key === 'Enter' || evt.key === ' ') && ($focus = $focus === 'c' ? '' : 'c')">
        <div class="tenant-grid__tenant-header">Tenant C</div>
        <div class="tenant-grid__box">Log</div>
        <div class="tenant-grid__box">Projections</div>
        <div class="tenant-grid__key">Key: K_C</div>
        <div class="tenant-grid__badge" data-show="$focus === 'c'">ISOLATED</div>
      </div>

    </div>
  </div>

  <figcaption class="interactive-section__figure-footer [ cluster ]">
    <div class="interactive-section__legend">
      <span class="legend-item">Click a tenant to focus it — click again to clear.</span>
    </div>
  </figcaption>
</figure>
</div>

## TenantId Type

Tenants are identified by a newtype wrapper:

```rust
// Not this (unsafe, can mix up IDs)
let tenant: u64 = 1;

// This (type-safe, cannot mix up IDs)
let tenant = TenantId::new(1);
```

**Type safety prevents:**
- Using a stream ID where a tenant ID is expected
- Using offset 123 where tenant 123 is expected
- SQL injection via tenant ID (it's not a string)

## Regional Placement

Tenants can be assigned to specific regions for compliance:

```rust
// Create tenant with regional constraint
db.create_tenant(TenantConfig {
    id: TenantId::new(1),
    region: Region::UsEast,  // Data stays in US-East
    retention: Duration::from_secs(86400 * 2555),  // 7 years
})?;
```

**Why this matters:**

- **GDPR:** EU citizen data must stay in EU
- **HIPAA:** Healthcare data may have regional constraints
- **Data sovereignty:** Government regulations may require in-country storage
- **Latency:** Keep data close to users

The directory service routes requests to the correct region:

```rust
impl Directory {
    fn route(&self, tenant_id: TenantId) -> NodeSet {
        let placement = self.placements.get(&tenant_id)?;
        self.nodes_in_region(placement.region)
    }
}
```

## Per-Tenant Encryption

Each tenant's data is encrypted with a unique key using envelope encryption:

<div class="doc-diagram-wrapper">
<figure class="interactive-section__figure"
        data-signals="{revealed: false}"
        tabindex="0">
  <header class="interactive-section__figure-header">
    <span class="interactive-section__fig-label">Fig. 2</span>
    <span class="interactive-section__fig-caption">Envelope encryption hierarchy: one master key wraps per-tenant KEKs, which wrap per-tenant DEKs.</span>
  </header>

  <div class="interactive-section__figure-content key-hierarchy">

    <div class="key-hierarchy__level">
      <div class="key-hierarchy__node key-hierarchy__node--master">Master Key (HSM / KMS)</div>
    </div>

    <div class="key-hierarchy__connectors">
      <div class="key-hierarchy__connector" data-class:is-active="$revealed"></div>
      <div class="key-hierarchy__connector" data-class:is-active="$revealed"></div>
      <div class="key-hierarchy__connector" data-class:is-active="$revealed"></div>
    </div>

    <div class="key-hierarchy__level">
      <div class="key-hierarchy__node">KEK_A<br><small>wrapped, per-tenant</small></div>
      <div class="key-hierarchy__node">KEK_B<br><small>wrapped, per-tenant</small></div>
      <div class="key-hierarchy__node">KEK_C<br><small>wrapped, per-tenant</small></div>
    </div>

    <div class="key-hierarchy__connectors" data-show="$revealed">
      <div class="key-hierarchy__connector is-active"></div>
      <div class="key-hierarchy__connector is-active"></div>
      <div class="key-hierarchy__connector is-active"></div>
    </div>

    <div class="key-hierarchy__dek-level" data-show="$revealed">
      <div class="key-hierarchy__node key-hierarchy__node--dek">DEK_A<br><small>AES-256-GCM</small></div>
      <div class="key-hierarchy__node key-hierarchy__node--dek">DEK_B<br><small>AES-256-GCM</small></div>
      <div class="key-hierarchy__node key-hierarchy__node--dek">DEK_C<br><small>AES-256-GCM</small></div>
    </div>

  </div>

  <figcaption class="interactive-section__figure-footer [ cluster ]">
    <button class="interactive-button"
            data-on:click="$revealed = !$revealed">
      <span data-show="!$revealed">Show derivation</span>
      <span data-show="$revealed">Hide derivation</span>
    </button>
  </figcaption>
</figure>
</div>

**Key hierarchy:**

1. **Master Key:** Stored in HSM/KMS (AWS KMS, HashiCorp Vault, etc.)
2. **KEK (Key Encryption Key):** Per-tenant, encrypted with master key
3. **DEK (Data Encryption Key):** Per-tenant, encrypts actual data

**Benefits:**

- **Tenant deletion:** Delete KEK, all data becomes unrecoverable
- **Key rotation:** Rotate DEK without re-encrypting all data (re-wrap KEK)
- **Cryptographic isolation:** Even if attacker gets DEK_A, cannot decrypt Tenant B's data

## Per-Tenant Quotas

Prevent one tenant from monopolizing resources:

```rust
struct TenantQuota {
    // Storage limits
    max_log_size: u64,        // Max bytes in log
    max_projections: u64,     // Max projection size

    // Throughput limits
    max_ops_per_sec: u64,     // Rate limit
    max_concurrent_ops: u64,  // Concurrency limit
}
```

**Enforcement:**

- **Storage:** Reject writes that would exceed quota
- **Throughput:** Rate-limit using token bucket algorithm
- **Concurrency:** Reject new operations if limit reached

## Query Isolation

Tenants cannot query each other's data:

```sql
-- Client for Tenant A
SELECT * FROM patients WHERE id = 123;
-- Returns Tenant A's patient 123 (if exists)

-- Client for Tenant B
SELECT * FROM patients WHERE id = 123;
-- Returns Tenant B's patient 123 (if exists)
-- CANNOT see Tenant A's data
```

**How it works:**

1. Client authenticates with `TenantId`
2. All queries implicitly filter by `tenant_id`
3. Type system prevents mixing tenant IDs
4. No "cross-tenant queries" allowed (must use data sharing API if needed)

## Data Sharing (Controlled Cross-Tenant Access)

In some scenarios, tenants need to share data (e.g., multi-hospital referrals). Kimberlite provides explicit data sharing:

```rust
// Tenant A grants read access to Tenant B
db.grant_access(GrantConfig {
    from_tenant: TenantId::new(1),  // Tenant A
    to_tenant: TenantId::new(2),    // Tenant B
    stream: StreamId::new(1, 100),   // Specific stream
    permissions: Permissions::Read,
    expiration: Some(Duration::from_secs(3600)),  // 1 hour
})?;
```

**Properties:**

- **Explicit:** Sharing requires grant, not implicit JOIN
- **Audited:** All cross-tenant access logged
- **Revocable:** Grants can be revoked at any time
- **Time-limited:** Grants expire automatically
- **Fine-grained:** Grant access to specific streams, not entire tenant

See [Data Sharing Design](/docs/internals/design/data-sharing) for details.

## Sharding Strategies

For large deployments, shard tenants across multiple VSR groups:

### Strategy 1: Static Sharding

```
Shard 0 (VSR group 0): Tenants 1-1000
Shard 1 (VSR group 1): Tenants 1001-2000
Shard 2 (VSR group 2): Tenants 2001-3000
```

**Pros:** Simple, predictable
**Cons:** Uneven load if tenant sizes vary

### Strategy 2: Hash Sharding

```rust
fn shard_for_tenant(tenant_id: TenantId) -> ShardId {
    ShardId::new(tenant_id.as_u64() % NUM_SHARDS)
}
```

**Pros:** Even distribution
**Cons:** Cannot easily rebalance

### Strategy 3: Directory-Based Sharding

```rust
// Directory service maintains mapping
struct Directory {
    tenant_to_shard: HashMap<TenantId, ShardId>,
}
```

**Pros:** Flexible, rebalance by updating directory
**Cons:** Directory is a bottleneck (cache aggressively)

Kimberlite uses **Strategy 3** (directory-based) for flexibility.

## Multi-Region Deployment

For global applications, deploy VSR groups per region:

```
┌─────────────────────────────────────────────────────────────┐
│ Global Kimberlite Deployment                                 │
│                                                              │
│  ┌──────────────────┐  ┌──────────────────┐                 │
│  │   US-East Region │  │   EU-West Region │                 │
│  │                  │  │                  │                 │
│  │  VSR Group 0     │  │  VSR Group 1     │                 │
│  │  Tenants 1-1000  │  │  Tenants 1001+   │                 │
│  └──────────────────┘  └──────────────────┘                 │
│                                                              │
│  ┌────────────────────────────────────────┐                 │
│  │ Directory Service (global)             │                 │
│  │ Tenant → Region mapping                │                 │
│  └────────────────────────────────────────┘                 │
└─────────────────────────────────────────────────────────────┘
```

**Routing:**

1. Client connects to nearest region
2. Directory service routes to correct VSR group
3. If cross-region, proxy request (with latency penalty)

## Compliance Benefits

Multi-tenancy architecture supports compliance:

| Requirement | How Kimberlite Provides It |
|-------------|----------------------------|
| **Data isolation** | Physical log separation + cryptographic keys |
| **Data sovereignty** | Regional placement enforcement |
| **Audit trails** | Per-tenant logs with cross-tenant access tracking |
| **Right to erasure (GDPR)** | Delete tenant KEK, data becomes unrecoverable |
| **Data portability** | Export tenant log in standard format |
| **Access controls** | Per-tenant authentication + authorization |

## Performance Characteristics

**Overhead of multi-tenancy:** Minimal (< 1%)

- Log writes: Same append-only file per tenant
- Query: Same B+tree lookup (with tenant prefix)
- Encryption: AES-GCM hardware-accelerated (Intel AES-NI)

**Scalability:**

- **Vertical:** Tenants on same node share resources
- **Horizontal:** Shard tenants across nodes
- **Typical:** 1000-10,000 tenants per node
- **Limit:** Directory size (O(tenants) memory)

## Example: Healthcare SaaS

```rust
// Hospital A creates patient record
let hospital_a = TenantId::new(1);
db.insert(hospital_a, Patient {
    id: 123,
    name: "Alice Smith",
    dob: "1985-03-15",
})?;

// Hospital B creates patient record (same ID, different tenant)
let hospital_b = TenantId::new(2);
db.insert(hospital_b, Patient {
    id: 123,  // Same ID, different patient
    name: "Bob Johnson",
    dob: "1990-07-22",
})?;

// Hospital A queries patient 123
let patient = db.query(hospital_a, "SELECT * FROM patients WHERE id = 123")?;
// Returns Alice Smith (Hospital A's patient)

// Hospital B queries patient 123
let patient = db.query(hospital_b, "SELECT * FROM patients WHERE id = 123")?;
// Returns Bob Johnson (Hospital B's patient)
// Cannot see Hospital A's data
```

## Related Documentation

- **[Compliance](compliance.md)** - How multi-tenancy supports compliance
- **[Data Sharing Design](/docs/internals/design/data-sharing)** - Cross-tenant access controls
- **[Cryptography](/docs/internals/architecture/crypto)** - Encryption implementation details
- **[Data Model](data-model.md)** - How tenants map to logs and streams

---

**Key Takeaway:** Multi-tenancy in Kimberlite is structural, cryptographic, and compliance-native. It's not a tenant_id column—it's physical isolation enforced by the architecture.
