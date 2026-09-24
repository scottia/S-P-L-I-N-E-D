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
from tui.library import AlbumStatus, LibraryModel


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
        "sources": {
            "cover_sources": ["itunes"],
            "exclude_cover_sources": [],
        },
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
    (directory / f"track01{suffix}").write_bytes(b"not parsed during inventory")
    return directory


class LightweightInventoryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / "music"
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

    def test_recursive_inventory_matches_windows_folder_model_and_cover_detection(self) -> None:
        albums, ignored = splined.inventory(
            self.root,
            list(self.cfg["library"]["ignored_subs"]),  # type: ignore[index]
            "cover",
        )
        self.assertEqual(
            [album.path for album in albums],
            [self.love, self.eden, self.toys],
        )
        self.assertEqual(albums[0].local_art_files, [self.love / "cover.jpg"])
        self.assertEqual(
            {path.name for path in ignored},
            {"[Artist Singles]", "[videos]", "@eaDir", ".stfolder-cache"},
        )

    def test_ignored_exact_and_wildcard_directories_are_never_traversed(self) -> None:
        visited: list[Path] = []
        original = splined.os.scandir

        def recording(path: Path | str):
            visited.append(Path(path))
            return original(path)

        with mock.patch.object(splined.os, "scandir", recording):
            albums, _ = splined.inventory(
                self.root,
                list(self.cfg["library"]["ignored_subs"]),  # type: ignore[index]
            )
        ignored_roots = {
            self.root / "[Artist Singles]",
            self.root / "[videos]",
            self.root / "@eaDir",
            self.root / ".stfolder-cache",
        }
        self.assertTrue(ignored_roots.isdisjoint(visited))
        self.assertTrue(
            all(not any(root in album.path.parents for root in ignored_roots) for album in albums)
        )

    def test_symlink_directory_is_not_followed(self) -> None:
        outside = Path(self.temp.name) / "outside"
        _album(outside, "Linked Artist", "Linked Album")
        link = self.root / "linked"
        try:
            os.symlink(outside, link, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest("directory symlinks are unavailable in this runner")
        albums, _ = splined.inventory(self.root, [])
        self.assertNotIn("Linked Album", {album.path.name for album in albums})

    def _selection_payload(
        self,
        albums: list[splined.AlbumDir],
        history: dict[str, object],
        *,
        bypassed: set[str] | None = None,
    ) -> dict[str, object]:
        emitted: list[tuple[str, dict[str, object]]] = []
        response = json.dumps(
            {"action": "launch", "scan_mode": "auto-selected", "selected": []}
        )
        with (
            mock.patch.object(splined, "tui_active", return_value=True),
            mock.patch.object(
                splined, "emit_ui", side_effect=lambda event, **payload: emitted.append((event, payload))
            ),
            mock.patch.object(splined, "read_input", return_value=response),
            mock.patch.object(splined, "enrich_representative_tracks", return_value=None),
            mock.patch.object(splined, "lookup_release", side_effect=AssertionError("MusicBrainz")),
            mock.patch.object(splined, "discover_all", side_effect=AssertionError("providers")),
            mock.patch.object(splined, "download_candidates", side_effect=AssertionError("downloads")),
            mock.patch.object(splined, "select_best", side_effect=AssertionError("ranking")),
            mock.patch.object(splined.Image, "open", side_effect=AssertionError("image decode")),
        ):
            selected, track_cache, _, _, _ = splined.prepare_tui_library_selection(
                self.root / "config.toml",
                self.cfg,
                ["itunes"],
                self.root,
                albums,
                history,
                24,
                bypassed_paths=bypassed,
            )
        self.assertEqual(selected, [])
        self.assertEqual(track_cache, {})
        return next(payload for event, payload in emitted if event == "library")

    def test_prelaunch_uses_only_filesystem_and_history_authority(self) -> None:
        albums, _ = splined.inventory(self.root, list(self.cfg["library"]["ignored_subs"]))  # type: ignore[index]
        love = next(album for album in albums if album.path == self.love)
        eden = next(album for album in albums if album.path == self.eden)
        history = {
            "version": 1,
            "albums": {
                str(eden.path): {
                    "completed_at_unix": time.time(),
                    "album_fingerprint": splined.album_scan_fingerprint(eden),
                    "policy_fingerprint": splined.scan_policy_fingerprint(self.cfg, ["itunes"]),
                    "outcome": "selected",
                }
            },
        }
        payload = self._selection_payload(
            albums,
            history,
            bypassed={str(self.toys)},
        )
        rows = {row["path"]: row for row in payload["albums"]}  # type: ignore[index]
        self.assertEqual(rows[str(love.path)]["status"], "processed")
        self.assertEqual(rows[str(eden.path)]["status"], "timeout")
        self.assertEqual(rows[str(self.toys)]["status"], "bypassed")
        self.assertEqual(rows[str(love.path)]["artist"], "10,000 Maniacs")
        self.assertEqual(rows[str(love.path)]["album"], "Love Among the Ruins")
        self.assertEqual(rows[str(love.path)]["formats"], ["JPEG"])
        self.assertNotIn("Ignored Album", {row["album"] for row in payload["albums"]})  # type: ignore[index]

        model = LibraryModel.from_payload(payload)
        self.assertEqual(model.statistics()["artists"], 2)
        self.assertEqual(model.statistics()["albums"], 3)
        self.assertEqual(model.selection_payload("auto-selected")["selected"], [])
        self.assertEqual(
            {item.status for item in model.albums},
            {AlbumStatus.PROCESSED, AlbumStatus.TIMEOUT, AlbumStatus.BYPASSED},
        )

    def test_representative_tags_start_only_after_library_model_is_visible(self) -> None:
        albums, _ = splined.inventory(self.root, [])
        emitted: list[tuple[str, dict[str, object]]] = []
        started = threading.Event()
        stopped = threading.Event()

        def background(
            values: list[splined.AlbumDir],
            cancel: threading.Event,
            on_result,
            *,
            workers: int = splined.TAG_ENRICHMENT_WORKERS,
        ) -> None:
            self.assertTrue(any(event == "library" for event, _ in emitted))
            self.assertEqual(values, albums)
            started.set()
            cancel.wait(2)
            stopped.set()

        response = json.dumps(
            {"action": "launch", "scan_mode": "auto-selected", "selected": []}
        )
        with (
            mock.patch.object(splined, "tui_active", return_value=True),
            mock.patch.object(
                splined,
                "emit_ui",
                side_effect=lambda event, **payload: emitted.append((event, payload)),
            ),
            mock.patch.object(splined, "read_input", return_value=response),
            mock.patch.object(splined, "enrich_representative_tracks", side_effect=background),
        ):
            selected, cached, _, _, _ = splined.prepare_tui_library_selection(
                self.root / "config.toml",
                self.cfg,
                ["itunes"],
                self.root,
                albums,
                {"version": 1, "albums": {}},
                24,
            )

        self.assertEqual(selected, [])
        self.assertEqual(cached, {})
        self.assertTrue(started.wait(1))
        self.assertTrue(stopped.wait(1))

    def test_representative_tag_event_updates_model_and_returns_run_cache(self) -> None:
        albums, _ = splined.inventory(self.root, [])
        target = next(album for album in albums if album.path == self.eden)
        representative = target.audio_files[0]
        stat = representative.stat()
        mbid = "1b022e01-4da6-387b-8658-8678046e4cef"
        track = splined.Track(
            representative,
            "Representative Track",
            "Track Artist",
            "Tagged Album",
            "Tagged Album Artist",
            mbid,
            None,
            None,
        )
        indexed = splined.IndexedTrack(track, stat.st_size, stat.st_mtime_ns)
        emitted: list[tuple[str, dict[str, object]]] = []
        enriched = threading.Event()

        def background(values, cancel, on_result, *, workers=4) -> None:
            on_result(target, indexed, None)

        def emit(event: str, **payload: object) -> None:
            emitted.append((event, payload))
            if event == "library_enrichment":
                enriched.set()

        def respond(*_args, **_kwargs) -> str:
            self.assertTrue(enriched.wait(1))
            return json.dumps(
                {
                    "action": "launch",
                    "scan_mode": "auto-selected",
                    "selected": [str(target.path)],
                }
            )

        with (
            mock.patch.object(splined, "tui_active", return_value=True),
            mock.patch.object(splined, "emit_ui", side_effect=emit),
            mock.patch.object(splined, "read_input", side_effect=respond),
            mock.patch.object(splined, "enrich_representative_tracks", side_effect=background),
        ):
            selected, cached, _, _, _ = splined.prepare_tui_library_selection(
                self.root / "config.toml",
                self.cfg,
                ["itunes"],
                self.root,
                albums,
                {"version": 1, "albums": {}},
                24,
            )

        self.assertEqual(selected, [target])
        self.assertIs(cached[str(target.path)].track, track)
        payload = next(
            payload for event, payload in emitted if event == "library_enrichment"
        )
        update = payload["items"][0]  # type: ignore[index]
        self.assertEqual(update["artist"], "Tagged Album Artist")
        self.assertEqual(update["album"], "Tagged Album")
        self.assertEqual(update["album_mbid"], mbid)

        model = LibraryModel.from_payload(
            next(payload for event, payload in emitted if event == "library")
        )
        item = next(value for value in model.albums if value.path == str(target.path))
        original_status = item.status
        original_selected = item.selected
        self.assertTrue(
            model.apply_tag_enrichment(
                str(target.path),
                artist="Tagged Album Artist",
                album="Tagged Album",
                album_mbid=mbid,
            )
        )
        self.assertEqual((item.artist, item.title), ("Tagged Album Artist", "Tagged Album"))
        self.assertEqual((item.status, item.selected), (original_status, original_selected))
        self.assertEqual(model.statistics()["musicbrainz"], 1)

    def test_post_launch_reuses_only_an_unchanged_representative_track(self) -> None:
        first_path = self.eden / "track01.flac"
        second_path = self.eden / "track02.flac"
        second_path.write_bytes(b"audio-two")
        album = splined.AlbumDir(self.eden, [first_path, second_path])
        stat = first_path.stat()
        cached_track = splined.Track(
            first_path,
            "Cached",
            "Artist",
            "Album",
            "Artist",
            None,
            None,
            None,
        )
        indexed = splined.IndexedTrack(cached_track, stat.st_size, stat.st_mtime_ns)

        def parsed(path: Path) -> splined.Track:
            return splined.Track(path, path.stem, "Artist", "Album", "Artist", None, None, None)

        with mock.patch.object(splined, "read_track", side_effect=parsed) as reader:
            tracks = splined.read_album_tracks(album, {str(album.path): indexed})
        self.assertIs(tracks[0], cached_track)
        reader.assert_called_once_with(second_path)

        first_path.write_bytes(b"audio-now-changed")
        with mock.patch.object(splined, "read_track", side_effect=parsed) as reader:
            tracks = splined.read_album_tracks(album, {str(album.path): indexed})
        self.assertEqual([track.path for track in tracks], [first_path, second_path])
        self.assertEqual(reader.call_count, 2)

    def test_representative_tag_enrichment_is_bounded(self) -> None:
        albums = [
            splined.AlbumDir(
                self.root / f"Tagged Artist {index:02d}" / "Album",
                [self.root / f"Tagged Artist {index:02d}" / "Album" / "track.mp3"],
            )
            for index in range(12)
        ]
        lock = threading.Lock()
        active = 0
        maximum_active = 0
        results: list[str] = []

        def index(album: splined.AlbumDir) -> splined.IndexedTrack:
            nonlocal active, maximum_active
            with lock:
                active += 1
                maximum_active = max(maximum_active, active)
            try:
                time.sleep(0.01)
                path = album.audio_files[0]
                return splined.IndexedTrack(
                    splined.Track(path, "Track", "Artist", "Album", "Artist", None, None, None),
                    1,
                    1,
                )
            finally:
                with lock:
                    active -= 1

        with mock.patch.object(splined, "index_representative_track", side_effect=index):
            splined.enrich_representative_tracks(
                albums,
                threading.Event(),
                lambda album, indexed, error: results.append(str(album.path)),
                workers=4,
            )

        self.assertEqual(len(results), len(albums))
        self.assertGreater(maximum_active, 1)
        self.assertLessEqual(maximum_active, 4)

    def test_local_art_presence_never_requires_image_decode(self) -> None:
        with mock.patch.object(splined.Image, "open", side_effect=AssertionError("decoded")):
            albums, _ = splined.inventory(self.root, [])
        love = next(album for album in albums if album.path == self.love)
        self.assertEqual(love.local_art_files, [self.love / "cover.jpg"])

    def test_recent_timeout_fingerprint_is_collected_during_scandir(self) -> None:
        policy = splined.scan_policy_fingerprint(self.cfg, ["itunes"])
        initial, _ = splined.inventory(self.root, [])
        eden = next(album for album in initial if album.path == self.eden)
        expected = splined.album_scan_fingerprint(eden)
        history = {
            "version": 1,
            "albums": {
                str(self.eden): {
                    "completed_at_unix": time.time(),
                    "album_fingerprint": expected,
                    "policy_fingerprint": policy,
                    "outcome": "selected",
                }
            },
        }
        fingerprint_paths = splined.timeout_fingerprint_paths(
            history, self.cfg, ["itunes"], 24
        )
        self.assertEqual(fingerprint_paths, {str(self.eden)})
        cached, _ = splined.inventory(
            self.root,
            [],
            fingerprint_paths=fingerprint_paths,
        )
        cached_eden = next(album for album in cached if album.path == self.eden)
        self.assertEqual(cached_eden.inventory_fingerprint, expected)
        with mock.patch.object(
            Path, "stat", side_effect=AssertionError("duplicate track stat")
        ):
            postponed, _ = splined.scan_completion_status(
                history,
                cached_eden,
                self.cfg,
                ["itunes"],
                24,
                policy_fingerprint=policy,
            )
        self.assertTrue(postponed)

    def test_expired_or_policy_mismatched_history_needs_no_track_metadata(self) -> None:
        history = {
            "version": 1,
            "albums": {
                str(self.love): {
                    "completed_at_unix": time.time() - 25 * 3600,
                    "policy_fingerprint": splined.scan_policy_fingerprint(
                        self.cfg, ["itunes"]
                    ),
                },
                str(self.eden): {
                    "completed_at_unix": time.time(),
                    "policy_fingerprint": "different-policy",
                },
            },
        }
        self.assertEqual(
            splined.timeout_fingerprint_paths(
                history, self.cfg, ["itunes"], 24
            ),
            set(),
        )

    def test_inventory_reports_bounded_live_progress(self) -> None:
        progress: list[tuple[int, int]] = []
        callback_threads: list[int] = []

        def record_progress(directories: int, count: int) -> None:
            progress.append((directories, count))
            callback_threads.append(threading.get_ident())

        albums, _ = splined.inventory(
            self.root,
            list(self.cfg["library"]["ignored_subs"]),  # type: ignore[index]
            progress=record_progress,
        )
        self.assertTrue(progress)
        self.assertEqual(progress[-1][1], len(albums))
        self.assertLessEqual(len(progress), 1 + progress[-1][0] // 250)
        self.assertEqual(set(callback_threads), {threading.get_ident()})

    def test_inventory_bounds_parallel_directory_enumeration_and_order(self) -> None:
        for index in range(16):
            album = self.root / f"Parallel Artist {index:02d}" / "Album"
            album.mkdir(parents=True)
            (album / "track01.flac").write_bytes(b"audio")

        expected, expected_ignored = splined.inventory(
            self.root,
            list(self.cfg["library"]["ignored_subs"]),  # type: ignore[index]
            workers=1,
        )
        original = splined.os.scandir
        lock = threading.Lock()
        active = 0
        maximum_active = 0

        def delayed_scandir(path: Path | str):
            nonlocal active, maximum_active
            with lock:
                active += 1
                maximum_active = max(maximum_active, active)
            try:
                time.sleep(0.01)
                return original(path)
            finally:
                with lock:
                    active -= 1

        with mock.patch.object(splined.os, "scandir", delayed_scandir):
            actual, actual_ignored = splined.inventory(
                self.root,
                list(self.cfg["library"]["ignored_subs"]),  # type: ignore[index]
                workers=4,
            )

        self.assertGreater(maximum_active, 1)
        self.assertLessEqual(maximum_active, 4)
        self.assertEqual(
            [(album.path, album.audio_files) for album in actual],
            [(album.path, album.audio_files) for album in expected],
        )
        self.assertEqual(actual_ignored, expected_ignored)


if __name__ == "__main__":
    unittest.main()
