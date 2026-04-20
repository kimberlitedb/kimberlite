/**
 * High-level Kimberlite client — Promise-based API backed by a Rust N-API
 * native addon. See `../native/index.js` for platform loading.
 */

import { DataClass, Placement, Event, ClientConfig, Offset, QueryResult, StreamId } from './types';
import { Value, ValueType } from './value';
import { ConnectionError, wrapNativeError } from './errors';
import { Subscription, SubscribeOptions } from './subscription';
import { AdminNamespace } from './admin';
import { ComplianceNamespace } from './compliance';
import {
  KimberliteClient as NativeConstructor,
  NativeKimberliteClient,
  JsClientConfig,
  JsDataClass as NativeDataClass,
  JsPlacement as NativePlacement,
  JsQueryParam as NativeQueryParam,
  JsQueryValue as NativeQueryValue,
} from './native';

/**
 * Kimberlite database client.
 *
 * Every method routes through `invoke()`, which catches a `ConnectionError`
 * thrown by the native layer and, when `autoReconnect` is enabled (the
 * default), opens a fresh native connection and retries the call exactly
 * once before surfacing the error. This matches the self-healing behaviour
 * of mature database drivers (`pg`, `mysql2`, `ioredis`) — long-lived
 * servers restart, idle timers fire, load balancers close connections, and
 * callers shouldn't have to babysit the socket.
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
  private readonly nativeConfig: JsClientConfig;
  private readonly autoReconnect: boolean;
  private reconnecting: Promise<void> | null = null;
  /** Incremented on every successful `reconnect()` so tests can observe it. */
  private _reconnectCount = 0;

  private constructor(n: NativeKimberliteClient, nativeConfig: JsClientConfig, autoReconnect: boolean) {
    this.native = n;
    this.nativeConfig = nativeConfig;
    this.autoReconnect = autoReconnect;
  }

  /** Connect to a Kimberlite server and complete the protocol handshake. */
  static async connect(config: ClientConfig): Promise<Client> {
    const addr = firstAddress(config.addresses);
    try {
      // napi-rs's Option<T> accepts `undefined` but not `null`; omit keys
      // rather than passing null.
      const nativeConfig: JsClientConfig = {
        address: addr,
        tenantId: config.tenantId,
      };
      if (config.authToken !== undefined) nativeConfig.authToken = config.authToken;
      if (config.readTimeoutMs !== undefined) nativeConfig.readTimeoutMs = config.readTimeoutMs;
      if (config.writeTimeoutMs !== undefined) nativeConfig.writeTimeoutMs = config.writeTimeoutMs;
      if (config.bufferSizeBytes !== undefined)
        nativeConfig.bufferSizeBytes = config.bufferSizeBytes;
      const n = await NativeConstructor.connect(nativeConfig);
      const autoReconnect = config.autoReconnect ?? true;
      return new Client(n, nativeConfig, autoReconnect);
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
   * Number of times `this` has replaced its underlying native handle via
   * auto-reconnect (or an explicit `reconnect()` call). Starts at zero and
   * monotonically increases for the life of the `Client`.
   */
  get reconnectCount(): number {
    return this._reconnectCount;
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

  /**
   * Force a reconnect. Useful after a long idle period or when the caller
   * knows the backend was restarted. Safe to call concurrently — in-flight
   * reconnects are deduplicated.
   */
  async reconnect(): Promise<void> {
    if (this.native === null) throw new Error('Client is closed');
    if (this.reconnecting !== null) return this.reconnecting;
    this.reconnecting = (async () => {
      try {
        const fresh = await NativeConstructor.connect(this.nativeConfig);
        this.native = fresh;
        this._reconnectCount += 1;
      } finally {
        this.reconnecting = null;
      }
    })();
    return this.reconnecting;
  }

  /** Create a new stream. Returns the stream ID. */
  async createStream(name: string, dataClass: DataClass): Promise<StreamId> {
    return this.invoke((n) => n.createStream(name, dataClass as NativeDataClass));
  }

  /** Create a new stream with a specific geographic placement policy. */
  async createStreamWithPlacement(
    name: string,
    dataClass: DataClass,
    placement: Placement,
  ): Promise<StreamId> {
    return this.invoke((n) =>
      n.createStreamWithPlacement(
        name,
        dataClass as NativeDataClass,
        placement as NativePlacement,
      ),
    );
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
    if (events.length === 0) {
      throw new Error('Cannot append empty event list');
    }
    return this.invoke((n) => n.append(streamId, events, expectedOffset));
  }

  /** Read events from a stream starting at `fromOffset`. */
  async read(
    streamId: StreamId,
    options: { fromOffset?: Offset; maxBytes?: number | bigint } = {},
  ): Promise<Event[]> {
    const fromOffset = options.fromOffset ?? 0n;
    const maxBytes = BigInt(options.maxBytes ?? 1024 * 1024);
    const resp = await this.invoke((n) => n.readEvents(streamId, fromOffset, maxBytes));
    return resp.events.map((data, i) => ({
      offset: fromOffset + BigInt(i),
      data,
    }));
  }

  /** Execute a SQL query against current state. */
  async query(sql: string, params: Value[] = []): Promise<QueryResult> {
    const resp = await this.invoke((n) => n.query(sql, params.map(valueToNativeParam)));
    return nativeResponseToQueryResult(resp);
  }

  /** Execute a SQL query at a specific log position (time travel). */
  async queryAt(sql: string, params: Value[], position: Offset): Promise<QueryResult> {
    const resp = await this.invoke((n) =>
      n.queryAt(sql, params.map(valueToNativeParam), position),
    );
    return nativeResponseToQueryResult(resp);
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
    const resp = await this.invoke((n) => n.execute(sql, params.map(valueToNativeParam)));
    return {
      rowsAffected: resp.rowsAffected,
      logOffset: resp.logOffset,
    };
  }

  /**
   * Subscribe to real-time events on a stream. Returns an async iterator
   * that yields `SubscriptionEvent`s as the server pushes them.
   *
   * Subscriptions are long-lived; reconnect logic does not apply once the
   * subscription stream is open — on drop the subscriber should restart
   * the subscription from the last-acknowledged offset.
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
    const ack = await this.invoke((n) =>
      n.subscribe(streamId, fromOffset, initialCredits, opts.consumerGroup ?? null),
    );
    return new Subscription(this.native!, ack.subscriptionId, ack.credits, lowWater, refill);
  }

  /** Flush pending writes to disk on the server. */
  async sync(): Promise<void> {
    await this.invoke((n) => n.sync());
  }

  private checkOpen(): void {
    if (!this.native) throw new Error('Client is closed');
  }

  /**
   * AUDIT-2026-04 S3.5 — healthcare BREAK_GLASS query.
   *
   * Prepends `WITH BREAK_GLASS REASON='<reason>'` to the SQL
   * and runs it through {@link Client.query}. The server emits
   * an audit signal tagged with the reason before executing
   * the inner statement under normal RBAC + masking.
   *
   * Use for emergency-access scenarios (ER intake, code-blue
   * queries) where regulators require attributable access.
   *
   * `reason` must not contain single quotes — the prefix
   * parser does not support escapes.
   */
  async queryBreakGlass(
    reason: string,
    sql: string,
    params: Value[] = [],
  ): Promise<QueryResult> {
    if (reason.includes("'")) {
      throw new Error(
        "queryBreakGlass: reason must not contain single quotes",
      );
    }
    return this.query(`WITH BREAK_GLASS REASON='${reason}' ${sql}`, params);
  }

  /**
   * AUDIT-2026-04 S3.3 — issue an `EXPLAIN <sql>` query and
   * return the rendered plan tree.
   *
   * Sugar over {@link Client.query} — equivalent to issuing
   * ``EXPLAIN ${sql}`` and unwrapping the single-cell `plan`
   * column. Useful for ops tooling and interactive REPLs where
   * you want to inspect a plan without parsing the
   * `QueryResult`.
   */
  async queryExplain(sql: string, params: Value[] = []): Promise<string> {
    const result = await this.query(`EXPLAIN ${sql}`, params);
    const firstRow = result.rows[0];
    if (firstRow === undefined) {
      throw new Error('queryExplain: server returned empty rows for EXPLAIN');
    }
    const cell = firstRow[0];
    if (cell === undefined) {
      throw new Error('queryExplain: EXPLAIN row had no cells');
    }
    if (cell.type !== ValueType.Text) {
      throw new Error(
        `queryExplain: expected Text plan cell, got ${ValueType[cell.type] ?? 'Unknown'}`,
      );
    }
    return cell.value;
  }

  /**
   * AUDIT-2026-04 S2.4 — port of notebar's `upsertRow` helper.
   *
   * UPDATE the row keyed by `columns[0] = values[0]`; if zero rows
   * were affected, INSERT a new row with the full column list.
   *
   * Kimberlite does not (yet) support `INSERT ... ON CONFLICT`, so
   * this UPDATE-then-INSERT dance is the canonical upsert shape.
   * Without this helper, every app rebuilds it (notebar had it
   * inside `packages/kimberlite-client/src/repo-kit.ts`).
   *
   * `columns[0]` is the primary-key column; callers providing a
   * mis-matched `columns.length !== values.length` get a thrown
   * `Error` without a network round-trip.
   *
   * Returns the number of rows affected by the winning path —
   * 1n if the UPDATE hit, 1n if the INSERT ran, 0n if both
   * yielded zero (shouldn't happen unless the table definition
   * is pathological).
   */
  async upsertRow(
    table: string,
    columns: readonly string[],
    values: readonly Value[],
  ): Promise<bigint> {
    if (columns.length === 0 || columns.length !== values.length) {
      throw new Error('upsertRow: columns and values must have matching non-zero length');
    }
    const pkCol = columns[0]!;
    const pkVal = values[0]!;
    const setCols = columns.slice(1);
    const setVals = values.slice(1);

    if (setCols.length > 0) {
      const setClause = setCols.map((c, i) => `${c} = $${String(i + 1)}`).join(', ');
      const updateSql = `UPDATE ${table} SET ${setClause} WHERE ${pkCol} = $${String(setCols.length + 1)}`;
      const res = await this.execute(updateSql, [...setVals, pkVal]);
      if (res.rowsAffected > 0n) return res.rowsAffected;
    }

    const colList = columns.join(', ');
    const placeholders = columns.map((_, i) => `$${String(i + 1)}`).join(', ');
    const insertSql = `INSERT INTO ${table} (${colList}) VALUES (${placeholders})`;
    const res = await this.execute(insertSql, [...values]);
    return res.rowsAffected;
  }

  /**
   * Dispatch a native call with wrap-and-reconnect semantics.
   *
   * 1. Run `fn(native)`; on success, return its result.
   * 2. On error, map it through `wrapNativeError`.
   * 3. If the wrapped error is a `ConnectionError` and `autoReconnect` is
   *    on, call `reconnect()` and invoke `fn` once more with the fresh
   *    native handle. The second attempt's errors are surfaced verbatim.
   */
  private async invoke<T>(fn: (n: NativeKimberliteClient) => Promise<T>): Promise<T> {
    this.checkOpen();
    try {
      return await fn(this.native!);
    } catch (e) {
      const wrapped = wrapNativeError(e);
      if (this.autoReconnect && wrapped instanceof ConnectionError) {
        await this.reconnect();
        this.checkOpen();
        try {
          return await fn(this.native!);
        } catch (e2) {
          throw wrapNativeError(e2);
        }
      }
      throw wrapped;
    }
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
