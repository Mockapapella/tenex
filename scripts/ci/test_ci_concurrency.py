from __future__ import annotations

import unittest
from pathlib import Path

import yaml


WORKFLOW_PATH = Path(__file__).resolve().parents[2] / ".github/workflows/ci.yml"
EXPECTED_CONCURRENCY = {
    "group": (
        "ci-${{ github.workflow }}-"
        "${{ github.event_name == 'pull_request' && github.ref || github.run_id }}"
    ),
    "cancel-in-progress": "${{ github.event_name == 'pull_request' }}",
}


class CiConcurrencyTests(unittest.TestCase):
    def test_workflow_uses_event_specific_concurrency_contract(self) -> None:
        workflow = yaml.safe_load(WORKFLOW_PATH.read_text(encoding="utf-8"))
        self.assertIsInstance(workflow, dict)
        self.assertEqual(workflow.get("concurrency"), EXPECTED_CONCURRENCY)


if __name__ == "__main__":
    unittest.main()
