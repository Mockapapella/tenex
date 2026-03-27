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
        "--json",
        type=Path,
        required=True,
        help="Path to the platform's llvm-cov summary json export (optionally .gz).",
    )
    parser.add_argument(
        "--theoretical-max",
        type=Path,
        required=True,
        help="Path to coverage-theoretical-max.json produced by coverage_theoretical_max.py.",
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
    actual_platform = coverage_theoretical_max.parse_llvm_cov_summary_json(args.json, root)
    actual_totals = coverage_theoretical_max.platform_totals(actual_platform)

    expected_payload = json.loads(args.theoretical_max.read_text(encoding="utf-8"))
    expected_platform = expected_payload["platforms"][args.platform]

    decimal_places = int(
        expected_payload.get(
            "decimal_places", coverage_theoretical_max.DEFAULT_DECIMAL_PLACES
        )
    )

    summary_lines = [
        "## Coverage theoretical max enforcement",
        "",
        f"- Platform: `{args.platform}`",
        f"- Unknown sources: `{len(actual_platform.unknown_sources)}`",
        "",
        "- Union amount:",
        f"  - regions: `{expected_payload['union']['regions']['amount']}`",
        f"  - functions: `{expected_payload['union']['functions']['amount']}`",
        f"  - lines: `{expected_payload['union']['lines']['amount']}`",
        f"  - branches: `{expected_payload['union']['branches']['amount']}`",
        "",
    ]

    errors: list[str] = []

    for metric in ("lines", "functions", "regions", "branches"):
        expected_union = int(expected_payload["union"][metric]["amount"])
        expected_metric = expected_platform[metric]
        expected_amount = int(expected_metric["amount"])
        expected_theoretical_percent = str(expected_metric["theoretical_max_percent"])

        counts = actual_totals[metric]
        amount = counts.instrumented
        covered = counts.covered
        missed = amount - covered

        theoretical_percent = coverage_theoretical_max.format_ratio_percent_decimal(
            amount,
            expected_union,
            decimal_places,
            empty_is_100=True,
        )
        actual_percent = coverage_theoretical_max.format_ratio_percent_decimal(
            covered,
            expected_union,
            decimal_places,
            empty_is_100=True,
        )

        summary_lines.extend(
            [
                f"### {metric.capitalize()}",
                "",
                f"- Amount: `{amount}`",
                f"- Covered: `{covered}`",
                f"- Missed coverable: `{missed}`",
                f"- Theoretical max (computed): `{theoretical_percent}%`",
                f"- Theoretical max (expected): `{expected_theoretical_percent}%`",
                f"- Actual: `{actual_percent}%`",
                "",
            ]
        )

        if amount != expected_amount:
            errors.append(
                f"ERROR: amount {metric} count mismatch for {args.platform}: "
                f"expected {expected_amount}, got {amount}",
            )

        if theoretical_percent != expected_theoretical_percent:
            errors.append(
                f"ERROR: theoretical max percent mismatch for {metric} on {args.platform}: "
                f"expected {expected_theoretical_percent}%, got {theoretical_percent}%",
            )

        if missed > 0:
            errors.append(
                f"ERROR: missed coverable {metric} on {args.platform}: {missed}",
            )

        if missed == 0 and actual_percent != expected_theoretical_percent:
            errors.append(
                f"ERROR: percent mismatch despite full coverage for {metric} on {args.platform}: "
                f"expected {expected_theoretical_percent}%, got {actual_percent}%",
            )

    write_summary(summary_lines)
    if errors:
        for message in errors:
            print(message, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
