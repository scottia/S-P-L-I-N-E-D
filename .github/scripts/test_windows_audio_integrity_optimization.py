"""Structural regression checks for optimized Windows writer validation."""

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "validate.yml"


class WindowsAudioIntegrityOptimizationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.job = workflow.split("  audio-integrity-windows:\n", 1)[1].split(
            "  scope:\n", 1
        )[0]

    def test_keeps_pinned_locked_real_writer_test_contract(self) -> None:
        self.assertIn("rustup toolchain install 1.99.0", self.job)
        self.assertIn("--locked `", self.job)
        self.assertIn("--target x86_64-pc-windows-msvc", self.job)
        for test_filter in (
            '"artwork_integrity::tests"',
            '"embedded_artwork"',
            '"local_artwork"',
        ):
            self.assertIn(test_filter, self.job)

    def test_compiles_once_then_executes_three_filters_directly(self) -> None:
        self.assertEqual(self.job.count("cargo +1.99.0 test"), 1)
        self.assertIn("--no-run `", self.job)
        self.assertIn("--message-format=json-render-diagnostics", self.job)
        self.assertIn("& $testExecutable $filter --nocapture", self.job)
        self.assertIn("Expected exactly one compiled splined_core test executable", self.job)

    def test_restores_version_normalized_dependencies_and_intermediates(self) -> None:
        self.assertIn('version = "<release>"', self.job)
        self.assertIn("~/.cargo/registry", self.job)
        self.assertIn("~/.cargo/git", self.job)
        self.assertIn("debug/deps", self.job)
        self.assertIn("!build/Windows/target/x86_64-pc-windows-msvc/debug/deps/*.exe", self.job)
        self.assertIn("!build/Windows/target/x86_64-pc-windows-msvc/debug/deps/*.pdb", self.job)
        self.assertNotIn("target/x86_64-pc-windows-msvc/debug/splined.exe", self.job)

    def test_reports_cache_and_elapsed_time(self) -> None:
        self.assertIn("Cargo dependency cache exact hit", self.job)
        self.assertIn("Compiled-intermediate cache exact hit", self.job)
        self.assertIn("Compile seconds", self.job)
        self.assertIn("Test seconds", self.job)
        self.assertIn("Total compile/test seconds", self.job)


if __name__ == "__main__":
    unittest.main()
