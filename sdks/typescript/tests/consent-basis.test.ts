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
  store: {
    lastGrantArgs?: [
      string,
      JsConsentPurpose,
      JsConsentBasis | null | undefined,
      string | null | undefined,
      boolean | null | undefined,
    ];
  },
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
    async consentGrant(subjectId, purpose, basis, termsVersion, accepted) {
      store.lastGrantArgs = [subjectId, purpose, basis, termsVersion, accepted];
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
    auditQuery: notImpl,
    maskingPolicyCreate: notImpl,
    maskingPolicyDrop: notImpl,
    maskingPolicyAttach: notImpl,
    maskingPolicyDetach: notImpl,
    maskingPolicyList: notImpl,
  } as NativeKimberliteClient;
}

describe('ConsentBasis — grant → native round-trip (wire v4)', () => {
  it('forwards basis={article,justification} through the native layer', async () => {
    const store: {
      lastGrantArgs?: [
        string,
        JsConsentPurpose,
        JsConsentBasis | null | undefined,
        string | null | undefined,
        boolean | null | undefined,
      ];
    } = {};
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
    const store: {
      lastGrantArgs?: [
        string,
        JsConsentPurpose,
        JsConsentBasis | null | undefined,
        string | null | undefined,
        boolean | null | undefined,
      ];
    } = {};
    const native = makeStubNative(store, []);
    const compliance = new ComplianceNamespace(native);

    await compliance.consent.grant('bob', 'Marketing');

    const [, , nativeBasis] = store.lastGrantArgs!;
    expect(nativeBasis).toBeNull();
  });

  it('maps a basis with undefined justification to native null', async () => {
    const store: {
      lastGrantArgs?: [
        string,
        JsConsentPurpose,
        JsConsentBasis | null | undefined,
        string | null | undefined,
        boolean | null | undefined,
      ];
    } = {};
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
      termsVersion: null,
      accepted: true,
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
      termsVersion: null,
      accepted: true,
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
      termsVersion: null,
      accepted: true,
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

describe('ConsentGrantOptions — v0.6.2 grant overload', () => {
  type StoreT = {
    lastGrantArgs?: [
      string,
      JsConsentPurpose,
      JsConsentBasis | null | undefined,
      string | null | undefined,
      boolean | null | undefined,
    ];
  };

  it('forwards termsVersion + accepted from the options bag', async () => {
    const store: StoreT = {};
    const native = makeStubNative(store, []);
    const compliance = new ComplianceNamespace(native);

    await compliance.consent.grant('dave', 'Research', {
      termsVersion: '2026-04-tos',
      accepted: true,
      basis: { article: 'Consent', justification: 'opt-in' },
    });

    const [, , nativeBasis, termsVersion, accepted] = store.lastGrantArgs!;
    expect(nativeBasis).toEqual({ article: 'Consent', justification: 'opt-in' });
    expect(termsVersion).toBe('2026-04-tos');
    expect(accepted).toBe(true);
  });

  it('records an explicit decline when accepted=false', async () => {
    const store: StoreT = {};
    const native = makeStubNative(store, []);
    const compliance = new ComplianceNamespace(native);

    await compliance.consent.grant('erin', 'Marketing', {
      termsVersion: 'v3',
      accepted: false,
    });

    const [, , nativeBasis, termsVersion, accepted] = store.lastGrantArgs!;
    expect(nativeBasis).toBeNull();
    expect(termsVersion).toBe('v3');
    expect(accepted).toBe(false);
  });

  it('keeps the v0.6.1 ConsentBasis-as-3rd-arg form working unchanged', async () => {
    // Backwards-compat: v0.6.1 callers passed a bare ConsentBasis as
    // the 3rd argument. The runtime type-guard on `'article' in arg3`
    // routes those through the basis-only path; termsVersion/accepted
    // arrive at the native layer as null.
    const store: StoreT = {};
    const native = makeStubNative(store, []);
    const compliance = new ComplianceNamespace(native);

    const basis: ConsentBasis = { article: 'LegalObligation' };
    await compliance.consent.grant('frank', 'Security', basis);

    const [, , nativeBasis, termsVersion, accepted] = store.lastGrantArgs!;
    expect(nativeBasis).toEqual({ article: 'LegalObligation', justification: null });
    expect(termsVersion).toBeNull();
    // Default acceptance — caller didn't specify, native layer defaults to true.
    expect(accepted).toBeNull();
  });

  it('surfaces termsVersion + accepted from the native record back to the SDK', async () => {
    const nativeRecord: JsConsentRecord = {
      consentId: 'consent-uuid-4',
      subjectId: 'gail',
      purpose: 'Marketing',
      scope: 'AllData',
      grantedAtNanos: 1_700_000_000_000_000_000n,
      withdrawnAtNanos: null,
      expiresAtNanos: null,
      notes: null,
      basis: null,
      termsVersion: 'v7',
      accepted: false,
    };
    const native = makeStubNative({}, [nativeRecord]);
    const compliance = new ComplianceNamespace(native);

    const records = await compliance.consent.list('gail');
    expect(records[0].termsVersion).toBe('v7');
    expect(records[0].accepted).toBe(false);
  });
});
