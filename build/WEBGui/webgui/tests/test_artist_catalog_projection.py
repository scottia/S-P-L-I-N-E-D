"""Artist Overview uses one canonical identity and complete Navidrome evidence."""

from __future__ import annotations

from contextlib import closing
from io import BytesIO
import json
from pathlib import Path
from sqlite3 import connect
from tempfile import TemporaryDirectory
import unittest
from unittest import mock

from PIL import Image

from webgui.catalog import (
    PreviewApp,
    artist_credit_position,
    artist_display_sort_key,
    embedded_album_artwork_summary,
    normalized_artist_identity,
    potential_collaboration_credits,
)
from splined_media_database import aggregate_artist, update_artist_aggregate


class ArtistCatalogProjectionTests(unittest.TestCase):
    def test_guarded_collaboration_credits_match_windows_presentation(self):
        self.assertEqual(
            potential_collaboration_credits("Rob Wasserman & Aaron Neville"),
            ("Rob Wasserman", "Aaron Neville"),
        )
        self.assertEqual(
            potential_collaboration_credits("Big & Rich"),
            ("Big", "Rich"),
        )
        self.assertEqual(
            artist_credit_position("Rob Wasserman & Aaron Neville", "Aaron Neville"),
            1,
        )
        # Expansion remains potential: the catalog projection requires an
        # independently indexed identity before exposing human-readable parts.

    def test_verified_album_material_overrides_stale_unprocessed_status(self):
        stale = {
            "status": "unprocessed",
            "cover_found": 1,
            "track_count": 10,
            "embedded_tracks": 10,
            "embedded_validation_status": "verified",
        }
        self.assertEqual(PreviewApp._effective_album_status(stale), "processed")
        self.assertEqual(aggregate_artist(["processed"] * 5), "complete")

        review = {**stale, "status": "timeout"}
        self.assertEqual(PreviewApp._effective_album_status(review), "timeout")
        self.assertEqual(aggregate_artist(["processed", "timeout"]), "partial")

    def test_artist_yellow_uses_indexed_static_album_cover_not_webp(self):
        """Index evidence decides Yellow without probing NAS artwork on list load."""
        album = {
            "status": "processed",
            "cover_path": "",
            "cover_found": 0,
            "track_count": 2,
            "embedded_tracks": 2,
            "embedded_validation_status": "verified",
        }
        missing = PreviewApp._artist_album_state_conditions(album)
        self.assertIn("missing_artist_art", missing)
        self.assertNotIn("incomplete_or_error", missing)
        self.assertEqual(
            PreviewApp._aggregate_album_status([{"status": "processed", "conditions": missing}]),
            "bypassed",
        )
        for filename in ("cover.webp", "COVER.WEBP"):
            with self.subTest(filename=filename):
                self.assertIn(
                    "missing_artist_art",
                    PreviewApp._artist_album_state_conditions(
                        {**album, "cover_path": filename, "cover_found": 1}
                    ),
                )
        static = {**album, "cover_path": "cover.jpg", "cover_found": 1}
        self.assertNotIn(
            "missing_artist_art", PreviewApp._artist_album_state_conditions(static)
        )
        self.assertNotIn(
            "missing_artist_art",
            PreviewApp._artist_album_state_conditions(
                album, {"writer": "windows"}
            ),
        )
        error = PreviewApp._artist_album_state_conditions({**album, "status": "error"})
        self.assertEqual(
            PreviewApp._aggregate_album_status([{"status": "error", "conditions": error}]),
            "incomplete",
        )

    def test_album_embedded_summary_matches_native_footer_semantics(self):
        first = BytesIO()
        second = BytesIO()
        Image.new("RGB", (1200, 1200), "red").save(first, format="JPEG")
        Image.new("RGB", (900, 900), "blue").save(second, format="PNG")
        bodies = [first.getvalue(), second.getvalue(), None]
        with mock.patch(
            "webgui.catalog.embedded_track_artwork_bytes", side_effect=bodies
        ):
            summary = embedded_album_artwork_summary(
                [Path("one.mp3"), Path("two.mp3"), Path("three.mp3")], 1200
            )

        self.assertEqual(summary["track_count"], 3)
        self.assertEqual(summary["embedded_count"], 2)
        self.assertEqual(summary["resolution"], "Varying Size")
        self.assertEqual(summary["bytes"], len(bodies[0]) + len(bodies[1]))
        self.assertEqual(
            (summary["current_width"], summary["current_height"]), (1200, 1200)
        )
        self.assertEqual(summary["current_bytes"], len(bodies[0]))
        self.assertTrue(summary["below_preference"])
        self.assertTrue(summary["varying"])

    def test_album_embedded_summary_flags_any_size_outside_preference(self):
        def jpeg(width: int, height: int) -> bytes:
            buffer = BytesIO()
            Image.new("RGB", (width, height), "teal").save(buffer, format="JPEG")
            return buffer.getvalue()

        for (width, height), below, outside in (
            ((1200, 1200), False, False),
            ((1199, 1199), True, True),
            ((1201, 1201), False, True),
            ((1200, 1201), False, True),
            ((1201, 1200), False, True),
        ):
            with self.subTest(dimensions=(width, height)):
                with mock.patch(
                    "webgui.catalog.embedded_track_artwork_bytes",
                    return_value=jpeg(width, height),
                ):
                    summary = embedded_album_artwork_summary(
                        [Path("sample.flac")], 1200
                    )
                self.assertEqual(summary["embedded_count"], 1)
                self.assertEqual(summary["below_preference"], below)
                self.assertEqual(summary["outside_preference"], outside)

        with mock.patch(
            "webgui.catalog.embedded_track_artwork_bytes", return_value=None
        ):
            missing = embedded_album_artwork_summary([Path("empty.flac")], 1200)
        self.assertTrue(missing["below_preference"])
        self.assertTrue(missing["outside_preference"])

    def test_display_sort_keeps_symbol_prefixed_artist_before_letters(self):
        artists = ["Aaliyah", "*NSYNC", "10,000 Maniacs", "Nirvana"]
        self.assertEqual(
            sorted(artists, key=artist_display_sort_key),
            ["*NSYNC", "10,000 Maniacs", "Aaliyah", "Nirvana"],
        )

    def test_folder_safe_alias_matches_tagged_artist_projection_identity(self):
        self.assertEqual(
            normalized_artist_identity("AC-DC"),
            normalized_artist_identity("AC/DC"),
        )

    def test_artist_list_returns_lazy_thumbnail_url_without_resolving_artwork(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,"
                    "path TEXT,compilation INTEGER,musicbrainz_albumid TEXT)"
                )
                db.execute(
                    "INSERT INTO artists VALUES ('artist-1','Artist','',?,'unprocessed')",
                    (str(root / "Artist"),),
                )
                db.execute(
                    "INSERT INTO albums VALUES ('album-1','artist-1',?,0,'')",
                    (str(root / "Artist" / "Album"),),
                )
                db.commit()
            app = PreviewApp(root, database, root, root)
            with mock.patch.object(
                app,
                "artist_thumbnail_path",
                side_effect=AssertionError("initial list must not resolve artwork paths"),
            ):
                self.assertEqual(
                    app.artists("")[0]["portrait"],
                    "/api/artist/thumbnail?id=artist-1",
                )

    def test_artist_list_aggregates_indexed_covers_and_featured_credits_without_scans(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,"
                    "album_name TEXT,path TEXT,cover_path TEXT,cover_found INTEGER,"
                    "track_count INTEGER,status TEXT,compilation INTEGER,"
                    "musicbrainz_albumid TEXT,release_year TEXT,embedded_tracks INTEGER,"
                    "embedded_validation_status TEXT,webp_found INTEGER)"
                )
                db.execute(
                    "CREATE TABLE album_artwork_write_coordination (album_key TEXT,"
                    "relative_path TEXT,writer TEXT,operation TEXT,event_id TEXT,"
                    "source_completed_ms INTEGER,revision INTEGER,completed_at TEXT)"
                )
                db.executemany(
                    "INSERT INTO artists VALUES (?,?,?,?,?)",
                    [
                        (artist, artist.title(), "", str(root / artist), "processed")
                        for artist in ("yellow", "green", "red")
                    ],
                )
                db.executemany(
                    "INSERT INTO albums VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                    [
                        ("yellow-album", "yellow", "Missing still", str(root / "yellow" / "Album"),
                         "cover.webp", 1, 2, "processed", 0, "mbid-yellow", "2025", 2, "verified", 1),
                        ("green-album", "green", "Good still", str(root / "green" / "Album"),
                         "cover.jpg", 1, 2, "processed", 0, "mbid-green", "2025", 2, "verified", 0),
                        ("red-album", "red", "Explicit error", str(root / "red" / "Album"),
                         "", 0, 2, "error", 0, "mbid-red", "2025", 0, "verified", 0),
                    ],
                )
                db.commit()
            for name in ("yellow", "green", "red"):
                (root / name / "Album").mkdir(parents=True)
            featured = {
                normalized_artist_identity("Featured"): {
                    "artist_key": "tag:featured", "artist_name": "Featured",
                    "musicbrainz_artistid": "", "primary_path": "",
                    "album_keys": {"yellow-album"},
                    "artist_album_keys": {"yellow-album"},
                    "statuses": {"yellow-album": "processed"},
                    "comp_statuses": {},
                }
            }
            app = PreviewApp(root, database, root, root)
            with (
                mock.patch.object(app, "_projected_track_artists", return_value=featured),
                mock.patch.object(app, "album_cover_path",
                                  side_effect=AssertionError("Artist list must not scan NAS covers")),
                mock.patch.object(app, "artist_thumbnail_path",
                                  side_effect=AssertionError("Artist list must not scan Artist Art")),
            ):
                listed = {item["name"]: item for item in app.artists("")}
            self.assertEqual(listed["Yellow"]["status"], "bypassed")
            self.assertFalse(listed["Yellow"]["has_incomplete_error"])
            self.assertEqual(listed["Featured"]["status"], "bypassed")
            self.assertEqual(listed["Green"]["status"], "complete")
            self.assertFalse(listed["Green"]["has_incomplete_error"])
            self.assertEqual(listed["Red"]["status"], "incomplete")
            self.assertTrue(listed["Red"]["has_incomplete_error"])

    def test_lazy_thumbnail_result_reports_missing_artist_art(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,"
                    "path TEXT,compilation INTEGER,musicbrainz_albumid TEXT)"
                )
                db.execute(
                    "INSERT INTO artists VALUES ('artist-1','Artist','',?,'unprocessed')",
                    (str(root / "Artist"),),
                )
                db.execute(
                    "INSERT INTO albums VALUES ('album-1','artist-1',?,0,'')",
                    (str(root / "Artist" / "Album"),),
                )
                db.commit()
            app = PreviewApp(root, database, root, root)
            with mock.patch.object(app, "_navidrome_artists_by_name", return_value={}):
                self.assertEqual(
                    app.artist_thumbnails(),
                    [{
                        "id": "artist-1",
                        "portrait": "",
                        "missing_artist_art": True,
                    }],
                )

    def test_musicbrainz_identity_is_canonical_when_name_only_duplicate_is_larger(self):
        rows = [
            {
                "artist_key": "tag:aerosmith",
                "artist_name": "Aerosmith",
                "musicbrainz_artistid": "",
                "album_count": 19,
            },
            {
                "artist_key": "mbid:aerosmith",
                "artist_name": "Aerosmith",
                "musicbrainz_artistid": "3d2b98e5-556f-4451-a3ff-c50ea18d57cb",
                "album_count": 1,
            },
        ]
        self.assertEqual(
            PreviewApp._canonical_artist_row(rows)["artist_key"],
            "mbid:aerosmith",
        )

    def test_opposing_override_counts_as_complete_in_artist_aggregate(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            band = root / "Band"
            local_green = band / "Local Green"
            windows_grey = band / "Windows Grey"
            local_green.mkdir(parents=True)
            windows_grey.mkdir(parents=True)
            local_cover = local_green / "cover.jpg"
            Image.new("RGB", (16, 16), "blue").save(local_cover)
            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,album_name TEXT,"
                    "path TEXT,cover_path TEXT,track_count INTEGER,status TEXT,compilation INTEGER,"
                    "musicbrainz_albumid TEXT,release_year TEXT,embedded_tracks INTEGER,"
                    "embedded_validation_status TEXT,webp_found INTEGER)"
                )
                db.execute(
                    "CREATE TABLE tracks (track_key TEXT PRIMARY KEY,album_key TEXT,path TEXT,"
                    "title TEXT,artist_name TEXT)"
                )
                db.execute(
                    "CREATE TABLE album_artwork_write_coordination (album_key TEXT,"
                    "relative_path TEXT,writer TEXT,operation TEXT,event_id TEXT,"
                    "source_completed_ms INTEGER,revision INTEGER,completed_at TEXT)"
                )
                db.execute(
                    "INSERT INTO artists VALUES ('band','Band','',?,'partial')",
                    (str(root / "Band"),),
                )
                db.executemany(
                    "INSERT INTO albums VALUES (?,?,?,?,?,1,?,?,?,'2024',1,'verified',0)",
                    [
                        ("local-green", "band", "Local Green", str(local_green), str(local_cover), "processed", 0, "release-green"),
                        ("windows-grey", "band", "Windows Grey", str(windows_grey), None, "incomplete", 0, "release-grey"),
                    ],
                )
                db.executemany(
                    "INSERT INTO tracks VALUES (?,?,?,?,?)",
                    [
                        ("track-green", "local-green", str(local_green / "one.mp3"), "One", "Band"),
                        ("track-grey", "windows-grey", str(windows_grey / "two.mp3"), "Two", "Band"),
                    ],
                )
                db.execute(
                    "INSERT INTO album_artwork_write_coordination VALUES "
                    "('windows-grey','','windows','write-cover','event-1',100,1,'now')"
                )
                db.commit()

            app = PreviewApp(root, database, root, root)
            listed = app.artists("")[0]
            self.assertEqual(listed["status"], "complete")
            self.assertEqual(listed["albums"], 2)
            detail = app.artist("band")
            self.assertEqual(detail["status"], "complete")
            self.assertEqual(
                next(item for item in detail["albums"] if item["id"] == "windows-grey")["override_writer"],
                "windows",
            )

    def test_compilation_owned_by_artist_key_is_not_a_primary_artist_album(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            credentials = root / "credentials"
            credentials.mkdir()
            studio = root / "Adele" / "30"
            compilation = root / "Various Artists" / "Billboard"
            for folder in (studio, compilation):
                folder.mkdir(parents=True)
                (folder / "song.mp3").touch()

            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,album_name TEXT,"
                    "path TEXT,cover_path TEXT,track_count INTEGER,status TEXT,compilation INTEGER,"
                    "musicbrainz_albumid TEXT,release_year TEXT,embedded_tracks INTEGER,webp_found INTEGER)"
                )
                db.execute(
                    "CREATE TABLE tracks (track_key TEXT PRIMARY KEY,album_key TEXT,path TEXT,title TEXT,artist_name TEXT)"
                )
                db.execute(
                    "INSERT INTO artists VALUES ('adele','Adele','adele-mbid',?,'partial')",
                    (str(root / "Adele"),),
                )
                db.executemany(
                    "INSERT INTO albums VALUES (?,?,?,?,NULL,1,?,?,?, '2021',0,0)",
                    [
                        ("30", "adele", "30", str(studio), "processed", 0, "release-30"),
                        ("billboard", "adele", "Billboard Hot 100", str(compilation), "incomplete", 1, ""),
                    ],
                )
                db.executemany("INSERT INTO tracks VALUES (?,?,?,?,?)", [
                    ("30-track", "30", str(studio / "song.mp3"), "Easy on Me", "Adele"),
                    ("billboard-track", "billboard", str(compilation / "song.mp3"), "Unrelated", "Adele"),
                ])
                db.commit()

            app = PreviewApp(root, database, root, credentials, cache=root / "cache")
            listed = app.artists("")[0]
            self.assertEqual(listed["albums"], 1)
            # The fallback Album remains excluded from the Album count, but
            # its incomplete Artist-scoped COMP track must make Adele red.
            self.assertEqual(listed["status"], "incomplete")
            artist = app.artist("adele")
            self.assertEqual(
                [item["id"] for item in artist["albums"]],
                ["30", "billboard"],
            )
            self.assertEqual(artist["status"], "incomplete")
            self.assertEqual(set(artist["top_tracks"]), {"Easy on Me", "Unrelated"})

            with mock.patch.object(
                app,
                "album_cover_path",
                side_effect=AssertionError("album index must not touch network artwork paths"),
            ):
                albums = app.albums("", False)
            self.assertEqual({item["id"] for item in albums}, {"30"})
            self.assertTrue(all(item["cover"].startswith("/api/album/thumbnail?id=") for item in albums))
            self.assertEqual({item["id"] for item in app.albums("Adele", False)}, {"30"})
            self.assertEqual(app.albums("Billboard", False), [])
            self.assertEqual(
                [item["id"] for item in app.albums("Billboard", True)], ["billboard"]
            )

    def test_comp_only_identity_is_not_a_visible_media_library_artist(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            credentials = root / "credentials"
            credentials.mkdir()
            standard = root / "Adele" / "30"
            compilation = root / "Various Artists" / "Billboard"
            for folder in (standard, compilation):
                folder.mkdir(parents=True)
                (folder / "song.mp3").touch()

            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,album_name TEXT,"
                    "path TEXT,cover_path TEXT,track_count INTEGER,status TEXT,compilation INTEGER,"
                    "musicbrainz_albumid TEXT,release_year TEXT,embedded_tracks INTEGER,webp_found INTEGER)"
                )
                db.execute(
                    "CREATE TABLE tracks (track_key TEXT PRIMARY KEY,album_key TEXT,path TEXT,title TEXT,artist_name TEXT)"
                )
                db.executemany(
                    "INSERT INTO artists VALUES (?,?,?,?,?)",
                    [
                        ("adele", "Adele", "adele-mbid", str(root / "Adele"), "processed"),
                        ("various", "Various Artists", "", str(root / "Various Artists"), "unprocessed"),
                    ],
                )
                db.executemany(
                    "INSERT INTO albums VALUES (?,?,?,?,NULL,1,?,?,?, '2021',0,0)",
                    [
                        ("30", "adele", "30", str(standard), "processed", 0, "release-30"),
                        ("billboard", "various", "Billboard Hot 100", str(compilation), "incomplete", 1, ""),
                    ],
                )
                db.commit()

            app = PreviewApp(root, database, root, credentials, cache=root / "cache")
            self.assertEqual([item["name"] for item in app.artists("")], ["Adele"])
            with mock.patch.object(app, "_navidrome_artists_by_name", return_value={}):
                self.assertEqual(
                    app.artist_thumbnails(),
                    [{
                        "id": "adele",
                        "portrait": "",
                        "missing_artist_art": True,
                    }],
                )

    def test_persisted_artist_aggregate_excludes_unidentified_comp_fallback(self):
        with closing(connect(":memory:")) as db:
            db.execute(
                "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,status TEXT,"
                "album_count INTEGER,unprocessed_count INTEGER,processed_count INTEGER,"
                "bypassed_count INTEGER,timeout_count INTEGER,updated_at TEXT)"
            )
            db.execute(
                "CREATE TABLE albums (artist_key TEXT,status TEXT,compilation INTEGER,"
                "musicbrainz_albumid TEXT)"
            )
            db.execute(
                "INSERT INTO artists VALUES ('artist','partial',2,1,1,0,0,'old')"
            )
            db.executemany(
                "INSERT INTO albums VALUES ('artist',?,?,?)",
                [
                    ("processed", 0, "release-owned"),
                    ("processed", 1, "release-identified-compilation"),
                    ("incomplete", 1, ""),
                ],
            )

            update_artist_aggregate(db, "artist")
            row = db.execute(
                "SELECT status,album_count,unprocessed_count,processed_count "
                "FROM artists WHERE artist_key='artist'"
            ).fetchone()

        self.assertEqual(row, ("complete", 2, 0, 2))

    def test_identified_compilation_is_a_primary_artist_album(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            credentials = root / "credentials"
            credentials.mkdir()
            standard = root / "Aaron Neville" / "The Grand Tour"
            identified_compilation = root / "Aaron Neville" / "Gold"
            hidden_comp = root / "Various Artists" / "Hidden COMP"
            for folder in (standard, identified_compilation, hidden_comp):
                folder.mkdir(parents=True)
                (folder / "song.mp3").touch()

            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,album_name TEXT,"
                    "path TEXT,cover_path TEXT,track_count INTEGER,status TEXT,compilation INTEGER,"
                    "musicbrainz_albumid TEXT,release_year TEXT,embedded_tracks INTEGER,webp_found INTEGER)"
                )
                db.execute(
                    "CREATE TABLE tracks (track_key TEXT PRIMARY KEY,album_key TEXT,path TEXT,title TEXT,artist_name TEXT)"
                )
                db.execute(
                    "INSERT INTO artists VALUES ('aaron','Aaron Neville','artist-mbid',?,'processed')",
                    (str(root / "Aaron Neville"),),
                )
                db.executemany(
                    "INSERT INTO albums VALUES (?,?,?,?,NULL,1,'processed',?,?, '2026',1,0)",
                    [
                        ("tour", "aaron", "The Grand Tour", str(standard), 0, "release-tour"),
                        ("gold", "aaron", "Gold", str(identified_compilation), 1, "release-gold"),
                        ("hidden", "aaron", "Hidden COMP", str(hidden_comp), 1, ""),
                    ],
                )
                db.executemany(
                    "INSERT INTO tracks VALUES (?,?,?,?,?)",
                    [
                        ("tour-track", "tour", str(standard / "song.mp3"), "Tour Song", "Aaron Neville"),
                        ("gold-track", "gold", str(identified_compilation / "song.mp3"), "Gold Song", "Aaron Neville"),
                        ("hidden-track", "hidden", str(hidden_comp / "song.mp3"), "Hidden Song", "Aaron Neville"),
                    ],
                )
                db.commit()

            app = PreviewApp(root, database, root, credentials, cache=root / "cache")
            self.assertEqual(app.artists("")[0]["albums"], 2)
            artist = app.artist("aaron")
            self.assertEqual(
                {item["id"] for item in artist["albums"]},
                {"tour", "gold", "hidden"},
            )
            self.assertEqual(
                set(artist["top_tracks"]),
                {"Tour Song", "Gold Song", "Hidden Song"},
            )

    def test_all_track_credits_project_identified_albums_into_real_artists(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            credentials = root / "credentials"
            credentials.mkdir()
            gold = root / "Aaron Neville" / "Gold"
            soundtrack = root / "[Soundtracks]" / "Film"
            various_album = root / "[Various Artists]" / "Collection"
            curated_album = root / "[Various Artists]" / "Billboard" / "Hot 100"
            for folder in (gold, soundtrack, various_album, curated_album):
                folder.mkdir(parents=True)
                (folder / "song.mp3").touch()

            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,album_name TEXT,"
                    "path TEXT,cover_path TEXT,track_count INTEGER,status TEXT,compilation INTEGER,"
                    "musicbrainz_albumid TEXT,release_year TEXT,embedded_tracks INTEGER,webp_found INTEGER)"
                )
                db.execute(
                    "CREATE TABLE tracks (track_key TEXT PRIMARY KEY,album_key TEXT,path TEXT,title TEXT,"
                    "artist_name TEXT,musicbrainz_artistid TEXT)"
                )
                db.executemany(
                    "INSERT INTO artists VALUES (?,?,?,?,?)",
                    [
                        ("aaron", "Aaron Neville", "aaron-mbid", str(root / "Aaron Neville"), "processed"),
                        ("various", "Various Artists", "", str(root / "[Soundtracks]"), "processed"),
                    ],
                )
                db.executemany(
                    "INSERT INTO albums VALUES (?,?,?,?,NULL,?,?,1,?,'2008',1,0)",
                    [
                        ("gold", "aaron", "Gold", str(gold), 2, "processed", "release-gold"),
                        ("film", "various", "Film", str(soundtrack), 1, "incomplete", "release-film"),
                        ("collection", "various", "Collection", str(various_album), 1, "processed", "release-collection"),
                        ("hot-100", "various", "Hot 100", str(curated_album), 1, "incomplete", ""),
                    ],
                )
                db.executemany(
                    "INSERT INTO tracks VALUES (?,?,?,?,?,?)",
                    [
                        (
                            "gold-rob", "gold", str(gold / "1-16 - Rob Wasserman & Aaron Neville - Song.mp3"),
                            "Song", "Rob Wasserman & Aaron Neville", "rob-mbid; aaron-mbid",
                        ),
                        (
                            "gold-linda", "gold", str(gold / "1-17 - Linda Ronstadt & Aaron Neville - Duet.mp3"),
                            "Duet", "Linda Ronstadt & Aaron Neville", "linda-mbid; aaron-mbid",
                        ),
                        (
                            "film-rob", "film", str(soundtrack / "1-01 - Rob Wasserman - Theme.mp3"),
                            "Theme", "Various Artists", "",
                        ),
                        (
                            "collection-rob", "collection", str(various_album / "song.mp3"),
                            "Collection Song", "Rob Wasserman", "rob-mbid",
                        ),
                        (
                            "collection-only", "collection", str(various_album / "guest.mp3"),
                            "Guest Song", "V/A Only Performer", "guest-mbid",
                        ),
                        (
                            "hot-100-rob", "hot-100", str(curated_album / "song.mp3"),
                            "Compilation Song", "Rob Wasserman", "rob-mbid",
                        ),
                    ],
                )
                db.commit()

            app = PreviewApp(
                root,
                database,
                root,
                credentials,
                various_artist_path=root / "[Various Artists]",
                soundtracks_path=root / "[Soundtracks]",
                curated_compilations_path=root / "[Various Artists]" / "Billboard",
                cache=root / "cache",
            )
            artists = {item["name"]: item for item in app.artists("")}
            self.assertEqual(
                set(artists), {"Aaron Neville", "Linda Ronstadt", "Rob Wasserman"}
            )
            self.assertNotIn("Rob Wasserman & Aaron Neville", artists)
            self.assertNotIn("Linda Ronstadt & Aaron Neville", artists)
            self.assertNotIn("V/A Only Performer", artists)
            # Rob qualifies through Gold, then keeps the non-Fallback V/A and
            # soundtrack appearances in his expanded Artist file list.
            self.assertEqual(artists["Rob Wasserman"]["albums"], 3)
            rob = app.artist(artists["Rob Wasserman"]["id"])
            self.assertEqual(
                {item["id"] for item in rob["albums"]},
                {"gold", "film", "collection", "hot-100"},
            )
            gold_card = next(item for item in rob["albums"] if item["id"] == "gold")
            film_card = next(item for item in rob["albums"] if item["id"] == "film")
            collection_card = next(item for item in rob["albums"] if item["id"] == "collection")
            curated_card = next(item for item in rob["albums"] if item["id"] == "hot-100")
            self.assertTrue(gold_card["featured"])
            self.assertFalse(gold_card["various_artists"])
            self.assertFalse(film_card["featured"])
            self.assertTrue(film_card["various_artists"])
            self.assertEqual(collection_card["library_group"], "various-artists")
            self.assertEqual(curated_card["library_group"], "fallback")
            self.assertEqual(
                set(rob["top_tracks"]),
                {"Song", "Theme", "Collection Song", "Compilation Song"},
            )

    def test_projected_featured_artist_does_not_merge_into_album_owner(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            credentials = root / "credentials"
            credentials.mkdir()
            here = root / "Alicia Keys" / "Here"
            keys = root / "Alicia Keys" / "Keys"
            for folder in (here, keys):
                folder.mkdir(parents=True)
                (folder / "song.mp3").touch()

            alicia_mbid = "8ef1df30-ae4f-4dbd-9351-1a32b208a01e"
            asap_mbid = "25b7b584-d952-4662-a8b9-dd8cdfbfeb64"
            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,album_name TEXT,"
                    "path TEXT,cover_path TEXT,track_count INTEGER,status TEXT,compilation INTEGER,"
                    "musicbrainz_albumid TEXT,release_year TEXT,embedded_tracks INTEGER,webp_found INTEGER)"
                )
                db.execute(
                    "CREATE TABLE tracks (track_key TEXT PRIMARY KEY,album_key TEXT,path TEXT,title TEXT,"
                    "artist_name TEXT,musicbrainz_artistid TEXT)"
                )
                db.execute(
                    "INSERT INTO artists VALUES (?,?,?,?,?)",
                    (
                        "alicia",
                        "Alicia Keys",
                        alicia_mbid,
                        str(root / "Alicia Keys"),
                        "processed",
                    ),
                )
                db.executemany(
                    "INSERT INTO albums VALUES (?,?,?,?,NULL,1,'processed',0,?,'2021',1,0)",
                    [
                        ("here", "alicia", "Here", str(here), "release-here"),
                        ("keys", "alicia", "Keys", str(keys), "release-keys"),
                    ],
                )
                db.executemany(
                    "INSERT INTO tracks VALUES (?,?,?,?,?,?)",
                    [
                        (
                            "here-track",
                            "here",
                            str(here / "song.mp3"),
                            "Blended Family",
                            "Alicia Keys\x1fA$AP Rocky",
                            f"{alicia_mbid},{asap_mbid}",
                        ),
                        (
                            "keys-track",
                            "keys",
                            str(keys / "song.mp3"),
                            "Only You",
                            "Alicia Keys",
                            alicia_mbid,
                        ),
                    ],
                )
                db.commit()

            app = PreviewApp(root, database, root, credentials, cache=root / "cache")
            listed = app.artists("A$AP")
            self.assertEqual(
                [(item["name"], item["albums"]) for item in listed],
                [("A$AP Rocky", 1)],
            )

            # The UI identity boundary is authoritative. Even a stale or
            # colliding projection MBID must not alias this virtual Featured
            # Artist into a persisted Album owner's full catalog.
            with closing(app.connect()) as db:
                projections = app._projected_track_artists(db)
            projections[normalized_artist_identity("A$AP Rocky")][
                "musicbrainz_artistid"
            ] = alicia_mbid
            with mock.patch.object(
                app,
                "_projected_track_artists",
                return_value=projections,
            ):
                selected = app.artist(str(listed[0]["id"]))
            self.assertEqual(selected["name"], "A$AP Rocky")
            self.assertEqual([album["name"] for album in selected["albums"]], ["Here"])
            self.assertTrue(selected["albums"][0]["featured"])

    def test_conflicting_musicbrainz_ids_merge_when_library_path_is_identical(self):
        rows = [
            {
                "artist_key": "mbid:correct",
                "artist_name": "Aerosmith",
                "musicbrainz_artistid": "3d2b98e5-556f-4451-a3ff-c50ea18d57cb",
                "primary_path": "/music/Aerosmith",
                "album_count": 19,
            },
            {
                "artist_key": "mbid:stray-tag",
                "artist_name": "Aerosmith",
                "musicbrainz_artistid": "8e045c17-6447-471f-8169-50b7129f0cbb",
                "primary_path": "/music/Aerosmith",
                "album_count": 1,
            },
        ]
        groups = PreviewApp._artist_groups(rows)
        self.assertEqual(len(groups), 1)
        self.assertEqual(
            PreviewApp._canonical_artist_row(groups[0])["artist_key"],
            "mbid:correct",
        )

    def test_same_name_artist_rows_with_distinct_mbids_and_paths_share_one_presentation(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            credentials = root / "credentials"
            credentials.mkdir()
            album_rows = [
                ("album-one", "artist-one", "First Album", root / "Beyonce-One" / "First Album"),
                ("album-two", "artist-two", "Second Album", root / "Beyonce-Two" / "Second Album"),
            ]
            for _, _, _, folder in album_rows:
                folder.mkdir(parents=True)
                (folder / "song.mp3").touch()
            portrait = album_rows[0][3].parent / "artist.jpg"
            Image.new("RGB", (16, 16), "purple").save(portrait)

            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,album_count INTEGER,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,album_name TEXT,"
                    "path TEXT,cover_path TEXT,track_count INTEGER,status TEXT,compilation INTEGER,"
                    "musicbrainz_albumid TEXT,release_year TEXT,embedded_tracks INTEGER,webp_found INTEGER)"
                )
                db.execute(
                    "CREATE TABLE tracks (track_key TEXT PRIMARY KEY,album_key TEXT,path TEXT,title TEXT,artist_name TEXT)"
                )
                db.executemany("INSERT INTO artists VALUES (?,?,?,?,?,?)", [
                    ("artist-one", "Beyoncé", "mbid-one", str(root / "Beyonce-One"), 1, "processed"),
                    ("artist-two", "Beyoncé", "mbid-two", str(root / "Beyonce-Two"), 1, "processed"),
                ])
                db.executemany(
                    "INSERT INTO albums VALUES (?,?,?,?,NULL,1,'processed',0,?,'2026',1,0)",
                    [(key, artist, name, str(folder), "release-" + key)
                     for key, artist, name, folder in album_rows],
                )
                db.executemany(
                    "INSERT INTO tracks VALUES (?,?,?,?,?)",
                    [("track-" + key, key, str(folder / "song.mp3"), name + " Song", "Beyoncé")
                     for key, _, name, folder in album_rows],
                )
                db.commit()

            app = PreviewApp(root, database, root, credentials, cache=root / "cache")

            listed = [item for item in app.artists("") if item["name"] == "Beyoncé"]
            self.assertEqual(len(listed), 1)
            self.assertEqual(listed[0]["albums"], 2)
            selected = app.artist(str(listed[0]["id"]))
            self.assertEqual(
                {item["id"] for item in selected["albums"]},
                {"album-one", "album-two"},
            )
            self.assertEqual(app.artist_thumbnail(str(listed[0]["id"])), portrait)

    def test_first_load_merges_duplicate_identity_and_projects_every_album_class(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            credentials = root / "credentials"
            credentials.mkdir()
            (credentials / "navidrome.json").write_text(
                json.dumps({"server_url": "http://navidrome", "auth_method": "api_key", "api_key": "fixture"}),
                encoding="utf-8",
            )

            album_rows = [
                ("artist-2001", "nsync-main", "Celebrity", "Artist/Celebrity", "release-1", "2001", 0),
                ("artist-2000", "nsync-main", "No Strings Attached", "Artist/No Strings", "release-2", "2000", 0),
                ("artist-1998", "nsync-duplicate", "*NSYNC", "Artist/Debut", "release-3", "1998", 0),
                ("soundtrack", "soundtracks", "Dr. Seuss' The Grinch", "Soundtracks/Grinch", "release-4", "2000", 1),
                ("various", "various", "Now That's Music", "Various Artists/Now", "release-5", "2001", 1),
                ("comp", "various", "Billboard Hot 100", "Various Artists/Billboard", "", "1999", 1),
            ]
            for _, _, _, relative, _, _, _ in album_rows:
                folder = root / relative
                folder.mkdir(parents=True)
                (folder / "song.mp3").touch()

            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,album_count INTEGER,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,album_name TEXT,"
                    "path TEXT,cover_path TEXT,track_count INTEGER,status TEXT,compilation INTEGER,"
                    "musicbrainz_albumid TEXT,release_year TEXT,embedded_tracks INTEGER,webp_found INTEGER)"
                )
                db.execute(
                    "CREATE TABLE tracks (track_key TEXT PRIMARY KEY,album_key TEXT,path TEXT,title TEXT,artist_name TEXT)"
                )
                db.executemany("INSERT INTO artists VALUES (?,?,?,?,?,?)", [
                    ("nsync-main", "*NSYNC", "artist-mbid", str(root / "Artist"), 2, "processed"),
                    ("nsync-duplicate", "NSYNC", "", str(root / "Artist"), 1, "processed"),
                    ("soundtracks", "Soundtracks", "", str(root / "Soundtracks"), 1, "processed"),
                    ("various", "Various Artists", "", str(root / "Various Artists"), 2, "processed"),
                ])
                db.executemany(
                    "INSERT INTO albums VALUES (?,?,?,?,NULL,1,'processed',?,?,?,1,0)",
                    [(key, artist, name, str(root / relative), compilation, mbid, year)
                     for key, artist, name, relative, mbid, year, compilation in album_rows],
                )
                db.executemany(
                    "INSERT INTO tracks VALUES (?,?,?,?,?)",
                    [("track-" + key, key, str(root / relative / "song.mp3"), name + " Song",
                      "*NSYNC" if key in {"artist-2001", "artist-2000", "artist-1998", "soundtrack", "various", "comp"} else "")
                     for key, _, name, relative, _, _, _ in album_rows],
                )
                db.commit()

            app = PreviewApp(root, database, root, credentials, cache=root / "cache")

            def navidrome(_credential, method, parameters):
                if method == "getArtists":
                    return {"artists": {"index": [{"artist": [{"id": "remote-nsync", "name": "*NSYNC"}]}]}}
                self.assertEqual(method, "search3")
                self.assertEqual(parameters["songOffset"], "0")
                songs = []
                for key, _, name, relative, _, year, _ in album_rows:
                    songs.append({
                        "id": "remote-" + key,
                        "title": name + " Song",
                        "artist": "*NSYNC",
                        "artistId": "remote-nsync",
                        "album": name,
                        "albumId": "remote-album-" + key,
                        "albumArtist": "*NSYNC" if key.startswith("artist-") else "Various Artists",
                        "path": relative.replace("\\", "/") + "/song.mp3",
                        "year": year,
                    })
                return {"searchResult3": {"song": songs}}

            app._navidrome_call = navidrome

            listed = [item for item in app.artists("") if item["name"].casefold().endswith("nsync")]
            self.assertEqual(len(listed), 1)
            # The center overview retains the COMP card, while the expanded
            # Artist file list never counts Fallback.
            self.assertEqual(listed[0]["albums"], 5)
            self.assertNotIn(
                "Various Artists",
                {item["name"] for item in app.artists("")},
            )
            self.assertNotIn(
                "Soundtracks",
                {item["name"] for item in app.artists("")},
            )
            self.assertEqual(
                {item["id"] for item in app.albums("", False)},
                {"artist-2001", "artist-2000", "artist-1998", "soundtrack", "various"},
            )
            self.assertEqual([item["id"] for item in app.albums("", True)], ["comp"])

            artist = app.artist("nsync-duplicate")
            self.assertEqual(artist["id"], "nsync-main")
            self.assertEqual({item["id"] for item in artist["albums"]}, {
                "artist-2001", "artist-2000", "artist-1998", "soundtrack", "various", "comp"
            })
            self.assertTrue(next(item for item in artist["albums"] if item["id"] == "soundtrack")["soundtrack"])
            self.assertTrue(next(item for item in artist["albums"] if item["id"] == "various")["various_artists"])
            self.assertFalse(next(item for item in artist["albums"] if item["id"] == "soundtrack")["featured"])
            self.assertFalse(next(item for item in artist["albums"] if item["id"] == "various")["featured"])

            comp = app.artist_comp_albums(artist["id"])
            self.assertEqual([group["id"] for group in comp], ["comp"])
            self.assertEqual([track["key"] for track in comp[0]["tracks"]], ["track-comp"])

    def test_postgres_catalog_track_credits_project_featured_releases_without_comp_duplicates(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            credentials = root / "credentials"
            credentials.mkdir()
            folders = {
                name: root / name
                for name in (
                    "own",
                    "featured",
                    "filename-featured",
                    "ordinary-title",
                    "comp",
                )
            }
            for folder in folders.values():
                folder.mkdir()
                (folder / "song.mp3").touch()
            with closing(connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                    "musicbrainz_artistid TEXT,primary_path TEXT,status TEXT)"
                )
                db.execute(
                    "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,album_name TEXT,"
                    "path TEXT,cover_path TEXT,track_count INTEGER,status TEXT,compilation INTEGER,"
                    "musicbrainz_albumid TEXT,release_year TEXT,embedded_tracks INTEGER,webp_found INTEGER)"
                )
                db.execute(
                    "CREATE TABLE tracks (track_key TEXT PRIMARY KEY,album_key TEXT,path TEXT,title TEXT,artist_name TEXT)"
                )
                db.execute("CREATE TABLE compilation_track_artwork (track_path TEXT,outcome TEXT)")
                db.executemany("INSERT INTO artists VALUES (?,?,?,?,?)", [
                    ("target", "Target Artist", "target-mbid", str(folders["own"]), "unprocessed"),
                    ("owner", "Album Owner", "owner-mbid", str(folders["featured"]), "unprocessed"),
                    ("various", "Various Artists", "", str(folders["comp"]), "unprocessed"),
                ])
                db.executemany(
                    "INSERT INTO albums VALUES (?,?,?,?,NULL,1,'unprocessed',?,?, '2026',0,0)",
                    [
                        ("own", "target", "Own Release", str(folders["own"]), 0, "release-own"),
                        ("featured", "owner", "Featured Release", str(folders["featured"]), 0, "release-featured"),
                        (
                            "filename-featured",
                            "owner",
                            "Filename Featured Release",
                            str(folders["filename-featured"]),
                            0,
                            "release-filename-featured",
                        ),
                        (
                            "ordinary-title",
                            "owner",
                            "Ordinary Title",
                            str(folders["ordinary-title"]),
                            0,
                            "release-ordinary-title",
                        ),
                        ("comp", "various", "COMP Folder", str(folders["comp"]), 1, ""),
                    ],
                )
                db.executemany("INSERT INTO tracks VALUES (?,?,?,?,?)", [
                    ("own-track", "own", str(folders["own"] / "song.mp3"), "Own", "Target Artist"),
                    ("featured-track", "featured", str(folders["featured"] / "song.mp3"), "Guest", "Album Owner feat. Target Artist"),
                    (
                        "filename-featured-track",
                        "filename-featured",
                        str(folders["filename-featured"] / "song.mp3"),
                        "1-07 - Album Owner feat. Target Artist - Guest Song",
                        "Album Owner",
                    ),
                    (
                        "ordinary-title-track",
                        "ordinary-title",
                        str(folders["ordinary-title"] / "song.mp3"),
                        "A Song About Target Artist",
                        "Album Owner",
                    ),
                    ("comp-track", "comp", str(folders["comp"] / "song.mp3"), "COMP", "Album Owner feat. Target Artist"),
                ])
                db.commit()

            app = PreviewApp(root, database, root, credentials, cache=root / "cache")
            target_list_row = next(
                item for item in app.artists("") if item["name"] == "Target Artist"
            )
            self.assertEqual(target_list_row["status"], "incomplete")

            artist = app.artist("target")
            albums = {item["id"]: item for item in artist["albums"]}
            self.assertEqual(
                set(albums),
                {"own", "featured", "filename-featured", "comp"},
            )
            self.assertFalse(albums["own"]["featured"])
            self.assertTrue(albums["featured"]["featured"])
            self.assertTrue(albums["filename-featured"]["featured"])
            self.assertFalse(albums["comp"]["featured"])
            self.assertEqual([group["id"] for group in app.artist_comp_albums("target")], ["comp"])

            with closing(connect(database)) as db:
                db.execute(
                    "INSERT INTO compilation_track_artwork VALUES (?,?)",
                    (str(folders["comp"] / "song.mp3"), "embedded-existing"),
                )
                db.commit()
            target_list_row = next(
                item for item in app.artists("") if item["name"] == "Target Artist"
            )
            # Once COMP is complete, these Albums remain unprocessed with
            # no indexed static covers: Artist Yellow outranks the default Red.
            self.assertEqual(target_list_row["status"], "bypassed")
            self.assertFalse(target_list_row["has_incomplete_error"])


if __name__ == "__main__":
    unittest.main()
