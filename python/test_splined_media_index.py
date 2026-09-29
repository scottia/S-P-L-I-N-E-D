from __future__ import annotations

import inspect
import json
from pathlib import Path
import sqlite3
import tempfile
from types import SimpleNamespace
import unittest

from splined_media_database import _insert_snapshot, database_path, schema_path
from splined_media_index import _resident_session_usable
from splined_media_runtime import clean_transient_cache, stats_from_row
from splined_media_tags import album_base_key, artist_key


class SplinedMediaIndexTests(unittest.TestCase):
    def test_database_path_uses_configured_cache_directory(self) -> None:
        self.assertEqual(
            database_path(Path("/_cache")),
            Path("/_cache/splined.db"),
        )

    def test_retained_session_requires_same_ready_library_and_database(self) -> None:
        session = SimpleNamespace(
            ready=True,
            library_root="/music",
            picker_path="/_cache/splined.db",
        )
        self.assertTrue(
            _resident_session_usable(
                session,
                library_root=Path("/music"),
                database=Path("/_cache/splined.db"),
            )
        )
        session.ready = False
        self.assertFalse(
            _resident_session_usable(
                session,
                library_root=Path("/music"),
                database=Path("/_cache/splined.db"),
            )
        )
        session.ready = True
        session.picker_path = "/_cache/other.db"
        self.assertFalse(
            _resident_session_usable(
                session,
                library_root=Path("/music"),
                database=Path("/_cache/splined.db"),
            )
        )

    def test_cache_cleanup_preserves_sqlite_database_family(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            preserved = {
                "splined.db",
                "splined.db-wal",
                "splined.db-shm",
                "splined.db-journal",
                "splined.db.corrupt-20260928-120000",
                "splined.db.schema-v2-20260928-120000",
            }
            for name in preserved:
                (cache / name).write_bytes(b"persistent")

            disposable_file = cache / "candidate.jpg"
            disposable_file.write_bytes(b"temporary")
            disposable_dir = cache / "samples"
            disposable_dir.mkdir()
            (disposable_dir / "sample.jpg").write_bytes(b"temporary")

            core = type("Core", (), {"SplinedError": RuntimeError})
            clean_transient_cache(core, cache)

            self.assertEqual(
                {path.name for path in cache.iterdir()},
                preserved,
            )
            self.assertFalse(disposable_file.exists())
            self.assertFalse(disposable_dir.exists())

    def test_tag_keys_do_not_depend_on_absolute_paths(self) -> None:
        artist = artist_key("Aaliyah", "artist-mbid")
        first = album_base_key(
            artist,
            "One in a Million",
            "release-mbid",
            "group-mbid",
            "1996",
            False,
        )
        second = album_base_key(
            artist,
            "Renamed Folder Does Not Matter",
            "release-mbid",
            "group-mbid",
            "1996",
            False,
        )
        self.assertEqual(artist, "mbid:artist-mbid")
        self.assertEqual(first, second)
        self.assertEqual(first, "mbid:release-mbid")

    def test_schema_indexes_artist_and_album_values_not_paths(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            connection = sqlite3.connect(Path(directory) / "splined.db")
            connection.executescript(schema_path().read_text(encoding="utf-8"))

            def indexed_columns(table: str) -> set[str]:
                columns: set[str] = set()
                for index_row in connection.execute(
                    f"PRAGMA index_list({table})"
                ):
                    index_name = str(index_row[1]).replace("'", "''")
                    columns.update(
                        str(column_row[2])
                        for column_row in connection.execute(
                            f"PRAGMA index_info('{index_name}')"
                        )
                    )
                return columns

            artist_columns = indexed_columns("artists")
            album_columns = indexed_columns("albums")
            retired_columns = indexed_columns("retired_album_paths")
            self.assertIn("artist_key", artist_columns)
            self.assertIn("album_key", album_columns)
            self.assertIn("artist_key", album_columns)
            self.assertIn("status", album_columns)
            self.assertNotIn("primary_path", artist_columns)
            self.assertNotIn("path", album_columns)
            self.assertNotIn("representative_file", album_columns)
            self.assertNotIn("cover_path", album_columns)
            self.assertNotIn("album_path", retired_columns)
            connection.close()

    def test_retired_path_upsert_conflicts_on_logical_album_key_only(self) -> None:
        source = inspect.getsource(_insert_snapshot)
        self.assertIn('"ON CONFLICT(album_key) DO UPDATE SET "', source)
        self.assertNotIn('"ON CONFLICT(album_key, album_path)', source)

    def test_navtagger_style_maintenance_tables_are_present(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            connection = sqlite3.connect(Path(directory) / "splined.db")
            connection.executescript(schema_path().read_text(encoding="utf-8"))
            names = {
                str(row[0])
                for row in connection.execute(
                    "SELECT name FROM sqlite_master WHERE type='table'"
                )
            }
            self.assertTrue(
                {
                    "cache_entries",
                    "cache_history",
                    "db_maintenance_state",
                    "picker_inventory",
                    "retired_album_paths",
                    "artists",
                    "albums",
                }.issubset(names)
            )
            connection.close()

    def test_album_statistics_are_materialized_from_database_row(self) -> None:
        row = {
            "path": "/music/Aaliyah/One in a Million",
            "album_name": "One in a Million",
            "artist_name": "Aaliyah",
            "release_year": "1996",
            "track_count": 17,
            "artwork_jpeg": 1,
            "artwork_png": 0,
            "artwork_webp": 0,
            "artwork_other": 1,
            "root_files": 2,
            "cover_files": 1,
            "cover_names_json": json.dumps(["Cover.jpeg"]),
            "cover_width": 1800,
            "cover_height": 1800,
            "cover_path": "/music/Aaliyah/One in a Million/Cover.jpeg",
            "other_filenames_json": json.dumps(["playlist.m3u"]),
            "webp_found": 0,
            "webp_size_mb": 0,
            "webp_resolution": "",
            "webp_conversion": 0,
        }
        stats = stats_from_row(row)
        self.assertEqual(stats["artist"], "Aaliyah")
        self.assertEqual(stats["cover_resolution"], "1800x1800")
        self.assertEqual(stats["cover_names"], ["Cover.jpeg"])


if __name__ == "__main__":
    unittest.main()
