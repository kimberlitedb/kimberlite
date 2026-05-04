// Kimberlite Cookbook — Secondary index lookup by non-PK column.
//
// AUDIT-2026-05 M-7 — closes the v0.7.0 ROADMAP item "Notebar's
// repos/communications.ts lookup-by-provider-message-id path was
// hitting full table scans because the index declaration step was
// missing".
//
// Run:
//   pnpm install
//   pnpm tsx main.ts
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

    const tableSuffix = Math.random().toString(36).slice(2, 8);
    const tableName = `messages_${tableSuffix}`;

    // Set up the projection. Primary key is `id` — provider lookups
    // are NOT on the PK, which is the whole point of the recipe.
    await client.execute(
        `CREATE TABLE ${tableName} (
            id BIGINT PRIMARY KEY,
            provider TEXT NOT NULL,
            provider_message_id TEXT NOT NULL,
            body TEXT
        )`,
    );

    // Composite secondary index on (provider, provider_message_id).
    // Without this declaration, the WHERE-clause lookup below would
    // fall through to a full TableScan.
    await client.execute(
        `CREATE INDEX idx_${tableName}_provider ON ${tableName} (provider, provider_message_id)`,
    );

    // Seed a few rows.
    for (let i = 0; i < 50; i++) {
        await client.execute(
            `INSERT INTO ${tableName} (id, provider, provider_message_id, body) VALUES ($1, $2, $3, $4)`,
            [BigInt(i), "twilio", `tw-${i}`, `body-${i}`],
        );
    }

    // EXPLAIN-verified IndexScan. Pre-fix notebar would have seen
    // TableScan here.
    const explain = await client.query(
        `EXPLAIN SELECT id FROM ${tableName} WHERE provider = $1 AND provider_message_id = $2`,
        ["twilio", "tw-7"],
    );
    const planText = JSON.stringify(explain.rows);
    if (!planText.includes("IndexScan")) {
        console.error(`FAIL: expected IndexScan in plan, got: ${planText}`);
        process.exit(1);
    }

    // Functional check — the query returns the right row.
    const result = await client.query(
        `SELECT id, body FROM ${tableName} WHERE provider = $1 AND provider_message_id = $2`,
        ["twilio", "tw-7"],
    );
    if (result.rows.length !== 1) {
        console.error(`FAIL: expected 1 row, got ${result.rows.length}`);
        process.exit(1);
    }

    await client.close();
    console.log("KMB_COOKBOOK_OK");
}

main().catch((err) => {
    console.error("FAIL:", err);
    process.exit(1);
});
