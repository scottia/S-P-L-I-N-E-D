"""Structural regression checks for independently dispatchable Workflows 20–24."""

from pathlib import Path
import os
import re
import shutil
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_ROOT = ROOT / ".github" / "workflows"
WORKFLOWS = {
    number: WORKFLOW_ROOT / filename
    for number, filename in {
        20: "20-quick-portable-windows.yml",
        21: "21-release-preparation.yml",
        22: "22-windows-build-store.yml",
        23: "23-webgui-build.yml",
        24: "24-complete-release.yml",
    }.items()
}


class Workflows20To24Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.sources = {
            number: path.read_text(encoding="utf-8")
            for number, path in WORKFLOWS.items()
        }

    def test_all_five_workflows_are_independently_dispatchable(self) -> None:
        for number, source in self.sources.items():
            self.assertIn("workflow_dispatch:", source, number)
            self.assertIn(f"name: {number}.", source.splitlines()[0], number)

    def test_quick_portable_is_never_a_release_input(self) -> None:
        source = self.sources[20]
        self.assertIn("release_eligible = $false", source)
        self.assertIn("--native-smoke", source)
        self.assertIn("Expected AMD64 PE architecture", source)
        for forbidden in (
            "Build-StorePackage.ps1",
            "gh release create",
            "git push origin",
            "Publish-StoreUpdate.ps1",
        ):
            self.assertNotIn(forbidden, source)

    def test_workflow20_reports_selected_targets_and_completion(self) -> None:
        source = self.sources[20]
        self.assertIn("TEST BUILD (default): KEEP CHECKED", source)
        self.assertIn("PR EVALUATION not checked::Check PR EVALUATION", source)
        self.assertIn('echo "- Outcome: $OUTCOME"', source)
        self.assertNotIn("echo 'QUICK PORTABLE')", source)
        start = source.index('          case "$TARGET_WINDOWS:$TARGET_WEBGUI" in')
        end = source.index('          {\n            echo "## Workflow 20 source plan"', start)
        shell = source[start:end] + '\nprintf "%s" "$OUTCOME"\n'
        cases = (
            ("true", "false", "WINDOWS"),
            ("false", "true", "WEBGUI"),
            ("true", "true", "WINDOWS + WEBGUI"),
        )
        bash = shutil.which("bash")
        if bash is None:
            self.skipTest("Bash is required to exercise Workflow 20 source-plan labels")
        for windows, webgui, target in cases:
            for report in ("true", "false"):
                with self.subTest(target=target, report=report):
                    env = dict(os.environ, TARGET_WINDOWS=windows,
                               TARGET_WEBGUI=webgui, REPORT_ONLY=report)
                    result = subprocess.run(
                        [bash, "-eu", "-c", shell], env=env,
                        check=True, capture_output=True, text=True,
                    )
                    expected = (
                        f"{target} — REPORT ONLY (NO BUILD)"
                        if report == "true" else f"{target} TEST BUILD"
                    )
                    self.assertEqual(result.stdout, expected)

    def test_release_preparation_records_main_and_dev_without_compiling(self) -> None:
        source = self.sources[21]
        self.assertIn("main:refs/remotes/origin/main", source)
        self.assertIn("dev:refs/remotes/origin/dev", source)
        self.assertIn('"main_commit": os.environ["MAIN_SHA"]', source)
        self.assertIn('"source_commit": os.environ["SOURCE_SHA"]', source)
        self.assertIn("workflow-21-release-candidate-approval", source)
        self.assertIn("splined-release-candidate", source)
        for forbidden in ("cargo build", "docker build", "Build-StorePackage.ps1"):
            self.assertNotIn(forbidden, source)

    def test_windows_standard_build_always_produces_msix_and_local_review(self) -> None:
        source = self.sources[22]
        self.assertIn("PARTIAL UPDATE (default)", source)
        self.assertIn("FULL SOURCE", source)
        self.assertIn("Build-StorePackage.ps1", source)
        self.assertIn("Test-DevelopmentPackage.ps1", source)
        self.assertIn("New-LocalStoreReviewBundle.ps1", source)
        self.assertIn("Partner Center contacted: ``false``", source)
        self.assertIn("Standard builds use local review", source)

    def test_store_submission_uses_existing_build_and_bypasses_compilation(self) -> None:
        source = self.sources[22]
        resolver = source.split("  audit-candidate:", 1)[0]
        self.assertIn('compile_mode="NOT APPLICABLE"', resolver)
        self.assertIn("SUBMIT EXISTING MSIX", resolver)
        submit = source.split("  submit-store:", 1)[1].split("  summary:", 1)[0]
        self.assertIn("splined-windows-release", submit)
        self.assertIn("Publish-StoreUpdate.ps1", submit)
        for forbidden in ("cargo build", "Build-StorePackage.ps1", "Compress-Archive"):
            self.assertNotIn(forbidden, submit)

    def test_webgui_retains_exact_smoke_tested_image(self) -> None:
        source = self.sources[23]
        self.assertIn("docker save splined-webgui:release-test", source)
        self.assertIn("splined-webgui-container-image.tar.gz", source)
        self.assertIn("database_ready", source)
        self.assertIn("cached_steps", source)
        for forbidden in ("gh release create", "docker push", "git push origin"):
            self.assertNotIn(forbidden, source)

    def test_publication_workflow_has_no_compiler_or_packager(self) -> None:
        source = self.sources[24]
        self.assertIn("workflow-24-publication-approval", source)
        self.assertIn("splined-publication-inputs", source)
        self.assertIn("Refusing release asset overwrite", source)
        self.assertIn("docker push", source)
        for forbidden in (
            "cargo build",
            "docker buildx build",
            "Build-StorePackage.ps1",
            "Compress-Archive",
            "Publish-StoreUpdate.ps1",
        ):
            self.assertNotIn(forbidden, source)

    def test_report_only_paths_skip_expensive_jobs(self) -> None:
        self.assertIn(
            "needs.resolve-source.outputs.report_only != 'true'",
            self.sources[20],
        )
        self.assertIn(
            "needs.resolve-plan.outputs.prepare_candidate == 'true'",
            self.sources[21],
        )
        self.assertIn(
            "needs.resolve-plan.outputs.completion == 'BUILD ONLY'",
            self.sources[22],
        )
        self.assertIn(
            "needs.resolve-plan.outputs.report_only != 'true'",
            self.sources[23],
        )
        self.assertIn(
            "needs.resolve-plan.outputs.publish == 'true'",
            self.sources[24],
        )

    def test_new_repository_content_has_no_historical_pull_request_references(self) -> None:
        paths = [*WORKFLOWS.values(), ROOT / ".github/scripts/release_artifact_contract.py"]
        paths.extend(
            ROOT / relative
            for relative in (
                "docs/release-automation.md",
                "docs/build-layout.md",
                "docs/comp-report-policy.md",
                "docs/media-state-classification-policy.md",
                "docs/webgui-docker-deployment.md",
                "docs/windows-build-guide.md",
                "docs/windows-store.md",
                "docs/windows-webgui-loading-parity.md",
                "docs/README.md",
                "release/README.md",
            )
        )
        pattern = re.compile(r"(?:PR\s*#|pull/)\d+", re.IGNORECASE)
        for path in paths:
            self.assertIsNone(pattern.search(path.read_text(encoding="utf-8")), path)

    def test_ordinary_operational_docs_use_the_new_architecture(self) -> None:
        paths = (
            ROOT / "docs/release-automation.md",
            ROOT / "docs/build-layout.md",
            ROOT / "docs/comp-report-policy.md",
            ROOT / "docs/media-state-classification-policy.md",
            ROOT / "docs/webgui-docker-deployment.md",
            ROOT / "docs/windows-build-guide.md",
            ROOT / "docs/windows-store.md",
            ROOT / "docs/windows-webgui-loading-parity.md",
            ROOT / "docs/README.md",
            ROOT / "release/README.md",
        )
        old_reference = re.compile(r"workflow\s*10|10-unified-build", re.IGNORECASE)
        for path in paths:
            self.assertIsNone(old_reference.search(path.read_text(encoding="utf-8")), path)


if __name__ == "__main__":
    unittest.main()
