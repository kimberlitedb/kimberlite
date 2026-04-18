---
title: "First Application"
section: "start"
slug: "first-app"
order: 3
---

# First Application

Build a minimal clinic-management app that touches every compliance
primitive Kimberlite exposes — schema, typed queries, consent, audit,
time-travel, and erasure — in one short session.

This tutorial uses the TypeScript SDK because it has the fewest moving
parts (no compile step). The same storyline is available in
[Rust](../../examples/rust/src/clinic.rs) and
[Python](../../examples/healthcare/clinic.py); see
[examples/healthcare/](../../examples/healthcare/) for a deeper walkthrough
that also covers SQL-level audit queries and time-travel SQL syntax.

## What you'll build

A patient-records app that:

1. Creates a HIPAA-aware schema (patients, providers, encounters, audit).
2. Inserts a handful of records with parameterised queries.
3. Projects rows into typed objects (`Patient[]` instead of raw cells).
4. Grants research consent for a subject and checks it.
5. Requests GDPR Article 17 erasure.
6. Shows the built-in audit trail via time-travel queries.

Total runtime: ~2 minutes.

## Prerequisites

- `kimberlite` CLI installed — `curl -fsSL https://kimberlite.dev/install.sh | sh`
- Node.js 18, 20, 22, or 24

## Step 1 — Start the dev server

One command brings up the database, the Studio UI, and logs to one place:

```bash
kimberlite init clinic-tracker
cd clinic-tracker
kimberlite dev
```

You should see:

```
 Database:  127.0.0.1:5432
 Studio:    http://127.0.0.1:5555
```

Leave that terminal running.

## Step 2 — Project setup

In a new terminal:

```bash
mkdir -p clinic-tracker-app && cd clinic-tracker-app
npm init -y
npm install --save @kimberlite/client
npm install --save-dev typescript @types/node ts-node
```

Create a minimal `tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "es2020",
    "module": "commonjs",
    "esModuleInterop": true,
    "moduleResolution": "node",
    "strict": false,
    "skipLibCheck": true,
    "types": ["node"]
  }
}
```

## Step 3 — Write the app

Create `app.ts`:

```ts
import { Client, ValueBuilder, valueToString, isBigInt, isText } from '@kimberlite/client';

interface Patient {
  id: bigint;
  name: string;
  dob: string;
}

async function main() {
  const client = await Client.connect({
    addresses: ['127.0.0.1:5432'],
    tenantId: 1n,
  });

  try {
    // --- Schema ------------------------------------------------------------
    await client.execute(`
      CREATE TABLE IF NOT EXISTS patients (
        id BIGINT NOT NULL PRIMARY KEY,
        name TEXT NOT NULL,
        dob TEXT NOT NULL
      )
    `);
    console.log('✓ schema ready');

    // --- Seed rows ---------------------------------------------------------
    for (const row of [
      [1n, 'Jane Doe', '1985-03-15'],
      [2n, 'John Smith', '1972-08-22'],
      [3n, 'Alice Johnson', '1990-11-05'],
    ]) {
      await client.execute(
        'INSERT INTO patients (id, name, dob) VALUES ($1, $2, $3)',
        [
          ValueBuilder.bigint(row[0] as bigint),
          ValueBuilder.text(row[1] as string),
          ValueBuilder.text(row[2] as string),
        ],
      );
    }
    console.log('✓ 3 patients inserted');

    // --- Typed query -------------------------------------------------------
    const patients = await client.queryRows<Patient>(
      'SELECT id, name, dob FROM patients ORDER BY id',
      [],
      (row, cols) => ({
        id: (row[cols.indexOf('id')] as any).value as bigint,
        name: isText(row[cols.indexOf('name')]) ? (row[cols.indexOf('name')] as any).value : '',
        dob: isText(row[cols.indexOf('dob')]) ? (row[cols.indexOf('dob')] as any).value : '',
      }),
    );
    for (const p of patients) {
      console.log(`  · #${p.id} ${p.name} (DOB ${p.dob})`);
    }

    // --- Consent (GDPR Art 6 / HIPAA) --------------------------------------
    const subject = 'patient:1';
    const granted = await client.compliance.consent.grant(subject, 'Research');
    console.log(`✓ consent granted (consentId=${granted.consentId})`);

    const ok = await client.compliance.consent.check(subject, 'Research');
    console.log(`  · consent.check(${subject}, 'Research') → ${ok}`);

    // --- Time travel --------------------------------------------------------
    // Ask what the table looked like at offset 0 — before the first insert.
    const pre = await client.queryAt('SELECT COUNT(*) FROM patients', [], 0n);
    console.log(`  · patients before any inserts: ${valueToString(pre.rows[0][0])}`);

    // --- GDPR Article 17 erasure -------------------------------------------
    const req = await client.compliance.erasure.request(subject);
    console.log(`✓ erasure requested (requestId=${req.requestId}, status=${req.status.kind})`);

    console.log('\nDone. Open http://127.0.0.1:5555 to see the Studio audit view.');
  } finally {
    await client.disconnect();
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
```

## Step 4 — Run it

```bash
npx ts-node app.ts
```

Expected output:

```
✓ schema ready
✓ 3 patients inserted
  · #1 Jane Doe (DOB 1985-03-15)
  · #2 John Smith (DOB 1972-08-22)
  · #3 Alice Johnson (DOB 1990-11-05)
✓ consent granted (consentId=…)
  · consent.check(patient:1, 'Research') → true
  · patients before any inserts: 0
✓ erasure requested (requestId=…, status=Pending)

Done. Open http://127.0.0.1:5555 to see the Studio audit view.
```

## What just happened

- **`client.execute()`** appended a DDL and three DML entries to the
  immutable log. Every one is recoverable via time-travel.
- **`client.queryRows<T>()`** projected result rows into typed `Patient`
  objects — no ad-hoc casting in the calling code.
- **`client.compliance.consent.grant()`** wrote a signed consent record
  that you can query or withdraw later. The record survives the app
  crash — it's persistent state on the server.
- **`client.queryAt(..., 0n)`** ran the same SQL at log offset 0,
  proving the table was empty before your inserts. No separate audit
  infrastructure needed.
- **`client.compliance.erasure.request()`** initiated a GDPR Article 17
  flow with a 30-day completion deadline. Marking streams complete
  (the application's responsibility) produces an HMAC-signed audit
  record proving the data is gone.

## Next steps

- **Full walkthrough:** [`examples/healthcare/`](../../examples/healthcare/) extends
  this app with access grants, RBAC-aware queries, the audit log,
  real-time subscriptions, and the same storyline in Rust and Python.
- **Reference:** [TypeScript SDK API](../reference/sdk/typescript-api.md)
- **Deeper concepts:**
  - [Consent management](../concepts/consent-management.md)
  - [Data portability / erasure](../concepts/data-portability.md)
  - [Compliance frameworks](../concepts/compliance.md) — HIPAA, GDPR, SOC 2, and 20 more
  - [RBAC](../concepts/rbac.md) — role-based column + row filtering
