// Kimberlite Cookbook — Consent decline round-trip in TypeScript.
//
// AUDIT-2026-05 M-7. Exercises the v0.6.2 termsVersion / accepted
// fields end-to-end and verifies the decline lands in the audit log.
//
// Prerequisite: a running `kmb-server` on localhost:5432.

import { Client } from "@kimberlitedb/client";

const KMB_ADDRESS = process.env.KMB_ADDRESS ?? "localhost:5432";
const KMB_TENANT = BigInt(process.env.KMB_TENANT ?? "1");

async function main(): Promise<void> {
    const client = await Client.connect({
        address: KMB_ADDRESS,
        tenantId: KMB_TENANT,
    });

    const subjectId = `subject_${Math.random().toString(36).slice(2, 10)}`;
    const termsVersion = "2026-05-04";

    // Record a decline. The user explicitly responded `false` against
    // a specific terms version. Pre-v0.6.2 there was no place to
    // attach the version or the boolean response.
    await client.compliance.consent.grant({
        subjectId,
        purpose: "Research",
        basis: { article: "GDPR-Art-6-1-a", justification: "explicit consent" },
        termsVersion,
        accepted: false,
    });

    // Verify the audit trail captured the decline. The
    // `ConsentGranted` action variant is retained even for declines
    // so the audit-event taxonomy stays single-track.
    const auditRows = await client.compliance.audit.query({
        subjectId,
        action: "ConsentGranted",
        limit: 10,
    });

    if (auditRows.rows.length !== 1) {
        console.error(
            `FAIL: expected exactly 1 audit row for ${subjectId}, got ${auditRows.rows.length}`,
        );
        process.exit(1);
    }

    const row = auditRows.rows[0];
    if (row.termsVersion !== termsVersion) {
        console.error(
            `FAIL: audit row termsVersion ${row.termsVersion!} ≠ expected ${termsVersion}`,
        );
        process.exit(1);
    }
    if (row.accepted !== false) {
        console.error(`FAIL: audit row accepted=${row.accepted!} (expected false)`);
        process.exit(1);
    }

    await client.close();
    console.log("KMB_COOKBOOK_OK");
}

main().catch((err) => {
    console.error("FAIL:", err);
    process.exit(1);
});
