"""Real-time subscribe example (protocol v2).

Run a Kimberlite server on 127.0.0.1:5432, then:

    python examples/subscribe_example.py

The example appends 10 events to a fresh stream and consumes them via a
Subscription iterator.
"""

import json
import time

from kimberlite import Client, DataClass


def main() -> None:
    with Client.connect(addresses=["127.0.0.1:5432"], tenant_id=1) as client:
        stream_id = client.create_stream("subscribe_demo", DataClass.PUBLIC)
        print(f"Created stream {stream_id}")

        # Produce 10 events.
        events = [
            json.dumps({"index": i, "ts": time.time()}).encode("utf-8")
            for i in range(10)
        ]
        client.append(stream_id, events)

        # Subscribe and consume.
        with client.subscribe(stream_id, initial_credits=16) as sub:
            print(f"Subscription {sub.id} opened with {sub.credits} credits")
            for i, event in enumerate(sub):
                print(f"offset={event.offset}: {event.data.decode('utf-8')}")
                if i + 1 >= 10:
                    break
            print(
                f"Closed? {sub.closed}, reason: {sub.close_reason}"
            )


if __name__ == "__main__":
    main()
