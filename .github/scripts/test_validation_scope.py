"""Regression tests for cross-client pull-request validation selection."""

import unittest

from validation_scope import select_checks


class ValidationScopeTests(unittest.TestCase):
    def test_windows_source_runs_full_webgui_validation(self):
        self.assertEqual(
            select_checks(["build/Windows/src/main.rs"]),
            {"webgui": True},
        )

    def test_webgui_and_shared_contract_changes_run_webgui_validation(self):
        for path in (
            "build/WEBGui/webgui/catalog.py",
            "schema/media.sql",
            ".github/workflows/validate.yml",
        ):
            with self.subTest(path=path):
                self.assertEqual(select_checks([path]), {"webgui": True})

    def test_documentation_only_change_remains_non_runtime(self):
        self.assertEqual(select_checks(["docs/POLICY-INDEX.md"]), {"webgui": False})

    def test_unknown_diff_fails_open_to_full_webgui_validation(self):
        self.assertEqual(select_checks(["__all__"]), {"webgui": True})


if __name__ == "__main__":
    unittest.main()
