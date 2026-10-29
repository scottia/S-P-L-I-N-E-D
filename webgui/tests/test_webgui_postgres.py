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
            temporary_cache_dir=str(self.root / "cache"),
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

    def test_browser_auth_csrf_real_settings_and_bypass(self):
        self.runtime.close()  # API owns its runtime for this test.
        app = create_app(self.config, self.url, token="test-access", start_worker=False)
        with TestClient(app) as client:
            status = client.get("/api/status").json()
            self.assertFalse(status["authenticated"])
            self.assertEqual(client.get("/api/artists").status_code, 401)
            self.assertEqual(
                client.post("/api/login", json={"token": "test-access"}).status_code,
                403,
            )
            client.headers["X-SPLINED-CSRF"] = status["csrf"]
            self.assertEqual(
                client.post("/api/login", json={"token": "test-access"}).status_code,
                200,
            )
            self.assertEqual(
                client.get("/api/artists").json()[0]["name"], "Test Artist"
            )
            settings = client.get("/api/settings").json()
            settings["config"]["mode"] = "read"
            self.assertEqual(
                client.post("/api/settings", json=settings).status_code, 200
            )
            self.assertEqual(tomllib.loads(self.config.read_text())["mode"], "read")
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
            self.assertEqual(
                client.post(
                    "/api/bypass", headers={"Origin": "https://example.org"}, json={}
                ).status_code,
                403,
            )
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
