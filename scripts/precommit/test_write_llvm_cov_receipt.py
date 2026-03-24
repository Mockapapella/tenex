from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


def load_module():
    path = Path(__file__).with_name("write_llvm_cov_receipt.py")
    spec = importlib.util.spec_from_file_location("write_llvm_cov_receipt", path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


write_llvm_cov_receipt = load_module()

RAW_WINDOWS_REPORT_A = """Filename                                 Regions    Missed Regions     Cover   Functions  Missed Functions  Executed       Lines      Missed Lines     Cover    Branches   Missed Branches     Cover
----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
runtime/docker.rs                           3805               603    84.15%         204                45    77.94%        2258               234    89.64%           0                 0         -
TOTAL                                      62764              8202    86.93%        3357               333    90.08%       36631              3536    90.35%           0                 0         -
"""

RAW_WINDOWS_REPORT_B = """Filename                                 Regions    Missed Regions     Cover   Functions  Missed Functions  Executed       Lines      Missed Lines     Cover    Branches   Missed Branches     Cover
----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
runtime/docker.rs                           3805               603    84.15%         204                45    77.94%        2258               232    89.73%           0                 0         -
TOTAL                                      62764              8202    86.93%        3357               333    90.08%       36631              3534    90.35%           0                 0         -
"""


class WriteLlvmCovReceiptTests(unittest.TestCase):
    def test_windows_receipt_drops_line_columns(self) -> None:
        self.assertEqual(
            write_llvm_cov_receipt.canonicalize_report("windows", RAW_WINDOWS_REPORT_A),
            write_llvm_cov_receipt.canonicalize_report("windows", RAW_WINDOWS_REPORT_B),
        )

    def test_non_windows_receipt_is_preserved(self) -> None:
        raw = "hello"
        self.assertEqual(
            write_llvm_cov_receipt.canonicalize_report("linux", raw),
            "hello\n",
        )

    def test_write_receipt_writes_sha_for_report_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            input_path = root / "input.txt"
            report_path = root / "report.txt"
            sha_path = root / "report.sha256"

            input_path.write_text(RAW_WINDOWS_REPORT_A, encoding="utf-8")
            write_llvm_cov_receipt.write_receipt(
                "windows",
                input_path,
                report_path,
                sha_path,
            )

            self.assertEqual(
                report_path.read_text(encoding="utf-8"),
                "\n".join(
                    [
                        "Filename\tRegions\tMissed Regions\tRegion Cover\tFunctions\tMissed Functions\tFunction Cover\tBranches\tMissed Branches\tBranch Cover",
                        "runtime/docker.rs\t3805\t603\t84.15%\t204\t45\t77.94%\t0\t0\t-",
                        "TOTAL\t62764\t8202\t86.93%\t3357\t333\t90.08%\t0\t0\t-",
                        "",
                    ]
                ),
            )
            self.assertEqual(
                sha_path.read_text(encoding="utf-8").split(maxsplit=1)[1].strip(),
                report_path.as_posix(),
            )


if __name__ == "__main__":
    unittest.main()
