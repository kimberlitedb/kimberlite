/**
 * Per-tenant Client cache.
 *
 * AUDIT-2026-04 S2.4 — lifts notebar's LRU-per-tenant adapter out
 * of `packages/kimberlite-client/src/adapter.ts` into the SDK so
 * every multi-tenant SaaS gets the same pattern without
 * re-implementing LRU + idle eviction + factory callbacks.
 *
 * Difference from `Pool`:
 * - `Pool` multiplexes N connections to a SINGLE tenant across
 *   concurrent callers.
 * - `TenantPool` holds one long-lived `Client` per tenantId so
 *   N concurrent tenants can talk to the server without
 *   reconnecting on every call.
 *
 * Typical React Router V7 / Express use — one pool per process,
 * `.acquire(tenantId)` in each handler:
 *
 * ```ts
 * const pool = new TenantPool({
 *   factory: (tenantId) => Client.connect({ addresses: [addr], tenantId }),
 *   maxSize: 128,
 *   idleTimeoutMs: 5 * 60_000,
 * });
 *
 * export async function loader({ params }) {
 *   const client = await pool.acquire(BigInt(params.tenantId));
 *   return json(await client.query('...'));
 * }
 * ```
 */

import { Client } from './client';

type TenantId = bigint;

/**
 * Configuration for {@link TenantPool}.
 */
export interface TenantPoolConfig {
  /**
   * Callback that opens a fresh `Client` for a tenant. Typically
   * `(id) => Client.connect({ addresses, tenantId: id, authToken })`.
   */
  readonly factory: (tenantId: TenantId) => Promise<Client>;
  /**
   * Maximum number of concurrent cached tenants. When exceeded,
   * the least-recently-used client is disconnected + evicted.
   * Defaults to 128.
   */
  readonly maxSize?: number;
  /**
   * Idle-timeout in milliseconds. Clients untouched for this long
   * are disconnected + evicted on the next `acquire()` call.
   * `0` disables idle eviction. Defaults to 5 minutes.
   */
  readonly idleTimeoutMs?: number;
  /**
   * Injectable clock for deterministic tests. Defaults to
   * `Date.now`.
   */
  readonly now?: () => number;
}

/**
 * Runtime statistics — exposed for dashboards and tests.
 */
export interface TenantPoolStats {
  readonly size: number;
  readonly hits: number;
  readonly misses: number;
  readonly evictions: number;
  readonly idleEvictions: number;
}

interface Entry {
  client: Client;
  lastUsedAt: number;
}

/**
 * LRU-per-tenant `Client` cache.
 */
export class TenantPool {
  private readonly clients = new Map<TenantId, Entry>();
  private readonly factory: (tenantId: TenantId) => Promise<Client>;
  private readonly maxSize: number;
  private readonly idleTimeoutMs: number;
  private readonly now: () => number;

  private hits = 0;
  private misses = 0;
  private evictions = 0;
  private idleEvictions = 0;
  // Per-tenant connection promises — dedup concurrent acquires
  // for the same tenantId so only one `factory()` call fires.
  private readonly inflight = new Map<TenantId, Promise<Client>>();

  constructor(cfg: TenantPoolConfig) {
    this.factory = cfg.factory;
    this.maxSize = cfg.maxSize ?? 128;
    this.idleTimeoutMs = cfg.idleTimeoutMs ?? 5 * 60_000;
    this.now = cfg.now ?? Date.now;
  }

  /**
   * Return the cached `Client` for `tenantId`, creating one via
   * the factory if absent. Updates the LRU recency stamp.
   *
   * Concurrent calls for the same `tenantId` deduplicate — only
   * one `factory()` call fires.
   */
  async acquire(tenantId: TenantId): Promise<Client> {
    this.expireIdle();

    const entry = this.clients.get(tenantId);
    if (entry !== undefined) {
      entry.lastUsedAt = this.now();
      this.hits += 1;
      return entry.client;
    }

    // Dedup concurrent misses for the same tenant.
    const pending = this.inflight.get(tenantId);
    if (pending !== undefined) {
      this.hits += 1;
      return pending;
    }

    this.misses += 1;
    const p = this.connectAndInsert(tenantId);
    this.inflight.set(tenantId, p);
    try {
      return await p;
    } finally {
      this.inflight.delete(tenantId);
    }
  }

  /**
   * Convenience — `acquire` + execute + updates lastUsedAt. The
   * client itself is never surfaced; useful when callers should
   * not retain a handle past the call's scope.
   */
  async withClient<T>(
    tenantId: TenantId,
    fn: (client: Client) => Promise<T>,
  ): Promise<T> {
    const client = await this.acquire(tenantId);
    return fn(client);
  }

  /**
   * Drop all cached clients, disconnecting each one. Subsequent
   * `acquire()` calls will reconnect via the factory.
   */
  async close(): Promise<void> {
    const toClose = Array.from(this.clients.values());
    this.clients.clear();
    await Promise.all(toClose.map((e) => e.client.disconnect().catch(() => undefined)));
  }

  /** Runtime stats snapshot. */
  stats(): TenantPoolStats {
    return {
      size: this.clients.size,
      hits: this.hits,
      misses: this.misses,
      evictions: this.evictions,
      idleEvictions: this.idleEvictions,
    };
  }

  private async connectAndInsert(tenantId: TenantId): Promise<Client> {
    this.evictIfFull();
    const client = await this.factory(tenantId);
    this.clients.set(tenantId, { client, lastUsedAt: this.now() });
    return client;
  }

  private evictIfFull(): void {
    if (this.clients.size < this.maxSize) return;
    let oldestKey: TenantId | null = null;
    let oldestAt = Number.POSITIVE_INFINITY;
    for (const [key, entry] of this.clients.entries()) {
      if (entry.lastUsedAt < oldestAt) {
        oldestAt = entry.lastUsedAt;
        oldestKey = key;
      }
    }
    if (oldestKey !== null) {
      const e = this.clients.get(oldestKey);
      if (e !== undefined) {
        void e.client.disconnect().catch(() => undefined);
      }
      this.clients.delete(oldestKey);
      this.evictions += 1;
    }
  }

  private expireIdle(): void {
    if (this.idleTimeoutMs === 0) return;
    const cutoff = this.now() - this.idleTimeoutMs;
    const stale: TenantId[] = [];
    for (const [key, entry] of this.clients.entries()) {
      if (entry.lastUsedAt < cutoff) stale.push(key);
    }
    for (const key of stale) {
      const e = this.clients.get(key);
      if (e !== undefined) {
        void e.client.disconnect().catch(() => undefined);
      }
      this.clients.delete(key);
      this.idleEvictions += 1;
    }
  }
}
