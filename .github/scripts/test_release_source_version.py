"""Tests for repository-authoritative release version selection."""
from pathlib import Path
import shutil
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / ".github" / "scripts"))

from release_source_version import choose_release_version
from source_version import (
    current_source_version,
    next_patch_version,
    source_versions,
    stamp_source_version,
)


class ReleaseSourceVersionTests(unittest.TestCase):
    def test_current_windows_webgui_and_workflow_sources_match(self) -> None:
        versions = source_versions(ROOT)
        self.assertEqual(len(set(versions.values())), 1, versions)
        self.assertEqual(current_source_version(ROOT), next(iter(versions.values())))

    def test_source_preparation_advances_one_patch_and_stamps_every_field(self) -> None:
        current = current_source_version(ROOT)
        major, minor, patch = map(int, current.split("."))
        next_version = f"{major}.{minor}.{patch + 1}"
        relative_paths = (
            "build/Windows/Cargo.toml",
            "build/Windows/Cargo.lock",
            "build/Windows/gui/app.manifest",
            "build/Windows/package/Package.appxmanifest",
            "build/Windows/package/SPLINED.appinstaller.template",
            "build/WEBGui/webgui/backend/splined.py",
            ".github/workflows/04-optimized-build-test.yml",
        )
        with tempfile.TemporaryDirectory() as temporary:
            test_root = Path(temporary)
            for relative in relative_paths:
                destination = test_root / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / relative, destination)
            stamp_source_version(test_root, next_version)
            self.assertEqual(current_source_version(test_root), next_version)
            with self.assertRaisesRegex(ValueError, "advance exactly one patch"):
                stamp_source_version(test_root, f"{major}.{minor}.{patch + 3}")

    def test_next_source_version_requires_latest_tag_to_match_source(self) -> None:
        current = current_source_version(ROOT)
        major, minor, patch = map(int, current.split("."))
        self.assertEqual(
            next_patch_version(current, [f"v{current}", "not-a-version"]),
            f"{major}.{minor}.{patch + 1}",
        )
        with self.assertRaisesRegex(ValueError, "latest main-history tag"):
            next_patch_version(current, [f"{major}.{minor}.{max(0, patch - 1)}"])

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
