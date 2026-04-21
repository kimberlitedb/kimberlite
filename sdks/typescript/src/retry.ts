/**
 * Retry helper for Kimberlite operations.
 *
 * AUDIT-2026-04 S2.4 — ports notebar's `retry.ts` idiom into the
 * SDK so every app built on Kimberlite gets identical backoff
 * semantics without hand-rolling a wrapper.
 *
 * Usage:
 *
 * ```ts
 * import { Client, withRetry, DEFAULT_RETRY } from '@kimberlitedb/client';
 *
 * const rows = await withRetry(
 *   () => client.query('SELECT * FROM patients WHERE id = $1', [42n]),
 *   DEFAULT_RETRY,
 * );
 * ```
 */

/**
 * Exponential-backoff retry policy.
 *
 * - `maxAttempts` — total attempts INCLUDING the initial call. A
 *   value of 1 disables retries.
 * - `baseDelayMs` — delay before the first retry. Subsequent
 *   retries double this (capped by `capDelayMs`).
 * - `capDelayMs` — upper bound on the delay between attempts.
 */
export interface RetryPolicy {
  readonly maxAttempts: number;
  readonly baseDelayMs: number;
  readonly capDelayMs: number;
}

/**
 * Sensible default: four attempts, 50 ms → 100 ms → 200 ms → 400 ms.
 * Total worst-case wall-clock overhead is ~750 ms before the final
 * error surfaces, which fits the 2-second human-perception budget
 * for synchronous interactive calls.
 */
export const DEFAULT_RETRY: RetryPolicy = {
  maxAttempts: 4,
  baseDelayMs: 50,
  capDelayMs: 800,
};

interface WithRetryableFlag {
  readonly isRetryable?: () => boolean;
}

/**
 * True if `e` exposes an `isRetryable()` predicate that returns
 * true. Kimberlite's error classes all implement this (see
 * `KimberliteError.isRetryable` in `errors.ts`).
 */
function canRetry(e: unknown): boolean {
  if (typeof e !== 'object' || e === null) return false;
  const withFlag = e as WithRetryableFlag;
  if (typeof withFlag.isRetryable === 'function') {
    return withFlag.isRetryable();
  }
  return false;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

/**
 * Run `op` with exponential-backoff retries for errors whose
 * `isRetryable()` returns true. Non-retryable errors propagate
 * immediately.
 *
 * The backoff doubles from `baseDelayMs` up to `capDelayMs`:
 *   attempt 1 fails → wait baseDelayMs → attempt 2
 *   attempt 2 fails → wait 2 × baseDelayMs → attempt 3
 *   attempt 3 fails → wait min(4 × baseDelayMs, capDelayMs) → attempt 4
 *
 * Giving up at `maxAttempts` re-throws the most recent error.
 */
export async function withRetry<T>(
  op: () => Promise<T>,
  policy: RetryPolicy = DEFAULT_RETRY,
): Promise<T> {
  let attempt = 0;
  for (;;) {
    try {
      return await op();
    } catch (e) {
      attempt += 1;
      if (attempt >= policy.maxAttempts || !canRetry(e)) {
        throw e;
      }
      const wait = Math.min(
        policy.capDelayMs,
        policy.baseDelayMs * 2 ** (attempt - 1),
      );
      await delay(wait);
    }
  }
}
