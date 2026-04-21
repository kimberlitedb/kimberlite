#!/usr/bin/env python3
"""
Validate VOPR coverage JSON artifacts against nightly thresholds.

Inputs:
    One or more VOPR run JSON files (``vopr-results/*.json``), produced by
    ``vopr --scenario ... --json``. Each file must carry a top-level
    ``coverage`` object with at least ``fault_point_coverage``,
    ``invariant_executions`` and ``view_changes`` fields.

Behavior:
    Computes the aggregate fault-point coverage and invariant-exercise count
    across all inputs. Compares against an absolute minimum *and* a rolling
    historical baseline (``.artifacts/vopr-trends/``) written by
    ``vopr-nightly.yml``. Fails (exit 1) when either threshold is violated by
    more than --regression-threshold (default 5%).

    The script is intentionally read-only — it never mutates the trend files.
    ``vopr-nightly.yml`` owns trend writes.

Design (pressurecraft):
    Pure-function style. Inputs are JSON files; the only output is an exit
    code plus human-readable stderr. Deterministic, no network/clock use.
    Every contract assumption is an explicit assert with a clear message.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Iterable


# Thresholds calibrated against the v0.4.x baseline (CHANGELOG "VOPR Enhanced
# Testing" — >70k sims/sec, 74 scenarios). Bump these upward in follow-up
# releases as baseline stabilises; never downward.
DEFAULT_MIN_FAULT_COVERAGE = 0.45  # aggregate fraction, 0..1
DEFAULT_MIN_INVARIANT_EXERCISES = 50  # distinct invariant executions observed
DEFAULT_REGRESSION_THRESHOLD = 0.05  # 5% relative drop vs rolling baseline


def _load_json(path: Path) -> dict:
    assert path.exists(), f"coverage input does not exist: {path}"
    assert path.is_file(), f"coverage input is not a file: {path}"
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    assert isinstance(payload, dict), f"coverage payload is not an object: {path}"
    return payload


def _read_trend(path: Path) -> list[float]:
    """Read newline-separated floats from a trend file; returns [] if absent."""
    if not path.exists():
        return []
    values: list[float] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            values.append(float(line))
        except ValueError:
            # Skip malformed rows (e.g. jq emitted "null"); don't fail the run.
            continue
    return values


def _summarise(runs: list[dict]) -> tuple[float, int, int]:
    """Return (max fault_point_coverage, total invariants exercised, total view changes)."""
    assert runs, "at least one VOPR run must be provided"
    fault = 0.0
    invariants = 0
    view_changes = 0
    for run in runs:
        coverage = run.get("coverage", {})
        fault = max(fault, float(coverage.get("fault_point_coverage", 0.0)))
        invariant_list = coverage.get("invariant_executions", [])
        assert isinstance(invariant_list, list), "invariant_executions must be a list"
        invariants += len(invariant_list)
        view_changes += int(coverage.get("view_changes", 0))
    return fault, invariants, view_changes


def _baseline_mean(values: Iterable[float], window: int = 14) -> float | None:
    """Rolling mean over the most-recent ``window`` values; None when we have <3 samples."""
    recent = list(values)[-window:]
    if len(recent) < 3:
        return None
    return sum(recent) / len(recent)


def validate(
    inputs: list[Path],
    *,
    min_fault: float,
    min_invariants: int,
    regression_threshold: float,
    trends_dir: Path,
) -> int:
    runs = [_load_json(p) for p in inputs]
    fault, invariants, view_changes = _summarise(runs)

    errors: list[str] = []
    warnings: list[str] = []

    # Absolute minima (hard floors)
    if fault < min_fault:
        errors.append(
            f"fault_point_coverage {fault:.4f} below minimum {min_fault:.4f}"
        )
    if invariants < min_invariants:
        errors.append(
            f"invariant_executions {invariants} below minimum {min_invariants}"
        )

    # Rolling-baseline regression check
    fault_trend = _read_trend(trends_dir / "fault-coverage.txt")
    invariant_trend = _read_trend(trends_dir / "invariant-count.txt")

    fault_mean = _baseline_mean(fault_trend)
    if fault_mean is not None and fault < fault_mean * (1 - regression_threshold):
        errors.append(
            f"fault_point_coverage {fault:.4f} regressed >{regression_threshold:.0%} "
            f"vs rolling baseline {fault_mean:.4f}"
        )
    elif fault_mean is None:
        warnings.append("fault-coverage baseline has <3 samples; skipping regression check")

    invariant_mean = _baseline_mean(invariant_trend)
    if invariant_mean is not None and invariants < invariant_mean * (1 - regression_threshold):
        errors.append(
            f"invariant_executions {invariants} regressed >{regression_threshold:.0%} "
            f"vs rolling baseline {invariant_mean:.1f}"
        )

    # Report
    print(f"VOPR coverage summary ({len(runs)} run(s)):", file=sys.stderr)
    print(f"  fault_point_coverage: {fault:.4f}", file=sys.stderr)
    print(f"  invariants exercised: {invariants}", file=sys.stderr)
    print(f"  view changes:         {view_changes}", file=sys.stderr)

    for warning in warnings:
        print(f"warn: {warning}", file=sys.stderr)

    if errors:
        for err in errors:
            print(f"error: {err}", file=sys.stderr)
        return 1

    print("ok: VOPR coverage within thresholds", file=sys.stderr)
    return 0


def _parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", nargs="+", type=Path, help="VOPR run JSON file(s)")
    parser.add_argument(
        "--min-fault-coverage",
        type=float,
        default=DEFAULT_MIN_FAULT_COVERAGE,
        help="Absolute floor for fault_point_coverage (0..1)",
    )
    parser.add_argument(
        "--min-invariants",
        type=int,
        default=DEFAULT_MIN_INVARIANT_EXERCISES,
        help="Absolute floor for total invariant_executions entries",
    )
    parser.add_argument(
        "--regression-threshold",
        type=float,
        default=DEFAULT_REGRESSION_THRESHOLD,
        help="Relative drop vs rolling baseline that counts as a regression",
    )
    parser.add_argument(
        "--trends-dir",
        type=Path,
        default=Path(".artifacts/vopr-trends"),
        help="Directory containing rolling trend files written by vopr-nightly.yml",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv if argv is not None else sys.argv[1:])
    assert 0.0 <= args.min_fault_coverage <= 1.0, "min-fault-coverage must be in [0,1]"
    assert args.min_invariants >= 0, "min-invariants must be non-negative"
    assert 0.0 <= args.regression_threshold <= 1.0, "regression-threshold must be in [0,1]"
    return validate(
        args.inputs,
        min_fault=args.min_fault_coverage,
        min_invariants=args.min_invariants,
        regression_threshold=args.regression_threshold,
        trends_dir=args.trends_dir,
    )


if __name__ == "__main__":
    sys.exit(main())
