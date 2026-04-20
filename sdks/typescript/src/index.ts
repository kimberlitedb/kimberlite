/**
 * Kimberlite TypeScript SDK.
 *
 * Promise-based client library for Kimberlite backed by a Rust N-API native
 * addon. Supported Node versions: 18, 20, 22, 24 (N-API v8).
 *
 * @packageDocumentation
 */

export { Client, ExecuteResult, RowMapper } from './client';
export { Pool, PooledClient, PoolConfig, PoolStats } from './pool';
export {
  Subscription,
  SubscriptionEvent,
  SubscribeOptions,
  SubscriptionCloseReason,
} from './subscription';
export {
  AdminNamespace,
  TableInfo,
  ColumnInfo,
  IndexInfo,
  DescribeTable,
  TenantInfo,
  TenantCreateResult,
  TenantDeleteResult,
  ApiKeyInfo,
  ApiKeyRegisterResult,
  ApiKeyRotateResult,
  ServerInfo,
  IssueApiKeyOptions,
} from './admin';
export {
  ComplianceNamespace,
  ConsentPurpose,
  ConsentScope,
  ConsentRecord,
  ConsentGrantResult,
  ErasureExemptionBasis,
  ErasureStatus,
  ErasureRequest,
  ErasureAuditRecord,
} from './compliance';
export { Query } from './query-builder';
export { withRetry, DEFAULT_RETRY, RetryPolicy } from './retry';
export {
  DomainError,
  Result,
  mapKimberliteError,
  asResult,
} from './domain-error';
export {
  AuditContext,
  runWithAudit,
  currentAudit,
  requireAudit,
} from './audit-context';
export {
  TenantPool,
  TenantPoolConfig,
  TenantPoolStats,
} from './tenant-pool';
export {
  DataClass,
  Placement,
  StreamId,
  Offset,
  TenantId,
  Event,
  ClientConfig,
  QueryResult,
} from './types';
export {
  Value,
  ValueType,
  ValueBuilder,
  valueToDate,
  valueToString,
  valueEquals,
  isNull,
  isBigInt,
  isText,
  isBoolean,
  isTimestamp,
} from './value';
export {
  KimberliteError,
  ConnectionError,
  StreamNotFoundError,
  PermissionDeniedError,
  AuthenticationError,
  TimeoutError,
  InternalError,
  ClusterUnavailableError,
  OffsetMismatchError,
  RateLimitedError,
  NotLeaderError,
  ServerError,
  ErrorCode,
} from './errors';
