/**
 * Compliance namespace — GDPR consent + erasure. Accessed via `client.compliance`.
 *
 * @example
 * ```ts
 * await client.compliance.consent.grant('alice', 'Marketing');
 * const valid = await client.compliance.consent.check('alice', 'Marketing');
 *
 * const req = await client.compliance.erasure.request('alice');
 * await client.compliance.erasure.markProgress(req.requestId, [streamId]);
 * await client.compliance.erasure.markStreamErased(req.requestId, streamId, 42n);
 * const audit = await client.compliance.erasure.complete(req.requestId);
 * ```
 */

import { StreamId } from './types';
import { wrapNativeError } from './errors';
import type {
  NativeKimberliteClient,
  JsConsentPurpose,
  JsConsentScope,
  JsConsentRecord,
  JsConsentBasis,
  JsErasureExemptionBasis,
  JsErasureRequestInfo,
  JsErasureAuditInfo,
  JsErasureStatusTag,
  JsAuditEntry,
  JsAuditQueryFilter,
} from './native';

export type ConsentPurpose = JsConsentPurpose;
export type ConsentScope = JsConsentScope;
export type ErasureExemptionBasis = JsErasureExemptionBasis;

/**
 * GDPR Article 6(1) lawful basis for processing. Regulated
 * industries (clinical ops, financial compliance) need the
 * paragraph letter captured alongside a free-form justification
 * for the audit trail. Threaded onto {@link ConsentRecord} and the
 * `grant(...)` call from wire protocol v4 (v0.6.0).
 */
export type GdprArticle =
  /** (a) the data subject has given consent. */
  | 'Consent'
  /** (b) necessary for performance of a contract. */
  | 'Contract'
  /** (c) necessary for compliance with a legal obligation. */
  | 'LegalObligation'
  /** (d) necessary to protect vital interests. */
  | 'VitalInterests'
  /** (e) necessary for a task carried out in the public interest. */
  | 'PublicTask'
  /** (f) necessary for the purposes of legitimate interests. */
  | 'LegitimateInterests';

export interface ConsentBasis {
  /** The GDPR Article 6(1)(a)–(f) lettered basis. */
  article: GdprArticle;
  /** Free-form justification captured at grant time. */
  justification?: string;
}

export interface ConsentRecord {
  consentId: string;
  subjectId: string;
  purpose: ConsentPurpose;
  scope: ConsentScope;
  grantedAtNanos: bigint;
  withdrawnAtNanos: bigint | null;
  expiresAtNanos: bigint | null;
  notes: string | null;
  /**
   * The lettered GDPR Article 6(1) basis + justification.
   * Populated when the grant call included a {@link ConsentBasis};
   * `null` on pre-v4 records.
   */
  basis: ConsentBasis | null;
  /**
   * v0.6.2 — terms-of-service version the subject responded to.
   * `null` on pre-v0.6.2 records and on grants that omitted the
   * field.
   */
  termsVersion: string | null;
  /**
   * v0.6.2 — whether the subject accepted (`true`, default) or
   * explicitly declined (`false`). Pre-v0.6.2 records always read
   * `true` because consent grants were acceptance-only.
   */
  accepted: boolean;
}

/**
 * v0.6.2 — extra options for {@link ConsentNamespace.grant}.
 *
 * Carries the GDPR Article 6(1) {@link ConsentBasis} (wire v4) plus
 * the v0.6.2 fields `termsVersion` and `accepted`. Pass any subset;
 * unset fields keep their pre-v0.6.2 defaults
 * (`basis: undefined`, `termsVersion: undefined`, `accepted: true`).
 */
export interface ConsentGrantOptions {
  /** GDPR Article 6(1) lawful basis + justification. */
  basis?: ConsentBasis;
  /**
   * Terms-of-service version the subject responded to (e.g. `"v3"`,
   * `"2026-04-tos"`).
   */
  termsVersion?: string;
  /**
   * Whether the subject accepted (default `true`) or declined
   * (`false`). A declined record is still a compliance event —
   * the audit trail captures the decline against `termsVersion`.
   */
  accepted?: boolean;
}

export interface ConsentGrantResult {
  consentId: string;
  grantedAtNanos: bigint;
}

export interface ErasureStatus {
  kind: 'Pending' | 'InProgress' | 'Complete' | 'Failed' | 'Exempt';
  streamsRemaining?: number;
  erasedAtNanos?: bigint;
  totalRecords?: bigint;
  reason?: string;
  retryAtNanos?: bigint;
  basis?: ErasureExemptionBasis;
}

export interface ErasureRequest {
  requestId: string;
  subjectId: string;
  requestedAtNanos: bigint;
  deadlineNanos: bigint;
  status: ErasureStatus;
  recordsErased: bigint;
  streamsAffected: StreamId[];
}

export interface ErasureAuditRecord {
  requestId: string;
  subjectId: string;
  requestedAtNanos: bigint;
  completedAtNanos: bigint;
  recordsErased: bigint;
  streamsAffected: StreamId[];
  erasureProofHex: string | null;
  /**
   * v0.6.0 Tier 2 #8 — idempotence marker. `true` iff this record is
   * a "second-call noop" replay: the subject was already erased by a
   * prior request and the caller invoked {@link ErasureApi.eraseSubject}
   * again. The noop replay carries the original `requestId`,
   * `streamsAffected`, and signed proof verbatim — no new
   * cryptographic shred event occurred. `false` (the default) on any
   * originating erasure.
   *
   * Absent on pre-v0.6.0 audit records; treat missing as `false`.
   */
  isNoopReplay?: boolean;
}

/**
 * AUDIT-2026-04 S4.3 — typed state-machine tokens for erasure.
 *
 * Each transition emits a token that the next call must consume, so
 * the TypeScript compiler refuses to let you call `markStreamErased`
 * on a request that hasn't been moved to `InProgress` yet. Avoids
 * the "Request not in expected state" runtime error notebar hit on
 * a live server.
 *
 * Consumers that prefer the string-based API continue to use
 * `erasure.request(subjectId)` → `erasure.markProgress(id, streams)`
 * etc. The typed API is additive — call `erasure.requestTyped(...)`
 * to opt in.
 */
export type ErasurePending = ErasureRequest & { readonly __state: 'Pending' };
export type ErasureInProgress = ErasureRequest & { readonly __state: 'InProgress' };
export type ErasureRecording = ErasureRequest & { readonly __state: 'Recording' };

/**
 * Events yielded by {@link ErasureNamespace.subscribe} — either a
 * periodic status snapshot or the final terminal event when the
 * erasure reaches a {@code Complete} or {@code Exempt} status.
 */
export type ErasureSubscriptionEvent =
  | { readonly kind: 'Status'; readonly request: ErasureRequest }
  | { readonly kind: 'Complete'; readonly request: ErasureRequest };

class ConsentNamespace {
  constructor(private readonly native: NativeKimberliteClient) {}

  /**
   * Grant consent for `subjectId` + `purpose`.
   *
   * The third argument is either a bare {@link ConsentBasis} (the
   * v0.6.0 / v0.6.1 form) or a {@link ConsentGrantOptions} bag (the
   * v0.6.2+ form). Both shapes are supported indefinitely — the
   * options bag is preferred for new code because it carries the
   * v0.6.2 fields `termsVersion` and `accepted`.
   *
   * @example
   * ```ts
   * // v0.6.1 form (still works):
   * await client.compliance.consent.grant('alice', 'Marketing', {
   *   article: 'Consent',
   *   justification: 'opt-in at signup',
   * });
   *
   * // v0.6.2 form (recommended for new flows):
   * await client.compliance.consent.grant('alice', 'tos-acceptance', {
   *   termsVersion: '2026-04-tos',
   *   accepted: true,
   *   basis: { article: 'Consent' },
   * });
   *
   * // Recording an explicit decline:
   * await client.compliance.consent.grant('alice', 'tos-acceptance', {
   *   termsVersion: 'v3',
   *   accepted: false,
   * });
   * ```
   */
  async grant(
    subjectId: string,
    purpose: ConsentPurpose,
    basis: ConsentBasis,
  ): Promise<ConsentGrantResult>;
  async grant(
    subjectId: string,
    purpose: ConsentPurpose,
    options?: ConsentGrantOptions,
  ): Promise<ConsentGrantResult>;
  async grant(
    subjectId: string,
    purpose: ConsentPurpose,
    arg3?: ConsentBasis | ConsentGrantOptions,
  ): Promise<ConsentGrantResult> {
    try {
      // v0.6.1 callers passed a bare ConsentBasis as arg3.
      // v0.6.2 callers pass ConsentGrantOptions. ConsentBasis has a
      // required `article` discriminator; the options bag does not
      // (its fields are all optional). The presence of `article`
      // disambiguates without ambiguity.
      let basis: ConsentBasis | undefined;
      let termsVersion: string | undefined;
      let accepted: boolean | undefined;
      if (arg3 && typeof arg3 === 'object') {
        if ('article' in arg3) {
          basis = arg3 as ConsentBasis;
        } else {
          const opts = arg3 as ConsentGrantOptions;
          basis = opts.basis;
          termsVersion = opts.termsVersion;
          accepted = opts.accepted;
        }
      }
      const nativeBasis: JsConsentBasis | null = basis
        ? {
            article: basis.article,
            justification: basis.justification ?? null,
          }
        : null;
      const r = await this.native.consentGrant(
        subjectId,
        purpose,
        nativeBasis,
        termsVersion ?? null,
        accepted ?? null,
      );
      return { consentId: r.consentId, grantedAtNanos: r.grantedAtNanos };
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async withdraw(consentId: string): Promise<bigint> {
    try {
      return await this.native.consentWithdraw(consentId);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async check(subjectId: string, purpose: ConsentPurpose): Promise<boolean> {
    try {
      return await this.native.consentCheck(subjectId, purpose);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async list(subjectId: string, opts: { validOnly?: boolean } = {}): Promise<ConsentRecord[]> {
    try {
      const rows = await this.native.consentList(subjectId, opts.validOnly ?? false);
      return rows.map(nativeConsentToRecord);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }
}

class ErasureNamespace {
  constructor(private readonly native: NativeKimberliteClient) {}

  async request(subjectId: string): Promise<ErasureRequest> {
    try {
      return nativeErasureRequestToRecord(await this.native.erasureRequest(subjectId));
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async markProgress(requestId: string, streamIds: StreamId[]): Promise<ErasureRequest> {
    try {
      return nativeErasureRequestToRecord(
        await this.native.erasureMarkProgress(requestId, streamIds),
      );
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async markStreamErased(
    requestId: string,
    streamId: StreamId,
    recordsErased: bigint,
  ): Promise<ErasureRequest> {
    try {
      return nativeErasureRequestToRecord(
        await this.native.erasureMarkStreamErased(requestId, streamId, recordsErased),
      );
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async complete(requestId: string): Promise<ErasureAuditRecord> {
    try {
      return nativeAuditToRecord(await this.native.erasureComplete(requestId));
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async exempt(requestId: string, basis: ErasureExemptionBasis): Promise<ErasureRequest> {
    try {
      return nativeErasureRequestToRecord(await this.native.erasureExempt(requestId, basis));
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async status(requestId: string): Promise<ErasureRequest> {
    try {
      return nativeErasureRequestToRecord(await this.native.erasureStatus(requestId));
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  async list(): Promise<ErasureAuditRecord[]> {
    try {
      const rows = await this.native.erasureList();
      return rows.map(nativeAuditToRecord);
    } catch (e) {
      throw wrapNativeError(e);
    }
  }

  // --- AUDIT-2026-04 S4.3 typed state-machine surface -----------------

  /**
   * Open an erasure request and return a typed {@link ErasurePending}
   * token. The compiler refuses to let callers skip ahead to
   * {@link markStreamErasedTyped} without first calling
   * {@link markProgressTyped}.
   */
  async requestTyped(subjectId: string): Promise<ErasurePending> {
    const r = await this.request(subjectId);
    return brand<ErasurePending>(r, 'Pending');
  }

  /** Transition Pending → InProgress. */
  async markProgressTyped(
    token: ErasurePending,
    streamIds: StreamId[],
  ): Promise<ErasureInProgress> {
    const r = await this.markProgress(token.requestId, streamIds);
    return brand<ErasureInProgress>(r, 'InProgress');
  }

  /** Record per-stream progress — valid only in InProgress / Recording. */
  async markStreamErasedTyped(
    token: ErasureInProgress | ErasureRecording,
    streamId: StreamId,
    recordsErased: bigint,
  ): Promise<ErasureRecording> {
    const r = await this.markStreamErased(token.requestId, streamId, recordsErased);
    return brand<ErasureRecording>(r, 'Recording');
  }

  /** Finalise the erasure. Accepts either InProgress or Recording. */
  async completeTyped(
    token: ErasureInProgress | ErasureRecording,
  ): Promise<ErasureAuditRecord> {
    return this.complete(token.requestId);
  }

  /**
   * AUDIT-2026-04 S4.15 — subscribe to erasure-lifecycle events
   * (Pending→InProgress→Recording→Complete). Today this is a
   * polling subscription against {@link status}; once the server
   * exposes a dedicated compliance pub-sub channel (0.6.0) the
   * iterator is upgraded to push-based.
   *
   * Callers typically use this after {@link eraseSubject} to drive
   * progress UIs without blocking on the synchronous
   * {@link complete} call.
   *
   * ```ts
   * for await (const event of erasure.subscribe(requestId)) {
   *   if (event.kind === 'Complete') {
   *     console.log('done:', event.audit);
   *     break;
   *   }
   *   progressBar.update(event.status);
   * }
   * ```
   */
  async *subscribe(
    requestId: string,
    opts: { pollIntervalMs?: number; signal?: AbortSignal } = {},
  ): AsyncIterableIterator<ErasureSubscriptionEvent> {
    const intervalMs = opts.pollIntervalMs ?? 1000;
    while (!opts.signal?.aborted) {
      const status = await this.status(requestId);
      yield { kind: 'Status', request: status };
      if (status.status.kind === 'Complete' || status.status.kind === 'Exempt') {
        yield { kind: 'Complete', request: status };
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
  }

  /**
   * One-call helper that opens an erasure request, enumerates affected
   * streams via {@link status}, runs the caller-supplied `onStream`
   * side-effect per stream (or skips if unset), records per-stream
   * progress, and completes. Returns the final audit record.
   *
   * AUDIT-2026-04 S4.4 — mirrors notebar's `ErasureOrchestrator`
   * boilerplate. Callers that need fine-grained control keep using
   * the per-step typed API.
   *
   * **v0.6.0 Tier 2 #8 — auto-discovery.** If {@code opts.streams} is
   * not supplied, the server auto-walks PHI/PII/Sensitive streams with
   * a {@code subject_id} column and populates
   * {@code request.streamsAffected} on the `requestTyped` response —
   * this helper then drives erasure against that list. When
   * {@code opts.streams} IS supplied, it wins and auto-discovery is
   * skipped.
   *
   * **v0.6.0 Tier 2 #8 — idempotence.** A second call with the same
   * {@code subjectId} returns a noop-replay audit record
   * ({@code isNoopReplay: true}) carrying the original
   * {@code signedProof}. No new shred event occurs.
   */
  async eraseSubject(
    subjectId: string,
    opts: {
      reason?: string;
      streams?: StreamId[];
      onStream?: (streamId: StreamId) => Promise<bigint>;
    } = {},
  ): Promise<ErasureAuditRecord> {
    const pending = await this.requestTyped(subjectId);
    // v0.6.0 Tier 2 #8: caller-supplied streams override
    // auto-discovery; otherwise the server-populated list wins.
    const affected = opts.streams ?? pending.streamsAffected;
    const inProgress = await this.markProgressTyped(pending, affected);
    let recording: ErasureInProgress | ErasureRecording = inProgress;
    for (const streamId of affected) {
      const erased = opts.onStream ? await opts.onStream(streamId) : 0n;
      recording = await this.markStreamErasedTyped(recording, streamId, erased);
    }
    void opts.reason; // Reserved for a future `complete(reason)` overload.
    return this.completeTyped(recording);
  }
}

function brand<T extends ErasureRequest & { readonly __state: string }>(
  r: ErasureRequest,
  state: T['__state'],
): T {
  return { ...r, __state: state } as T;
}

/**
 * AUDIT-2026-04 S4.12 — client-side audit-chain verification +
 * report generation.
 *
 * Kimberlite's server maintains a SHA-256 hash chain over every
 * compliance audit event ({@code ComplianceAuditEvent.prev_hash} ⟶
 * {@code event_hash}). This namespace exposes the chain walk to
 * SDK callers so a regulator-visible "verify our audit log hasn't
 * been tampered with" surface exists without shelling out to the
 * server CLI.
 *
 * Note: wire-level {@code VerifyAuditChainRequest} /
 * {@code GenerateAuditReportRequest} are on the 0.6.0 roadmap. For
 * 0.5.0 the client walks the log via the existing audit-query API
 * and folds a report in-process. That keeps the helper useful
 * today while the wire surface stabilises.
 */
export interface ChainVerification {
  /** Number of audit events walked. */
  readonly eventCount: number;
  /** Head of the chain at verification time. */
  readonly chainHeadHex: string;
  /** `true` if the chain walked cleanly from genesis to head. */
  readonly ok: boolean;
  /** Populated when `ok === false`; names the first broken link. */
  readonly firstBrokenAt?: string;
}

/**
 * **v0.6.0 Tier 2 #9** — PHI-safe audit-log entry.
 *
 * Returned by {@link AuditNamespace.query}. The `changedFieldNames`
 * list names the fields the underlying action touched; it **never**
 * contains the values themselves.
 *
 * @example
 * ```ts
 * const entries = await client.compliance.audit.query({
 *   subjectId: 'alice@example.com',
 *   fromTs: new Date(Date.now() - 30 * 24 * 60 * 60 * 1000),
 * });
 * for (const e of entries) {
 *   console.log(e.occurredAt, e.action, e.changedFieldNames);
 * }
 * ```
 */
export interface AuditEntry {
  readonly actor: string | null;
  readonly action: string;
  readonly subjectId: string | null;
  readonly correlationId: string | null;
  readonly requestId: string | null;
  readonly occurredAt: Date;
  readonly reason: string | null;
  readonly changedFieldNames: readonly string[];
}

/**
 * **v0.6.0 Tier 2 #9** — filter for
 * {@link AuditNamespace.query}. All fields are optional; unset
 * fields don't constrain the result set.
 */
export interface AuditQueryFilter {
  /** Filter to events referencing this subject id. */
  subjectId?: string;
  /** Filter to events performed by this actor. */
  actor?: string;
  /** Filter to events whose action kind matches this prefix
   *  (e.g. `"Consent"`, `"Erasure"`). */
  action?: string;
  /** Lower time bound (inclusive). */
  fromTs?: Date;
  /** Upper time bound (inclusive). */
  toTs?: Date;
  /** Maximum number of entries to return. */
  limit?: number;
}

export interface AuditReport {
  /** Unix nanos — inclusive lower bound. */
  readonly fromNanos: bigint;
  /** Unix nanos — inclusive upper bound. */
  readonly toNanos: bigint;
  /** Raw audit entries in the window (PHI-safe). */
  readonly events: ReadonlyArray<AuditEntry>;
  /** Chain verification result, captured at report time. */
  readonly verification: ChainVerification;
}

class AuditNamespace {
  constructor(private readonly native: NativeKimberliteClient) {}

  /**
   * **v0.6.0 Tier 2 #9** — query the compliance audit log.
   *
   * Returns PHI-safe {@link AuditEntry} rows — each row lists the
   * field names the action touched (`changedFieldNames`) but never
   * the values.
   *
   * Filters are applied server-side. Missing filters don't
   * constrain the result set; combine freely.
   */
  async query(filter: AuditQueryFilter = {}): Promise<AuditEntry[]> {
    const native: JsAuditQueryFilter = {
      subjectId: filter.subjectId ?? null,
      actionType: filter.action ?? null,
      timeFromNanos:
        filter.fromTs !== undefined
          ? BigInt(filter.fromTs.getTime()) * 1_000_000n
          : null,
      timeToNanos:
        filter.toTs !== undefined
          ? BigInt(filter.toTs.getTime()) * 1_000_000n
          : null,
      actor: filter.actor ?? null,
      limit: filter.limit ?? null,
    };
    const entries = await this.native.auditQuery(native);
    return entries.map(nativeAuditEntryToEntry);
  }

  /**
   * **v0.8.0** — subscribe to new matching audit events via polling.
   *
   * Implemented as a poll loop over the existing {@link query} surface
   * (the Subscribe primitive is per-stream, and a cross-stream
   * subscription-filter hook is a separate piece of server work).
   * Each tick polls events newer than the last-seen timestamp; calls
   * the supplied `onEntry` callback with anything new; sleeps
   * `intervalMs` before the next tick. Returns a handle whose
   * {@code close()} stops the loop and resolves once the in-flight
   * tick (if any) finishes.
   *
   * @param filter — same shape as {@link query}; the `fromTs` field
   *   is overwritten on each tick to the most recent event timestamp.
   * @param onEntry — called once per new entry. Awaited per call so
   *   the loop won't fire faster than the slowest consumer.
   * @param opts.intervalMs — poll cadence. Default `1000`.
   * @param opts.signal — cooperative cancellation. The loop also
   *   stops on `close()`.
   */
  async subscribe(
    filter: AuditQueryFilter,
    onEntry: (entry: AuditEntry) => void | Promise<void>,
    opts: { intervalMs?: number; signal?: AbortSignal } = {},
  ): Promise<{ close(): Promise<void> }> {
    const intervalMs = opts.intervalMs ?? 1000;
    let stopped = false;
    let inflight: Promise<void> | null = null;
    // Track the highest occurredAt seen so successive ticks only fetch
    // events newer than the last one. Initialised to the caller's
    // `fromTs` (or "now" if unset) so we don't replay history.
    let highWater: Date = filter.fromTs ?? new Date();

    const tick = async (): Promise<void> => {
      const tickFilter: AuditQueryFilter = { ...filter, fromTs: highWater };
      const entries = await this.query(tickFilter);
      for (const entry of entries) {
        if (stopped || opts.signal?.aborted) return;
        // The query is `>= fromTs`; skip exactly-equal occurredAt to
        // avoid re-yielding the boundary entry.
        if (entry.occurredAt.getTime() <= highWater.getTime()) continue;
        await onEntry(entry);
        if (entry.occurredAt > highWater) highWater = entry.occurredAt;
      }
    };

    const loop = async (): Promise<void> => {
      while (!stopped && !opts.signal?.aborted) {
        inflight = tick();
        try {
          await inflight;
        } catch {
          // Swallow per-tick errors — a transient query failure
          // shouldn't terminate the subscription. Future work: surface
          // via an `onError` callback.
        } finally {
          inflight = null;
        }
        if (stopped || opts.signal?.aborted) break;
        await new Promise((resolve) => setTimeout(resolve, intervalMs));
      }
    };
    void loop();

    return {
      close: async (): Promise<void> => {
        stopped = true;
        if (inflight) {
          try {
            await inflight;
          } catch {
            // Ignore — the subscription is closing.
          }
        }
      },
    };
  }

  /**
   * **v0.8.0** — walk the compliance audit log's SHA-256 hash chain
   * server-side and return a structured verification report. Replaces
   * the v0.5.0 / v0.6.0 stubs that returned a hardcoded `ok: true`.
   *
   * On success, `ok === true`, `eventCount` carries the number of
   * events walked, and `chainHeadHex` is the hex-encoded SHA-256 of
   * the current chain head. On a tampering detection, `ok === false`,
   * `firstBrokenAt` carries the earliest mismatched event index (as
   * a string for forward compatibility), and the underlying error
   * message is surfaced through {@link ChainVerification.firstBrokenAt}.
   */
  async verifyChain(): Promise<ChainVerification> {
    const report = await this.native.verifyAuditChain();
    return {
      eventCount: Number(report.eventCount),
      chainHeadHex: report.chainHeadHex,
      ok: report.ok,
      firstBrokenAt:
        report.mismatchAtIndex !== null && report.mismatchAtIndex !== undefined
          ? report.mismatchAtIndex.toString()
          : report.errorMessage ?? undefined,
    };
  }

  /**
   * Generate a compliance report over `[fromNanos, toNanos]`.
   *
   * Wraps {@link AuditNamespace.query} with the window bounds and
   * bundles the PHI-safe entries alongside a chain-verification
   * summary.
   */
  async generateReport(opts: {
    fromNanos: bigint;
    toNanos: bigint;
  }): Promise<AuditReport> {
    const events = await this.query({
      fromTs: new Date(Number(opts.fromNanos / 1_000_000n)),
      toTs: new Date(Number(opts.toNanos / 1_000_000n)),
    });
    return {
      fromNanos: opts.fromNanos,
      toNanos: opts.toNanos,
      events,
      verification: await this.verifyChain(),
    };
  }
}

function nativeAuditEntryToEntry(e: JsAuditEntry): AuditEntry {
  return {
    actor: e.actor ?? null,
    action: e.action,
    subjectId: e.subjectId ?? null,
    correlationId: e.correlationId ?? null,
    requestId: e.requestId ?? null,
    // timestampNanos → Date (ms precision — JS Date is ms-resolution).
    occurredAt: new Date(Number(e.timestampNanos / 1_000_000n)),
    reason: e.reason ?? null,
    changedFieldNames: e.changedFieldNames,
  };
}

export class ComplianceNamespace {
  readonly consent: ConsentNamespace;
  readonly erasure: ErasureNamespace;
  readonly audit: AuditNamespace;

  constructor(native: NativeKimberliteClient) {
    this.consent = new ConsentNamespace(native);
    this.erasure = new ErasureNamespace(native);
    this.audit = new AuditNamespace(native);
  }
}

function nativeConsentToRecord(r: JsConsentRecord): ConsentRecord {
  const nativeBasis = r.basis;
  const basis: ConsentBasis | null = nativeBasis
    ? {
        article: nativeBasis.article,
        justification: nativeBasis.justification ?? undefined,
      }
    : null;
  return {
    consentId: r.consentId,
    subjectId: r.subjectId,
    purpose: r.purpose,
    scope: r.scope,
    grantedAtNanos: r.grantedAtNanos,
    withdrawnAtNanos: r.withdrawnAtNanos ?? null,
    expiresAtNanos: r.expiresAtNanos ?? null,
    notes: r.notes ?? null,
    basis,
    termsVersion: r.termsVersion ?? null,
    // `accepted` is non-optional on the v0.6.2 native record; older
    // server/native pairs that lack the field surface as `undefined`,
    // which `?? true` maps to the v0.6.1 acceptance-only default.
    accepted: r.accepted ?? true,
  };
}

function nativeErasureStatusToStatus(s: JsErasureStatusTag): ErasureStatus {
  return {
    kind: s.kind as ErasureStatus['kind'],
    streamsRemaining: s.streamsRemaining ?? undefined,
    erasedAtNanos: s.erasedAtNanos ?? undefined,
    totalRecords: s.totalRecords ?? undefined,
    reason: s.reason ?? undefined,
    retryAtNanos: s.retryAtNanos ?? undefined,
    basis: s.basis ?? undefined,
  };
}

function nativeErasureRequestToRecord(r: JsErasureRequestInfo): ErasureRequest {
  return {
    requestId: r.requestId,
    subjectId: r.subjectId,
    requestedAtNanos: r.requestedAtNanos,
    deadlineNanos: r.deadlineNanos,
    status: nativeErasureStatusToStatus(r.status),
    recordsErased: r.recordsErased,
    streamsAffected: r.streamsAffected.map((s) => StreamId.from(s)),
  };
}

function nativeAuditToRecord(a: JsErasureAuditInfo): ErasureAuditRecord {
  return {
    requestId: a.requestId,
    subjectId: a.subjectId,
    requestedAtNanos: a.requestedAtNanos,
    completedAtNanos: a.completedAtNanos,
    recordsErased: a.recordsErased,
    streamsAffected: a.streamsAffected.map((s) => StreamId.from(s)),
    erasureProofHex: a.erasureProofHex ?? null,
    isNoopReplay: a.isNoopReplay ?? false,
  };
}
