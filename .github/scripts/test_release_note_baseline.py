"""Regression tests for release-note lineage across temporary stamp commits."""

from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / ".github" / "scripts"))

from release_note_baseline import resolve_release_note_baseline
from source_version import stamp_release_version


VERSION_FILES = (
    "build/Windows/Cargo.toml",
    "build/Windows/Cargo.lock",
    "build/Windows/gui/app.manifest",
    "build/Windows/package/Package.appxmanifest",
    "build/Windows/package/SPLINED.appinstaller.template",
    "build/WEBGui/webgui/backend/splined.py",
)


class ReleaseNoteBaselineTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.repository = Path(self.temporary.name)
        self._git("init", "-b", "main")
        self._git("config", "user.name", "SPLINED Test")
        self._git("config", "user.email", "splined@example.invalid")
        for relative in VERSION_FILES:
            destination = self.repository / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, destination)
        sentinel = self.repository / "schema/sentinel.json"
        sentinel.parent.mkdir(parents=True, exist_ok=True)
        sentinel.write_text('{"revision": 0}\n', encoding="utf-8")
        self.base = self._commit("base source")
        self._git("tag", "3.0.27")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def _git(self, *args: str) -> str:
        return subprocess.check_output(
            ["git", "-C", str(self.repository), *args], text=True
        ).strip()

    def _commit(self, message: str) -> str:
        self._git("add", "-A")
        self._git("commit", "-m", message)
        return self._git("rev-parse", "HEAD")

    def _source_commit(self, revision: int) -> str:
        (self.repository / "schema/sentinel.json").write_text(
            f'{{"revision": {revision}}}\n', encoding="utf-8"
        )
        return self._commit(f"source {revision}")

    def _stamp(self, version: str, tag: str | None = None) -> str:
        stamp_release_version(self.repository, version)
        commit = self._commit(f"release {version}")
        self._git("tag", tag or version)
        return commit

    def test_direct_previous_tag_remains_the_range_baseline(self) -> None:
        self._source_commit(1)
        self._stamp("3.0.28")
        result = resolve_release_note_baseline(self.repository, "3.0.28")
        self.assertEqual(result.previous_tag, "3.0.27")
        self.assertEqual(result.baseline_commit, self.base)
        self.assertEqual(result.range, "3.0.27..3.0.28")
        self.assertEqual(result.mode, "direct-tag-ancestor")

    def test_previous_temporary_stamp_uses_its_source_parent(self) -> None:
        source_one = self._source_commit(1)
        self._git("switch", "-c", "release-28")
        self._stamp("3.0.28")
        self._git("switch", "-c", "dev", source_one)
        self._source_commit(2)
        self._stamp("3.0.29")

        result = resolve_release_note_baseline(self.repository, "3.0.29")
        self.assertEqual(result.previous_tag, "3.0.28")
        self.assertEqual(result.baseline_commit, source_one)
        self.assertEqual(result.range, f"{source_one}..3.0.29")
        self.assertEqual(result.mode, "temporary-stamp-parent")

    def test_malformed_temporary_stamp_fails_closed(self) -> None:
        source_one = self._source_commit(1)
        self._git("switch", "-c", "bad-release")
        stamp_release_version(self.repository, "3.0.28")
        (self.repository / "schema/sentinel.json").write_text(
            '{"revision": 999}\n', encoding="utf-8"
        )
        self._commit("malformed release 3.0.28")
        self._git("tag", "3.0.28")
        self._git("switch", "-c", "current", source_one)
        self._source_commit(2)
        self._stamp("3.0.29")

        with self.assertRaisesRegex(ValueError, "not a valid temporary version stamp"):
            resolve_release_note_baseline(self.repository, "3.0.29")

    def test_unrelated_previous_numeric_tag_fails_closed(self) -> None:
        source_one = self._source_commit(1)
        self._git("switch", "--orphan", "unrelated")
        (self.repository / "unrelated.txt").write_text("unrelated\n", encoding="utf-8")
        self._commit("unrelated root")
        self._git("tag", "3.0.28")
        self._git("switch", "-c", "current", source_one)
        self._source_commit(2)
        self._stamp("3.0.29")

        with self.assertRaisesRegex(ValueError, "not a single-parent temporary version stamp"):
            resolve_release_note_baseline(self.repository, "3.0.29")


if __name__ == "__main__":
    unittest.main()
