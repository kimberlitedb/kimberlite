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
  MaskingPolicyNamespace,
  MaskingStrategy,
  CreateMaskingPolicyOptions,
  MaskingPolicyInfo,
  MaskingAttachmentInfo,
  MaskingPolicyListResult,
} from './admin';
export {
  ComplianceNamespace,
  ConsentPurpose,
  ConsentScope,
  ConsentRecord,
  ConsentGrantResult,
  ConsentGrantOptions,
  ConsentBasis,
  GdprArticle,
  ErasureExemptionBasis,
  ErasureStatus,
  ErasureRequest,
  ErasureAuditRecord,
  ErasurePending,
  ErasureInProgress,
  ErasureRecording,
  ErasureSubscriptionEvent,
  ChainVerification,
  AuditReport,
  AuditEntry,
  AuditQueryFilter,
} from './compliance';
export {
  CommandHandler,
  Projector,
  EventCodec,
  jsonCodec,
  replay,
  runCommand,
  applyCommand,
} from './event-sourcing';
export { Query } from './query-builder';
export {
  // v0.7.0 typed primitives — see typed-primitives.ts.
  DateField,
  TruncatableDateField,
  Interval,
  IntervalOverflowError,
  SubstringRange,
  AggregateMemoryBudget,
  AggregateMemoryBudgetTooSmallError,
  NANOS_PER_DAY,
  AGGREGATE_BUDGET_MIN_BYTES,
  AGGREGATE_BUDGET_DEFAULT_BYTES,
  dateFieldKeyword,
  extractFromSql,
  dateTruncSql,
  intervalFromComponents,
  intervalFromMonths,
  intervalFromDays,
  intervalFromNanos,
  intervalLiteral,
  substringFromStart,
  substringWithLength,
  substringSql,
  aggregateMemoryBudget,
} from './typed-primitives';
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
  TenantPoolEvent,
} from './tenant-pool';
export {
  AtClause,
  DataClass,
  Placement,
  StreamId,
  Offset,
  TenantId,
  Event,
  ClientConfig,
  QueryResult,
  RowView,
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
  ResponseTooLargeError,
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
