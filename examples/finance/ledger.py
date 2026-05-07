#!/usr/bin/env python3
"""End-to-end finance ledger walkthrough — Python SDK.

Prerequisites:

    # Start server + load schema:
    examples/finance/00-setup.sh

    # Install the SDK (from the repo root):
    pip install -e sdks/python

Run:

    python examples/finance/ledger.py

The script walks the audit-trail, RBAC, time-travel, and consent-erasure
flows that a regulated fintech would need on day one. Mirror of the
healthcare clinic.py — same SDK shape, swapped domain.
"""

from __future__ import annotations

import os
from dataclasses import dataclass

from kimberlite import Client, Pool
from kimberlite.errors import KimberliteError
from kimberlite.query_builder import Query
from kimberlite.value import Value


# ---------------------------------------------------------------------------
# Dataclasses we'll project query rows into.
# ---------------------------------------------------------------------------


@dataclass
class Account:
    id: int
    account_number: str
    account_type: str
    owner_name: str
    status: str


@dataclass
class Trade:
    id: int
    account_id: int
    trade_date: str
    symbol: str
    side: str
    quantity: int
    price_cents: int
    total_cents: int
    trader_id: str


@dataclass
class Position:
    account_id: int
    symbol: str
    quantity: int
    avg_cost_cents: int
    market_value_cents: int | None


# ---------------------------------------------------------------------------


def main() -> None:
    address = os.environ.get("KIMBERLITE_ADDR", "127.0.0.1:5432")
    tenant_id = 1

    with Pool(address=address, tenant_id=tenant_id, max_size=8) as pool:
        with Client.connect(addresses=[address], tenant_id=tenant_id) as admin:
            print("✓ pool + admin client ready")

            # 1. Admin — list tables.
            tables = admin.admin.list_tables()
            names = ", ".join(t.name for t in tables)
            print(f"✓ admin.list_tables → {len(tables)} tables: {names}")

            # 2. Typed row mapping — list active accounts.
            with pool.acquire() as client:
                accounts = client.query_model(
                    "SELECT id, account_number, account_type, owner_name, status "
                    "FROM accounts WHERE status = $1 ORDER BY id",
                    [Value.text("Active")],
                    model=Account,
                )
            print(f"✓ typed query → {len(accounts)} active account(s)")
            for a in accounts:
                print(
                    f"  · #{a.id} {a.account_number} ({a.account_type}) → "
                    f"{a.owner_name}"
                )

            # 3. Trades for a specific account, projected into Trade.
            with pool.acquire() as client:
                trades = client.query_model(
                    "SELECT id, account_id, trade_date, symbol, side, quantity, "
                    "price_cents, total_cents, trader_id "
                    "FROM trades WHERE account_id = $1 ORDER BY trade_date, id",
                    [Value.bigint(1)],
                    model=Trade,
                )
            print(
                f"✓ typed query → account 1 has {len(trades)} trade(s) "
                "in immutable history"
            )
            for t in trades:
                dollars = t.total_cents / 100
                print(
                    f"  · {t.trade_date} {t.side} {t.quantity} {t.symbol} "
                    f"@ ${t.price_cents / 100:.2f} = ${dollars:,.2f} "
                    f"by {t.trader_id}"
                )

            # 4. Query builder — filter to BUYs only.
            q = (
                Query.from_table("trades")
                .select(["id", "symbol", "quantity", "price_cents"])
                .where_eq("side", Value.text("BUY"))
                .order_by("id")
                .build()
            )
            sql, params = q
            with pool.acquire() as client:
                buys = client.query(sql, params)
            print(f"✓ query-builder → {len(buys.rows)} BUY trade(s) recorded")

            # 5. Time-travel — reconstruct positions as of an earlier timestamp.
            #    The TSLA position was opened on 2024-01-16; querying as of
            #    2024-01-15 should NOT show it.
            with pool.acquire() as client:
                positions = client.query_model(
                    "SELECT account_id, symbol, quantity, avg_cost_cents, "
                    "market_value_cents FROM positions "
                    "AS OF TIMESTAMP '2024-01-15T23:59:59Z' "
                    "ORDER BY account_id, symbol",
                    [],
                    model=Position,
                )
            print(
                f"✓ time-travel → {len(positions)} position(s) as of "
                "2024-01-15 EOD"
            )
            for p in positions:
                print(
                    f"  · account {p.account_id}: {p.quantity} {p.symbol} "
                    f"@ avg ${p.avg_cost_cents / 100:.2f}"
                )

            # 6. Consent — KYC subject under GDPR Article 6 lawful basis.
            #    Fintechs need this for any EU-resident account holder.
            subject_id = "account:2"  # Sarah Chen (Individual)
            granted = admin.compliance.consent.grant(
                subject_id, "Contract"
            )
            print(
                f"✓ compliance.consent.grant → "
                f"basis=Contract consent_id={granted.consent_id}"
            )
            ok = admin.compliance.consent.check(subject_id, "Contract")
            print(f"  · consent.check({subject_id}, 'Contract') → {ok}")

            # 7. Erasure — GDPR Article 17. Note SEC 17a-4 typically requires
            #    a 6-year retention, so in production a fintech would hold
            #    erasure pending the retention window. The SDK still tracks
            #    the request and per-stream completion.
            req = admin.compliance.erasure.request(subject_id)
            print(
                f"✓ erasure.request → request_id={req.request_id} "
                f"status={req.status.kind}"
            )
            if req.streams_affected:
                admin.compliance.erasure.mark_progress(
                    req.request_id, list(req.streams_affected)
                )
                print(
                    f"  · mark_progress for {len(req.streams_affected)} "
                    "stream(s)"
                )
            print(
                "  · complete() skipped — SEC 17a-4 retention applies in "
                "production; see docs/concepts/data-portability.md"
            )

        # 8. Pool stats.
        stats = pool.stats()
        print(
            f"✓ pool.stats → open={stats.open} in_use={stats.in_use} "
            f"idle={stats.idle}"
        )

        print("\n✅ ledger walkthrough complete")


if __name__ == "__main__":
    try:
        main()
    except KimberliteError as e:
        print(f"❌ ledger walkthrough failed: {e}")
        raise
