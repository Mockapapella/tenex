#!/usr/bin/env python3
from __future__ import annotations

import argparse
import gzip
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

DEFAULT_DECIMAL_PLACES = 26
METRICS = ("regions", "functions", "lines", "branches")


@dataclass
class MetricCounts:
    instrumented: int
    covered: int


@dataclass
class PlatformCoverage:
    by_file: dict[str, dict[str, MetricCounts]]
    unknown_sources: set[str]


def repo_root() -> Path:
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode == 0:
        root = result.stdout.strip()
        if root:
            return Path(root)
    return Path.cwd()


def git_head_sha(root: Path) -> Optional[str]:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        return None
    sha = result.stdout.strip()
    return sha or None


def normalize_source_path(raw: str, root: Path) -> Optional[str]:
    value = raw.strip().replace("\\", "/")
    if not value:
        return None

    parts = [part for part in value.split("/") if part]
    best: Optional[str] = None
    for idx in range(len(parts)):
        candidate = "/".join(parts[idx:])
        if (root / candidate).is_file():
            best = candidate
    return best


def load_json(path: Path) -> Any:
    if path.suffix == ".gz":
        with gzip.open(path, "rt", encoding="utf-8", errors="replace") as handle:
            return json.load(handle)
    return json.loads(path.read_text(encoding="utf-8", errors="replace"))


def parse_llvm_cov_summary_json(path: Path, root: Path) -> PlatformCoverage:
    payload = load_json(path)
    data = payload.get("data")
    if not isinstance(data, list) or not data:
        raise ValueError("llvm-cov json export missing data[]")
    entry = data[0]
    files = entry.get("files")
    if not isinstance(files, list):
        raise ValueError("llvm-cov json export missing data[0].files[]")

    by_file: dict[str, dict[str, MetricCounts]] = {}
    unknown_sources: set[str] = set()

    for file_entry in files:
        if not isinstance(file_entry, dict):
            continue
        filename_raw = str(file_entry.get("filename", "")).strip()
        source = normalize_source_path(filename_raw, root)
        if source is None:
            if filename_raw:
                unknown_sources.add(filename_raw)
            continue

        summary = file_entry.get("summary")
        if not isinstance(summary, dict):
            continue

        metrics: dict[str, MetricCounts] = {}
        for metric in METRICS:
            item = summary.get(metric)
            if not isinstance(item, dict):
                instrumented = 0
                covered = 0
            else:
                instrumented = int(item.get("count", 0) or 0)
                covered = int(item.get("covered", 0) or 0)
            metrics[metric] = MetricCounts(instrumented=instrumented, covered=covered)

        by_file[source] = metrics

    return PlatformCoverage(by_file=by_file, unknown_sources=unknown_sources)


def percent_bps(numerator: int, denominator: int, *, empty_is_100: bool = False) -> int:
    if denominator <= 0:
        if empty_is_100 and numerator == 0:
            return 10_000
        return 0
    return (numerator * 10_000 + denominator // 2) // denominator


def format_bps(bps: int) -> str:
    whole = bps // 100
    frac = bps % 100
    return f"{whole}.{frac:02d}%"


def format_ratio_percent_decimal(
    numerator: int,
    denominator: int,
    decimal_places: int = DEFAULT_DECIMAL_PLACES,
    *,
    empty_is_100: bool = False,
) -> str:
    if denominator <= 0:
        if empty_is_100 and numerator == 0:
            return f"100.{('0' * decimal_places)}"
        return f"0.{('0' * decimal_places)}"

    scaled = numerator * 100
    whole = scaled // denominator
    remainder = scaled % denominator

    digits: list[int] = []
    for _ in range(decimal_places):
        remainder *= 10
        digit = remainder // denominator
        remainder %= denominator
        digits.append(int(digit))

    remainder *= 10
    next_digit = remainder // denominator
    if next_digit >= 5:
        carry = 1
        for idx in range(len(digits) - 1, -1, -1):
            if carry == 0:
                break
            value = digits[idx] + carry
            if value >= 10:
                digits[idx] = value - 10
                carry = 1
            else:
                digits[idx] = value
                carry = 0
        if carry:
            whole += carry

    frac = "".join(str(d) for d in digits)
    return f"{whole}.{frac}"


def platform_totals(platform: PlatformCoverage) -> dict[str, MetricCounts]:
    totals = {metric: MetricCounts(instrumented=0, covered=0) for metric in METRICS}
    for file_metrics in platform.by_file.values():
        for metric in METRICS:
            counts = file_metrics.get(metric)
            if counts is None:
                continue
            totals[metric].instrumented += counts.instrumented
            totals[metric].covered += counts.covered
    return totals


def union_instrumented(platforms: dict[str, PlatformCoverage]) -> dict[str, int]:
    all_files: set[str] = set()
    for platform in platforms.values():
        all_files |= set(platform.by_file.keys())

    union: dict[str, int] = {metric: 0 for metric in METRICS}
    for path in sorted(all_files):
        for metric in METRICS:
            best = 0
            for platform in platforms.values():
                counts = platform.by_file.get(path, {}).get(metric)
                if counts is not None and counts.instrumented > best:
                    best = counts.instrumented
            union[metric] += best
    return union


def compute_summary(
    platforms: dict[str, PlatformCoverage], sha: Optional[str]
) -> tuple[dict, str]:
    union_counts = union_instrumented(platforms)

    payload: dict[str, object] = {
        "commit": {"sha": sha} if sha else None,
        "decimal_places": DEFAULT_DECIMAL_PLACES,
        "union": {metric: {"instrumented": union_counts[metric]} for metric in METRICS},
        "platforms": {},
    }

    def metric_row(
        *,
        instrumented: int,
        covered: int,
        union_count: int,
        decimal_places: int,
    ) -> dict[str, object]:
        missed = instrumented - covered

        theoretical_bps = percent_bps(instrumented, union_count, empty_is_100=True)
        actual_bps = percent_bps(covered, union_count, empty_is_100=True)
        coverable_bps = percent_bps(covered, instrumented, empty_is_100=True)

        return {
            "instrumented": instrumented,
            "covered": covered,
            "missed_coverable": missed,
            "theoretical_max_bps": theoretical_bps,
            "actual_bps": actual_bps,
            "coverage_of_coverable_bps": coverable_bps,
            "theoretical_max_percent": format_ratio_percent_decimal(
                instrumented,
                union_count,
                decimal_places,
                empty_is_100=True,
            ),
            "actual_percent": format_ratio_percent_decimal(
                covered,
                union_count,
                decimal_places,
                empty_is_100=True,
            ),
            "coverage_of_coverable_percent": format_ratio_percent_decimal(
                covered,
                instrumented,
                decimal_places,
                empty_is_100=True,
            ),
        }

    rows: list[tuple[str, dict[str, object]]] = []
    for os_name in ("linux", "macos", "windows"):
        coverage = platforms[os_name]
        totals = platform_totals(coverage)
        row: dict[str, object] = {
            "unknown_sources": len(coverage.unknown_sources),
        }
        for metric in METRICS:
            counts = totals[metric]
            row[metric] = metric_row(
                instrumented=counts.instrumented,
                covered=counts.covered,
                union_count=union_counts[metric],
                decimal_places=DEFAULT_DECIMAL_PLACES,
            )
        rows.append((os_name, row))

    payload["platforms"] = {name: row for name, row in rows}

    markdown_lines = [
        "## Coverage theoretical max",
        "",
        "- Union instrumented:",
        f"  - regions: `{union_counts['regions']}`",
        f"  - functions: `{union_counts['functions']}`",
        f"  - lines: `{union_counts['lines']}`",
        f"  - branches: `{union_counts['branches']}`",
        "",
    ]

    for metric in ("lines", "functions", "regions", "branches"):
        markdown_lines.extend(
            [
                f"### {metric.capitalize()}",
                "",
                "| OS | Coverable | Covered | Missed | Theoretical max | Actual | Covered/coverable |",
                "|---|---:|---:|---:|---:|---:|---:|",
            ]
        )
        for name, row in rows:
            metrics = row[metric]
            markdown_lines.append(
                "| "
                + name
                + " | "
                + f"`{metrics['instrumented']}`"
                + " | "
                + f"`{metrics['covered']}`"
                + " | "
                + f"`{metrics['missed_coverable']}`"
                + " | "
                + format_bps(metrics["theoretical_max_bps"])
                + " | "
                + format_bps(metrics["actual_bps"])
                + " | "
                + format_bps(metrics["coverage_of_coverable_bps"])
                + " |"
            )
        markdown_lines.append("")

    markdown_lines.extend(
        [
            "### Theoretical max (26 dp)",
            "",
        ]
    )
    for name, row in rows:
        for metric in ("regions", "functions", "lines", "branches"):
            percent_value = row[metric]["theoretical_max_percent"]
            markdown_lines.append(f"- {metric} {name}: `{percent_value}%`")

    markdown = "\n".join(markdown_lines) + "\n"
    return payload, markdown


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--linux-json", type=Path, required=True)
    parser.add_argument("--macos-json", type=Path, required=True)
    parser.add_argument("--windows-json", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=Path("coverage-theoretical-max.json"))
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = repo_root()
    sha = git_head_sha(root)

    platforms = {
        "linux": parse_llvm_cov_summary_json(args.linux_json, root),
        "macos": parse_llvm_cov_summary_json(args.macos_json, root),
        "windows": parse_llvm_cov_summary_json(args.windows_json, root),
    }

    payload, markdown = compute_summary(platforms, sha)

    args.out.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with Path(summary_path).open("a", encoding="utf-8") as handle:
            handle.write(markdown)
    else:
        sys.stdout.write(markdown)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
