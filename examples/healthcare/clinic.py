#!/usr/bin/env python3
"""End-to-end clinic-management walkthrough — Python SDK.

Prerequisites:

    # Start server + load schema:
    examples/healthcare/00-setup.sh

    # Install the SDK (from the repo root):
    pip install -e sdks/python

Run:

    python examples/healthcare/clinic.py

The script mirrors ``clinic.ts`` and ``clinic.rs``. Data-plane ops run
through a ``Pool`` for concurrency; admin / compliance / subscribe run on a
dedicated ``Client`` because those flows are typically one-shot and the
``PooledClient`` intentionally exposes only the hot path.
"""

from __future__ import annotations

import os
from dataclasses import dataclass

from kimberlite import Client, Pool
from kimberlite.errors import KimberliteError
from kimberlite.query_builder import Query
from kimberlite.value import Value


# ---------------------------------------------------------------------------
# Dataclass we'll project query rows into.
# ---------------------------------------------------------------------------


@dataclass
class Patient:
    id: int
    medical_record_number: str
    first_name: str
    last_name: str
    date_of_birth: str
    primary_provider_id: int


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

            # 2. Typed row mapping — returns list[Patient].
            with pool.acquire() as client:
                patients = client.query_model(
                    "SELECT id, medical_record_number, first_name, last_name, "
                    "date_of_birth, primary_provider_id FROM patients "
                    "WHERE active = $1 ORDER BY id",
                    [Value.boolean(True)],
                    model=Patient,
                )
            print(f"✓ typed query → {len(patients)} active patients")
            for p in patients:
                print(
                    f"  · #{p.id} {p.first_name} {p.last_name} "
                    f"(MRN {p.medical_record_number}) → provider "
                    f"{p.primary_provider_id}"
                )

            # 3. Query builder — fluent composition.
            q = (
                Query.from_table("patients")
                .select(["id", "first_name", "last_name"])
                .where_eq("primary_provider_id", Value.bigint(2))
                .order_by("id")
                .build()
            )
            sql, params = q
            with pool.acquire() as client:
                dr_chen = client.query(sql, params)
            print(
                f"✓ query-builder → Dr. Chen has {len(dr_chen.rows)} patient(s)"
            )

            # 4. Consent — grant research consent for patient 1.
            subject_id = "patient:1"
            granted = admin.compliance.consent.grant(subject_id, "Research")
            print(
                f"✓ compliance.consent.grant → "
                f"consent_id={granted.consent_id}"
            )
            ok = admin.compliance.consent.check(subject_id, "Research")
            print(f"  · consent.check({subject_id}, 'Research') → {ok}")

            # 5. Real-time subscribe — see docs/reference/sdk/python-api.md
            #    for the full pattern:
            #
            #      stream = admin.create_stream('encounters', DataClass.PHI)
            #      with admin.subscribe(stream, initial_credits=128) as sub:
            #          for ev in sub:
            #              dashboard.push(ev)
            print(
                "✓ subscribe → skipped "
                "(see docs/reference/sdk/python-api.md for a full example)"
            )

            # 6. Erasure — GDPR Article 17.
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
                "  · complete() skipped in demo — see "
                "docs/concepts/data-portability.md"
            )

        # 7. Pool stats.
        stats = pool.stats()
        print(
            f"✓ pool.stats → open={stats.open} in_use={stats.in_use} "
            f"idle={stats.idle}"
        )

        print("\n✅ clinic walkthrough complete")


if __name__ == "__main__":
    try:
        main()
    except KimberliteError as e:
        print(f"❌ clinic walkthrough failed: {e}")
        raise
