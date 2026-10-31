"""Artist Overview uses one canonical identity and complete Navidrome evidence."""

from __future__ import annotations

from contextlib import closing
import json
from pathlib import Path
from sqlite3 import connect
from tempfile import TemporaryDirectory
import unittest

from webgui.catalog import PreviewApp


class ArtistCatalogProjectionTests(unittest.TestCase):
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

    def test_conflicting_musicbrainz_ids_merge_when_library_path_is_identical(self):
        rows = [
            {
                "artist_key": "mbid:correct",
                "artist_name": "Aerosmith",
                "musicbrainz_artistid": "3d2b98e5-556f-4451-a3ff-c50ea18d57cb",
                "primary_path": "N:/media/music/Aerosmith",
                "album_count": 19,
            },
            {
                "artist_key": "mbid:stray-tag",
                "artist_name": "Aerosmith",
                "musicbrainz_artistid": "8e045c17-6447-471f-8169-50b7129f0cbb",
                "primary_path": "N:/media/music/Aerosmith",
                "album_count": 1,
            },
        ]
        groups = PreviewApp._artist_groups(rows)
        self.assertEqual(len(groups), 1)
        self.assertEqual(
            PreviewApp._canonical_artist_row(groups[0])["artist_key"],
            "mbid:correct",
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


if __name__ == "__main__":
    unittest.main()
