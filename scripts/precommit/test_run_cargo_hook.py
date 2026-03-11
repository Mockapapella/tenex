from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


def load_module():
    path = Path(__file__).with_name("run_cargo_hook.py")
    spec = importlib.util.spec_from_file_location("run_cargo_hook", path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


run_cargo_hook = load_module()


class RunCargoHookTests(unittest.TestCase):
    def test_muxd_pids_from_proc_matches_exact_socket(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc_root = Path(tmp)

            matching = proc_root / "100"
            matching.mkdir()
            (matching / "cmdline").write_bytes(b"/repo/target/debug/tenex\0muxd\0")
            (matching / "environ").write_bytes(
                b"TENEX_MUX_SOCKET=/tmp/live.sock\0TENEX_STATE_PATH=/tmp/state.json\0"
            )

            wrong_socket = proc_root / "101"
            wrong_socket.mkdir()
            (wrong_socket / "cmdline").write_bytes(b"/repo/target/debug/tenex\0muxd\0")
            (wrong_socket / "environ").write_bytes(b"TENEX_MUX_SOCKET=/tmp/other.sock\0")

            not_muxd = proc_root / "102"
            not_muxd.mkdir()
            (not_muxd / "cmdline").write_bytes(b"/repo/target/debug/tenex\0test\0")
            (not_muxd / "environ").write_bytes(b"TENEX_MUX_SOCKET=/tmp/live.sock\0")

            pids = run_cargo_hook.muxd_pids_from_proc(proc_root, "/tmp/live.sock")
            self.assertEqual(pids, {100})

    def test_muxd_pids_from_pidfile_matches_exact_socket(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = Path(tmp)
            pidfile = run_cargo_hook.muxd_pidfile_path(state_dir, "/tmp/live.sock")
            pidfile.write_text(
                json.dumps({"pid": 321, "socket": "/tmp/live.sock"}),
                encoding="utf-8",
            )

            self.assertEqual(
                run_cargo_hook.muxd_pids_from_pidfile(state_dir, "/tmp/live.sock"),
                {321},
            )
            self.assertEqual(
                run_cargo_hook.muxd_pids_from_pidfile(state_dir, "/tmp/other.sock"),
                set(),
            )


if __name__ == "__main__":
    unittest.main()
