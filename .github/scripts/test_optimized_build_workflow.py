"""Read-only contract checks for manually dispatched optimized QA Workflow 04."""

from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/04-optimized-build-test.yml"
RELEASE = ROOT / ".github/workflows/01-full-release.yml"


class OptimizedBuildWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.release = RELEASE.read_text(encoding="utf-8")

    def test_workflow_is_manual_and_retains_all_release_platform_choices(self):
        script = self.workflow
        self.assertIn("workflow_dispatch:", script)
        self.assertNotIn("  pull_request:", script)
        for control in (
            "release_version", "platform", "extended_windows_validation",
            "store_submit_update", "store_submit_draft", "release_confirmation",
            "benchmark_mode",
        ):
            self.assertRegex(script, rf"(?m)^      {control}:$")
        for option in ('"All"', '"Windows"', '"WebGUI / Python"'):
            self.assertIn(f'- {option}', script)
        self.assertIn('default: "All"', script)
        self.assertIn('"Run optimized test only"', script)
        self.assertIn('"Stop - no test"', script)
        self.assertIn('"Cold and optimized"', script)

    def test_workflow_has_no_publish_credentials_or_ref_mutations(self):
        script = self.workflow
        self.assertIn("permissions:\n  contents: read\n", script)
        self.assertIn("persist-credentials: false", script)
        self.assertIn("TEST_CONFIRMATION", script)
        self.assertIn("SIMULATE Submit-mode Store QA only", script)
        forbidden = (
            "contents: write", "packages: write", "id-token: write",
            "git push", "git commit", "git tag", "gh release create",
            "docker push", "winapp store submit", "store submit --",
            "publish-release:", "publish-ghcr:", "publish-store:",
            "Create release now", "PARTNER_CENTER",
        )
        for command in forbidden:
            with self.subTest(command=command):
                self.assertNotIn(command, script)
        self.assertIn("No release/tag/Store/GHCR publication", script)

    def test_optimized_windows_compiles_fresh_binary_from_cached_dependencies(self):
        text = self.workflow
        self.assertIn('RUST_TOOLCHAIN: "1.99.0"', text)
        self.assertIn('WINDOWS_TARGET: "x86_64-pc-windows-msvc"', text)
        self.assertIn("build/Windows/target/x86_64-pc-windows-msvc/release/deps", text)
        self.assertIn("build/Windows/target/x86_64-pc-windows-msvc/release/.fingerprint", text)
        self.assertIn("build/Windows/target/release/build", text)
        self.assertIn("name: Normalize application-only version for the compiled cache key", text)
        self.assertIn('version = "<release>"', text)
        self.assertIn("cargo clean --manifest-path build/Windows/Cargo.toml -p splined-windows --release --target", text)
        self.assertIn("cargo build --locked --release --manifest-path build/Windows/Cargo.toml --target", text)
        self.assertNotIn("Remove-Item -LiteralPath $releaseRoot -Recurse -Force", text)
        self.assertIn("SPLINED_BUILD_COMMIT: ${{ github.sha }}", text)
        self.assertIn('if ($actual.version -ne $version -or $actual.commit -ne $env:EXPECTED_COMMIT', text)
        self.assertIn('if ([BitConverter]::ToUInt16($image, $offset + 4) -ne 0x8664)', text)
        self.assertIn("--native-smoke", text)
        self.assertIn("Get-FileHash -Algorithm SHA256", text)
        self.assertIn("QA-NOT-A-RELEASE.txt", text)

    def test_cold_benchmark_is_isolated_and_opt_in(self):
        script = self.workflow
        self.assertIn("if: ${{ inputs.benchmark_mode == 'Cold and optimized' }}", script)
        self.assertIn('$coldTarget = Join-Path $env:RUNNER_TEMP "splined-cold-qa"', script)
        self.assertIn("--target-dir $coldTarget", script)
        self.assertIn('steps.optimized.outputs.elapsed_seconds', script)
        self.assertIn('steps.cold.outputs.elapsed_seconds', script)
        self.assertIn('steps.rust-cache.outputs.cache-hit', script)

    def test_store_qa_reuses_production_packaging_without_submission(self):
        script = self.workflow
        self.assertIn('if [ "$STORE_UPDATE_QA" = true ] && [ "$EXTENDED" != true ]', script)
        self.assertIn('Build-StorePackage.ps1 @parameters', script)
        self.assertIn('Test-DevelopmentPackage.ps1 @parameters', script)
        self.assertIn('"@microsoft/winappcli@0.7.1"', script)
        self.assertIn("splined-store-unsigned-qa-", script)
        self.assertIn("never submitted to Partner Center", script)

    def test_all_reuses_webgui_validation_and_reports_selected_result(self):
        script = self.workflow
        self.assertIn("if: ${{ inputs.platform == 'WebGUI / Python' || inputs.platform == 'All' }}", script)
        self.assertIn("if: ${{ inputs.platform == 'Windows' || inputs.platform == 'All' }}", script)
        for snippet in (
            "python -m unittest discover -s build/WEBGui/webgui/tests -v",
            "node --check build/WEBGui/webgui/static/app.js",
            "docker build --file build/WEBGui/Dockerfile",
            "postgres:17",
            "python3 .github/scripts/validate_release_policy_contracts.py",
            "needs: [preflight, windows, webgui]",
        ):
            self.assertIn(snippet, script)

    def test_release_workflow_retains_production_validation_path(self):
        source = self.release
        self.assertIn("name: 01. Full Release - Build Selected Target and Publish", source)
        self.assertIn("git bundle create release-source.bundle", source)
        self.assertIn("cargo build --locked --release --manifest-path build/Windows/Cargo.toml", source)
        self.assertIn("Publish GitHub Release", source)
        self.assertNotIn("splined-optimized-qa-", source)


if __name__ == "__main__":
    unittest.main()
