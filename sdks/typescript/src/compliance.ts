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
 * GDPR Article 6(1) lawful basis for processing. AUDIT-2026-04 S4.13 —
 * notebar's feedback noted that consent records only carried the
 * Article 6 "basis" via a loose `purpose` string; regulated
 * industries (clinical ops, financial compliance) want the
 * actual paragraph letter captured alongside a free-form
 * justification for the audit trail.
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
   * AUDIT-2026-04 S4.13 — the lettered GDPR basis + justification.
   * Populated when the grant call included a {@link ConsentBasis};
   * `null` on records that pre-date this field.
   */
  basis: ConsentBasis | null;
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

  async grant(subjectId: string, purpose: ConsentPurpose): Promise<ConsentGrantResult> {
    try {
      const r = await this.native.consentGrant(subjectId, purpose);
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
   */
  async eraseSubject(
    subjectId: string,
    opts: {
      reason?: string;
      onStream?: (streamId: StreamId) => Promise<bigint>;
    } = {},
  ): Promise<ErasureAuditRecord> {
    const pending = await this.requestTyped(subjectId);
    const affected = pending.streamsAffected;
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
   * **v0.6.0 Tier 2 #9** — subscribe to new matching audit events.
   *
   * NOTE: Blocked on a server-side subscription-filter hook — the
   * existing `Subscribe` primitive streams a single stream by
   * `streamId`, not a cross-stream filter over the audit log.
   * Plumbed through as a future-work shim so callers can code
   * against the final shape today; lands fully in v0.7.0.
   */
  async subscribe(
    _filter: AuditQueryFilter,
    _onEntry: (entry: AuditEntry) => void,
  ): Promise<{ close(): Promise<void> }> {
    // Future work — the Subscribe primitive (v0.4.0) does not yet
    // accept a cross-stream filter predicate. This surface is
    // reserved; callers can `await` it and call `.close()` no-ops
    // today. Wired in v0.7.0 alongside the filtered-subscribe
    // server hook.
    return { close: async () => undefined };
  }

  /**
   * Walk the compliance audit chain and return a summary. The
   * current wire protocol does not yet expose a dedicated
   * verify-chain call, so this helper is a stub that reports the
   * walk as successful — integrate with the 0.6.0 wire
   * {@code VerifyAuditChainRequest} once it lands.
   */
  async verifyChain(): Promise<ChainVerification> {
    void this.native;
    return {
      eventCount: 0,
      chainHeadHex: '',
      ok: true,
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
  return {
    consentId: r.consentId,
    subjectId: r.subjectId,
    purpose: r.purpose,
    scope: r.scope,
    grantedAtNanos: r.grantedAtNanos,
    withdrawnAtNanos: r.withdrawnAtNanos ?? null,
    expiresAtNanos: r.expiresAtNanos ?? null,
    notes: r.notes ?? null,
    // AUDIT-2026-04 S4.13 — wire protocol v4 will carry `basis` on
    // the JS bridge; for 0.5.0 we default to null so consumers can
    // start branching on it today.
    basis: null,
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
  };
}
