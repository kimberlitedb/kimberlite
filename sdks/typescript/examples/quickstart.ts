/**
 * Quickstart example for Kimberlite TypeScript SDK.
 *
 * This example demonstrates basic usage:
 * - Connecting to a server
 * - Creating a stream
 * - Appending events
 * - Reading events back
 *
 * To run: ts-node examples/quickstart.ts
 * (Requires kmb-server running on localhost:5432)
 */

import { Client, DataClass, ConnectionError } from '../src';

async function main(): Promise<number> {
  try {
    // Connect to Kimberlite server
    const client = await Client.connect({
      addresses: ['localhost:5432'],
      tenantId: 1n,
      authToken: 'development-token',
    });

    console.log('✓ Connected to Kimberlite server');

    try {
      // Create a stream for PHI data
      const streamId = await client.createStream('patient_events', DataClass.PHI);
      console.log(`✓ Created stream with ID: ${streamId}`);

      // Append some events
      const events = [
        Buffer.from('{"type": "admission", "patient_id": "P123"}'),
        Buffer.from('{"type": "diagnosis", "patient_id": "P123", "code": "I10"}'),
        Buffer.from('{"type": "discharge", "patient_id": "P123"}'),
      ];
      const firstOffset = await client.append(streamId, events);
      console.log(`✓ Appended ${events.length} events starting at offset ${firstOffset}`);

      // Read events back
      const readEvents = await client.read(streamId, { fromOffset: firstOffset, maxBytes: 1024 });
      console.log(`✓ Read ${readEvents.length} events:`);

      for (const event of readEvents) {
        console.log(`  Offset ${event.offset}: ${event.data.toString('utf-8')}`);
      }
    } finally {
      await client.disconnect();
    }

    return 0;
  } catch (error) {
    if (error instanceof ConnectionError) {
      console.error(`✗ Failed to connect: ${error.message}`);
      console.error('\nMake sure kmb-server is running:');
      console.error('  cargo run --bin kmb-server -- --port 5432 --tenant-id 1');
    } else {
      console.error(`✗ Error: ${error}`);
    }
    return 1;
  }
}

main().then((code) => process.exit(code));
