/**
 * Real-time subscribe example (protocol v2).
 *
 * Run a Kimberlite server on 127.0.0.1:5432, then:
 *
 *   npx ts-node examples/subscribe-example.ts
 *
 * The example appends 10 events to a fresh stream and then consumes them
 * via a subscription iterator.
 */

import { Client, DataClass } from '../src';

async function main(): Promise<void> {
  const client = await Client.connect({
    addresses: ['127.0.0.1:5432'],
    tenantId: 1n,
  });

  try {
    const streamId = await client.createStream('subscribe_demo', DataClass.Public);
    console.log(`Created stream ${streamId}`);

    // Produce 10 events.
    await client.append(
      streamId,
      Array.from({ length: 10 }, (_, i) =>
        Buffer.from(JSON.stringify({ index: i, ts: Date.now() })),
      ),
    );

    // Subscribe and consume.
    const sub = await client.subscribe(streamId, { initialCredits: 16 });
    console.log(`Subscription ${sub.id} opened with ${sub.credits} credits`);

    let count = 0;
    for await (const event of sub) {
      console.log(`offset=${event.offset}: ${event.data.toString('utf-8')}`);
      count += 1;
      if (count >= 10) {
        await sub.unsubscribe();
        break;
      }
    }
    console.log(`Received ${count} events; close reason: ${sub.closeReason}`);
  } finally {
    await client.disconnect();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
