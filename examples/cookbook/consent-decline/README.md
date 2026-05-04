# Cookbook: Consent decline + audit-trail verification

What this teaches:

- `client.compliance.consent.grant({ subjectId, purpose, basis,
  termsVersion, accepted })` exercises the v0.6.2 fields end-to-end.
- A **decline** is a compliance event in its own right —
  `accepted: false` records the user's response against the specific
  `termsVersion` they were shown. Pre-v0.6.2 there was no surface for
  this; consumers were either omitting decline tracking or
  out-of-band logging it.
- The audit trail captures the decline. `client.compliance.audit.query({
  subjectId, action: "ConsentGranted" })` returns the structured row
  including the `accepted: false` field.
- The variant name `ConsentGranted` is intentionally retained even
  for declines — the audit-event taxonomy is "a consent decision was
  recorded", not "consent was approved". This avoids the audit trail
  growing two parallel decline/approve event types whose ordering
  could fall out of sync.

Prerequisites:

```bash
just kmb-server-dev
```

Run:

```bash
cd typescript && pnpm install && pnpm tsx main.ts
# or
cd python && python main.py
```

Expected stdout (last line):

```
KMB_COOKBOOK_OK
```

## Related docs

- [`docs/coding/recipes/consent-decline.md`](../../../docs/coding/recipes/consent-decline.md)
- [`CHANGELOG.md`](../../../CHANGELOG.md) v0.6.2 entry "Added — consent-grant terms-acceptance fields"
