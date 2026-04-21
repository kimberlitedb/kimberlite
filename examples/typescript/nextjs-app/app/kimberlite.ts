/**
 * Shared Kimberlite pool for the Next.js example. Initialised lazily on
 * first request so the pool survives hot-reloads under `next dev`.
 */

import { Pool } from '@kimberlitedb/client';

let poolPromise: Promise<Pool> | null = null;

export function getPool(): Promise<Pool> {
  if (!poolPromise) {
    poolPromise = Pool.create({
      address: process.env.KIMBERLITE_ADDR ?? '127.0.0.1:5432',
      tenantId: BigInt(process.env.KIMBERLITE_TENANT_ID ?? '1'),
      maxSize: 8,
    });
  }
  return poolPromise;
}
