"""ROADMAP v0.5.1 — Python mirror of ``@kimberlitedb/client/testing``.

Spawns a ``kimberlite-test-harness-cli`` subprocess, reads
``ADDR=127.0.0.1:<port>`` from stdout, and connects a normal
:class:`kimberlite.Client` to that address. The harness owns a
fresh tempdir + in-process server; disposing the handle shuts the
child down and reclaims the tempdir.

Example::

    from kimberlite.testing import create_test_kimberlite, dispose_test_kimberlite

    harness = create_test_kimberlite()
    try:
        harness.client.execute(
            "CREATE TABLE t (id BIGINT PRIMARY KEY, name TEXT NOT NULL)", []
        )
        harness.client.execute(
            "INSERT INTO t (id, name) VALUES ($1, $2)", [1, "Ada"]
        )
        rs = harness.client.query(
            "SELECT UPPER(name) FROM t WHERE id = $1", [1]
        )
        assert rs.rows == [["ADA"]]
    finally:
        dispose_test_kimberlite(harness)
"""

from __future__ import annotations

import os
import subprocess
import threading
import time
from dataclasses import dataclass
from typing import Optional

from .client import Client

_DEFAULT_BINARY = "kimberlite-test-harness-cli"
_DEFAULT_READY_TIMEOUT_S = 15.0
_DEFAULT_TENANT = 1_000_000


@dataclass
class TestKimberlite:
    """Handle returned by :func:`create_test_kimberlite`.

    Pass to :func:`dispose_test_kimberlite` at teardown.
    """

    addr: str
    """Loopback ``host:port`` the harness bound to."""
    tenant: int
    """Tenant id the client is scoped to."""
    client: Client
    """Ready-to-use SDK client. Use exactly as in production."""
    _child: "subprocess.Popen[str]"
    """Internal subprocess handle — do not terminate directly."""


def create_test_kimberlite(
    *,
    tenant: Optional[int] = None,
    backend: str = "tempdir",
    binary_path: Optional[str] = None,
    ready_timeout_s: float = _DEFAULT_READY_TIMEOUT_S,
) -> TestKimberlite:
    """Spawn a fresh in-process Kimberlite instance.

    Args:
        tenant: Override the tenant id the harness client binds to.
            Defaults to ``1_000_000`` — the same value the Rust crate
            uses.
        backend: Which Rust-side storage backend to use. ``"tempdir"``
            (default) is the real on-disk backend. ``"memory"`` (v0.6.0)
            is the pure in-memory
            ``kimberlite_storage::MemoryStorage`` — no fsync, no disk.
            Use for hot-path test workloads where restart/recovery
            semantics are not under test.
        binary_path: Path to the harness launcher binary. Defaults to
            the ``KIMBERLITE_TEST_HARNESS_BIN`` environment variable,
            falling back to ``kimberlite-test-harness-cli`` on
            ``$PATH``. CI pipelines should set this explicitly to the
            workspace-local
            ``target/debug/kimberlite-test-harness-cli``.
        ready_timeout_s: Timeout for the child process to emit its
            ``ADDR=`` line. Default: 15s — matches the harness's cold
            spin-up budget (<50ms on CI) with plenty of slack for slow
            Docker runners.

    Returns:
        A :class:`TestKimberlite` handle. Pass to
        :func:`dispose_test_kimberlite` at teardown.
    """
    if backend not in ("tempdir", "memory"):
        raise ValueError(
            f"unknown backend {backend!r}; expected 'tempdir' or 'memory'"
        )

    binary = binary_path or os.environ.get("KIMBERLITE_TEST_HARNESS_BIN") or _DEFAULT_BINARY

    args = [binary]
    if tenant is not None:
        args.append(f"--tenant={tenant}")
    if backend != "tempdir":
        # Only pass the flag when we diverge from the default — keeps
        # forward compatibility with older harness binaries that don't
        # recognise `--backend=`.
        args.append(f"--backend={backend}")

    child = subprocess.Popen(
        args,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=None,  # inherit — parent sees tracing output directly
        text=True,
        bufsize=1,  # line-buffered
    )

    # Read the machine-readable ADDR=/TENANT= prelude in a background
    # thread so we can honour `ready_timeout_s` without deadlocking on
    # child.stdout.readline().
    addr: Optional[str] = None
    tenant_id: Optional[int] = None
    err: list[BaseException] = []
    done = threading.Event()

    def _reader() -> None:
        nonlocal addr, tenant_id
        try:
            assert child.stdout is not None
            while addr is None or tenant_id is None:
                line = child.stdout.readline()
                if not line:
                    raise RuntimeError(
                        f"kimberlite-test-harness-cli exited before emitting ADDR "
                        f"(exit code {child.poll()})"
                    )
                line = line.strip()
                if line.startswith("ADDR="):
                    addr = line[len("ADDR="):]
                elif line.startswith("TENANT="):
                    tenant_id = int(line[len("TENANT="):])
        except BaseException as e:  # noqa: BLE001
            err.append(e)
        finally:
            done.set()

    t = threading.Thread(target=_reader, daemon=True)
    t.start()
    got_ready = done.wait(timeout=ready_timeout_s)

    if not got_ready:
        _terminate(child)
        raise TimeoutError(
            f"kimberlite-test-harness-cli did not emit ADDR within {ready_timeout_s}s"
        )
    if err:
        _terminate(child)
        raise RuntimeError(str(err[0]))
    if addr is None or tenant_id is None:
        _terminate(child)
        raise RuntimeError("internal: harness prelude completed without addr/tenant")

    client = Client.connect(addresses=[addr], tenant_id=tenant_id)
    return TestKimberlite(addr=addr, tenant=tenant_id, client=client, _child=child)


def dispose_test_kimberlite(harness: TestKimberlite) -> None:
    """Dispose a harness returned by :func:`create_test_kimberlite`.

    Writes the ``shutdown`` IPC signal, waits up to 5s for the child
    to exit, then SIGKILLs if needed.
    """
    try:
        harness.client.disconnect()
    except Exception:  # noqa: BLE001 — best-effort
        pass
    try:
        if harness._child.stdin is not None:
            harness._child.stdin.write("shutdown\n")
            harness._child.stdin.close()
    except Exception:  # noqa: BLE001
        pass
    try:
        harness._child.wait(timeout=5.0)
    except subprocess.TimeoutExpired:
        _terminate(harness._child)


def _terminate(child: "subprocess.Popen[str]") -> None:
    """Best-effort termination — TERM then KILL."""
    try:
        child.terminate()
        child.wait(timeout=2.0)
    except Exception:  # noqa: BLE001
        try:
            child.kill()
        except Exception:  # noqa: BLE001
            pass
    _ = time  # silence unused-import when this helper is the only path
