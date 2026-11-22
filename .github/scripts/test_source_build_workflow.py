"""Safety and contract checks for Workflow 05 source preparation."""
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_04 = ROOT / ".github/workflows/04-optimized-build-test.yml"
WORKFLOW_05 = ROOT / ".github/workflows/05-prepare-source-version.yml"
VALIDATE = ROOT / ".github/workflows/validate.yml"


def job(source: str, name: str) -> str:
    match = re.search(r"(?m)^  " + re.escape(name) + r":\s*$", source)
    if not match:
        raise AssertionError(f"Missing job: {name}")
    following = re.search(r"(?m)^  [a-z][a-z0-9-]*:\s*$", source[match.end():])
    end = match.end() + following.start() if following else len(source)
    return source[match.start():end]


class SourceBuildWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.workflow_04 = WORKFLOW_04.read_text(encoding="utf-8")
        cls.workflow_05 = WORKFLOW_05.read_text(encoding="utf-8")
        cls.validate = VALIDATE.read_text(encoding="utf-8")

    def test_source_build_is_separate_from_four_release_presets(self) -> None:
        self.assertIn("Select One of Four Build Presets", self.workflow_04)
        self.assertNotIn("SOURCE BUILD", self.workflow_04)
        for preset in ("ALL", "PYTHON / WEBGUI", "WINDOWS", "TEST BUILD"):
            self.assertIn(f'- "{preset}"', self.workflow_04)
        self.assertIn("Prepare Next Source Version", self.workflow_05)

    def test_draft_pr_precedes_source_stamping(self) -> None:
        prepare = job(self.workflow_05, "prepare-source")
        create_pr = prepare.index("gh pr create")
        stamp = prepare.index("source_version.py stamp")
        self.assertLess(create_pr, stamp)
        self.assertIn("--draft", prepare)
        self.assertIn("--base dev", prepare)
        self.assertIn("source-version-preparation.md", prepare)
        self.assertIn("rm .github/pr-intent/source-version-preparation.md", prepare)
        self.assertLess(
            prepare.index('echo "pr_url=$PR_URL"'),
            prepare.index("source_version.py stamp"),
        )
        self.assertNotIn("gh pr ready", self.workflow_05)

    def test_preparation_requires_synchronized_main_and_dev_trees(self) -> None:
        prepare = job(self.workflow_05, "prepare-source")
        self.assertIn('git fetch origin main dev --tags --force', prepare)
        self.assertIn('MAIN_SHA=$(git rev-parse origin/main)', prepare)
        self.assertIn('DEV_SHA=$(git rev-parse origin/dev)', prepare)
        self.assertIn('MAIN_TREE=$(git rev-parse "$MAIN_SHA^{tree}")', prepare)
        self.assertIn('DEV_TREE=$(git rev-parse "$DEV_SHA^{tree}")', prepare)
        self.assertIn('if [ "$MAIN_TREE" != "$DEV_TREE" ]', prepare)
        self.assertNotIn('if [ "$MAIN_SHA" != "$DEV_SHA" ]', prepare)
        self.assertIn('if [ "$GITHUB_REF" != "refs/heads/main" ]', prepare)

    def test_preparation_never_writes_protected_branches_or_publishes(self) -> None:
        self.assertNotIn("HEAD:refs/heads/dev", self.workflow_05)
        self.assertNotIn("HEAD:refs/heads/main", self.workflow_05)
        self.assertNotIn("--force", self.workflow_05.replace("--tags --force", ""))
        for forbidden in (
            "gh release create",
            "docker push",
            "msstore",
            "Publish-StoreUpdate",
            "git push --atomic",
            "gh pr merge",
        ):
            self.assertNotIn(forbidden, self.workflow_05)
        self.assertNotRegex(self.workflow_05, r"(?m)^\s+git tag\s")
        for protected in (
            "media-state-classification.json",
            "comp-report-policy.json",
        ):
            self.assertNotIn(protected, self.workflow_05)

    def test_exact_candidate_compiles_for_both_clients_without_integrity_rerun(self) -> None:
        windows = job(self.workflow_05, "compile-windows")
        webgui = job(self.workflow_05, "compile-webgui")
        for block in (windows, webgui):
            self.assertIn("needs.prepare-source.outputs.commit", block)
        self.assertIn("cargo build --locked --release", windows)
        self.assertIn("--write-build-info", windows)
        self.assertIn("docker build --file build/WEBGui/Dockerfile", webgui)
        self.assertIn("curl --fail --silent", webgui)
        for forbidden in (
            "test_audio_integrity.py",
            "test_comp_job_scope.py",
            "test_cross_format_cover_overwrite.py",
            "artwork_integrity::tests",
            "embedded_artwork",
            "local_artwork",
        ):
            self.assertNotIn(forbidden, self.workflow_05)

    def test_candidate_receives_required_validation_once(self) -> None:
        prepare = job(self.workflow_05, "prepare-source")
        self.assertIn("gh workflow run validate.yml", prepare)
        self.assertIn('--ref "$BRANCH_NAME"', prepare)
        self.assertNotIn("-f base_sha=", prepare)
        self.assertNotIn("-f head_sha=", prepare)
        self.assertIn("workflow_dispatch:", self.validate)
        self.assertIn("VALIDATION_BASE_SHA", self.validate)
        self.assertIn("VALIDATION_HEAD_SHA", self.validate)
        self.assertIn("github.sha", self.validate)
        self.assertEqual(self.validate.count("git fetch origin dev"), 2)
        self.assertEqual(
            self.validate.count("ref: ${{ env.VALIDATION_HEAD_SHA }}"),
            7,
        )
        self.assertIn("Audio integrity — WebGUI real artwork writer", self.validate)
        self.assertIn("Audio integrity — Windows real artwork writer", self.validate)

    def test_candidate_stays_draft_after_compilation(self) -> None:
        summary = job(self.workflow_05, "source-summary")
        self.assertIn("remains **Draft**", summary)
        self.assertIn("Automatic merge or publication: none", summary)
        self.assertIn("The Draft PR remains unmerged", summary)


if __name__ == "__main__":
    unittest.main()
