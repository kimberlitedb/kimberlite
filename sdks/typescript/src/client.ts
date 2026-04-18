/**
 * High-level Kimberlite client — Promise-based API backed by a Rust N-API
 * native addon. See `../native/index.js` for platform loading.
 */

import { DataClass, Placement, Event, ClientConfig, Offset, QueryResult, StreamId } from './types';
import { Value, ValueType } from './value';
import { wrapNativeError } from './errors';
import { Subscription, SubscribeOptions } from './subscription';
import { AdminNamespace } from './admin';
import { ComplianceNamespace } from './compliance';
import {
  KimberliteClient as NativeConstructor,
  NativeKimberliteClient,
  JsDataClass as NativeDataClass,
  JsPlacement as NativePlacement,
  JsQueryParam as NativeQueryParam,
  JsQueryValue as NativeQueryValue,
} from './native';

/**
 * Kimberlite database client.
 *
 * @example
 * ```typescript
 * import { Client, DataClass, ValueBuilder } from '@kimberlite/client';
 *
 * const client = await Client.connect({
 *   addresses: ['127.0.0.1:5432'],
 *   tenantId: 1n,
 * });
 *
 * try {
 *   const stream = await client.createStream('events', DataClass.PHI);
 *   await client.append(stream, [Buffer.from('hello')]);
 *
 *   const result = await client.query(
 *     'SELECT * FROM patients WHERE id = $1',
 *     [ValueBuilder.bigint(1)],
 *   );
 * } finally {
 *   await client.disconnect();
 * }
 * ```
 */
export class Client {
  private native: NativeKimberliteClient | null;

  private constructor(n: NativeKimberliteClient) {
    this.native = n;
  }

  /** Connect to a Kimberlite server and complete the protocol handshake. */
  static async connect(config: ClientConfig): Promise<Client> {
    const addr = firstAddress(config.addresses);
    try {
      // napi-rs's Option<T> accepts `undefined` but not `null`; omit keys
      // rather than passing null.
      const nativeConfig: import('./native').JsClientConfig = {
        address: addr,
        tenantId: config.tenantId,
      };
      if (config.authToken !== undefined) nativeConfig.authToken = config.authToken;
      if (config.readTimeoutMs !== undefined) nativeConfig.readTimeoutMs = config.readTimeoutMs;
      if (config.writeTimeoutMs !== undefined) nativeConfig.writeTimeoutMs = config.writeTimeoutMs;
      if (config.bufferSizeBytes !== undefined)
        nativeConfig.bufferSizeBytes = config.bufferSizeBytes;
      const n = await NativeConstructor.connect(nativeConfig);
      return new Client(n);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /** Tenant ID this client is connected as. */
  get tenantId(): bigint {
    this.checkOpen();
    return this.native!.tenantId;
  }

  /**
   * Wire request ID of the most recently sent request, or `null` if no
   * request has been sent yet. Useful for correlating client logs with
   * server-side tracing output.
   */
  get lastRequestId(): bigint | null {
    this.checkOpen();
    return this.native!.lastRequestId;
  }

  /**
   * Admin operations namespace — schema introspection, tenant lifecycle,
   * API-key lifecycle, server info. All operations require the Admin role.
   *
   * @example
   * ```ts
   * const tables = await client.admin.listTables();
   * const info = await client.admin.serverInfo();
   * ```
   */
  get admin(): AdminNamespace {
    this.checkOpen();
    return new AdminNamespace(this.native!);
  }

  /**
   * Compliance operations namespace — GDPR consent + erasure.
   *
   * @example
   * ```ts
   * await client.compliance.consent.grant('alice', 'Marketing');
   * const req = await client.compliance.erasure.request('alice');
   * ```
   */
  get compliance(): ComplianceNamespace {
    this.checkOpen();
    return new ComplianceNamespace(this.native!);
  }

  /** Disconnect. Safe to call more than once. */
  async disconnect(): Promise<void> {
    // The native addon's Drop impl closes the socket when the object is GC'd.
    // We just drop our reference so a subsequent call throws.
    this.native = null;
  }

  /** Create a new stream. Returns the stream ID. */
  async createStream(name: string, dataClass: DataClass): Promise<StreamId> {
    this.checkOpen();
    try {
      return await this.native!.createStream(name, dataClass as NativeDataClass);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /** Create a new stream with a specific geographic placement policy. */
  async createStreamWithPlacement(
    name: string,
    dataClass: DataClass,
    placement: Placement,
  ): Promise<StreamId> {
    this.checkOpen();
    try {
      return await this.native!.createStreamWithPlacement(
        name,
        dataClass as NativeDataClass,
        placement as NativePlacement,
      );
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /**
   * Append events to a stream with optimistic concurrency. Returns the offset
   * of the first appended event.
   */
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

  /** Read events from a stream starting at `fromOffset`. */
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

  /** Execute a SQL query against current state. */
  async query(sql: string, params: Value[] = []): Promise<QueryResult> {
    this.checkOpen();
    try {
      const resp = await this.native!.query(sql, params.map(valueToNativeParam));
      return nativeResponseToQueryResult(resp);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /** Execute a SQL query at a specific log position (time travel). */
  async queryAt(sql: string, params: Value[], position: Offset): Promise<QueryResult> {
    this.checkOpen();
    try {
      const resp = await this.native!.queryAt(sql, params.map(valueToNativeParam), position);
      return nativeResponseToQueryResult(resp);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /**
   * Execute a SQL query and map each row through the supplied `mapper` to a
   * typed value. Use this when you want `T[]` directly rather than the
   * dynamic `QueryResult` shape.
   *
   * @example
   * ```ts
   * interface User { id: bigint; name: string; }
   * const users = await client.queryRows<User>(
   *   'SELECT id, name FROM users WHERE tenant = $1',
   *   [ValueBuilder.bigint(42n)],
   *   (row, cols) => ({
   *     id: valueToBigInt(row[cols.indexOf('id')]),
   *     name: valueToString(row[cols.indexOf('name')]) ?? '',
   *   }),
   * );
   * ```
   */
  async queryRows<T>(
    sql: string,
    params: Value[],
    mapper: RowMapper<T>,
  ): Promise<T[]> {
    const result = await this.query(sql, params);
    return result.rows.map((row) => mapper(row, result.columns));
  }

  /**
   * Execute a DDL/DML statement (INSERT / UPDATE / DELETE / CREATE / ALTER).
   *
   * Returns `{ rowsAffected, logOffset }`. For DDL statements the row count
   * is typically 0. For `UPDATE ... RETURNING`, use `query` instead.
   */
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

  /**
   * Subscribe to real-time events on a stream. Returns an async iterator
   * that yields `SubscriptionEvent`s as the server pushes them.
   *
   * @example
   * ```ts
   * const sub = await client.subscribe(streamId, { initialCredits: 128 });
   * for await (const event of sub) {
   *   console.log(event.offset, event.data);
   * }
   * ```
   */
  async subscribe(streamId: StreamId, opts: SubscribeOptions = {}): Promise<Subscription> {
    this.checkOpen();
    const initialCredits = opts.initialCredits ?? 128;
    const fromOffset = opts.fromOffset ?? 0n;
    const lowWater = opts.lowWater ?? Math.floor(initialCredits / 4);
    const refill = opts.refill ?? initialCredits;
    try {
      const ack = await this.native!.subscribe(
        streamId,
        fromOffset,
        initialCredits,
        opts.consumerGroup ?? null,
      );
      return new Subscription(this.native!, ack.subscriptionId, ack.credits, lowWater, refill);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  /** Flush pending writes to disk on the server. */
  async sync(): Promise<void> {
    this.checkOpen();
    try {
      await this.native!.sync();
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  private checkOpen(): void {
    if (!this.native) throw new Error('Client is closed');
  }
}

/**
 * Result of a DML/DDL `execute()` call.
 */
export interface ExecuteResult {
  /** Number of rows inserted / updated / deleted (0 for DDL). */
  rowsAffected: bigint;
  /** Log offset at which the change was committed. */
  logOffset: bigint;
}

/**
 * Maps a result-set row (array of Values, column-name list) to a typed `T`.
 */
export type RowMapper<T> = (row: Value[], columns: string[]) => T;

// ============================================================================
// Value <-> native param/value conversion
// ============================================================================

function valueToNativeParam(v: Value): NativeQueryParam {
  switch (v.type) {
    case ValueType.Null:
      return { kind: 'null' };
    case ValueType.BigInt:
      return { kind: 'bigint', intValue: v.value };
    case ValueType.Text:
      return { kind: 'text', textValue: v.value };
    case ValueType.Boolean:
      return { kind: 'boolean', boolValue: v.value };
    case ValueType.Timestamp:
      return { kind: 'timestamp', timestampValue: v.value };
  }
}

function nativeValueToValue(v: NativeQueryValue): Value {
  switch (v.kind) {
    case 'null':
      return { type: ValueType.Null };
    case 'bigint':
      return { type: ValueType.BigInt, value: v.intValue ?? 0n };
    case 'text':
      return { type: ValueType.Text, value: v.textValue ?? '' };
    case 'boolean':
      return { type: ValueType.Boolean, value: v.boolValue ?? false };
    case 'timestamp':
      return { type: ValueType.Timestamp, value: v.timestampValue ?? 0n };
  }
}

function nativeResponseToQueryResult(resp: {
  columns: string[];
  rows: NativeQueryValue[][];
}): QueryResult {
  return {
    columns: resp.columns,
    rows: resp.rows.map((row) => row.map(nativeValueToValue)),
  };
}

function firstAddress(addresses: string[] | string): string {
  if (typeof addresses === 'string') return addresses;
  if (addresses.length === 0) {
    throw new Error('ClientConfig.addresses must not be empty');
  }
  // The Rust client connects to a single address; multi-address HA failover
  // is planned. First-address-wins preserves the existing API shape.
  return addresses[0];
}
