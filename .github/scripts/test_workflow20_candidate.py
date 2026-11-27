"""Candidate labels and reference-only Workflow 20 provenance."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from workflow20_candidate import (
    SCHEMA, candidate_id, create_report, legacy_references, reference,
    resolve_references, validate_report,
)


REPOSITORY = "owner/repository"
SOURCE = "b" * 40
DEFINITION = "a" * 40
ENV = {
    "GITHUB_REPOSITORY": REPOSITORY,
    "GITHUB_RUN_ID": "200",
    "GITHUB_RUN_NUMBER": "8",
    "GITHUB_RUN_ATTEMPT": "1",
    "GITHUB_SHA": DEFINITION,
    "CANDIDATE_ID": "3.0.31-rc.8",
    "OFFICIAL_RELEASE_VERSION": "3.0.31",
    "SOURCE_COMMIT": SOURCE,
    "SOURCE_VERSION": "3.0.35",
    "COMMITTED_SOURCE_VERSION": "3.0.28",
    "SOURCE_MODE": "pr-evaluation",
    "EVALUATION_PR": "356",
    "TARGET_WINDOWS": "true",
    "TARGET_WEBGUI": "true",
    "WINDOWS_RESULT": "success",
    "WEBGUI_RESULT": "success",
}


def run() -> dict:
    return {
        "id": 200,
        "run_number": 8,
        "run_attempt": 1,
        "status": "completed",
        "conclusion": "success",
        "event": "workflow_dispatch",
        "path": ".github/workflows/20-quick-portable-windows.yml",
        "display_title": "WF20 PR 356 v3.0.35",
        "html_url": "https://example.invalid/run/200",
        "repository": {"full_name": REPOSITORY},
        "head_repository": {"full_name": REPOSITORY},
    }


class Workflow20CandidateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_candidate_id_uses_official_target_and_run_number(self):
        self.assertEqual(candidate_id("3.0.31", 8), "3.0.31-rc.8")
        for version, number in (("3.0.31-rc.1", 8), ("3.0.31", 0)):
            with self.assertRaises(ValueError):
                candidate_id(version, number)

    def test_candidate_report_keeps_evaluation_version_reference_only(self):
        with patch("workflow20_candidate.release_product_digest", return_value="d" * 64):
            report = create_report(self.root, ENV)
        self.assertEqual(report["schema"], SCHEMA)
        self.assertEqual(report["candidate_id"], "3.0.31-rc.8")
        self.assertEqual(report["official_release_version_reference"], "3.0.31")
        self.assertEqual(report["evaluation_version"], "3.0.35")
        self.assertFalse(report["release_eligible"])
        self.assertTrue(report["reference_only"])
        self.assertEqual(json.loads((self.root / "workflow20-candidate-report.json").read_text()), report)

    def test_report_validation_and_target_reference(self):
        with patch("workflow20_candidate.release_product_digest", return_value="d" * 64):
            report = create_report(self.root, ENV)
        validate_report(report, run(), REPOSITORY, "3.0.31", "d" * 64)
        windows = reference(report, "windows", run())
        self.assertEqual(windows["candidate_id"], "3.0.31-rc.8")
        self.assertEqual(windows["evaluation_version"], "3.0.35")
        self.assertTrue(windows["reference_only"])
        with self.assertRaises(ValueError):
            validate_report(dict(report, product_digest="e" * 64), run(), REPOSITORY, "3.0.31", "d" * 64)

    def test_legacy_successful_pair_becomes_reference_only(self):
        pull = {
            "merged_at": "2026-11-27T01:38:57Z",
            "base": {"ref": "dev"},
            "head": {"sha": SOURCE, "repo": {"full_name": REPOSITORY}},
        }
        jobs = {
            "total_count": 2,
            "jobs": [
                {"name": "Compile and validate standalone splined.exe", "conclusion": "success"},
                {"name": "Validate and package WebGUI pre-release source", "conclusion": "success"},
            ],
        }
        with patch("workflow20_candidate.api", side_effect=[pull, jobs]), patch(
            "workflow20_candidate.exact_legacy_product", return_value=True
        ):
            refs = legacy_references(run(), REPOSITORY, "3.0.31", "c" * 40)
        self.assertEqual(refs["windows"]["candidate_id"], "3.0.31-rc.8")
        self.assertEqual(refs["webgui"]["evaluation_version"], "3.0.35")
        self.assertEqual(refs["webgui"]["provenance"], "legacy-successful-workflow20-reference")

    def test_resolver_maps_one_candidate_to_selected_clients(self):
        with patch("workflow20_candidate.release_product_digest", return_value="d" * 64), patch(
            "workflow20_candidate.api", return_value={"workflow_runs": [run()]}
        ), patch("workflow20_candidate.download_report") as download:
            with patch("workflow20_candidate.release_product_digest", return_value="d" * 64):
                report = create_report(self.root, ENV)
            download.return_value = report
            refs = resolve_references(
                self.root, {"GITHUB_REPOSITORY": REPOSITORY}, "3.0.31", "c" * 40,
                {"windows", "webgui"},
            )
        self.assertEqual(refs["windows"]["candidate_id"], "3.0.31-rc.8")
        self.assertEqual(refs["webgui"]["candidate_id"], "3.0.31-rc.8")


if __name__ == "__main__":
    unittest.main()
