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
    artist_display_sort_key,
    embedded_album_artwork_summary,
)
from splined_media_database import update_artist_aggregate


class ArtistCatalogProjectionTests(unittest.TestCase):
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

    def test_display_sort_keeps_symbol_prefixed_artist_before_letters(self):
        artists = ["Aaliyah", "*NSYNC", "10,000 Maniacs", "Nirvana"]
        self.assertEqual(
            sorted(artists, key=artist_display_sort_key),
            ["*NSYNC", "10,000 Maniacs", "Aaliyah", "Nirvana"],
        )

    def test_artist_list_is_database_only_until_lazy_thumbnail_hydration(self):
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
                    "compilation INTEGER,musicbrainz_albumid TEXT)"
                )
                db.execute(
                    "INSERT INTO artists VALUES ('artist-1','Artist','',?,'unprocessed')",
                    (str(root / "Artist"),),
                )
                db.execute("INSERT INTO albums VALUES ('album-1','artist-1',0,'')")
                db.commit()
            app = PreviewApp(root, database, root, root)
            with mock.patch.object(
                app,
                "artist_thumbnail_path",
                side_effect=AssertionError("initial list must not resolve artwork paths"),
            ):
                self.assertEqual(app.artists("")[0]["portrait"], None)

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
            self.assertEqual(listed["status"], "complete")
            artist = app.artist("adele")
            self.assertEqual([item["id"] for item in artist["albums"]], ["30"])
            self.assertEqual(artist["status"], "complete")
            self.assertEqual(artist["top_tracks"], ["Easy on Me"])

            with mock.patch.object(
                app,
                "album_cover_path",
                side_effect=AssertionError("album index must not touch network artwork paths"),
            ):
                albums = app.albums("")
            self.assertEqual({item["id"] for item in albums}, {"30", "billboard"})
            self.assertTrue(all(item["cover"].startswith("/api/album/thumbnail?id=") for item in albums))
            self.assertEqual(
                {item["id"] for item in app.albums("Adele")}, {"30", "billboard"}
            )
            self.assertEqual(
                [item["id"] for item in app.albums("Billboard")], ["billboard"]
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
            self.assertEqual({item["id"] for item in artist["albums"]}, {"tour", "gold"})
            self.assertEqual(set(artist["top_tracks"]), {"Tour Song", "Gold Song"})

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

    def test_posix_paths_differing_only_by_case_remain_separate(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "index.sqlite"
            credentials = root / "credentials"
            credentials.mkdir()
            album_rows = [
                ("album-upper", "artist-upper", "Upper Album", root / "albums" / "upper"),
                ("album-lower", "artist-lower", "Lower Album", root / "albums" / "lower"),
            ]
            for _, _, _, folder in album_rows:
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
                    ("artist-upper", "Band", "mbid-upper", "/music/Band", 1, "processed"),
                    ("artist-lower", "Band", "mbid-lower", "/music/band", 1, "processed"),
                ])
                db.executemany(
                    "INSERT INTO albums VALUES (?,?,?,?,NULL,1,'processed',0,?,'2026',1,0)",
                    [(key, artist, name, str(folder), "release-" + key)
                     for key, artist, name, folder in album_rows],
                )
                db.executemany(
                    "INSERT INTO tracks VALUES (?,?,?,?,?)",
                    [("track-" + key, key, str(folder / "song.mp3"), name + " Song", "Band")
                     for key, _, name, folder in album_rows],
                )
                db.commit()

            app = PreviewApp(root, database, root, credentials, cache=root / "cache")

            listed = [item for item in app.artists("") if item["name"] == "Band"]
            self.assertEqual({item["id"] for item in listed}, {"artist-upper", "artist-lower"})
            self.assertEqual(
                {item["id"] for item in app.artist("artist-upper")["albums"]},
                {"album-upper"},
            )
            self.assertEqual(
                {item["id"] for item in app.artist("artist-lower")["albums"]},
                {"album-lower"},
            )

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
            self.assertEqual(listed[0]["albums"], 3)

            artist = app.artist("nsync-duplicate")
            self.assertEqual(artist["id"], "nsync-main")
            self.assertEqual({item["id"] for item in artist["albums"]}, {
                "artist-2001", "artist-2000", "artist-1998", "soundtrack", "various"
            })
            self.assertTrue(next(item for item in artist["albums"] if item["id"] == "soundtrack")["soundtrack"])
            self.assertTrue(next(item for item in artist["albums"] if item["id"] == "various")["various_artists"])

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
                        "1-07 - Album Owner & Target Artist - Guest Song",
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
            artist = app.artist("target")
            albums = {item["id"]: item for item in artist["albums"]}
            self.assertEqual(set(albums), {"own", "featured", "filename-featured"})
            self.assertFalse(albums["own"]["featured"])
            self.assertTrue(albums["featured"]["featured"])
            self.assertTrue(albums["filename-featured"]["featured"])
            self.assertEqual([group["id"] for group in app.artist_comp_albums("target")], ["comp"])


if __name__ == "__main__":
    unittest.main()
