// Kimberlite Cookbook — Time-travel queries in TypeScript.
//
// Demonstrates `SELECT … AS OF TIMESTAMP '<iso>'`: reconstruct the
// projection state at any point in the past from the immutable log.
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

    // Per-run unique table so the example is repeatable without DROP.
    const suffix = Math.random().toString(36).slice(2, 10);
    const table = `cookbook_tt_${suffix}`;

    await client.execute(
        `CREATE TABLE ${table} (id BIGINT PRIMARY KEY, balance BIGINT)`,
        [],
    );

    // T1 — initial state.
    await client.execute(`INSERT INTO ${table} VALUES (1, 100)`, []);
    const t1 = new Date();

    // Wait long enough that the timestamp index can distinguish T1 and T2.
    await new Promise((resolve) => setTimeout(resolve, 50));

    // T2 — update.
    await client.execute(`UPDATE ${table} SET balance = 75 WHERE id = 1`, []);

    // Now: balance is 75.
    const now = await client.query(`SELECT balance FROM ${table} WHERE id = 1`, []);
    const nowBalance = Number(now.rows[0]?.balance ?? -1);
    if (nowBalance !== 75) {
        console.error(`FAIL: current balance expected 75, got ${nowBalance}`);
        process.exit(1);
    }

    // As of T1: balance was 100.
    const past = await client.queryAt(
        `SELECT balance FROM ${table} WHERE id = 1`,
        [],
        t1,
    );
    const pastBalance = Number(past.rows[0]?.balance ?? -1);
    if (pastBalance !== 100) {
        console.error(
            `FAIL: balance AS OF T1 expected 100, got ${pastBalance}`,
        );
        process.exit(1);
    }

    await client.close();
    console.log("KMB_COOKBOOK_OK");
}

main().catch((err) => {
    console.error("FAIL:", err);
    process.exit(1);
});
