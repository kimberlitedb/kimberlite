// Kimberlite Cookbook — Multi-tenant isolation in TypeScript.
//
// Demonstrates that two clients connecting with different `tenantId`s
// see entirely separate projection stores. The same table name and
// primary key exist independently in each tenant's view; cross-tenant
// reads return zero rows by construction (not by query rewriting).
//
// Run:
//   pnpm install
//   pnpm tsx main.ts
//
// Prerequisite: a running `kmb-server` on localhost:5432
//   (e.g. `just kmb-server-dev`).

import { Client } from "@kimberlitedb/client";

const KMB_ADDRESS = process.env.KMB_ADDRESS ?? "localhost:5432";

async function main(): Promise<void> {
    // Two distinct tenants. The server keeps separate projection stores
    // per tenant id; nothing in SQL can cross the boundary.
    const tenantA = await Client.connect({
        address: KMB_ADDRESS,
        tenantId: 1n,
    });

    const tenantB = await Client.connect({
        address: KMB_ADDRESS,
        tenantId: 2n,
    });

    const suffix = Math.random().toString(36).slice(2, 10);
    const table = `cookbook_mt_${suffix}`;

    // Each tenant creates the same-named table independently.
    await tenantA.execute(
        `CREATE TABLE ${table} (id BIGINT PRIMARY KEY, owner TEXT)`,
        [],
    );
    await tenantB.execute(
        `CREATE TABLE ${table} (id BIGINT PRIMARY KEY, owner TEXT)`,
        [],
    );

    // Each tenant inserts the SAME primary key with different values.
    // In a non-isolated database, the second INSERT would fail or
    // overwrite the first.
    await tenantA.execute(
        `INSERT INTO ${table} VALUES (1, 'tenant-a-record')`,
        [],
    );
    await tenantB.execute(
        `INSERT INTO ${table} VALUES (1, 'tenant-b-record')`,
        [],
    );

    // Each tenant sees only its own row.
    const aResult = await tenantA.query(
        `SELECT owner FROM ${table} WHERE id = 1`,
        [],
    );
    const bResult = await tenantB.query(
        `SELECT owner FROM ${table} WHERE id = 1`,
        [],
    );

    const aOwner = aResult.rows[0]?.owner;
    const bOwner = bResult.rows[0]?.owner;

    if (aOwner !== "tenant-a-record") {
        console.error(`FAIL: tenant A saw '${aOwner}', expected 'tenant-a-record'`);
        process.exit(1);
    }
    if (bOwner !== "tenant-b-record") {
        console.error(`FAIL: tenant B saw '${bOwner}', expected 'tenant-b-record'`);
        process.exit(1);
    }

    // Row counts: each tenant sees exactly 1 row, not 2.
    const aCount = await tenantA.query(
        `SELECT COUNT(*) AS n FROM ${table}`,
        [],
    );
    const bCount = await tenantB.query(
        `SELECT COUNT(*) AS n FROM ${table}`,
        [],
    );

    const aN = Number(aCount.rows[0]?.n ?? 0);
    const bN = Number(bCount.rows[0]?.n ?? 0);

    if (aN !== 1 || bN !== 1) {
        console.error(
            `FAIL: each tenant should see 1 row; got A=${aN}, B=${bN}`,
        );
        process.exit(1);
    }

    await tenantA.close();
    await tenantB.close();
    console.log("KMB_COOKBOOK_OK");
}

main().catch((err) => {
    console.error("FAIL:", err);
    process.exit(1);
});
