from __future__ import annotations

import json
import os
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

import splined
from tui.library import LibraryModel
from tui.picker_index import PICKER_DB_NAME, PickerAlbum, PickerIndex


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


class LazyPickerInventoryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.base = Path(self.temp.name)
        self.root = self.base / "music"
        self.cache = self.base / "cache"
        self.root.mkdir()
        self.love = _album(self.root, "10,000 Maniacs", "Love Among the Ruins")
        self.eden = _album(self.root, "10,000 Maniacs", "Our Time in Eden")
        self.toys = _album(self.root, "Aerosmith", "Toys in the Attic", ".mp3")
        (self.love / "cover.jpg").write_bytes(b"not an image")
        (self.love / "booklet.png").write_bytes(b"not a configured cover")
        _album(self.root, "[Artist Singles]", "Ignored Album", ".mp3")
        _album(self.root, "[videos]", "Ignored Video Album")
        _album(self.root, "@eaDir", "junk", ".mp3")
        _album(self.root, ".stfolder-cache", "hidden", ".mp3")
        self.cfg = _config(self.root)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _run(
        self,
        responses: list[dict[str, object]],
        *,
        history: dict[str, object] | None = None,
        bypassed: set[str] | None = None,
    ):
        emitted: list[tuple[str, dict[str, object]]] = []
        encoded = iter(json.dumps(item) for item in responses)
        with (
            mock.patch.object(splined, "tui_active", return_value=True),
            mock.patch.object(
                splined,
                "emit_ui",
                side_effect=lambda event, **payload: emitted.append((event, payload)),
            ),
            mock.patch.object(splined, "read_input", side_effect=lambda *_a, **_k: next(encoded)),
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
            )
        return result, emitted

    def test_initial_startup_reads_root_only_and_exposes_uncached_artists(self) -> None:
        visited: list[Path] = []
        original_scandir = os.scandir

        def recording(path: Path | str):
            visited.append(Path(path))
            return original_scandir(path)

        with (
            mock.patch("tui.picker_index.os.scandir", side_effect=recording),
            mock.patch.object(splined, "inventory", side_effect=AssertionError("descended")),
        ):
            (selected, _overrides, _timeouts, _sources, known), emitted = self._run(
                [{"action": "launch", "scan_mode": "auto-selected", "selected": []}]
            )
        self.assertEqual(visited, [self.root])
        self.assertEqual(selected, [])
        self.assertEqual(known, [])
        payload = next(payload for event, payload in emitted if event == "library")
        self.assertEqual(
            [row["name"] for row in payload["artists"]],
            ["10,000 Maniacs", "Aerosmith"],
        )
        self.assertEqual(payload["albums"], [])
        self.assertTrue(all(not row["indexed"] for row in payload["artists"]))

    def test_ignored_roots_are_absent_from_index_and_never_traversed(self) -> None:
        ignored = {"[Artist Singles]", "[videos]", "@eaDir", ".stfolder-cache"}
        (_result, emitted) = self._run(
            [{"action": "launch", "scan_mode": "auto-selected", "selected": []}]
        )
        payload = next(payload for event, payload in emitted if event == "library")
        self.assertTrue(ignored.isdisjoint({row["name"] for row in payload["artists"]}))
        with PickerIndex.open(
            self.cache / PICKER_DB_NAME,
            self.root,
            self.cfg["library"]["ignored_subs"],  # type: ignore[index]
        ) as index:
            self.assertTrue(ignored.isdisjoint({row.name for row in index.artists()}))

    def test_open_uncached_artist_scans_only_once_and_returns_exact_selection(self) -> None:
        artist_path = str(self.root / "10,000 Maniacs")
        calls: list[Path] = []
        original_inventory = splined.inventory

        def inventory(path: Path, *args, **kwargs):
            calls.append(path)
            return original_inventory(path, *args, **kwargs)

        responses = [
            {"action": "load-artist", "artist_path": artist_path, "selected": [], "select_new": False},
            {"action": "load-artist", "artist_path": artist_path, "selected": [str(self.eden)], "select_new": False},
            {"action": "launch", "scan_mode": "filtered-read", "selected": [str(self.eden)], "select_new": False},
        ]
        with mock.patch.object(splined, "inventory", side_effect=inventory):
            (selected, _overrides, _timeouts, _sources, known), _emitted = self._run(responses)
        self.assertEqual(calls, [Path(artist_path)])
        self.assertEqual([album.path for album in selected], [self.eden])
        self.assertEqual({album.path for album in known}, {self.love, self.eden})

    def test_multiple_artists_process_only_checked_album_paths(self) -> None:
        maniacs = str(self.root / "10,000 Maniacs")
        aerosmith = str(self.root / "Aerosmith")
        responses = [
            {"action": "load-artist", "artist_path": maniacs, "selected": [], "select_new": False},
            {"action": "load-artist", "artist_path": aerosmith, "selected": [], "select_new": False},
            {
                "action": "launch",
                "scan_mode": "auto-selected",
                "selected": [str(self.eden), str(self.toys)],
                "select_new": False,
            },
        ]
        (selected, _overrides, _timeouts, _sources, _known), emitted = self._run(responses)
        self.assertEqual([album.path for album in selected], [self.eden, self.toys])
        self.assertNotIn(self.love, [album.path for album in selected])
        scope = next(
            payload
            for event, payload in emitted
            if event == "activity" and payload.get("category") == "selection"
        )
        self.assertIn("2 checked album(s)", str(scope["message"]))

    def test_warm_restart_uses_persisted_topology_without_artist_scan(self) -> None:
        artist_path = str(self.root / "10,000 Maniacs")
        self._run(
            [
                {"action": "load-artist", "artist_path": artist_path, "selected": [], "select_new": False},
                {"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False},
            ]
        )
        with mock.patch.object(splined, "inventory", side_effect=AssertionError("rescanned")):
            (_result, emitted) = self._run(
                [{"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False}]
            )
        payload = next(payload for event, payload in emitted if event == "library")
        self.assertEqual(
            {row["album"] for row in payload["albums"]},
            {"Love Among the Ruins", "Our Time in Eden"},
        )

    def test_auto_all_explicitly_indexes_every_artist(self) -> None:
        calls: list[Path] = []
        original_inventory = splined.inventory

        def inventory(path: Path, *args, **kwargs):
            calls.append(path)
            return original_inventory(path, *args, **kwargs)

        with mock.patch.object(splined, "inventory", side_effect=inventory):
            (selected, _overrides, _timeouts, _sources, known), emitted = self._run(
                [{"action": "launch", "scan_mode": "auto-all", "selected": []}]
            )
        self.assertEqual(set(calls), {self.root / "10,000 Maniacs", self.root / "Aerosmith"})
        self.assertEqual({album.path for album in known}, {self.love, self.eden, self.toys})
        self.assertEqual({album.path for album in selected}, {self.eden, self.toys})
        progress = [
            payload["message"]
            for event, payload in emitted
            if event == "activity" and payload.get("source") == "auto-all"
        ]
        self.assertTrue(any("Artists indexed:" in str(value) for value in progress))

    def test_prelaunch_boundary_never_reads_tags_network_candidates_or_images(self) -> None:
        artist_path = str(self.root / "10,000 Maniacs")
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
            self._run(
                [
                    {"action": "load-artist", "artist_path": artist_path, "selected": [], "select_new": False},
                    {"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False},
                ]
            )

    def test_folder_identity_and_order_are_stable_without_tag_enrichment(self) -> None:
        artist_path = str(self.root / "10,000 Maniacs")
        (_result, emitted) = self._run(
            [
                {"action": "load-artist", "artist_path": artist_path, "selected": [], "select_new": False},
                {"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False},
            ]
        )
        updates = [payload for event, payload in emitted if event in {"library", "library_update"}]
        final = LibraryModel.from_payload(updates[-1])
        self.assertEqual([row.name for row in final.visible_artists()], ["10,000 Maniacs", "Aerosmith"])
        final.active_artist = "10,000 Maniacs"
        self.assertEqual(
            [row.title for row in final.visible_albums(active_artist_only=True)],
            ["Love Among the Ruins", "Our Time in Eden"],
        )
        self.assertFalse(any(event == "library_enrichment" for event, _payload in emitted))

    def test_history_bypass_timeout_reconciles_after_artist_validation(self) -> None:
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
        artist_path = str(self.root / "10,000 Maniacs")
        (_result, emitted) = self._run(
            [
                {"action": "load-artist", "artist_path": artist_path, "selected": [], "select_new": False},
                {"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False},
            ],
            history=history,
            bypassed={str(self.love)},
        )
        update = [payload for event, payload in emitted if event == "library_update"][-1]
        rows = {row["path"]: row for row in update["albums"]}
        self.assertEqual(rows[str(self.love)]["status"], "bypassed")
        self.assertEqual(rows[str(self.eden)]["status"], "timeout")

    def test_local_art_detection_does_not_decode_image(self) -> None:
        artist_path = str(self.root / "10,000 Maniacs")
        with mock.patch.object(splined.Image, "open", side_effect=AssertionError("decoded")):
            (_result, emitted) = self._run(
                [
                    {"action": "load-artist", "artist_path": artist_path, "selected": [], "select_new": False},
                    {"action": "launch", "scan_mode": "auto-selected", "selected": [], "select_new": False},
                ]
            )
        update = [payload for event, payload in emitted if event == "library_update"][-1]
        love = next(row for row in update["albums"] if row["path"] == str(self.love))
        self.assertEqual(love["formats"], ["JPEG"])

    def test_deleting_and_corrupting_picker_cache_are_safe(self) -> None:
        self._run([{"action": "launch", "scan_mode": "auto-selected", "selected": []}])
        database = self.cache / PICKER_DB_NAME
        database.unlink()
        self._run([{"action": "launch", "scan_mode": "auto-selected", "selected": []}])
        database.write_bytes(b"not sqlite")
        (_result, emitted) = self._run(
            [{"action": "launch", "scan_mode": "auto-selected", "selected": []}]
        )
        self.assertTrue(database.exists())
        self.assertTrue(any(self.cache.glob(f"{PICKER_DB_NAME}.corrupt-*")))
        self.assertTrue(any(event == "log" and payload.get("level") == "WARN" for event, payload in emitted))

    def test_ignored_signature_or_library_root_change_invalidates_rows(self) -> None:
        path = self.cache / PICKER_DB_NAME
        with PickerIndex.open(path, self.root, []) as index:
            artists = index.reconcile_root()
            index.replace_artist_albums(
                artists[0],
                [PickerAlbum(str(self.love), artists[0].path, self.love.name, ())],
            )
            self.assertTrue(index.albums())
        with PickerIndex.open(path, self.root, ["new-ignore"]) as index:
            self.assertEqual(index.albums(), [])
        other = self.base / "other"
        other.mkdir()
        with PickerIndex.open(path, other, ["new-ignore"]) as index:
            self.assertEqual(index.artists(), [])

    def test_symlink_artist_is_not_indexed(self) -> None:
        outside = self.base / "outside"
        _album(outside, "Linked Artist", "Linked Album")
        link = self.root / "linked"
        try:
            os.symlink(outside, link, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest("directory symlinks are unavailable in this runner")
        (_result, emitted) = self._run(
            [{"action": "launch", "scan_mode": "auto-selected", "selected": []}]
        )
        payload = next(payload for event, payload in emitted if event == "library")
        self.assertNotIn("linked", {row["name"] for row in payload["artists"]})

    def test_root_only_large_artist_fixture_never_touches_album_topology(self) -> None:
        root = self.root

        class Entry:
            def __init__(self, index: int) -> None:
                self.name = f"Artist {index:04d}"
                self.path = str(root / self.name)

            def is_symlink(self) -> bool:
                return False

            def is_dir(self, *, follow_symlinks: bool = False) -> bool:
                return True

        class Entries:
            def __init__(self, values):
                self.values = values

            def __enter__(self):
                return iter(self.values)

            def __exit__(self, *_args):
                return False

        values = [Entry(index) for index in range(2000)]
        started = time.perf_counter()
        with (
            mock.patch("tui.picker_index.os.scandir", return_value=Entries(values)) as scandir,
            mock.patch.object(splined, "inventory", side_effect=AssertionError("album topology")),
        ):
            (_result, emitted) = self._run(
                [{"action": "launch", "scan_mode": "auto-selected", "selected": []}]
            )
        elapsed = time.perf_counter() - started
        payload = next(payload for event, payload in emitted if event == "library")
        self.assertEqual(len(payload["artists"]), 2000)
        self.assertEqual(payload["albums"], [])
        scandir.assert_called_once_with(self.root)
        self.assertLess(elapsed, 5.0)

    def test_warm_2000_artist_15000_album_fixture_is_sqlite_only(self) -> None:
        root = self.root

        class Entry:
            def __init__(self, index: int) -> None:
                self.name = f"Artist {index:04d}"
                self.path = str(root / self.name)

            def is_symlink(self) -> bool:
                return False

            def is_dir(self, *, follow_symlinks: bool = False) -> bool:
                return True

        class Entries:
            def __init__(self, values):
                self.values = values

            def __enter__(self):
                return iter(self.values)

            def __exit__(self, *_args):
                return False

        values = [Entry(index) for index in range(2000)]
        database = self.cache / PICKER_DB_NAME
        ignored = self.cfg["library"]["ignored_subs"]  # type: ignore[index]
        with mock.patch(
            "tui.picker_index.os.scandir",
            return_value=Entries(values),
        ):
            with PickerIndex.open(database, self.root, ignored) as index:
                artists = index.reconcile_root()
                album_rows = []
                for artist_number, artist in enumerate(artists):
                    count = 8 if artist_number < 1000 else 7
                    album_rows.extend(
                        (
                            str(Path(artist.path) / f"Album {album_number:02d}"),
                            artist.path,
                            f"Album {album_number:02d}",
                            "[]",
                        )
                        for album_number in range(count)
                    )
                with index.connection:
                    index.connection.executemany(
                        "INSERT INTO albums(album_path, artist_path, album_name, local_art_json) VALUES(?, ?, ?, ?)",
                        album_rows,
                    )
                    index.connection.execute(
                        "UPDATE artists SET indexed=1, last_indexed=?",
                        (time.time(),),
                    )
        self.assertEqual(len(album_rows), 15000)
        started = time.perf_counter()
        with (
            mock.patch("tui.picker_index.os.scandir", return_value=Entries(values)),
            mock.patch.object(splined, "inventory", side_effect=AssertionError("warm recursive scan")),
        ):
            (_result, emitted) = self._run(
                [
                    {
                        "action": "launch",
                        "scan_mode": "auto-selected",
                        "selected": [],
                        "select_new": False,
                    }
                ]
            )
        elapsed = time.perf_counter() - started
        payload = next(payload for event, payload in emitted if event == "library")
        self.assertEqual(len(payload["artists"]), 2000)
        self.assertEqual(len(payload["albums"]), 15000)
        self.assertLess(elapsed, 5.0)

    def test_auto_all_explicitly_indexes_2000_artists_and_15000_albums(self) -> None:
        root = self.root

        class Entry:
            def __init__(self, index: int) -> None:
                self.name = f"Artist {index:04d}"
                self.path = str(root / self.name)

            def is_symlink(self) -> bool:
                return False

            def is_dir(self, *, follow_symlinks: bool = False) -> bool:
                return True

        class Entries:
            def __init__(self, values):
                self.values = values

            def __enter__(self):
                return iter(self.values)

            def __exit__(self, *_args):
                return False

        values = [Entry(index) for index in range(2000)]
        inventory_calls: list[Path] = []

        def synthetic_inventory(path: Path, *_args, **_kwargs):
            inventory_calls.append(path)
            artist_number = int(path.name.rsplit(" ", 1)[-1])
            count = 8 if artist_number < 1000 else 7
            return (
                [
                    splined.AlbumDir(
                        path / f"Album {album_number:02d}",
                        [path / f"Album {album_number:02d}" / "track.flac"],
                    )
                    for album_number in range(count)
                ],
                0,
            )

        started = time.perf_counter()
        with (
            mock.patch(
                "tui.picker_index.os.scandir",
                return_value=Entries(values),
            ),
            mock.patch.object(splined, "inventory", side_effect=synthetic_inventory),
        ):
            (selected, _overrides, _timeouts, _sources, known), emitted = self._run(
                [{"action": "launch", "scan_mode": "auto-all", "selected": []}]
            )
        elapsed = time.perf_counter() - started
        self.assertEqual(len(inventory_calls), 2000)
        self.assertEqual(len({str(path) for path in inventory_calls}), 2000)
        self.assertEqual(len(known), 15000)
        self.assertEqual(len(selected), 15000)
        completed = [
            str(payload["message"])
            for event, payload in emitted
            if event == "activity"
            and payload.get("source") == "auto-all"
            and payload.get("state") == "done"
        ]
        self.assertEqual(len(completed), 1)
        self.assertIn("2,000 Artist(s)", completed[0])
        self.assertIn("15,000 Album(s)", completed[0])
        self.assertLess(elapsed, 30.0)

    def test_inventory_parallelism_and_deterministic_order_are_preserved(self) -> None:
        for index in range(16):
            _album(self.root, f"Parallel Artist {index:02d}", "Album")
        expected, expected_ignored = splined.inventory(
            self.root,
            self.cfg["library"]["ignored_subs"],  # type: ignore[index]
            workers=1,
        )
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
            actual, actual_ignored = splined.inventory(
                self.root,
                self.cfg["library"]["ignored_subs"],  # type: ignore[index]
                workers=4,
            )
        self.assertGreater(maximum, 1)
        self.assertLessEqual(maximum, 4)
        self.assertEqual([album.path for album in actual], [album.path for album in expected])
        self.assertEqual(actual_ignored, expected_ignored)

    def test_post_launch_reads_every_authoritative_track_without_picker_cache(self) -> None:
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
