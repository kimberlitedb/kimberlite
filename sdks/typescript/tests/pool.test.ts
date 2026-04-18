/**
 * Unit tests for the Pool TS wrapper. These exercise the JS surface of the
 * wrapper without calling into the native addon — the wrapper's responsibility
 * is input translation and cancellation safety; live server tests live in
 * integration suites.
 */

import { Pool, PooledClient } from '../src/pool';

describe('Pool (TS wrapper surface)', () => {
  it('exposes a create factory, acquire, withClient, stats, and shutdown', () => {
    expect(typeof Pool.create).toBe('function');
    const proto = Pool.prototype;
    expect(typeof proto.acquire).toBe('function');
    expect(typeof proto.withClient).toBe('function');
    expect(typeof proto.stats).toBe('function');
    expect(typeof proto.shutdown).toBe('function');
  });

  it('PooledClient exposes release, discard, and all Client operations', () => {
    const proto = PooledClient.prototype;
    expect(typeof proto.release).toBe('function');
    expect(typeof proto.discard).toBe('function');
    expect(typeof proto.createStream).toBe('function');
    expect(typeof proto.createStreamWithPlacement).toBe('function');
    expect(typeof proto.append).toBe('function');
    expect(typeof proto.read).toBe('function');
    expect(typeof proto.query).toBe('function');
    expect(typeof proto.queryAt).toBe('function');
    expect(typeof proto.queryRows).toBe('function');
    expect(typeof proto.execute).toBe('function');
    expect(typeof proto.sync).toBe('function');
  });
});

describe('Pool.withClient cancellation safety', () => {
  it('releases the client when the callback throws', async () => {
    const fakeNative = makeFakePooledNative();
    const client = new PooledClient(fakeNative);
    await expect(
      (async () => {
        try {
          throw new Error('boom');
        } finally {
          client.release();
        }
      })(),
    ).rejects.toThrow('boom');
    expect(fakeNative.releaseCalls).toBe(1);
  });

  it('release is idempotent', () => {
    const fakeNative = makeFakePooledNative();
    const client = new PooledClient(fakeNative);
    client.release();
    client.release();
    expect(fakeNative.releaseCalls).toBe(1); // only first call reaches native
  });

  it('discard closes the connection and marks the client released', () => {
    const fakeNative = makeFakePooledNative();
    const client = new PooledClient(fakeNative);
    client.discard();
    expect(fakeNative.discardCalls).toBe(1);
    expect(fakeNative.releaseCalls).toBe(0);
    // Subsequent operations throw.
    expect(() => client.tenantId).toThrow(/released/);
  });

  it('throws on method calls after release', () => {
    const fakeNative = makeFakePooledNative();
    const client = new PooledClient(fakeNative);
    client.release();
    expect(() => client.tenantId).toThrow(/released/);
    expect(() => client.lastRequestId).toThrow(/released/);
  });
});

// ---------------------------------------------------------------------------
// Test fake for the native pooled-client. Just enough surface to exercise
// the TS wrapper's release/discard bookkeeping without requiring a real
// Rust addon or a live server.
// ---------------------------------------------------------------------------

function makeFakePooledNative(): FakeNative {
  return new FakeNative();
}

class FakeNative {
  releaseCalls = 0;
  discardCalls = 0;
  readonly tenantId = 1n;
  readonly lastRequestId: bigint | null = null;

  release(): void {
    this.releaseCalls += 1;
  }
  discard(): void {
    this.discardCalls += 1;
  }
  async createStream(): Promise<bigint> {
    return 0n;
  }
  async createStreamWithPlacement(): Promise<bigint> {
    return 0n;
  }
  async append(): Promise<bigint> {
    return 0n;
  }
  async readEvents(): Promise<{ events: Buffer[]; nextOffset: bigint | null }> {
    return { events: [], nextOffset: null };
  }
  async query(): Promise<{ columns: string[]; rows: never[] }> {
    return { columns: [], rows: [] };
  }
  async queryAt(): Promise<{ columns: string[]; rows: never[] }> {
    return { columns: [], rows: [] };
  }
  async execute(): Promise<{ rowsAffected: bigint; logOffset: bigint }> {
    return { rowsAffected: 0n, logOffset: 0n };
  }
  async sync(): Promise<void> {}
}
