#!/usr/bin/env python3
"""
Run `cargo ...` with an isolated TENEX_STATE_PATH to match CI.

This prevents tests/coverage from reading or writing a developer's real Tenex state file.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: cargo_with_isolated_state.py <cargo-args...>", file=sys.stderr)
        print("example: cargo_with_isolated_state.py test -- --test-threads=1", file=sys.stderr)
        return 2

    with tempfile.TemporaryDirectory(prefix="tenex-pre-commit-") as tmpdir:
        state_path = os.path.join(tmpdir, "state.json")
        env = os.environ.copy()
        env["TENEX_STATE_PATH"] = state_path

        return subprocess.run(["cargo", *sys.argv[1:]], env=env, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
