"""Workflow 20 PR evaluation source and version guards."""
from copy import deepcopy
from pathlib import Path
import unittest

from quick_portable_evaluation import resolve_pr, validate_version


REPO = "owner/repository"
SHA = "a" * 40
PR = {
    "number": 42,
    "state": "open",
    "base": {"ref": "dev", "repo": {"full_name": REPO}},
    "head": {"ref": "fix/evaluation", "sha": SHA, "repo": {"full_name": REPO}},
}


class QuickPortableEvaluationTests(unittest.TestCase):
    def test_resolves_selected_open_pr_to_exact_head(self):
        self.assertEqual(resolve_pr(PR, REPO, 42), SHA)

    def test_rejects_wrong_number_closed_pr_fork_wrong_base_invalid_head(self):
        with self.assertRaises(ValueError):
            resolve_pr(PR, REPO, 43)
        for field, value in (
            ("sha", "a" * 7),
            ("repo", {"full_name": "fork/repository"}),
            ("repo", None),
            ("ref", "main"),
            ("ref", "dev"),
        ):
            candidate = deepcopy(PR)
            candidate["head"][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                resolve_pr(candidate, REPO, 42)
        candidate = deepcopy(PR)
        candidate["state"] = "closed"
        with self.assertRaises(ValueError):
            resolve_pr(candidate, REPO, 42)
        candidate = deepcopy(PR)
        candidate["base"]["ref"] = "main"
        with self.assertRaises(ValueError):
            resolve_pr(candidate, REPO, 42)

    def test_explicit_numeric_version_floor(self):
        validate_version("3.0.28", "3.0.32", ["3.0.30", "v3.0.31", "not-a-version"])
        for version in ("3.0.28", "3.0.31", "3.0.32-dev", "03.0.32", ""):
            with self.subTest(version=version), self.assertRaises(ValueError):
                validate_version("3.0.28", version, ["v3.0.31"])

    def test_workflow_definition_and_product_pr_are_independent(self):
        workflow = (
            Path(__file__).resolve().parents[1] / "workflows/20-quick-portable-windows.yml"
        ).read_text(encoding="utf-8")
        self.assertIn('SOURCE_SHA=$(git rev-parse origin/dev)', workflow)
        self.assertIn('SOURCE_REF="refs/pull/$EVALUATION_PR/head"', workflow)
        self.assertIn('repos/$GITHUB_REPOSITORY/pulls/$EVALUATION_PR', workflow)
        self.assertIn('git rev-parse FETCH_HEAD', workflow)
        self.assertIn('cp .github/scripts/quick_portable_evaluation.py "$SELECTOR"', workflow)
        self.assertNotIn('SOURCE_SHA="$GITHUB_SHA"', workflow)
        self.assertIn("source_version.py stamp-release", workflow)
        self.assertIn("release_eligible = $false", workflow)
        self.assertLess(workflow.index("source_version.py stamp-release"), workflow.index("cargo build --locked"))
        for forbidden in ("git push", "gh release create", "Build-StorePackage.ps1"):
            self.assertNotIn(forbidden, workflow)


if __name__ == "__main__":
    unittest.main()
