#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime as dt
import gzip
import importlib.util
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from types import ModuleType


DEFAULT_BASE_REF = "origin/master"
DEFAULT_OUT_ROOT = Path("target/remote-precommit")
DEFAULT_LOCK_NAME = "tenex-remote-validation"

DEFAULT_MAC_HOST = ""
DEFAULT_MAC_SOURCE_REPO = "~/src/tenex"
DEFAULT_MAC_WORKTREE_ROOT = "~/src/tenex-worktrees"
DEFAULT_MAC_RUN_ROOT = "~/.cache/tenex-remote-precommit"

DEFAULT_WINDOWS_HOST = ""
DEFAULT_WINDOWS_SOURCE_REPO = "~/src/tenex-source"
DEFAULT_WINDOWS_WORKTREE_ROOT = "~/src/tenex-worktrees"
DEFAULT_WINDOWS_RUN_ROOT = "~/.cache/tenex-remote-precommit"


REMOTE_SCRIPT = r"""#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"

coord_helper="$HOME/.local/bin/tenex-remote-coordination"
coord_state="${XDG_STATE_HOME:-$HOME/.local/state}/tenex/remote-coordination"
coord_lock_name="${TENEX_REMOTE_VALIDATION_LOCK_NAME:-tenex-remote-validation}"
coord_kind="remote-pre-push"
test -x "$coord_helper"

coord_hb_pid=
cleanup_worktree() {
  if [ -z "${source_repo:-}" ] || [ -z "${worktree_dir:-}" ]; then
    return 0
  fi

  cd / 2>/dev/null || true
  git -C "$source_repo" worktree remove --force "$worktree_dir" 2>/dev/null || true
  git -C "$source_repo" worktree prune || true
  rm -rf "$worktree_dir" || true
}
coord_release() {
  if [ -n "${coord_hb_pid:-}" ]; then
    kill "$coord_hb_pid" 2>/dev/null || true
    wait "$coord_hb_pid" 2>/dev/null || true
  fi
  cleanup_worktree || true
  "$coord_helper" lock release --state-dir "$coord_state" --name "$coord_lock_name" || true
}

"$coord_helper" lock acquire \
  --state-dir "$coord_state" \
  --name "$coord_lock_name" \
  --kind "$coord_kind" \
  --stale-seconds 60 \
  --check-live-leases
(
  while kill -0 "$$" 2>/dev/null; do
    "$coord_helper" lock heartbeat --state-dir "$coord_state" --name "$coord_lock_name" || true
    sleep 5
  done
) &
coord_hb_pid=$!
trap coord_release EXIT INT TERM HUP

base_sha="${BASE_SHA:?}"
run_id="${RUN_ID:?}"
source_repo="${SOURCE_REPO:?}"
worktree_root="${WORKTREE_ROOT:?}"
run_root="${RUN_ROOT:?}"

source_repo="${source_repo/#~/$HOME}"
worktree_root="${worktree_root/#~/$HOME}"
run_root="${run_root/#~/$HOME}"

run_dir="$run_root/$run_id"
cov_out="$run_dir/cov"
patch_path="$run_dir/wip.patch"
worktree_dir="$worktree_root/$run_id"

mkdir -p "$cov_out" "$worktree_root" "$run_dir"

if [ ! -f "$source_repo/.cargo-llvm-cov-version" ]; then
  echo "ERROR: SOURCE_REPO is not a tenex checkout: $source_repo" >&2
  exit 1
fi

required_cov="$(tr -d '\r\n' < "$source_repo/.cargo-llvm-cov-version")"
installed_cov="$(cargo llvm-cov --version 2>/dev/null | awk '{print $2}' || true)"
if [ "$installed_cov" != "$required_cov" ]; then
  echo "ERROR: Expected cargo-llvm-cov $required_cov, found ${installed_cov:-<missing>}" >&2
  exit 1
fi

git -C "$source_repo" fetch --prune origin "+refs/heads/*:refs/remotes/origin/*"
git -C "$source_repo" worktree remove --force "$worktree_dir" 2>/dev/null || true
rm -rf "$worktree_dir"
git -C "$source_repo" worktree prune
git -C "$source_repo" worktree add --force --detach "$worktree_dir" "$base_sha"

cd "$worktree_dir"
git reset --hard
git clean -fd
git apply --binary "$patch_path"

cargo llvm-cov \
  --all-targets \
  --all-features \
  --profile coverage \
  --ignore-filename-regex 'crates/vt100-ctt/' \
  --no-report \
  -- --list
cargo llvm-cov report \
  --profile coverage \
  --ignore-filename-regex 'crates/vt100-ctt/' \
  --summary-only \
  --json \
  --output-path "$cov_out/llvm-cov-instrumented.summary.json"
gzip -9 -f "$cov_out/llvm-cov-instrumented.summary.json"

python3 scripts/precommit/run_cargo_hook.py coverage
if [ ! -d target/llvm-cov-target/coverage ]; then
  echo "ERROR: No coverage artifacts found at target/llvm-cov-target/coverage" >&2
  cleanup_worktree
  exit 1
fi

cargo llvm-cov report \
  --profile coverage \
  --ignore-filename-regex 'crates/vt100-ctt/' \
  --summary-only \
  --json \
  --output-path "$cov_out/llvm-cov-report.summary.json"
gzip -9 -f "$cov_out/llvm-cov-report.summary.json"

cleanup_worktree
"""


def repo_root() -> Path:
    output = subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True)
    return Path(output.strip())


def load_run_cargo_hook(root: Path) -> ModuleType:
    path = root / "scripts" / "precommit" / "run_cargo_hook.py"
    spec = importlib.util.spec_from_file_location("run_cargo_hook", path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def gzip_file(src: Path, dst: Path) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    with src.open("rb") as input_handle:
        with gzip.open(dst, "wb", compresslevel=9) as output_handle:
            shutil.copyfileobj(input_handle, output_handle)


def git(args: list[str], *, root: Path, capture: bool = True) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=root,
        check=False,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        message = (result.stderr or "").strip()
        raise RuntimeError(message or f"git {' '.join(args)} failed")
    return (result.stdout or "").strip() if capture else ""


def slugify(value: str) -> str:
    value = value.strip()
    value = re.sub(r"[\s/]+", "-", value)
    return re.sub(r"[^A-Za-z0-9_.-]+", "-", value)


def expand_remote_home(path: str) -> str:
    if path == "~":
        return "$HOME"
    if path.startswith("~/"):
        return "$HOME/" + path[2:]
    return path


def create_patch(*, root: Path, base_sha: str, out_dir: Path) -> Path:
    patch_path = out_dir / "wip.patch"
    with patch_path.open("wb") as handle:
        result = subprocess.run(
            ["git", "diff", "--binary", "--cached", base_sha],
            cwd=root,
            stdout=handle,
            stderr=subprocess.PIPE,
        )
    if result.returncode != 0:
        raise RuntimeError((result.stderr or b"").decode("utf-8", "replace").strip())
    return patch_path


def export_instrumented_summary(
    *, root: Path, out_dir: Path, run_cargo_hook: ModuleType
) -> Path:
    out_json = out_dir / "llvm-cov-instrumented.summary.json"
    out_gz = out_dir / "llvm-cov-instrumented.summary.json.gz"
    out_dir.mkdir(parents=True, exist_ok=True)

    if not run_cargo_hook.verify_llvm_cov_version(root):
        raise RuntimeError("cargo-llvm-cov version mismatch")

    state_dir = Path(tempfile.mkdtemp(prefix="tenex-pre-push-"))
    mux_socket = str(state_dir / "mux.sock")
    env = os.environ.copy()
    env["TENEX_STATE_PATH"] = str(state_dir / "state.json")
    env["TENEX_MUX_SOCKET"] = mux_socket

    run_cargo_hook.cleanup_hook_muxd(state_dir, mux_socket)
    try:
        cmd = [
            "cargo",
            "llvm-cov",
            "--all-targets",
            "--all-features",
            "--profile",
            "coverage",
            "--ignore-filename-regex",
            "crates/vt100-ctt/",
            "--no-report",
            "--",
            "--list",
        ]
        result = subprocess.run(cmd, cwd=root, env=env, check=False)
        if result.returncode != 0:
            raise RuntimeError("cargo llvm-cov list-only failed")

        result = subprocess.run(
            [
                "cargo",
                "llvm-cov",
                "report",
                "--profile",
                "coverage",
                "--ignore-filename-regex",
                "crates/vt100-ctt/",
                "--summary-only",
                "--json",
                "--output-path",
                str(out_json),
            ],
            cwd=root,
            env=env,
            check=False,
        )
        if result.returncode != 0:
            raise RuntimeError("cargo llvm-cov summary export failed")

        gzip_file(out_json, out_gz)
        out_json.unlink(missing_ok=True)
        return out_gz
    finally:
        run_cargo_hook.cleanup_hook_muxd(state_dir, mux_socket)
        shutil.rmtree(state_dir, ignore_errors=True)


def export_coverage_summary(*, root: Path, out_dir: Path) -> Path:
    out_json = out_dir / "llvm-cov-report.summary.json"
    out_gz = out_dir / "llvm-cov-report.summary.json.gz"
    out_dir.mkdir(parents=True, exist_ok=True)

    result = subprocess.run(
        ["python3", "scripts/precommit/run_cargo_hook.py", "coverage"],
        cwd=root,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError("cargo llvm-cov coverage run failed")

    if not (root / "target" / "llvm-cov-target" / "coverage").is_dir():
        raise RuntimeError("missing target/llvm-cov-target/coverage artifacts")

    result = subprocess.run(
        [
            "cargo",
            "llvm-cov",
            "report",
            "--profile",
            "coverage",
            "--ignore-filename-regex",
            "crates/vt100-ctt/",
            "--summary-only",
            "--json",
            "--output-path",
            str(out_json),
        ],
        cwd=root,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError("cargo llvm-cov coverage summary export failed")

    gzip_file(out_json, out_gz)
    out_json.unlink(missing_ok=True)
    return out_gz


def run_remote_platform(
    *,
    host: str,
    platform: str,
    base_sha: str,
    run_id: str,
    patch: bytes,
    source_repo: str,
    worktree_root: str,
    run_root: str,
    lock_name: str,
    out_dir: Path,
    windows_wsl: bool,
) -> None:
    raw_run_root = run_root.rstrip("/")
    remote_run_dir = raw_run_root + f"/{run_id}"
    if not windows_wsl:
        remote_run_dir = expand_remote_home(remote_run_dir)

    if windows_wsl:
        upload_cmd = (
            'wsl.exe -e bash -lc "'
            f'mkdir -p {remote_run_dir} && cat > {remote_run_dir}/wip.patch"'
        )
        run_cmd = (
            'wsl.exe -e bash -lc "'
            f"BASE_SHA='{base_sha}' RUN_ID='{run_id}' "
            f"SOURCE_REPO='{source_repo}' WORKTREE_ROOT='{worktree_root}' RUN_ROOT='{run_root}' "
            f"TENEX_REMOTE_VALIDATION_LOCK_NAME='{lock_name}' bash -s\""
        )
        tar_cmd = f'wsl.exe -e bash -lc "tar -C {remote_run_dir}/cov -czf - ."'
    else:
        upload_cmd = f"mkdir -p {remote_run_dir} && cat > {remote_run_dir}/wip.patch"
        run_cmd = (
            f"BASE_SHA='{base_sha}' RUN_ID='{run_id}' "
            f"SOURCE_REPO='{source_repo}' WORKTREE_ROOT='{worktree_root}' RUN_ROOT='{run_root}' "
            f"TENEX_REMOTE_VALIDATION_LOCK_NAME='{lock_name}' bash -s"
        )
        tar_cmd = f"tar -C {remote_run_dir}/cov -czf - ."

    upload = subprocess.run(
        ["ssh", host, upload_cmd],
        input=patch,
        check=False,
    )
    if upload.returncode != 0:
        raise RuntimeError(f"{platform}: failed to upload patch to {host}")

    run_result = subprocess.run(
        ["ssh", host, run_cmd],
        input=REMOTE_SCRIPT.encode("utf-8"),
        check=False,
    )
    if run_result.returncode != 0:
        raise RuntimeError(f"{platform}: remote coverage run failed on {host}")

    out_dir.mkdir(parents=True, exist_ok=True)
    tgz_path = out_dir / f"{platform}.tgz"
    with tgz_path.open("wb") as handle:
        tar_result = subprocess.run(
            ["ssh", host, tar_cmd],
            stdout=handle,
            check=False,
        )
    if tar_result.returncode != 0:
        raise RuntimeError(f"{platform}: failed to download artifacts from {host}")

    extract_dir = out_dir / platform
    extract_dir.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["tar", "-xzf", str(tgz_path), "-C", str(extract_dir)],
        check=True,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--base-ref",
        default=os.environ.get("TENEX_THEORETICAL_MAX_BASE_REF", DEFAULT_BASE_REF),
        help="Git ref that exists on all machines (default: origin/master).",
    )
    parser.add_argument(
        "--out-root",
        type=Path,
        default=Path(os.environ.get("TENEX_REMOTE_PRECOMMIT_OUT_ROOT", DEFAULT_OUT_ROOT)),
        help="Where to write run artifacts (default: target/remote-precommit).",
    )
    parser.add_argument(
        "--lock-name",
        default=os.environ.get("TENEX_REMOTE_VALIDATION_LOCK_NAME", DEFAULT_LOCK_NAME),
        help="Remote validation lock name (default: tenex-remote-validation).",
    )
    parser.add_argument(
        "--mac-host",
        default=os.environ.get("TENEX_REMOTE_MAC_HOST", DEFAULT_MAC_HOST),
    )
    parser.add_argument(
        "--mac-source-repo",
        default=os.environ.get("TENEX_REMOTE_MAC_SOURCE_REPO", DEFAULT_MAC_SOURCE_REPO),
    )
    parser.add_argument(
        "--mac-worktree-root",
        default=os.environ.get(
            "TENEX_REMOTE_MAC_WORKTREE_ROOT", DEFAULT_MAC_WORKTREE_ROOT
        ),
    )
    parser.add_argument(
        "--mac-run-root",
        default=os.environ.get("TENEX_REMOTE_MAC_RUN_ROOT", DEFAULT_MAC_RUN_ROOT),
    )
    parser.add_argument(
        "--windows-host",
        default=os.environ.get("TENEX_REMOTE_WINDOWS_HOST", DEFAULT_WINDOWS_HOST),
    )
    parser.add_argument(
        "--windows-source-repo",
        default=os.environ.get(
            "TENEX_REMOTE_WINDOWS_SOURCE_REPO", DEFAULT_WINDOWS_SOURCE_REPO
        ),
    )
    parser.add_argument(
        "--windows-worktree-root",
        default=os.environ.get(
            "TENEX_REMOTE_WINDOWS_WORKTREE_ROOT", DEFAULT_WINDOWS_WORKTREE_ROOT
        ),
    )
    parser.add_argument(
        "--windows-run-root",
        default=os.environ.get("TENEX_REMOTE_WINDOWS_RUN_ROOT", DEFAULT_WINDOWS_RUN_ROOT),
    )
    return parser.parse_args()


def main() -> int:
    if os.environ.get("CI") == "true" or os.environ.get("GITHUB_ACTIONS") == "true":
        return 0

    args = parse_args()
    if not args.mac_host.strip() or not args.windows_host.strip():
        print(
            "ERROR: cross-OS theoretical-max gate requires SSH targets for macOS and Windows.",
            file=sys.stderr,
        )
        print(
            "Set TENEX_REMOTE_MAC_HOST and TENEX_REMOTE_WINDOWS_HOST (or pass --mac-host/--windows-host).",
            file=sys.stderr,
        )
        return 1
    root = repo_root()
    out_root = args.out_root
    if not out_root.is_absolute():
        out_root = root / out_root
    run_cargo_hook = load_run_cargo_hook(root)

    git(["fetch", "--prune", "origin", "+refs/heads/*:refs/remotes/origin/*"], root=root, capture=False)
    base_sha = git(["rev-parse", args.base_ref], root=root)

    branch = git(["rev-parse", "--abbrev-ref", "HEAD"], root=root)
    run_id = (
        f"pre-push-{slugify(branch)}-"
        f"{dt.datetime.now().strftime('%Y%m%d-%H%M%S')}-"
        f"{os.getpid()}"
    )

    out_dir = out_root / run_id
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "base-ref.txt").write_text(base_sha + "\n", encoding="utf-8")

    patch_path = create_patch(root=root, base_sha=base_sha, out_dir=out_dir)
    patch_bytes = patch_path.read_bytes()

    linux_dir = out_dir / "linux"
    export_instrumented_summary(root=root, out_dir=linux_dir, run_cargo_hook=run_cargo_hook)
    export_coverage_summary(root=root, out_dir=linux_dir)

    run_remote_platform(
        host=args.mac_host,
        platform="macos",
        base_sha=base_sha,
        run_id=run_id,
        patch=patch_bytes,
        source_repo=args.mac_source_repo,
        worktree_root=args.mac_worktree_root,
        run_root=args.mac_run_root,
        lock_name=args.lock_name,
        out_dir=out_dir,
        windows_wsl=False,
    )

    run_remote_platform(
        host=args.windows_host,
        platform="windows",
        base_sha=base_sha,
        run_id=run_id,
        patch=patch_bytes,
        source_repo=args.windows_source_repo,
        worktree_root=args.windows_worktree_root,
        run_root=args.windows_run_root,
        lock_name=args.lock_name,
        out_dir=out_dir,
        windows_wsl=True,
    )

    theoretical_max = out_dir / "coverage-theoretical-max.json"
    result = subprocess.run(
        [
            "python3",
            "scripts/ci/coverage_theoretical_max.py",
            "--linux-json",
            str((out_dir / "linux" / "llvm-cov-instrumented.summary.json.gz")),
            "--macos-json",
            str((out_dir / "macos" / "llvm-cov-instrumented.summary.json.gz")),
            "--windows-json",
            str((out_dir / "windows" / "llvm-cov-instrumented.summary.json.gz")),
            "--out",
            str(theoretical_max),
        ],
        cwd=root,
        check=False,
    )
    if result.returncode != 0:
        return result.returncode

    for platform in ("linux", "macos", "windows"):
        json_path = out_dir / platform / "llvm-cov-report.summary.json.gz"
        verify = subprocess.run(
            [
                "python3",
                "scripts/ci/coverage_verify_platform_theoretical_max.py",
                "--platform",
                platform,
                "--json",
                str(json_path),
                "--theoretical-max",
                str(theoretical_max),
            ],
            cwd=root,
            check=False,
        )
        if verify.returncode != 0:
            return verify.returncode

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
