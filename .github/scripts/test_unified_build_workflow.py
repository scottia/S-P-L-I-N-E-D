"""Nonmutating contract checks for the owner-approved Workflow 10 model."""

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/10-unified-build.yml"


class UnifiedBuildWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = WORKFLOW.read_text(encoding="utf-8")

    def test_old_manual_flows_are_retired(self):
        self.assertFalse((ROOT / ".github/workflows/04-optimized-build-test.yml").exists())
        self.assertFalse((ROOT / ".github/workflows/05-prepare-source-version.yml").exists())

    def test_form_has_target_outcome_and_store_action(self):
        for value in (
            "target:", "outcome:", "store_action:", '- "ALL"', '- "WINDOWS"',
            '- "WEBGUI"', '- "COMPLETE RELEASE"', '- "BUILD ONLY, REPORT"',
            '- "LOCAL REVIEW ONLY"', '- "AUTO SUBMIT"',
        ):
            self.assertIn(value, self.source)
        self.assertNotIn("      compile_mode:", self.source.split("permissions:", 1)[0])
        self.assertNotIn("release_version:", self.source.split("permissions:", 1)[0])

    def test_workflow_never_creates_or_merges_pull_requests(self):
        lowered = self.source.lower()
        for forbidden in ("gh pr create", "gh pr merge", "create-pull-request", ":refs/heads/dev", ":refs/heads/main"):
            self.assertNotIn(forbidden, lowered)

    def test_main_dispatch_selects_exact_dev_source_and_cross_client_version(self):
        self.assertIn('if [ "$GITHUB_REF" != "refs/heads/main" ]', self.source)
        self.assertIn("git fetch --force origin main dev", self.source)
        self.assertIn("git checkout --detach \"$DEV_SHA\"", self.source)
        self.assertIn('git merge-base --is-ancestor "$MAIN_SHA" "$DEV_SHA"', self.source)
        self.assertIn("source_sha=$DEV_SHA", self.source)
        self.assertIn("Committed source versions disagree", self.source)
        self.assertIn('RELEASE_VERSION="$REQUESTED_VERSION"', self.source)
        self.assertNotIn('$RELEASE_COMMIT:refs/heads/main', self.source)

    def test_plan_is_approved_before_compilation(self):
        approval = self.source.index("name: workflow-10-plan-approval")
        self.assertLess(approval, self.source.index("  build-windows:"))
        self.assertLess(approval, self.source.index("  build-webgui:"))

    def test_full_and_partial_are_automatic_complete_compile_strategies(self):
        self.assertIn('compile_mode="PARTIAL UPDATE"', self.source)
        self.assertIn('compile_mode="FULL SOURCE"', self.source)
        self.assertIn('if [ "${MAIN_VERSION:-}" = "$SOURCE_VERSION" ]', self.source)
        self.assertIn("cargo clean --manifest-path", self.source)
        self.assertIn("cargo build --locked --release", self.source)
        self.assertIn("--cache-from type=gha,scope=splined-workflow10-webgui", self.source)
        self.assertIn("--no-cache", self.source)

    def test_existing_release_completion_never_clobbers_assets(self):
        self.assertIn("gh release upload", self.source)
        self.assertIn("Refusing release-asset overwrite", self.source)
        self.assertNotIn("--clobber", self.source)
        self.assertIn("Source changed after this release", self.source)

    def test_exact_smoke_tested_webgui_image_is_published(self):
        self.assertIn("splined-webgui-container-image", self.source)
        self.assertIn("docker save splined-webgui:release-test", self.source)
        self.assertIn("gzip -dc container-image/splined-webgui-container-image.tar.gz | docker load", self.source)

    def test_workflow_10_does_not_repeat_real_writer_integrity_suites(self):
        for marker in (
            "python -m unittest build/WEBGui/webgui/tests/test_audio_integrity.py",
            "artwork_integrity::tests --",
            "test_cross_format_cover_overwrite.py -v",
        ):
            self.assertNotIn(marker, self.source)
        for excluded in (
            "test_splined_compilation_authority_policy.py", "test_audio_integrity.py",
            "test_comp_job_scope.py", "test_cross_format_cover_overwrite.py",
        ):
            self.assertIn(excluded, self.source)

    def test_build_only_report_is_nonpublishing_and_audits_credentials(self):
        self.assertIn("BUILD ONLY, REPORT — source, policy and credential audit", self.source)
        self.assertIn("AZURE_AD_APPLICATION_CLIENT_ID", self.source)
        self.assertIn("Branch write, PR, tag, release, GHCR, and Store submission", self.source)

    def test_store_defaults_to_review_and_supports_explicit_auto_submit(self):
        self.assertIn("store_draft=$build_windows", self.source)
        self.assertIn('"LOCAL REVIEW ONLY") store_submit=false', self.source)
        self.assertIn("Submit validated Microsoft Store update", self.source)
        self.assertIn("AZURE_AD_APPLICATION_SECRET", self.source)
        self.assertIn("msstore reconfigure", self.source)
        self.assertIn("Publish-StoreUpdate.ps1", self.source)


if __name__ == "__main__":
    unittest.main()
