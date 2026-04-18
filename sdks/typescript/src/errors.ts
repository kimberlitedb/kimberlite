/**
 * Error types for the Kimberlite TypeScript SDK.
 *
 * Native-addon errors surface as plain `Error` objects whose message contains
 * the structured message from `ClientError::fmt`. We inspect the prefix to
 * map back to a specific subclass so callers can `instanceof` their way to
 * targeted handling.
 */

export class KimberliteError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'KimberliteError';
    Object.setPrototypeOf(this, KimberliteError.prototype);
  }
}

export class ConnectionError extends KimberliteError {
  constructor(message: string) {
    super(message);
    this.name = 'ConnectionError';
    Object.setPrototypeOf(this, ConnectionError.prototype);
  }
}

export class StreamNotFoundError extends KimberliteError {
  constructor(message: string) {
    super(message);
    this.name = 'StreamNotFoundError';
    Object.setPrototypeOf(this, StreamNotFoundError.prototype);
  }
}

export class PermissionDeniedError extends KimberliteError {
  constructor(message: string) {
    super(message);
    this.name = 'PermissionDeniedError';
    Object.setPrototypeOf(this, PermissionDeniedError.prototype);
  }
}

export class AuthenticationError extends KimberliteError {
  constructor(message: string) {
    super(message);
    this.name = 'AuthenticationError';
    Object.setPrototypeOf(this, AuthenticationError.prototype);
  }
}

export class TimeoutError extends KimberliteError {
  constructor(message: string) {
    super(message);
    this.name = 'TimeoutError';
    Object.setPrototypeOf(this, TimeoutError.prototype);
  }
}

export class InternalError extends KimberliteError {
  constructor(message: string) {
    super(message);
    this.name = 'InternalError';
    Object.setPrototypeOf(this, InternalError.prototype);
  }
}

export class ClusterUnavailableError extends KimberliteError {
  constructor(message: string) {
    super(message);
    this.name = 'ClusterUnavailableError';
    Object.setPrototypeOf(this, ClusterUnavailableError.prototype);
  }
}

/**
 * Wrap an error thrown by the native addon in the appropriate subclass.
 * If `err` is already a KimberliteError, returns it unchanged.
 *
 * napi-rs renders errors as `"Error: <message>"`, where `<message>` is
 * `ClientError::Display` from crates/kimberlite-client/src/error.rs, so we
 * match via `includes` rather than `startsWith`.
 */
export function wrapNativeError(err: unknown): KimberliteError {
  if (err instanceof KimberliteError) return err;
  const msg = err instanceof Error ? err.message : String(err);

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
