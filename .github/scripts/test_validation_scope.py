import unittest

from validation_scope import select_checks


NO_RUNTIME = {"webgui": False, "linux": False}


class ValidationScopeTests(unittest.TestCase):
    def test_webgui_changes_run_webgui_without_windows_diagnostics(self):
        for path in [
            "webgui/static/app.css",
            "webgui/server.py",
            "webgui/requirements.txt",
            "docker/compose.yaml",
        ]:
            with self.subTest(path=path):
                self.assertEqual(
                    select_checks([path]),
                    {"webgui": True, "linux": False},
                )

    def test_windows_source_slint_and_build_inputs_run_no_strict_pr_lane(self):
        for path in [
            "windows/src/main.rs",
            "windows/ui/app.slint",
            "windows/Cargo.lock",
            "windows/Cargo.toml",
            "windows/build.rs",
            "windows/gui/app.ico",
            "windows/gui/app.manifest",
        ]:
            with self.subTest(path=path):
                self.assertEqual(select_checks([path]), NO_RUNTIME)

    def test_dependency_updates_never_select_windows_diagnostics(self):
        cases = {
            "Cargo.lock": {"webgui": False, "linux": True},
            "windows/Cargo.lock": NO_RUNTIME,
            "webgui/requirements.txt": {
                "webgui": True,
                "linux": False,
            },
            ".github/dependabot.yml": {
                "webgui": True,
                "linux": True,
            },
        }
        for path, expected in cases.items():
            with self.subTest(path=path):
                self.assertEqual(select_checks([path]), expected)

    def test_shared_schema_and_validation_changes_keep_other_runtime_checks(self):
        for path in [
            "schema/media.sql",
            "fixtures/legacy.spl",
            ".gitattributes",
            ".github/workflows/validate.yml",
            ".github/scripts/validation_scope.py",
        ]:
            with self.subTest(path=path):
                self.assertEqual(
                    select_checks([path]),
                    {"webgui": True, "linux": True},
                )

    def test_docs_naming_and_display_only_changes_do_not_build_runtimes(self):
        examples = [
            [],
            ["README.md", "docs/browser-workspace-guide.md", "windows/README.md"],
            ["windows/scripts/Install-LocalTestBuild.ps1"],
            ["windows/package/README-WINDOWS.txt"],
            ["windows/config.example.toml"],
            ["windows/gui/imagination-message.svg"],
            ["assets/branding/splined-slogan.svg"],
        ]
        for paths in examples:
            with self.subTest(paths=paths):
                self.assertEqual(select_checks(paths), NO_RUNTIME)

    def test_root_config_example_runs_webgui_and_linux_only(self):
        self.assertEqual(
            select_checks(["config.example.toml"]),
            {"webgui": True, "linux": True},
        )

    def test_cross_runtime_and_promotion_diffs_never_select_windows_diagnostics(self):
        self.assertEqual(
            select_checks(["windows/src/old.rs", "webgui/new.py"]),
            {"webgui": True, "linux": False},
        )
        self.assertEqual(
            select_checks(["windows/src/main.rs", "Cargo.lock", "webgui/server.py"]),
            {"webgui": True, "linux": True},
        )

    def test_unknown_diff_fails_safe_without_breaking_permanent_exception(self):
        self.assertEqual(
            select_checks(["__all__"]),
            {"webgui": True, "linux": True},
        )


if __name__ == "__main__":
    unittest.main()
