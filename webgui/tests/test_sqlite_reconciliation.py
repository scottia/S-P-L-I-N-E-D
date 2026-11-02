"""Safety and contract tests for incremental SQLite reconciliation."""

from __future__ import annotations

from contextlib import closing
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch
import os
import sqlite3
import unittest

from webgui.reconciliation import (
    SQLiteReconciler,
    classify_difference,
    map_library_path,
)


class SQLiteReconciliationTests(unittest.TestCase):
    def test_windows_snapshot_paths_require_and_apply_explicit_root_mapping(self):
        mapped = map_library_path(
            r"D:\Music\Artist\Album\01.flac",
            r"D:\Music",
            r"E:\WebGUI Music",
        )
        normalized = mapped.replace("\\", "/")
        self.assertTrue(normalized.endswith("WebGUI Music/Artist/Album/01.flac"))
        with self.assertRaisesRegex(ValueError, "outside the mapped source"):
            map_library_path(r"D:\Other\01.flac", r"D:\Music", r"E:\Music")

    def test_path_backed_track_key_is_mapped_with_track_path(self):
        source = {
            "track_key": r"D:\Music\Adele\30\01.flac",
            "path": r"D:\Music\Adele\30\01.flac",
        }
        mapped = SQLiteReconciler._map_row(
            "tracks", source, r"D:\Music", r"E:\WebGUI Music"
        )
        self.assertEqual(mapped["track_key"], mapped["path"])
        self.assertTrue(
            mapped["track_key"].replace("\\", "/").endswith(
                "WebGUI Music/Adele/30/01.flac"
            )
        )

    def test_track_identity_never_falls_back_to_another_track_by_artist_name(self):
        target = {
            "track_key": r"E:\Music\Billboard\Adele.mp3",
            "path": r"E:\Music\Billboard\Adele.mp3",
            "artist_name": "Adele",
            "album_key": "billboard",
        }
        maps = SQLiteReconciler._match_maps("tracks", [target])
        source = {
            "track_key": r"E:\Music\Adele\30\01.mp3",
            "path": r"E:\Music\Adele\30\01.mp3",
            "artist_name": "Adele",
            "album_key": "30",
        }
        self.assertIsNone(SQLiteReconciler._find_match("tracks", source, maps))

    def test_snapshot_fingerprint_ignores_sqlite_read_sidecars(self):
        with TemporaryDirectory() as directory:
            snapshot = Path(directory) / "snapshot.sqlite"
            snapshot.write_bytes(b"standalone snapshot")
            before = SQLiteReconciler._fingerprint(snapshot)
            Path(str(snapshot) + "-shm").write_bytes(b"reader state")
            Path(str(snapshot) + "-wal").write_bytes(b"")
            self.assertEqual(SQLiteReconciler._fingerprint(snapshot), before)

    def test_album_without_mbid_can_match_only_within_its_artist_identity(self):
        target = {
            "album_key": "target-key",
            "artist_key": "adele",
            "album_name": "30",
            "path": r"E:\Music\Adele\30",
        }
        maps = SQLiteReconciler._match_maps("albums", [target])
        self.assertIs(
            SQLiteReconciler._find_match(
                "albums",
                {
                    "album_key": "source-key",
                    "artist_key": "adele",
                    "album_name": "30",
                    "path": r"E:\Other\30",
                },
                maps,
            ),
            target,
        )
        self.assertIsNone(
            SQLiteReconciler._find_match(
                "albums",
                {
                    "album_key": "other-source-key",
                    "artist_key": "other-artist",
                    "album_name": "30",
                    "path": r"E:\Other\Other Artist\30",
                },
                maps,
            )
        )

    def test_only_matched_newer_catalog_authority_is_safe(self):
        newer = {"updated_at": "2026-01-02T00:00:00+00:00"}
        older = {"updated_at": "2026-01-01T00:00:00+00:00"}
        self.assertEqual(classify_difference("albums", newer, older), "safe")
        self.assertEqual(classify_difference("albums", older, newer), "conflict")
        self.assertEqual(classify_difference("artists", newer, None), "conflict")
        self.assertEqual(classify_difference("albums", newer, None), "conflict")
        self.assertEqual(classify_difference("tracks", newer, None), "conflict")
        self.assertEqual(classify_difference("cache_entries", newer, None), "conflict")
        self.assertEqual(
            classify_difference("compilation_album_progress", newer, older),
            "derived",
        )

    def test_windows_embed_queue_completion_projects_webgui_embedded_count(self):
        mapped = SQLiteReconciler._map_row(
            "albums",
            {
                "album_key": "album",
                "path": r"D:\Music\Artist\Album",
                "status": "processed",
                "processed_at": "2026-01-02T00:00:00Z",
                "selected_source": "embed-queue",
                "track_count": 10,
                "compilation": 0,
            },
            r"D:\Music",
            r"E:\WebGUI Music",
        )
        self.assertEqual(mapped["embedded_tracks"], 10)

    def test_identified_compilation_projects_normal_album_completion(self):
        mapped = SQLiteReconciler._map_row(
            "albums",
            {
                "album_key": "identified-compilation",
                "path": r"D:\Music\Aaron Neville\Gold",
                "musicbrainz_albumid": "308a6440-3c1c-448c-85ce-84b7e53cc756",
                "status": "processed",
                "processed_at": "2026-11-02T05:58:16Z",
                "selected_source": "embed-queue",
                "track_count": 36,
                "compilation": 1,
            },
            r"D:\Music",
            r"E:\WebGUI Music",
        )
        self.assertEqual(mapped["embedded_tracks"], 36)

    def test_unidentified_compilation_keeps_comp_track_authority(self):
        mapped = SQLiteReconciler._map_row(
            "albums",
            {
                "album_key": "hidden-comp",
                "path": r"D:\Music\Various Artists\Hidden COMP",
                "musicbrainz_albumid": "",
                "status": "processed",
                "processed_at": "2026-11-02T05:58:16Z",
                "selected_source": "embed-queue",
                "track_count": 36,
                "compilation": 1,
            },
            r"D:\Music",
            r"E:\WebGUI Music",
        )
        self.assertNotIn("embedded_tracks", mapped)

    def test_equal_windows_embed_completion_is_safe_only_when_other_authority_matches(self):
        source = {
            "album_key": "album",
            "processed_at": "2026-01-02T00:00:00Z",
            "selected_source": "embed-queue",
            "track_count": 10,
            "embedded_tracks": 10,
            "timeout_until": None,
            "updated_at": "2026-01-02T00:00:00Z",
        }
        target = dict(source, embedded_tracks=0, timeout_until="")
        self.assertEqual(classify_difference("albums", source, target), "safe")
        self.assertEqual(
            classify_difference(
                "albums",
                source,
                dict(target, updated_at="2026-01-03T00:00:00Z"),
            ),
            "conflict",
        )
        self.assertEqual(
            classify_difference("albums", source, dict(target, cover_found=0)),
            "conflict",
        )

    def test_authorized_source_inventory_is_bounded_to_cache_database_files(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            snapshot = root / "snapshot.sqlite"
            with closing(sqlite3.connect(snapshot)) as database, database:
                database.execute("PRAGMA user_version=2")
            (root / "notes.txt").write_text("not a database", encoding="utf-8")
            reconciler = SQLiteReconciler(
                object(), root, root / "music", start_scheduler=False
            )
            try:
                self.assertEqual(
                    [item["label"] for item in reconciler.sources()],
                    ["snapshot.sqlite"],
                )
                self.assertEqual(reconciler._source(snapshot), snapshot.resolve())
                with TemporaryDirectory() as outside:
                    other = Path(outside) / "outside.db"
                    other.touch()
                    with self.assertRaises(PermissionError):
                        reconciler._source(other)
            finally:
                reconciler.close()

    def test_dedicated_snapshot_root_is_authorized_without_widening_filesystem_access(self):
        with TemporaryDirectory() as cache_directory, TemporaryDirectory() as snapshot_directory, TemporaryDirectory() as outside_directory:
            cache_root = Path(cache_directory)
            snapshot_root = Path(snapshot_directory)
            cache_snapshot = cache_root / "older-cache-copy.db"
            cache_snapshot.touch()
            snapshot = snapshot_root / "splined.db"
            with closing(sqlite3.connect(snapshot)) as database, database:
                database.execute("PRAGMA user_version=2")
            outside = Path(outside_directory) / "outside.db"
            outside.touch()
            reconciler = SQLiteReconciler(
                object(),
                cache_root,
                cache_root / "music",
                snapshot_roots=(snapshot_root,),
                start_scheduler=False,
            )
            try:
                sources = reconciler.sources()
                self.assertEqual(len(sources), 1)
                self.assertEqual(sources[0]["path"], str(snapshot.resolve()))
                self.assertEqual(
                    sources[0]["label"],
                    str(Path(snapshot_root.name) / "splined.db"),
                )
                self.assertEqual(sources[0]["bytes"], snapshot.stat().st_size)
                self.assertNotIn(str(cache_snapshot.resolve()), {item["path"] for item in sources})
                self.assertEqual(reconciler._source(snapshot), snapshot.resolve())
                self.assertEqual(
                    reconciler._source(cache_snapshot),
                    cache_snapshot.resolve(),
                )
                with self.assertRaises(PermissionError):
                    reconciler._source(outside)
            finally:
                reconciler.close()

    def test_snapshot_root_environment_is_explicit_and_container_default_is_bounded(self):
        from webgui.server import _sqlite_snapshot_roots

        with TemporaryDirectory() as directory, patch.dict(
            os.environ,
            {"SPLINED_SQLITE_SNAPSHOT_ROOT": directory},
        ):
            self.assertEqual(_sqlite_snapshot_roots(), (Path(directory),))

    def test_migration_has_exact_history_name_idempotence_key_and_disabled_schedule(self):
        migration = (
            Path(__file__).parents[1] / "migrations" / "010_sqlite_reconciliation.sql"
        ).read_text(encoding="utf-8")
        self.assertIn("CREATE TABLE salite_diff", migration)
        self.assertIn("UNIQUE (\n        source_fingerprint", migration)
        self.assertIn("enabled BOOLEAN NOT NULL DEFAULT FALSE", migration)
        self.assertIn("auto_apply_safe BOOLEAN NOT NULL DEFAULT FALSE", migration)
        startup = (
            Path(__file__).parents[1] / "migrations" / "012_reconciliation_startup_mode.sql"
        ).read_text(encoding="utf-8")
        self.assertIn("startup_mode TEXT NOT NULL DEFAULT 'off'", startup)
        self.assertIn("'compare', 'apply-safe'", startup)

    def test_ui_and_apply_contract_never_offer_conflict_application(self):
        root = Path(__file__).parents[1]
        operations = (root / "static" / "operations.js").read_text(encoding="utf-8")
        reconciliation = (root / "reconciliation.py").read_text(encoding="utf-8")
        self.assertIn("Compare / Dry Run", operations)
        self.assertIn("Save Snapshot & Paths", operations)
        self.assertIn("Saved SQLite snapshot and library path mapping", operations)
        self.assertIn("Apply Safe Differences", operations)
        self.assertIn("Auto SNAP on start", operations)
        self.assertIn('input.type = "radio"', operations)
        self.assertIn("Automatically apply safe differences only", operations)
        self.assertIn("Choose an authorized snapshot", operations)
        self.assertIn("not the folder containing the .db file", operations)
        self.assertIn("resolution='safe'", reconciliation)
        self.assertIn('source.as_uri() + "?mode=ro&immutable=1"', reconciliation)
        self.assertIn('"conflicts_applied": 0', reconciliation)
        reconciled_tables = reconciliation.split("table_names = [", 1)[1].split("]", 1)[0]
        self.assertNotIn("web_jobs", reconciled_tables)


if __name__ == "__main__":
    unittest.main()
