from __future__ import annotations

from pathlib import Path
import sqlite3
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import Mock, patch

from mutagen.id3 import ID3, TALB, TCMP, TIT2, TPE1, TPE2, TXXX, UFID

import splined
import splined_compilation_authority_policy as policy
from splined_media_database import schema_path
import splined_media_tags


RECORDING_ID = "59a0c68f-ec68-418d-a29a-fa54a7d9aea9"
ARTIST_ID = "291dcfb8-b31c-496a-905b-9955509d75b6"
OTHER_ARTIST_ID = "11111111-1111-4111-8111-111111111111"
RELEASE_ID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"


class FakeResponse:
    def __init__(self, status_code: int, payload: dict | None = None) -> None:
        self.status_code = status_code
        self._payload = payload or {}

    def json(self) -> dict:
        return self._payload


class CompilationAuthorityPolicyTests(unittest.TestCase):
    def setUp(self) -> None:
        policy._INSTALLED = False

    def test_album_and_track_eligibility_use_local_tags_only(self) -> None:
        missing_album = SimpleNamespace(
            compilation="1",
            album_mbid=None,
            recording_mbid=RECORDING_ID,
            artist_mbid=ARTIST_ID,
        )
        normal = SimpleNamespace(
            compilation="1",
            album_mbid="aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            recording_mbid=RECORDING_ID,
            artist_mbid=ARTIST_ID,
        )
        self.assertTrue(policy.manual_album_eligible([missing_album]))
        self.assertFalse(policy.manual_album_eligible([normal]))
        self.assertEqual(policy.manual_track_eligible(missing_album), (True, ""))

        missing_album.artist_mbid = f"{ARTIST_ID}; {OTHER_ARTIST_ID}"
        self.assertEqual(policy.manual_track_eligible(missing_album), (True, ""))

        missing_album.recording_mbid = None
        eligible, reason = policy.manual_track_eligible(missing_album)
        self.assertFalse(eligible)
        self.assertIn("Recording ID", reason)

    def test_completed_track_ledger_resumes_and_finishes_album(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "splined.db"
            connection = sqlite3.connect(database)
            connection.executescript(
                schema_path().read_text(encoding="utf-8")
            )
            connection.close()
            first = SimpleNamespace(
                path=root / "01.flac",
                recording_mbid=RECORDING_ID,
                artist_mbid=ARTIST_ID,
            )
            second = SimpleNamespace(
                path=root / "02.flac",
                recording_mbid="22222222-2222-4222-8222-222222222222",
                artist_mbid=ARTIST_ID,
            )
            core = SimpleNamespace(
                runtime_cache_dir=lambda *_args: root,
                display_version=lambda: "test",
            )

            policy.record_application(
                core,
                root / "config.toml",
                {},
                first,
                source_kind="local-library",
                source_locator=str(root / "cover.jpg"),
                release_mbid="aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                artwork=b"first",
                outcome="embedded-replaced",
                album_path=str(root / "Compilation"),
                total_tracks=2,
                completed_tracks=1,
            )
            completed = policy.completed_track_paths(
                core,
                root / "config.toml",
                {},
                str(root / "Compilation"),
                [first, second],
            )
            self.assertEqual(completed, {str(first.path)})

            policy.record_application(
                core,
                root / "config.toml",
                {},
                second,
                source_kind="coverartarchive",
                source_locator="https://example.test/cover.jpg",
                release_mbid="bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                artwork=b"second",
                outcome="embedded-replaced",
                album_path=str(root / "Compilation"),
                total_tracks=2,
                completed_tracks=2,
            )
            connection = sqlite3.connect(database)
            row = connection.execute(
                "SELECT total_tracks, completed_tracks, status "
                "FROM compilation_album_progress"
            ).fetchone()
            connection.close()
            self.assertEqual(row, (2, 2, "complete"))

    def test_zero_completed_progress_is_persisted_as_incomplete(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            connection = sqlite3.connect(root / "splined.db")
            connection.executescript(
                schema_path().read_text(encoding="utf-8")
            )
            connection.close()
            core = SimpleNamespace(
                runtime_cache_dir=lambda *_args: root,
                display_version=lambda: "test",
            )
            policy.record_progress(
                core,
                root / "config.toml",
                {},
                str(root / "Compilation"),
                100,
                0,
            )
            connection = sqlite3.connect(root / "splined.db")
            row = connection.execute(
                "SELECT total_tracks, completed_tracks, status "
                "FROM compilation_album_progress"
            ).fetchone()
            connection.close()
            self.assertEqual(row, (100, 0, "incomplete"))

    def test_standard_musicbrainz_ufid_is_the_recording_id(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "track.mp3"
            tags = ID3()
            tags.add(TIT2(encoding=3, text=["Theme From A Summer Place"]))
            tags.add(TPE1(encoding=3, text=["Percy Faith"]))
            tags.add(TALB(encoding=3, text=["Curated Compilation"]))
            tags.add(TPE2(encoding=3, text=["Various Artists"]))
            tags.add(TCMP(encoding=3, text=["1"]))
            tags.add(
                TXXX(
                    encoding=3,
                    desc="MusicBrainz Artist Id",
                    text=[f"{ARTIST_ID}; {OTHER_ARTIST_ID}"],
                )
            )
            tags.add(
                UFID(
                    owner="http://musicbrainz.org",
                    data=RECORDING_ID.encode("ascii"),
                )
            )
            tags.save(path, v2_version=4)

            runtime_track = splined.read_track(path)
            self.assertEqual(runtime_track.recording_mbid, RECORDING_ID)
            self.assertEqual(
                policy._mbids(runtime_track.artist_mbid),
                {ARTIST_ID, OTHER_ARTIST_ID},
            )

            parsed = SimpleNamespace(tags=ID3(path))
            with patch.object(
                splined_media_tags,
                "MutagenFile",
                return_value=parsed,
            ):
                identity = splined_media_tags.read_track_identity_tags(path)
            self.assertEqual(identity["recording_mbid"], RECORDING_ID)
            self.assertEqual(
                policy._mbids(identity["artist_mbid"]),
                {ARTIST_ID, OTHER_ARTIST_ID},
            )

    def test_credential_options_use_exact_json_names(self) -> None:
        core = SimpleNamespace(
            credential_file=lambda *_args: Path("musicbrainz.json"),
            load_json=lambda *_args: {
                "options": {
                    "min_delay": 1.05,
                    "recording_timeout": 7,
                    "retry_max": 4,
                }
            },
            SplinedError=ValueError,
        )
        result = policy.credential_options(core, Path("config.toml"), {})
        self.assertEqual(result, policy.MusicBrainzOptions(4, 1.05, 7.0))

        core.load_json = lambda *_args: {"options": {"retry_max": 4}}
        with self.assertRaisesRegex(ValueError, "min_delay"):
            policy.credential_options(core, Path("config.toml"), {})

    def test_release_order_is_album_soundtrack_compilation(self) -> None:
        def release(
            release_id: str,
            title: str,
            primary: str,
            secondary: list[str],
            date: str,
        ) -> dict:
            return {
                "id": release_id,
                "title": title,
                "date": date,
                "status": "Official",
                "artist-credit": [
                    {"artist": {"id": ARTIST_ID, "name": "Artist"}}
                ],
                "release-group": {
                    "id": "99999999-9999-4999-8999-999999999999",
                    "primary-type": primary,
                    "secondary-types": secondary,
                },
            }

        payload = {
            "releases": [
                release("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "OST", "Album", ["Soundtrack"], "1970"),
                release("cccccccc-cccc-4ccc-8ccc-cccccccccccc", "Comp", "Album", ["Compilation"], "1980"),
                release("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "Original", "Album", [], "1960"),
                release("dddddddd-dddd-4ddd-8ddd-dddddddddddd", "Single", "Single", [], "1959"),
                release("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee", "Live", "Album", ["Live"], "1961"),
            ]
        }
        result = policy._rank_releases(payload, RECORDING_ID, ARTIST_ID)
        self.assertEqual(
            [item.release_class for item in result],
            ["album", "soundtrack", "compilation"],
        )
        self.assertEqual(result[0].release_title, "Original")

    def test_retry_max_is_total_attempt_count(self) -> None:
        http = SimpleNamespace(
            last_mb_request=None,
            get=Mock(return_value=FakeResponse(503)),
        )
        core = SimpleNamespace(
            MB_BASE="https://musicbrainz.invalid/ws/2",
            mb_headers=lambda *_args: ({"Authorization": "Bearer secret"}, "OAuthBearer"),
        )
        track = SimpleNamespace(
            recording_mbid=RECORDING_ID,
            artist_mbid=ARTIST_ID,
        )
        with (
            patch.object(policy, "_cached_releases", return_value=()),
            patch.object(policy.time, "sleep"),
        ):
            result = policy.resolve_release_candidates(
                core,
                http,
                Path("config.toml"),
                {},
                track,
                policy.MusicBrainzOptions(4, 1.05, 7.0),
            )
        self.assertEqual(http.get.call_count, 4)
        self.assertEqual(result.source, "musicbrainz")
        self.assertIn("503", result.error)
        self.assertTrue(
            all(call.kwargs["timeout"] == 7.0 for call in http.get.call_args_list)
        )

    def test_oauth_setup_failure_is_reported_without_http(self) -> None:
        http = SimpleNamespace(
            last_mb_request=None,
            get=Mock(side_effect=AssertionError("HTTP must not run")),
        )
        core = SimpleNamespace(
            MB_BASE="https://musicbrainz.invalid/ws/2",
            mb_headers=Mock(side_effect=ValueError("private detail")),
        )
        track = SimpleNamespace(
            recording_mbid=RECORDING_ID,
            artist_mbid=ARTIST_ID,
        )
        with patch.object(policy, "_cached_releases", return_value=()):
            result = policy.resolve_release_candidates(
                core,
                http,
                Path("config.toml"),
                {},
                track,
                policy.MusicBrainzOptions(4, 1.05, 7.0),
            )
        self.assertEqual(result.candidates, ())
        self.assertEqual(result.source, "musicbrainz")
        self.assertIn("authentication/refresh failed", result.error)
        self.assertNotIn("private detail", result.error)
        http.get.assert_not_called()

    def test_positive_recording_release_cache_avoids_http(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            connection = sqlite3.connect(root / "splined.db")
            connection.executescript(
                "CREATE TABLE recording_release_lookups ("
                "recording_mbid TEXT PRIMARY KEY, artist_mbids_key TEXT, "
                "fetched_at TEXT, splined_version TEXT);"
                "CREATE TABLE recording_release_candidates ("
                "recording_mbid TEXT, release_mbid TEXT, "
                "release_group_mbid TEXT, release_class TEXT, "
                "class_rank INTEGER, candidate_rank INTEGER, "
                "release_title TEXT, release_artist TEXT, "
                "artist_mbids_key TEXT, release_date TEXT, "
                "PRIMARY KEY(recording_mbid, release_mbid));"
            )
            connection.close()
            core = SimpleNamespace(
                runtime_cache_dir=lambda *_args: root,
                display_version=lambda: "test",
            )
            candidate = policy.ReleaseCandidate(
                RECORDING_ID,
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "99999999-9999-4999-8999-999999999999",
                "album",
                0,
                0,
                "Original",
                "Artist",
                ARTIST_ID,
                "1960",
            )
            policy._cache_releases(
                core,
                root / "config.toml",
                {},
                (candidate,),
            )
            http = SimpleNamespace(
                get=Mock(side_effect=AssertionError("HTTP must not run"))
            )
            result = policy.resolve_release_candidates(
                core,
                http,
                root / "config.toml",
                {},
                SimpleNamespace(
                    recording_mbid=RECORDING_ID,
                    artist_mbid=ARTIST_ID,
                ),
                policy.MusicBrainzOptions(4, 1.05, 7.0),
            )
            self.assertEqual(result.source, "sql-cache")
            self.assertEqual(result.candidates, (candidate,))
            http.get.assert_not_called()

    def test_force_refresh_bypasses_positive_release_cache(self) -> None:
        cached = policy.ReleaseCandidate(
            RECORDING_ID,
            RELEASE_ID,
            "",
            "album",
            0,
            0,
            "Cached",
            "Percy Faith",
            ARTIST_ID,
            "1960",
        )
        payload = {
            "id": RECORDING_ID,
            "title": "Theme From A Summer Place",
            "artist-credit": [
                {"artist": {"id": ARTIST_ID, "name": "Percy Faith"}}
            ],
            "releases": [
                {
                    "id": RELEASE_ID,
                    "title": "A Summer Place",
                    "status": "Official",
                    "date": "1960",
                    "artist-credit": [
                        {"artist": {"id": ARTIST_ID, "name": "Percy Faith"}}
                    ],
                    "release-group": {
                        "id": "99999999-9999-4999-8999-999999999999",
                        "primary-type": "Album",
                        "secondary-types": [],
                    },
                }
            ],
        }
        http = SimpleNamespace(
            last_mb_request=None,
            get=Mock(return_value=FakeResponse(200, payload)),
        )
        core = SimpleNamespace(
            MB_BASE="https://musicbrainz.invalid/ws/2",
            mb_headers=lambda *_args: ({}, "OAuthBearer"),
        )
        with (
            patch.object(policy, "_cached_releases", return_value=(cached,)),
            patch.object(policy, "_cache_releases"),
        ):
            result = policy.resolve_release_candidates(
                core,
                http,
                Path("config.toml"),
                {},
                SimpleNamespace(
                    recording_mbid=RECORDING_ID,
                    artist_mbid=ARTIST_ID,
                ),
                policy.MusicBrainzOptions(1, 1.05, 7.0),
                force_refresh=True,
            )
        self.assertEqual(result.source, "musicbrainz")
        self.assertEqual(result.candidates[0].release_title, "A Summer Place")
        http.get.assert_called_once()

    def test_exact_single_release_override_validates_recording_and_artist(self) -> None:
        payload = {
            "id": RELEASE_ID,
            "title": "Go, Jimmy, Go",
            "status": "Official",
            "date": "1960",
            "artist-credit": [
                {"artist": {"id": ARTIST_ID, "name": "Percy Faith"}}
            ],
            "release-group": {
                "id": "99999999-9999-4999-8999-999999999999",
                "primary-type": "Single",
                "secondary-types": [],
            },
            "media": [
                {
                    "tracks": [
                        {
                            "recording": {
                                "id": RECORDING_ID,
                                "title": "Theme From A Summer Place",
                                "artist-credit": [
                                    {
                                        "artist": {
                                            "id": ARTIST_ID,
                                            "name": "Percy Faith",
                                        }
                                    }
                                ],
                            }
                        }
                    ]
                }
            ],
        }
        http = SimpleNamespace(
            last_mb_request=None,
            get=Mock(return_value=FakeResponse(200, payload)),
        )
        core = SimpleNamespace(
            MB_BASE="https://musicbrainz.invalid/ws/2",
            mb_headers=lambda *_args: ({}, "OAuthBearer"),
        )
        result = policy.resolve_release_by_id(
            core,
            http,
            Path("config.toml"),
            {},
            SimpleNamespace(
                recording_mbid=RECORDING_ID,
                artist_mbid=ARTIST_ID,
            ),
            RELEASE_ID,
            policy.MusicBrainzOptions(1, 1.05, 7.0),
        )
        self.assertEqual(result.source, "musicbrainz-manual-release")
        self.assertEqual(result.candidates[0].release_mbid, RELEASE_ID)
        self.assertEqual(result.candidates[0].release_class, "single")
        self.assertEqual(result.recording_artist, "Percy Faith")
        self.assertIn(f"/release/{RELEASE_ID}", http.get.call_args.args[0])

    def test_exact_release_override_rejects_non_official_release(self) -> None:
        payload = {
            "id": RELEASE_ID,
            "title": "Unofficial release",
            "status": "Bootleg",
            "release-group": {
                "id": "99999999-9999-4999-8999-999999999999",
                "primary-type": "Album",
                "secondary-types": [],
            },
        }
        http = SimpleNamespace(
            last_mb_request=None,
            get=Mock(return_value=FakeResponse(200, payload)),
        )
        core = SimpleNamespace(
            MB_BASE="https://musicbrainz.invalid/ws/2",
            mb_headers=lambda *_args: ({}, "OAuthBearer"),
        )
        result = policy.resolve_release_by_id(
            core,
            http,
            Path("config.toml"),
            {},
            SimpleNamespace(
                recording_mbid=RECORDING_ID,
                artist_mbid=ARTIST_ID,
            ),
            RELEASE_ID,
            policy.MusicBrainzOptions(1, 1.05, 7.0),
        )
        self.assertEqual(result.candidates, ())
        self.assertEqual(result.source, "musicbrainz")
        self.assertEqual(result.error, "Release is not Official")

    def test_embedded_artwork_is_capped_by_configured_ladder(self) -> None:
        captured: dict = {}

        def prepare(_candidate, cfg, target, **kwargs):
            captured.update(cfg=cfg, target=target, kwargs=kwargs)
            return b"image", {"width": 3600, "height": 3600}

        core = SimpleNamespace(
            section=lambda cfg, name: cfg[name],
            formats=lambda _cfg: ["jpeg"],
            prepare_final=prepare,
            SplinedError=ValueError,
        )
        cfg = {
            "range": {"ideal": 1800, "ladder": 3600},
            "output": {
                "upscale_below_ideal": True,
                "evaluate_final_image": False,
            },
        }
        candidate = SimpleNamespace(format="jpeg")
        policy.prepare_embedded_artwork(core, candidate, cfg)
        self.assertEqual(captured["cfg"]["range"]["ideal"], 3600)
        self.assertFalse(captured["cfg"]["output"]["upscale_below_ideal"])
        self.assertTrue(captured["cfg"]["output"]["evaluate_final_image"])
        self.assertEqual(cfg["range"]["ideal"], 1800)

    def test_manual_local_lookup_is_lazy_artist_scoped_and_cached(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "splined.db"
            album_path = root / "Artist" / "Album"
            other_path = root / "Other" / "Album"
            album_path.mkdir(parents=True)
            other_path.mkdir(parents=True)
            track_path = album_path / "01.flac"
            other_track = other_path / "01.flac"
            track_path.write_bytes(b"track")
            other_track.write_bytes(b"other")
            cover_path = album_path / "cover.jpg"
            other_cover = other_path / "cover.jpg"
            cover_path.write_bytes(b"cover")
            other_cover.write_bytes(b"cover")

            connection = sqlite3.connect(database)
            connection.executescript(
                "CREATE TABLE artists (artist_key TEXT PRIMARY KEY, "
                "artist_name TEXT, musicbrainz_artistid TEXT);"
                "CREATE TABLE albums (album_key TEXT PRIMARY KEY, "
                "artist_key TEXT, album_name TEXT, musicbrainz_albumid TEXT, "
                "musicbrainz_releasegroupid TEXT, release_year TEXT, "
                "compilation INTEGER, path TEXT, cover_path TEXT, "
                "cover_format TEXT, cover_width INTEGER, cover_height INTEGER, "
                "cover_found INTEGER);"
                "CREATE TABLE tracks (track_key TEXT PRIMARY KEY, album_key TEXT, "
                "path TEXT, title TEXT, artist_name TEXT, "
                "musicbrainz_recordingid TEXT, musicbrainz_artistid TEXT, "
                "file_size INTEGER, file_mtime_ns INTEGER, updated_at TEXT, "
                "splined_version TEXT);"
            )
            connection.executemany(
                "INSERT INTO artists VALUES(?, ?, ?)",
                [
                    ("artist", "Artist", ARTIST_ID),
                    ("other", "Other", OTHER_ARTIST_ID),
                ],
            )
            connection.executemany(
                "INSERT INTO albums VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                [
                    ("album", "artist", "Original", "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "", "1960", 0, str(album_path), str(cover_path), "JPEG", 1000, 1000, 1),
                    ("other-album", "other", "Other", "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "", "1961", 0, str(other_path), str(other_cover), "JPEG", 1000, 1000, 1),
                ],
            )
            connection.commit()
            connection.close()

            core = SimpleNamespace(
                runtime_cache_dir=lambda *_args: root,
                AUDIO_EXTENSIONS={".flac"},
                display_version=lambda: "test",
            )
            source_track = SimpleNamespace(
                recording_mbid=RECORDING_ID,
                artist_mbid=ARTIST_ID,
            )
            stat = track_path.stat()
            indexed_rows = [
                {
                    "track_key": str(track_path),
                    "album_key": "album",
                    "path": str(track_path),
                    "title": "Song",
                    "artist_name": "Artist",
                    "musicbrainz_recordingid": RECORDING_ID,
                    "musicbrainz_artistid": ARTIST_ID,
                    "file_size": stat.st_size,
                    "file_mtime_ns": stat.st_mtime_ns,
                    "updated_at": "now",
                }
            ]
            with patch.object(
                policy,
                "inspect_album_tracks",
                return_value=(indexed_rows, 1, 0),
            ) as inspect:
                first = policy.local_artwork_rows(
                    core,
                    root / "config.toml",
                    {},
                    source_track,
                    current_album_path=root / "Compilation",
                )
            self.assertEqual(len(first), 1)
            self.assertEqual(first[0]["album_name"], "Original")
            self.assertEqual(inspect.call_count, 1)
            self.assertEqual(inspect.call_args.args[0].audio_files, [track_path])

            with patch.object(
                policy,
                "inspect_album_tracks",
                side_effect=AssertionError("SQL cache should satisfy lookup"),
            ):
                second = policy.local_artwork_rows(
                    core,
                    root / "config.toml",
                    {},
                    source_track,
                    current_album_path=root / "Compilation",
                )
            self.assertEqual(len(second), 1)

    def test_manual_local_cache_accepts_multi_artist_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "splined.db"
            album_path = root / "Artist" / "Album"
            album_path.mkdir(parents=True)
            track_path = album_path / "01.flac"
            cover_path = album_path / "cover.jpg"
            track_path.write_bytes(b"track")
            cover_path.write_bytes(b"cover")
            stat = track_path.stat()
            artist_key = policy._artist_key({ARTIST_ID, OTHER_ARTIST_ID})

            connection = sqlite3.connect(database)
            connection.executescript(
                "CREATE TABLE artists (artist_key TEXT PRIMARY KEY, "
                "artist_name TEXT, musicbrainz_artistid TEXT);"
                "CREATE TABLE albums (album_key TEXT PRIMARY KEY, "
                "artist_key TEXT, album_name TEXT, musicbrainz_albumid TEXT, "
                "musicbrainz_releasegroupid TEXT, release_year TEXT, "
                "compilation INTEGER, path TEXT, cover_path TEXT, "
                "cover_format TEXT, cover_width INTEGER, cover_height INTEGER, "
                "cover_found INTEGER);"
                "CREATE TABLE tracks (track_key TEXT PRIMARY KEY, album_key TEXT, "
                "path TEXT, title TEXT, artist_name TEXT, "
                "musicbrainz_recordingid TEXT, musicbrainz_artistid TEXT, "
                "file_size INTEGER, file_mtime_ns INTEGER, updated_at TEXT, "
                "splined_version TEXT);"
            )
            connection.execute(
                "INSERT INTO artists VALUES(?, ?, ?)",
                ("artist", "Artist", ARTIST_ID),
            )
            connection.execute(
                "INSERT INTO albums VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    "album", "artist", "Original",
                    "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "", "1960", 0,
                    str(album_path), str(cover_path), "JPEG", 1000, 1000, 1,
                ),
            )
            connection.execute(
                "INSERT INTO tracks VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    str(track_path), "album", str(track_path), "Duet", "Artists",
                    RECORDING_ID, artist_key, stat.st_size, stat.st_mtime_ns,
                    "now", "test",
                ),
            )
            connection.commit()
            connection.close()

            core = SimpleNamespace(
                runtime_cache_dir=lambda *_args: root,
                AUDIO_EXTENSIONS={".flac"},
                display_version=lambda: "test",
            )
            rows = policy.local_artwork_rows(
                core,
                root / "config.toml",
                {},
                SimpleNamespace(
                    recording_mbid=RECORDING_ID,
                    artist_mbid=f"{ARTIST_ID}; {OTHER_ARTIST_ID}",
                ),
                current_album_path=root / "Compilation",
            )
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0]["album_name"], "Original")

    def test_install_does_not_wrap_normal_track_reader(self) -> None:
        reader = lambda _album: ["normal"]
        core = SimpleNamespace(read_album_tracks=reader)
        policy.install(core)
        self.assertIs(core.read_album_tracks, reader)
        self.assertTrue(callable(core.compilation_local_artwork_rows))
        self.assertTrue(callable(core.compilation_resolve_releases))
        self.assertTrue(callable(core.compilation_resolve_release_id))
        self.assertTrue(callable(core.compilation_record_progress))


if __name__ == "__main__":
    unittest.main()
