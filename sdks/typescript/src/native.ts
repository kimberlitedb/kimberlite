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
