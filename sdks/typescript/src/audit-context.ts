/**
 * Audit context propagation.
 *
 * AUDIT-2026-04 S2.4 — provides an ambient context carrier so apps
 * can set `{ actor, reason, requestId, correlationId }` once per
 * request and have every nested Kimberlite operation pick it up
 * for structured logging / distributed tracing.
 *
 * Uses Node's native `AsyncLocalStorage` so the context survives
 * `await` boundaries without manual threading. React Router V7
 * loaders typically set the context at the top of the loader and
 * let every SDK call read it transparently:
 *
 * ```ts
 * import { runWithAudit, AuditContext } from '@kimberlitedb/client';
 *
 * export async function loader({ request }) {
 *   const ctx: AuditContext = {
 *     actor: session.userId,
 *     reason: 'patient-chart-view',
 *     correlationId: request.headers.get('x-request-id') ?? undefined,
 *   };
 *   return runWithAudit(ctx, async () => {
 *     return json(await client.query('...'));
 *   });
 * }
 * ```
 *
 * **Wire propagation** — as of protocol v3 (AUDIT-2026-04 S3.9) the
 * audit context is attached to every outgoing `Request.audit` and
 * flows into the server's `ComplianceAuditLog`. The `Client.invoke`
 * wrapper reads the active context and stages it on the native
 * handle via `setAuditContext()` / `clearAuditContext()` around each
 * call so attribution survives the JS → N-API → Rust → wire path.
 */

import { AsyncLocalStorage } from 'node:async_hooks';

/**
 * Structured audit context carried through an async call chain.
 *
 * `actor` and `reason` are mandatory in regulated-industry apps
 * (HIPAA minimum-necessary, GDPR purpose limitation, FedRAMP
 * audit-trail completeness). `requestId` correlates with server
 * logs; `correlationId` ties together a span of related calls
 * (typically an HTTP trace ID).
 */
export interface AuditContext {
  /**
   * Identifier of the user/service that initiated the call.
   * Opaque string; apps typically use an IdentityId or email.
   */
  readonly actor: string;
  /**
   * Free-form reason — why this access is happening. Critical for
   * break-glass/emergency reads where the audit record must
   * capture the justification.
   */
  readonly reason: string;
  /**
   * Wire request ID, if the caller has one (e.g. copied from a
   * server's response). Optional.
   */
  readonly requestId?: string;
  /**
   * Distributed-tracing correlation ID (often an HTTP
   * `X-Request-Id`). Optional.
   */
  readonly correlationId?: string;
}

const storage = new AsyncLocalStorage<AuditContext>();

/**
 * Run `fn` with the given audit context active. Nested calls see
 * the innermost context; outer contexts are restored on return.
 *
 * Supports both sync and async `fn` — the context survives
 * `await` boundaries via `AsyncLocalStorage`.
 */
export function runWithAudit<T>(ctx: AuditContext, fn: () => T): T {
  return storage.run(ctx, fn);
}

/**
 * Return the current audit context, or `undefined` if none is
 * active. Callers that require a context should either throw or
 * default to a synthesised "system" context:
 *
 * ```ts
 * const ctx = currentAudit() ?? { actor: 'system', reason: 'background-job' };
 * ```
 */
export function currentAudit(): AuditContext | undefined {
  return storage.getStore();
}

/**
 * Return the current audit context, throwing if none is active.
 * Use at SDK call sites that refuse to run without attribution
 * (e.g. break-glass queries, PHI exports).
 */
export function requireAudit(): AuditContext {
  const ctx = storage.getStore();
  if (ctx === undefined) {
    throw new Error(
      'requireAudit(): no audit context active — wrap the call in runWithAudit({ actor, reason })',
    );
  }
  return ctx;
}
