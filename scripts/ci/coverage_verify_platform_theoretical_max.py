#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

import coverage_theoretical_max


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--platform",
        choices=("linux", "macos", "windows"),
        required=True,
        help="Platform whose theoretical max should be enforced.",
    )
    parser.add_argument(
        "--lcov",
        type=Path,
        required=True,
        help="Path to the platform's llvm-cov lcov export (optionally .gz).",
    )
    parser.add_argument(
        "--theoretical-max",
        type=Path,
        required=True,
        help="Path to coverage-theoretical-max.json produced by coverage_theoretical_max.py.",
    )
    parser.add_argument(
        "--fail-on-missed-coverable-lines",
        action="store_true",
        help="Exit non-zero when the platform misses a coverable line.",
    )
    return parser.parse_args()


def write_summary(lines: list[str]) -> None:
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if not summary_path:
        return
    with Path(summary_path).open("a", encoding="utf-8") as handle:
        handle.write("\n".join(lines) + "\n")


def main() -> int:
    args = parse_args()
    root = coverage_theoretical_max.repo_root()
    actual = coverage_theoretical_max.parse_lcov(args.lcov, root)

    expected_payload = json.loads(args.theoretical_max.read_text(encoding="utf-8"))
    expected_platform = expected_payload["platforms"][args.platform]

    decimal_places = int(
        expected_payload.get(
            "decimal_places", coverage_theoretical_max.DEFAULT_DECIMAL_PLACES
        )
    )

    union_lines = int(expected_payload["union"]["instrumented_lines"])
    expected_instrumented = int(expected_platform["instrumented_lines"])
    expected_theoretical_percent = str(expected_platform["theoretical_max_lines_percent"])

    instrumented = len(actual.instrumented)
    covered = len(actual.covered)
    missed = instrumented - covered

    actual_percent = coverage_theoretical_max.format_ratio_percent_decimal(
        covered, union_lines, decimal_places
    )
    theoretical_percent = coverage_theoretical_max.format_ratio_percent_decimal(
        instrumented, union_lines, decimal_places
    )

    summary_lines = [
        "## Coverage theoretical max enforcement (lines)",
        "",
        f"- Platform: `{args.platform}`",
        f"- Union instrumented lines: `{union_lines}`",
        f"- Instrumented lines: `{instrumented}`",
        f"- Covered lines: `{covered}`",
        f"- Missed coverable lines: `{missed}`",
        f"- Theoretical max (computed): `{theoretical_percent}%`",
        f"- Theoretical max (expected): `{expected_theoretical_percent}%`",
        f"- Actual: `{actual_percent}%`",
    ]
    write_summary(summary_lines)

    if instrumented != expected_instrumented:
        print(
            "ERROR: instrumented line count mismatch for "
            f"{args.platform}: expected {expected_instrumented}, got {instrumented}",
            file=sys.stderr,
        )
        return 1

    if theoretical_percent != expected_theoretical_percent:
        print(
            "ERROR: theoretical max percent mismatch for "
            f"{args.platform}: expected {expected_theoretical_percent}%, got {theoretical_percent}%",
            file=sys.stderr,
        )
        return 1

    if args.fail_on_missed_coverable_lines and missed > 0:
        print(
            f"ERROR: missed coverable lines on {args.platform}: {missed}",
            file=sys.stderr,
        )
        return 1

    if missed == 0 and actual_percent != expected_theoretical_percent:
        print(
            "ERROR: percent mismatch despite full coverage for "
            f"{args.platform}: expected {expected_theoretical_percent}%, got {actual_percent}%",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
