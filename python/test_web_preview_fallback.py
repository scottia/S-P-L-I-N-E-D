"""The browser fallback list follows the Windows Album ID authority rule."""

from pathlib import Path
from contextlib import closing
from sqlite3 import connect
from tempfile import TemporaryDirectory
import unittest
from io import BytesIO

from PIL import Image
from mutagen.id3 import APIC, ID3

from web_preview import PreviewApp


class BrowserFallbackTests(unittest.TestCase):
    def test_only_missing_musicbrainz_album_ids_appear_in_fallback(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            with closing(connect(database)) as db:
                db.execute("CREATE TABLE artists (artist_key TEXT PRIMARY KEY, artist_name TEXT)")
                db.execute("CREATE TABLE albums (album_key TEXT PRIMARY KEY, album_name TEXT, "
                           "artist_key TEXT, cover_path TEXT, track_count INTEGER, status TEXT, "
                           "compilation INTEGER, musicbrainz_albumid TEXT)")
                db.execute("INSERT INTO artists VALUES ('various', 'Various Artists')")
                db.executemany("INSERT INTO albums VALUES (?, ?, 'various', NULL, 10, 'unprocessed', ?, ?)", [
                    ("soundtrack", "American Pie 2", 1, "612d3e8c-0dae-4c17-9126-1d54e76b719d"),
                    ("unidentified-comp", "Unknown Compilation", 1, ""),
                    ("unidentified-album", "Unknown Album", 0, None),
                ])
                db.commit()
            app = PreviewApp(root, database, root, root)
            self.assertEqual({item["id"] for item in app.albums("", True)},
                             {"unidentified-comp", "unidentified-album"})
            self.assertEqual(len(app.albums("", False)), 3)

    def test_fallback_tracks_use_embedded_art_not_folder_cover(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            folder = root / "compilation"
            folder.mkdir()
            database = root / "index.sqlite"
            cover = BytesIO()
            Image.new("RGB", (48, 48), "blue").save(cover, "PNG")
            (folder / "cover.png").write_bytes(cover.getvalue())
            first = folder / "01 First.mp3"
            second = folder / "02 Second.mp3"
            ID3().save(second)
            tag = ID3()
            tag.add(APIC(encoding=3, mime="image/png", type=3, desc="Cover", data=cover.getvalue()))
            tag.save(first)
            with closing(connect(database)) as db:
                db.execute("CREATE TABLE albums (album_key TEXT PRIMARY KEY, path TEXT, musicbrainz_albumid TEXT)")
                db.execute("CREATE TABLE tracks (track_key TEXT PRIMARY KEY, album_key TEXT, path TEXT, title TEXT, artist_name TEXT)")
                db.execute("CREATE TABLE compilation_track_artwork (track_path TEXT, outcome TEXT)")
                db.execute("INSERT INTO albums VALUES ('comp', ?, '')", (str(folder),))
                db.execute("INSERT INTO albums VALUES ('identified', ?, 'mb-release')", (str(folder),))
                db.execute("INSERT INTO tracks VALUES ('first', 'comp', ?, 'First', 'A')", (str(first),))
                db.execute("INSERT INTO tracks VALUES ('second', 'comp', ?, 'Second', 'B')", (str(second),))
                db.execute("INSERT INTO compilation_track_artwork VALUES (?, 'embedded-existing')", (str(first),))
                db.commit()
            app = PreviewApp(root, database, root, root)
            tracks = app.fallback_tracks("comp")
            self.assertEqual([item["key"] for item in tracks], ["first", "second"])
            self.assertEqual([item["completed"] for item in tracks], [True, False])
            self.assertIsNotNone(app.fallback_track_preview("comp", "first")["image"])
            self.assertIsNone(app.fallback_track_preview("comp", "second")["image"])
            with self.assertRaises(ValueError):
                app.fallback_tracks("identified")

    def test_upscale_preview_is_memory_only(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            database.touch()
            artwork = BytesIO()
            Image.new("RGB", (1000, 1000), "red").save(artwork, "PNG")
            app = PreviewApp(root, database, root, root)
            result = app.upscale_preview(app.image(artwork.getvalue()))
            self.assertEqual((result["width"], result["height"]), (1800, 1800))
            self.assertTrue(result["upscaled"])
            self.assertEqual({path.name for path in root.iterdir()}, {"index.sqlite"})


if __name__ == "__main__":
    unittest.main()
