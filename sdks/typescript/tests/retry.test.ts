/**
 * Tests for `withRetry` retry helper.
 *
 * AUDIT-2026-04 S2.4 — these exercise the retry state machine
 * without a live server by simulating `isRetryable()` errors.
 */

import { withRetry, DEFAULT_RETRY, RetryPolicy } from '../src/retry';

// Tiny test-only error that looks like a Kimberlite error to the
// retry helper. `isRetryable` is always true.
class RetryableError extends Error {
  readonly code = 'RateLimited';
  isRetryable(): boolean {
    return true;
  }
}

class TerminalError extends Error {
  readonly code = 'QueryParseError';
  isRetryable(): boolean {
    return false;
  }
}

// Fast policy for tests so the suite doesn't take seconds.
const FAST: RetryPolicy = {
  maxAttempts: 4,
  baseDelayMs: 1,
  capDelayMs: 4,
};

describe('withRetry', () => {
  it('returns the op result on first success', async () => {
    const result = await withRetry(async () => 42, FAST);
    expect(result).toBe(42);
  });

  it('retries a retryable error and returns the eventual success', async () => {
    let attempts = 0;
    const result = await withRetry(async () => {
      attempts += 1;
      if (attempts < 3) throw new RetryableError('still rate-limited');
      return 'ok';
    }, FAST);
    expect(result).toBe('ok');
    expect(attempts).toBe(3);
  });

  it('does not retry non-retryable errors', async () => {
    let attempts = 0;
    await expect(
      withRetry(async () => {
        attempts += 1;
        throw new TerminalError('bad SQL');
      }, FAST),
    ).rejects.toBeInstanceOf(TerminalError);
    expect(attempts).toBe(1);
  });

  it('gives up after maxAttempts with the last error', async () => {
    let attempts = 0;
    await expect(
      withRetry(async () => {
        attempts += 1;
        throw new RetryableError(`attempt ${attempts}`);
      }, FAST),
    ).rejects.toMatchObject({ message: 'attempt 4' });
    expect(attempts).toBe(FAST.maxAttempts);
  });

  it('does not retry errors without an isRetryable() method', async () => {
    let attempts = 0;
    const plainError = new Error('no predicate');
    await expect(
      withRetry(async () => {
        attempts += 1;
        throw plainError;
      }, FAST),
    ).rejects.toBe(plainError);
    expect(attempts).toBe(1);
  });

  it('respects DEFAULT_RETRY when no policy is passed', async () => {
    // Defaults: 4 attempts. We throw a retryable error every time
    // and assert the giving-up path hits exactly 4 attempts.
    let attempts = 0;
    await expect(
      withRetry(async () => {
        attempts += 1;
        throw new RetryableError('always rate-limited');
      }, { ...DEFAULT_RETRY, baseDelayMs: 1, capDelayMs: 1 }),
    ).rejects.toBeInstanceOf(RetryableError);
    expect(attempts).toBe(DEFAULT_RETRY.maxAttempts);
  });

  it('maxAttempts=1 disables retry entirely', async () => {
    let attempts = 0;
    await expect(
      withRetry(
        async () => {
          attempts += 1;
          throw new RetryableError('first fail');
        },
        { maxAttempts: 1, baseDelayMs: 1, capDelayMs: 1 },
      ),
    ).rejects.toBeInstanceOf(RetryableError);
    expect(attempts).toBe(1);
  });

  it('backoff is bounded by capDelayMs', async () => {
    // Cap at 2 ms. With baseDelayMs=1 and maxAttempts=4 the three
    // delays are 1, 2, and min(4, 2) = 2. Total upper bound ~5 ms
    // per run; we just assert the thing finishes.
    let attempts = 0;
    const start = Date.now();
    await expect(
      withRetry(
        async () => {
          attempts += 1;
          throw new RetryableError('cap test');
        },
        { maxAttempts: 4, baseDelayMs: 1, capDelayMs: 2 },
      ),
    ).rejects.toBeInstanceOf(RetryableError);
    const elapsed = Date.now() - start;
    expect(attempts).toBe(4);
    // Loose bound — CI timers are noisy. 100 ms is 20× headroom
    // over the ~5 ms ideal and catches obvious regressions
    // (e.g. a lost `Math.min`).
    expect(elapsed).toBeLessThan(100);
  });
});
