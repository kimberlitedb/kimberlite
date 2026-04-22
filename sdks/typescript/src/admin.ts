/**
 * Admin API surface — schema introspection, tenant lifecycle, API-key
 * management, and server info. Accessed via `client.admin`.
 *
 * Every method here is admin-only: the server gates on the `Admin` role.
 * Calls from non-Admin identities reject with `AuthenticationError`.
 *
 * @example
 * ```ts
 * // List every table in the caller's tenant.
 * const tables = await client.admin.listTables();
 *
 * // Issue a fresh API key for a service account.
 * const { key, info } = await client.admin.issueApiKey({
 *   subject: 'billing-service',
 *   tenantId: 1n,
 *   roles: ['User'],
 * });
 *
 * // Report server state.
 * const info = await client.admin.serverInfo();
 * console.log(`Kimberlite ${info.buildVersion} — uptime ${info.uptimeSecs}s`);
 * ```
 */

import { TenantId } from './types';
import { wrapNativeError } from './errors';
import type {
  NativeKimberliteClient,
  JsApiKeyInfo,
  JsMaskingStrategy,
} from './native';

export interface TableInfo {
  name: string;
  columnCount: number;
}

export interface ColumnInfo {
  name: string;
  dataType: string;
  nullable: boolean;
  primaryKey: boolean;
}

export interface IndexInfo {
  name: string;
  columns: string[];
}

export interface DescribeTable {
  tableName: string;
  columns: ColumnInfo[];
}

export interface TenantInfo {
  tenantId: TenantId;
  name: string | null;
  tableCount: number;
  createdAtNanos: bigint | null;
}

export interface TenantCreateResult {
  tenant: TenantInfo;
  /** `true` if the call registered a new tenant; `false` if idempotent. */
  created: boolean;
}

export interface TenantDeleteResult {
  deleted: boolean;
  tablesDropped: number;
}

export interface ApiKeyInfo {
  /** Short (8-char) stable identifier for display. Not the plaintext key. */
  keyId: string;
  subject: string;
  tenantId: TenantId;
  roles: string[];
  expiresAtNanos: bigint | null;
}

export interface ApiKeyRegisterResult {
  /** Plaintext key — returned exactly once. Persist immediately. */
  key: string;
  info: ApiKeyInfo;
}

export interface ApiKeyRotateResult {
  newKey: string;
  info: ApiKeyInfo;
}

export interface ServerInfo {
  buildVersion: string;
  protocolVersion: number;
  capabilities: string[];
  uptimeSecs: bigint;
  clusterMode: 'Standalone' | 'Clustered';
  tenantCount: number;
}

// -- Masking policy (v0.6.0 Tier 2 #7) --------------------------------------

/**
 * Masking strategy for CREATE MASKING POLICY. Discriminated union —
 * the `kind` field tags the variant so TypeScript narrowing works.
 *
 * @example
 * ```ts
 * // Redact SSNs except for clinical roles
 * await client.admin.maskingPolicy.create('ssn_policy', {
 *   strategy: { kind: 'RedactSsn' },
 *   exemptRoles: ['clinician', 'billing'],
 * });
 *
 * // Truncate keeping only 4 leading characters
 * await client.admin.maskingPolicy.create('trunc', {
 *   strategy: { kind: 'Truncate', maxChars: 4 },
 *   exemptRoles: ['admin'],
 * });
 * ```
 */
export type MaskingStrategy =
  | { kind: 'RedactSsn' }
  | { kind: 'RedactPhone' }
  | { kind: 'RedactEmail' }
  | { kind: 'RedactCreditCard' }
  | { kind: 'RedactCustom'; replacement: string }
  | { kind: 'Hash' }
  | { kind: 'Tokenize' }
  | { kind: 'Truncate'; maxChars: number }
  | { kind: 'Null' };

export interface CreateMaskingPolicyOptions {
  strategy: MaskingStrategy;
  /** Roles exempt from masking. Must be non-empty. */
  exemptRoles: string[];
}

export interface MaskingPolicyInfo {
  name: string;
  strategy: MaskingStrategy;
  exemptRoles: string[];
  defaultMasked: boolean;
  attachmentCount: number;
}

export interface MaskingAttachmentInfo {
  tableName: string;
  columnName: string;
  policyName: string;
}

export interface MaskingPolicyListResult {
  policies: MaskingPolicyInfo[];
  attachments: MaskingAttachmentInfo[];
}

export interface IssueApiKeyOptions {
  subject: string;
  tenantId: TenantId;
  roles: string[];
  /** Optional expiry as Unix nanoseconds. */
  expiresAtNanos?: bigint;
}

/**
 * Admin-operations namespace. Accessible as `client.admin`.
 */
export class AdminNamespace {
  /**
   * Grouped masking-policy catalogue surface.
   * v0.6.0 Tier 2 #7. See {@link MaskingPolicyNamespace}.
   */
  public readonly maskingPolicy: MaskingPolicyNamespace;

  constructor(private readonly native: NativeKimberliteClient) {
    this.maskingPolicy = new MaskingPolicyNamespace(native);
  }

  // ----- Schema introspection -----

  async listTables(): Promise<TableInfo[]> {
    try {
      const rows = await this.native.listTables();
      return rows.map((t) => ({ name: t.name, columnCount: t.columnCount }));
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async describeTable(tableName: string): Promise<DescribeTable> {
    try {
      const r = await this.native.describeTable(tableName);
      return {
        tableName: r.tableName,
        columns: r.columns.map((c) => ({
          name: c.name,
          dataType: c.dataType,
          nullable: c.nullable,
          primaryKey: c.primaryKey,
        })),
      };
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async listIndexes(tableName: string): Promise<IndexInfo[]> {
    try {
      const rows = await this.native.listIndexes(tableName);
      return rows.map((i) => ({ name: i.name, columns: i.columns }));
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  // ----- Tenant lifecycle -----

  async createTenant(tenantId: TenantId, name?: string): Promise<TenantCreateResult> {
    try {
      const r = await this.native.tenantCreate(tenantId, name ?? null);
      return {
        tenant: nativeTenantToTenant(r.tenant),
        created: r.created,
      };
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async listTenants(): Promise<TenantInfo[]> {
    try {
      const rows = await this.native.tenantList();
      return rows.map(nativeTenantToTenant);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async deleteTenant(tenantId: TenantId): Promise<TenantDeleteResult> {
    try {
      const r = await this.native.tenantDelete(tenantId);
      return { deleted: r.deleted, tablesDropped: r.tablesDropped };
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async getTenant(tenantId: TenantId): Promise<TenantInfo> {
    try {
      const r = await this.native.tenantGet(tenantId);
      return nativeTenantToTenant(r);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  // ----- API keys -----

  /**
   * Issue a new API key. The plaintext is returned exactly once — persist
   * it immediately; the server retains only a hash.
   */
  async issueApiKey(opts: IssueApiKeyOptions): Promise<ApiKeyRegisterResult> {
    try {
      const r = await this.native.apiKeyRegister(
        opts.subject,
        opts.tenantId,
        opts.roles,
        opts.expiresAtNanos ?? null,
      );
      return { key: r.key, info: nativeApiKeyToApiKey(r.info) };
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /** Revoke an API key by its plaintext. */
  async revokeApiKey(key: string): Promise<boolean> {
    try {
      return await this.native.apiKeyRevoke(key);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async listApiKeys(tenantId?: TenantId): Promise<ApiKeyInfo[]> {
    try {
      const rows = await this.native.apiKeyList(tenantId ?? null);
      return rows.map(nativeApiKeyToApiKey);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /**
   * Atomically rotate an API key. Returns the new plaintext and revokes the
   * old key in one operation; no window exists with both keys live or
   * neither key live.
   */
  async rotateApiKey(oldKey: string): Promise<ApiKeyRotateResult> {
    try {
      const r = await this.native.apiKeyRotate(oldKey);
      return { newKey: r.newKey, info: nativeApiKeyToApiKey(r.info) };
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  // ----- Server info -----

  async serverInfo(): Promise<ServerInfo> {
    try {
      const r = await this.native.serverInfo();
      return {
        buildVersion: r.buildVersion,
        protocolVersion: r.protocolVersion,
        capabilities: r.capabilities,
        uptimeSecs: r.uptimeSecs,
        clusterMode: r.clusterMode as 'Standalone' | 'Clustered',
        tenantCount: r.tenantCount,
      };
    } catch (e) {
      throw wrapNativeError(e);
    }
  }
}

function nativeTenantToTenant(t: {
  tenantId: bigint;
  name: string | null;
  tableCount: number;
  createdAtNanos: bigint | null;
}): TenantInfo {
  return {
    tenantId: t.tenantId,
    name: t.name,
    tableCount: t.tableCount,
    createdAtNanos: t.createdAtNanos,
  };
}

function nativeApiKeyToApiKey(k: JsApiKeyInfo): ApiKeyInfo {
  return {
    keyId: k.keyId,
    subject: k.subject,
    tenantId: k.tenantId,
    roles: k.roles,
    expiresAtNanos: k.expiresAtNanos,
  };
}

// ============================================================================
// Masking policy namespace — v0.6.0 Tier 2 #7
// ============================================================================

/**
 * Grouped namespace for masking-policy catalogue operations.
 * Accessed as `client.admin.maskingPolicy.*`.
 *
 * @example
 * ```ts
 * await client.admin.maskingPolicy.create('ssn_policy', {
 *   strategy: { kind: 'RedactSsn' },
 *   exemptRoles: ['clinician'],
 * });
 * await client.admin.maskingPolicy.attach('patients', 'medicare_number', 'ssn_policy');
 * const { policies, attachments } = await client.admin.maskingPolicy.list(true);
 * ```
 */
export class MaskingPolicyNamespace {
  constructor(private readonly native: NativeKimberliteClient) {}

  /**
   * Create a masking policy in this tenant's catalogue.
   * @throws {InvalidRequestError} if `exemptRoles` is empty, the policy
   *   name duplicates an existing one, or the strategy is malformed.
   */
  async create(name: string, opts: CreateMaskingPolicyOptions): Promise<void> {
    validateIdentifier(name, 'policy name');
    if (opts.exemptRoles.length === 0) {
      throw new Error('exemptRoles must contain at least one role');
    }
    try {
      await this.native.maskingPolicyCreate(
        name,
        maskingStrategyToNative(opts.strategy),
        opts.exemptRoles,
      );
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /**
   * Drop a masking policy from this tenant's catalogue.
   * Rejected if any column still attaches to it — detach first.
   */
  async drop(name: string): Promise<void> {
    validateIdentifier(name, 'policy name');
    try {
      await this.native.maskingPolicyDrop(name);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /** Attach a pre-existing policy to `(table, column)`. One policy per column. */
  async attach(
    table: string,
    column: string,
    policyName: string,
  ): Promise<void> {
    validateIdentifier(table, 'table name');
    validateIdentifier(column, 'column name');
    validateIdentifier(policyName, 'policy name');
    try {
      await this.native.maskingPolicyAttach(table, column, policyName);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /** Detach the masking policy (if any) from `(table, column)`. */
  async detach(table: string, column: string): Promise<void> {
    validateIdentifier(table, 'table name');
    validateIdentifier(column, 'column name');
    try {
      await this.native.maskingPolicyDetach(table, column);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /**
   * List every masking policy in this tenant's catalogue.
   *
   * @param includeAttachments When `true`, the returned `attachments`
   *   list enumerates every `(table, column, policy)` pair. When
   *   `false` (default), only per-policy metadata is returned —
   *   `attachmentCount` on each policy is always populated.
   */
  async list(includeAttachments: boolean = false): Promise<MaskingPolicyListResult> {
    try {
      const r = await this.native.maskingPolicyList(includeAttachments);
      return {
        policies: r.policies.map((p) => ({
          name: p.name,
          strategy: nativeStrategyToMasking(p.strategy),
          exemptRoles: p.exemptRoles,
          defaultMasked: p.defaultMasked,
          attachmentCount: p.attachmentCount,
        })),
        attachments: r.attachments.map((a) => ({
          tableName: a.tableName,
          columnName: a.columnName,
          policyName: a.policyName,
        })),
      };
    } catch (e) {
      throw wrapNativeError(e);
    }
  }
}

/**
 * Client-side identifier guard. Rejects non-`[A-Za-z_][A-Za-z0-9_]*`
 * shapes before the napi boundary so SQL-injection-shaped inputs never
 * reach the parser.
 */
function validateIdentifier(s: string, label: string): void {
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(s)) {
    throw new Error(`${label} '${s}' is not a valid SQL identifier`);
  }
}

/**
 * Translate a typed `MaskingStrategy` discriminated union into the
 * napi-friendly flat `JsMaskingStrategy` shape.
 */
function maskingStrategyToNative(s: MaskingStrategy): JsMaskingStrategy {
  switch (s.kind) {
    case 'RedactSsn':
    case 'RedactPhone':
    case 'RedactEmail':
    case 'RedactCreditCard':
    case 'Hash':
    case 'Tokenize':
    case 'Null':
      return { kind: s.kind };
    case 'RedactCustom':
      return { kind: 'RedactCustom', replacement: s.replacement };
    case 'Truncate':
      return { kind: 'Truncate', maxChars: s.maxChars };
    default: {
      // Exhaustiveness check — the `never` binding trips at compile
      // time if a new `MaskingStrategy` variant is added without a case here.
      const _exhaustive: never = s;
      throw new Error(
        `unknown masking strategy kind: ${JSON.stringify(_exhaustive)}`,
      );
    }
  }
}

/**
 * Translate the flat napi-returned strategy back into the typed union.
 */
function nativeStrategyToMasking(s: JsMaskingStrategy): MaskingStrategy {
  switch (s.kind) {
    case 'RedactSsn':
    case 'RedactPhone':
    case 'RedactEmail':
    case 'RedactCreditCard':
    case 'Hash':
    case 'Tokenize':
    case 'Null':
      return { kind: s.kind };
    case 'RedactCustom':
      if (s.replacement === undefined) {
        throw new Error('RedactCustom strategy missing replacement');
      }
      return { kind: 'RedactCustom', replacement: s.replacement };
    case 'Truncate':
      if (s.maxChars === undefined) {
        throw new Error('Truncate strategy missing maxChars');
      }
      return { kind: 'Truncate', maxChars: s.maxChars };
    default: {
      const _exhaustive: never = s.kind;
      throw new Error(`unknown masking strategy kind from native: ${_exhaustive}`);
    }
  }
}
