from __future__ import annotations

import json
import os
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

import splined
from tui.library import LibraryModel
from tui.picker_index import PickerIndex


def _config(root: Path) -> dict[str, object]:
    return {
        "config_version": 5,
        "mode": "read",
        "library": {
            "music_library": str(root),
            "ignored_subs": ["[Artist Singles]", "[videos]", "@eaDir", ".stfolder-*"],
        },
        "scan": {
            "scan_mode_timeout": 24,
            "scan_mode": True,
            "library_scan": False,
            "cache_dir": "_cache",
            "log_dir": "_logs",
            "history_dir": "_logs/_history",
            "scan_library_dir": str(root),
        },
        "history": {"enabled": True, "retention_days": 0},
        "logging": {"retention_days": 14},
        "credentials": {"credential_dir": "credentials"},
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


class DirectLazyInventoryTests(unittest.TestCase):
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
        picker_session: splined.PickerSessionState | None = None,
        initial_event: str = "library",
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
            mock.patch.object(
                splined,
                "read_input",
                side_effect=lambda *_a, **_k: next(encoded),
            ),
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

    def test_initial_startup_reads_root_only_without_picker_cache(self) -> None:
        visited: list[Path] = []
        original_scandir = os.scandir

        def recording(path: Path | str):
            visited.append(Path(path))
            return original_scandir(path)

        with (
            mock.patch.object(splined.os, "scandir", side_effect=recording),
            mock.patch.object(
                splined,
                "inventory",
                side_effect=AssertionError("descended into Album topology"),
            ),
            mock.patch.object(
                PickerIndex,
                "open",
                side_effect=AssertionError("SQLite picker cache opened"),
            ),
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
        self.assertTrue(all(not row["loaded"] for row in payload["artists"]))
        self.assertFalse(
            any(event in {"cache_build_start", "cache_progress"} for event, _ in emitted)
        )

    def test_open_artist_scans_only_that_artist_once(self) -> None:
        artist_path = str(self.root / "10,000 Maniacs")
        calls: list[Path] = []
        original_inventory = splined.inventory

        def inventory(path: Path, *args, **kwargs):
            calls.append(path)
            return original_inventory(path, *args, **kwargs)

        responses = [
            {
                "action": "load-artist",
                "artist_path": artist_path,
                "selected": [],
            },
            {
                "action": "load-artist",
                "artist_path": artist_path,
                "selected": [str(self.eden)],
            },
            {
                "action": "launch",
                "scan_mode": "filtered-read",
                "selected": [str(self.eden)],
            },
        ]
        with mock.patch.object(splined, "inventory", side_effect=inventory):
            (selected, _overrides, _timeouts, _sources, known), emitted = self._run(
                responses
            )

        self.assertEqual(calls, [Path(artist_path)])
        self.assertEqual([album.path for album in selected], [self.eden])
        self.assertEqual({album.path for album in known}, {self.love, self.eden})
        update = [payload for event, payload in emitted if event == "library_update"][-1]
        artist = next(row for row in update["artists"] if row["name"] == "10,000 Maniacs")
        self.assertTrue(artist["loaded"])

    def test_same_session_return_uses_resident_folder_model(self) -> None:
        artist_path = str(self.root / "10,000 Maniacs")
        session = splined.PickerSessionState()
        self._run(
            [
                {"action": "load-artist", "artist_path": artist_path, "selected": []},
                {"action": "launch", "scan_mode": "auto-selected", "selected": []},
            ],
            picker_session=session,
        )

        with (
            mock.patch.object(
                splined.os,
                "scandir",
                side_effect=AssertionError("library root reread"),
            ),
            mock.patch.object(
                splined,
                "inventory",
                side_effect=AssertionError("Artist rescanned"),
            ),
            mock.patch.object(
                PickerIndex,
                "open",
                side_effect=AssertionError("SQLite reopened"),
            ),
        ):
            (_result, emitted) = self._run(
                [{"action": "launch", "scan_mode": "auto-selected", "selected": []}],
                picker_session=session,
                initial_event="library_update",
            )

        payload = next(
            payload for event, payload in emitted if event == "library_update"
        )
        self.assertEqual(
            {row["album"] for row in payload["albums"]},
            {"Love Among the Ruins", "Our Time in Eden"},
        )

    def test_select_all_explicitly_reads_every_artist(self) -> None:
        calls: list[Path] = []
        original_inventory = splined.inventory

        def inventory(path: Path, *args, **kwargs):
            calls.append(path)
            return original_inventory(path, *args, **kwargs)

        with mock.patch.object(splined, "inventory", side_effect=inventory):
            (selected, _overrides, _timeouts, _sources, known), _emitted = self._run(
                [
                    {"action": "select-all", "selected": []},
                    {
                        "action": "launch",
                        "scan_mode": "auto-selected",
                        "selected": [str(self.eden), str(self.toys)],
                    },
                ]
            )

        self.assertEqual(
            set(calls),
            {self.root / "10,000 Maniacs", self.root / "Aerosmith"},
        )
        self.assertEqual({album.path for album in known}, {self.love, self.eden, self.toys})
        self.assertEqual({album.path for album in selected}, {self.eden, self.toys})

    def test_auto_all_is_an_explicit_whole_library_traversal(self) -> None:
        calls: list[Path] = []
        original_inventory = splined.inventory

        def inventory(path: Path, *args, **kwargs):
            calls.append(path)
            return original_inventory(path, *args, **kwargs)

        with mock.patch.object(splined, "inventory", side_effect=inventory):
            (selected, _overrides, _timeouts, _sources, known), _emitted = self._run(
                [{"action": "launch", "scan_mode": "auto-all", "selected": []}]
            )

        self.assertEqual(
            set(calls),
            {self.root / "10,000 Maniacs", self.root / "Aerosmith"},
        )
        self.assertEqual({album.path for album in known}, {self.love, self.eden, self.toys})
        self.assertEqual({album.path for album in selected}, {self.eden, self.toys})

    def test_refresh_only_reloads_root_artist_folders(self) -> None:
        with mock.patch.object(
            splined,
            "inventory",
            side_effect=AssertionError("refresh descended into Artist folders"),
        ):
            (_result, emitted) = self._run(
                [
                    {"action": "refresh-index", "selected": []},
                    {"action": "launch", "scan_mode": "auto-selected", "selected": []},
                ]
            )
        self.assertTrue(
            any(
                event == "activity"
                and payload.get("message", "").startswith("Artist folder list refreshed")
                for event, payload in emitted
            )
        )
        self.assertFalse(
            any(event in {"cache_build_start", "cache_progress"} for event, _ in emitted)
        )

    def test_prelaunch_boundary_never_reads_tags_network_images_or_ai(self) -> None:
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
                    {"action": "load-artist", "artist_path": artist_path, "selected": []},
                    {"action": "launch", "scan_mode": "auto-selected", "selected": []},
                ]
            )

    def test_history_bypass_timeout_reconciles_when_artist_is_loaded(self) -> None:
        albums, _ignored = splined.inventory(self.root, [])
        eden = next(album for album in albums if album.path == self.eden)
        history = {
            "version": 1,
            "albums": {
                str(self.eden): {
                    "completed_at_unix": time.time(),
                    "album_fingerprint": splined.album_scan_fingerprint(eden),
                    "policy_fingerprint": splined.scan_policy_fingerprint(
                        self.cfg, ["itunes"]
                    ),
                    "outcome": "selected",
                }
            },
        }
        artist_path = str(self.root / "10,000 Maniacs")
        (_result, emitted) = self._run(
            [
                {"action": "load-artist", "artist_path": artist_path, "selected": []},
                {"action": "launch", "scan_mode": "auto-selected", "selected": []},
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
        with mock.patch.object(
            splined.Image,
            "open",
            side_effect=AssertionError("decoded image before Launch"),
        ):
            (_result, emitted) = self._run(
                [
                    {"action": "load-artist", "artist_path": artist_path, "selected": []},
                    {"action": "launch", "scan_mode": "auto-selected", "selected": []},
                ]
            )
        update = [payload for event, payload in emitted if event == "library_update"][-1]
        love = next(row for row in update["albums"] if row["path"] == str(self.love))
        self.assertEqual(love["formats"], ["JPEG"])
        self.assertEqual(love["status"], "processed")

    def test_hidden_ignored_and_symlink_roots_never_enter_picker(self) -> None:
        outside = self.base / "outside"
        _album(outside, "Linked Artist", "Linked Album")
        link = self.root / "linked"
        try:
            os.symlink(outside, link, target_is_directory=True)
        except (OSError, NotImplementedError):
            link = None

        (_result, emitted) = self._run(
            [{"action": "launch", "scan_mode": "auto-selected", "selected": []}]
        )
        payload = next(payload for event, payload in emitted if event == "library")
        names = {row["name"] for row in payload["artists"]}
        self.assertEqual(names, {"10,000 Maniacs", "Aerosmith"})
        if link is not None:
            self.assertNotIn("linked", names)

    def test_large_root_folder_list_never_touches_album_topology(self) -> None:
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
            mock.patch.object(splined.os, "scandir", return_value=Entries(values)) as scandir,
            mock.patch.object(
                splined,
                "inventory",
                side_effect=AssertionError("Album topology read"),
            ),
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

    def test_post_launch_reads_every_authoritative_track(self) -> None:
        second = self.eden / "track02.flac"
        second.write_bytes(b"audio")
        album = splined.AlbumDir(self.eden, [self.eden / "track01.flac", second])
        tracks = [
            splined.Track(
                path,
                path.stem,
                "Artist",
                "Album",
                "Artist",
                None,
                None,
                None,
            )
            for path in album.audio_files
        ]
        with mock.patch.object(splined, "read_track", side_effect=tracks) as reader:
            self.assertEqual(splined.read_album_tracks(album), tracks)
        self.assertEqual(reader.call_count, 2)


if __name__ == "__main__":
    unittest.main()
