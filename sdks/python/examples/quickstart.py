#!/usr/bin/env python3
"""Quickstart example for Kimberlite Python SDK.

This example demonstrates basic usage of the Kimberlite client:
- Connecting to a server
- Creating a stream
- Appending events
- Reading events back

To run this example, you need a Kimberlite server running on localhost:5432.
"""

from kimberlite import Client, DataClass, ConnectionError

def main():
    """Run the quickstart example."""
    try:
        # Connect to Kimberlite server
        with Client.connect(
            addresses=["localhost:5432"],
            tenant_id=1,
            auth_token="development-token",
        ) as client:
            print("✓ Connected to Kimberlite server")

            # Create a stream for PHI data
            stream_id = client.create_stream("patient_events", DataClass.PHI)
            print(f"✓ Created stream with ID: {stream_id}")

            # Append some events
            events = [
                b'{"type": "admission", "patient_id": "P123"}',
                b'{"type": "diagnosis", "patient_id": "P123", "code": "I10"}',
                b'{"type": "discharge", "patient_id": "P123"}',
            ]
            first_offset = client.append(stream_id, events)
            print(f"✓ Appended {len(events)} events starting at offset {first_offset}")

            # Read events back
            read_events = client.read(stream_id, from_offset=first_offset, max_bytes=1024)
            print(f"✓ Read {len(read_events)} events:")

            for event in read_events:
                print(f"  Offset {event.offset}: {event.data.decode('utf-8')}")

    except ConnectionError as e:
        print(f"✗ Failed to connect: {e}")
        print("\nMake sure kmb-server is running:")
        print("  cargo run --bin kmb-server -- --port 5432 --tenant-id 1")
        return 1

    except Exception as e:
        print(f"✗ Error: {e}")
        return 1

    return 0


if __name__ == "__main__":
    exit(main())
