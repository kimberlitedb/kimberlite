# Cookbook: Audit chain verification (v0.8.0)

What this teaches:

- `compliance.audit.verifyChain()` walks the compliance audit log's
  SHA-256 hash chain server-side and returns a structured verification
  report.
- On success: `ok === true`, `eventCount` carries the number of events
  walked, `chainHeadHex` is the hex-encoded SHA-256 of the chain head.
- On tampering detection: `ok === false`, `firstBrokenAt` carries the
  earliest mismatched event index — regulator-visible signal pinpointing
  the tamper location.

This replaces the v0.5.0 / v0.6.0 stub that returned a hardcoded
`{ ok: true }`. Real attestation lands in v0.8.0.

Prerequisites:

```bash
just kmb-server-dev   # runs on 5432
```

Run:

```bash
cd typescript && pnpm install && pnpm tsx main.ts
```

Expected stdout (last line):

```
KMB_COOKBOOK_OK
```

## Why this matters

HIPAA §164.312(b) "Audit Controls", SOX §404 internal-controls proof,
SEC 17a-4 "non-rewriteable storage" all demand demonstrable
tamper-evidence on the audit trail. `verifyChain()` is the on-demand
proof: any auditor can call it at any time and get a cryptographic
attestation of integrity.

## Related docs

- [`docs/coding/recipes/audit-trails.md`](../../../docs/coding/recipes/audit-trails.md)
- [`docs/reference/sdk/parity.md`](../../../docs/reference/sdk/parity.md) — verifyChain ✅ on Rust + TS, 🚧 v0.9 on Python
- [`sdks/typescript/src/compliance.ts`](../../../sdks/typescript/src/compliance.ts) — implementation reference (line 715)
