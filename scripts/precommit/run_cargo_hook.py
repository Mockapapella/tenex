#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
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


def clear_local_git_environment(env: dict[str, str], *, root: Path) -> None:
    result = subprocess.run(
        ["git", "rev-parse", "--local-env-vars"],
        cwd=root,
        env=env,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise RuntimeError(detail or "git rev-parse --local-env-vars failed")

    for name in result.stdout.splitlines():
        if name:
            env.pop(name, None)


def sanitized_subprocess_environment(
    *, root: Path, source_env: dict[str, str] | None = None
) -> dict[str, str]:
    env = os.environ.copy() if source_env is None else source_env.copy()
    clear_local_git_environment(env, root=root)
    return env


def muxd_pids_from_proc(proc_dir: Path, socket: str, state_path: Path) -> set[int]:
    wanted_socket = f"TENEX_MUX_SOCKET={socket}"
    wanted_state_path = f"TENEX_STATE_PATH={state_path}"
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

        if "muxd" not in parts:
            continue

        environ_path = entry / "environ"
        try:
            environ = environ_path.read_bytes()
        except OSError:
            continue
        if not environ:
            continue

        env_parts = [part.decode("utf-8", "replace") for part in environ.split(b"\0") if part]
        if wanted_socket not in env_parts and wanted_state_path not in env_parts:
            continue

        pids.add(int(entry.name))
    return pids


def fnv1a64(value: str) -> int:
    hash_value = 0xCBF29CE484222325
    for byte in value.encode("utf-8"):
        hash_value ^= byte
        hash_value = (hash_value * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return hash_value


def muxd_pidfile_path(state_dir: Path, socket: str) -> Path:
    return state_dir / f"tenex-muxd-{fnv1a64(socket):016x}.pid"


def muxd_pids_from_pidfile(state_dir: Path, socket: str) -> set[int]:
    pidfile_path = muxd_pidfile_path(state_dir, socket)
    try:
        payload = json.loads(pidfile_path.read_text(encoding="utf-8"))
    except (OSError, ValueError, json.JSONDecodeError):
        return set()

    if str(payload.get("socket", "")).strip() != socket:
        return set()

    pid = payload.get("pid")
    if not isinstance(pid, int) or pid <= 0:
        return set()

    return {pid}


def muxd_pids_from_pidfiles(state_dir: Path) -> set[int]:
    pids: set[int] = set()
    for entry in state_dir.iterdir():
        name = entry.name
        if not name.startswith("tenex-muxd-") or not name.endswith(".pid"):
            continue
        if not entry.is_file():
            continue
        try:
            payload = json.loads(entry.read_text(encoding="utf-8"))
        except (OSError, ValueError, json.JSONDecodeError):
            continue
        pid = payload.get("pid")
        if isinstance(pid, int) and pid > 0:
            pids.add(pid)
    return pids


def cleanup_hook_muxd(state_dir: Path, socket: str) -> None:
    proc_dir = Path("/proc")
    state_path = state_dir / "state.json"
    pids = (
        muxd_pids_from_proc(proc_dir, socket, state_path)
        if proc_dir.is_dir()
        else muxd_pids_from_pidfiles(state_dir)
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


def verify_llvm_cov_version(root: Path, *, env: dict[str, str]) -> bool:
    required_version = (root / ".cargo-llvm-cov-version").read_text(encoding="utf-8").strip()
    result = subprocess.run(
        ["cargo", "llvm-cov", "--version"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
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


def build_command(mode: str, *, root: Path) -> list[str]:
    build_jobs = os.environ.get("TENEX_CARGO_BUILD_JOBS")
    job_args: list[str] = []
    if build_jobs:
        build_jobs = build_jobs.strip()
        if build_jobs.isdigit():
            job_args = ["--jobs", build_jobs]

    if mode == "fmt":
        return ["cargo", "fmt", "--all", "--", "--check"]

    if mode == "clippy":
        return [
            "cargo",
            "clippy",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ]

    if mode == "test":
        return [
            "cargo",
            "test",
            *job_args,
            "--all-targets",
            "--all-features",
        ]

    if mode == "report":
        return [
            "cargo",
            "llvm-cov",
            "report",
            "--profile",
            "coverage",
            "--ignore-filename-regex",
            "crates/vt100-ctt/",
        ]

    command = [
        "cargo",
        "llvm-cov",
        "--branch",
        *job_args,
        "--all-targets",
        "--all-features",
        "--profile",
        "coverage",
    ]
    command.extend(
        [
            "--ignore-filename-regex",
            "crates/vt100-ctt/",
        ]
    )
    return command


def configure_rustc_ice(env: dict[str, str]) -> None:
    rustc_ice = env.get("RUSTC_ICE", "").strip()
    if not rustc_ice:
        cache_home = env.get("XDG_CACHE_HOME", "").strip()
        home = env.get("HOME", "").strip()
        if cache_home:
            rustc_ice_path = Path(cache_home) / "tenex" / "rustc-ice"
        elif home:
            rustc_ice_path = Path(home) / ".cache" / "tenex" / "rustc-ice"
        else:
            return
        rustc_ice_path.mkdir(parents=True, exist_ok=True)
        env["RUSTC_ICE"] = str(rustc_ice_path)
        return

    if rustc_ice != "0":
        Path(rustc_ice).mkdir(parents=True, exist_ok=True)


def hook_tmp_root(env: dict[str, str]) -> str | None:
    override = env.get("TENEX_HOOK_TMPDIR", "").strip()
    if override and os.path.isdir(override) and os.access(override, os.W_OK):
        return override

    # Prefer /tmp on macOS where the default system temp dir can be long enough
    # to exceed Unix socket path limits (SUN_LEN) once we append mux.sock.
    for candidate in ("/tmp",):
        if os.path.isdir(candidate) and os.access(candidate, os.W_OK):
            return candidate

    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("fmt", "clippy", "test", "coverage", "report"))
    args = parser.parse_args()

    root = repo_root()
    env = sanitized_subprocess_environment(root=root)

    if args.mode in ("coverage", "report") and not verify_llvm_cov_version(root, env=env):
        return 1

    configure_rustc_ice(env)
    command = build_command(args.mode, root=root)
    if args.mode not in ("test", "coverage"):
        result = subprocess.run(command, cwd=root, env=env, check=False)
        return result.returncode

    if args.mode == "coverage":
        shutil.rmtree(root / "target" / "llvm-cov-target", ignore_errors=True)

    tmp_root = hook_tmp_root(env)
    if tmp_root:
        env["TMPDIR"] = tmp_root

    state_dir = Path(
        tempfile.mkdtemp(prefix="tenex-pre-commit-", dir=tmp_root) if tmp_root else tempfile.mkdtemp(prefix="tenex-pre-commit-")
    )
    mux_socket = str(state_dir / "mux.sock")
    env["TENEX_STATE_PATH"] = str(state_dir / "state.json")
    env["TENEX_MUX_SOCKET"] = mux_socket

    cleanup_hook_muxd(state_dir, mux_socket)
    try:
        result = subprocess.run(
            command,
            cwd=root,
            env=env,
            check=False,
        )
        return result.returncode
    finally:
        cleanup_hook_muxd(state_dir, mux_socket)
        shutil.rmtree(state_dir, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
