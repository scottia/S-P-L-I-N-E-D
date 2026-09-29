from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from types import SimpleNamespace
import tempfile
import threading
import unittest
from unittest.mock import patch

from splined_media_database import connect, utc_now
import splined_media_runtime as runtime
from splined_media_warm_start_policy import (
    cached_stats_readthrough,
    populate_session_readonly,
)


@dataclass
class PickerArtist:
    path: str
    name: str
    loaded: bool = False
    modified: float | None = None


@dataclass
class PickerAlbum:
    path: str
    artist_path: str
    name: str
    local_art: tuple[str, ...]
    inventory_fingerprint: str | None


@dataclass
class AlbumDir:
    path: Path
    audio_files: list[Path]
    local_art_files: list[Path]
    inventory_fingerprint: str | None


class DummySession:
    def __init__(self) -> None:
        self.lock = threading.RLock()
        self.artists = []
        self.album_records = []
        self.loaded_artists = set()
        self.probed_artist_statuses = {}
        self.selected_paths = set()
        self.selected_statistics = {}
        self.selected_statistics_pending = set()
        self.status_probe_started = False
        self.status_probe_complete = False
        self.status_probe_thread = None
        self.status_probe_cancel = threading.Event()
        self.ready = False
        self.library_root = ""
        self.picker_path = ""
        self.initialized_paths = set()


class WarmStartPolicyTests(unittest.TestCase):
    def test_physical_artist_folder_uses_library_top_level(self) -> None:
        self.assertEqual(
            runtime.physical_artist_folder(
                "/music",
                "/music/Christina Aguilera/AGUILERA",
            ),
            "/music/Christina Aguilera",
        )
        self.assertEqual(
            runtime.physical_artist_folder(
                "/music",
                "/music/[Soundtracks]/A Star Is Born",
            ),
            "/music/[Soundtracks]",
        )
        self.assertEqual(
            runtime.physical_artist_folder(
                "/music",
                "/music/[Various Artists]/[Various Artists]/Pop/Album",
            ),
            "/music/[Various Artists]",
        )

    def test_picker_statuses_are_isolated_by_physical_folder(self) -> None:
        rows = [
            {"artist_path": "/music/Christina Aguilera", "status": "processed"},
            {"artist_path": "/music/Christina Aguilera", "status": "unprocessed"},
            {"artist_path": "/music/[Soundtracks]", "status": "processed"},
        ]
        self.assertEqual(
            runtime.project_picker_folder_statuses(rows),
            {
                "/music/Christina Aguilera": "partial",
                "/music/[Soundtracks]": "complete",
            },
        )

    def tearDown(self) -> None:
        runtime._ACTIVE_DB_PATH = None
        runtime._PATH_TO_ALBUM_KEY = {}
        runtime._ARTIST_BY_ALBUM_KEY = {}
        runtime._STATUS_BY_PATH = {}
        runtime._STATS_BY_PATH = {}

    def test_populate_session_uses_compact_rows_and_lazy_statistics(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            database_path = root / "splined.db"
            connection = connect(database_path, "test")
            now = utc_now()
            authority_path = root / "music" / "[Soundtracks]"
            artist_path = root / "music" / "Christina Aguilera"
            album_path = artist_path / "AGUILERA"

            with connection:
                connection.execute(
                    "INSERT INTO artists"
                    "(artist_key, artist_name, artist_sort, "
                    "musicbrainz_artistid, primary_path, status, album_count, "
                    "unprocessed_count, processed_count, bypassed_count, "
                    "timeout_count, created_at, updated_at, last_seen_at, "
                    "splined_version) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    (
                        "mbid:artist",
                        "Authority Artist",
                        "Authority Artist",
                        "artist",
                        str(authority_path),
                        "unprocessed",
                        1,
                        1,
                        0,
                        0,
                        0,
                        now,
                        now,
                        now,
                        "test",
                    ),
                )
                connection.execute(
                    "INSERT INTO artists"
                    "(artist_key, artist_name, artist_sort, "
                    "musicbrainz_artistid, primary_path, status, album_count, "
                    "unprocessed_count, processed_count, bypassed_count, "
                    "timeout_count, created_at, updated_at, last_seen_at, "
                    "splined_version) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    (
                        "mbid:guest",
                        "Guest Authority",
                        "Guest Authority",
                        "guest",
                        str(authority_path),
                        "unprocessed",
                        0,
                        0,
                        0,
                        0,
                        0,
                        now,
                        now,
                        now,
                        "test",
                    ),
                )
                connection.execute(
                    "INSERT INTO albums"
                    "(album_key, artist_key, album_name, album_sort, "
                    "musicbrainz_albumid, musicbrainz_releasegroupid, "
                    "release_year, compilation, path, representative_file, "
                    "representative_size, representative_mtime_ns, "
                    "tag_signature, track_count, inventory_fingerprint, "
                    "status, cover_required, cover_found, cover_path, "
                    "cover_name, cover_format, cover_width, cover_height, "
                    "artwork_jpeg, artwork_png, artwork_webp, artwork_other, "
                    "root_files, cover_files, cover_names_json, "
                    "local_art_json, other_filenames_json, webp_found, "
                    "webp_size_mb, webp_resolution, webp_conversion, "
                    "processed_at, bypassed, timeout_until, selected_source, "
                    "created_at, updated_at, last_seen_at, splined_version) "
                    "VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    (
                        "mbid:album",
                        "mbid:artist",
                        "Album",
                        "Album",
                        "album",
                        "group",
                        "2026",
                        0,
                        str(album_path),
                        str(album_path / "01.flac"),
                        1,
                        1,
                        "tag",
                        10,
                        None,
                        "unprocessed",
                        1,
                        0,
                        "",
                        "",
                        "",
                        None,
                        None,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        "[]",
                        "[]",
                        "[]",
                        0,
                        0.0,
                        "",
                        0,
                        "",
                        0,
                        "",
                        "",
                        now,
                        now,
                        now,
                        "test",
                    ),
                )

            events: list[tuple[str, dict]] = []
            logs: list[str] = []
            core = SimpleNamespace(
                PickerArtist=PickerArtist,
                PickerAlbum=PickerAlbum,
                AlbumDir=AlbumDir,
                emit_ui=lambda event, **payload: events.append((event, payload)),
                debug_log=lambda message: logs.append(message),
                scan_policy_fingerprint=lambda _cfg, _sources: "policy",
                scan_completion_status=lambda *_args, **_kwargs: (False, 0.0),
            )
            session = DummySession()
            context = SimpleNamespace(
                core=core,
                cfg={},
                sources=[],
                completion_history={"albums": {}},
                timeout_hours=0.0,
                bypassed_paths=set(),
                library_root=root / "music",
                db_path=database_path,
                session=session,
            )

            statements: list[str] = []
            connection.set_trace_callback(statements.append)
            with patch.dict(
                "os.environ",
                {"SPLINED_SQLITE_LOCAL_SNAPSHOT": "never"},
            ):
                populate_session_readonly(context, connection)
            connection.set_trace_callback(None)

            mutating = [
                statement
                for statement in statements
                if statement.lstrip().upper().startswith(
                    ("UPDATE ", "INSERT ", "DELETE ", "REPLACE ")
                )
            ]
            album_reads = [
                statement
                for statement in statements
                if "FROM ALBUMS" in statement.upper()
            ]
            self.assertEqual(mutating, [])
            self.assertTrue(album_reads)
            self.assertTrue(
                all("JOIN ARTISTS" not in statement.upper() for statement in album_reads)
            )
            self.assertTrue(
                all("ALBUMS.*" not in statement.upper() for statement in album_reads)
            )
            self.assertTrue(
                all("ORDER BY" not in statement.upper() for statement in album_reads)
            )
            self.assertTrue(session.ready)
            self.assertEqual(len(session.artists), 1)
            self.assertEqual(session.artists[0].name, "Christina Aguilera")
            self.assertEqual(session.loaded_artists, {str(artist_path)})
            self.assertEqual(len(session.album_records), 1)
            self.assertEqual(session.album_records[0].artist_path, str(artist_path))
            self.assertEqual(runtime._STATS_BY_PATH, {})
            self.assertEqual(
                session.probed_artist_statuses[str(artist_path)],
                "unprocessed",
            )
            self.assertTrue(
                any("warm_load.artist_rows" in message for message in logs)
            )
            self.assertTrue(
                any(
                    "authority_artists=2" in message
                    and "picker_folders=1" in message
                    for message in logs
                )
            )
            self.assertTrue(
                any("warm_load.album_rows" in message for message in logs)
            )
            self.assertTrue(
                any("warm_load.done" in message for message in logs)
            )
            self.assertTrue(
                any(
                    event == "cache_progress"
                    and payload.get("phase") == "ready"
                    for event, payload in events
                )
            )

            runtime._ACTIVE_DB_PATH = database_path
            stats = cached_stats_readthrough(album_path)
            self.assertIsNotNone(stats)
            self.assertEqual(stats["album"], "Album")
            self.assertEqual(stats["tracks"], 10)
            self.assertIn(str(album_path), runtime._STATS_BY_PATH)
            connection.close()


if __name__ == "__main__":
    unittest.main()
