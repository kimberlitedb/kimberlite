/**
 * Tests for `mapKimberliteError` and `asResult`.
 *
 * AUDIT-2026-04 S2.4 — exercises the wire-error → domain-error
 * translation for every error shape a compliance-vertical app
 * needs to distinguish.
 */

import {
  mapKimberliteError,
  asResult,
  DomainError,
} from '../src/domain-error';
import {
  KimberliteError,
  StreamNotFoundError,
  AuthenticationError,
  RateLimitedError,
  TimeoutError,
  OffsetMismatchError,
} from '../src/errors';

describe('mapKimberliteError', () => {
  it.each<[string, KimberliteError, DomainError['kind']]>([
    ['StreamNotFound', new StreamNotFoundError('no such stream'), 'NotFound'],
    [
      'TableNotFound',
      new KimberliteError('no such table', 'TableNotFound'),
      'NotFound',
    ],
    [
      'TenantNotFound',
      new KimberliteError('no such tenant', 'TenantNotFound'),
      'NotFound',
    ],
    [
      'AuthenticationFailed',
      new AuthenticationError('bad token'),
      'Forbidden',
    ],
    ['RateLimited', new RateLimitedError('slow down'), 'RateLimited'],
    ['Timeout', new TimeoutError('server did not reply'), 'Timeout'],
    [
      'OffsetMismatch',
      new OffsetMismatchError('expected 10, got 11'),
      'ConcurrentModification',
    ],
    [
      'QueryParseError',
      new KimberliteError('bad SQL', 'QueryParseError'),
      'Validation',
    ],
    [
      'InvalidRequest',
      new KimberliteError('malformed', 'InvalidRequest'),
      'Validation',
    ],
    [
      'StreamAlreadyExists',
      new KimberliteError('duplicate', 'StreamAlreadyExists'),
      'Conflict',
    ],
    [
      'TenantAlreadyExists',
      new KimberliteError('id reused', 'TenantAlreadyExists'),
      'Conflict',
    ],
    [
      'InternalError',
      new KimberliteError('boom', 'InternalError'),
      'Unavailable',
    ],
  ])('maps %s to %s', (_tag, err, expectedKind) => {
    const mapped = mapKimberliteError(err);
    expect(mapped.kind).toBe(expectedKind);
  });

  it('Validation carries the original message', () => {
    const err = new KimberliteError('unexpected token at position 4', 'QueryParseError');
    const mapped = mapKimberliteError(err);
    expect(mapped).toEqual({
      kind: 'Validation',
      message: 'unexpected token at position 4',
    });
  });

  it('Conflict carries the reason in a list', () => {
    const err = new KimberliteError('stream foo exists', 'StreamAlreadyExists');
    const mapped = mapKimberliteError(err);
    expect(mapped).toEqual({
      kind: 'Conflict',
      reasons: ['stream foo exists'],
    });
  });

  it('falls through non-Kimberlite Error to Unavailable', () => {
    const mapped = mapKimberliteError(new Error('random TypeError'));
    expect(mapped).toEqual({
      kind: 'Unavailable',
      message: 'random TypeError',
    });
  });

  it('stringifies primitive thrown values', () => {
    expect(mapKimberliteError('string error')).toEqual({
      kind: 'Unavailable',
      message: 'string error',
    });
    expect(mapKimberliteError(42)).toEqual({
      kind: 'Unavailable',
      message: '42',
    });
  });
});

describe('asResult', () => {
  it('wraps a successful promise in { ok: true, value }', async () => {
    const result = await asResult(async () => 'value');
    expect(result).toEqual({ ok: true, value: 'value' });
  });

  it('wraps a thrown Kimberlite error in { ok: false, err: DomainError }', async () => {
    const result = await asResult(async () => {
      throw new StreamNotFoundError('gone');
    });
    // Implementation emits BOTH `err` and `error` (alias); see
    // domain-error.ts:135. `objectContaining` tolerates the alias so
    // the canonical `err` key remains the test contract.
    expect(result).toEqual(
      expect.objectContaining({ ok: false, err: { kind: 'NotFound' } }),
    );
  });

  it('wraps non-Kimberlite errors in Unavailable', async () => {
    const result = await asResult(async () => {
      throw new Error('boom');
    });
    expect(result).toEqual(
      expect.objectContaining({
        ok: false,
        err: { kind: 'Unavailable', message: 'boom' },
      }),
    );
  });
});
