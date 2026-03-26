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
from typing import Optional

LineKey = tuple[str, int]
DEFAULT_DECIMAL_PLACES = 26


@dataclass
class LineCoverage:
    instrumented: set[LineKey]
    covered: set[LineKey]
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


def iter_text_lines(path: Path) -> list[str]:
    if path.suffix == ".gz":
        with gzip.open(path, "rt", encoding="utf-8", errors="replace") as handle:
            return [line.rstrip("\n") for line in handle]
    return path.read_text(encoding="utf-8", errors="replace").splitlines()


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


def parse_lcov(path: Path, root: Path) -> LineCoverage:
    instrumented: set[LineKey] = set()
    covered: set[LineKey] = set()
    unknown_sources: set[str] = set()

    current_source: Optional[str] = None
    for raw in iter_text_lines(path):
        if raw.startswith("SF:"):
            source_raw = raw[3:].strip()
            current_source = normalize_source_path(source_raw, root)
            if current_source is None:
                unknown_sources.add(source_raw)
        elif raw.startswith("DA:") and current_source is not None:
            fields = raw[3:].strip().split(",")
            if len(fields) < 2:
                raise ValueError(f"Malformed DA line: {raw}")

            try:
                line_no = int(fields[0])
                hits = int(fields[1])
            except ValueError as exc:
                raise ValueError(f"Malformed DA values: {raw}") from exc

            key = (current_source, line_no)
            instrumented.add(key)
            if hits > 0:
                covered.add(key)
        elif raw == "end_of_record":
            current_source = None

    return LineCoverage(
        instrumented=instrumented,
        covered=covered,
        unknown_sources=unknown_sources,
    )


def percent_bps(numerator: int, denominator: int) -> int:
    if denominator <= 0:
        return 0
    return (numerator * 10_000 + denominator // 2) // denominator


def format_bps(bps: int) -> str:
    whole = bps // 100
    frac = bps % 100
    return f"{whole}.{frac:02d}%"


def format_ratio_percent_decimal(
    numerator: int, denominator: int, decimal_places: int = DEFAULT_DECIMAL_PLACES
) -> str:
    if denominator <= 0:
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


def compute_summary(
    platforms: dict[str, LineCoverage], sha: Optional[str]
) -> tuple[dict, str]:
    union_instrumented: set[LineKey] = set()
    for coverage in platforms.values():
        union_instrumented |= coverage.instrumented

    union_lines = len(union_instrumented)

    payload: dict[str, object] = {
        "commit": {"sha": sha} if sha else None,
        "decimal_places": DEFAULT_DECIMAL_PLACES,
        "union": {"instrumented_lines": union_lines},
        "platforms": {},
    }

    rows: list[tuple[str, dict[str, int]]] = []
    for os_name in ("linux", "macos", "windows"):
        coverage = platforms[os_name]
        instrumented = len(coverage.instrumented)
        covered = len(coverage.covered)
        missed = instrumented - covered

        theoretical_bps = percent_bps(instrumented, union_lines)
        actual_bps = percent_bps(covered, union_lines)
        coverable_bps = percent_bps(covered, instrumented)

        row = {
            "instrumented_lines": instrumented,
            "covered_lines": covered,
            "missed_coverable_lines": missed,
            "theoretical_max_lines_bps": theoretical_bps,
            "actual_lines_bps": actual_bps,
            "coverage_of_coverable_lines_bps": coverable_bps,
            "theoretical_max_lines_percent": format_ratio_percent_decimal(
                instrumented, union_lines
            ),
            "actual_lines_percent": format_ratio_percent_decimal(covered, union_lines),
            "coverage_of_coverable_lines_percent": format_ratio_percent_decimal(
                covered, instrumented
            ),
            "unknown_sources": len(coverage.unknown_sources),
        }
        rows.append((os_name, row))

    payload["platforms"] = {name: row for name, row in rows}

    markdown_lines = [
        "## Coverage theoretical max (lines)",
        "",
        f"- Union instrumented lines: `{union_lines}`",
        "",
        "| OS | Coverable | Covered | Missed | Theoretical max | Actual | Covered/coverable |",
        "|---|---:|---:|---:|---:|---:|---:|",
    ]
    for name, row in rows:
        markdown_lines.append(
            "| "
            + name
            + " | "
            + f"`{row['instrumented_lines']}`"
            + " | "
            + f"`{row['covered_lines']}`"
            + " | "
            + f"`{row['missed_coverable_lines']}`"
            + " | "
            + format_bps(row["theoretical_max_lines_bps"])
            + " | "
            + format_bps(row["actual_lines_bps"])
            + " | "
            + format_bps(row["coverage_of_coverable_lines_bps"])
            + " |"
        )

    markdown_lines.extend(
        [
            "",
            "### Theoretical max (26 dp)",
            "",
        ]
    )
    for name, row in rows:
        markdown_lines.append(
            f"- {name}: `{row['theoretical_max_lines_percent']}%`"
        )

    markdown = "\n".join(markdown_lines) + "\n"
    return payload, markdown


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--linux", type=Path, required=True)
    parser.add_argument("--macos", type=Path, required=True)
    parser.add_argument("--windows", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=Path("coverage-theoretical-max.json"))
    parser.add_argument(
        "--fail-on-missed-coverable-lines",
        action="store_true",
        help="Exit non-zero when any platform misses a coverable line.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = repo_root()
    sha = git_head_sha(root)

    platforms = {
        "linux": parse_lcov(args.linux, root),
        "macos": parse_lcov(args.macos, root),
        "windows": parse_lcov(args.windows, root),
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

    if args.fail_on_missed_coverable_lines:
        missed = [
            name
            for name, metrics in payload["platforms"].items()
            if metrics["missed_coverable_lines"] > 0
        ]
        if missed:
            print(
                f"ERROR: missed coverable lines on: {', '.join(missed)}",
                file=sys.stderr,
            )
            return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
