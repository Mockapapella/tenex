#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path


WINDOWS_RECEIPT_HEADER = "\t".join(
    [
        "Filename",
        "Regions",
        "Missed Regions",
        "Region Cover",
        "Functions",
        "Missed Functions",
        "Function Cover",
        "Branches",
        "Missed Branches",
        "Branch Cover",
    ]
)


def canonicalize_windows_report(text: str) -> str:
    lines: list[str] = [WINDOWS_RECEIPT_HEADER]
    for raw_line in text.splitlines():
        stripped = raw_line.strip()
        if not stripped or stripped.startswith("Filename") or set(stripped) == {"-"}:
            continue

        fields = stripped.split()
        if len(fields) != 13:
            raise ValueError(f"unexpected llvm-cov report row: {raw_line}")

        lines.append(
            "\t".join(
                [
                    fields[0],
                    fields[1],
                    fields[2],
                    fields[3],
                    fields[4],
                    fields[5],
                    fields[6],
                    fields[10],
                    fields[11],
                    fields[12],
                ]
            )
        )
    return "\n".join(lines) + "\n"


def canonicalize_report(receipt_os: str, text: str) -> str:
    if receipt_os == "windows":
        return canonicalize_windows_report(text)
    if text.endswith("\n"):
        return text
    return text + "\n"


def write_receipt(receipt_os: str, input_path: Path, report_path: Path, sha_path: Path) -> None:
    receipt_text = canonicalize_report(receipt_os, input_path.read_text(encoding="utf-8"))
    report_path.write_text(receipt_text, encoding="utf-8")

    digest = hashlib.sha256(report_path.read_bytes()).hexdigest()
    sha_path.write_text(f"{digest}  {report_path.as_posix()}\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--os", choices=("linux", "macos", "windows"), required=True)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--report-path", type=Path, required=True)
    parser.add_argument("--sha-path", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    write_receipt(args.os, args.input, args.report_path, args.sha_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
