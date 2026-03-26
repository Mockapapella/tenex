from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


def load_module():
    path = Path(__file__).with_name("coverage_theoretical_max.py")
    spec = importlib.util.spec_from_file_location("coverage_theoretical_max", path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


coverage_theoretical_max = load_module()


class CoverageTheoreticalMaxTests(unittest.TestCase):
    def test_normalize_source_path_prefers_shortest_existing_suffix(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src" / "action").mkdir(parents=True)
            (root / "src" / "action" / "agent.rs").write_text("x", encoding="utf-8")
            (root / "build.rs").write_text("x", encoding="utf-8")

            normalized = coverage_theoretical_max.normalize_source_path(
                "/home/user/workspace/src/action/agent.rs",
                root,
            )
            self.assertEqual(normalized, "src/action/agent.rs")

            normalized = coverage_theoretical_max.normalize_source_path(
                r"C:\Users\me\repo\build.rs",
                root,
            )
            self.assertEqual(normalized, "build.rs")

            normalized = coverage_theoretical_max.normalize_source_path(
                "/nope/does/not/exist.rs",
                root,
            )
            self.assertIsNone(normalized)

    def test_parse_lcov_collects_line_sets(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "src" / "foo.rs").write_text("x", encoding="utf-8")
            (root / "src" / "bar.rs").write_text("x", encoding="utf-8")

            lcov = "\n".join(
                [
                    "SF:/abs/prefix/src/foo.rs",
                    "DA:1,0",
                    "DA:2,3",
                    "end_of_record",
                    "SF:/other/src/bar.rs",
                    "DA:10,1",
                    "end_of_record",
                    "",
                ]
            )
            lcov_path = root / "sample.lcov"
            lcov_path.write_text(lcov, encoding="utf-8")

            parsed = coverage_theoretical_max.parse_lcov(lcov_path, root)
            self.assertEqual(len(parsed.instrumented), 3)
            self.assertEqual(len(parsed.covered), 2)

    def test_compute_summary_unions_by_file_and_line(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "src" / "foo.rs").write_text("x", encoding="utf-8")
            (root / "src" / "bar.rs").write_text("x", encoding="utf-8")

            linux = "\n".join(
                [
                    "SF:/x/src/foo.rs",
                    "DA:1,1",
                    "DA:2,0",
                    "end_of_record",
                    "",
                ]
            )
            macos = "\n".join(
                [
                    "SF:/y/src/foo.rs",
                    "DA:2,1",
                    "end_of_record",
                    "SF:/y/src/bar.rs",
                    "DA:10,0",
                    "end_of_record",
                    "",
                ]
            )

            (root / "linux.lcov").write_text(linux, encoding="utf-8")
            (root / "macos.lcov").write_text(macos, encoding="utf-8")
            (root / "windows.lcov").write_text(linux, encoding="utf-8")

            platforms = {
                "linux": coverage_theoretical_max.parse_lcov(root / "linux.lcov", root),
                "macos": coverage_theoretical_max.parse_lcov(root / "macos.lcov", root),
                "windows": coverage_theoretical_max.parse_lcov(root / "windows.lcov", root),
            }
            payload, markdown = coverage_theoretical_max.compute_summary(platforms, sha=None)

            self.assertEqual(payload["union"]["instrumented_lines"], 3)
            self.assertEqual(payload["platforms"]["linux"]["instrumented_lines"], 2)
            self.assertEqual(payload["platforms"]["linux"]["covered_lines"], 1)
            self.assertIn("| linux |", markdown)


if __name__ == "__main__":
    unittest.main()
