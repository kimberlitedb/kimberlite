/**
 * Error types for Kimberlite TypeScript SDK.
 */

/**
 * Base error class for all Kimberlite errors.
 */
export class KimberliteError extends Error {
  constructor(
    message: string,
    public readonly code?: number
  ) {
    super(message);
    this.name = 'KimberliteError';
    Object.setPrototypeOf(this, KimberliteError.prototype);
  }
}

/**
 * Failed to connect to Kimberlite server.
 */
export class ConnectionError extends KimberliteError {
  constructor(message: string, code?: number) {
    super(message, code);
    this.name = 'ConnectionError';
    Object.setPrototypeOf(this, ConnectionError.prototype);
  }
}

/**
 * Stream ID does not exist.
 */
export class StreamNotFoundError extends KimberliteError {
  constructor(message: string, code?: number) {
    super(message, code);
    this.name = 'StreamNotFoundError';
    Object.setPrototypeOf(this, StreamNotFoundError.prototype);
  }
}

/**
 * Operation not permitted for this tenant.
 */
export class PermissionDeniedError extends KimberliteError {
  constructor(message: string, code?: number) {
    super(message, code);
    this.name = 'PermissionDeniedError';
    Object.setPrototypeOf(this, PermissionDeniedError.prototype);
  }
}

/**
 * Authentication failed.
 */
export class AuthenticationError extends KimberliteError {
  constructor(message: string, code?: number) {
    super(message, code);
    this.name = 'AuthenticationError';
    Object.setPrototypeOf(this, AuthenticationError.prototype);
  }
}

/**
 * Operation timed out.
 */
export class TimeoutError extends KimberliteError {
  constructor(message: string, code?: number) {
    super(message, code);
    this.name = 'TimeoutError';
    Object.setPrototypeOf(this, TimeoutError.prototype);
  }
}

/**
 * Internal server error.
 */
export class InternalError extends KimberliteError {
  constructor(message: string, code?: number) {
    super(message, code);
    this.name = 'InternalError';
    Object.setPrototypeOf(this, InternalError.prototype);
  }
}

/**
 * No cluster replicas available.
 */
export class ClusterUnavailableError extends KimberliteError {
  constructor(message: string, code?: number) {
    super(message, code);
    this.name = 'ClusterUnavailableError';
    Object.setPrototypeOf(this, ClusterUnavailableError.prototype);
  }
}

/**
 * Error code to exception class mapping.
 */
const ERROR_MAP: Record<number, typeof KimberliteError> = {
  1: KimberliteError, // NULL pointer
  2: KimberliteError, // Invalid UTF-8
  3: ConnectionError,
  4: StreamNotFoundError,
  5: PermissionDeniedError,
  6: KimberliteError, // Invalid data class
  7: KimberliteError, // Offset out of range
  8: KimberliteError, // Query syntax
  9: KimberliteError, // Query execution
  10: KimberliteError, // Tenant not found
  11: AuthenticationError,
  12: TimeoutError,
  13: InternalError,
  14: ClusterUnavailableError,
  15: KimberliteError, // Unknown
};

/**
 * Throw appropriate exception for FFI error code.
 *
 * @param code - FFI error code (0 = success)
 * @param message - Error message
 * @throws {KimberliteError} Appropriate exception for error code
 */
export function throwForErrorCode(code: number, message: string): never {
  const ErrorClass = ERROR_MAP[code] || KimberliteError;
  throw new ErrorClass(message, code);
}
