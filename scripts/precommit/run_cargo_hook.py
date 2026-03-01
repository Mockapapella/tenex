#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def repo_root() -> Path:
    output = subprocess.check_output(
        ["git", "rev-parse", "--show-toplevel"],
        text=True,
    )
    return Path(output.strip())


def muxd_pids_from_proc(proc_dir: Path, target_prefix: str) -> set[int]:
    pids: set[int] = set()
    for entry in proc_dir.iterdir():
        if not entry.name.isdigit():
            continue

        cmdline_path = entry / "cmdline"
        try:
            raw = cmdline_path.read_bytes()
        except OSError:
            continue
        if not raw:
            continue

        parts = [part.decode("utf-8", "replace") for part in raw.split(b"\0") if part]
        if not parts:
            continue

        executable = parts[0].replace("\\", "/")
        if not executable.startswith(target_prefix):
            continue
        if "muxd" not in parts:
            continue

        pids.add(int(entry.name))
    return pids


def muxd_pids_from_ps(target_prefix: str) -> set[int]:
    result = subprocess.run(
        ["ps", "-axo", "pid=,command="],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        return set()

    pids: set[int] = set()
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line:
            continue

        fields = line.split(None, 1)
        if len(fields) != 2:
            continue

        pid_str, command = fields
        if not pid_str.isdigit():
            continue

        parts = command.split()
        if not parts:
            continue

        executable = parts[0].replace("\\", "/")
        if not executable.startswith(target_prefix):
            continue
        if "muxd" not in parts:
            continue

        pids.add(int(pid_str))
    return pids


def cleanup_repo_muxd(root: Path) -> None:
    target_prefix = (root / "target").as_posix().rstrip("/") + "/"
    proc_dir = Path("/proc")
    pids = (
        muxd_pids_from_proc(proc_dir, target_prefix)
        if proc_dir.is_dir()
        else muxd_pids_from_ps(target_prefix)
    )

    if not pids:
        return

    for pid in pids:
        try:
            os.kill(pid, 15)
        except OSError:
            continue

    deadline = time.monotonic() + 1.0
    while time.monotonic() < deadline:
        remaining: list[int] = []
        for pid in pids:
            try:
                os.kill(pid, 0)
            except OSError:
                continue
            remaining.append(pid)
        if not remaining:
            return
        pids = remaining
        time.sleep(0.05)

    for pid in pids:
        try:
            os.kill(pid, 9)
        except OSError:
            continue


def verify_llvm_cov_version(root: Path) -> bool:
    required_version = (root / ".cargo-llvm-cov-version").read_text(encoding="utf-8").strip()
    result = subprocess.run(
        ["cargo", "llvm-cov", "--version"],
        capture_output=True,
        text=True,
        check=False,
    )
    installed_version = ""
    if result.returncode == 0:
        fields = result.stdout.strip().split()
        if len(fields) >= 2:
            installed_version = fields[1]
    if installed_version == required_version:
        return True

    message = installed_version if installed_version else "<missing>"
    print(
        f"ERROR: Expected cargo-llvm-cov {required_version}, found {message}",
        file=sys.stderr,
    )
    print(
        f"Install: cargo install cargo-llvm-cov --version {required_version} --locked --force",
        file=sys.stderr,
    )
    return False


def build_command(mode: str, *, skip_fail_under: bool = False) -> list[str]:
    if mode == "test":
        return [
            "cargo",
            "test",
            "--jobs",
            "1",
            "--all-targets",
            "--all-features",
            "--",
            "--test-threads=1",
        ]

    command = [
        "cargo",
        "llvm-cov",
        "--jobs",
        "1",
        "--all-targets",
        "--all-features",
        "--profile",
        "coverage",
    ]
    if not skip_fail_under:
        command.extend(
            [
                "--fail-under-lines",
                "90",
                "--fail-under-functions",
                "90",
            ]
        )
    command.extend(
        [
            "--ignore-filename-regex",
            "crates/vt100-ctt/",
            "--",
            "--test-threads=1",
        ]
    )
    return command


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("test", "coverage"))
    parser.add_argument(
        "--no-fail-under",
        action="store_true",
        help="Only for coverage mode: run coverage/tests without fail-under thresholds.",
    )
    args = parser.parse_args()

    root = repo_root()

    if args.mode != "coverage" and args.no_fail_under:
        parser.error("--no-fail-under can only be used with coverage mode")

    if args.mode == "coverage" and not verify_llvm_cov_version(root):
        return 1

    if args.mode == "coverage":
        shutil.rmtree(root / "target" / "llvm-cov-target", ignore_errors=True)

    state_dir = Path(tempfile.mkdtemp(prefix="tenex-pre-commit-"))
    env = os.environ.copy()
    env["TENEX_STATE_PATH"] = str(state_dir / "state.json")
    env["TENEX_MUX_SOCKET"] = str(state_dir / "mux.sock")

    cleanup_repo_muxd(root)
    try:
        command = build_command(args.mode, skip_fail_under=args.no_fail_under)
        result = subprocess.run(
            command,
            cwd=root,
            env=env,
            check=False,
        )
        return result.returncode
    finally:
        cleanup_repo_muxd(root)
        shutil.rmtree(state_dir, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
