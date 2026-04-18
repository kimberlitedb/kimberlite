/**
 * Error types for the Kimberlite TypeScript SDK.
 *
 * The native addon tags every client error with a `[KMB_ERR_<code>]` prefix
 * (see `crates/kimberlite-node/src/lib.rs::client_error_to_napi`). We parse
 * that prefix to dispatch to a specific subclass and expose the wire
 * `code` field for callers that want to `switch` on it without depending on
 * `instanceof`.
 */

/** Wire error-code tags emitted by the native addon. */
export type ErrorCode =
  | 'Unknown'
  | 'InternalError'
  | 'InvalidRequest'
  | 'AuthenticationFailed'
  | 'TenantNotFound'
  | 'StreamNotFound'
  | 'TableNotFound'
  | 'QueryParseError'
  | 'QueryExecutionError'
  | 'PositionAhead'
  | 'StreamAlreadyExists'
  | 'InvalidOffset'
  | 'StorageError'
  | 'ProjectionLag'
  | 'RateLimited'
  | 'NotLeader'
  | 'OffsetMismatch'
  | 'SubscriptionNotFound'
  | 'SubscriptionClosed'
  | 'SubscriptionBackpressure'
  | 'ApiKeyNotFound'
  | 'TenantAlreadyExists'
  // Client-side synthetic codes (no wire counterpart):
  | 'Connection'
  | 'Timeout'
  | 'NotConnected'
  | 'HandshakeFailed'
  | 'Wire'
  | 'ResponseMismatch'
  | 'UnexpectedResponse';

const RETRYABLE_CODES = new Set<ErrorCode>([
  'Connection',
  'Timeout',
  'RateLimited',
  'NotLeader',
  'ProjectionLag',
]);

export class KimberliteError extends Error {
  /** Wire error-code tag extracted from the native error prefix. */
  readonly code: ErrorCode;

  constructor(message: string, code: ErrorCode = 'Unknown') {
    super(message);
    this.name = 'KimberliteError';
    this.code = code;
    Object.setPrototypeOf(this, KimberliteError.prototype);
  }

  /** True if the error is likely to succeed on retry (transient failure). */
  isRetryable(): boolean {
    return RETRYABLE_CODES.has(this.code);
  }
}

export class ConnectionError extends KimberliteError {
  constructor(message: string, code: ErrorCode = 'Connection') {
    super(message, code);
    this.name = 'ConnectionError';
    Object.setPrototypeOf(this, ConnectionError.prototype);
  }
}

export class StreamNotFoundError extends KimberliteError {
  constructor(message: string) {
    super(message, 'StreamNotFound');
    this.name = 'StreamNotFoundError';
    Object.setPrototypeOf(this, StreamNotFoundError.prototype);
  }
}

export class PermissionDeniedError extends KimberliteError {
  constructor(message: string, code: ErrorCode = 'AuthenticationFailed') {
    super(message, code);
    this.name = 'PermissionDeniedError';
    Object.setPrototypeOf(this, PermissionDeniedError.prototype);
  }
}

export class AuthenticationError extends KimberliteError {
  constructor(message: string, code: ErrorCode = 'AuthenticationFailed') {
    super(message, code);
    this.name = 'AuthenticationError';
    Object.setPrototypeOf(this, AuthenticationError.prototype);
  }
}

export class TimeoutError extends KimberliteError {
  constructor(message: string) {
    super(message, 'Timeout');
    this.name = 'TimeoutError';
    Object.setPrototypeOf(this, TimeoutError.prototype);
  }
}

export class InternalError extends KimberliteError {
  constructor(message: string, code: ErrorCode = 'InternalError') {
    super(message, code);
    this.name = 'InternalError';
    Object.setPrototypeOf(this, InternalError.prototype);
  }
}

export class ClusterUnavailableError extends KimberliteError {
  constructor(message: string) {
    super(message, 'InternalError');
    this.name = 'ClusterUnavailableError';
    Object.setPrototypeOf(this, ClusterUnavailableError.prototype);
  }
}

/**
 * Optimistic-concurrency conflict on append — the expected offset did not
 * match the stream's current offset. Re-read the offset and retry.
 */
export class OffsetMismatchError extends KimberliteError {
  constructor(message: string) {
    super(message, 'OffsetMismatch');
    this.name = 'OffsetMismatchError';
    Object.setPrototypeOf(this, OffsetMismatchError.prototype);
  }
}

/** The server rejected the request because rate limits were exceeded. */
export class RateLimitedError extends KimberliteError {
  constructor(message: string) {
    super(message, 'RateLimited');
    this.name = 'RateLimitedError';
    Object.setPrototypeOf(this, RateLimitedError.prototype);
  }
}

/**
 * The request was sent to a follower replica. The error message may include
 * a leader hint (see the `NotLeader` error mapping in the server).
 */
export class NotLeaderError extends KimberliteError {
  constructor(message: string) {
    super(message, 'NotLeader');
    this.name = 'NotLeaderError';
    Object.setPrototypeOf(this, NotLeaderError.prototype);
  }
}

/**
 * A generic server-side error that doesn't map to a more specific subclass.
 * The `code` property exposes the wire error-code tag for inspection.
 */
export class ServerError extends KimberliteError {
  constructor(message: string, code: ErrorCode) {
    super(message, code);
    this.name = 'ServerError';
    Object.setPrototypeOf(this, ServerError.prototype);
  }
}

const ERROR_PREFIX_RE = /^\[KMB_ERR_([A-Za-z]+)\]\s*(.*)$/s;

/**
 * Wrap an error thrown by the native addon in the appropriate subclass.
 *
 * If `err` is already a KimberliteError, returns it unchanged. Otherwise
 * parses the `[KMB_ERR_<code>]` prefix emitted by the native layer and
 * dispatches to a typed subclass.
 */
export function wrapNativeError(err: unknown): KimberliteError {
  if (err instanceof KimberliteError) return err;
  const raw = err instanceof Error ? err.message : String(err);
  const match = raw.match(ERROR_PREFIX_RE);

  if (match) {
    const [, codeTag, inner] = match;
    return constructTypedError(codeTag as ErrorCode, inner);
  }

  // Fallback: legacy string-matching for native builds that pre-date the
  // KMB_ERR_ prefix. Covers the same heuristics as the 0.4.x wrapper.
  return legacyWrap(raw);
}

function constructTypedError(code: ErrorCode, message: string): KimberliteError {
  switch (code) {
    case 'Connection':
    case 'NotConnected':
      return new ConnectionError(message, code);
    case 'Timeout':
      return new TimeoutError(message);
    case 'HandshakeFailed':
    case 'AuthenticationFailed':
      return new AuthenticationError(message, code);
    case 'StreamNotFound':
      return new StreamNotFoundError(message);
    case 'OffsetMismatch':
      return new OffsetMismatchError(message);
    case 'RateLimited':
      return new RateLimitedError(message);
    case 'NotLeader':
      return new NotLeaderError(message);
    case 'TenantNotFound':
    case 'TableNotFound':
    case 'StreamAlreadyExists':
    case 'InvalidOffset':
    case 'PositionAhead':
    case 'QueryParseError':
    case 'QueryExecutionError':
    case 'StorageError':
    case 'ProjectionLag':
    case 'InvalidRequest':
    case 'Wire':
    case 'ResponseMismatch':
    case 'UnexpectedResponse':
    case 'SubscriptionNotFound':
    case 'SubscriptionClosed':
    case 'SubscriptionBackpressure':
    case 'ApiKeyNotFound':
    case 'TenantAlreadyExists':
      return new ServerError(message, code);
    case 'InternalError':
    case 'Unknown':
    default:
      return new InternalError(message, code);
  }
}

function legacyWrap(msg: string): KimberliteError {
  if (msg.includes('connection timeout') || msg.includes('timed out')) {
    return new TimeoutError(msg);
  }
  if (msg.includes('handshake failed')) {
    return new AuthenticationError(msg);
  }
  if (msg.includes('connection error') || msg.includes('not connected')) {
    return new ConnectionError(msg);
  }
  if (msg.includes('server error')) {
    if (msg.includes('StreamNotFound') || msg.includes('stream not found')) {
      return new StreamNotFoundError(msg);
    }
    if (msg.includes('PermissionDenied') || msg.includes('permission')) {
      return new PermissionDeniedError(msg);
    }
    if (msg.includes('AuthFailed') || msg.includes('Unauthorized')) {
      return new AuthenticationError(msg);
    }
    if (msg.includes('ClusterUnavailable') || msg.includes('cluster unavailable')) {
      return new ClusterUnavailableError(msg);
    }
    return new InternalError(msg);
  }
  if (msg.includes('wire protocol error') || msg.includes('response ID')) {
    return new InternalError(msg);
  }
  return new KimberliteError(msg);
}
