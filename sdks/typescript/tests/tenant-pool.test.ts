/**
 * Tests for `TenantPool`.
 *
 * AUDIT-2026-04 S2.4 — uses a stub factory and a controllable
 * clock so the full pool behaviour (LRU, idle eviction,
 * deduplication) is exercised without live connections.
 */

import { TenantPool } from '../src/tenant-pool';

// Fake Client — just needs `disconnect()`.
interface FakeClient {
  id: bigint;
  closed: boolean;
  disconnect: () => Promise<void>;
}

function makeFakeClient(id: bigint): FakeClient {
  const c: FakeClient = {
    id,
    closed: false,
    async disconnect() {
      c.closed = true;
    },
  };
  return c;
}

describe('TenantPool', () => {
  it('acquire creates a new client on first call', async () => {
    const factory = jest.fn(async (id: bigint) => makeFakeClient(id) as any);
    const pool = new TenantPool({ factory, maxSize: 8, idleTimeoutMs: 0 });
    const c = await pool.acquire(1n);
    expect(c).toBeDefined();
    expect(factory).toHaveBeenCalledTimes(1);
    expect(factory).toHaveBeenCalledWith(1n);
    expect(pool.stats().size).toBe(1);
    expect(pool.stats().hits).toBe(0);
    expect(pool.stats().misses).toBe(1);
  });

  it('acquire returns cached client on second call', async () => {
    const factory = jest.fn(async (id: bigint) => makeFakeClient(id) as any);
    const pool = new TenantPool({ factory, maxSize: 8, idleTimeoutMs: 0 });
    const a = await pool.acquire(1n);
    const b = await pool.acquire(1n);
    expect(a).toBe(b);
    expect(factory).toHaveBeenCalledTimes(1);
    expect(pool.stats().hits).toBe(1);
  });

  it('separate tenants each get their own client', async () => {
    const factory = jest.fn(async (id: bigint) => makeFakeClient(id) as any);
    const pool = new TenantPool({ factory, maxSize: 8, idleTimeoutMs: 0 });
    const a = await pool.acquire(1n);
    const b = await pool.acquire(2n);
    expect(a).not.toBe(b);
    expect(pool.stats().size).toBe(2);
  });

  it('LRU evicts the least-recently-used client when over maxSize', async () => {
    let t = 0;
    const clock = () => t;
    const clients: FakeClient[] = [];
    const factory = jest.fn(async (id: bigint) => {
      const c = makeFakeClient(id);
      clients.push(c);
      return c as any;
    });
    const pool = new TenantPool({
      factory,
      maxSize: 2,
      idleTimeoutMs: 0,
      now: clock,
    });

    await pool.acquire(1n);
    t = 10;
    await pool.acquire(2n);
    t = 20;
    // Touch tenant 1 → becomes most-recent.
    await pool.acquire(1n);

    // Insert tenant 3 → should evict tenant 2 (LRU).
    t = 30;
    await pool.acquire(3n);

    const byTenant = new Map(clients.map((c) => [c.id, c]));
    expect(byTenant.get(2n)!.closed).toBe(true);
    expect(byTenant.get(1n)!.closed).toBe(false);
    expect(byTenant.get(3n)!.closed).toBe(false);
    expect(pool.stats().size).toBe(2);
    expect(pool.stats().evictions).toBe(1);
  });

  it('idle eviction drops stale clients on next acquire', async () => {
    let t = 0;
    const clock = () => t;
    const clients: FakeClient[] = [];
    const factory = jest.fn(async (id: bigint) => {
      const c = makeFakeClient(id);
      clients.push(c);
      return c as any;
    });
    const pool = new TenantPool({
      factory,
      maxSize: 8,
      idleTimeoutMs: 100,
      now: clock,
    });

    // Tenant 1 acquired at t=0.
    await pool.acquire(1n);
    // Tenant 2 acquired at t=80 — well inside idle window.
    t = 80;
    await pool.acquire(2n);

    // Advance to t=150. cutoff = 150-100 = 50.
    //   tenant 1 lastUsedAt=0  → 0 < 50 → evict.
    //   tenant 2 lastUsedAt=80 → 80 ≥ 50 → keep.
    t = 150;
    await pool.acquire(3n);

    const byTenant = new Map(clients.map((c) => [c.id, c]));
    expect(byTenant.get(1n)!.closed).toBe(true);
    expect(byTenant.get(2n)!.closed).toBe(false);
    expect(pool.stats().idleEvictions).toBe(1);
  });

  it('concurrent acquires for the same tenant dedupe to one factory call', async () => {
    let resolveFactory: ((c: any) => void) | null = null;
    const pending = new Promise<any>((resolve) => {
      resolveFactory = resolve;
    });
    const factory = jest.fn((_id: bigint) => pending);
    const pool = new TenantPool({ factory, maxSize: 8, idleTimeoutMs: 0 });

    const p1 = pool.acquire(1n);
    const p2 = pool.acquire(1n);
    const fake = makeFakeClient(1n);
    resolveFactory!(fake);
    const [a, b] = await Promise.all([p1, p2]);
    expect(a).toBe(b);
    expect(factory).toHaveBeenCalledTimes(1);
  });

  it('withClient hands over the acquired client and updates lastUsedAt', async () => {
    const factory = jest.fn(async (id: bigint) => makeFakeClient(id) as any);
    const pool = new TenantPool({ factory, maxSize: 8, idleTimeoutMs: 0 });
    const result = await pool.withClient(7n, async (c) => {
      expect(c).toBeDefined();
      return 'ok';
    });
    expect(result).toBe('ok');
    expect(factory).toHaveBeenCalledTimes(1);
  });

  it('close disconnects all cached clients and resets size', async () => {
    const clients: FakeClient[] = [];
    const factory = async (id: bigint) => {
      const c = makeFakeClient(id);
      clients.push(c);
      return c as any;
    };
    const pool = new TenantPool({ factory, maxSize: 8, idleTimeoutMs: 0 });
    await pool.acquire(1n);
    await pool.acquire(2n);
    await pool.close();
    expect(clients.every((c) => c.closed)).toBe(true);
    expect(pool.stats().size).toBe(0);
  });
});
