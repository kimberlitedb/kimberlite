#!/usr/bin/env python3
"""
Violation Detection and Analysis

Analyzes VOPR output JSON to detect, classify, and prioritize invariant violations
for bug bounty submissions.

Usage:
    ./detect-violations.py results/byzantine/all_attacks.json
    ./detect-violations.py results/byzantine/*.json --summary
    ./detect-violations.py results/byzantine/view_change.json --top 5
"""

import json
import sys
from collections import defaultdict
from pathlib import Path
from typing import Dict, List, Any
import argparse

# Bounty values by invariant type
BOUNTY_VALUES = {
    "vsr_agreement": 20000,
    "vsr_prefix_property": 18000,
    "vsr_view_change_safety": 10000,
    "vsr_durability": 10000,
    "vsr_recovery_safety": 15000,
    "mvcc_snapshot_isolation": 8000,
    "tenant_isolation": 12000,
    "crypto_nonce_uniqueness": 10000,
}

# Severity levels
SEVERITY = {
    "CRITICAL": ["vsr_agreement", "vsr_prefix_property", "tenant_isolation"],
    "HIGH": ["vsr_view_change_safety", "vsr_durability", "vsr_recovery_safety"],
    "MEDIUM": ["mvcc_snapshot_isolation", "crypto_nonce_uniqueness"],
}


class ViolationAnalyzer:
    """Analyzes VOPR output for invariant violations."""

    def __init__(self):
        self.violations = []
        self.stats = defaultdict(int)
        self.seeds_by_invariant = defaultdict(list)

    def load_json_file(self, filepath: Path):
        """Load violations from a JSON file."""
        try:
            with open(filepath, "r") as f:
                data = json.load(f)

            violations = data.get("violations", [])
            for v in violations:
                self.violations.append(v)
                invariant = v.get("invariant", "unknown")
                seed = v.get("seed", 0)

                self.stats[invariant] += 1
                self.seeds_by_invariant[invariant].append(seed)

        except json.JSONDecodeError as e:
            print(f"Error parsing {filepath}: {e}", file=sys.stderr)
        except FileNotFoundError:
            print(f"File not found: {filepath}", file=sys.stderr)

    def load_multiple_files(self, filepaths: List[Path]):
        """Load violations from multiple JSON files."""
        for filepath in filepaths:
            self.load_json_file(filepath)

    def get_severity(self, invariant: str) -> str:
        """Get severity level for an invariant."""
        for level, invariants in SEVERITY.items():
            if invariant in invariants:
                return level
        return "LOW"

    def get_bounty_value(self, invariant: str) -> int:
        """Get bounty value for an invariant."""
        return BOUNTY_VALUES.get(invariant, 1000)

    def print_summary(self):
        """Print summary of violations."""
        print("\n" + "=" * 70)
        print("  VIOLATION SUMMARY")
        print("=" * 70 + "\n")

        if not self.violations:
            print("No violations detected.\n")
            return

        print(f"Total violations: {len(self.violations)}")
        print(f"Unique invariants violated: {len(self.stats)}\n")

        # Sort by bounty value
        sorted_invariants = sorted(
            self.stats.items(),
            key=lambda x: self.get_bounty_value(x[0]),
            reverse=True,
        )

        print(f"{'Invariant':<35} {'Count':<8} {'Bounty':<12} {'Severity'}")
        print("-" * 70)

        total_potential_bounty = 0
        for invariant, count in sorted_invariants:
            bounty = self.get_bounty_value(invariant)
            severity = self.get_severity(invariant)
            total_potential_bounty += bounty

            print(f"{invariant:<35} {count:<8} ${bounty:>10,} {severity}")

        print("-" * 70)
        print(f"{'TOTAL POTENTIAL':<35} {'':<8} ${total_potential_bounty:>10,}\n")

    def print_critical_violations(self):
        """Print critical violations with seeds."""
        print("\n" + "=" * 70)
        print("  CRITICAL VIOLATIONS (Ready for Bounty Submission)")
        print("=" * 70 + "\n")

        critical_found = False
        for invariant in SEVERITY["CRITICAL"]:
            if invariant in self.seeds_by_invariant:
                critical_found = True
                seeds = self.seeds_by_invariant[invariant]
                bounty = self.get_bounty_value(invariant)

                print(f"Invariant: {invariant}")
                print(f"Bounty Value: ${bounty:,}")
                print(f"Occurrences: {len(seeds)}")
                print(f"Seeds: {', '.join(map(str, seeds[:10]))}")
                if len(seeds) > 10:
                    print(f"       ... and {len(seeds) - 10} more")
                print()

        if not critical_found:
            print("No critical violations detected.\n")

    def print_top_violations(self, n: int = 5):
        """Print top N violations by bounty value."""
        print("\n" + "=" * 70)
        print(f"  TOP {n} VIOLATIONS BY BOUNTY VALUE")
        print("=" * 70 + "\n")

        sorted_invariants = sorted(
            self.stats.items(),
            key=lambda x: self.get_bounty_value(x[0]),
            reverse=True,
        )[:n]

        for i, (invariant, count) in enumerate(sorted_invariants, 1):
            bounty = self.get_bounty_value(invariant)
            severity = self.get_severity(invariant)
            seeds = self.seeds_by_invariant[invariant][:5]

            print(f"{i}. {invariant}")
            print(f"   Bounty: ${bounty:,}")
            print(f"   Severity: {severity}")
            print(f"   Occurrences: {count}")
            print(f"   Sample seeds: {', '.join(map(str, seeds))}")
            print()

    def export_json(self, output_path: Path):
        """Export analysis results as JSON."""
        results = {
            "total_violations": len(self.violations),
            "unique_invariants": len(self.stats),
            "violations_by_invariant": dict(self.stats),
            "seeds_by_invariant": {k: list(v) for k, v in self.seeds_by_invariant.items()},
            "critical_violations": [
                {
                    "invariant": inv,
                    "count": self.stats[inv],
                    "bounty": self.get_bounty_value(inv),
                    "seeds": self.seeds_by_invariant[inv],
                }
                for inv in SEVERITY["CRITICAL"]
                if inv in self.stats
            ],
        }

        with open(output_path, "w") as f:
            json.dump(results, f, indent=2)

        print(f"\nResults exported to: {output_path}\n")


def main():
    parser = argparse.ArgumentParser(
        description="Analyze VOPR output for invariant violations"
    )
    parser.add_argument(
        "files", nargs="+", type=Path, help="JSON files to analyze"
    )
    parser.add_argument(
        "--summary", action="store_true", help="Print summary only"
    )
    parser.add_argument(
        "--top", type=int, default=5, help="Number of top violations to show"
    )
    parser.add_argument(
        "--export", type=Path, help="Export results to JSON file"
    )

    args = parser.parse_args()

    analyzer = ViolationAnalyzer()

    # Load all files
    print(f"\nAnalyzing {len(args.files)} file(s)...")
    analyzer.load_multiple_files(args.files)

    # Print results
    analyzer.print_summary()

    if not args.summary:
        analyzer.print_critical_violations()
        analyzer.print_top_violations(args.top)

    # Export if requested
    if args.export:
        analyzer.export_json(args.export)


if __name__ == "__main__":
    main()
