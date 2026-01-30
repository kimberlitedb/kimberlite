/**
 * Kimberlite TypeScript SDK.
 *
 * Promise-based client library for Kimberlite database with full TypeScript support.
 *
 * @packageDocumentation
 */

export { Client } from './client';
export { DataClass, StreamId, Offset, TenantId, Event, ClientConfig } from './types';
export {
  KimberliteError,
  ConnectionError,
  StreamNotFoundError,
  PermissionDeniedError,
  AuthenticationError,
  TimeoutError,
  InternalError,
  ClusterUnavailableError,
} from './errors';
