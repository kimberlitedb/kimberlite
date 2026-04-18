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
  constructor(private readonly native: NativeKimberliteClient) {}

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
