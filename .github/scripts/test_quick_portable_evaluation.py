"""Workflow 20 PR evaluation selection and version guards."""
from copy import deepcopy
from pathlib import Path
import unittest

from quick_portable_evaluation import resolve_pr, validate_version


REPO = "owner/repository"
BRANCH = "fix/evaluation"
SHA = "a" * 40
PR = {
    "number": 42,
    "state": "open",
    "base": {"ref": "dev", "repo": {"full_name": REPO}},
    "head": {"ref": BRANCH, "sha": SHA, "repo": {"full_name": REPO}},
}


class QuickPortableEvaluationTests(unittest.TestCase):
    def test_exact_open_pr(self):
        self.assertEqual(resolve_pr([PR], REPO, BRANCH, SHA), 42)

    def test_reject_stale_head_closed_pr_fork_wrong_base_and_ambiguous_pr(self):
        for field, value in (("sha", "b" * 40), ("repo", {"full_name": "fork/repository"})):
            candidate = deepcopy(PR)
            candidate["head"][field] = value
            with self.assertRaises(ValueError):
                resolve_pr([candidate], REPO, BRANCH, SHA)
        candidate = deepcopy(PR)
        candidate["state"] = "closed"
        with self.assertRaises(ValueError):
            resolve_pr([candidate], REPO, BRANCH, SHA)
        candidate = deepcopy(PR)
        candidate["base"]["ref"] = "main"
        with self.assertRaises(ValueError):
            resolve_pr([candidate], REPO, BRANCH, SHA)
        for pulls in ([], [PR, PR]):
            with self.assertRaises(ValueError):
                resolve_pr(pulls, REPO, BRANCH, SHA)
        for branch in ("main", "dev"):
            with self.assertRaises(ValueError):
                resolve_pr([PR], REPO, branch, SHA)

    def test_explicit_numeric_version_floor(self):
        validate_version("3.0.28", "3.0.32", ["3.0.30", "v3.0.31", "not-a-version"])
        for version in ("3.0.28", "3.0.31", "3.0.32-dev", ""):
            with self.assertRaises(ValueError):
                validate_version("3.0.28", version, ["v3.0.31"])

    def test_workflow_keeps_normal_source_and_nonpublishable_artifact(self):
        workflow = (
            Path(__file__).resolve().parents[1] / "workflows/20-quick-portable-windows.yml"
        ).read_text(encoding="utf-8")
        self.assertIn('SOURCE_SHA=$(git rev-parse origin/dev)', workflow)
        self.assertIn('SOURCE_SHA="$GITHUB_SHA"', workflow)
        self.assertIn("quick_portable_evaluation.py", workflow)
        self.assertIn("source_version.py stamp-release", workflow)
        self.assertIn("release_eligible = $false", workflow)
        self.assertLess(workflow.index("source_version.py stamp-release"), workflow.index("cargo build --locked"))
        for forbidden in ("git push", "gh release create", "Build-StorePackage.ps1"):
            self.assertNotIn(forbidden, workflow)


if __name__ == "__main__":
    unittest.main()
