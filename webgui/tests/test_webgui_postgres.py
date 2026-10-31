"""Integration tests use a dedicated PostgreSQL schema and generated silent audio."""

from __future__ import annotations
from contextlib import closing
from io import BytesIO
import os
from pathlib import Path
import shutil
import tempfile
import time
import tomllib
import unittest
from unittest.mock import patch
from types import SimpleNamespace
from urllib.parse import urlsplit, urlunsplit, parse_qsl, urlencode, quote
from uuid import uuid4

import psycopg
from psycopg import sql
from fastapi.testclient import TestClient
from PIL import Image
import numpy as np
import soundfile as sf
from mutagen.flac import FLAC
import tomlkit
import splined as core

from webgui.database import Database
from webgui.runtime import Runtime
from webgui.services import Services
from webgui.jobs import JobQueue
from webgui.server import create_app
from webgui.catalog import embedded_track_artwork_bytes
from webgui.credentials import Credentials


@unittest.skipUnless(
    os.environ.get("SPLINED_TEST_DATABASE_URL"),
    "Set SPLINED_TEST_DATABASE_URL for real PostgreSQL integration tests",
)
class PostgreSQLBrowserTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.schema = "splined_test_" + uuid4().hex
        self.base_url = os.environ["SPLINED_TEST_DATABASE_URL"]
        with psycopg.connect(self.base_url, autocommit=True) as db:
            db.execute(sql.SQL("CREATE SCHEMA {}").format(sql.Identifier(self.schema)))
        parsed = urlsplit(self.base_url)
        params = dict(parse_qsl(parsed.query))
        params["options"] = "-c search_path=" + self.schema
        self.url = urlunsplit(parsed._replace(query=urlencode(params, quote_via=quote)))
        self.db = Database(self.url)
        self.db.migrate()
        self.cfg = tomllib.loads(
            (Path(__file__).parents[2] / "docker/config.example.toml").read_text(
                encoding="utf-8"
            )
        )
        self.cfg["mode"] = "write"
        self.cfg["library"]["music_library"] = str(self.root / "music")
        self.cfg["scan"].update(
            cache_dir=str(self.root / "cache"),
            temporary_cache_dir=str(self.root / "temporary"),
            shared_artist_art_dir=str(self.root / "artist-art"),
            log_dir=str(self.root / "logs"),
        )
        self.cfg["credentials"]["credential_dir"] = str(self.root / "credentials")
        self.cfg["output"].update(preserve_file=False, embedded_thumbnail_size=300)
        self.config = self.root / "config.toml"
        self.config.write_text(tomlkit.dumps(self.cfg), encoding="utf-8")
        album = self.root / "music" / "Test Artist" / "Test Album"
        album.mkdir(parents=True)
        self.files = []
        for number in (1, 2):
            path = album / (str(number) + ".flac")
            sf.write(path, np.zeros((4410, 2)), 44100)
            audio = FLAC(path)
            for key, value in {
                "title": f"Track {number}",
                "artist": "Test Artist",
                "albumartist": "Test Artist",
                "album": "Test Album",
                "musicbrainz_albumid": "33333333-3333-4333-8333-333333333333",
                "musicbrainz_artistid": "22222222-2222-4222-8222-222222222222",
                "musicbrainz_trackid": f"44444444-4444-4444-8444-{number:012d}",
            }.items():
                audio[key] = value
            audio.save()
            self.files.append(path)
        self.cover = album / "cover.jpg"
        Image.new("RGB", (180, 180), "navy").save(self.cover)
        self.artist_portrait = album.parent / "artist.png"
        Image.new("RGB", (180, 180), "teal").save(self.artist_portrait)
        self.runtime = Runtime(self.config, self.db)
        self.services = Services(self.runtime)
        self.services.refresh_index()
        with closing(self.db.connect()) as db:
            self.row = db.execute("SELECT * FROM albums").fetchone()
        self.queue = None

    def tearDown(self):
        if self.queue:
            self.queue.close()
        self.runtime.close()
        with psycopg.connect(self.base_url, autocommit=True) as db:
            db.execute(
                sql.SQL("DROP SCHEMA {} CASCADE").format(sql.Identifier(self.schema))
            )
        self.temp.cleanup()

    def payload(self):
        return {
            "album_id": self.row["album_key"],
            "image": self.services.catalog.image(self.cover),
            "adjustments": {"Brightness": 40},
        }

    def test_comp_existing_title_match_uses_the_postgres_catalog(self):
        rows = core.compilation_local_artwork_rows(
            self.config,
            self.cfg,
            SimpleNamespace(
                recording_mbid="55555555-5555-4555-8555-555555555555",
                artist_mbid="22222222-2222-4222-8222-222222222222",
                title="Track 1",
            ),
            current_album_path=self.root / "music" / "Various Artists" / "Billboard",
        )
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["album_name"], "Test Album")
        self.assertFalse(rows[0]["recording_match"])
        self.assertEqual(Path(rows[0]["cover_path"]), self.cover)

    def test_actual_cover_artist_and_album_embedding_preserve_audio(self):
        result = self.services.write_cover(self.payload())
        with Image.open(
            BytesIO(self.services.catalog.image_bytes(result["image"]))
        ) as preview:
            with Image.open(self.cover) as saved:
                self.assertEqual(preview.tobytes(), saved.tobytes())
        banner_component = {
            "kind": "banner",
            "image": result["image"],
            "adjustments": {"Brightness": -35, "Vivid Color": 20},
            "fit": {"zoom": 1.4, "x": 20, "y": -10},
        }
        expected_banner = self.services.preview(banner_component)
        self.services.write_artist(
            {
                "artist_id": self.row["artist_key"],
                "components": [
                    {"kind": "artist", "image": result["image"]},
                    banner_component,
                ],
            }
        )
        self.assertTrue(list(self.root.rglob("artist.png")))
        banner = list(self.root.rglob("banner.png"))[0]
        with Image.open(BytesIO(self.services.catalog.image_bytes(expected_banner["image"]))) as preview:
            with Image.open(banner) as saved:
                self.assertEqual(preview.size, saved.size)
                self.assertEqual(preview.tobytes(), saved.tobytes())
        staged = self.services.stage_album(self.payload())
        self.services.embed_album(staged)
        for path in self.files:
            audio = FLAC(path)
            self.assertEqual(audio["artist"], ["Test Artist"])
            samples, rate = sf.read(path)
            self.assertEqual((len(samples), rate), (4410, 44100))
            with Image.open(BytesIO(embedded_track_artwork_bytes(path))) as image:
                self.assertEqual(image.size, (300, 300))
        self.assertEqual(
            self.services.catalog.album(self.row["album_key"])["embedded"], 2
        )
        self.assertEqual(self.services.index_changes()["count"], 0)

        # WebGUI-owned writes advance the saved inventory baseline, while a
        # later external change to the same Album still requires review.
        stat = self.files[0].stat()
        os.utime(
            self.files[0],
            ns=(stat.st_atime_ns, stat.st_mtime_ns + 1_000_000_000),
        )
        changes = self.services.index_changes()
        self.assertEqual(changes["count"], 1)
        self.assertEqual(changes["changed"], 1)

    def test_postgres_track_credit_projects_featured_album_for_selected_artist(self):
        folder = self.root / "music" / "Album Owner" / "Guest Release"
        folder.mkdir(parents=True)
        path = folder / "guest.flac"
        sf.write(path, np.zeros((4410, 2)), 44100)
        audio = FLAC(path)
        for key, value in {
            "title": "Guest Track",
            "artist": "Album Owner feat. Test Artist",
            "albumartist": "Album Owner",
            "album": "Guest Release",
            "musicbrainz_albumid": "99999999-9999-4999-8999-999999999999",
            "musicbrainz_artistid": "88888888-8888-4888-8888-888888888888",
        }.items():
            audio[key] = value
        audio.save()
        Image.new("RGB", (180, 180), "purple").save(folder / "cover.jpg")
        self.services.refresh_index()

        artist = self.services.catalog.artist(self.row["artist_key"])
        featured = [album for album in artist["albums"] if album["name"] == "Guest Release"]
        self.assertEqual(len(featured), 1)
        self.assertTrue(featured[0]["featured"])

    def test_artist_comp_report_is_scoped_while_full_fallback_keeps_every_track(self):
        comp = self.root / "music" / "Various Artists" / "Billboard Fixture"
        other_album = self.root / "music" / "Other Artist" / "Other Release"
        comp.mkdir(parents=True)
        other_album.mkdir(parents=True)

        def write_track(path, *, title, artist, albumartist, album, album_mbid="", artist_mbid=""):
            sf.write(path, np.zeros((4410, 2)), 44100)
            audio = FLAC(path)
            values = {
                "title": title,
                "artist": artist,
                "albumartist": albumartist,
                "album": album,
            }
            if album_mbid:
                values["musicbrainz_albumid"] = album_mbid
            if artist_mbid:
                values["musicbrainz_artistid"] = artist_mbid
            if albumartist == "Various Artists":
                values["musicbrainz_albumartistid"] = "89ad4ac3-39f7-470e-963a-56509c546377"
            for key, value in values.items():
                audio[key] = value
            audio.save()

        target_path = comp / "01 Test Artist - Target.flac"
        other_path = comp / "02 Other Artist - Other.flac"
        featured_path = comp / "03 Various Artists feat Test Artist.flac"
        write_track(
            target_path,
            title="Target",
            artist="test artist",
            albumartist="Various Artists",
            album="Billboard Fixture",
            artist_mbid="22222222-2222-4222-8222-222222222222",
        )
        write_track(
            other_path,
            title="Other",
            artist="Other Artist",
            albumartist="Various Artists",
            album="Billboard Fixture",
            artist_mbid="77777777-7777-4777-8777-777777777777",
        )
        write_track(
            featured_path,
            title="Featured Target",
            artist="Various Artists feat. Test Artist",
            albumartist="Various Artists",
            album="Billboard Fixture",
            artist_mbid="22222222-2222-4222-8222-222222222222",
        )
        write_track(
            other_album / "01 Other.flac",
            title="Other Release Track",
            artist="Other Artist",
            albumartist="Other Artist",
            album="Other Release",
            album_mbid="66666666-6666-4666-8666-666666666666",
            artist_mbid="77777777-7777-4777-8777-777777777777",
        )
        Image.new("RGB", (180, 180), "purple").save(comp / "cover.jpg")
        self.services.refresh_index()

        with closing(self.db.connect()) as db:
            comp_row = db.execute(
                "SELECT album_key FROM albums WHERE path=?", (str(comp),)
            ).fetchone()
            other_artist = db.execute(
                "SELECT artist_key FROM artists WHERE artist_name='Other Artist'"
            ).fetchone()

        first = self.services.comp_report(self.row["artist_key"])
        repeated = self.services.comp_report(self.row["artist_key"])
        other = self.services.comp_report(other_artist["artist_key"])
        full = self.services.catalog.fallback_tracks(comp_row["album_key"])

        self.assertEqual([group["id"] for group in first], [comp_row["album_key"]])
        self.assertEqual(
            {track["file"] for track in first[0]["tracks"]},
            {target_path.name, featured_path.name},
        )
        self.assertEqual(
            [track["key"] for track in repeated[0]["tracks"]],
            [track["key"] for track in first[0]["tracks"]],
        )
        self.assertEqual(
            {track["file"] for track in other[0]["tracks"]}, {other_path.name}
        )
        self.assertEqual({track["file"] for track in full}, {
            target_path.name, other_path.name, featured_path.name,
        })
        target_key = next(
            track["key"] for track in first[0]["tracks"]
            if track["file"] == target_path.name
        )
        other_key = next(track["key"] for track in full if track["file"] == other_path.name)
        scoped_payload = self.services.validate_comp_payload({
            "artist_id": self.row["artist_key"],
            "album_id": comp_row["album_key"],
            "track_keys": [target_key],
        })
        self.assertEqual(scoped_payload["track_keys"], [target_key])
        with self.assertRaisesRegex(ValueError, "not in this Artist's COMP report"):
            self.services.validate_comp_payload({
                "artist_id": self.row["artist_key"],
                "album_id": comp_row["album_key"],
                "track_keys": [other_key],
            })
        full_payload = self.services.validate_comp_payload({
            "album_id": comp_row["album_key"],
            "track_keys": [target_key, other_key],
        })
        self.assertEqual(full_payload["track_keys"], [target_key, other_key])

        candidate = self.runtime.temporary_cache / "selected-comp-candidate.png"
        candidate.parent.mkdir(parents=True, exist_ok=True)
        Image.new("RGB", (900, 600), "orange").save(candidate)
        embedded = self.services.embed_comp_track({
            "artist_id": self.row["artist_key"],
            "album_id": comp_row["album_key"],
            "track_key": target_key,
            "image": self.services.catalog.image(candidate),
            "adjustments": {"Brightness": 8},
            "upscale": True,
        })
        self.assertEqual((embedded["width"], embedded["height"]), (300, 300))
        with Image.open(BytesIO(embedded_track_artwork_bytes(target_path))) as image:
            self.assertEqual(image.size, (300, 300))
        self.assertIsNone(embedded_track_artwork_bytes(other_path))
        self.assertIsNone(embedded_track_artwork_bytes(featured_path))
        with closing(self.db.connect()) as db:
            written = list(db.execute(
                "SELECT track_path FROM compilation_track_artwork"
            ))
        self.assertEqual([row["track_path"] for row in written], [str(target_path)])

    def test_read_mode_and_invalid_destinations_do_not_write(self):
        original = self.cover.read_bytes()
        self.runtime.cfg["mode"] = "read"
        with self.assertRaises(PermissionError):
            self.services.write_cover(self.payload())
        with self.assertRaises(PermissionError):
            self.services.stage_album(self.payload())
        self.assertEqual(self.cover.read_bytes(), original)
        with self.assertRaises(ValueError):
            self.services.catalog.image_source("/etc/passwd")
        outside = self.root / "outside.png"
        Image.new("RGB", (20, 20), "red").save(outside)
        self.assertIsNone(self.services.catalog.image(outside))

    def test_upscale_preview_honors_native_resolution_and_percentage_ceiling(self):
        source = self.runtime.cache / "small-cover.png"
        source.parent.mkdir(parents=True, exist_ok=True)
        Image.new("RGB", (600, 900), "purple").save(source)
        image = self.services.catalog.image(source)
        self.runtime.cfg["output"]["upscale_below_ideal"] = False
        preserved = self.services.preview(
            {"image": image, "kind": "cover", "upscale": True}
        )
        self.assertEqual((preserved["width"], preserved["height"]), (600, 900))
        self.assertFalse(preserved["upscaled"])
        self.assertIn("Actual source resolution preserved", preserved["reason"])

        self.runtime.cfg["output"]["upscale_below_ideal"] = True
        self.runtime.cfg["output"]["upscale_max_percent"] = 200
        enlarged = self.services.preview(
            {"image": image, "kind": "cover", "upscale": True}
        )
        self.assertEqual((enlarged["width"], enlarged["height"]), (1200, 1800))
        self.assertTrue(enlarged["upscaled"])
        self.assertIn("200% ceiling", enlarged["reason"])
        self.assertTrue(list((self.runtime.temporary_cache / "web-images").iterdir()))
        self.assertFalse((self.runtime.cache / "web-images").exists())

    def test_staged_queue_survives_worker_restart_and_verifies_artifact(self):
        self.queue = JobQueue(self.services)
        result = self.queue.enqueue(
            "embed-album", self.services.stage_album(self.payload()), staged=True
        )
        self.queue.close()
        self.queue = JobQueue(self.services)
        self.assertEqual(self.queue.list()[0]["state"], "staged")
        self.queue.action("start")
        for _ in range(100):
            row = next(item for item in self.queue.list() if item["id"] == result["id"])
            if row["state"] in {"done", "failed"}:
                break
            time.sleep(0.05)
        self.assertEqual(row["state"], "done", row["error"])
        staged = self.services.stage_album(self.payload())
        (self.runtime.cache / "staged-artwork" / staged["prepared_file"]).write_bytes(
            b"corrupt"
        )
        before = self.files[0].read_bytes()
        with self.assertRaisesRegex(ValueError, "verification"):
            self.services.embed_album(staged)
        self.assertEqual(self.files[0].read_bytes(), before)

    def test_comp_queue_deduplicates_only_the_same_track_and_artist_scope(self):
        queue = JobQueue(self.services)
        queue.close()
        full = queue.enqueue("comp", {
            "album_id": "comp-1", "track_keys": ["artist-track", "other-track"],
        })
        scoped = queue.enqueue("comp", {
            "artist_id": "artist-1", "album_id": "comp-1", "track_keys": ["artist-track"],
        })
        repeated = queue.enqueue("comp", {
            "artist_id": "artist-1", "album_id": "comp-1", "track_keys": ["artist-track"],
        })

        self.assertNotEqual(full["id"], scoped["id"])
        self.assertEqual(repeated["id"], scoped["id"])
        self.assertTrue(repeated["already_queued"])

    def test_queue_start_leaves_paused_work_paused_and_retry_clears_stale_review(self):
        self.queue = JobQueue(self.services)
        self.queue.close()
        queue = self.queue
        self.queue = None
        with closing(self.db.connect()) as db, db:
            db.execute(
                "INSERT INTO web_jobs(id,kind,state,payload,progress,decision,error) VALUES"
                "('staged-work','embed-album','staged','{}','{}',NULL,''),"
                "('paused-work','embed-album','paused','{}',"
                "'{\"prompt\":{\"nonce\":\"stale\"}}',"
                "'{\"nonce\":\"stale\",\"value\":\"1\"}','old failure')"
            )
        queue.action("start")
        with closing(self.db.connect()) as db:
            staged = db.execute("SELECT state FROM web_jobs WHERE id='staged-work'").fetchone()
            paused = db.execute("SELECT state FROM web_jobs WHERE id='paused-work'").fetchone()
        self.assertEqual(staged["state"], "queued")
        self.assertEqual(paused["state"], "paused")

        queue.action("retry", "paused-work")
        with closing(self.db.connect()) as db:
            retried = db.execute(
                "SELECT state,progress,decision,error FROM web_jobs WHERE id='paused-work'"
            ).fetchone()
        self.assertEqual(retried["state"], "queued")
        self.assertNotIn("prompt", retried["progress"])
        self.assertIsNone(retried["decision"])
        self.assertEqual(retried["error"], "")

    def test_browser_auth_csrf_real_settings_and_bypass(self):
        self.runtime.close()  # API owns its runtime for this test.
        app = create_app(self.config, self.url, token="test-access", start_worker=False)
        with TestClient(app) as client:
            status = client.get("/api/status").json()
            self.assertFalse(status["authenticated"])
            self.assertTrue(status["admin_setup_required"])
            self.assertEqual(client.get("/api/artists").status_code, 401)
            self.assertEqual(
                client.post(
                    "/api/admin/setup",
                    json={
                        "username": "administrator",
                        "password": "StrongPassword123",
                        "confirmation": "StrongPassword123",
                        "token": "test-access",
                    },
                ).status_code,
                403,
            )
            client.headers["X-SPLINED-CSRF"] = status["csrf"]
            self.assertEqual(
                client.post(
                    "/api/admin/setup",
                    json={
                        "username": "administrator",
                        "password": "StrongPassword123",
                        "confirmation": "StrongPassword123",
                        "token": "wrong-token",
                    },
                ).status_code,
                403,
            )
            self.assertEqual(
                client.post(
                    "/api/admin/setup",
                    json={
                        "username": "administrator",
                        "password": "StrongPassword123",
                        "confirmation": "StrongPassword123",
                        "token": "test-access",
                    },
                ).status_code,
                200,
            )
            self.assertFalse(client.get("/api/status").json()["admin_setup_required"])
            artists = client.get("/api/artists").json()
            self.assertEqual(artists[0]["name"], "Test Artist")
            self.assertIsNone(artists[0]["portrait"])
            thumbnails = client.get("/api/artists/thumbnails").json()
            portrait = next(
                item["portrait"]
                for item in thumbnails
                if item["id"] == artists[0]["id"]
            )
            self.assertEqual(client.get(portrait).status_code, 200)
            settings = client.get("/api/settings").json()
            settings["config"]["mode"] = "read"
            settings["config"]["webgui_ui"] = {
                "enhancement": {
                    "banner_styles": {
                        "compiled": {
                            "name": "COMPILED",
                            "background": "#3478FF",
                            "frame": "#D9AD43",
                            "letters": "#D9AD43",
                        }
                    }
                }
            }
            self.assertEqual(
                client.post("/api/settings", json=settings).status_code, 200
            )
            stored = tomllib.loads(self.config.read_text())
            self.assertEqual(stored["mode"], "read")
            self.assertEqual(
                stored["webgui_ui"]["enhancement"]["banner_styles"]["compiled"][
                    "background"
                ],
                "#3478FF",
            )
            self.assertEqual(
                client.post("/api/artwork/cover", json=self.payload()).status_code, 403
            )
            self.assertEqual(
                client.post(
                    "/api/bypass", json={"album_id": self.row["album_key"]}
                ).status_code,
                200,
            )
            self.assertEqual(
                client.get("/api/album", params={"id": self.row["album_key"]}).json()[
                    "status"
                ],
                "bypassed",
            )
            diagnostics = client.get("/api/diagnostics")
            self.assertEqual(diagnostics.status_code, 200)
            self.assertTrue(
                any(event["action"] == "bypass" for event in diagnostics.json())
            )
            storage = client.get("/api/storage")
            self.assertEqual(storage.status_code, 200)
            self.assertTrue(storage.json()["database"].startswith("PostgreSQL · "))
            self.assertGreater(storage.json()["database_bytes"], 0)
            self.assertEqual(storage.json()["configuration"], str(self.config.resolve()))
            self.assertEqual(
                client.post(
                    "/api/bypass", headers={"Origin": "https://example.org"}, json={}
                ).status_code,
                403,
            )
            recovery = {
                "username": "administrator",
                "password": "ReplacementPassword456",
                "confirmation": "ReplacementPassword456",
                "token": "wrong-token",
            }
            self.assertEqual(
                client.post("/api/admin/recover", json=recovery).status_code, 403
            )
            recovery["token"] = "test-access"
            self.assertEqual(
                client.post("/api/admin/recover", json=recovery).status_code, 200
            )
            self.assertEqual(client.get("/api/artists").status_code, 401)
            status = client.get("/api/status").json()
            client.headers["X-SPLINED-CSRF"] = status["csrf"]
            self.assertEqual(
                client.post(
                    "/api/login",
                    json={
                        "username": "administrator",
                        "password": "StrongPassword123",
                    },
                ).status_code,
                403,
            )
            self.assertEqual(
                client.post(
                    "/api/login",
                    json={
                        "username": "administrator",
                        "password": "ReplacementPassword456",
                    },
                ).status_code,
                200,
            )
            diagnostics = client.get("/api/diagnostics").json()
            self.assertTrue(
                any(event["action"] == "reset-admin-password" for event in diagnostics)
            )
            self.assertEqual(client.post("/api/logout").status_code, 200)
            self.assertEqual(client.get("/api/artists").status_code, 401)
        self.runtime = Runtime(self.config, self.db)

    def test_credentials_mask_secrets_and_retain_blank_values(self):
        credentials = Credentials(self.runtime)
        credentials.save(
            "navidrome",
            {
                "server_url": "http://127.0.0.1:4533",
                "auth_method": "api_key",
                "api_key": "fixture-secret",
            },
        )
        credentials.save("navidrome", {"api_key": ""})
        self.assertNotIn("fixture-secret", str(credentials.describe("navidrome")))
        self.assertEqual(credentials.read("navidrome")["api_key"], "fixture-secret")
        with self.assertRaises(ValueError):
            credentials.save("navidrome", {"../bad": "secret"})

    def test_interrupted_job_pauses_and_decision_nonce_rejects_stale_input(self):
        with closing(self.db.connect()) as db, db:
            db.execute(
                "INSERT INTO web_jobs(id,kind,state,payload) VALUES('interrupted','comp','waiting','{}')"
            )
        self.queue = JobQueue(self.services)
        self.assertEqual(self.queue.list()[0]["state"], "paused")
        with self.assertRaises(ValueError):
            self.queue.action("decision", "interrupted", "1", "stale")
        self.queue.action("remove", "interrupted")
        self.assertEqual(self.queue.list(), [])

    def test_album_queue_bulk_actions_never_change_hidden_comp_work(self):
        self.queue = JobQueue(self.services)
        self.queue.close()
        album = self.queue.enqueue("embed-album", {}, staged=True)
        comp = self.queue.enqueue("comp", {"track_keys": ["track"]}, staged=True)

        self.queue.action("start", kind="embed-album")
        states = {item["id"]: item["state"] for item in self.queue.list()}
        self.assertEqual(states[album["id"]], "queued")
        self.assertEqual(states[comp["id"]], "staged")

        self.queue.action("clear", kind="embed-album")
        remaining = {item["id"]: item for item in self.queue.list()}
        self.assertNotIn(album["id"], remaining)
        self.assertEqual(remaining[comp["id"]]["state"], "staged")

    @patch("splined_scan._manual_remote_candidates", return_value=([], []))
    def test_comp_decision_writes_one_track_and_records_postgres_completion(
        self, _remote
    ):
        import shutil

        self.services.hydrate_tracks(self.row["album_key"])
        folder = self.root / "music" / "Compilations" / "Various"
        folder.mkdir(parents=True)
        target = folder / "comp.flac"
        shutil.copyfile(self.files[0], target)
        audio = FLAC(target)
        del audio["musicbrainz_albumid"]
        audio["album"] = "Various"
        audio.save()
        original_cover = self.cover.read_bytes()
        self.services.refresh_index()
        with closing(self.db.connect()) as db:
            row = db.execute(
                "SELECT * FROM albums WHERE path=?", (str(folder),)
            ).fetchone()
            self.assertEqual(
                db.execute(
                    "SELECT count(*) FROM tracks WHERE album_key=?",
                    (row["album_key"],),
                ).fetchone()[0],
                1,
            )
        with patch.object(self.services, "hydrate_tracks", wraps=self.services.hydrate_tracks) as hydrate:
            report = self.services.comp_report(row["artist_key"])
            self.assertFalse(hydrate.called)
        self.assertEqual([group["id"] for group in report], [row["album_key"]])
        tracks = self.services.hydrate_tracks(row["album_key"])
        self.queue = JobQueue(self.services)
        job = self.queue.enqueue(
            "comp",
            {"album_id": row["album_key"], "track_keys": [tracks[0]["track_key"]]},
        )
        for _ in range(120):
            item = next(
                value for value in self.queue.list() if value["id"] == job["id"]
            )
            if item["state"] in {"waiting", "failed", "done"}:
                break
            time.sleep(0.05)
        self.assertEqual(item["state"], "waiting", item.get("error"))
        self.assertTrue(item["progress"]["current"]["candidates"]["items"])
        prompt = item["progress"]["prompt"]
        self.assertEqual(prompt["context"]["album_id"], row["album_key"])
        self.queue.action("decision", job["id"], "1", prompt["nonce"])
        for _ in range(120):
            item = next(
                value for value in self.queue.list() if value["id"] == job["id"]
            )
            if item["state"] in {"done", "failed"}:
                break
            time.sleep(0.05)
        self.assertEqual(item["state"], "done", item.get("error"))
        self.assertEqual(item["result"]["installed"], 1, item["result"])
        with Image.open(BytesIO(embedded_track_artwork_bytes(target))) as image:
            self.assertEqual(image.size, (300, 300))
        self.assertEqual(
            self.services.catalog.comp_track_status(
                row["album_key"], tracks[0]["track_key"]
            )["status"],
            "green",
        )
        self.assertEqual(self.cover.read_bytes(), original_cover)
        self.assertNotIn("musicbrainz_albumid", FLAC(target))

    def test_musicbrainz_authorization_is_pkce_and_never_returns_saved_tokens(self):
        credentials = Credentials(self.runtime)
        credentials.save(
            "musicbrainz",
            {"client_id": "fixture-client", "client_secret": "fixture-client-secret"},
        )
        session = {}
        flow = credentials.authorize_musicbrainz(session)
        from urllib.parse import parse_qs

        query = parse_qs(urlsplit(flow["url"]).query)
        self.assertEqual(query["code_challenge_method"], ["S256"])
        self.assertNotIn("fixture-client-secret", flow["url"])
        with patch("requests.post") as request:
            request.return_value.ok = True
            request.return_value.json.return_value = {
                "access_token": "fixture-access",
                "refresh_token": "fixture-refresh",
                "expires_in": 3600,
            }
            result = credentials.complete_musicbrainz(
                session, {"state": flow["state"], "code": "fixture-code"}
            )
        self.assertTrue(result["ok"])
        self.assertNotIn("fixture-access", str(credentials.describe("musicbrainz")))
        with self.assertRaises(ValueError):
            credentials.complete_musicbrainz(
                session, {"state": flow["state"], "code": "reused"}
            )

    def test_large_library_handles_stay_valid_and_reject_tampering(self):
        catalog = self.services.catalog
        first = catalog.image(self.cover)
        for _ in range(500):
            catalog.image(self.cover)
        self.assertEqual(
            catalog.image_source(first).read_bytes(), self.cover.read_bytes()
        )
        with self.assertRaises(ValueError):
            catalog.image_source(first[:-1] + ("a" if first[-1] != "a" else "b"))

    def test_candidate_pixels_survive_many_enhancement_previews(self):
        catalog = self.services.catalog
        body = BytesIO()
        Image.new("RGB", (80, 80), "red").save(body, format="PNG")
        original = catalog.image(body.getvalue())
        for value in range(110):
            image = BytesIO()
            Image.new("RGB", (80, 80), (value, 0, 0)).save(image, format="PNG")
            catalog.image(image.getvalue())
        self.assertEqual(catalog.image_source(original).read_bytes(), body.getvalue())

    def test_second_worker_cannot_take_over_a_live_catalog(self):
        self.queue = JobQueue(self.services)
        with self.assertRaisesRegex(ValueError, "already owns"):
            JobQueue(self.services)

    def test_parameter_values_and_large_file_metadata_round_trip(self):
        with closing(self.db.connect()) as db, db:
            value = "artist'; DROP TABLE albums; -- % ?"
            db.execute(
                "UPDATE artists SET artist_name=? WHERE artist_key=?",
                (value, self.row["artist_key"]),
            )
            db.execute(
                "UPDATE albums SET representative_mtime_ns=? WHERE album_key=?",
                (1777777777777777777, self.row["album_key"]),
            )
            self.assertEqual(
                db.execute("SELECT artist_name FROM artists").fetchone()[0], value
            )
            self.assertEqual(
                db.execute("SELECT representative_mtime_ns FROM albums").fetchone()[0],
                1777777777777777777,
            )

    def test_incremental_index_refresh_preserves_unchanged_album_rows(self):
        with closing(self.db.connect()) as db:
            original_updated_at = db.execute(
                "SELECT updated_at FROM albums WHERE album_key=?",
                (self.row["album_key"],),
            ).fetchone()[0]
        added = self.root / "music" / "Other Artist" / "New Album"
        added.mkdir(parents=True)
        added_track = added / "1.flac"
        shutil.copy2(self.files[0], added_track)
        audio = FLAC(added_track)
        audio["artist"] = "Other Artist"
        audio["albumartist"] = "Other Artist"
        audio["album"] = "New Album"
        audio["musicbrainz_albumid"] = "55555555-5555-4555-8555-555555555555"
        audio["musicbrainz_artistid"] = "66666666-6666-4666-8666-666666666666"
        audio.save()

        changes = self.services.index_changes()
        self.assertEqual(changes["added"], 1)
        self.assertEqual(changes["changed"], 0)
        self.assertEqual(changes["removed"], 0)
        result = self.services.refresh_index(mode="incremental")
        self.assertEqual(result["changed_albums"], 1)
        with closing(self.db.connect()) as db:
            self.assertEqual(
                db.execute("SELECT count(*) FROM albums").fetchone()[0], 2
            )
            self.assertEqual(
                db.execute(
                    "SELECT updated_at FROM albums WHERE album_key=?",
                    (self.row["album_key"],),
                ).fetchone()[0],
                original_updated_at,
            )

        shutil.rmtree(added)
        changes = self.services.index_changes()
        self.assertEqual(changes["removed"], 1)
        result = self.services.refresh_index(mode="incremental")
        self.assertEqual(result["removed_albums"], 1)
        with closing(self.db.connect()) as db:
            self.assertEqual(
                db.execute("SELECT count(*) FROM albums").fetchone()[0], 1
            )


if __name__ == "__main__":
    unittest.main()
