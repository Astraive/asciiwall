"""Regression tests for QA-101 hosted evidence manifest failures."""
from __future__ import annotations

import unittest

import qa101_ci_evidence


class RunnerFailureResultTests(unittest.TestCase):
    def test_preserves_observed_scenario_status_after_evidence_failure(self):
        manifest = {"test": {"status": "fail"}}

        qa101_ci_evidence.runner_failure_result(
            manifest,
            stage="qa004-validation",
            error_type="RuntimeError",
            observed_status="fail",
            observed_summary="Scenario ran and failed.",
        )

        self.assertEqual(manifest["result"], {"status": "fail", "summary": "Scenario ran and failed."})
        self.assertEqual(manifest["test"]["status"], "fail")
        self.assertEqual(
            manifest["evidence_processing_failure"],
            {"stage": "qa004-validation", "error_type": "RuntimeError"},
        )

    def test_records_not_run_when_scenario_never_ran(self):
        manifest = {}

        qa101_ci_evidence.runner_failure_result(
            manifest,
            stage="cargo-build",
            error_type="RuntimeError",
            observed_status=None,
            observed_summary=None,
        )

        self.assertEqual(manifest["result"], {"status": "not-run"})
        self.assertEqual(
            manifest["evidence_processing_failure"],
            {"stage": "cargo-build", "error_type": "RuntimeError"},
        )


if __name__ == "__main__":
    unittest.main()
