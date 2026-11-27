"""Structural contracts for the three official single-run release workflows."""

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_ROOT = ROOT / ".github" / "workflows"
ALL = WORKFLOW_ROOT / "01-build-all-release.yml"
WINDOWS = WORKFLOW_ROOT / "02-build-windows-release.yml"
WEBGUI = WORKFLOW_ROOT / "03-build-webgui-release.yml"


def source(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class OfficialReleaseWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.all = source(ALL)
        cls.windows = source(WINDOWS)
        cls.webgui = source(WEBGUI)

    def test_only_three_official_numbered_release_entries_exist(self) -> None:
        expected = {
            "01-build-all-release.yml",
            "02-build-windows-release.yml",
            "03-build-webgui-release.yml",
        }
        actual = {
            path.name
            for path in WORKFLOW_ROOT.glob("*.yml")
            if path.name[:2] in {"01", "02", "03", "10", "21", "22", "23", "24"}
        }
        self.assertEqual(actual, expected)

    def test_public_names_and_manual_dispatch_are_exact(self) -> None:
        for path, workflow, name in (
            (ALL, self.all, "01. Build ALL [Release]"),
            (WINDOWS, self.windows, "02. Build Windows [Release]"),
            (WEBGUI, self.webgui, "03. Build WebGUI [Release]"),
        ):
            with self.subTest(path=path):
                self.assertEqual(workflow.splitlines()[0], f"name: {name}")
                self.assertIn("workflow_dispatch:", workflow)

    def test_target_workflows_call_the_same_internal_release_graph(self) -> None:
        for workflow, target in ((self.windows, "WINDOWS"), (self.webgui, "WEBGUI")):
            self.assertIn("uses: ./.github/workflows/01-build-all-release.yml", workflow)
            self.assertIn(f"release_target: {target}", workflow)
            self.assertIn("secrets: inherit", workflow)
            self.assertNotIn("run_id", workflow.lower())
        self.assertIn("workflow_call:", self.all)
        self.assertIn("inputs.release_target || 'ALL'", self.all)

    def test_release_source_is_exact_main_and_ignores_workflow20_artifacts(self) -> None:
        self.assertIn('if [ "$GITHUB_REF" != "refs/heads/main" ]', self.all)
        self.assertIn("main:refs/remotes/origin/main", self.all)
        self.assertIn('test "$MAIN_SHA" = "$GITHUB_SHA"', self.all)
        self.assertIn('source_relation=exact-main', self.all)
        self.assertIn("dev:refs/remotes/origin/dev", self.all)
        self.assertIn("DEV_CACHE_SHA=$(git rev-parse origin/dev)", self.all)
        self.assertIn("dev_cache_sha=$DEV_CACHE_SHA", self.all)
        self.assertNotIn("splined-quick-portable", self.all)
        self.assertIn('"workflow20_artifact_reused": False', self.all)

    def test_candidate_and_build_reports_remember_identity_without_manual_ids(self) -> None:
        self.assertIn("candidate_id=$candidate_id", self.all)
        self.assertIn("splined-official-candidate-report", self.all)
        self.assertIn("splined-official-build-report", self.all)
        self.assertIn('"msix_created": False', self.all)
        self.assertIn("Candidate ID changed between validation and build", self.all)
        self.assertLess(
            self.all.index("candidate-report:"),
            self.all.index("build-windows:"),
        )
        self.assertLess(
            self.all.index("validate-build-report:"),
            self.all.index("finalize-tag:"),
        )

    def test_release_trusts_dev_promotion_without_rerunning_source_tests(self) -> None:
        self.assertIn('"candidate_validation": "trusted-dev-promotion"', self.all)
        self.assertNotIn("webgui-tests:", self.all)
        self.assertNotIn("validate_release_policy_contracts.py", self.all)
        self.assertNotIn("python3 -m unittest", self.all)
        self.assertNotIn("python -m unittest", self.all)
        self.assertIn("Build and smoke-test WebGUI container", self.all)
        self.assertIn("--native-smoke", self.all)

    def test_windows_release_does_not_run_rust_slint_strict_validation(self) -> None:
        for command in ("cargo test", "cargo check", "cargo fmt", "cargo clippy"):
            self.assertNotIn(command, self.all)
        self.assertIn(
            "cargo build --locked --release --manifest-path build/Windows/Cargo.toml",
            self.all,
        )

    def test_selected_targets_build_from_exact_source_with_current_dev_cache(self) -> None:
        self.assertIn("compile_mode=FULL SOURCE", self.all)
        self.assertNotIn("cargo clean --manifest-path build/Windows/Cargo.toml --release", self.all)
        self.assertIn("CARGO_PROFILE_RELEASE_INCREMENTAL: \"true\"", self.all)
        self.assertIn("splined-wf20-incremental-compiled-v3", self.all)
        self.assertIn("needs.resolve-plan.outputs.dev_cache_sha", self.all)
        self.assertIn("cargo build --locked --release --manifest-path build/Windows/Cargo.toml", self.all)
        self.assertIn("Build-StorePackage.ps1", self.all)
        self.assertIn("Test-DevelopmentPackage.ps1", self.all)
        self.assertIn("docker save splined-webgui:release-test", self.all)
        self.assertIn("actions/github-script@ed597411d8f924073f98dfc5c65a23a2325f34cd", self.all)
        self.assertNotIn("crazy-max/ghaction-github-runtime", self.all)
        self.assertIn('core.exportVariable("ACTIONS_RUNTIME_TOKEN", token)', self.all)
        self.assertIn("--cache-from type=gha,scope=splined-workflow20-webgui-${DEV_CACHE_SHA}", self.all)
        self.assertIn("--cache-to type=gha,mode=max,scope=splined-official-release-webgui", self.all)
        self.assertIn('docker buildx build --progress=plain "${cache_args[@]}" --load', self.all)
        self.assertIn("docker push", self.all)
        self.assertIn("Generate-ReleaseNotes.ps1", self.all)
        self.assertIn("-ConfigPath release/cliff.toml", self.all)

    def test_store_result_is_draft_only_and_never_partner_center_submission(self) -> None:
        self.assertIn("name: Prepare local Microsoft Store review bundle", self.all)
        self.assertIn("New-LocalStoreReviewBundle.ps1", self.all)
        self.assertNotIn("Publish-StoreUpdate.ps1", self.all)
        self.assertNotIn("microsoft-store-apppublisher", self.all)
        self.assertNotIn("AZURE_AD_APPLICATION_SECRET", self.all)

    def test_every_external_action_is_pinned(self) -> None:
        for path, workflow in ((ALL, self.all), (WINDOWS, self.windows), (WEBGUI, self.webgui)):
            for line in workflow.splitlines():
                stripped = line.strip()
                if not stripped.startswith("uses: "):
                    continue
                action = stripped.removeprefix("uses: ").split()[0]
                if action.startswith("./"):
                    continue
                self.assertIn("@", action, f"{path}: {action}")
                revision = action.rsplit("@", 1)[1]
                self.assertEqual(len(revision), 40, f"{path}: {action}")
                self.assertTrue(all(ch in "0123456789abcdefABCDEF" for ch in revision))


if __name__ == "__main__":
    unittest.main()
