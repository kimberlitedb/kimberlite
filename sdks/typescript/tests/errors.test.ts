/**
 * Tests for the typed-error dispatch layer. These are unit tests that fabricate
 * errors in the `[KMB_ERR_<code>]` prefix form the native addon emits, so no
 * running server is required.
 */

import {
  wrapNativeError,
  KimberliteError,
  ConnectionError,
  StreamNotFoundError,
  AuthenticationError,
  TimeoutError,
  InternalError,
  OffsetMismatchError,
  RateLimitedError,
  NotLeaderError,
  ServerError,
} from '../src/errors';

function fakeNativeError(tag: string, message: string): Error {
  return new Error(`[KMB_ERR_${tag}] ${message}`);
}

describe('wrapNativeError — tag-based dispatch', () => {
  it.each([
    ['Connection', ConnectionError, 'Connection'],
    ['NotConnected', ConnectionError, 'NotConnected'],
    ['Timeout', TimeoutError, 'Timeout'],
    ['HandshakeFailed', AuthenticationError, 'HandshakeFailed'],
    ['AuthenticationFailed', AuthenticationError, 'AuthenticationFailed'],
    ['StreamNotFound', StreamNotFoundError, 'StreamNotFound'],
    ['OffsetMismatch', OffsetMismatchError, 'OffsetMismatch'],
    ['RateLimited', RateLimitedError, 'RateLimited'],
    ['NotLeader', NotLeaderError, 'NotLeader'],
    ['InternalError', InternalError, 'InternalError'],
  ])('maps %s to the right subclass with code=%s', (tag, Ctor, expectedCode) => {
    const err = wrapNativeError(fakeNativeError(tag, 'boom'));
    expect(err).toBeInstanceOf(Ctor);
    expect(err).toBeInstanceOf(KimberliteError);
    expect(err.code).toBe(expectedCode);
    expect(err.message).toBe('boom');
  });

  it.each([
    'TenantNotFound',
    'TableNotFound',
    'StreamAlreadyExists',
    'InvalidOffset',
    'PositionAhead',
    'QueryParseError',
    'QueryExecutionError',
    'StorageError',
    'ProjectionLag',
    'InvalidRequest',
    'Wire',
    'ResponseMismatch',
    'UnexpectedResponse',
  ])('maps %s to ServerError preserving the code tag', (tag) => {
    const err = wrapNativeError(fakeNativeError(tag, 'details'));
    expect(err).toBeInstanceOf(ServerError);
    expect(err.code).toBe(tag);
  });

  it('reports isRetryable() for transient codes', () => {
    expect(wrapNativeError(fakeNativeError('Timeout', 'x')).isRetryable()).toBe(true);
    expect(
      wrapNativeError(fakeNativeError('RateLimited', 'x')).isRetryable(),
    ).toBe(true);
    expect(wrapNativeError(fakeNativeError('NotLeader', 'x')).isRetryable()).toBe(true);
    expect(
      wrapNativeError(fakeNativeError('ProjectionLag', 'x')).isRetryable(),
    ).toBe(true);
    expect(
      wrapNativeError(fakeNativeError('Connection', 'x')).isRetryable(),
    ).toBe(true);
  });

  it('reports isRetryable() false for permanent codes', () => {
    expect(
      wrapNativeError(fakeNativeError('AuthenticationFailed', 'x')).isRetryable(),
    ).toBe(false);
    expect(
      wrapNativeError(fakeNativeError('StreamNotFound', 'x')).isRetryable(),
    ).toBe(false);
    expect(
      wrapNativeError(fakeNativeError('QueryParseError', 'x')).isRetryable(),
    ).toBe(false);
  });

  it('preserves already-wrapped KimberliteError instances', () => {
    const original = new OffsetMismatchError('previous conflict');
    const wrapped = wrapNativeError(original);
    expect(wrapped).toBe(original);
    expect(wrapped.code).toBe('OffsetMismatch');
  });

  it('falls back to legacy string matching when no KMB_ERR_ prefix is present', () => {
    const err = wrapNativeError(new Error('connection timeout after 30s'));
    expect(err).toBeInstanceOf(TimeoutError);
  });

  it('handles non-Error throwables by stringifying', () => {
    const err = wrapNativeError('plain string failure');
    expect(err).toBeInstanceOf(KimberliteError);
    expect(err.message).toContain('plain string failure');
  });
});
