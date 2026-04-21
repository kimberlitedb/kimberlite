"""ROADMAP v0.5.1 — smoke test for :mod:`kimberlite.testing`.

Skipped by default because it requires ``kimberlite-test-harness-cli``
to be built and on ``$PATH`` (or pointed at via
``KIMBERLITE_TEST_HARNESS_BIN``). CI sets
``KIMBERLITE_TEST_HARNESS_BIN`` after
``cargo build -p kimberlite-test-harness --bin
kimberlite-test-harness-cli``.
"""

from __future__ import annotations

import os
import shutil

import pytest

from kimberlite.testing import create_test_kimberlite, dispose_test_kimberlite


def _harness_available() -> bool:
    binary = os.environ.get("KIMBERLITE_TEST_HARNESS_BIN")
    if binary and os.path.isfile(binary):
        return True
    if shutil.which("kimberlite-test-harness-cli") is not None:
        return True
    return False


@pytest.mark.skipif(
    not _harness_available(),
    reason="kimberlite-test-harness-cli not on PATH and KIMBERLITE_TEST_HARNESS_BIN unset",
)
def test_create_and_dispose_roundtrips_select() -> None:
    harness = create_test_kimberlite(tenant=42)
    try:
        assert harness.addr.startswith("127.0.0.1:")
        assert harness.tenant == 42

        harness.client.execute(
            "CREATE TABLE t (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
            [],
        )
        harness.client.execute(
            "INSERT INTO t (id, name) VALUES ($1, $2)",
            [1, "Ada"],
        )
        rs = harness.client.query(
            "SELECT UPPER(name) FROM t WHERE id = $1",
            [1],
        )
        assert len(rs.rows) == 1
        # Output layout is [upper] — single scalar projection.
        assert rs.rows[0] == ["ADA"]
    finally:
        dispose_test_kimberlite(harness)
