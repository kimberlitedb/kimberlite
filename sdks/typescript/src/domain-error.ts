/**
 * Domain-level error mapping for Kimberlite.
 *
 * AUDIT-2026-04 S2.4 — lifts `mapKimberliteError` out of
 * `notebar/packages/kimberlite-client/src/retry.ts` into the SDK
 * so every app (React Router V7 loaders, Express handlers, CLI
 * tools) gets a single canonical translation from wire-level
 * errors to app-visible domain-error shapes.
 *
 * Typical usage in an HTTP handler:
 *
 * ```ts
 * import { asResult, DomainError } from '@kimberlite/sdk';
 *
 * export async function loader({ params }) {
 *   const result = await asResult(() => client.query('...'));
 *   if (!result.ok) return renderErrorResponse(result.err);
 *   return json(result.value);
 * }
 * ```
 */

import { KimberliteError } from './errors';

/**
 * Discriminated union of domain-facing error shapes. A 1-1 mapping
 * from wire-level `ErrorCode` to the kind of failure an app UI or
 * HTTP endpoint needs to distinguish.
 */
export type DomainError =
  | { readonly kind: 'NotFound' }
  | { readonly kind: 'Forbidden' }
  | { readonly kind: 'ConcurrentModification' }
  | { readonly kind: 'Conflict'; readonly reasons: readonly string[] }
  | { readonly kind: 'InvariantViolation'; readonly name: string }
  | { readonly kind: 'Unavailable'; readonly message: string }
  | { readonly kind: 'RateLimited' }
  | { readonly kind: 'Timeout' }
  | { readonly kind: 'Validation'; readonly message: string };

/**
 * Translate any thrown value into a `DomainError`. Non-Kimberlite
 * errors fall through to `Unavailable` with the stringified
 * message — never opaque, never reveals stack frames.
 */
export function mapKimberliteError(e: unknown): DomainError {
  if (e instanceof KimberliteError) {
    switch (e.code) {
      case 'OffsetMismatch':
        return { kind: 'ConcurrentModification' };
      case 'StreamNotFound':
      case 'TableNotFound':
      case 'TenantNotFound':
      case 'ApiKeyNotFound':
        return { kind: 'NotFound' };
      case 'AuthenticationFailed':
        return { kind: 'Forbidden' };
      case 'RateLimited':
        return { kind: 'RateLimited' };
      case 'Timeout':
        return { kind: 'Timeout' };
      case 'QueryParseError':
      case 'InvalidRequest':
      case 'InvalidOffset':
        return {
          kind: 'Validation',
          message: e.message || 'invalid request',
        };
      case 'TenantAlreadyExists':
      case 'StreamAlreadyExists':
        return {
          kind: 'Conflict',
          reasons: [e.message || String(e.code)],
        };
      default:
        return {
          kind: 'Unavailable',
          message: e.message || 'unknown error',
        };
    }
  }
  if (typeof e === 'object' && e !== null && 'message' in e) {
    const msg = (e as { message?: unknown }).message;
    return { kind: 'Unavailable', message: typeof msg === 'string' ? msg : String(e) };
  }
  return { kind: 'Unavailable', message: String(e) };
}

/** Lightweight `Result<T, E>` type so apps can avoid throwing. */
export type Result<T, E> =
  | { readonly ok: true; readonly value: T }
  | { readonly ok: false; readonly err: E };

/**
 * Run `op` and translate any thrown error into a
 * `DomainError`. Returns a `Result` that HTTP handlers / React
 * Router V7 loaders can pattern-match on without a try/catch
 * around every call site.
 */
export async function asResult<T>(
  op: () => Promise<T>,
): Promise<Result<T, DomainError>> {
  try {
    return { ok: true, value: await op() };
  } catch (e) {
    return { ok: false, err: mapKimberliteError(e) };
  }
}
