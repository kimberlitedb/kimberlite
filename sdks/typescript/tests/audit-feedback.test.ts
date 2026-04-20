/**
 * AUDIT-2026-04 S4.* — unit tests covering the notebar-feedback
 * fixes that don't require a live server. End-to-end wire-level
 * audit-context propagation is exercised by the Rust-side
 * `e2e_healthcare` integration test.
 */

import {
  StreamId,
  ValueBuilder,
  isBigInt,
  isText,
  isNull,
  valueToDate,
  valueEquals,
} from '../src';
import { asResult, mapKimberliteError } from '../src/domain-error';
import { AuditContext, runWithAudit, currentAudit } from '../src/audit-context';
import {
  ErasurePending,
  ErasureInProgress,
  ErasureRecording,
} from '../src/compliance';

describe('Result — .error canonical field (AUDIT-2026-04 S4.1)', () => {
  it('populates both .error and the deprecated .err alias', async () => {
    const result = await asResult(async () => {
      throw new Error('boom');
    });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toBeDefined();
      expect(result.err).toBeDefined();
      // Both aliases point at the same DomainError until 0.6.0.
      expect(result.error).toBe(result.err);
    }
  });

  it('maps unknown errors to Unavailable', () => {
    const d = mapKimberliteError(new Error('lost contact'));
    expect(d.kind).toBe('Unavailable');
    if (d.kind === 'Unavailable') {
      expect(d.message).toContain('lost contact');
    }
  });
});

describe('StreamId — branded nominal type (AUDIT-2026-04 S4.6)', () => {
  it('mints and recovers round-trips', () => {
    const id = StreamId.from(42n);
    expect(StreamId.raw(id)).toBe(42n);
  });

  it('StreamId.from(number) widens to bigint', () => {
    const id = StreamId.from(7);
    expect(StreamId.raw(id)).toBe(7n);
  });
});

describe('Value — kind-tagged string literals (AUDIT-2026-04 S4.5)', () => {
  it('ValueBuilder.bigint uses kind: bigint', () => {
    const v = ValueBuilder.bigint(42n);
    expect(v.kind).toBe('bigint');
    expect(isBigInt(v)).toBe(true);
    expect(isText(v)).toBe(false);
  });

  it('ValueBuilder.null uses kind: null', () => {
    const v = ValueBuilder.null();
    expect(v.kind).toBe('null');
    expect(isNull(v)).toBe(true);
  });

  it('ValueBuilder.text round-trips through the kind guard', () => {
    const v = ValueBuilder.text('hello');
    expect(v.kind).toBe('text');
    expect(isText(v)).toBe(true);
  });

  it('valueToDate works on kind=timestamp values', () => {
    const nanos = 1_700_000_000_000_000_000n;
    const v = ValueBuilder.timestamp(nanos);
    const date = valueToDate(v);
    expect(date).not.toBeNull();
    expect(date?.getTime()).toBe(Number(nanos / 1_000_000n));
  });

  it('valueEquals compares across the same kind', () => {
    expect(valueEquals(ValueBuilder.bigint(42n), ValueBuilder.bigint(42n))).toBe(true);
    expect(valueEquals(ValueBuilder.bigint(42n), ValueBuilder.bigint(43n))).toBe(false);
    expect(valueEquals(ValueBuilder.text('a'), ValueBuilder.bigint(42n))).toBe(false);
    expect(valueEquals(ValueBuilder.null(), ValueBuilder.null())).toBe(true);
  });
});

describe('AuditContext — in-process propagation (AUDIT-2026-04 S3.9)', () => {
  it('runWithAudit exposes actor/reason to the inner scope', () => {
    const ctx: AuditContext = {
      actor: 'dr.smith@example.com',
      reason: 'patient-chart-view',
    };
    const observed = runWithAudit(ctx, () => currentAudit());
    expect(observed?.actor).toBe('dr.smith@example.com');
    expect(observed?.reason).toBe('patient-chart-view');
  });

  it('clears context after the scope returns', () => {
    runWithAudit({ actor: 'a', reason: 'b' }, () => undefined);
    expect(currentAudit()).toBeUndefined();
  });
});

describe('Erasure typed tokens compile-time safety (AUDIT-2026-04 S4.3)', () => {
  // Smoke-test: the tokens are plain objects with branded state tags.
  // Full end-to-end exercised in the Rust + Python parity suites.
  it('token shapes carry their state tag', () => {
    const pending = { requestId: 'r-1', __state: 'Pending' as const } as ErasurePending;
    const inProgress = { requestId: 'r-1', __state: 'InProgress' as const } as ErasureInProgress;
    const recording = { requestId: 'r-1', __state: 'Recording' as const } as ErasureRecording;
    expect(pending.__state).toBe('Pending');
    expect(inProgress.__state).toBe('InProgress');
    expect(recording.__state).toBe('Recording');
  });
});
