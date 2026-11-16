from pathlib import Path
from contextlib import closing
import sqlite3
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from webgui.catalog import Catalog


class PathAlbumGroupingTests(unittest.TestCase):
    def test_curated_child_overrides_various_artist_without_duplicates(self):
        with TemporaryDirectory() as directory:
            music = Path(directory) / "music"
            various = music / "Various Artists"
            soundtracks = music / "Soundtracks"
            curated = various / "Billboard"
            for path in (curated / "Volume 1", soundtracks / "Film", various / "Collection"):
                path.mkdir(parents=True, exist_ok=True)
            catalog = Catalog(
                music,
                Path(directory) / "unused.db",
                Path(directory),
                Path(directory),
                various_artist_path=various,
                soundtracks_path=soundtracks,
                curated_compilations_path=curated,
            )

            rows = [
                ({"path": str(curated / "Volume 1"), "artist_name": "Various Artists", "compilation": True}, "fallback"),
                ({"path": str(soundtracks / "Film"), "artist_name": "Soundtracks", "compilation": True}, "soundtracks"),
                ({"path": str(various / "Collection"), "artist_name": "Various Artists", "compilation": True}, "various-artists"),
                ({"path": str(music / "Artist" / "Release"), "artist_name": "Artist", "compilation": False}, "artist"),
            ]
            for row, expected in rows:
                self.assertEqual(catalog.album_library_group(row), expected)

    def test_shared_music_root_is_an_inherited_default_not_three_claims(self):
        with TemporaryDirectory() as directory:
            music = Path(directory) / "music"
            album = music / "Various Artists" / "Collection"
            album.mkdir(parents=True)
            catalog = Catalog(
                music,
                Path(directory) / "unused.db",
                Path(directory),
                Path(directory),
                various_artist_path=music,
                soundtracks_path=music,
                curated_compilations_path=music,
            )
            self.assertEqual(
                catalog.album_library_group({
                    "path": str(album),
                    "artist_name": "Various Artists",
                    "compilation": True,
                }),
                "various-artists",
            )

    def test_single_roots_match_imported_windows_album_paths(self):
        with TemporaryDirectory() as directory:
            music = Path(directory) / "music"
            various = music / "[Various Artists]"
            soundtracks = music / "[Soundtracks]"
            curated = various / "Billboard"
            for path in (various, soundtracks, curated):
                path.mkdir(parents=True, exist_ok=True)
            catalog = Catalog(
                music,
                Path(directory) / "unused.db",
                Path(directory),
                Path(directory),
                various_artist_path=various,
                soundtracks_path=soundtracks,
                curated_compilations_path=curated,
            )

            with patch.object(
                Path, "resolve", side_effect=AssertionError("filesystem access")
            ):
                self.assertEqual(catalog.album_library_group({
                    "path": r"K:\Music\[Soundtracks]\2 Fast 2 Furious",
                    "artist_name": "Various Artists",
                    "compilation": True,
                }), "soundtracks")
                self.assertEqual(catalog.album_library_group({
                    "path": r"K:\Music\[Various Artists]\Billboard\Hot 100",
                    "artist_name": "Various Artists",
                    "compilation": True,
                }), "fallback")
                self.assertEqual(catalog.album_library_group({
                    "path": r"K:\Music\[Various Artists]\Collection",
                    "artist_name": "Various Artists",
                    "compilation": True,
                }), "various-artists")

    def test_root_owned_va_and_soundtrack_albums_do_not_become_artist_cards(self):
        with TemporaryDirectory() as directory:
            music = Path(directory) / "music"
            various = music / "[Various Artists]"
            soundtracks = music / "[Soundtracks]"
            curated = various / "Billboard"
            for path in (various, soundtracks, curated):
                path.mkdir(parents=True, exist_ok=True)
            catalog = Catalog(
                music,
                Path(directory) / "unused.db",
                Path(directory),
                Path(directory),
                various_artist_path=various,
                soundtracks_path=soundtracks,
                curated_compilations_path=curated,
            )

            self.assertTrue(catalog.artist_album_allowed({
                "path": str(music / "Aaron Neville" / "Gold"),
                "artist_name": "Aaron Neville",
                "compilation": False,
            }))
            for path in (
                soundtracks / "2 Fast 2 Furious",
                various / "Bass in Your Face",
                curated / "Hot 100",
            ):
                self.assertFalse(catalog.artist_album_allowed({
                    "path": str(path),
                    "artist_name": "Various Artists",
                    "compilation": True,
                }))

    def test_album_override_wins_over_root_and_can_restore_automatic_grouping(self):
        with TemporaryDirectory() as directory:
            music = Path(directory) / "music"
            various = music / "[Various Artists]"
            album = various / "Artist Release"
            album.mkdir(parents=True)
            catalog = Catalog(
                music,
                Path(directory) / "unused.db",
                Path(directory),
                Path(directory),
                various_artist_path=various,
                soundtracks_path=music,
                curated_compilations_path=music,
            )
            row = {
                "album_key": "album-1",
                "path": str(album),
                "artist_name": "Actual Artist",
                "compilation": False,
            }

            self.assertEqual(catalog.album_library_group(row), "various-artists")
            catalog.category_overrides["album-1"] = "artist"
            self.assertEqual(catalog.album_library_group(row), "artist")
            self.assertTrue(catalog.artist_album_allowed(row))
            catalog.category_overrides["album-1"] = "soundtracks"
            self.assertEqual(catalog.album_library_group(row), "soundtracks")
            catalog.category_overrides.pop("album-1")
            self.assertEqual(catalog.album_library_group(row), "various-artists")

    def test_album_override_is_persisted_without_scanning_media(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            music = root / "music"
            various = music / "[Various Artists]"
            album = various / "Artist Release"
            album.mkdir(parents=True)
            database = root / "catalog.db"
            with closing(sqlite3.connect(database)) as db, db:
                db.executescript(
                    "CREATE TABLE artists(artist_key TEXT PRIMARY KEY,artist_name TEXT);"
                    "CREATE TABLE albums(album_key TEXT PRIMARY KEY,artist_key TEXT,path TEXT,"
                    "compilation INTEGER,musicbrainz_albumid TEXT);"
                    "CREATE TABLE album_category_overrides(album_key TEXT PRIMARY KEY,"
                    "category TEXT,updated_at TEXT);"
                )
                db.execute("INSERT INTO artists VALUES('artist-1','Actual Artist')")
                db.execute(
                    "INSERT INTO albums VALUES('album-1','artist-1',?,0,'release-id')",
                    (str(album),),
                )

            def connect():
                connection = sqlite3.connect(database)
                connection.row_factory = sqlite3.Row
                return connection

            catalog = Catalog(
                music, database, root, root,
                various_artist_path=various,
                soundtracks_path=music,
                curated_compilations_path=music,
            )
            with patch.object(catalog, "connect", side_effect=connect):
                result = catalog.set_album_category("album-1", "artist")
            self.assertEqual(result["library_group"], "artist")

            restarted = Catalog(
                music, database, root, root,
                various_artist_path=various,
                soundtracks_path=music,
                curated_compilations_path=music,
            )
            with patch.object(restarted, "connect", side_effect=connect):
                restarted.reload_category_overrides()
                restored = restarted.set_album_category("album-1", "")
            self.assertEqual(restarted.category_overrides, {})
            self.assertEqual(restored["library_group"], "various-artists")


if __name__ == "__main__":
    unittest.main()
