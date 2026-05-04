/**
 * Unit tests for Kimberlite TypeScript client.
 */

import { Client } from '../src/client';
import { DataClass, StreamId } from '../src/types';
import {
  ConnectionError,
  ResponseTooLargeError,
  StreamNotFoundError,
  PermissionDeniedError,
  AuthenticationError,
} from '../src/errors';

describe('Client', () => {
  describe('Type Safety', () => {
    it('should enforce bigint for StreamId', () => {
      const streamId: bigint = 123n;
      expect(typeof streamId).toBe('bigint');
    });

    it('should enforce bigint for Offset', () => {
      const offset: bigint = 456n;
      expect(typeof offset).toBe('bigint');
    });

    it('should have DataClass enum with string values matching the native addon', () => {
      expect(DataClass.PHI).toBe('PHI');
      expect(DataClass.Deidentified).toBe('Deidentified');
      expect(DataClass.Public).toBe('Public');
      expect(DataClass.PCI).toBe('PCI');
    });
  });

  describe('Error Classes', () => {
    it('should create ConnectionError', () => {
      const err = new ConnectionError('Connection failed');
      expect(err).toBeInstanceOf(ConnectionError);
      expect(err).toBeInstanceOf(Error);
      expect(err.message).toBe('Connection failed');
      expect(err.name).toBe('ConnectionError');
    });

    it('should create StreamNotFoundError', () => {
      const err = new StreamNotFoundError('Stream not found');
      expect(err).toBeInstanceOf(StreamNotFoundError);
      expect(err.message).toBe('Stream not found');
    });

    it('should create PermissionDeniedError', () => {
      const err = new PermissionDeniedError('Permission denied');
      expect(err).toBeInstanceOf(PermissionDeniedError);
    });

    it('should create AuthenticationError', () => {
      const err = new AuthenticationError('Auth failed');
      expect(err).toBeInstanceOf(AuthenticationError);
    });
  });

  describe('Client Configuration', () => {
    it('should require addresses array', () => {
      const config = {
        addresses: ['localhost:5432'],
        tenantId: 1n,
      };
      expect(config.addresses).toHaveLength(1);
      expect(config.tenantId).toBe(1n);
    });

    it('should accept optional authToken', () => {
      const config = {
        addresses: ['localhost:5432'],
        tenantId: 1n,
        authToken: 'secret',
      };
      expect(config.authToken).toBe('secret');
    });

    it('should accept multiple addresses', () => {
      const config = {
        addresses: ['node1:5432', 'node2:5432', 'node3:5432'],
        tenantId: 1n,
      };
      expect(config.addresses).toHaveLength(3);
    });
  });

  describe('Buffer Handling', () => {
    it('should create Buffer from string', () => {
      const data = Buffer.from('{"type": "test"}');
      expect(Buffer.isBuffer(data)).toBe(true);
      expect(data.toString('utf-8')).toBe('{"type": "test"}');
    });

    it('should create Buffer from JSON', () => {
      const obj = { type: 'test', id: 123 };
      const data = Buffer.from(JSON.stringify(obj));
      const parsed = JSON.parse(data.toString('utf-8'));
      expect(parsed).toEqual(obj);
    });

    it('should handle array of Buffers', () => {
      const events = [
        Buffer.from('event1'),
        Buffer.from('event2'),
        Buffer.from('event3'),
      ];
      expect(events).toHaveLength(3);
      expect(Buffer.isBuffer(events[0])).toBe(true);
    });
  });

  describe('Event Structure', () => {
    it('should have correct Event interface', () => {
      const event: {
        offset: bigint;
        data: Buffer;
      } = {
        offset: 100n,
        data: Buffer.from('test'),
      };
      expect(event.offset).toBe(100n);
      expect(Buffer.isBuffer(event.data)).toBe(true);
    });
  });
});

describe('auto-reconnect', () => {
  /**
   * `Client.invoke()` is a private method, but exercising it through the
   * public surface requires a working native handle. We bypass that by
   * constructing a `Client` via its private constructor with a tiny fake
   * native — enough methods to stand in for the N-API addon, and a hook
   * that makes the next `append()` throw a broken-pipe error.
   */
  type FakeNative = {
    tenantId: bigint;
    lastRequestId: bigint | null;
    append: jest.Mock;
  };

  function makeFakeNative(behaviour: { failNextAppend: boolean }): FakeNative {
    return {
      tenantId: 1n,
      lastRequestId: null,
      append: jest.fn(async () => {
        if (behaviour.failNextAppend) {
          behaviour.failNextAppend = false;
          // Shape the native layer emits on Broken pipe — `wrapNativeError`
          // routes messages containing "connection error" to ConnectionError.
          throw new Error('connection error: Broken pipe (os error 32)');
        }
        return 7n;
      }),
    };
  }

  /** Call the private Client constructor without going through the native connect. */
  function newClientWith(native: FakeNative, autoReconnect: boolean): Client {
    const Ctor = Client as unknown as new (
      n: unknown,
      cfg: unknown,
      autoReconnect: boolean,
    ) => Client;
    return new Ctor(
      native,
      { address: 'localhost:5432', tenantId: 1n },
      autoReconnect,
    );
  }

  it('retries once on ConnectionError and succeeds', async () => {
    const first = makeFakeNative({ failNextAppend: true });
    const second = makeFakeNative({ failNextAppend: false });
    const client = newClientWith(first, true);

    // Intercept `reconnect()` so we don't try to touch the real native layer.
    const anyClient = client as unknown as { reconnect: () => Promise<void>; native: unknown; _reconnectCount: number };
    const origReconnect = anyClient.reconnect.bind(client);
    jest.spyOn(anyClient, 'reconnect').mockImplementation(async () => {
      anyClient.native = second;
      anyClient._reconnectCount += 1;
    });

    const offset = await client.append(StreamId.from(42n), [Buffer.from('hello')], 0n);
    expect(offset).toBe(7n);
    expect(client.reconnectCount).toBe(1);
    expect(first.append).toHaveBeenCalledTimes(1);
    expect(second.append).toHaveBeenCalledTimes(1);

    // Silence unused-binding warning while keeping the reference for future edits.
    void origReconnect;
  });

  it('does not retry when autoReconnect: false', async () => {
    const first = makeFakeNative({ failNextAppend: true });
    const client = newClientWith(first, false);

    await expect(client.append(StreamId.from(42n), [Buffer.from('hello')], 0n)).rejects.toThrow(
      ConnectionError,
    );
    expect(first.append).toHaveBeenCalledTimes(1);
    expect(client.reconnectCount).toBe(0);
  });
});

describe('readAll (v0.6.2)', () => {
  /**
   * Stub the native readEvents endpoint with a scripted sequence of
   * batches. Each batch is `{ events: Buffer[], nextOffset: bigint | null }` —
   * `null` signals end-of-stream.
   */
  type Batch = { events: Buffer[]; nextOffset: bigint | null };

  function makeReadStub(batches: Batch[]) {
    const calls: Array<{ from: bigint; max: bigint }> = [];
    let i = 0;
    const native = {
      tenantId: 1n,
      lastRequestId: null,
      readEvents: jest.fn(async (_sid: bigint, from: bigint, max: bigint) => {
        calls.push({ from, max });
        if (i >= batches.length) {
          return { events: [], nextOffset: null };
        }
        return batches[i++];
      }),
    };
    return { native, calls };
  }

  function newClient(native: unknown): Client {
    const Ctor = Client as unknown as new (
      n: unknown,
      cfg: unknown,
      autoReconnect: boolean,
    ) => Client;
    return new Ctor(native, { address: 'localhost:5432', tenantId: 1n }, false);
  }

  it('yields nothing for an empty stream', async () => {
    const { native } = makeReadStub([{ events: [], nextOffset: null }]);
    const client = newClient(native);

    const out: Array<{ offset: bigint; size: number }> = [];
    for await (const ev of client.readAll(StreamId.from(1n))) {
      out.push({ offset: ev.offset, size: ev.data.length });
    }
    expect(out).toEqual([]);
    expect(native.readEvents).toHaveBeenCalledTimes(1);
  });

  it('yields a single batch when the stream fits in one read', async () => {
    const events = [Buffer.from('a'), Buffer.from('bb'), Buffer.from('ccc')];
    const { native } = makeReadStub([{ events, nextOffset: null }]);
    const client = newClient(native);

    const out: Array<{ offset: bigint; data: string }> = [];
    for await (const ev of client.readAll(StreamId.from(1n))) {
      out.push({ offset: ev.offset, data: ev.data.toString() });
    }
    expect(out).toEqual([
      { offset: 0n, data: 'a' },
      { offset: 1n, data: 'bb' },
      { offset: 2n, data: 'ccc' },
    ]);
    expect(native.readEvents).toHaveBeenCalledTimes(1);
  });

  it('paginates across multiple batches and preserves offset continuity', async () => {
    const { native, calls } = makeReadStub([
      { events: [Buffer.from('e0'), Buffer.from('e1')], nextOffset: 2n },
      { events: [Buffer.from('e2')], nextOffset: 3n },
      { events: [Buffer.from('e3'), Buffer.from('e4')], nextOffset: null },
    ]);
    const client = newClient(native);

    const out: bigint[] = [];
    for await (const ev of client.readAll(StreamId.from(1n))) {
      out.push(ev.offset);
    }
    expect(out).toEqual([0n, 1n, 2n, 3n, 4n]);
    // Three native calls: from=0, from=2, from=3.
    expect(calls.map((c) => c.from)).toEqual([0n, 2n, 3n]);
  });

  it('honours fromOffset when provided', async () => {
    const { native, calls } = makeReadStub([
      { events: [Buffer.from('x')], nextOffset: null },
    ]);
    const client = newClient(native);

    const out: bigint[] = [];
    for await (const ev of client.readAll(StreamId.from(1n), { fromOffset: 100n })) {
      out.push(ev.offset);
    }
    expect(out).toEqual([100n]);
    expect(calls[0].from).toBe(100n);
  });

  it('passes batchSize to the native maxBytes argument', async () => {
    const { native, calls } = makeReadStub([
      { events: [Buffer.from('x')], nextOffset: null },
    ]);
    const client = newClient(native);

    for await (const _ of client.readAll(StreamId.from(1n), { batchSize: 4096 })) {
      // exhaust
    }
    expect(calls[0].max).toBe(4096n);
  });

  it('throws when the server reports a non-empty nextOffset but returns zero events', async () => {
    // Simulates a single event larger than batchSize: server says
    // "more data ahead at offset N" but the empty batch means we
    // can't make progress without raising the budget.
    const { native } = makeReadStub([
      { events: [], nextOffset: 5n },
    ]);
    const client = newClient(native);

    await expect(async () => {
      for await (const _ of client.readAll(StreamId.from(1n), { batchSize: 1 })) {
        // unreachable
      }
    }).rejects.toThrow(/larger than batchSize/);
  });

  it('terminates cleanly when the consumer breaks out early', async () => {
    const { native } = makeReadStub([
      { events: [Buffer.from('a'), Buffer.from('b')], nextOffset: 2n },
      { events: [Buffer.from('c')], nextOffset: null },
    ]);
    const client = newClient(native);

    const out: bigint[] = [];
    for await (const ev of client.readAll(StreamId.from(1n))) {
      out.push(ev.offset);
      if (out.length === 1) break;
    }
    expect(out).toEqual([0n]);
    // Generator returned after the first yield — only the first
    // native call should have been issued.
    expect(native.readEvents).toHaveBeenCalledTimes(1);
  });
});

describe('ResponseTooLargeError dispatch (v0.6.2)', () => {
  it('wrapNativeError routes the v0.6.2 response-too-large prefix to ResponseTooLargeError', () => {
    const { wrapNativeError } = require('../src/errors');
    const raw = new Error(
      '[KMB_ERR_Connection] response too large: 9000000 bytes received exceeds framing cap of 8388608 bytes (= 2 * bufferSizeBytes). Increase `bufferSizeBytes`, lower `maxBytes`, or use `client.readAll()` for full-stream replay.',
    );
    const wrapped = wrapNativeError(raw);
    expect(wrapped).toBeInstanceOf(ResponseTooLargeError);
    expect(wrapped).toBeInstanceOf(ConnectionError);
    expect((wrapped as Error).message).toMatch(/response too large/);
    expect((wrapped as Error).message).toMatch(/bufferSizeBytes/);
    expect((wrapped as Error).message).toMatch(/readAll/);
  });

  it('legacy (no KMB_ERR_ prefix) message also dispatches to ResponseTooLargeError', () => {
    const { wrapNativeError } = require('../src/errors');
    const raw = new Error('connection error: response too large: 5MB exceeds 2MB');
    const wrapped = wrapNativeError(raw);
    expect(wrapped).toBeInstanceOf(ResponseTooLargeError);
  });

  it('plain ConnectionError still routes to ConnectionError', () => {
    const { wrapNativeError } = require('../src/errors');
    const raw = new Error('[KMB_ERR_Connection] Broken pipe (os error 32)');
    const wrapped = wrapNativeError(raw);
    expect(wrapped).toBeInstanceOf(ConnectionError);
    expect(wrapped).not.toBeInstanceOf(ResponseTooLargeError);
  });
});

describe('Integration Tests (require running server)', () => {
  let client: Client | null = null;

  beforeEach(() => {
    // Reset client before each test
    client = null;
  });

  afterEach(async () => {
    // Clean up client after each test
    if (client) {
      try {
        await client.disconnect();
      } catch (e) {
        // Ignore disconnect errors in cleanup
      }
      client = null;
    }
  });

  describe('Connection', () => {
    it('should fail to connect to non-existent server', async () => {
      await expect(
        Client.connect({
          addresses: ['localhost:9999'],
          tenantId: 1n,
        })
      ).rejects.toThrow(ConnectionError);
    });

    it('should validate addresses array is not empty', async () => {
      await expect(
        Client.connect({
          addresses: [],
          tenantId: 1n,
        })
      ).rejects.toThrow();
    });
  });

  // These tests require a running kmb-server instance
  describe.skip('Live Server Tests', () => {
    beforeEach(async () => {
      try {
        client = await Client.connect({
          addresses: ['localhost:5432'],
          tenantId: 1n,
        });
      } catch (e) {
        throw new Error('Server not available for integration tests');
      }
    });

    it('should connect successfully', () => {
      expect(client).not.toBeNull();
    });

    it('should create stream', async () => {
      if (!client) throw new Error('Client not initialized');

      const streamId = await client.createStream('test_stream', DataClass.Public);
      expect(streamId).toBeGreaterThan(0n);
    });

    it('should append events', async () => {
      if (!client) throw new Error('Client not initialized');

      const streamId = await client.createStream('test_append', DataClass.Public);
      const events = [
        Buffer.from(JSON.stringify({ type: 'test', id: 1 })),
        Buffer.from(JSON.stringify({ type: 'test', id: 2 })),
      ];

      const offset = await client.append(streamId, events);
      expect(offset).toBeGreaterThanOrEqual(0n);
    });

    it('should read events', async () => {
      if (!client) throw new Error('Client not initialized');

      const streamId = await client.createStream('test_read', DataClass.Public);
      const writeEvents = [
        Buffer.from(JSON.stringify({ type: 'test', id: 1 })),
        Buffer.from(JSON.stringify({ type: 'test', id: 2 })),
      ];

      const firstOffset = await client.append(streamId, writeEvents);
      const readEvents = await client.read(streamId, {
        fromOffset: firstOffset,
        maxBytes: 1024,
      });

      expect(readEvents.length).toBeGreaterThan(0);
      expect(readEvents[0].offset).toBe(firstOffset);
      expect(Buffer.isBuffer(readEvents[0].data)).toBe(true);
    });

    it('should handle stream not found', async () => {
      if (!client) throw new Error('Client not initialized');

      const nonExistentStreamId = StreamId.from(999999n);
      await expect(
        client.read(nonExistentStreamId, { fromOffset: 0n })
      ).rejects.toThrow(StreamNotFoundError);
    });

    it('should handle JSON serialization', async () => {
      if (!client) throw new Error('Client not initialized');

      const streamId = await client.createStream('test_json', DataClass.Public);
      const data = {
        type: 'user_action',
        user_id: 12345,
        action: 'login',
        timestamp: new Date().toISOString(),
      };

      const events = [Buffer.from(JSON.stringify(data))];
      const offset = await client.append(streamId, events);

      const readEvents = await client.read(streamId, { fromOffset: offset });
      const parsed = JSON.parse(readEvents[0].data.toString('utf-8'));

      expect(parsed).toEqual(data);
    });

    it('should handle batch operations', async () => {
      if (!client) throw new Error('Client not initialized');

      const streamId = await client.createStream('test_batch', DataClass.Public);
      const batchSize = 100;
      const events = Array.from({ length: batchSize }, (_, i) =>
        Buffer.from(JSON.stringify({ id: i, timestamp: Date.now() }))
      );

      const offset = await client.append(streamId, events);
      expect(offset).toBeGreaterThanOrEqual(0n);

      const readEvents = await client.read(streamId, {
        fromOffset: offset,
        maxBytes: 1024 * 1024,
      });
      expect(readEvents.length).toBe(batchSize);
    });

    it('should disconnect cleanly', async () => {
      if (!client) throw new Error('Client not initialized');

      await client.disconnect();
      client = null;

      // Should not throw
      expect(true).toBe(true);
    });
  });
});
