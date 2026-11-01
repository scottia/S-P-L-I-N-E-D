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
from webgui.services import Services


class BrowserFallbackTests(unittest.TestCase):
    def test_animated_webp_cover_is_preserved_for_write_and_staging(self) -> None:
        class Catalog:
            def __init__(self, source: Path):
                self.source = source
                self.body = b""

            def image_source(self, _image: str):
                return self.source

            def image(self, body: bytes):
                self.body = body
                return "/api/image/test"

        with TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "candidate.webp"
            frames = [Image.new("RGB", (40, 40), color) for color in ("red", "blue")]
            frames[0].save(
                source,
                format="WEBP",
                save_all=True,
                append_images=frames[1:],
                duration=80,
                loop=0,
            )
            service = Services.__new__(Services)
            service.runtime = type(
                "Runtime",
                (),
                {
                    "cfg": {
                        "output": {
                            "file_formats": ["jpeg", "png", "webp"],
                            "upscale_below_ideal": True,
                            "upscale_max_percent": 200,
                        },
                        "range": {"ideal": 1800},
                    },
                    "temporary_cache": root / "tmp",
                },
            )()
            service.catalog = Catalog(source)
            result = service.preview({"image": "candidate", "kind": "cover"})
            self.assertEqual(result["format"], "webp")
            self.assertEqual(service.catalog.body, source.read_bytes())
            with Image.open(BytesIO(service.catalog.body)) as written:
                self.assertEqual(written.format, "WEBP")
                self.assertGreater(int(getattr(written, "n_frames", 1)), 1)

    def test_artist_selection_uses_summary_and_settings_hide_native_sqlite(self) -> None:
        static = Path(__file__).parents[1] / "static"
        application = (static / "app.js").read_text(encoding="utf-8")
        choose_artist = application.split("async function chooseArtist", 1)[1].split(
            "async function chooseAlbum", 1
        )[0]
        open_report = application.split("async function openArtistCompReport", 1)[1].split(
            "async function probeArtistCompStatuses", 1
        )[0]
        self.assertIn("/api/artist/overview", choose_artist)
        self.assertNotIn("/api/artist/comp-report", choose_artist)
        self.assertNotIn("Promise.all", choose_artist)
        self.assertNotIn('/api/artist?id=', choose_artist)
        self.assertNotIn('/api/artist/comp-albums?id=', choose_artist)
        self.assertIn("/api/artist/comp-report", open_report)

        operations = (static / "operations.js").read_text(encoding="utf-8")
        self.assertIn('draft.scan.shared_artist_art_dir ??= ""', operations)
        self.assertIn("delete draft.scan.sqlite_shared", operations)
        self.assertIn("presentQueuedCandidates(job, prompt, options)", operations)
        self.assertIn("await focusQueuedAlbum(state.queuedDecision)", operations)
        self.assertIn('\"EMBED Track\"', operations)
        self.assertIn('job.kind === "embed-album"', operations)
        self.assertIn('kind === "musicbrainz-results"', operations)
        self.assertIn('"__manual_candidates__"', application)
        self.assertIn('"Advanced IDs / Apply IDs"', application)
        self.assertIn('state.workspace === "enhance" && state.radioCurrent', operations)
        self.assertIn('$("fullscreenButton")', operations)
        self.assertNotIn('$("reviewWork")', operations)
        self.assertNotIn("submitQueuedDecision(candidate.queueValue)", application)
        self.assertNotIn('node("button", "candidate-preview"', application)
        self.assertNotIn('node("button", "candidate-choose"', application)
        self.assertIn("cover: group.cover || album.cover || null", application)
        self.assertIn("fallback_complete", application)
        self.assertIn("normalizedColor", application)
        self.assertIn("renderedPreview", application)
        self.assertIn("compReportLoading", application)
        enter_artist_art = application.split("function enterArtistArt", 1)[1].split(
            "function showDialog", 1
        )[0]
        self.assertNotIn("searchArtist", enter_artist_art)
        self.assertIn('state.workspace === "enhance" ? searchArtist() : launchArtist()', operations)
        self.assertIn('status: "check-error"', application)
        document = (static / "index.html").read_text(encoding="utf-8")
        self.assertIn('id="fullscreenButton"', document)
        self.assertNotIn('id="reviewWork"', document)

        self.assertLess(
            document.index('id="bypassButton"'),
            document.index('id="uploadArtwork"'),
        )

    def test_signed_path_handle_changes_when_saved_artwork_changes(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "artist.png"
            Image.new("RGB", (30, 30), "red").save(source)
            app = PreviewApp(root, root / "index.sqlite", root, root)
            first = app.image(source)
            Image.new("RGB", (31, 30), "blue").save(source)
            second = app.image(source)
            self.assertNotEqual(first, second)
            self.assertEqual(app.image_source(second), source)
            # A queued job may still hold the earlier signed URL after a
            # same-path artwork write. The old handle remains authorized for
            # that allowed path while new browser renders receive a new URL.
            self.assertEqual(app.image_source(first), source)

    def test_artist_report_combines_matching_tracks_across_comp_albums(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            folders = [
                root / "comp-one", root / "comp-two", root / "comp-three",
                root / "identified",
            ]
            for folder in folders:
                folder.mkdir()
            paths = [folder / "01 Track.mp3" for folder in folders]
            covers = [folder / "cover.jpg" for folder in folders]
            third = folders[1] / "02 Blue.mp3"
            uncredited = folders[1] / "03 Untagged.mp3"
            other_artist = folders[1] / "04 Other.mp3"
            for path in paths:
                path.touch()
            for index, cover_path in enumerate(covers):
                Image.new("RGB", (640 + index, 640 + index), "navy").save(cover_path, "JPEG")
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
                db.execute("CREATE TABLE artist_album_appearances (artist_key TEXT, album_key TEXT, "
                           "featured INTEGER, navidrome_album_id TEXT, song_paths_json TEXT)")
                db.execute("INSERT INTO artists VALUES ('target', '3 Doors Down')")
                db.executemany("INSERT INTO albums VALUES (?, ?, ?, ?, '2003', ?, ?)", [
                    ("one", "COMP One", str(folders[0]), str(covers[0]), "", "various"),
                    ("two", "COMP Two", str(folders[1]), str(covers[1]), None, "target"),
                    ("three", "COMP Three", str(folders[2]), str(covers[2]), "", "various"),
                    ("identified", "Has Release ID", str(folders[3]), str(covers[3]), "release-id", "target"),
                ])
                db.executemany("INSERT INTO tracks VALUES (?, ?, ?, 'Track', ?)", [
                    ("first", "one", str(paths[0]), "3 Doors Down; Guest"),
                    ("second", "two", str(paths[1]), "3 Doors Down"),
                    ("blue", "two", str(third), "3 Doors Down"),
                    ("untagged", "two", str(uncredited), ""),
                    ("other", "two", str(other_artist), "3 Doors Downward"),
                    ("appearance", "three", str(paths[2]), "Alias Ensemble"),
                    ("excluded", "identified", str(paths[3]), "3 Doors Down"),
                ])
                db.execute(
                    "INSERT INTO artist_album_appearances VALUES "
                    "('target','three',1,'navidrome-three','[\"comp-three/01 Track.mp3\"]')"
                )
                db.execute("INSERT INTO compilation_track_artwork VALUES (?, 'embedded-existing')", (str(paths[0]),))
                db.execute("INSERT INTO compilation_track_artwork VALUES (?, 'embedded-existing')", (str(paths[1]),))
                db.commit()
            app = PreviewApp(root, database, root, root, allow_comp_repairs=True)
            with closing(app.connect()) as db:
                self.assertEqual(
                    app.fallback_thumbnail_keys(db),
                    {"one": "first", "two": "second"},
                )
            summary = app.artist_comp_albums("target")
            report = app.artist_comp_report("target")
            full_fallback = app.fallback_tracks("two")
            self.assertEqual(set(app.artist_comp_album_ids("target")), {"one", "two", "three"})
            self.assertEqual({group["id"] for group in summary}, {"one", "two", "three"})
            self.assertEqual(
                {group["id"]: len(group["tracks"]) for group in summary},
                {"one": 1, "two": 3, "three": 1},
            )
            self.assertTrue(summary[0]["cover"].startswith("/api/image/p."))
            self.assertEqual(app.image_bytes(summary[0]["cover"]), covers[0].read_bytes())
            self.assertNotIn("/api/fallback/thumbnail", summary[0]["cover"])
            with Image.open(BytesIO(app.fallback_thumbnail("one", "first"))) as thumbnail:
                self.assertEqual(thumbnail.size, (1200, 1200))
            report_by_id = {group["id"]: group for group in report}
            self.assertEqual(set(report_by_id), {"one", "two", "three"})
            self.assertTrue(report_by_id["one"]["tracks"][0]["completed"])
            self.assertTrue(report_by_id["two"]["tracks"][0]["completed"])
            self.assertFalse(report_by_id["two"]["tracks"][1]["completed"])
            self.assertEqual(
                {track["key"] for track in report_by_id["two"]["tracks"]},
                {"second", "blue", "untagged"},
            )
            self.assertEqual(
                {track["key"] for track in report_by_id["three"]["tracks"]},
                {"appearance"},
            )
            self.assertEqual(
                {track["key"] for track in full_fallback},
                {"second", "blue", "untagged", "other"},
            )
            self.assertEqual(app.comp_track_status("one", "first")["status"], "green")
            self.assertEqual(app.comp_track_status("two", "second")["status"], "purple")
            self.assertEqual(app.comp_track_status("two", "blue")["status"], "blue")
            app.repair_comp_track("two", "second")
            self.assertEqual(app.comp_track_status("two", "second")["status"], "green")
            self.assertEqual(len(list((root / "comp-repair-backups").glob("*.original-image"))), 1)

    def test_only_missing_musicbrainz_album_ids_appear_in_fallback(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            album_folders = [root / name for name in ("soundtrack", "comp", "album")]
            for folder in album_folders:
                folder.mkdir()
            folder_cover = album_folders[1] / "cover.png"
            Image.new("RGB", (500, 500), "green").save(folder_cover, "PNG")
            with closing(connect(database)) as db:
                db.execute("CREATE TABLE artists (artist_key TEXT PRIMARY KEY, artist_name TEXT)")
                db.execute("CREATE TABLE albums (album_key TEXT PRIMARY KEY, album_name TEXT, "
                           "artist_key TEXT, path TEXT, cover_path TEXT, track_count INTEGER, status TEXT, "
                           "compilation INTEGER, musicbrainz_albumid TEXT)")
                db.execute("INSERT INTO artists VALUES ('various', 'Various Artists')")
                db.executemany("INSERT INTO albums VALUES (?, ?, 'various', ?, ?, 10, 'unprocessed', ?, ?)", [
                    ("soundtrack", "American Pie 2", str(album_folders[0]), None, 1, "612d3e8c-0dae-4c17-9126-1d54e76b719d"),
                    ("unidentified-comp", "Unknown Compilation", str(album_folders[1]), None, 1, ""),
                    ("unidentified-album", "Unknown Album", str(album_folders[2]), None, 0, None),
                ])
                db.commit()
            app = PreviewApp(root, database, root, root)
            fallback = app.albums("", True)
            self.assertEqual({item["id"] for item in fallback}, {"unidentified-comp", "unidentified-album"})
            projected = next(item for item in fallback if item["id"] == "unidentified-comp")
            self.assertEqual(app.image_bytes(projected["cover"]), folder_cover.read_bytes())
            self.assertNotIn("/api/fallback/thumbnail", projected["cover"])
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
