/**
 * Connection pool — Promise-based API backed by the Rust `Pool`.
 *
 * `Pool` holds up to `maxSize` live `Client` connections. Callers `acquire()`
 * a `PooledClient` and must release it (either via `release()` explicitly or
 * via the `withClient()` helper which auto-releases on promise settlement).
 */

import {
  AtClause,
  DataClass,
  Event,
  Offset,
  Placement,
  QueryResult,
  StreamId,
  TenantId,
} from './types';
import { Value, ValueType } from './value';
import { wrapNativeError } from './errors';
import { ExecuteResult, RowMapper } from './client';
import {
  KimberlitePool as NativePoolCtor,
  NativeKimberlitePool,
  NativeKimberlitePooledClient,
  JsDataClass as NativeDataClass,
  JsPlacement as NativePlacement,
  JsQueryParam as NativeQueryParam,
  JsQueryValue as NativeQueryValue,
} from './native';

/** Configuration passed to `Pool.create`. */
export interface PoolConfig {
  /** Server address as "host:port". */
  address: string;
  /** Tenant identifier. */
  tenantId: TenantId;
  /** Optional bearer token. */
  authToken?: string;
  /** Maximum concurrent connections (default: 10). */
  maxSize?: number;
  /** Milliseconds to wait on `acquire` (0 = wait forever). Default 30 000. */
  acquireTimeoutMs?: number;
  /** Milliseconds an idle connection stays open (0 = never expire). Default 300 000. */
  idleTimeoutMs?: number;
  /** Per-connection read timeout in ms (default 30 000). */
  readTimeoutMs?: number;
  /** Per-connection write timeout in ms (default 30 000). */
  writeTimeoutMs?: number;
  /** Per-connection buffer size (default 64 KiB). */
  bufferSizeBytes?: number;
}

/** Snapshot of pool utilisation. */
export interface PoolStats {
  maxSize: number;
  open: number;
  idle: number;
  inUse: number;
  shutdown: boolean;
}

/**
 * A thread-safe connection pool.
 *
 * @example
 * ```typescript
 * const pool = await Pool.create({ address: '127.0.0.1:5432', tenantId: 1n });
 *
 * // Run one op with auto-release:
 * const rows = await pool.withClient(async (client) => {
 *   const r = await client.query('SELECT 1');
 *   return r.rows;
 * });
 *
 * // Or acquire/release manually:
 * const client = await pool.acquire();
 * try {
 *   await client.query('SELECT 1');
 * } finally {
 *   client.release();
 * }
 *
 * await pool.shutdown();
 * ```
 */
export class Pool {
  private native: NativeKimberlitePool | null;

  private constructor(n: NativeKimberlitePool) {
    this.native = n;
  }

  /** Create a new pool. Connections are not opened eagerly. */
  static async create(config: PoolConfig): Promise<Pool> {
    try {
      const nativeConfig: import('./native').JsPoolConfig = {
        address: config.address,
        tenantId: config.tenantId,
      };
      if (config.authToken !== undefined) nativeConfig.authToken = config.authToken;
      if (config.maxSize !== undefined) nativeConfig.maxSize = config.maxSize;
      if (config.acquireTimeoutMs !== undefined)
        nativeConfig.acquireTimeoutMs = config.acquireTimeoutMs;
      if (config.idleTimeoutMs !== undefined) nativeConfig.idleTimeoutMs = config.idleTimeoutMs;
      if (config.readTimeoutMs !== undefined) nativeConfig.readTimeoutMs = config.readTimeoutMs;
      if (config.writeTimeoutMs !== undefined)
        nativeConfig.writeTimeoutMs = config.writeTimeoutMs;
      if (config.bufferSizeBytes !== undefined)
        nativeConfig.bufferSizeBytes = config.bufferSizeBytes;

      const n = await NativePoolCtor.create(nativeConfig);
      return new Pool(n);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /**
   * Acquire a client from the pool. Blocks up to `acquireTimeoutMs` before
   * throwing `TimeoutError`.
   */
  async acquire(): Promise<PooledClient> {
    this.checkOpen();
    try {
      const native = await this.native!.acquire();
      return new PooledClient(native);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /**
   * Acquire a client, pass it to `fn`, and release it when the returned
   * promise settles (success OR rejection). Safe to use inside async
   * handlers that may throw.
   */
  async withClient<T>(fn: (client: PooledClient) => Promise<T>): Promise<T> {
    const client = await this.acquire();
    try {
      return await fn(client);
    } finally {
      client.release();
    }
  }

  /** Returns current pool utilisation. */
  stats(): PoolStats {
    this.checkOpen();
    return this.native!.stats();
  }

  /**
   * Shut the pool down. Subsequent acquires reject; in-flight clients close
   * when released. Idempotent.
   */
  shutdown(): void {
    if (!this.native) return;
    this.native.shutdown();
    this.native = null;
  }

  private checkOpen(): void {
    if (!this.native) throw new Error('Pool has been shut down');
  }
}

/**
 * Pool-borrowed client. Exposes the same operations as `Client`, plus
 * `release()` / `discard()` for returning the connection to the pool.
 *
 * A `PooledClient` becomes inert after `release()` or `discard()`; further
 * calls throw.
 */
export class PooledClient {
  private native: NativeKimberlitePooledClient | null;

  constructor(native: NativeKimberlitePooledClient) {
    this.native = native;
  }

  /** Return the connection to the pool. Idempotent. */
  release(): void {
    if (this.native) {
      this.native.release();
      this.native = null;
    }
  }

  /**
   * Close the underlying TCP connection instead of returning it to the pool.
   * Use after a fatal protocol error.
   */
  discard(): void {
    if (this.native) {
      this.native.discard();
      this.native = null;
    }
  }

  get tenantId(): bigint {
    this.checkOpen();
    return this.native!.tenantId;
  }

  get lastRequestId(): bigint | null {
    this.checkOpen();
    return this.native!.lastRequestId;
  }

  async createStream(name: string, dataClass: DataClass): Promise<StreamId> {
    this.checkOpen();
    try {
      const id = await this.native!.createStream(name, dataClass as NativeDataClass);
      return StreamId.from(id);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async createStreamWithPlacement(
    name: string,
    dataClass: DataClass,
    placement: Placement,
  ): Promise<StreamId> {
    this.checkOpen();
    try {
      const id = await this.native!.createStreamWithPlacement(
        name,
        dataClass as NativeDataClass,
        placement as NativePlacement,
      );
      return StreamId.from(id);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async append(
    streamId: StreamId,
    events: Buffer[],
    expectedOffset: Offset = 0n,
  ): Promise<Offset> {
    this.checkOpen();
    if (events.length === 0) {
      throw new Error('Cannot append empty event list');
    }
    try {
      return await this.native!.append(streamId, events, expectedOffset);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async read(
    streamId: StreamId,
    options: { fromOffset?: Offset; maxBytes?: number | bigint } = {},
  ): Promise<Event[]> {
    this.checkOpen();
    const fromOffset = options.fromOffset ?? 0n;
    const maxBytes = BigInt(options.maxBytes ?? 1024 * 1024);
    try {
      const resp = await this.native!.readEvents(streamId, fromOffset, maxBytes);
      return resp.events.map((data, i) => ({
        offset: fromOffset + BigInt(i),
        data,
      }));
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async query(sql: string, params: Value[] = []): Promise<QueryResult> {
    this.checkOpen();
    try {
      const resp = await this.native!.query(sql, params.map(valueToNativeParam));
      return nativeResponseToQueryResult(resp);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async queryAt(
    sql: string,
    params: Value[],
    at: Offset | Date | string | bigint | AtClause,
  ): Promise<QueryResult> {
    this.checkOpen();
    try {
      const clause = normaliseAtClauseForPool(at);
      if (clause.kind === 'offset') {
        const resp = await this.native!.queryAt(sql, params.map(valueToNativeParam), clause.value);
        return nativeResponseToQueryResult(resp);
      }
      // Timestamp form — splice AS OF TIMESTAMP '<iso>' into the SQL.
      // See client.ts for design notes on why we keep the wire protocol
      // offset-only.
      const iso = nanosecondsToIsoStringForPool(clause.value);
      const trimmed = sql.replace(/;\s*$/, '');
      const withClause = `${trimmed} AS OF TIMESTAMP '${iso}'`;
      const resp = await this.native!.query(withClause, params.map(valueToNativeParam));
      return nativeResponseToQueryResult(resp);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async queryRows<T>(
    sql: string,
    params: Value[],
    mapper: RowMapper<T>,
  ): Promise<T[]> {
    const result = await this.query(sql, params);
    return result.rows.map((row) => mapper(row, result.columns));
  }

  async execute(sql: string, params: Value[] = []): Promise<ExecuteResult> {
    this.checkOpen();
    try {
      const resp = await this.native!.execute(sql, params.map(valueToNativeParam));
      return {
        rowsAffected: resp.rowsAffected,
        logOffset: resp.logOffset,
      };
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async sync(): Promise<void> {
    this.checkOpen();
    try {
      await this.native!.sync();
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  private checkOpen(): void {
    if (!this.native) {
      throw new Error('PooledClient has been released');
    }
  }
}

// ============================================================================
// Value <-> native param/value conversion (duplicated with client.ts; kept
// local so the pool module has no cross-module state that could be tangled
// by bundler dead-code elimination).
// ============================================================================

function valueToNativeParam(v: Value): NativeQueryParam {
  switch (v.kind) {
    case 'null':
      return { kind: 'null' };
    case 'bigint':
      return { kind: 'bigint', intValue: v.value };
    case 'text':
      return { kind: 'text', textValue: v.value };
    case 'boolean':
      return { kind: 'boolean', boolValue: v.value };
    case 'timestamp':
      return { kind: 'timestamp', timestampValue: v.value };
  }
}

function nativeValueToValue(v: NativeQueryValue): Value {
  switch (v.kind) {
    case 'null':
      return { kind: 'null', type: ValueType.Null };
    case 'bigint':
      return { kind: 'bigint', type: ValueType.BigInt, value: v.intValue ?? 0n };
    case 'text':
      return { kind: 'text', type: ValueType.Text, value: v.textValue ?? '' };
    case 'boolean':
      return { kind: 'boolean', type: ValueType.Boolean, value: v.boolValue ?? false };
    case 'timestamp':
      return { kind: 'timestamp', type: ValueType.Timestamp, value: v.timestampValue ?? 0n };
  }
}

/**
 * Mirror of `normaliseAtClause` in `client.ts` — local copy to avoid
 * a cross-module dependency on an internal helper. Kept byte-for-byte
 * equivalent so the pooled and non-pooled code paths agree on how
 * `queryAt(at)` forms are dispatched.
 */
function normaliseAtClauseForPool(
  at: Offset | Date | string | bigint | AtClause,
): AtClause {
  if (typeof at === 'object' && at !== null && 'kind' in at) {
    return at;
  }
  if (at instanceof Date) {
    const ms = at.getTime();
    if (Number.isNaN(ms)) {
      throw new TypeError(`PooledClient.queryAt: Date must be valid, got ${at.toString()}`);
    }
    return { kind: 'timestampNs', value: BigInt(ms) * 1_000_000n };
  }
  if (typeof at === 'string') {
    const ms = Date.parse(at);
    if (Number.isNaN(ms)) {
      throw new TypeError(`PooledClient.queryAt: unparseable ISO-8601 timestamp: '${at}'`);
    }
    return { kind: 'timestampNs', value: BigInt(ms) * 1_000_000n };
  }
  if (typeof at === 'bigint') {
    return { kind: 'offset', value: at };
  }
  throw new TypeError(`PooledClient.queryAt: unsupported 'at' value: ${String(at)}`);
}

function nanosecondsToIsoStringForPool(ns: bigint): string {
  const ms = Number(ns / 1_000_000n);
  const remainderNs = Number(ns % 1_000_000n);
  const base = new Date(ms).toISOString();
  if (remainderNs === 0) {
    return base;
  }
  const remainderStr = remainderNs.toString().padStart(6, '0');
  return base.replace('Z', `${remainderStr}Z`);
}

function nativeResponseToQueryResult(resp: {
  columns: string[];
  rows: NativeQueryValue[][];
}): QueryResult {
  const columns = resp.columns;
  const rows = resp.rows.map((row) => row.map(nativeValueToValue));
  return {
    columns,
    rows,
    row(index: number) {
      const r = rows[index];
      if (r === undefined) {
        throw new RangeError(
          `QueryResult.row(${index}): only ${rows.length} rows in result`,
        );
      }
      return {
        values: r,
        get(column: string): Value | undefined {
          const i = columns.indexOf(column);
          return i >= 0 ? r[i] : undefined;
        },
      };
    },
  };
}
