// Kimberlite Cookbook — Real-time subscriptions in TypeScript.
//
// AUDIT-2026-05 M-7 — closes the v0.7.0 ROADMAP gap "Notebar still
// believes the client is pull-only" by giving the next consumer
// integrating Kimberlite a one-file walking-skeleton example.
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

    // Use a per-run unique stream so this example is repeatable
    // without DROP plumbing. Notebar's integration tests use the
    // same fixture pattern (v0.6.2 CHANGELOG "Per-invocation unique
    // stream names").
    const streamSuffix = Math.random().toString(36).slice(2, 10);
    const streamId = `cookbook_subscriptions_${streamSuffix}`;
    await client.createStream(streamId);

    // Append a small batch of events. We'll subscribe and see them
    // delivered via push — no polling.
    const eventCount = 5;
    for (let i = 0; i < eventCount; i++) {
        await client.append(streamId, [{ ordinal: i, payload: `event-${i}` }]);
    }

    // Open the subscription. Credits gate how many events the server
    // will push without acknowledgement; lowWater is the auto-refill
    // threshold. Defaults are sensible — overridden here for clarity.
    const subscription = await client.subscribe(streamId, {
        startOffset: 0n,
        initialCredits: 16,
        lowWater: 4,
    });

    // AsyncIterable surface — drive with `for await`.
    let received = 0;
    try {
        for await (const event of subscription) {
            console.log(`got event ${received}: ${JSON.stringify(event.payload)}`);
            received++;
            if (received >= eventCount) {
                break;
            }
        }
    } finally {
        // Idempotent — safe to call from cleanup paths (React
        // useEffect return, signal handlers, etc.).
        await subscription.unsubscribe();
    }

    if (received !== eventCount) {
        console.error(
            `FAIL: expected ${eventCount} events, got ${received}`,
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
