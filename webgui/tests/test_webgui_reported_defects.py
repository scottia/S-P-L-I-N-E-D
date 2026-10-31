"""Focused regressions for the reported WebGUI processing defects."""

from __future__ import annotations

from contextlib import closing
from pathlib import Path
import sqlite3
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest

from webgui.services import Services


class _SqliteDatabase:
    def __init__(self, path: Path):
        self.path = path

    def connect(self):
        connection = sqlite3.connect(self.path)
        connection.row_factory = sqlite3.Row
        return connection


class ReportedDefectTests(unittest.TestCase):
    def test_comp_report_backfills_only_albums_in_selected_artist_scope(self):
        with TemporaryDirectory() as directory:
            database_path = Path(directory) / "catalog.sqlite"
            with closing(sqlite3.connect(database_path)) as db:
                db.execute("CREATE TABLE albums (album_key TEXT PRIMARY KEY,track_count INTEGER,musicbrainz_albumid TEXT)")
                db.execute("CREATE TABLE tracks (track_key TEXT PRIMARY KEY,album_key TEXT)")
                db.executemany("INSERT INTO albums VALUES (?,?, '')", [("target", 2), ("unrelated", 3)])
                db.commit()

            class Catalog:
                def artist_comp_album_ids(self, artist_id):
                    self.requested = artist_id
                    return ["target"]

                def artist_comp_report(self, artist_id):
                    return [{"id": "target", "tracks": []}]

            services = Services.__new__(Services)
            services.runtime = SimpleNamespace(database=_SqliteDatabase(database_path))
            services.catalog = Catalog()
            hydrated = []
            services.hydrate_tracks = hydrated.append

            report = services.comp_report("artist-1")

            self.assertEqual(report, [{"id": "target", "tracks": []}])
            self.assertEqual(hydrated, ["target"])

    def test_scan_skip_advances_batch_and_reports_progress_without_writing_in_read_mode(self):
        services = Services.__new__(Services)
        services.runtime = SimpleNamespace(cfg={"mode": "read"})
        rows = {
            "one": {"album_key": "one", "album_name": "One", "artist_name": "Artist"},
            "two": {"album_key": "two", "album_name": "Two", "artist_name": "Artist"},
        }
        services.album_row = rows.__getitem__
        services.search_album = lambda album_id: {
            "candidates": [{"image": album_id, "acceptable": True}], "warnings": []
        }
        writes = []
        services.write_cover = writes.append

        class Adapter:
            def __init__(self):
                self.answers = iter(("k", "s"))
                self.progress_values = []
                self.context = {}

            def cancelled(self):
                return False

            def emit(self, _event, _payload):
                pass

            def read(self, _prompt, _context):
                return next(self.answers)

            def progress(self, value):
                self.progress_values.append(value)

        adapter = Adapter()
        result = services.run_scan({"album_ids": ["one", "two"]}, adapter)

        self.assertEqual(result, {"reviewed": 1, "total": 2, "mode": "read"})
        self.assertEqual(writes, [])
        self.assertEqual(adapter.progress_values[0]["completed"], 0)
        self.assertEqual(adapter.progress_values[0]["total"], 2)
        self.assertEqual(adapter.progress_values[-1]["completed"], 2)

    def test_static_workspace_has_structured_artist_information_and_no_automatic_inventory_scan(self):
        static = Path(__file__).parents[1] / "static"
        document = (static / "index.html").read_text(encoding="utf-8")
        application = (static / "app.js").read_text(encoding="utf-8")
        operations = (static / "operations.js").read_text(encoding="utf-8")
        for identity in (
            'id="artistInfoRows"',
            'id="artistInfoAlbums"',
            'id="artistInfoStatus"',
            'id="artistInfoMbid"',
            'id="artistInfoGenres"',
            'id="artistInfoRoles"',
            'id="refreshArtistInfo"',
            'id="musicbrainzArtistButton"',
        ):
            self.assertIn(identity, document)
        initialize = operations.split("async function initialize", 1)[1].split(
            "function showAuthentication", 1
        )[0]
        self.assertNotIn("checkLibraryChanges", initialize)
        self.assertIn("track.thumbnail", application)
        self.assertIn("COMP status", application)


if __name__ == "__main__":
    unittest.main()
