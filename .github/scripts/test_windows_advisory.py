from pathlib import Path
import tempfile
import unittest

from windows_advisory import POLICY_OUTCOME, append_summary, render_summary


REPOSITORY = Path(__file__).resolve().parents[2]


class WindowsAdvisoryTests(unittest.TestCase):
    def test_summary_reports_permanent_exception_without_claiming_pass(self):
        summary = render_summary(
            revision="a" * 40,
            run_url="https://github.example/actions/runs/1",
        )
        self.assertIn("Windows Rust/Slint — advisory", summary)
        self.assertIn(POLICY_OUTCOME, summary)
        self.assertIn("`" + "a" * 40 + "`", summary)
        self.assertIn("[workflow run](https://github.example/actions/runs/1)", summary)
        self.assertIn("No Windows runner", summary)
        self.assertNotIn("Passed", summary)

    def test_summary_file_is_appended_without_executing_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "summary.md"
            append_summary("policy report\n", str(destination))
            self.assertEqual(destination.read_text(encoding="utf-8"), "policy report\n")

        script = (REPOSITORY / ".github/scripts/windows_advisory.py").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("subprocess", script)
        self.assertNotIn("rustup", script)
        self.assertNotIn('("cargo",', script)

    def test_pr_workflow_keeps_lightweight_ubuntu_compatibility_check(self):
        workflow = (REPOSITORY / ".github/workflows/validate.yml").read_text(
            encoding="utf-8"
        )
        advisory = workflow.split("  windows-advisory:", 1)[1]
        self.assertIn("name: Windows Rust/Slint — advisory", advisory)
        self.assertIn("runs-on: ubuntu-latest", advisory)
        self.assertIn("continue-on-error: true", advisory)
        self.assertIn("Report permanent PR policy exception", advisory)
        self.assertIn("windows_advisory.py", advisory)
        self.assertNotIn("windows-latest", advisory)
        self.assertNotIn("setup-python", advisory)
        self.assertNotIn("needs.scope.outputs.windows_advisory", advisory)

    def test_official_windows_release_retains_cached_exact_source_and_msix_validation(self):
        workflow = (REPOSITORY / ".github/workflows/01-build-all-release.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("compile_mode=FULL SOURCE", workflow)
        self.assertNotIn("cargo clean --manifest-path build/Windows/Cargo.toml --release", workflow)
        self.assertIn("splined-wf20-incremental-compiled-v3", workflow)
        self.assertIn("dev_cache_sha", workflow)
        self.assertIn(
            "cargo build --locked --release --manifest-path build/Windows/Cargo.toml",
            workflow,
        )
        self.assertIn("Test-DevelopmentPackage.ps1", workflow)
        self.assertIn("New-LocalStoreReviewBundle.ps1", workflow)


if __name__ == "__main__":
    unittest.main()
