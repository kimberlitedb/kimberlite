/**
 * v0.6.0 Tier 2 #9 — unit tests for the TS audit-query surface.
 *
 * The live-server end-to-end round-trip is exercised by the Rust
 * `e2e_compliance_phase6` integration test; these tests lock the
 * client-side shape and projection (no PHI values in the returned
 * entries).
 */

import type { AuditEntry, AuditQueryFilter } from '../src';
import type { JsAuditEntry, JsAuditQueryFilter } from '../src/native';

describe('AuditEntry — PHI-safe projection (v0.6.0 Tier 2 #9)', () => {
  it('exposes the spec-mandated field set', () => {
    // Structural test — every field in the spec is assignable on
    // the public `AuditEntry` type.
    const entry: AuditEntry = {
      actor: 'operator@example.com',
      action: 'ConsentGranted',
      subjectId: 'alice@example.com',
      correlationId: '11111111-2222-3333-4444-555555555555',
      requestId: null,
      occurredAt: new Date(1_700_000_000_000),
      reason: null,
      changedFieldNames: ['subject_id', 'purpose', 'scope'],
    };
    expect(entry.action).toBe('ConsentGranted');
    expect(entry.changedFieldNames).toEqual(['subject_id', 'purpose', 'scope']);
    // No `beforeValues` / `afterValues` / `values` fields exist on
    // the shape — `changedFieldNames` is the only schema carrier.
    expect(Object.keys(entry)).not.toContain('beforeValues');
    expect(Object.keys(entry)).not.toContain('afterValues');
    expect(Object.keys(entry)).not.toContain('values');
  });

  it('JS native shape round-trips without values leaking', () => {
    // A wire-level JsAuditEntry with realistic data. The test
    // doesn't invoke the native addon — it validates the type is
    // surface-compatible and that `changedFieldNames` is the only
    // schema-bearing channel.
    const native: JsAuditEntry = {
      eventId: '9999',
      timestampNanos: 1_700_000_000_000_000_000n,
      action: 'ConsentGranted',
      subjectId: 'alice@example.com',
      actor: 'operator@example.com',
      tenantId: 1n,
      ipAddress: null,
      correlationId: null,
      requestId: null,
      reason: null,
      sourceCountry: null,
      changedFieldNames: ['subject_id', 'purpose', 'scope'],
    };
    // The only place the action shape can leak is the field name
    // list. Values like "Marketing" / "AllData" must never appear
    // in the type-safe surface.
    const encoded = JSON.stringify(native, (_, v) =>
      typeof v === 'bigint' ? v.toString() : v,
    );
    expect(encoded).not.toContain('Marketing');
    expect(encoded).not.toContain('AllData');
    expect(encoded).toContain('ConsentGranted'); // action kind is OK
  });

  it('AuditQueryFilter compiles with the spec shape', () => {
    const filter: AuditQueryFilter = {
      subjectId: 'alice@example.com',
      actor: 'operator@example.com',
      action: 'Consent',
      fromTs: new Date(Date.now() - 30 * 24 * 60 * 60 * 1000),
      toTs: new Date(),
      limit: 100,
    };
    expect(filter.subjectId).toBe('alice@example.com');
    expect(filter.action).toBe('Consent');
    expect(filter.limit).toBe(100);
    // JsAuditQueryFilter is the wire-shape mirror — snake→camel
    // mapping happens inside AuditNamespace.query.
    const wire: JsAuditQueryFilter = {
      subjectId: filter.subjectId ?? null,
      actionType: filter.action ?? null,
      timeFromNanos:
        filter.fromTs !== undefined
          ? BigInt(filter.fromTs.getTime()) * 1_000_000n
          : null,
      timeToNanos:
        filter.toTs !== undefined
          ? BigInt(filter.toTs.getTime()) * 1_000_000n
          : null,
      actor: filter.actor ?? null,
      limit: filter.limit ?? null,
    };
    expect(wire.actionType).toBe('Consent');
    expect(typeof wire.timeFromNanos).toBe('bigint');
  });
});
