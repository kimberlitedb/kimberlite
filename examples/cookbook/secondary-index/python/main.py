"""Kimberlite Cookbook — Secondary-index lookup in Python.

Companion to the TS recipe. Same EXPLAIN-asserts-IndexScan contract.
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

    table_suffix = secrets.token_hex(3)
    table = f"messages_{table_suffix}"

    client.execute(
        f"""
        CREATE TABLE {table} (
            id BIGINT PRIMARY KEY,
            provider TEXT NOT NULL,
            provider_message_id TEXT NOT NULL,
            body TEXT
        )
        """,
    )
    client.execute(
        f"CREATE INDEX idx_{table}_provider ON {table} (provider, provider_message_id)",
    )

    for i in range(50):
        client.execute(
            f"INSERT INTO {table} (id, provider, provider_message_id, body) VALUES ($1, $2, $3, $4)",
            [i, "twilio", f"tw-{i}", f"body-{i}"],
        )

    explain = client.query(
        f"EXPLAIN SELECT id FROM {table} WHERE provider = $1 AND provider_message_id = $2",
        ["twilio", "tw-7"],
    )
    plan_text = repr(explain.rows)
    if "IndexScan" not in plan_text:
        print(f"FAIL: expected IndexScan in plan, got: {plan_text}", file=sys.stderr)
        return 1

    result = client.query(
        f"SELECT id, body FROM {table} WHERE provider = $1 AND provider_message_id = $2",
        ["twilio", "tw-7"],
    )
    if len(result.rows) != 1:
        print(f"FAIL: expected 1 row, got {len(result.rows)}", file=sys.stderr)
        return 1

    client.close()
    print("KMB_COOKBOOK_OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
