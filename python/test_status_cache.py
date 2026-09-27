from __future__ import annotations

import os
import tempfile
import types
import unittest
from dataclasses import dataclass, field
from pathlib import Path
from unittest import mock

import splined_status_cache as cachemod


@dataclass
class FakeAlbumDir:
    path: Path
    audio_files: list[Path]
    local_art_files: list[Path] = field(default_factory=list)
    inventory_fingerprint: str | None = None


class PersistentStatusCacheTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.base = Path(self.temp.name)
        self.library = self.base / "music"
        self.history = self.base / "history"
        self.artist = self.library / "Artist"
        self.album = self.artist / "Album"
        self.album.mkdir(parents=True)
        self.track = self.album / "track.flac"
        self.cover = self.album / "cover.jpg"
        self.track.write_bytes(b"audio")
        self.cover.write_bytes(b"art")
        with cachemod._LOCK:
            cachemod._PAYLOAD = None
            cachemod._CACHE_PATH = None
            cachemod._DIRTY = 0
            cachemod._LAST_SAVE = 0.0
            cachemod._CURRENT_BASELINE = 0
            cachemod._CURRENT_TOTAL = 0
            cachemod._CURRENT_FIRST_RUN = False

    def tearDown(self) -> None:
        with cachemod._LOCK:
            cachemod._PAYLOAD = None
            cachemod._CACHE_PATH = None
            cachemod._DIRTY = 0
            cachemod._CURRENT_BASELINE = 0
            cachemod._CURRENT_TOTAL = 0
            cachemod._CURRENT_FIRST_RUN = False
        self.temp.cleanup()

    def _core(self):
        calls: list[Path] = []
        events: list[tuple[str, dict[str, object]]] = []

        def inventory(root, _ignored, _name="cover", **_kwargs):
            calls.append(Path(root))
            return [
                FakeAlbumDir(
                    self.album,
                    [self.track],
                    [self.cover],
                    None,
                )
            ], []

        def fingerprint(values):
            return "|".join(
                f"{path.name}:{size}:{modified}"
                for path, size, modified in values
            )

        core = types.SimpleNamespace(
            inventory=inventory,
            emit_ui=lambda event, **payload: events.append((event, payload)),
            AlbumDir=FakeAlbumDir,
            _fingerprint_from_metadata=fingerprint,
            debug_log=lambda _message: None,
        )
        return core, calls, events

    def test_cache_reuses_unchanged_artist_and_rescans_changed_album(self) -> None:
        core, calls, _events = self._core()
        with mock.patch.object(
            cachemod,
            "_runtime_settings",
            return_value=(self.history, self.library, [], "cover"),
        ):
            cachemod.install_core_patch(core)
            first, _ = core.inventory(
                self.artist,
                [],
                "cover",
                workers=1,
            )
            self.assertEqual(len(first), 1)
            self.assertEqual(calls, [self.artist])
            cachemod.flush()
            history_file = self.history / cachemod.CACHE_FILE
            self.assertTrue(history_file.is_file())
            self.assertFalse((self.base / "cache" / cachemod.CACHE_FILE).exists())

            second, _ = core.inventory(
                self.artist,
                [],
                "cover",
                workers=1,
            )
            self.assertEqual(len(second), 1)
            self.assertEqual(calls, [self.artist])

            stat = self.album.stat()
            os.utime(
                self.album,
                ns=(stat.st_atime_ns, stat.st_mtime_ns + 1_000_000),
            )
            third, _ = core.inventory(
                self.artist,
                [],
                "cover",
                workers=1,
            )
            self.assertEqual(len(third), 1)
            self.assertEqual(calls, [self.artist, self.artist])

    def test_complete_cache_starts_at_full_coverage_but_waits_for_validation(self) -> None:
        core, _calls, events = self._core()
        with mock.patch.object(
            cachemod,
            "_runtime_settings",
            return_value=(self.history, self.library, [], "cover"),
        ):
            cachemod.install_core_patch(core)
            core.inventory(self.artist, [], "cover", workers=1)
            cachemod.flush()
            core.emit_ui(
                "folder_status_progress",
                processed=0,
                total=1,
                percent=0.0,
                albums=0,
                done=False,
            )

        event, payload = events[-1]
        self.assertEqual(event, "folder_status_progress")
        self.assertEqual(payload["processed"], 0)
        self.assertEqual(payload["percent"], 0.0)
        self.assertEqual(payload["cached_baseline"], 1)
        self.assertFalse(payload["first_status_build"])
        self.assertFalse(payload["done"])

    def test_missing_status_json_marks_one_time_first_build(self) -> None:
        core, _calls, events = self._core()
        with mock.patch.object(
            cachemod,
            "_runtime_settings",
            return_value=(self.history, self.library, [], "cover"),
        ):
            cachemod.install_core_patch(core)
            core.emit_ui(
                "folder_status_progress",
                processed=0,
                total=1,
                percent=0.0,
                albums=0,
                done=False,
            )

        _event, payload = events[-1]
        self.assertTrue(payload["first_status_build"])
        self.assertEqual(payload["cached_baseline"], 0)


if __name__ == "__main__":
    unittest.main()
