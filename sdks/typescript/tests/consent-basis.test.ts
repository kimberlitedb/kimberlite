/**
 * v0.6.0 Tier 1 #2 — ConsentBasis round-trip test for the TypeScript
 * SDK. Exercises `ConsentNamespace.grant(...)` → the N-API bridge
 * and the inverse `nativeConsentToRecord` that drives
 * `ConsentNamespace.list()`.
 *
 * Stubs the native addon so the test runs without a live server —
 * the parity with a real server is covered by the Rust-side
 * `consent_basis.rs` integration test. Here we prove the
 * TypeScript layer forwards `basis` through the native contract and
 * surfaces the native record's `basis` back to the caller.
 */

import { ComplianceNamespace, ConsentBasis, ConsentRecord } from '../src/compliance';
import type {
  NativeKimberliteClient,
  JsConsentBasis,
  JsConsentRecord,
  JsConsentPurpose,
} from '../src/native';

/**
 * Minimal native stub — only the methods we need for the consent
 * round-trip. All others throw so a regression (accidental call)
 * is loud.
 */
function makeStubNative(
  store: { lastGrantArgs?: [string, JsConsentPurpose, JsConsentBasis | null | undefined] },
  listResult: JsConsentRecord[],
): NativeKimberliteClient {
  const notImpl = (): never => {
    throw new Error('not implemented in stub');
  };
  return {
    tenantId: 1n,
    lastRequestId: null,
    setAuditContext: () => undefined,
    clearAuditContext: () => undefined,
    createStream: notImpl,
    createStreamWithPlacement: notImpl,
    append: notImpl,
    readEvents: notImpl,
    query: notImpl,
    queryAt: notImpl,
    execute: notImpl,
    sync: notImpl,
    subscribe: notImpl,
    grantCredits: notImpl,
    unsubscribe: notImpl,
    nextSubscriptionEvent: notImpl,
    async consentGrant(subjectId, purpose, basis) {
      store.lastGrantArgs = [subjectId, purpose, basis];
      return { consentId: 'consent-uuid', grantedAtNanos: 1_700_000_000_000_000_000n };
    },
    consentWithdraw: notImpl,
    consentCheck: notImpl,
    async consentList() {
      return listResult;
    },
    erasureRequest: notImpl,
    erasureMarkProgress: notImpl,
    erasureMarkStreamErased: notImpl,
    erasureComplete: notImpl,
    erasureExempt: notImpl,
    erasureStatus: notImpl,
    erasureList: notImpl,
    listTables: notImpl,
    describeTable: notImpl,
    listIndexes: notImpl,
    tenantCreate: notImpl,
    tenantList: notImpl,
    tenantDelete: notImpl,
    tenantGet: notImpl,
    apiKeyRegister: notImpl,
    apiKeyRevoke: notImpl,
    apiKeyList: notImpl,
    apiKeyRotate: notImpl,
    serverInfo: notImpl,
  } as NativeKimberliteClient;
}

describe('ConsentBasis — grant → native round-trip (wire v4)', () => {
  it('forwards basis={article,justification} through the native layer', async () => {
    const store: { lastGrantArgs?: [string, JsConsentPurpose, JsConsentBasis | null | undefined] } =
      {};
    const native = makeStubNative(store, []);
    const compliance = new ComplianceNamespace(native);

    const basis: ConsentBasis = {
      article: 'Consent',
      justification: 'clinical research opt-in',
    };
    const result = await compliance.consent.grant('alice', 'Research', basis);

    expect(result.consentId).toBe('consent-uuid');
    expect(store.lastGrantArgs).toBeDefined();
    const [, , nativeBasis] = store.lastGrantArgs!;
    expect(nativeBasis).not.toBeNull();
    expect(nativeBasis).toEqual({
      article: 'Consent',
      justification: 'clinical research opt-in',
    });
  });

  it('forwards basis=null when caller omits the argument', async () => {
    const store: { lastGrantArgs?: [string, JsConsentPurpose, JsConsentBasis | null | undefined] } =
      {};
    const native = makeStubNative(store, []);
    const compliance = new ComplianceNamespace(native);

    await compliance.consent.grant('bob', 'Marketing');

    const [, , nativeBasis] = store.lastGrantArgs!;
    expect(nativeBasis).toBeNull();
  });

  it('maps a basis with undefined justification to native null', async () => {
    const store: { lastGrantArgs?: [string, JsConsentPurpose, JsConsentBasis | null | undefined] } =
      {};
    const native = makeStubNative(store, []);
    const compliance = new ComplianceNamespace(native);

    const basis: ConsentBasis = { article: 'LegalObligation' };
    await compliance.consent.grant('carol', 'Security', basis);

    const [, , nativeBasis] = store.lastGrantArgs!;
    expect(nativeBasis).toEqual({ article: 'LegalObligation', justification: null });
  });
});

describe('ConsentBasis — list → record round-trip (wire v4)', () => {
  it('surfaces basis from the native record verbatim', async () => {
    const nativeRecord: JsConsentRecord = {
      consentId: 'consent-uuid-1',
      subjectId: 'alice',
      purpose: 'Research',
      scope: 'AllData',
      grantedAtNanos: 1_700_000_000_000_000_000n,
      withdrawnAtNanos: null,
      expiresAtNanos: null,
      notes: null,
      basis: {
        article: 'Consent',
        justification: 'clinical research opt-in',
      },
    };
    const native = makeStubNative({}, [nativeRecord]);
    const compliance = new ComplianceNamespace(native);

    const records: ConsentRecord[] = await compliance.consent.list('alice');
    expect(records).toHaveLength(1);
    expect(records[0].basis).toEqual({
      article: 'Consent',
      justification: 'clinical research opt-in',
    });
  });

  it('maps native basis=null to record basis=null', async () => {
    const nativeRecord: JsConsentRecord = {
      consentId: 'consent-uuid-2',
      subjectId: 'bob',
      purpose: 'Marketing',
      scope: 'AllData',
      grantedAtNanos: 1_700_000_000_000_000_000n,
      withdrawnAtNanos: null,
      expiresAtNanos: null,
      notes: null,
      basis: null,
    };
    const native = makeStubNative({}, [nativeRecord]);
    const compliance = new ComplianceNamespace(native);

    const records = await compliance.consent.list('bob');
    expect(records[0].basis).toBeNull();
  });

  it('preserves a basis with no justification', async () => {
    const nativeRecord: JsConsentRecord = {
      consentId: 'consent-uuid-3',
      subjectId: 'carol',
      purpose: 'Security',
      scope: 'AllData',
      grantedAtNanos: 1_700_000_000_000_000_000n,
      withdrawnAtNanos: null,
      expiresAtNanos: null,
      notes: null,
      basis: { article: 'LegalObligation', justification: null },
    };
    const native = makeStubNative({}, [nativeRecord]);
    const compliance = new ComplianceNamespace(native);

    const records = await compliance.consent.list('carol');
    expect(records[0].basis).toEqual({
      article: 'LegalObligation',
      justification: undefined,
    });
  });
});
