import unittest

from validation_scope import select_checks


class ValidationScopeTests(unittest.TestCase):
    def test_webgui_changes_skip_both_cargo_builds(self):
        for path in ["webgui/static/app.css", "webgui/server.py", "webgui/requirements.txt", "docker/compose.yaml"]:
            with self.subTest(path=path):
                self.assertEqual(
                    select_checks([path]),
                    {"webgui": True, "linux": False, "windows_advisory": False},
                )

    def test_windows_source_slint_and_build_inputs_select_advisory(self):
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
                self.assertEqual(
                    select_checks([path]),
                    {"webgui": False, "linux": False, "windows_advisory": True},
                )

    def test_root_security_lockfile_selects_root_rust(self):
        self.assertEqual(
            select_checks(["Cargo.lock"]),
            {"webgui": False, "linux": True, "windows_advisory": False},
        )

    def test_shared_files_and_validation_changes_select_all(self):
        for path in ["schema/media.sql", "fixtures/legacy.spl", ".gitattributes", ".github/workflows/validate.yml", ".github/scripts/validation_scope.py"]:
            with self.subTest(path=path):
                self.assertTrue(all(select_checks([path]).values()))

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
                self.assertFalse(any(select_checks(paths).values()))

    def test_root_config_example_skips_windows_compilation(self):
        self.assertEqual(
            select_checks(["config.example.toml"]),
            {"webgui": True, "linux": True, "windows_advisory": False},
        )

    def test_cross_runtime_rename_checks_both_sides(self):
        self.assertEqual(
            select_checks(["windows/src/old.rs", "webgui/new.py"]),
            {"webgui": True, "linux": False, "windows_advisory": True},
        )

    def test_unknown_diff_fails_safe(self):
        self.assertTrue(all(select_checks(["__all__"]).values()))


if __name__ == "__main__":
    unittest.main()
