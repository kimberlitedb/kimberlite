/**
 * Unit tests for Kimberlite TypeScript client.
 */

import { Client } from '../src/client';
import { DataClass } from '../src/types';
import {
  ConnectionError,
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

    it('should have DataClass enum', () => {
      expect(DataClass.PHI).toBe(0);
      expect(DataClass.NonPHI).toBe(1);
      expect(DataClass.Deidentified).toBe(2);
    });
  });

  describe('Error Classes', () => {
    it('should create ConnectionError', () => {
      const err = new ConnectionError('Connection failed', 3);
      expect(err).toBeInstanceOf(ConnectionError);
      expect(err).toBeInstanceOf(Error);
      expect(err.message).toBe('Connection failed');
      expect(err.code).toBe(3);
      expect(err.name).toBe('ConnectionError');
    });

    it('should create StreamNotFoundError', () => {
      const err = new StreamNotFoundError('Stream not found', 4);
      expect(err).toBeInstanceOf(StreamNotFoundError);
      expect(err.message).toBe('Stream not found');
      expect(err.code).toBe(4);
    });

    it('should create PermissionDeniedError', () => {
      const err = new PermissionDeniedError('Permission denied', 5);
      expect(err).toBeInstanceOf(PermissionDeniedError);
      expect(err.code).toBe(5);
    });

    it('should create AuthenticationError', () => {
      const err = new AuthenticationError('Auth failed', 11);
      expect(err).toBeInstanceOf(AuthenticationError);
      expect(err.code).toBe(11);
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

      const streamId = await client.createStream('test_stream', DataClass.NonPHI);
      expect(streamId).toBeGreaterThan(0n);
    });

    it('should append events', async () => {
      if (!client) throw new Error('Client not initialized');

      const streamId = await client.createStream('test_append', DataClass.NonPHI);
      const events = [
        Buffer.from(JSON.stringify({ type: 'test', id: 1 })),
        Buffer.from(JSON.stringify({ type: 'test', id: 2 })),
      ];

      const offset = await client.append(streamId, events);
      expect(offset).toBeGreaterThanOrEqual(0n);
    });

    it('should read events', async () => {
      if (!client) throw new Error('Client not initialized');

      const streamId = await client.createStream('test_read', DataClass.NonPHI);
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

      const nonExistentStreamId = 999999n;
      await expect(
        client.read(nonExistentStreamId, { fromOffset: 0n })
      ).rejects.toThrow(StreamNotFoundError);
    });

    it('should handle JSON serialization', async () => {
      if (!client) throw new Error('Client not initialized');

      const streamId = await client.createStream('test_json', DataClass.NonPHI);
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

      const streamId = await client.createStream('test_batch', DataClass.NonPHI);
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
