from __future__ import annotations

import json
import os
import sqlite3
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

import splined
from tui.library import LibraryModel
from tui.picker_index import (
    PICKER_DB_NAME,
    PICKER_SCHEMA_VERSION,
    PickerAlbum,
    PickerArtist,
    PickerIndex,
)


def _config(root: Path) -> dict[str, object]:
    return {
        "config_version": 5,
        "mode": "read",
        "library": {
            "music_library": str(root),
            "ignored_subs": ["[Artist Singles]", "[videos]", "@eaDir", ".stfolder-*"],
        },
        "scan": {"scan_mode_timeout": 24},
        "history": {"enabled": True, "retention_days": 0},
        "output": {
            "file_name": "cover",
            "file_formats": ["jpeg", "png", "webp"],
            "preserve_file": True,
            "square": True,
            "square_mode": "crop",
            "square_round_to": 16,
            "upscale_below_ideal": False,
            "evaluate_final_image": True,
        },
        "range": {"min": 1200, "ideal": 1800, "max": 2400, "ladder": 3600},
        "samples": {"sample_write": False},
        "sources": {"cover_sources": ["itunes"], "exclude_cover_sources": []},
        "source_policies": {},
        "aisplined": {
            "enabled": False,
            "endpoint": "",
            "minimum_short_side": 600,
            "allow_below_minimum_override": False,
        },
    }


def _album(root: Path, artist: str, title: str, suffix: str = ".flac") -> Path:
    directory = root / artist / title
    directory.mkdir(parents=True)
    (directory / f"track01{suffix}").write_bytes(b"not parsed by Select Media")
    return directory


class CompletePickerSnapshotTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.base = Path(self.temp.name)
        self.root = self.base / "music"
        self.cache = self.base / "cache"
        self.root.mkdir()
        self.love = _album(self.root, "10,000 Maniacs", "Love Among the Ruins")
        self.eden = _album(self.root, "10,000 Maniacs", "Our Time in Eden")
        self.toys = _album(self.root, "Aerosmith", "Toys in the Attic", ".mp3")
        (self.love / "cover.jpg").write_bytes(b"filename-only local art")
        (self.love / "booklet.png").write_bytes(b"not a configured cover")
        _album(self.root, "[Artist Singles]", "Ignored Album", ".mp3")
        _album(self.root, "[videos]", "Ignored Video Album")
        _album(self.root, "@eaDir", "junk", ".mp3")
        _album(self.root, ".stfolder-cache", "hidden", ".mp3")
        _album(self.root, ".animatedartworkdownloader", "hidden", ".mp3")
        self.cfg = _config(self.root)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _run(
        self,
        responses: list[dict[str, object]],
        *,
        history: dict[str, object] | None = None,
        bypassed: set[str] | None = None,
        picker_session: splined.PickerSessionState | None = None,
        initial_event: str = "library",
        read_input=None,
    ):
        emitted: list[tuple[str, dict[str, object]]] = []
        encoded = iter(json.dumps(item) for item in responses)
        reader = read_input or (lambda *_a, **_k: next(encoded))
        with (
            mock.patch.object(splined, "tui_active", return_value=True),
            mock.patch.object(
                splined,
                "emit_ui",
                side_effect=lambda event, **payload: emitted.append((event, payload)),
            ),
            mock.patch.object(splined, "read_input", side_effect=reader),
        ):
            result = splined.prepare_tui_library_selection(
                self.root / "config.toml",
                self.cfg,
                ["itunes"],
                self.root,
                history or {"version": 1, "albums": {}},
                24,
                cache=self.cache,
                library_root=self.root,
                bypassed_paths=bypassed,
                picker_session=picker_session,
                initial_event=initial_event,
            )
        return result, emitted

    def _cold_build(self) -> list[tuple[str, dict[str, object]]]:
        (_result, emitted) = self._run(
            [{"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False}]
        )
        return emitted

    def test_cold_build_persists_complete_topology_and_exact_progress(self) -> None:
        emitted = self._cold_build()
        payload = next(payload for event, payload in emitted if event == "library")
        self.assertEqual([row["name"] for row in payload["artists"]], ["10,000 Maniacs", "Aerosmith"])
        self.assertEqual(len(payload["albums"]), 3)
        self.assertTrue(payload["snapshot_complete"])
        progress = [payload for event, payload in emitted if event == "cache_progress"]
        self.assertEqual(progress[-1]["processed"], 2)
        self.assertEqual(progress[-1]["total"], 2)
        self.assertEqual(progress[-1]["percent"], 100.0)
        self.assertEqual(progress[-1]["albums"], 3)
        with PickerIndex.open(
            self.cache / PICKER_DB_NAME,
            self.root,
            self.cfg["library"]["ignored_subs"],  # type: ignore[index]
        ) as index:
            info = index.snapshot_info()
            self.assertIsNotNone(info)
            assert info is not None
            self.assertEqual((info.artist_count, info.album_count), (2, 3))
            meta = index._meta()
            self.assertEqual(meta["schema_version"], str(PICKER_SCHEMA_VERSION))
            self.assertEqual(meta["snapshot_complete"], "1")

    def test_warm_first_render_is_sqlite_only_when_media_access_fails(self) -> None:
        self._cold_build()
        with (
            mock.patch("tui.picker_index.os.scandir", side_effect=AssertionError("/music root accessed")),
            mock.patch.object(splined, "inventory", side_effect=AssertionError("/music tree accessed")),
        ):
            (_result, emitted) = self._run(
                [{"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False}]
            )
        payload = next(payload for event, payload in emitted if event == "library")
        self.assertEqual(len(payload["artists"]), 2)
        self.assertEqual(len(payload["albums"]), 3)
        self.assertTrue(all(row["indexed"] and row["loaded"] for row in payload["artists"]))
        self.assertNotIn("Inventory not loaded", json.dumps(payload))

    def test_same_session_return_uses_memory_without_sqlite_or_filesystem(self) -> None:
        session = splined.PickerSessionState()
        self._run(
            [{"action": "launch", "scan_mode": "auto-selected", "selected": [str(self.love)], "select_new": False}],
            picker_session=session,
        )
        history = {"version": 1, "albums": {str(self.love): {"outcome": "normal-selected"}}}
        with (
            mock.patch.object(PickerIndex, "open", side_effect=AssertionError("SQLite reopened")),
            mock.patch.object(splined, "inventory", side_effect=AssertionError("filesystem rescanned")),
            mock.patch("tui.picker_index.os.scandir", side_effect=AssertionError("root reconciled")),
        ):
            (_result, emitted) = self._run(
                [{"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False}],
                history=history,
                picker_session=session,
                initial_event="library_update",
            )
        update = next(payload for event, payload in emitted if event == "library_update")
        row = next(row for row in update["albums"] if row["path"] == str(self.love))
        self.assertEqual(row["status"], "processed")
        self.assertFalse(row["selected"])

    def test_complete_snapshot_returns_exact_checked_paths_across_artists(self) -> None:
        (selected, _overrides, _timeouts, _sources, known), emitted = self._run(
            [{
                "action": "launch",
                "scan_mode": "auto-selected",
                "selected": [str(self.eden), str(self.toys)],
                "select_new": False,
            }]
        )
        self.assertEqual([album.path for album in selected], [self.eden, self.toys])
        self.assertNotIn(self.love, [album.path for album in selected])
        self.assertEqual({album.path for album in known}, {self.love, self.eden, self.toys})
        scope = next(
            payload for event, payload in emitted
            if event == "activity" and payload.get("category") == "selection"
        )
        self.assertIn("2 checked album(s)", str(scope["message"]))

    def test_prelaunch_boundary_forbids_tags_network_candidates_images_and_ai(self) -> None:
        forbidden = AssertionError("processing crossed the Launch boundary")
        with (
            mock.patch.object(splined, "MutagenFile", side_effect=forbidden),
            mock.patch.object(splined, "read_track", side_effect=forbidden),
            mock.patch.object(splined, "lookup_release", side_effect=forbidden),
            mock.patch.object(splined, "discover_all", side_effect=forbidden),
            mock.patch.object(splined, "download_candidates", side_effect=forbidden),
            mock.patch.object(splined, "select_best", side_effect=forbidden),
            mock.patch.object(splined.Image, "open", side_effect=forbidden),
        ):
            self._cold_build()

    def test_folder_identity_status_local_art_and_complete_counts(self) -> None:
        albums, _ignored = splined.inventory(self.root, [])
        eden = next(album for album in albums if album.path == self.eden)
        history = {
            "version": 1,
            "albums": {
                str(self.eden): {
                    "completed_at_unix": time.time(),
                    "album_fingerprint": splined.album_scan_fingerprint(eden),
                    "policy_fingerprint": splined.scan_policy_fingerprint(self.cfg, ["itunes"]),
                    "outcome": "selected",
                }
            },
        }
        with mock.patch.object(splined.Image, "open", side_effect=AssertionError("decoded")):
            (_result, emitted) = self._run(
                [{"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False}],
                history=history,
                bypassed={str(self.toys)},
            )
        payload = next(payload for event, payload in emitted if event == "library")
        model = LibraryModel.from_payload(payload)
        self.assertEqual([item.name for item in model.visible_artists()], ["10,000 Maniacs", "Aerosmith"])
        self.assertEqual(len(model.albums), 3)
        rows = {row["path"]: row for row in payload["albums"]}
        self.assertEqual(rows[str(self.love)]["formats"], ["JPEG"])
        self.assertEqual(rows[str(self.eden)]["status"], "timeout")
        self.assertEqual(rows[str(self.toys)]["status"], "bypassed")
        self.assertFalse(any(event == "library_enrichment" for event, _payload in emitted))

    def test_hidden_ignored_and_symlink_roots_never_enter_snapshot(self) -> None:
        outside = self.base / "outside"
        _album(outside, "Linked Artist", "Linked Album")
        link = self.root / "linked"
        try:
            os.symlink(outside, link, target_is_directory=True)
        except (OSError, NotImplementedError):
            link = None
        emitted = self._cold_build()
        payload = next(payload for event, payload in emitted if event == "library")
        names = {row["name"] for row in payload["artists"]}
        self.assertEqual(names, {"10,000 Maniacs", "Aerosmith"})
        if link is not None:
            self.assertNotIn("linked", names)
        with PickerIndex.open(
            self.cache / PICKER_DB_NAME,
            self.root,
            self.cfg["library"]["ignored_subs"],  # type: ignore[index]
        ) as index:
            all_paths = [path for (path,) in index.connection.execute("SELECT artist_path FROM artists")]
            self.assertFalse(any(Path(path).name.startswith(".") for path in all_paths))
            self.assertFalse(any(Path(path).name in {"[Artist Singles]", "[videos]", "@eaDir"} for path in all_paths))

    def test_deleted_corrupt_wrong_root_and_changed_ignore_cache_rebuild_safely(self) -> None:
        self._cold_build()
        database = self.cache / PICKER_DB_NAME
        database.unlink()
        self._cold_build()
        database.write_bytes(b"not sqlite")
        emitted = self._cold_build()
        self.assertTrue(database.exists())
        self.assertTrue(any(self.cache.glob(f"{PICKER_DB_NAME}.corrupt-*")))
        self.assertTrue(any(event == "log" and payload.get("level") == "WARN" for event, payload in emitted))
        with PickerIndex.open(database, self.root, ["different-ignore"]) as index:
            self.assertIsNone(index.snapshot_info())
        other = self.base / "other"
        other.mkdir()
        with PickerIndex.open(database, other, ["different-ignore"]) as index:
            self.assertIsNone(index.snapshot_info())

    def test_failed_generation_promotion_retains_last_complete_snapshot(self) -> None:
        path = self.cache / PICKER_DB_NAME
        artist = PickerArtist(str(self.root / "Artist"), "Artist")
        album = PickerAlbum(str(self.root / "Artist" / "Album"), artist.path, "Album")
        with PickerIndex.open(path, self.root, []) as index:
            original = index.promote_snapshot([artist], [album])
            invalid = PickerAlbum(str(self.root / "Missing" / "Album"), str(self.root / "Missing"), "Album")
            with self.assertRaises(sqlite3.IntegrityError):
                index.promote_snapshot([artist], [invalid])
            self.assertEqual(index.snapshot_info(), original)
            self.assertEqual(index.albums(), [album])

    def test_background_validation_promotes_one_coherent_snapshot(self) -> None:
        self._cold_build()
        removed_album = self.root / "Aerosmith" / "Toys in the Attic"
        for path in removed_album.iterdir():
            path.unlink()
        removed_album.rmdir()
        new_album = _album(self.root, "New Artist", "New Album")
        session = splined.PickerSessionState()
        emitted: list[tuple[str, dict[str, object]]] = []

        def reader(*_args, **_kwargs):
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if any(event == "library_update" for event, _payload in emitted):
                    return json.dumps({"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False})
                time.sleep(0.01)
            raise AssertionError("background validation did not publish")

        with (
            mock.patch.object(splined, "tui_active", return_value=True),
            mock.patch.object(
                splined,
                "emit_ui",
                side_effect=lambda event, **payload: emitted.append((event, payload)),
            ),
            mock.patch.object(splined, "read_input", side_effect=reader),
        ):
            splined.prepare_tui_library_selection(
                self.root / "config.toml",
                self.cfg,
                ["itunes"],
                self.root,
                {"version": 1, "albums": {}},
                24,
                cache=self.cache,
                library_root=self.root,
                picker_session=session,
            )
        updates = [payload for event, payload in emitted if event == "library_update"]
        self.assertEqual(len(updates), 1)
        paths = {row["path"] for row in updates[0]["albums"]}
        self.assertIn(str(new_album), paths)
        self.assertNotIn(str(removed_album), paths)
        self.assertTrue(any(event == "cache_progress" and payload.get("percent") == 100.0 for event, payload in emitted))

    def test_large_complete_fixture_and_sqlite_only_warm_restart(self) -> None:
        root = self.root

        class Entry:
            def __init__(self, index: int) -> None:
                self.name = f"Artist {index:04d}"
                self.path = str(root / self.name)

            def is_symlink(self) -> bool:
                return False

            def is_dir(self, *, follow_symlinks: bool = False) -> bool:
                return True

        values = [Entry(index) for index in range(2000)]

        class Entries:
            def __enter__(self):
                return iter(values)

            def __exit__(self, *_args):
                return False

        audio_file_count = 0

        def synthetic_inventory(path: Path, *_args, **_kwargs):
            nonlocal audio_file_count
            artist_number = int(path.name.rsplit(" ", 1)[-1])
            count = 8 if artist_number < 1000 else 7
            rows = []
            for album_number in range(count):
                track_count = 5 if album_number < 5 else 4
                audio_file_count += track_count
                album_path = path / f"Album {album_number:02d}"
                rows.append(
                    splined.AlbumDir(
                        album_path,
                        [album_path / f"track{track:02d}.flac" for track in range(track_count)],
                        inventory_fingerprint=f"fp-{artist_number}-{album_number}",
                    )
                )
            return rows, []

        cold_started = time.perf_counter()
        with (
            mock.patch("tui.picker_index.os.scandir", return_value=Entries()),
            mock.patch.object(splined, "inventory", side_effect=synthetic_inventory),
        ):
            (_result, cold_events) = self._run(
                [{"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False}]
            )
        cold_elapsed = time.perf_counter() - cold_started
        cold_payload = next(payload for event, payload in cold_events if event == "library")
        self.assertEqual((len(cold_payload["artists"]), len(cold_payload["albums"])), (2000, 15000))
        self.assertEqual(audio_file_count, 70000)
        progress = [payload for event, payload in cold_events if event == "cache_progress"]
        self.assertEqual((progress[-1]["processed"], progress[-1]["total"], progress[-1]["percent"]), (2000, 2000, 100.0))

        warm_started = time.perf_counter()
        with (
            mock.patch("tui.picker_index.os.scandir", side_effect=AssertionError("warm root access")),
            mock.patch.object(splined, "inventory", side_effect=AssertionError("warm tree access")),
        ):
            (_result, warm_events) = self._run(
                [{"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False}]
            )
        warm_elapsed = time.perf_counter() - warm_started
        warm_payload = next(payload for event, payload in warm_events if event == "library")
        self.assertEqual((len(warm_payload["artists"]), len(warm_payload["albums"])), (2000, 15000))
        self.assertLess(cold_elapsed, 30.0)
        self.assertLess(warm_elapsed, 5.0)
        print(
            "LARGE_FIXTURE_TIMING "
            f"artists=2000 albums=15000 audio_filenames={audio_file_count} "
            f"cold_seconds={cold_elapsed:.6f} warm_seconds={warm_elapsed:.6f}"
        )

    def test_inventory_parallelism_and_deterministic_order_are_preserved(self) -> None:
        for index in range(16):
            _album(self.root, f"Parallel Artist {index:02d}", "Album")
        ignored = self.cfg["library"]["ignored_subs"]  # type: ignore[index]
        expected, expected_ignored = splined.inventory(self.root, ignored, workers=1)
        original = splined.os.scandir
        lock = threading.Lock()
        active = 0
        maximum = 0

        def delayed(path: Path | str):
            nonlocal active, maximum
            with lock:
                active += 1
                maximum = max(maximum, active)
            try:
                time.sleep(0.005)
                return original(path)
            finally:
                with lock:
                    active -= 1

        with mock.patch.object(splined.os, "scandir", side_effect=delayed):
            actual, actual_ignored = splined.inventory(self.root, ignored, workers=4)
        self.assertGreater(maximum, 1)
        self.assertLessEqual(maximum, 4)
        self.assertEqual([album.path for album in actual], [album.path for album in expected])
        self.assertEqual(actual_ignored, expected_ignored)

    def test_post_launch_reads_every_authoritative_track(self) -> None:
        second = self.eden / "track02.flac"
        second.write_bytes(b"audio")
        album = splined.AlbumDir(self.eden, [self.eden / "track01.flac", second])
        tracks = [
            splined.Track(path, path.stem, "Artist", "Album", "Artist", None, None, None)
            for path in album.audio_files
        ]
        with mock.patch.object(splined, "read_track", side_effect=tracks) as reader:
            self.assertEqual(splined.read_album_tracks(album), tracks)
        self.assertEqual(reader.call_count, 2)


if __name__ == "__main__":
    unittest.main()
