/**
 * Thin TS wrapper around the N-API addon.
 *
 * Loads the platform-specific native binary (`kimberlite-node.<triple>.node`)
 * via the sibling `../native/index.js` loader and re-exports the addon's
 * surface with proper TypeScript types so the rest of the SDK can import
 * strictly-typed classes from here instead of going through `require`.
 */

// The loader picks between optional-dependency packages and locally-built
// addons. Runtime only — not included in `dist/`.
// eslint-disable-next-line @typescript-eslint/no-var-requires
const addon: NativeAddon = require('../native/index.js');

export type JsDataClass =
  | 'PHI'
  | 'Deidentified'
  | 'PII'
  | 'Sensitive'
  | 'PCI'
  | 'Financial'
  | 'Confidential'
  | 'Public';

export type JsPlacement = 'Global' | 'UsEast1' | 'ApSoutheast2';

export interface JsClientConfig {
  address: string;
  tenantId: bigint;
  authToken?: string | null;
  readTimeoutMs?: number | null;
  writeTimeoutMs?: number | null;
  /**
   * Internal read buffer size in bytes (default: 4 MiB). The framing
   * layer caps a single response at `2 * bufferSizeBytes`. Must be ≥
   * the largest `read({ maxBytes })` value plus a margin for framing
   * overhead — keep `bufferSizeBytes ≥ 2 * maxBytes`.
   */
  bufferSizeBytes?: number | null;
}

export type JsParamKind = 'null' | 'bigint' | 'text' | 'boolean' | 'timestamp';

export interface JsQueryParam {
  kind: JsParamKind;
  intValue?: bigint | null;
  textValue?: string | null;
  boolValue?: boolean | null;
  timestampValue?: bigint | null;
}

export interface JsQueryValue {
  kind: JsParamKind;
  intValue?: bigint | null;
  textValue?: string | null;
  boolValue?: boolean | null;
  timestampValue?: bigint | null;
}

export interface JsQueryResponse {
  columns: string[];
  rows: JsQueryValue[][];
}

export interface JsReadEventsResponse {
  events: Buffer[];
  nextOffset: bigint | null;
}

export interface JsExecuteResult {
  rowsAffected: bigint;
  logOffset: bigint;
}

export interface JsSubscribeAck {
  subscriptionId: bigint;
  startOffset: bigint;
  credits: number;
}

export type JsSubscriptionCloseReason =
  | 'ClientCancelled'
  | 'ServerShutdown'
  | 'StreamDeleted'
  | 'BackpressureTimeout'
  | 'ProtocolError';

export interface JsSubscriptionEvent {
  offset: bigint;
  data: Buffer | null;
  closed: boolean;
  closeReason: JsSubscriptionCloseReason | null;
}

export interface NativeKimberliteClient {
  readonly tenantId: bigint;
  readonly lastRequestId: bigint | null;
  /**
   * AUDIT-2026-04 S3.9 — stage the SDK-supplied audit context for
   * subsequent async method calls. The TS wrapper calls this
   * synchronously before each method and `clearAuditContext()` after.
   * Any null field is treated as "not provided".
   */
  setAuditContext(
    actor: string | null,
    reason: string | null,
    correlationId: string | null,
    idempotencyKey: string | null,
  ): void;
  clearAuditContext(): void;
  createStream(name: string, dataClass: JsDataClass): Promise<bigint>;
  createStreamWithPlacement(
    name: string,
    dataClass: JsDataClass,
    placement: JsPlacement,
  ): Promise<bigint>;
  append(streamId: bigint, events: Buffer[], expectedOffset: bigint): Promise<bigint>;
  readEvents(
    streamId: bigint,
    fromOffset: bigint,
    maxBytes: bigint,
  ): Promise<JsReadEventsResponse>;
  streamLength(streamId: bigint): Promise<bigint>;
  query(sql: string, params?: JsQueryParam[] | null): Promise<JsQueryResponse>;
  queryAt(
    sql: string,
    params: JsQueryParam[] | null | undefined,
    position: bigint,
  ): Promise<JsQueryResponse>;
  execute(sql: string, params?: JsQueryParam[] | null): Promise<JsExecuteResult>;
  sync(): Promise<void>;
  subscribe(
    streamId: bigint,
    fromOffset: bigint,
    initialCredits: number,
    consumerGroup?: string | null,
  ): Promise<JsSubscribeAck>;
  grantCredits(subscriptionId: bigint, additional: number): Promise<number>;
  unsubscribe(subscriptionId: bigint): Promise<void>;
  nextSubscriptionEvent(subscriptionId: bigint): Promise<JsSubscriptionEvent>;

  // Phase 5 compliance
  consentGrant(
    subjectId: string,
    purpose: JsConsentPurpose,
    basis?: JsConsentBasis | null,
    /**
     * v0.6.2 — terms-of-service version the subject responded to.
     * `null`/omitted on pre-v0.6.2 callers; the server still
     * accepts the request.
     */
    termsVersion?: string | null,
    /**
     * v0.6.2 — whether the subject accepted (`true`, default) or
     * declined (`false`). Pass `null`/omit to use the default;
     * pass `false` to record an explicit decline.
     */
    accepted?: boolean | null,
  ): Promise<{ consentId: string; grantedAtNanos: bigint }>;
  consentWithdraw(consentId: string): Promise<bigint>;
  consentCheck(subjectId: string, purpose: JsConsentPurpose): Promise<boolean>;
  consentList(subjectId: string, validOnly: boolean): Promise<JsConsentRecord[]>;
  erasureRequest(subjectId: string): Promise<JsErasureRequestInfo>;
  erasureMarkProgress(
    requestId: string,
    streamIds: bigint[],
  ): Promise<JsErasureRequestInfo>;
  erasureMarkStreamErased(
    requestId: string,
    streamId: bigint,
    recordsErased: bigint,
  ): Promise<JsErasureRequestInfo>;
  erasureComplete(requestId: string): Promise<JsErasureAuditInfo>;
  erasureExempt(
    requestId: string,
    basis: JsErasureExemptionBasis,
  ): Promise<JsErasureRequestInfo>;
  erasureStatus(requestId: string): Promise<JsErasureRequestInfo>;
  erasureList(): Promise<JsErasureAuditInfo[]>;

  // v0.6.0 Tier 2 #9 — SDK-safe audit-log query. PHI-free by
  // construction (see JsAuditEntry).
  auditQuery(filter: JsAuditQueryFilter): Promise<JsAuditEntry[]>;
  // v0.8.0 — chain-verification report.
  verifyAuditChain(): Promise<JsVerifyAuditChainReport>;

  // Phase 4 admin + schema + server info
  listTables(): Promise<JsTableInfo[]>;
  describeTable(tableName: string): Promise<JsDescribeTable>;
  listIndexes(tableName: string): Promise<JsIndexInfo[]>;
  tenantCreate(tenantId: bigint, name?: string | null): Promise<JsTenantCreateResult>;
  tenantList(): Promise<JsTenantInfo[]>;
  tenantDelete(tenantId: bigint): Promise<JsTenantDeleteResult>;
  tenantGet(tenantId: bigint): Promise<JsTenantInfo>;
  apiKeyRegister(
    subject: string,
    tenantId: bigint,
    roles: string[],
    expiresAtNanos?: bigint | null,
  ): Promise<JsApiKeyRegisterResult>;
  apiKeyRevoke(key: string): Promise<boolean>;
  apiKeyList(tenantId?: bigint | null): Promise<JsApiKeyInfo[]>;
  apiKeyRotate(oldKey: string): Promise<JsApiKeyRotateResult>;
  serverInfo(): Promise<JsServerInfo>;

  // Phase 6: Masking policy catalogue (v0.6.0 Tier 2 #7)
  maskingPolicyCreate(
    name: string,
    strategy: JsMaskingStrategy,
    exemptRoles: string[],
  ): Promise<void>;
  maskingPolicyDrop(name: string): Promise<void>;
  maskingPolicyAttach(table: string, column: string, policyName: string): Promise<void>;
  maskingPolicyDetach(table: string, column: string): Promise<void>;
  maskingPolicyList(includeAttachments: boolean): Promise<JsMaskingPolicyListResponse>;
}

/**
 * Masking strategy descriptor passed across the napi boundary.
 * The `kind` field tags the variant; `replacement` and `maxChars`
 * are only set when their respective variant requires them.
 */
export interface JsMaskingStrategy {
  kind:
    | 'RedactSsn'
    | 'RedactPhone'
    | 'RedactEmail'
    | 'RedactCreditCard'
    | 'RedactCustom'
    | 'Hash'
    | 'Tokenize'
    | 'Truncate'
    | 'Null';
  replacement?: string;
  maxChars?: number;
}

export interface JsMaskingPolicyInfo {
  name: string;
  strategy: JsMaskingStrategy;
  exemptRoles: string[];
  defaultMasked: boolean;
  attachmentCount: number;
}

export interface JsMaskingAttachmentInfo {
  tableName: string;
  columnName: string;
  policyName: string;
}

export interface JsMaskingPolicyListResponse {
  policies: JsMaskingPolicyInfo[];
  attachments: JsMaskingAttachmentInfo[];
}

export interface JsTableInfo {
  name: string;
  columnCount: number;
}

export interface JsColumnInfo {
  name: string;
  dataType: string;
  nullable: boolean;
  primaryKey: boolean;
}

export interface JsIndexInfo {
  name: string;
  columns: string[];
}

export interface JsDescribeTable {
  tableName: string;
  columns: JsColumnInfo[];
}

export interface JsTenantInfo {
  tenantId: bigint;
  name: string | null;
  tableCount: number;
  createdAtNanos: bigint | null;
}

export interface JsTenantCreateResult {
  tenant: JsTenantInfo;
  created: boolean;
}

export interface JsTenantDeleteResult {
  deleted: boolean;
  tablesDropped: number;
}

export interface JsApiKeyInfo {
  keyId: string;
  subject: string;
  tenantId: bigint;
  roles: string[];
  expiresAtNanos: bigint | null;
}

export interface JsApiKeyRegisterResult {
  key: string;
  info: JsApiKeyInfo;
}

export interface JsApiKeyRotateResult {
  newKey: string;
  info: JsApiKeyInfo;
}

export interface JsServerInfo {
  buildVersion: string;
  protocolVersion: number;
  capabilities: string[];
  uptimeSecs: bigint;
  clusterMode: 'Standalone' | 'Clustered';
  tenantCount: number;
}

export type JsConsentPurpose =
  | 'Marketing'
  | 'Analytics'
  | 'Contractual'
  | 'LegalObligation'
  | 'VitalInterests'
  | 'PublicTask'
  | 'Research'
  | 'Security';

export type JsConsentScope =
  | 'AllData'
  | 'ContactInfo'
  | 'AnalyticsOnly'
  | 'ContractualNecessity';

export type JsErasureExemptionBasis =
  | 'LegalObligation'
  | 'PublicHealth'
  | 'Archiving'
  | 'LegalClaims';

/**
 * GDPR Article 6(1) lawful basis — added on wire protocol v4
 * (v0.6.0). Mirrors the `GdprArticle` string-literal union in
 * `compliance.ts`; kept here as a separate native-shape alias so
 * the N-API boundary stays bivalent with the Rust enum.
 */
export type JsGdprArticle =
  | 'Consent'
  | 'Contract'
  | 'LegalObligation'
  | 'VitalInterests'
  | 'PublicTask'
  | 'LegitimateInterests';

export interface JsConsentBasis {
  article: JsGdprArticle;
  justification?: string | null;
}

export interface JsConsentRecord {
  consentId: string;
  subjectId: string;
  purpose: JsConsentPurpose;
  scope: JsConsentScope;
  grantedAtNanos: bigint;
  withdrawnAtNanos: bigint | null;
  expiresAtNanos: bigint | null;
  notes: string | null;
  /** Populated on records granted via wire v4+; `null` on older ones. */
  basis: JsConsentBasis | null;
  /**
   * v0.6.2 — terms-of-service version. `null` on pre-v0.6.2 records.
   */
  termsVersion: string | null;
  /**
   * v0.6.2 — acceptance flag. Pre-v0.6.2 records always read `true`.
   */
  accepted: boolean;
}

export interface JsErasureStatusTag {
  kind: string;
  streamsRemaining?: number | null;
  erasedAtNanos?: bigint | null;
  totalRecords?: bigint | null;
  reason?: string | null;
  retryAtNanos?: bigint | null;
  basis?: JsErasureExemptionBasis | null;
}

export interface JsErasureRequestInfo {
  requestId: string;
  subjectId: string;
  requestedAtNanos: bigint;
  deadlineNanos: bigint;
  status: JsErasureStatusTag;
  recordsErased: bigint;
  streamsAffected: bigint[];
}

export interface JsErasureAuditInfo {
  requestId: string;
  subjectId: string;
  requestedAtNanos: bigint;
  completedAtNanos: bigint;
  recordsErased: bigint;
  streamsAffected: bigint[];
  erasureProofHex: string | null;
  /** v0.6.0 Tier 2 #8 — idempotence marker. Absent on pre-0.6.0 servers. */
  isNoopReplay?: boolean;
}

/** v0.6.0 Tier 2 #9 — PHI-safe audit entry. Carries field names,
 *  never values. */
export interface JsAuditEntry {
  eventId: string;
  timestampNanos: bigint;
  action: string;
  subjectId: string | null;
  actor: string | null;
  tenantId: bigint | null;
  ipAddress: string | null;
  correlationId: string | null;
  requestId: string | null;
  reason: string | null;
  sourceCountry: string | null;
  changedFieldNames: string[];
}

export interface JsAuditQueryFilter {
  subjectId?: string | null;
  actionType?: string | null;
  timeFromNanos?: bigint | null;
  timeToNanos?: bigint | null;
  actor?: string | null;
  limit?: number | null;
}

export interface JsVerifyAuditChainReport {
  ok: boolean;
  eventCount: bigint;
  chainHeadHex: string;
  mismatchAtIndex: bigint | null;
  errorMessage: string | null;
}

export interface JsPoolConfig {
  address: string;
  tenantId: bigint;
  authToken?: string | null;
  maxSize?: number | null;
  acquireTimeoutMs?: number | null;
  idleTimeoutMs?: number | null;
  readTimeoutMs?: number | null;
  writeTimeoutMs?: number | null;
  bufferSizeBytes?: number | null;
}

export interface JsPoolStats {
  maxSize: number;
  open: number;
  idle: number;
  inUse: number;
  shutdown: boolean;
}

export interface NativeKimberlitePooledClient {
  readonly tenantId: bigint;
  readonly lastRequestId: bigint | null;
  release(): void;
  discard(): void;
  createStream(name: string, dataClass: JsDataClass): Promise<bigint>;
  createStreamWithPlacement(
    name: string,
    dataClass: JsDataClass,
    placement: JsPlacement,
  ): Promise<bigint>;
  append(streamId: bigint, events: Buffer[], expectedOffset: bigint): Promise<bigint>;
  readEvents(
    streamId: bigint,
    fromOffset: bigint,
    maxBytes: bigint,
  ): Promise<JsReadEventsResponse>;
  streamLength(streamId: bigint): Promise<bigint>;
  query(sql: string, params?: JsQueryParam[] | null): Promise<JsQueryResponse>;
  queryAt(
    sql: string,
    params: JsQueryParam[] | null | undefined,
    position: bigint,
  ): Promise<JsQueryResponse>;
  execute(sql: string, params?: JsQueryParam[] | null): Promise<JsExecuteResult>;
  sync(): Promise<void>;
}

export interface NativeKimberlitePool {
  acquire(): Promise<NativeKimberlitePooledClient>;
  stats(): JsPoolStats;
  shutdown(): void;
}

export interface KimberlitePoolCtor {
  create(config: JsPoolConfig): Promise<NativeKimberlitePool>;
}

export interface KimberliteClientCtor {
  connect(config: JsClientConfig): Promise<NativeKimberliteClient>;
}

interface NativeAddon {
  KimberliteClient: KimberliteClientCtor;
  KimberlitePool: KimberlitePoolCtor;
  JsDataClass: Record<string, JsDataClass>;
  JsPlacement: Record<string, JsPlacement>;
}

export const KimberliteClient: KimberliteClientCtor = addon.KimberliteClient;
export const KimberlitePool: KimberlitePoolCtor = addon.KimberlitePool;
