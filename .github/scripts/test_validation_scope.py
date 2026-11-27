"""Regression tests for cross-client pull-request validation selection."""

import unittest

from pathlib import Path

from validation_scope import (
    VALIDATION_GATE_PATHS,
    VERSION_METADATA_PATHS,
    is_release_metadata_only,
    select_checks,
)


ROOT = Path(__file__).resolve().parents[2]


class ValidationScopeTests(unittest.TestCase):
    def test_windows_runtime_runs_webgui_validation(self):
        self.assertEqual(
            select_checks(["build/Windows/src/main.rs"]),
            {"webgui": True},
        )

    def test_webgui_runtime_runs_webgui_validation(self):
        self.assertEqual(
            select_checks(["build/WEBGui/webgui/catalog.py"]),
            {"webgui": True},
        )

    def test_shared_contract_runs_webgui_validation(self):
        self.assertEqual(
            select_checks(["schema/media.sql"]),
            {"webgui": True},
        )

    def test_validation_gate_changes_run_webgui_validation(self):
        for path in VALIDATION_GATE_PATHS:
            with self.subTest(path=path):
                self.assertEqual(
                    select_checks([path]),
                    {"webgui": True},
                )

    def test_documentation_only_change_remains_non_runtime(self):
        self.assertEqual(
            select_checks(["docs/POLICY-INDEX.md"]),
            {"webgui": False},
        )

    def test_unknown_diff_fails_open_to_full_webgui_validation(self):
        self.assertEqual(
            select_checks(["__all__"]),
            {"webgui": True},
        )

    def test_synchronized_release_metadata_runs_webgui_validation(self):
        old = "3.0.25"
        new = "3.0.26"
        mutations = [
            f'-USER_AGENT = "SPLINED/{old} (https://github.com/scottia/S-P-L-I-N-E-D)"',
            f'+USER_AGENT = "SPLINED/{new} (https://github.com/scottia/S-P-L-I-N-E-D)"',
            f'-VERSION = "{old}"', f'+VERSION = "{new}"',
            f'-version = "{old}"', f'+version = "{new}"',
            f'-version = "{old}"', f'+version = "{new}"',
            f'-  <assemblyIdentity version="{old}.0" name="SPLINED.Windows"/>',
            f'+  <assemblyIdentity version="{new}.0" name="SPLINED.Windows"/>',
            f'-    Version="{old}.0"', f'+    Version="{new}.0"',
            f'-  Version="{old}.0"', f'+  Version="{new}.0"',
            f'-    Version="{old}.0"', f'+    Version="{new}.0"',
        ]
        self.assertTrue(is_release_metadata_only(VERSION_METADATA_PATHS, "\n".join(mutations)))
        self.assertEqual(
            select_checks(VERSION_METADATA_PATHS, release_metadata_only=True),
            {"webgui": True},
        )

    def test_release_metadata_recognizer_rejects_additional_runtime_change(self):
        self.assertFalse(is_release_metadata_only(
            [*VERSION_METADATA_PATHS, "build/Windows/src/main.rs"],
            '-version = "3.0.25"\n+version = "3.0.26"',
        ))

    def test_release_metadata_recognizer_rejects_mixed_versions(self):
        mutations = self.release_mutations("3.0.25", "3.0.26")
        mutations[-1] = mutations[-1].replace("3.0.26", "3.0.27")
        self.assertFalse(is_release_metadata_only(VERSION_METADATA_PATHS, "\n".join(mutations)))

    def test_release_metadata_recognizer_rejects_same_version(self):
        self.assertFalse(is_release_metadata_only(
            VERSION_METADATA_PATHS,
            "\n".join(self.release_mutations("3.0.26", "3.0.26")),
        ))

    def test_workflow_jobs_use_scope_outputs(self):
        workflow = (ROOT / ".github/workflows/validate.yml").read_text(encoding="utf-8")
        webgui = workflow.split("  webgui:\n", 1)[1].split("  windows-advisory:\n", 1)[0]
        self.assertIn("needs.scope.outputs.webgui == 'true'", webgui)
        self.assertNotIn("audio_webgui", workflow)
        self.assertNotIn("audio_windows", workflow)

    @staticmethod
    def release_mutations(old, new):
        return [
            f'-USER_AGENT = "SPLINED/{old} (https://github.com/scottia/S-P-L-I-N-E-D)"',
            f'+USER_AGENT = "SPLINED/{new} (https://github.com/scottia/S-P-L-I-N-E-D)"',
            f'-VERSION = "{old}"', f'+VERSION = "{new}"',
            f'-version = "{old}"', f'+version = "{new}"',
            f'-version = "{old}"', f'+version = "{new}"',
            f'-  <assemblyIdentity version="{old}.0" name="SPLINED.Windows"/>',
            f'+  <assemblyIdentity version="{new}.0" name="SPLINED.Windows"/>',
            f'-    Version="{old}.0"', f'+    Version="{new}.0"',
            f'-  Version="{old}.0"', f'+  Version="{new}.0"',
            f'-    Version="{old}.0"', f'+    Version="{new}.0"',
        ]


if __name__ == "__main__":
    unittest.main()
