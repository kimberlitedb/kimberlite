/**
 * High-level Kimberlite client with Promise-based async API.
 */

import * as ref from 'ref-napi';
import {
  lib,
  KmbClientConfig,
  KmbReadResult,
  KmbQueryParam,
  KmbQueryValue,
  KmbQueryResult,
  uint64,
  int64,
  size_t,
  KMB_OK,
} from './ffi';
import {
  StreamId,
  Offset,
  TenantId,
  DataClass,
  Event,
  ClientConfig,
  QueryResult,
} from './types';
import { Value, ValueType } from './value';
import { throwForErrorCode } from './errors';

/**
 * Kimberlite database client.
 *
 * Provides a Promise-based TypeScript interface to Kimberlite.
 *
 * @example
 * ```typescript
 * const client = await Client.connect({
 *   addresses: ['localhost:5432'],
 *   tenantId: 1n,
 *   authToken: 'secret'
 * });
 *
 * try {
 *   const streamId = await client.createStream('events', DataClass.PHI);
 *   const offset = await client.append(streamId, [
 *     Buffer.from('event1'),
 *     Buffer.from('event2')
 *   ]);
 *   const events = await client.read(streamId, { fromOffset: 0n, maxBytes: 1024 });
 * } finally {
 *   await client.disconnect();
 * }
 * ```
 */
export class Client {
  private handle: Buffer | null;
  private closed = false;

  private constructor(handle: Buffer) {
    this.handle = handle;
  }

  /**
   * Connect to Kimberlite cluster.
   *
   * @param config - Connection configuration
   * @returns Connected client instance
   * @throws {ConnectionError} If connection fails
   * @throws {AuthenticationError} If authentication fails
   */
  static async connect(config: ClientConfig): Promise<Client> {
    return new Promise((resolve, reject) => {
      try {
        // Prepare addresses array
        const addressArray = Buffer.alloc(config.addresses.length * ref.sizeof.pointer);
        config.addresses.forEach((addr, i) => {
          const addrBuf = Buffer.from(addr + '\0', 'utf-8');
          ref.writePointer(addressArray, i * ref.sizeof.pointer, addrBuf);
        });

        // Prepare config
        const ffiConfig = new KmbClientConfig();
        ffiConfig.addresses = addressArray;
        ffiConfig.address_count = config.addresses.length;
        ffiConfig.tenant_id = config.tenantId;
        ffiConfig.auth_token = config.authToken
          ? Buffer.from(config.authToken + '\0', 'utf-8')
          : ref.NULL;
        ffiConfig.client_name = Buffer.from(
          (config.clientName || 'kimberlite-typescript') + '\0',
          'utf-8'
        );
        ffiConfig.client_version = Buffer.from(
          (config.clientVersion || '0.1.0') + '\0',
          'utf-8'
        );

        // Connect
        const handlePtr = ref.alloc(ref.refType(ref.types.void));
        const err = lib.kmb_client_connect(ffiConfig.ref(), handlePtr);

        if (err !== KMB_OK) {
          const msg = lib.kmb_error_message(err);
          throwForErrorCode(err, msg);
        }

        const handle = ref.readPointer(handlePtr, 0);
        resolve(new Client(handle));
      } catch (error) {
        reject(error);
      }
    });
  }

  /**
   * Disconnect from cluster and free resources.
   *
   * Safe to call multiple times.
   */
  async disconnect(): Promise<void> {
    if (!this.closed && this.handle) {
      lib.kmb_client_disconnect(this.handle);
      this.closed = true;
      this.handle = null;
    }
  }

  /**
   * Verify client is still connected.
   */
  private checkConnected(): void {
    if (this.closed || !this.handle) {
      throw new Error('Client is closed');
    }
  }

  /**
   * Create a new stream.
   *
   * @param name - Stream name (alphanumeric + underscore, max 256 chars)
   * @param dataClass - Data classification
   * @returns Stream identifier
   * @throws {StreamAlreadyExistsError} If stream name already exists
   * @throws {PermissionDeniedError} If tenant lacks permission for data class
   */
  async createStream(name: string, dataClass: DataClass): Promise<StreamId> {
    return new Promise((resolve, reject) => {
      try {
        this.checkConnected();

        const streamIdPtr = ref.alloc(uint64);
        const err = lib.kmb_client_create_stream(
          this.handle!,
          name,
          dataClass,
          streamIdPtr
        );

        if (err !== KMB_OK) {
          const msg = lib.kmb_error_message(err);
          throwForErrorCode(err, msg);
        }

        const streamId = ref.readUInt64LE(streamIdPtr, 0);
        resolve(streamId);
      } catch (error) {
        reject(error);
      }
    });
  }

  /**
   * Append events to a stream.
   *
   * @param streamId - Target stream identifier
   * @param events - List of event payloads (raw bytes)
   * @returns Offset of first appended event
   * @throws {StreamNotFoundError} If stream does not exist
   * @throws {PermissionDeniedError} If write not permitted
   */
  async append(streamId: StreamId, events: Buffer[]): Promise<Offset> {
    return new Promise((resolve, reject) => {
      try {
        this.checkConnected();

        if (events.length === 0) {
          throw new Error('Cannot append empty event list');
        }

        // Prepare event arrays
        const eventPtrs = Buffer.alloc(events.length * ref.sizeof.pointer);
        const eventLengths = Buffer.alloc(events.length * ref.sizeof.size_t);

        events.forEach((event, i) => {
          ref.writePointer(eventPtrs, i * ref.sizeof.pointer, event);
          ref.writeUInt64LE(eventLengths, i * ref.sizeof.size_t, event.length);
        });

        const firstOffsetPtr = ref.alloc(uint64);
        const err = lib.kmb_client_append(
          this.handle!,
          streamId,
          eventPtrs,
          eventLengths,
          events.length,
          firstOffsetPtr
        );

        if (err !== KMB_OK) {
          const msg = lib.kmb_error_message(err);
          throwForErrorCode(err, msg);
        }

        const firstOffset = ref.readUInt64LE(firstOffsetPtr, 0);
        resolve(firstOffset);
      } catch (error) {
        reject(error);
      }
    });
  }

  /**
   * Read events from a stream.
   *
   * @param streamId - Source stream identifier
   * @param options - Read options
   * @returns List of events with offsets and data
   * @throws {StreamNotFoundError} If stream does not exist
   * @throws {PermissionDeniedError} If read not permitted
   */
  async read(
    streamId: StreamId,
    options: { fromOffset?: Offset; maxBytes?: number } = {}
  ): Promise<Event[]> {
    return new Promise((resolve, reject) => {
      try {
        this.checkConnected();

        const fromOffset = options.fromOffset ?? 0n;
        const maxBytes = options.maxBytes ?? 1024 * 1024; // 1 MB default

        const resultPtrPtr = ref.alloc(ref.refType(KmbReadResult));
        const err = lib.kmb_client_read_events(
          this.handle!,
          streamId,
          fromOffset,
          maxBytes,
          resultPtrPtr
        );

        if (err !== KMB_OK) {
          const msg = lib.kmb_error_message(err);
          throwForErrorCode(err, msg);
        }

        const resultPtr = ref.readPointer(resultPtrPtr, 0);
        const result = ref.deref(resultPtr) as typeof KmbReadResult;

        const events: Event[] = [];
        for (let i = 0; i < result.event_count; i++) {
          const eventPtr = ref.readPointer(result.events, i * ref.sizeof.pointer);
          const eventLen = ref.readUInt64LE(result.event_lengths, i * ref.sizeof.size_t);

          const data = ref.reinterpret(eventPtr, eventLen);
          const offset = fromOffset + BigInt(i);

          events.push({ offset, data });
        }

        // Free result
        lib.kmb_read_result_free(resultPtr);

        resolve(events);
      } catch (error) {
        reject(error);
      }
    });
  }

  /**
   * Execute a SQL query against current state.
   *
   * @param sql - SQL query string (use $1, $2, $3 for parameters)
   * @param params - Query parameters (optional)
   * @returns QueryResult with columns and rows
   * @throws {QuerySyntaxError} If SQL is invalid
   * @throws {QueryExecutionError} If execution fails
   *
   * @example
   * ```typescript
   * const result = await client.query(
   *   'SELECT * FROM users WHERE id = $1',
   *   [ValueBuilder.bigint(42)]
   * );
   * for (const row of result.rows) {
   *   console.log(`ID: ${row[0]}, Name: ${row[1]}`);
   * }
   * ```
   */
  async query(sql: string, params: Value[] = []): Promise<QueryResult> {
    return new Promise((resolve, reject) => {
      try {
        this.checkConnected();

        // Convert params to FFI format
        let paramsPtr: Buffer | null = null;
        if (params.length > 0) {
          const paramsBuf = Buffer.alloc(params.length * KmbQueryParam.size);
          params.forEach((param, i) => {
            const ffiParam = this.valueToParam(param);
            ffiParam.ref().copy(paramsBuf, i * KmbQueryParam.size);
          });
          paramsPtr = paramsBuf;
        }

        // Call FFI
        const resultPtrPtr = ref.alloc(ref.refType(KmbQueryResult));
        const err = lib.kmb_client_query(
          this.handle!,
          sql,
          paramsPtr,
          params.length,
          resultPtrPtr
        );

        if (err !== KMB_OK) {
          const msg = lib.kmb_error_message(err);
          throwForErrorCode(err, msg);
        }

        const resultPtr = ref.readPointer(resultPtrPtr, 0);
        const result = this.parseQueryResult(resultPtr);

        // Free result
        lib.kmb_query_result_free(resultPtr);

        resolve(result);
      } catch (error) {
        reject(error);
      }
    });
  }

  /**
   * Execute a SQL query at a specific log position (point-in-time).
   *
   * Critical for compliance: Query historical state for audits.
   *
   * @param sql - SQL query string (use $1, $2, $3 for parameters)
   * @param params - Query parameters (optional)
   * @param position - Log position (offset) to query at
   * @returns QueryResult as of that point in time
   * @throws {QuerySyntaxError} If SQL is invalid
   * @throws {QueryExecutionError} If execution fails
   * @throws {PositionAheadError} If position is in the future
   *
   * @example
   * ```typescript
   * // Capture current position
   * const offset = 1000n;
   * // Query state as of that position
   * const result = await client.queryAt(
   *   'SELECT COUNT(*) FROM users',
   *   [],
   *   offset
   * );
   * ```
   */
  async queryAt(
    sql: string,
    params: Value[],
    position: Offset
  ): Promise<QueryResult> {
    return new Promise((resolve, reject) => {
      try {
        this.checkConnected();

        // Convert params to FFI format
        let paramsPtr: Buffer | null = null;
        if (params.length > 0) {
          const paramsBuf = Buffer.alloc(params.length * KmbQueryParam.size);
          params.forEach((param, i) => {
            const ffiParam = this.valueToParam(param);
            ffiParam.ref().copy(paramsBuf, i * KmbQueryParam.size);
          });
          paramsPtr = paramsBuf;
        }

        // Call FFI
        const resultPtrPtr = ref.alloc(ref.refType(KmbQueryResult));
        const err = lib.kmb_client_query_at(
          this.handle!,
          sql,
          paramsPtr,
          params.length,
          position,
          resultPtrPtr
        );

        if (err !== KMB_OK) {
          const msg = lib.kmb_error_message(err);
          throwForErrorCode(err, msg);
        }

        const resultPtr = ref.readPointer(resultPtrPtr, 0);
        const result = this.parseQueryResult(resultPtr);

        // Free result
        lib.kmb_query_result_free(resultPtr);

        resolve(result);
      } catch (error) {
        reject(error);
      }
    });
  }

  /**
   * Execute DDL/DML statement (CREATE TABLE, INSERT, UPDATE, DELETE).
   *
   * @param sql - SQL statement (use $1, $2, $3 for parameters)
   * @param params - Query parameters (optional)
   * @returns Number of rows affected (0 for DDL)
   * @throws {QuerySyntaxError} If SQL is invalid
   * @throws {QueryExecutionError} If execution fails
   *
   * @example
   * ```typescript
   * // DDL
   * await client.execute('CREATE TABLE users (id BIGINT PRIMARY KEY, name TEXT)');
   *
   * // DML with parameters
   * await client.execute(
   *   'INSERT INTO users (id, name) VALUES ($1, $2)',
   *   [ValueBuilder.bigint(1), ValueBuilder.text('Alice')]
   * );
   *
   * // UPDATE with RETURNING
   * const result = await client.query(
   *   'UPDATE users SET name = $2 WHERE id = $1 RETURNING *',
   *   [ValueBuilder.bigint(1), ValueBuilder.text('Bob')]
   * );
   * ```
   */
  async execute(sql: string, params: Value[] = []): Promise<number> {
    const result = await this.query(sql, params);
    return result.rows.length;
  }

  /**
   * Convert a TypeScript Value to FFI KmbQueryParam.
   */
  private valueToParam(val: Value): typeof KmbQueryParam {
    const param = new KmbQueryParam();

    switch (val.type) {
      case ValueType.Null:
        param.param_type = 0; // KmbParamNull
        break;
      case ValueType.BigInt:
        param.param_type = 1; // KmbParamBigInt
        ref.writeInt64LE(param.ref(), 4, val.value);
        break;
      case ValueType.Text:
        param.param_type = 2; // KmbParamText
        param.text_val = Buffer.from(val.value + '\0', 'utf-8');
        break;
      case ValueType.Boolean:
        param.param_type = 3; // KmbParamBoolean
        param.bool_val = val.value ? 1 : 0;
        break;
      case ValueType.Timestamp:
        param.param_type = 4; // KmbParamTimestamp
        ref.writeInt64LE(param.ref(), 20, val.value);
        break;
    }

    return param;
  }

  /**
   * Parse FFI KmbQueryResult to TypeScript QueryResult.
   */
  private parseQueryResult(resultPtr: Buffer): QueryResult {
    const result = ref.deref(resultPtr) as typeof KmbQueryResult;

    // Extract columns
    const columns: string[] = [];
    for (let i = 0; i < result.column_count; i++) {
      const colPtr = ref.readPointer(result.columns, i * ref.sizeof.pointer);
      const colName = ref.readCString(colPtr, 0);
      columns.push(colName);
    }

    // Extract rows
    const rows: Value[][] = [];
    for (let i = 0; i < result.row_count; i++) {
      const rowPtr = ref.readPointer(result.rows, i * ref.sizeof.pointer);
      const rowLen = ref.readUInt64LE(result.row_lengths, i * ref.sizeof.size_t);

      const row: Value[] = [];
      for (let j = 0; j < rowLen; j++) {
        const valueOffset = j * KmbQueryValue.size;
        const valueBuf = ref.reinterpret(rowPtr, KmbQueryValue.size, valueOffset);
        const value = this.parseQueryValue(valueBuf);
        row.push(value);
      }
      rows.push(row);
    }

    return { columns, rows };
  }

  /**
   * Parse FFI KmbQueryValue to TypeScript Value.
   */
  private parseQueryValue(valueBuf: Buffer): Value {
    const valueType = ref.readInt32LE(valueBuf, 0);

    switch (valueType) {
      case 0: // KmbValueNull
        return { type: ValueType.Null };
      case 1: // KmbValueBigInt
        const bigintVal = ref.readInt64LE(valueBuf, 4);
        return { type: ValueType.BigInt, value: bigintVal };
      case 2: // KmbValueText
        const textPtr = ref.readPointer(valueBuf, 12);
        if (textPtr.address() === 0) {
          return { type: ValueType.Null };
        }
        const text = ref.readCString(textPtr, 0);
        return { type: ValueType.Text, value: text };
      case 3: // KmbValueBoolean
        const boolVal = ref.readInt32LE(valueBuf, 20);
        return { type: ValueType.Boolean, value: boolVal !== 0 };
      case 4: // KmbValueTimestamp
        const timestampVal = ref.readInt64LE(valueBuf, 24);
        return { type: ValueType.Timestamp, value: timestampVal };
      default:
        throw new Error(`Unknown query value type: ${valueType}`);
    }
  }
}
