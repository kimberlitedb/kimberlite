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
} from './native';

export type ConsentPurpose = JsConsentPurpose;
export type ConsentScope = JsConsentScope;
export type ErasureExemptionBasis = JsErasureExemptionBasis;

export interface ConsentRecord {
  consentId: string;
  subjectId: string;
  purpose: ConsentPurpose;
  scope: ConsentScope;
  grantedAtNanos: bigint;
  withdrawnAtNanos: bigint | null;
  expiresAtNanos: bigint | null;
  notes: string | null;
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
}

export class ComplianceNamespace {
  readonly consent: ConsentNamespace;
  readonly erasure: ErasureNamespace;

  constructor(native: NativeKimberliteClient) {
    this.consent = new ConsentNamespace(native);
    this.erasure = new ErasureNamespace(native);
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
    streamsAffected: r.streamsAffected,
  };
}

function nativeAuditToRecord(a: JsErasureAuditInfo): ErasureAuditRecord {
  return {
    requestId: a.requestId,
    subjectId: a.subjectId,
    requestedAtNanos: a.requestedAtNanos,
    completedAtNanos: a.completedAtNanos,
    recordsErased: a.recordsErased,
    streamsAffected: a.streamsAffected,
    erasureProofHex: a.erasureProofHex ?? null,
  };
}
