"""Tests for repository-authoritative release version selection."""
from pathlib import Path
import sys
import unittest


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / ".github" / "scripts"))

from release_source_version import choose_release_version


class ReleaseSourceVersionTests(unittest.TestCase):
    def test_current_windows_and_webgui_sources_are_3022(self) -> None:
        cargo = (ROOT / "build/Windows/Cargo.toml").read_text(encoding="utf-8-sig")
        lock = (ROOT / "build/Windows/Cargo.lock").read_text(encoding="utf-8")
        executable_manifest = (ROOT / "build/Windows/gui/app.manifest").read_text(
            encoding="utf-8-sig"
        )
        package_manifest = (
            ROOT / "build/Windows/package/Package.appxmanifest"
        ).read_text(encoding="utf-8-sig")
        appinstaller = (
            ROOT / "build/Windows/package/SPLINED.appinstaller.template"
        ).read_text(encoding="utf-8-sig")
        webgui = (ROOT / "build/WEBGui/webgui/backend/splined.py").read_text(
            encoding="utf-8"
        )
        self.assertRegex(cargo, r'(?m)^version = "3\.0\.22"$')
        self.assertRegex(
            lock,
            r'(?m)^name = "splined-windows"\nversion = "3\.0\.22"$',
        )
        self.assertIn('version="3.0.22.0" name="SPLINED.Windows"', executable_manifest)
        self.assertIn('Version="3.0.22.0"', package_manifest)
        self.assertEqual(appinstaller.count('Version="3.0.22.0"'), 2)
        self.assertRegex(webgui, r'(?m)^VERSION = "3\.0\.22"$')
        self.assertRegex(webgui, r'(?m)^USER_AGENT = "SPLINED/3\.0\.22 ')

    def test_blank_or_exact_request_uses_committed_source(self) -> None:
        active = [(3, 0, 21)]
        tags = ["3.0.21"]
        self.assertEqual(choose_release_version("3.0.22", active, tags), "3.0.22")
        self.assertEqual(
            choose_release_version("3.0.22", active, tags, "3.0.22"),
            "3.0.22",
        )

    def test_different_requested_version_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "must equal the committed"):
            choose_release_version("3.0.22", [(3, 0, 21)], ["3.0.21"], "3.0.23")

    def test_source_must_be_ahead_and_tag_name_unused(self) -> None:
        with self.assertRaisesRegex(ValueError, "must exceed"):
            choose_release_version("3.0.21", [(3, 0, 21)], ["3.0.21"])
        with self.assertRaisesRegex(ValueError, "already exists"):
            choose_release_version("3.0.22", [(3, 0, 21)], ["v3.0.22"])

    def test_release_workflows_call_shared_resolver(self) -> None:
        for name in ("01-full-release.yml", "04-optimized-build-test.yml"):
            workflow = (ROOT / ".github/workflows" / name).read_text(encoding="utf-8")
            self.assertIn("from release_source_version import choose_release_version", workflow)
            self.assertIn("git commit -m \"release: prepare ${RELEASE_VERSION}\" --allow-empty", workflow)
            self.assertNotIn("Automatic release version must exceed", workflow)


if __name__ == "__main__":
    unittest.main()
