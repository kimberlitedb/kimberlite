"""Kimberlite Cookbook — Real-time subscriptions in Python.

AUDIT-2026-05 M-7. Companion to the TS recipe — same primitive
contract, same KMB_COOKBOOK_OK exit signal so CI gates uniformly.

Prerequisite: a running `kmb-server` on localhost:5432
(e.g. `just kmb-server-dev`).

Run:
    python main.py
"""

from __future__ import annotations

import os
import secrets
import sys

import kimberlite

KMB_ADDRESS = os.environ.get("KMB_ADDRESS", "localhost:5432")
KMB_TENANT = int(os.environ.get("KMB_TENANT", "1"))


def main() -> int:
    client = kimberlite.Client.connect(address=KMB_ADDRESS, tenant_id=KMB_TENANT)

    # Per-run unique stream — see notebar's integration-fixture
    # pattern for the rationale.
    stream_suffix = secrets.token_hex(4)
    stream_id = f"cookbook_subscriptions_{stream_suffix}"
    client.create_stream(stream_id)

    event_count = 5
    for i in range(event_count):
        client.append(stream_id, [{"ordinal": i, "payload": f"event-{i}"}])

    subscription = client.subscribe(
        stream_id,
        start_offset=0,
        initial_credits=16,
        low_water=4,
    )

    received = 0
    try:
        for event in subscription:
            print(f"got event {received}: {event.payload!r}")
            received += 1
            if received >= event_count:
                break
    finally:
        subscription.unsubscribe()

    if received != event_count:
        print(f"FAIL: expected {event_count} events, got {received}", file=sys.stderr)
        return 1

    client.close()
    print("KMB_COOKBOOK_OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
