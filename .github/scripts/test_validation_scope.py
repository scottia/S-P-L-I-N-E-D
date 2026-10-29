import unittest

from validation_scope import select_checks


class ValidationScopeTests(unittest.TestCase):
    def test_webgui_changes_skip_both_cargo_builds(self):
        for path in ["webgui/static/app.css", "webgui/server.py", "webgui/requirements.txt", "docker/compose.yaml"]:
            with self.subTest(path=path):
                self.assertEqual(select_checks([path]), {"webgui": True, "linux": False, "windows": False})

    def test_windows_source_and_lockfile_select_windows(self):
        for path in ["windows/src/main.rs", "windows/ui/app.slint", "windows/Cargo.lock"]:
            with self.subTest(path=path):
                self.assertEqual(select_checks([path]), {"webgui": False, "linux": False, "windows": True})

    def test_root_security_lockfile_selects_root_rust(self):
        self.assertEqual(select_checks(["Cargo.lock"]), {"webgui": False, "linux": True, "windows": False})

    def test_shared_files_and_validation_changes_select_all(self):
        for path in ["schema/media.sql", "fixtures/legacy.spl", ".gitattributes", ".github/workflows/validate.yml", ".github/scripts/validation_scope.py"]:
            with self.subTest(path=path):
                self.assertTrue(all(select_checks([path]).values()))

    def test_docs_and_empty_changes_do_not_build_runtimes(self):
        for paths in [[], ["README.md", "docs/browser-workspace-guide.md", "windows/README.md"]]:
            self.assertFalse(any(select_checks(paths).values()))

    def test_cross_runtime_rename_checks_both_sides(self):
        self.assertEqual(select_checks(["windows/old.py", "webgui/new.py"]), {"webgui": True, "linux": False, "windows": True})

    def test_unknown_diff_fails_safe(self):
        self.assertTrue(all(select_checks(["__all__"]).values()))


if __name__ == "__main__":
    unittest.main()
