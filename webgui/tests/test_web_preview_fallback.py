"""The browser fallback list follows the Windows Album ID authority rule."""

from pathlib import Path
from contextlib import closing
from sqlite3 import connect
from tempfile import TemporaryDirectory
import unittest
from io import BytesIO

from PIL import Image
from mutagen.id3 import APIC, ID3

from webgui.catalog import PreviewApp


class BrowserFallbackTests(unittest.TestCase):
    def test_artist_selection_uses_summary_and_settings_hide_native_sqlite(self) -> None:
        static = Path(__file__).parents[1] / "static"
        application = (static / "app.js").read_text(encoding="utf-8")
        choose_artist = application.split("async function chooseArtist", 1)[1].split(
            "async function chooseAlbum", 1
        )[0]
        open_report = application.split("async function openArtistCompReport", 1)[1].split(
            "async function probeArtistCompStatuses", 1
        )[0]
        self.assertIn("/api/artist/comp-albums", choose_artist)
        self.assertNotIn("/api/artist/comp-report", choose_artist)
        self.assertIn("/api/artist/comp-report", open_report)

        operations = (static / "operations.js").read_text(encoding="utf-8")
        self.assertIn('cfg.scan.shared_artist_art_dir ??= ""', operations)
        self.assertIn('keys.join(".") === "scan.sqlite_shared"', operations)

    def test_artist_report_combines_matching_tracks_across_comp_albums(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            folders = [root / "comp-one", root / "comp-two", root / "identified"]
            for folder in folders:
                folder.mkdir()
            paths = [folder / "01 Track.mp3" for folder in folders]
            third = folders[1] / "02 Blue.mp3"
            uncredited = folders[1] / "03 Untagged.mp3"
            other_artist = folders[1] / "04 Other.mp3"
            for path in paths:
                path.touch()
            third.touch()
            uncredited.touch()
            other_artist.touch()
            for path, side in [(paths[0], 1200), (paths[1], 400)]:
                picture = BytesIO()
                Image.new("RGB", (side, side), "blue").save(picture, "PNG")
                tags = ID3()
                tags.add(APIC(encoding=3, mime="image/png", type=3, desc="Cover", data=picture.getvalue()))
                tags.save(path)
            with closing(connect(database)) as db:
                db.execute("CREATE TABLE artists (artist_key TEXT PRIMARY KEY, artist_name TEXT)")
                db.execute("CREATE TABLE albums (album_key TEXT PRIMARY KEY, album_name TEXT, "
                           "path TEXT, cover_path TEXT, release_year TEXT, musicbrainz_albumid TEXT, artist_key TEXT)")
                db.execute("CREATE TABLE tracks (track_key TEXT PRIMARY KEY, album_key TEXT, "
                           "path TEXT, title TEXT, artist_name TEXT)")
                db.execute("CREATE TABLE compilation_track_artwork (track_path TEXT, outcome TEXT)")
                db.execute("INSERT INTO artists VALUES ('target', '3 Doors Down')")
                db.executemany("INSERT INTO albums VALUES (?, ?, ?, NULL, '2003', ?, ?)", [
                    ("one", "COMP One", str(folders[0]), "", "various"),
                    ("two", "COMP Two", str(folders[1]), None, "target"),
                    ("identified", "Has Release ID", str(folders[2]), "release-id", "target"),
                ])
                db.executemany("INSERT INTO tracks VALUES (?, ?, ?, 'Track', ?)", [
                    ("first", "one", str(paths[0]), "3 Doors Down; Guest"),
                    ("second", "two", str(paths[1]), "3 Doors Down"),
                    ("blue", "two", str(third), "3 Doors Down"),
                    ("untagged", "two", str(uncredited), ""),
                    ("other", "two", str(other_artist), "3 Doors Downward"),
                    ("excluded", "identified", str(paths[2]), "3 Doors Down"),
                ])
                db.execute("INSERT INTO compilation_track_artwork VALUES (?, 'embedded-existing')", (str(paths[0]),))
                db.execute("INSERT INTO compilation_track_artwork VALUES (?, 'embedded-existing')", (str(paths[1]),))
                db.commit()
            app = PreviewApp(root, database, root, root, allow_comp_repairs=True)
            summary = app.artist_comp_albums("target")
            report = app.artist_comp_report("target")
            self.assertEqual([group["id"] for group in summary], ["one", "two"])
            self.assertEqual([len(group["tracks"]) for group in summary], [1, 3])
            self.assertEqual([group["id"] for group in report], ["one", "two"])
            self.assertEqual([group["tracks"][0]["completed"] for group in report], [True, True])
            self.assertFalse(report[1]["tracks"][1]["completed"])
            self.assertEqual({track["key"] for track in report[1]["tracks"]}, {"second", "blue", "untagged"})
            self.assertEqual(app.comp_track_status("one", "first")["status"], "green")
            self.assertEqual(app.comp_track_status("two", "second")["status"], "purple")
            self.assertEqual(app.comp_track_status("two", "blue")["status"], "blue")
            app.repair_artist_comp_track("target", "two", "second")
            self.assertEqual(app.comp_track_status("two", "second")["status"], "green")
            self.assertEqual(len(list((root / "comp-repair-backups").glob("*.original-image"))), 1)

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
