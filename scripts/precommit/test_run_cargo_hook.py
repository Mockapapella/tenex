from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock


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
    def test_sanitized_subprocess_environment_preserves_outer_repository(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            outer = Path(tmp) / "hook-repo"
            fixture = Path(tmp) / "fixture-repo"
            bootstrap_env = run_cargo_hook.sanitized_subprocess_environment(
                root=run_cargo_hook.repo_root()
            )

            def git(repo: Path, *args: str, env: dict[str, str]) -> str:
                result = run_cargo_hook.subprocess.run(
                    ["git", "-C", str(repo), *args],
                    check=True,
                    capture_output=True,
                    text=True,
                    env=env,
                )
                return result.stdout.strip()

            outer.mkdir()
            git(outer, "init", "-q", env=bootstrap_env)
            git(outer, "config", "user.name", "Tenex Test", env=bootstrap_env)
            git(
                outer,
                "config",
                "user.email",
                "tenex-test@example.invalid",
                env=bootstrap_env,
            )
            git(outer, "config", "commit.gpgsign", "false", env=bootstrap_env)
            (outer / "README.md").write_text("base\n", encoding="utf-8")
            git(outer, "add", "README.md", env=bootstrap_env)
            git(outer, "commit", "-q", "--no-verify", "-m", "base", env=bootstrap_env)
            (outer / "pending.txt").write_text("pending\n", encoding="utf-8")
            git(outer, "add", "pending.txt", env=bootstrap_env)

            head_before = git(outer, "rev-parse", "HEAD", env=bootstrap_env)
            tree_before = git(outer, "write-tree", env=bootstrap_env)
            gitdir_before = git(outer, "rev-parse", "--absolute-git-dir", env=bootstrap_env)
            index_path = outer / ".git" / "index"
            index_before = index_path.read_bytes()

            contaminated_env = bootstrap_env.copy()
            contaminated_env["GIT_DIR"] = str(outer / ".git")
            contaminated_env["GIT_WORK_TREE"] = str(outer)
            contaminated_env["GIT_INDEX_FILE"] = str(index_path)
            fixture_env = run_cargo_hook.sanitized_subprocess_environment(
                root=outer,
                source_env=contaminated_env,
            )

            fixture.mkdir()
            git(fixture, "init", "-q", env=fixture_env)
            git(fixture, "config", "user.name", "Tenex Test", env=fixture_env)
            git(
                fixture,
                "config",
                "user.email",
                "tenex-test@example.invalid",
                env=fixture_env,
            )
            git(fixture, "config", "commit.gpgsign", "false", env=fixture_env)
            (fixture / "README.md").write_text("fixture\n", encoding="utf-8")
            git(fixture, "add", "README.md", env=fixture_env)
            git(fixture, "commit", "-q", "--no-verify", "-m", "fixture", env=fixture_env)

            self.assertTrue(git(fixture, "rev-parse", "HEAD", env=fixture_env))
            self.assertEqual(git(outer, "rev-parse", "HEAD", env=bootstrap_env), head_before)
            self.assertEqual(git(outer, "write-tree", env=bootstrap_env), tree_before)
            self.assertEqual(
                git(outer, "rev-parse", "--absolute-git-dir", env=bootstrap_env),
                gitdir_before,
            )
            self.assertEqual(index_path.read_bytes(), index_before)

    def test_clear_local_git_environment_removes_only_git_local_variables(self) -> None:
        env = {
            "GIT_DIR": "/repo/.git",
            "GIT_INDEX_FILE": "/repo/.git/index",
            "GIT_PAGER": "cat",
            "PATH": "/usr/bin",
        }
        completed = run_cargo_hook.subprocess.CompletedProcess(
            args=["git", "rev-parse", "--local-env-vars"],
            returncode=0,
            stdout="GIT_DIR\nGIT_INDEX_FILE\n",
            stderr="",
        )

        with mock.patch.object(run_cargo_hook.subprocess, "run", return_value=completed):
            run_cargo_hook.clear_local_git_environment(env, root=Path("/repo"))

        self.assertEqual(env, {"GIT_PAGER": "cat", "PATH": "/usr/bin"})

    def test_clear_local_git_environment_fails_closed(self) -> None:
        env = {"GIT_DIR": "/repo/.git"}
        completed = run_cargo_hook.subprocess.CompletedProcess(
            args=["git", "rev-parse", "--local-env-vars"],
            returncode=1,
            stdout="",
            stderr="cannot inspect git environment",
        )

        with mock.patch.object(run_cargo_hook.subprocess, "run", return_value=completed):
            with self.assertRaisesRegex(RuntimeError, "cannot inspect git environment"):
                run_cargo_hook.clear_local_git_environment(env, root=Path("/repo"))

        self.assertEqual(env, {"GIT_DIR": "/repo/.git"})

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

            pids = run_cargo_hook.muxd_pids_from_proc(
                proc_root,
                "/tmp/live.sock",
                Path("/tmp/state.json"),
            )
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

    def test_configure_rustc_ice_defaults_to_xdg_cache_home(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            cache_home = Path(tmp) / "cache"
            env = {"XDG_CACHE_HOME": str(cache_home)}

            run_cargo_hook.configure_rustc_ice(env)

            expected = cache_home / "tenex" / "rustc-ice"
            self.assertEqual(env["RUSTC_ICE"], str(expected))
            self.assertTrue(expected.is_dir())

    def test_configure_rustc_ice_keeps_explicit_zero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            env = {
                "HOME": tmp,
                "RUSTC_ICE": "0",
            }

            run_cargo_hook.configure_rustc_ice(env)

            self.assertEqual(env["RUSTC_ICE"], "0")
            self.assertFalse((Path(tmp) / ".cache" / "tenex" / "rustc-ice").exists())


if __name__ == "__main__":
    unittest.main()
