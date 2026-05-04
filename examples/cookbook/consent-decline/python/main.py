"""Kimberlite Cookbook — Consent decline round-trip in Python.

Companion to the TS recipe.
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

    subject_id = f"subject_{secrets.token_hex(4)}"
    terms_version = "2026-05-04"

    client.compliance.consent.grant(
        subject_id=subject_id,
        purpose="Research",
        basis=kimberlite.compliance.ConsentBasis(
            article="GDPR-Art-6-1-a",
            justification="explicit consent",
        ),
        terms_version=terms_version,
        accepted=False,
    )

    audit_rows = client.compliance.audit.query(
        subject_id=subject_id,
        action="ConsentGranted",
        limit=10,
    )
    if len(audit_rows.rows) != 1:
        print(
            f"FAIL: expected 1 audit row, got {len(audit_rows.rows)}",
            file=sys.stderr,
        )
        return 1

    row = audit_rows.rows[0]
    if row.terms_version != terms_version:
        print(
            f"FAIL: terms_version {row.terms_version!r} ≠ {terms_version!r}",
            file=sys.stderr,
        )
        return 1
    if row.accepted is not False:
        print(f"FAIL: accepted={row.accepted!r} (expected False)", file=sys.stderr)
        return 1

    client.close()
    print("KMB_COOKBOOK_OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
