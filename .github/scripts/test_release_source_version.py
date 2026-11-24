"""Tests for repository-authoritative release version selection."""
from pathlib import Path
import shutil
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / ".github" / "scripts"))

from release_source_version import (
    choose_release_version,
    choose_workflow10_release_version,
)
from source_version import (
    current_source_version,
    next_patch_version,
    release_product_digest,
    source_versions,
    stamp_release_version,
    stamp_source_version,
)


class ReleaseSourceVersionTests(unittest.TestCase):
    def test_current_windows_and_webgui_sources_match(self) -> None:
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

    def test_temporary_release_stamp_can_follow_newer_release_tags(self) -> None:
        current = current_source_version(ROOT)
        major, minor, patch = map(int, current.split("."))
        release_version = f"{major}.{minor}.{patch + 2}"
        relative_paths = (
            "build/Windows/Cargo.toml",
            "build/Windows/Cargo.lock",
            "build/Windows/gui/app.manifest",
            "build/Windows/package/Package.appxmanifest",
            "build/Windows/package/SPLINED.appinstaller.template",
            "build/WEBGui/webgui/backend/splined.py",
        )
        with tempfile.TemporaryDirectory() as temporary:
            source_root = Path(temporary) / "source"
            stamped_root = Path(temporary) / "stamped"
            for test_root in (source_root, stamped_root):
                for relative in relative_paths:
                    destination = test_root / relative
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(ROOT / relative, destination)
                (test_root / "schema").mkdir(parents=True)
                (test_root / "schema" / "sentinel.json").write_text("{}\n")
            stamp_release_version(stamped_root, release_version)
            self.assertEqual(current_source_version(stamped_root), release_version)
            self.assertEqual(
                release_product_digest(source_root),
                release_product_digest(stamped_root),
            )
            with (stamped_root / "schema" / "sentinel.json").open("a") as handle:
                handle.write("changed\n")
            self.assertNotEqual(
                release_product_digest(source_root),
                release_product_digest(stamped_root),
            )

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

    def test_workflow10_uses_committed_next_version_when_available(self) -> None:
        self.assertEqual(
            choose_workflow10_release_version(
                "3.0.27", [(3, 0, 26)], ["3.0.26"], False
            ),
            ("3.0.27", False),
        )

    def test_workflow10_advances_changed_source_after_published_version(self) -> None:
        self.assertEqual(
            choose_workflow10_release_version(
                "3.0.26", [(3, 0, 26)], ["3.0.26"], False
            ),
            ("3.0.27", True),
        )
        self.assertEqual(
            choose_workflow10_release_version(
                "3.0.26", [(3, 0, 27)], ["3.0.27"], False
            ),
            ("3.0.28", True),
        )

    def test_workflow10_reuses_matching_latest_release(self) -> None:
        self.assertEqual(
            choose_workflow10_release_version(
                "3.0.26", [(3, 0, 27)], ["3.0.27"], True
            ),
            ("3.0.27", True),
        )

    def test_release_workflows_call_shared_resolver(self) -> None:
        release = (ROOT / ".github/workflows/01-full-release.yml").read_text(encoding="utf-8")
        self.assertIn("from release_source_version import choose_release_version", release)
        self.assertIn("git commit -m \"release: prepare ${RELEASE_VERSION}\" --allow-empty", release)
        unified = (ROOT / ".github/workflows/10-unified-build.yml").read_text(encoding="utf-8")
        self.assertIn("choose_workflow10_release_version", unified)
        self.assertIn("stamp-release", unified)
        self.assertNotIn('$RELEASE_COMMIT:refs/heads/main', unified)


if __name__ == "__main__":
    unittest.main()
