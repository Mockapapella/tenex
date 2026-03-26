from __future__ import annotations

import importlib.util
import json
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


def write_summary_json(path: Path, *, files: list[dict[str, object]]) -> None:
    payload = {"data": [{"files": files, "totals": {}}]}
    path.write_text(json.dumps(payload), encoding="utf-8")


class CoverageTheoreticalMaxTests(unittest.TestCase):
    def test_format_ratio_percent_decimal_rounds_and_pads(self) -> None:
        self.assertEqual(
            coverage_theoretical_max.format_ratio_percent_decimal(1, 2, 5),
            "50.00000",
        )
        self.assertEqual(
            coverage_theoretical_max.format_ratio_percent_decimal(1, 3, 2),
            "33.33",
        )
        self.assertEqual(
            coverage_theoretical_max.format_ratio_percent_decimal(2, 3, 2),
            "66.67",
        )

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

    def test_parse_llvm_cov_summary_json_collects_file_summaries(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "src" / "foo.rs").write_text("x", encoding="utf-8")

            summary_path = root / "sample.summary.json"
            write_summary_json(
                summary_path,
                files=[
                    {
                        "filename": "/abs/prefix/src/foo.rs",
                        "summary": {
                            "lines": {"count": 2, "covered": 1},
                            "functions": {"count": 1, "covered": 1},
                            "regions": {"count": 3, "covered": 2},
                            "branches": {"count": 0, "covered": 0},
                        },
                    }
                ],
            )

            parsed = coverage_theoretical_max.parse_llvm_cov_summary_json(
                summary_path, root
            )
            self.assertEqual(parsed.unknown_sources, set())
            self.assertIn("src/foo.rs", parsed.by_file)
            metrics = parsed.by_file["src/foo.rs"]
            self.assertEqual(metrics["lines"].instrumented, 2)
            self.assertEqual(metrics["lines"].covered, 1)

    def test_compute_summary_unions_by_file_max(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "src" / "foo.rs").write_text("x", encoding="utf-8")

            linux_path = root / "linux.summary.json"
            macos_path = root / "macos.summary.json"
            windows_path = root / "windows.summary.json"

            write_summary_json(
                linux_path,
                files=[
                    {
                        "filename": "/x/src/foo.rs",
                        "summary": {
                            "lines": {"count": 2, "covered": 1},
                            "functions": {"count": 1, "covered": 1},
                            "regions": {"count": 3, "covered": 2},
                            "branches": {"count": 0, "covered": 0},
                        },
                    }
                ],
            )
            write_summary_json(
                macos_path,
                files=[
                    {
                        "filename": "/y/src/foo.rs",
                        "summary": {
                            "lines": {"count": 3, "covered": 3},
                            "functions": {"count": 2, "covered": 2},
                            "regions": {"count": 4, "covered": 4},
                            "branches": {"count": 0, "covered": 0},
                        },
                    }
                ],
            )
            write_summary_json(
                windows_path,
                files=[
                    {
                        "filename": "/z/src/foo.rs",
                        "summary": {
                            "lines": {"count": 2, "covered": 2},
                            "functions": {"count": 1, "covered": 1},
                            "regions": {"count": 3, "covered": 3},
                            "branches": {"count": 0, "covered": 0},
                        },
                    }
                ],
            )

            platforms = {
                "linux": coverage_theoretical_max.parse_llvm_cov_summary_json(
                    linux_path, root
                ),
                "macos": coverage_theoretical_max.parse_llvm_cov_summary_json(
                    macos_path, root
                ),
                "windows": coverage_theoretical_max.parse_llvm_cov_summary_json(
                    windows_path, root
                ),
            }
            payload, markdown = coverage_theoretical_max.compute_summary(platforms, sha=None)

            self.assertEqual(payload["union"]["lines"]["instrumented"], 3)
            self.assertEqual(payload["union"]["functions"]["instrumented"], 2)
            self.assertEqual(payload["union"]["regions"]["instrumented"], 4)
            self.assertEqual(payload["union"]["branches"]["instrumented"], 0)

            self.assertEqual(payload["platforms"]["linux"]["lines"]["instrumented"], 2)
            self.assertEqual(payload["platforms"]["linux"]["lines"]["covered"], 1)

            self.assertIn("### Lines", markdown)
            self.assertIn("| linux |", markdown)


if __name__ == "__main__":
    unittest.main()

