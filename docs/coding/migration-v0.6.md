# Migrating from v0.5.1 to v0.6.0

v0.6.0 rounds out the SQL + SDK + compliance surface: upsert,
correlated subqueries, `AS OF TIMESTAMP`, masking policies, audit
query, consent basis, `eraseSubject` auto-discovery, and wire
protocol v4. The migration is small — the breaking-change surface is
two items.

## TL;DR

1. **Upgrade client and server in lockstep.** Wire protocol bumped v3
   → v4. v3 ↔ v4 is rejected at frame-header decode; there is no
   compat shim.
2. **Bump SDK pins:**
   - TypeScript: `@kimberlitedb/client@0.6.0`
   - Python: `kimberlite==0.6.0`
   - Rust: `kimberlite-client = "0.6"` (workspace-pin all
     `kimberlite-*` deps together).
3. **Rename any duplicate ABAC rule names within a single policy.**
   `Policy::add_rule` now rejects duplicates.
4. **Delete your workarounds.** If you have custom upsert helpers
   (`upsertRow`, "UPDATE then INSERT"), fake timestamp resolvers, or
   hand-rolled audit query code, replace them with the kernel surface
   below.

## Breaking changes

### 1. Wire protocol v3 → v4

`PROTOCOL_VERSION` is now `4`. `ConsentGrantRequest` and `ConsentRecord`
gained `basis: Option<ConsentBasis>` carrying the GDPR article
justification (`Consent`, `Contract`, `LegalObligation`, `VitalInterests`,
`PublicTask`, `LegitimateInterests`).

- v3 clients against a v4 server: `"unsupported protocol version: 3,
  server speaks 4"`.
- v4 clients against a v3 server: mirror.
- No shim. Upgrade both sides together.

The 4-cell compat matrix (v3↔v3, v3→v4, v4→v3, v4↔v4) is tested in
`crates/kimberlite-wire/src/tests.rs::v3_v4_compat`.

### 2. ABAC rule-name uniqueness

Previously `Policy::add_rule` would silently accept duplicate rule names.
v0.6.0 rejects duplicates with `Policy::RuleNameAlreadyExists`.

**Migration:**

```rust
// Before (v0.5.1): duplicate names silently accepted, audit log ambiguous
policy.add_rule(Rule::new("can_read", ...));
policy.add_rule(Rule::new("can_read", ...));  // shadowed, no error

// After (v0.6.0): rename the second rule
policy.add_rule(Rule::new("can_read_self", ...));
policy.add_rule(Rule::new("can_read_team", ...));
```

Audit-log rule references by name; stable identity is the invariant.

## New surface — what to delete from your app

### Upsert (replaces `upsertRow` helpers)

```sql
INSERT INTO patients (id, name, dob)
VALUES ($1, $2, $3)
ON CONFLICT (id) DO UPDATE
  SET name = EXCLUDED.name, dob = EXCLUDED.dob
RETURNING id, name;

-- or skip the update
INSERT INTO patients (id, ...) VALUES (...) ON CONFLICT (id) DO NOTHING;
```

One atomic `UpsertApplied` event with `resolution: Inserted | Updated |
NoOp`. Delete your two-round-trip `upsertRow` helpers. (TS
`client.upsertRow` is `@deprecated` pointing at ON CONFLICT.)

### Correlated subqueries

```sql
SELECT p.* FROM patients p
WHERE EXISTS (
  SELECT 1 FROM consents c
  WHERE c.subject_id = p.id
    AND c.purpose = 'HealthcareDelivery'
    AND c.withdrawn_at IS NULL
);
```

`EXISTS`, `NOT EXISTS`, `IN (SELECT)`, `NOT IN (SELECT)` with outer
column refs. Decorrelation to semi-join when provable; correlated-loop
fallback. Cardinality guard (`max_correlated_row_evaluations`, default
10M) rejects pathological shapes with `CorrelatedCardinalityExceeded`.

### `AS OF TIMESTAMP`

All three SDKs' `queryAt` / `query_at` / `AsOf` now accept a polymorphic
time reference:

```typescript
await client.queryAt('SELECT * FROM patients WHERE id = $1', [123], 4200n);
await client.queryAt('SELECT * FROM patients WHERE id = $1', [123], new Date('2026-01-15'));
await client.queryAt('SELECT * FROM patients WHERE id = $1', [123], '2026-01-15T00:00:00Z');
```

Timestamp resolves to the largest offset with commit ts ≤ target.
Errors honestly with `AsOfBeforeRetentionHorizon` if the target
predates retained state.

### Masking policies

```sql
CREATE MASKING POLICY ssn_redact
  WITH (strategy = REDACT SSN)
  EXEMPT ROLE ('clinician', 'auditor');

ALTER TABLE patients
  ALTER COLUMN ssn SET MASKING POLICY ssn_redact;

DROP MASKING POLICY ssn_redact;  -- only allowed after all attachments detached
```

Read-time mask application — writes store the real value, reads from
non-exempt roles see the masked form. Composes with RBAC and break-glass.

### Audit query SDK

```typescript
const entries = await client.compliance.audit.query({
  subjectId: 'patient-123',
  fromTs: '2026-01-01T00:00:00Z',
  toTs: '2026-01-31T23:59:59Z',
  actor: 'clinician-456',
  limit: 100,
});
// Response is value-stripped (only changed-field names, not values).
```

### Consent basis

```typescript
await client.compliance.consent.grant({
  subjectId: 'patient-123',
  purpose: 'HealthcareDelivery',
  basis: { article: 'Consent', evidence: '<signed-form-id>' },
});
```

### Erase subject (auto-discovery)

```typescript
// v0.5.1: caller had to list every stream explicitly
await client.compliance.eraseSubject({ subjectId, streams: [...] });

// v0.6.0: kernel walks the PHI/PII/Sensitive catalog for you
await client.compliance.eraseSubject({ subjectId });
```

Cryptographic shredding (DEK shred) — preserves the immutable-log
principle. Idempotent; a second call returns the existing receipt plus
a noop audit entry.

### `StorageBackend::Memory` for tests

```typescript
const client = await createClient({
  testing: { backend: 'memory' },
});
```

17.7× faster than TempDir on Apple M-series in release. Delete your
`fake-kimberlite.ts` and `sql-parser.ts` regex shims.

## Checklist

- [ ] All kimberlite SDK deps at 0.6.0
- [ ] Server rolled to 0.6.0
- [ ] ABAC rule-name collisions resolved
- [ ] Custom upsert helpers replaced with ON CONFLICT
- [ ] Fake timestamp resolvers deleted
- [ ] Custom audit query code replaced with `compliance.audit.query`
- [ ] `eraseSubject` callers simplified to auto-discovery form
- [ ] In-memory test harness adopted where appropriate

## Where to get help

- CHANGELOG — `CHANGELOG.md` § [0.6.0] for the full surface
- SQL reference — `docs/reference/sql/queries.md`
- SDK parity — `docs/reference/sdk/parity.md`
- Previous migration — `docs/coding/migration-v0.5.md` (v0.4 → v0.5)
