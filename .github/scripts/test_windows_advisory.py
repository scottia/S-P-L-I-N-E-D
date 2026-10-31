from pathlib import Path
import subprocess
import tempfile
import unittest

from windows_advisory import (
    COMMANDS,
    classify,
    overall_outcome,
    render_summary,
    run_advisory,
)


REPOSITORY = Path(__file__).resolve().parents[2]


class WindowsAdvisoryTests(unittest.TestCase):
    def test_outcomes_distinguish_pass_warnings_and_failure(self):
        formatting = next(item for item in COMMANDS if item.key == "formatting")
        rust_slint = next(item for item in COMMANDS if item.key == "rust_slint")
        clippy = next(item for item in COMMANDS if item.key == "clippy")
        self.assertEqual(classify(formatting, 1, "Diff in main.rs"), ("Warnings", 0))
        self.assertEqual(classify(formatting, 1, "error: parse failed"), ("Failed", 0))
        self.assertEqual(classify(rust_slint, 101, "error: build failed"), ("Failed", 0))
        self.assertEqual(classify(clippy, 0, "warning: needless borrow"), ("Warnings", 1))
        self.assertEqual(classify(clippy, 0, "Finished"), ("Passed", 0))

    def test_diagnostic_failures_do_not_stop_later_commands(self):
        completed = [
            subprocess.CompletedProcess([], 0, "toolchain", ""),
            subprocess.CompletedProcess([], 0, "components", ""),
            subprocess.CompletedProcess([], 1, "format diff", ""),
            subprocess.CompletedProcess([], 101, "", "error: Slint failed"),
            subprocess.CompletedProcess([], 0, "", "warning: clippy finding"),
        ]
        calls = []

        def runner(command, **_kwargs):
            calls.append(command)
            return completed[len(calls) - 1]

        results = run_advisory(runner=runner, repository=REPOSITORY)
        self.assertEqual(len(calls), len(COMMANDS))
        self.assertEqual(
            [item.outcome for item in results],
            ["Passed", "Passed", "Warnings", "Failed", "Warnings"],
        )
        self.assertEqual(overall_outcome(results), "Failed")

    def test_failed_setup_marks_dependent_commands_not_run(self):
        def runner(_command, **_kwargs):
            return subprocess.CompletedProcess([], 1, "", "toolchain unavailable")

        results = run_advisory(runner=runner, repository=REPOSITORY)
        self.assertEqual(results[0].outcome, "Failed")
        self.assertTrue(all(item.outcome == "Not run" for item in results[1:]))

    def test_summary_reports_revision_commands_exits_and_logs(self):
        completed = iter(
            subprocess.CompletedProcess([], 0, "ok", "") for _ in COMMANDS
        )
        results = run_advisory(
            runner=lambda _command, **_kwargs: next(completed),
            repository=REPOSITORY,
        )
        summary = render_summary(
            results,
            revision="a" * 40,
            run_url="https://github.example/actions/runs/1",
        )
        self.assertIn("Windows Rust/Slint — advisory", summary)
        self.assertIn("`" + "a" * 40 + "`", summary)
        self.assertIn("Exit result", summary)
        self.assertIn("cargo +1.99.0 clippy", summary)
        self.assertIn("[workflow run](https://github.example/actions/runs/1)", summary)
        self.assertIn("tolerated", summary)

    def test_pr_workflow_is_advisory_without_windows_tests_or_warning_escalation(self):
        workflow = (REPOSITORY / ".github/workflows/validate.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("Windows Rust/Slint — advisory", workflow)
        self.assertIn("continue-on-error: true", workflow)
        self.assertIn("windows_advisory.py", workflow)
        self.assertNotIn("-D warnings", workflow)
        self.assertNotIn(
            "cargo +${{ matrix.toolchain }} test --manifest-path windows/Cargo.toml",
            workflow,
        )

    def test_release_workflow_retains_strict_extended_windows_validation(self):
        workflow = (REPOSITORY / ".github/workflows/01-full-release.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("cargo clippy", workflow)
        self.assertIn("-D warnings", workflow)
        self.assertIn("cargo test --locked --manifest-path windows/Cargo.toml", workflow)


if __name__ == "__main__":
    unittest.main()
