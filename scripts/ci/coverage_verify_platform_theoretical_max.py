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
    parser.add_argument(
        "--fail-on-missed-coverable",
        action="store_true",
        help="Exit non-zero when the platform misses a coverable region/function/line/branch.",
    )
    parser.add_argument(
        "--fail-on-missed-coverable-lines",
        action="store_true",
        help="Exit non-zero when the platform misses a coverable line.",
    )
    parser.add_argument(
        "--fail-on-missed-coverable-functions",
        action="store_true",
        help="Exit non-zero when the platform misses a coverable function.",
    )
    parser.add_argument(
        "--fail-on-missed-coverable-regions",
        action="store_true",
        help="Exit non-zero when the platform misses a coverable region.",
    )
    parser.add_argument(
        "--fail-on-missed-coverable-branches",
        action="store_true",
        help="Exit non-zero when the platform misses a coverable branch.",
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

    want_all = bool(args.fail_on_missed_coverable)
    want_lines = want_all or bool(args.fail_on_missed_coverable_lines)
    want_functions = want_all or bool(args.fail_on_missed_coverable_functions)
    want_regions = want_all or bool(args.fail_on_missed_coverable_regions)
    want_branches = want_all or bool(args.fail_on_missed_coverable_branches)

    summary_lines = [
        "## Coverage theoretical max enforcement",
        "",
        f"- Platform: `{args.platform}`",
        f"- Unknown sources: `{len(actual_platform.unknown_sources)}`",
        "",
        "- Union instrumented:",
        f"  - regions: `{expected_payload['union']['regions']['instrumented']}`",
        f"  - functions: `{expected_payload['union']['functions']['instrumented']}`",
        f"  - lines: `{expected_payload['union']['lines']['instrumented']}`",
        f"  - branches: `{expected_payload['union']['branches']['instrumented']}`",
        "",
    ]

    for metric in ("lines", "functions", "regions", "branches"):
        expected_union = int(expected_payload["union"][metric]["instrumented"])
        expected_metric = expected_platform[metric]
        expected_instrumented = int(expected_metric["instrumented"])
        expected_theoretical_percent = str(expected_metric["theoretical_max_percent"])

        counts = actual_totals[metric]
        instrumented = counts.instrumented
        covered = counts.covered
        missed = instrumented - covered

        theoretical_percent = coverage_theoretical_max.format_ratio_percent_decimal(
            instrumented,
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
                f"- Instrumented: `{instrumented}`",
                f"- Covered: `{covered}`",
                f"- Missed coverable: `{missed}`",
                f"- Theoretical max (computed): `{theoretical_percent}%`",
                f"- Theoretical max (expected): `{expected_theoretical_percent}%`",
                f"- Actual: `{actual_percent}%`",
                "",
            ]
        )

        if instrumented != expected_instrumented:
            print(
                f"ERROR: instrumented {metric} count mismatch for {args.platform}: "
                f"expected {expected_instrumented}, got {instrumented}",
                file=sys.stderr,
            )
            return 1

        if theoretical_percent != expected_theoretical_percent:
            print(
                f"ERROR: theoretical max percent mismatch for {metric} on {args.platform}: "
                f"expected {expected_theoretical_percent}%, got {theoretical_percent}%",
                file=sys.stderr,
            )
            return 1

        if metric == "lines" and want_lines and missed > 0:
            print(f"ERROR: missed coverable lines on {args.platform}: {missed}", file=sys.stderr)
            return 1
        if metric == "functions" and want_functions and missed > 0:
            print(
                f"ERROR: missed coverable functions on {args.platform}: {missed}",
                file=sys.stderr,
            )
            return 1
        if metric == "regions" and want_regions and missed > 0:
            print(
                f"ERROR: missed coverable regions on {args.platform}: {missed}",
                file=sys.stderr,
            )
            return 1
        if metric == "branches" and want_branches and missed > 0:
            print(
                f"ERROR: missed coverable branches on {args.platform}: {missed}",
                file=sys.stderr,
            )
            return 1

        if missed == 0 and actual_percent != expected_theoretical_percent:
            print(
                f"ERROR: percent mismatch despite full coverage for {metric} on {args.platform}: "
                f"expected {expected_theoretical_percent}%, got {actual_percent}%",
                file=sys.stderr,
            )
            return 1

    write_summary(summary_lines)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

