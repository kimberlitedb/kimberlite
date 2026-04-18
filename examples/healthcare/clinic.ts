/**
 * End-to-end clinic-management walkthrough — TypeScript SDK.
 *
 * Prerequisites:
 *   1. Run examples/healthcare/00-setup.sh to get a server on 127.0.0.1:5432
 *      with the schema + seed data loaded.
 *   2. Build the SDK:
 *        cd sdks/typescript && npm install && npm run build
 *
 * Run with:
 *   ts-node examples/healthcare/clinic.ts
 *   # or (after tsc):  node examples/healthcare/clinic.js
 *
 * The script walks the full lifecycle: connection pool for CRUD → dedicated
 * Client for admin / compliance / subscribe → typed row mapping → query
 * builder → consent grant + check → real-time subscription → GDPR erasure.
 * Every step prints a line describing what it proves.
 *
 * Design note: `PooledClient` exposes the hot-path data plane (create / append
 * / read / query / execute / sync). Admin, compliance, and subscribe live on
 * the full `Client` because they tend to be one-shot flows where pooling
 * adds complexity without benefit.
 */

import {
  Client,
  Pool,
  Query,
  ValueBuilder,
  valueToString,
  isBigInt,
  isText,
  OffsetMismatchError,
  RateLimitedError,
  NotLeaderError,
  type Value,
} from '../../sdks/typescript/src';

// ----------------------------------------------------------------------------
// Types we'll materialise out of query rows.
// ----------------------------------------------------------------------------

interface Patient {
  id: bigint;
  mrn: string;
  name: string;
  dob: string;
  primaryProviderId: bigint;
}

// ----------------------------------------------------------------------------

async function main() {
  const address = process.env.KIMBERLITE_ADDR ?? '127.0.0.1:5432';
  const tenantId = 1n;

  const pool = await Pool.create({ address, tenantId, maxSize: 8 });
  const admin = await Client.connect({ addresses: [address], tenantId });

  try {
    console.log('✓ pool + admin client ready');

    // 1. Admin — introspect the schema we just loaded.
    const tables = await admin.admin.listTables();
    console.log(
      `✓ admin.listTables → ${tables.length} tables: ${tables.map((t) => t.name).join(', ')}`,
    );

    // 2. Typed row mapping — returns Patient[] instead of raw Value[][].
    const patients = await pool.withClient((client) =>
      client.queryRows<Patient>(
        'SELECT id, medical_record_number, first_name, last_name, date_of_birth, primary_provider_id FROM patients WHERE active = $1 ORDER BY id',
        [ValueBuilder.boolean(true)],
        (row, cols) => ({
          id: bigintAt(row, cols, 'id'),
          mrn: textAt(row, cols, 'medical_record_number'),
          name: `${textAt(row, cols, 'first_name')} ${textAt(row, cols, 'last_name')}`,
          dob: textAt(row, cols, 'date_of_birth'),
          primaryProviderId: bigintAt(row, cols, 'primary_provider_id'),
        }),
      ),
    );
    console.log(`✓ typed query → ${patients.length} active patients`);
    patients.forEach((p) =>
      console.log(
        `  · #${p.id} ${p.name} (MRN ${p.mrn}) → provider ${p.primaryProviderId}`,
      ),
    );

    // 3. Query builder — same idea, fluent composition.
    const drChenPatients = await pool.withClient(async (client) => {
      const built = Query.from('patients')
        .select(['id', 'first_name', 'last_name'])
        .whereEq('primary_provider_id', ValueBuilder.bigint(2))
        .orderBy('id')
        .build();
      return client.query(built.sql, built.params);
    });
    console.log(
      `✓ query-builder → Dr. Chen has ${drChenPatients.rows.length} patient(s)`,
    );

    // 4. Consent — grant research consent for patient 1.
    const subjectId = 'patient:1';
    const granted = await admin.compliance.consent.grant(subjectId, 'Research');
    console.log(`✓ compliance.consent.grant → consentId=${granted.consentId}`);

    const consentOk = await admin.compliance.consent.check(subjectId, 'Research');
    console.log(`  · consent.check(${subjectId}, 'Research') → ${consentOk}`);

    // 5. Real-time subscribe — see docs/reference/sdk/typescript-api.md
    //    for the full pattern:
    //
    //      const stream = await admin.createStream('encounters', DataClass.PHI);
    //      const sub = await admin.subscribe(stream, { fromOffset: 0n, initialCredits: 128 });
    //      for await (const ev of sub) { dashboard.push(ev); }
    //
    //    We skip the live demo here because a dev server that already has a
    //    stream of the same tenant from a prior run will reject the call
    //    with StreamAlreadyExists; a real application would subscribe to
    //    known-existing streams rather than creating a fresh one each time.
    console.log(
      '✓ subscribe → skipped (see docs/reference/sdk/typescript-api.md for a full example)',
    );

    // 6. Erasure — GDPR Article 17 "right to be forgotten".
    //    Request, mark progress on affected streams, then defer completion
    //    to the application (which actually erases rows + calls complete()).
    const req = await admin.compliance.erasure.request(subjectId);
    console.log(
      `✓ erasure.request → requestId=${req.requestId} status=${req.status.kind}`,
    );
    if (req.streamsAffected.length > 0) {
      await admin.compliance.erasure.markProgress(
        req.requestId,
        req.streamsAffected,
      );
      console.log(`  · markProgress for ${req.streamsAffected.length} stream(s)`);
    }
    console.log(
      '  · complete() skipped in demo — see docs/concepts/data-portability.md',
    );

    // 7. Pool stats — operator-facing health.
    const stats = await pool.stats();
    console.log(
      `✓ pool.stats → open=${stats.open} inUse=${stats.inUse} idle=${stats.idle}`,
    );

    console.log('\n✅ clinic walkthrough complete');
  } catch (e) {
    if (
      e instanceof RateLimitedError ||
      e instanceof NotLeaderError ||
      e instanceof OffsetMismatchError
    ) {
      console.warn(`⚠ transient error (${e.code}): ${e.message}`);
    } else {
      throw e;
    }
  } finally {
    await admin.disconnect();
    await pool.shutdown();
  }
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

function bigintAt(row: Value[], cols: string[], name: string): bigint {
  const v = row[cols.indexOf(name)];
  if (!isBigInt(v)) throw new Error(`expected BIGINT at column '${name}'`);
  return v.value;
}

function textAt(row: Value[], cols: string[], name: string): string {
  const v = row[cols.indexOf(name)];
  if (!isText(v)) return valueToString(v);
  return v.value;
}

main().catch((e) => {
  console.error('❌ clinic walkthrough failed:', e);
  process.exit(1);
});
