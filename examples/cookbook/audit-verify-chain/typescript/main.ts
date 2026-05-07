// Kimberlite Cookbook — Audit chain verification in TypeScript (v0.8.0).
//
// Demonstrates `compliance.audit.verifyChain()`: server-walked SHA-256
// hash-chain attestation over the compliance audit log. Replaces the
// pre-v0.8.0 hardcoded `{ ok: true }` stub with real integrity proof.
//
// Run:
//   pnpm install
//   pnpm tsx main.ts
//
// Prerequisite: a running `kmb-server` on localhost:5432
//   (e.g. `just kmb-server-dev`).

import { Client } from "@kimberlitedb/client";

const KMB_ADDRESS = process.env.KMB_ADDRESS ?? "localhost:5432";
const KMB_TENANT = BigInt(process.env.KMB_TENANT ?? "1");

async function main(): Promise<void> {
    const client = await Client.connect({
        address: KMB_ADDRESS,
        tenantId: KMB_TENANT,
    });

    // Generate some audit events by exercising the compliance surface.
    // `consent.grant` writes a ConsentGranted event into the audit log;
    // `erasure.request` writes an ErasureRequested event. Both flow
    // through the SHA-256 hash chain.
    const subjectSuffix = Math.random().toString(36).slice(2, 10);
    const subjectId = `cookbook_subject_${subjectSuffix}`;

    await client.compliance.consent.grant(subjectId, "Research");
    await client.compliance.consent.check(subjectId, "Research");
    const erasure = await client.compliance.erasure.request(subjectId);

    // Verify the chain server-side. The server walks every audit
    // event's prev_hash → event_hash linkage and returns a structured
    // report.
    const report = await client.compliance.audit.verifyChain();

    if (!report.ok) {
        console.error(
            `FAIL: chain verification reported tampering at ${report.firstBrokenAt}`,
        );
        process.exit(1);
    }

    if (report.eventCount < 3) {
        console.error(
            `FAIL: expected at least 3 events walked, got ${report.eventCount}`,
        );
        process.exit(1);
    }

    if (!report.chainHeadHex || report.chainHeadHex.length !== 64) {
        console.error(
            `FAIL: chainHeadHex must be 64-char SHA-256 hex, got '${report.chainHeadHex}'`,
        );
        process.exit(1);
    }

    console.log(
        `verified ${report.eventCount} audit events; chain head = ${report.chainHeadHex.slice(0, 16)}…`,
    );
    console.log(`erasure request id = ${erasure.requestId}`);

    await client.close();
    console.log("KMB_COOKBOOK_OK");
}

main().catch((err) => {
    console.error("FAIL:", err);
    process.exit(1);
});
