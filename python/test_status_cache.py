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
            cachemod._VALIDATION_STARTED = 0.0
            cachemod._VALIDATION_CHECKED = 0
            cachemod._VALIDATION_UNCHANGED = 0
            cachemod._VALIDATION_RESCANNED = 0

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

    def test_cache_reuses_unchanged_artist_and_rescans_changed_structure(self) -> None:
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

            (self.artist / "New Album").mkdir()
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

    def test_shallow_sentinel_reuses_unchanged_artist_and_rescans_structure_change(self) -> None:
        core, calls, _events = self._core()
        category = self.artist / "Category"
        category.mkdir()
        with mock.patch.object(
            cachemod,
            "_runtime_settings",
            return_value=(self.history, self.library, [], "cover"),
        ):
            cachemod.install_core_patch(core)
            core.inventory(self.artist, [], "cover", workers=1)
            cachemod.flush()
            self.assertEqual(calls, [self.artist])

            core.inventory(self.artist, [], "cover", workers=1)
            self.assertEqual(calls, [self.artist])

            added = self.artist / "New Category"
            added.mkdir()
            core.inventory(self.artist, [], "cover", workers=1)
            self.assertEqual(calls, [self.artist, self.artist])

    def test_library_update_persists_status_immediately(self) -> None:
        core, _calls, _events = self._core()
        with mock.patch.object(
            cachemod,
            "_runtime_settings",
            return_value=(self.history, self.library, [], "cover"),
        ):
            cachemod.install_core_patch(core)
            core.inventory(self.artist, [], "cover", workers=1)
            cachemod.flush()
            core.emit_ui(
                "library_update",
                albums=[
                    {
                        "path": str(self.album),
                        "status": "processed",
                    }
                ],
            )
            history_file = self.history / cachemod.CACHE_FILE
            raw = __import__("json").loads(history_file.read_text(encoding="utf-8"))
            cached_album = raw["artists"][str(self.artist)]["albums"][0]
            self.assertEqual(cached_album["status"], "processed")
            self.assertGreater(cached_album["status_updated_at_unix"], 0.0)
            self.assertEqual(cachemod._DIRTY, 0)

    def test_validation_metrics_report_cache_hits(self) -> None:
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
            core.inventory(self.artist, [], "cover", workers=1)
            core.emit_ui(
                "folder_status_progress",
                processed=1,
                total=1,
                percent=100.0,
                albums=1,
                done=True,
            )
        activity = [payload for event, payload in events if event == "activity"]
        self.assertTrue(activity)
        self.assertIn("1 Artists checked", activity[-1]["message"])
        self.assertIn("1 unchanged", activity[-1]["message"])
        self.assertIn("0 rescanned", activity[-1]["message"])


    def test_cached_artist_statuses_returns_live_statuses_for_unchanged_artist(self) -> None:
        core, _calls, _events = self._core()
        with mock.patch.object(
            cachemod,
            "_runtime_settings",
            return_value=(self.history, self.library, [], "cover"),
        ):
            cachemod.install_core_patch(core)
            core.inventory(self.artist, [], "cover", workers=1)
            cachemod.flush()
            core.emit_ui(
                "library_update",
                albums=[{"path": str(self.album), "status": "processed"}],
            )
            cached = cachemod.cached_artist_statuses(self.artist, [], "cover")
            self.assertIsNotNone(cached)
            assert cached is not None
            statuses, artist_path = cached
            self.assertEqual(statuses, ["processed"])
            self.assertEqual(artist_path, str(self.artist))

            (self.artist / "New Album").mkdir()
            self.assertIsNone(
                cachemod.cached_artist_statuses(self.artist, [], "cover")
            )



if __name__ == "__main__":
    unittest.main()
