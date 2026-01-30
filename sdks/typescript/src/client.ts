/**
 * High-level Kimberlite client with Promise-based async API.
 */

import * as ref from 'ref-napi';
import { lib, KmbClientConfig, KmbReadResult, uint64, size_t, KMB_OK } from './ffi';
import { StreamId, Offset, TenantId, DataClass, Event, ClientConfig } from './types';
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
}
