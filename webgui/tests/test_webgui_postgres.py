"""Integration tests use a dedicated PostgreSQL schema and generated silent audio."""

from __future__ import annotations
from contextlib import closing
from dataclasses import replace
from io import BytesIO
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
import tempfile
import threading
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
from psycopg.types.json import Jsonb
import tomlkit
import uvicorn
import splined as core
import splined_compilation_authority_policy as compilation

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

    def make_comp_track(self, folder_name, *, recording_id=True):
        folder = self.root / "music" / "Various Artists" / folder_name
        folder.mkdir(parents=True)
        target = folder / "01 Test Artist - Track 1.flac"
        shutil.copyfile(self.files[0], target)
        audio = FLAC(target)
        del audio["musicbrainz_albumid"]
        if not recording_id:
            del audio["musicbrainz_trackid"]
        audio["albumartist"] = "Various Artists"
        audio["album"] = folder_name
        audio.save()
        self.services.refresh_index(mode="full")
        with closing(self.db.connect()) as db:
            row = db.execute(
                "SELECT * FROM albums WHERE path=?", (str(folder),)
            ).fetchone()
        track = self.services.hydrate_tracks(row["album_key"])[0]
        return row, track, target

    @staticmethod
    def title_suggestion_resolution():
        item = compilation.DiscoveryCandidate(
            recording_mbid="44444444-4444-4444-8444-000000000001",
            recording_title="Track 1",
            recording_artist="Test Artist",
            artist_mbids_key="22222222-2222-4222-8222-222222222222",
            release_mbid="33333333-3333-4333-8333-333333333333",
            release_group_mbid="aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            release_class="album",
            class_rank=0,
            release_title="Test Album",
            release_artist="Test Artist",
            release_date="2000",
            country="US",
            score=88,
            release_group_title="Test Album",
            match_basis="same-artist-title",
        )
        return compilation.DiscoveryResolution(
            candidates=(item,),
            identity_status="unresolved",
            identity_message="Recording ID missing; same-Artist title suggestion available",
            recording_status="missing",
            artist_status="valid",
            friendly_artist="Test Artist",
            friendly_title="Track 1",
            focused_artist_mbid="22222222-2222-4222-8222-222222222222",
            used_fuzzy_search=True,
        )

    def wait_for_job(self, job_id, predicate, timeout=10.0):
        deadline = time.monotonic() + timeout
        item = None
        while time.monotonic() < deadline:
            item = next(value for value in self.queue.list() if value["id"] == job_id)
            if predicate(item):
                return item
            time.sleep(0.05)
        self.fail(f"job {job_id} did not reach the expected state: {item}")

    def drive_comp_candidate(self, row, track):
        result = self.queue.enqueue(
            "comp",
            {"album_id": row["album_key"], "track_keys": [track["track_key"]]},
        )
        item = self.wait_for_job(
            result["id"],
            lambda value: value["state"] == "waiting"
            and value["progress"]["prompt"]["context"].get("kind")
            == "musicbrainz-results",
        )
        prompt = item["progress"]["prompt"]
        self.queue.action("decision", result["id"], "1", prompt["nonce"])
        item = self.wait_for_job(
            result["id"],
            lambda value: value["state"] == "waiting"
            and value["progress"]["prompt"]["context"].get("kind")
            != "musicbrainz-results"
            and bool(value["progress"].get("current", {}).get("candidates", {}).get("items")),
        )
        prompt = item["progress"]["prompt"]
        self.queue.action("decision", result["id"], "1", prompt["nonce"])
        return self.wait_for_job(
            result["id"], lambda value: value["state"] in {"done", "failed"}
        )

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

        def write_track(
            path,
            *,
            title,
            artist,
            albumartist,
            album,
            album_mbid="",
            artist_mbid="",
            recording_mbid="",
        ):
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
            if recording_mbid:
                values["musicbrainz_trackid"] = recording_mbid
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
            recording_mbid="11111111-1111-4111-8111-111111111111",
        )
        write_track(
            other_path,
            title="Other",
            artist="Other Artist",
            albumartist="Various Artists",
            album="Billboard Fixture",
            artist_mbid="77777777-7777-4777-8777-777777777777",
            recording_mbid="33333333-3333-4333-8333-333333333333",
        )
        write_track(
            featured_path,
            title="Featured Target",
            artist="Various Artists feat. Test Artist",
            albumartist="Various Artists",
            album="Billboard Fixture",
            artist_mbid="22222222-2222-4222-8222-222222222222",
            recording_mbid="44444444-4444-4444-8444-444444444444",
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

        target_art = embedded_track_artwork_bytes(target_path)
        full_embedded = self.services.embed_comp_track({
            "album_id": comp_row["album_key"],
            "track_key": other_key,
            "image": self.services.catalog.image(candidate),
            "adjustments": {"Exposure": -4},
            "upscale": False,
        })
        self.assertEqual((full_embedded["width"], full_embedded["height"]), (300, 300))
        self.assertEqual(embedded_track_artwork_bytes(target_path), target_art)
        with Image.open(BytesIO(embedded_track_artwork_bytes(other_path))) as image:
            self.assertEqual(image.size, (300, 300))
        self.assertIsNone(embedded_track_artwork_bytes(featured_path))
        with closing(self.db.connect()) as db:
            written = {
                row["track_path"]
                for row in db.execute(
                    "SELECT track_path FROM compilation_track_artwork"
                )
            }
        self.assertEqual(written, {str(target_path), str(other_path)})

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
        staged = self.services.stage_album(self.payload())
        self.assertIsNone(staged["cover"])
        self.assertEqual(staged["prepared_kind"], "thumbnail-v1")
        prepared = self.runtime.cache / "staged-artwork" / staged["prepared_file"]
        full_cover = self.runtime.cache / "staged-artwork" / staged["cover_file"]
        with Image.open(prepared) as image:
            self.assertEqual(image.size, (300, 300))
        with Image.open(full_cover) as image:
            self.assertEqual(
                image.size,
                (staged["cover_width"], staged["cover_height"]),
            )

        first_projection = self.services.staged_album_projection(staged)
        self.assertTrue(first_projection["staged_valid"])
        first_handle = first_projection["cover"]
        self.assertEqual(
            self.services.catalog.image_bytes(first_handle), full_cover.read_bytes()
        )
        self.queue = JobQueue(self.services)
        result = self.queue.enqueue("embed-album", staged, staged=True)

        # Both index paths leave the durable Album work and its exact prepared
        # artwork intact.
        self.services.refresh_index(mode="incremental")
        self.services.refresh_index(mode="full")
        self.assertEqual(
            next(item for item in self.queue.list() if item["id"] == result["id"])[
                "state"
            ],
            "staged",
        )
        self.queue.close()

        # A recreated service has a new signing key. The stored payload never
        # contains the old handle; queue projection signs the persistent cache
        # snapshot for this process.
        restarted = Services(self.runtime)
        with self.assertRaisesRegex(ValueError, "Invalid artwork handle"):
            restarted.catalog.image_bytes(first_handle)
        self.queue = JobQueue(restarted)
        restored = next(
            item for item in self.queue.list() if item["id"] == result["id"]
        )
        self.assertEqual(restored["state"], "staged")
        self.assertTrue(restored["payload"]["staged_valid"])
        self.assertNotEqual(restored["payload"]["cover"], first_handle)
        self.assertEqual(
            restarted.catalog.image_bytes(restored["payload"]["cover"]),
            full_cover.read_bytes(),
        )

        self.queue.action("start", kind="embed-album")
        for _ in range(100):
            row = next(item for item in self.queue.list() if item["id"] == result["id"])
            if row["state"] in {"done", "failed"}:
                break
            time.sleep(0.05)
        self.assertEqual(row["state"], "done", row["error"])
        self.assertTrue(row["result"]["verified"])
        corrupt_payload = self.payload()
        corrupt_payload["image"] = restarted.catalog.image(self.cover)
        corrupt = restarted.stage_album(corrupt_payload)
        (self.runtime.cache / "staged-artwork" / corrupt["prepared_file"]).write_bytes(
            b"corrupt"
        )
        before = self.files[0].read_bytes()
        with self.assertRaisesRegex(ValueError, "verification"):
            restarted.embed_album(corrupt)
        self.assertEqual(self.files[0].read_bytes(), before)

    def test_album_staging_rejects_comp_before_it_can_enter_the_queue(self):
        # A refresh publishes the current classification to PostgreSQL. Model
        # that accepted catalog transition directly so this test remains about
        # the staging boundary rather than tag-reader cache behavior.
        with closing(self.db.connect()) as db, db:
            db.execute(
                "UPDATE albums SET musicbrainz_albumid='' WHERE album_key=?",
                (self.row["album_key"],),
            )
            fallback = db.execute(
                "SELECT * FROM albums WHERE path=?", (str(self.cover.parent),)
            ).fetchone()
        self.assertEqual(str(fallback["musicbrainz_albumid"] or ""), "")
        original_cover = self.cover.read_bytes()
        with self.assertRaisesRegex(ValueError, "COMP Fallback Albums"):
            self.services.stage_album(
                {
                    "album_id": fallback["album_key"],
                    "image": self.services.catalog.image(self.cover),
                    "adjustments": {"Brightness": 40},
                }
            )
        self.assertEqual(self.cover.read_bytes(), original_cover)
        with closing(self.db.connect()) as db:
            self.assertEqual(
                db.execute(
                    "SELECT count(*) FROM web_jobs WHERE kind='embed-album'"
                ).fetchone()[0],
                0,
            )

    def test_album_embedding_rejects_malformed_ids_at_stage_retry_and_execution(self):
        staged = self.services.stage_album(self.payload())
        self.queue = JobQueue(self.services)
        self.queue.close()
        queued = self.queue.enqueue("embed-album", staged, staged=True)
        before = [path.read_bytes() for path in self.files]
        with closing(self.db.connect()) as db, db:
            db.execute(
                "UPDATE albums SET musicbrainz_albumid='release-not-a-uuid' "
                "WHERE album_key=?",
                (self.row["album_key"],),
            )

        with self.assertRaisesRegex(ValueError, "valid MusicBrainz Release ID"):
            self.services.stage_album(self.payload())
        with self.assertRaisesRegex(ValueError, "valid MusicBrainz Release ID"):
            self.services.embed_album(
                {"album_id": self.row["album_key"], "image": self.services.catalog.image(self.cover)}
            )
        projection = self.services.staged_album_projection(staged)
        self.assertFalse(projection["staged_valid"])
        self.assertIn("valid MusicBrainz Release ID", projection["staged_error"])
        self.queue.action("start", kind="embed-album")
        failed = next(item for item in self.queue.list() if item["id"] == queued["id"])
        self.assertEqual(failed["state"], "failed")
        self.assertIn("valid MusicBrainz Release ID", failed["error"])
        with self.assertRaisesRegex(ValueError, "valid MusicBrainz Release ID"):
            self.queue.action("retry", queued["id"])
        self.assertEqual([path.read_bytes() for path in self.files], before)

    def test_identified_compilation_album_remains_album_embedding_eligible(self):
        with closing(self.db.connect()) as db, db:
            db.execute(
                "UPDATE albums SET compilation=1 WHERE album_key=?",
                (self.row["album_key"],),
            )
        staged = self.services.stage_album(self.payload())
        row, files, _ = self.services.validate_staged_album(staged)
        self.assertEqual(row["album_key"], self.row["album_key"])
        self.assertEqual(files, self.files)
        result = self.services.embed_album(staged)
        self.assertTrue(result["verified"])
        self.assertEqual(result["tracks"], len(self.files))

    def test_reclassified_album_job_fails_visibly_without_embedding_comp_tracks(self):
        self.queue = JobQueue(self.services)
        self.queue.close()
        staged = self.services.stage_album(self.payload())
        result = self.queue.enqueue("embed-album", staged, staged=True)
        before = [embedded_track_artwork_bytes(path) for path in self.files]

        # The job survives an index refresh. Then model the refreshed catalog
        # reclassifying this path as COMP and prove the obsolete Album job is
        # visible but cannot be started.
        self.services.refresh_index(mode="full")
        with closing(self.db.connect()) as db, db:
            db.execute(
                "UPDATE albums SET musicbrainz_albumid='' WHERE album_key=?",
                (self.row["album_key"],),
            )
            fallback = db.execute(
                "SELECT * FROM albums WHERE path=?", (str(self.cover.parent),)
            ).fetchone()
        self.assertEqual(str(fallback["musicbrainz_albumid"] or ""), "")

        restored = next(
            item for item in self.queue.list() if item["id"] == result["id"]
        )
        self.assertFalse(restored["payload"]["staged_valid"])
        self.assertRegex(
            restored["payload"]["staged_error"],
            "(now COMP Fallback|no longer indexed)",
        )
        self.queue.action("start", kind="embed-album")
        failed = next(
            item for item in self.queue.list() if item["id"] == result["id"]
        )
        self.assertEqual(failed["state"], "failed")
        self.assertRegex(failed["error"], "(now COMP Fallback|no longer indexed)")
        self.assertEqual(
            [embedded_track_artwork_bytes(path) for path in self.files], before
        )

    def test_active_album_job_is_not_hidden_by_mixed_job_history(self):
        self.queue = JobQueue(self.services)
        self.queue.close()
        result = self.queue.enqueue(
            "embed-album", self.services.stage_album(self.payload()), staged=True
        )
        with closing(self.db.connect()) as db, db:
            with db.raw.cursor() as cursor:
                cursor.executemany(
                    "INSERT INTO web_jobs(id,kind,state,payload) VALUES(%s,'comp','done',%s)",
                    [
                        (
                            f"historical-{number:03d}",
                            Jsonb({"track_keys": [str(number)]}),
                        )
                        for number in range(250)
                    ],
                )
        listed = {item["id"]: item for item in self.queue.list()}
        self.assertIn(result["id"], listed)
        self.assertEqual(listed[result["id"]]["state"], "staged")

    def test_album_queue_gates_order_and_remove_all_safety(self):
        self.queue = JobQueue(self.services)
        self.queue.close()
        second_folder = self.root / "music" / "Test Artist" / "Second Album"
        second_folder.mkdir(parents=True)
        second_track = second_folder / "1.flac"
        sf.write(second_track, np.zeros((4410, 2)), 44100)
        second_audio = FLAC(second_track)
        for key, value in {
            "title": "Second Track",
            "artist": "Test Artist",
            "albumartist": "Test Artist",
            "album": "Second Album",
            "musicbrainz_albumid": "55555555-5555-4555-8555-555555555555",
            "musicbrainz_artistid": "22222222-2222-4222-8222-222222222222",
        }.items():
            second_audio[key] = value
        second_audio.save()
        second_cover = second_folder / "cover.jpg"
        Image.new("RGB", (180, 180), "purple").save(second_cover)
        self.services.refresh_index(mode="full")
        with closing(self.db.connect()) as db:
            second_row = db.execute(
                "SELECT * FROM albums WHERE path=?", (str(second_folder),)
            ).fetchone()

        staged = self.services.stage_album(self.payload())
        second_staged = self.services.stage_album(
            {
                "album_id": second_row["album_key"],
                "image": self.services.catalog.image(second_cover),
            }
        )
        snapshots = [
            self.runtime.cache / "staged-artwork" / item[field]
            for item in (staged, second_staged)
            for field in ("prepared_file", "cover_file")
        ]
        result = self.queue.enqueue("embed-album", staged, staged=True)
        second_result = self.queue.enqueue(
            "embed-album", second_staged, staged=True
        )
        self.queue.action("start", kind="embed-album")

        paused_after = self.queue.action("pause-after", kind="embed-album")
        self.assertTrue(paused_after["pause_after_album"])
        self.assertIsNone(self.queue.claim())
        resumed = self.queue.action("resume-queue", kind="embed-album")
        self.assertFalse(resumed["pause_after_album"])

        paused_all = self.queue.action("pause-all", kind="embed-album")
        self.assertTrue(paused_all["pause_all_albums"])
        self.assertIsNone(self.queue.claim())
        resumed_all = self.queue.action("resume-all", kind="embed-album")
        self.assertFalse(resumed_all["pause_all_albums"])
        claimed = self.queue.claim()
        self.assertEqual(claimed["id"], result["id"])
        with self.assertRaisesRegex(ValueError, "Pause ALL Albums"):
            self.queue.action("clear", kind="embed-album")

        self.queue.action("pause", result["id"])
        self.queue.action("pause", second_result["id"])
        self.queue.action("clear", kind="embed-album")
        self.assertTrue(all(not path.exists() for path in snapshots))
        self.assertFalse(
            any(item["kind"] == "embed-album" for item in self.queue.list())
        )

    def test_authenticated_server_recreation_restores_staged_preview(self):
        self.runtime.close()
        first_app = create_app(
            self.config, self.url, token="restart-test-token", start_worker=True
        )
        with TestClient(first_app) as client:
            status = client.get("/api/status").json()
            client.headers["X-SPLINED-CSRF"] = status["csrf"]
            setup = client.post(
                "/api/admin/setup",
                json={
                    "username": "restart-admin",
                    "password": "RestartPassword123",
                    "confirmation": "RestartPassword123",
                    "token": "restart-test-token",
                },
            )
            self.assertEqual(setup.status_code, 200, setup.text)
            album = client.get(
                "/api/album", params={"id": self.row["album_key"]}
            ).json()
            staged = client.post(
                "/api/jobs",
                json={
                    "kind": "embed-album",
                    "payload": {
                        "album_id": self.row["album_key"],
                        "image": album["cover"],
                    },
                },
            )
            self.assertEqual(staged.status_code, 200, staged.text)
            first_jobs = client.get("/api/jobs").json()
            first = next(item for item in first_jobs if item["id"] == staged.json()["id"])
            first_cover = first["payload"]["cover"]
            self.assertTrue(first["payload"]["staged_valid"])
            self.assertEqual(client.get(first_cover).status_code, 200)
            # A page refresh reads the same staged identity and current handle.
            refreshed = client.get("/api/jobs").json()
            self.assertEqual(
                next(item for item in refreshed if item["id"] == first["id"])[
                    "payload"
                ]["cover"],
                first_cover,
            )

        second_app = create_app(
            self.config, self.url, token="restart-test-token", start_worker=True
        )
        with TestClient(second_app) as client:
            status = client.get("/api/status").json()
            self.assertFalse(status["admin_setup_required"])
            client.headers["X-SPLINED-CSRF"] = status["csrf"]
            login = client.post(
                "/api/login",
                json={
                    "username": "restart-admin",
                    "password": "RestartPassword123",
                },
            )
            self.assertEqual(login.status_code, 200, login.text)
            restored_jobs = client.get("/api/jobs").json()
            restored = next(item for item in restored_jobs if item["id"] == first["id"])
            restored_cover = restored["payload"]["cover"]
            self.assertTrue(restored["payload"]["staged_valid"])
            self.assertNotEqual(restored_cover, first_cover)
            self.assertEqual(client.get(restored_cover).status_code, 200)
            self.assertEqual(client.get(first_cover).status_code, 400)

        self.runtime = Runtime(self.config, self.db)
        self.services = Services(self.runtime)

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
        staged_payload = self.services.stage_album(self.payload())
        with closing(self.db.connect()) as db, db:
            db.raw.execute(
                "INSERT INTO web_jobs(id,kind,state,payload,progress,decision,error) VALUES"
                "('staged-work','embed-album','staged',%s,'{}',NULL,''),"
                "('paused-work','embed-album','paused',%s,"
                "'{\"prompt\":{\"nonce\":\"stale\"}}',"
                "'{\"nonce\":\"stale\",\"value\":\"1\"}','old failure')",
                (Jsonb(staged_payload), Jsonb(staged_payload)),
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
        staged = self.services.stage_album(self.payload())
        album = self.queue.enqueue("embed-album", staged, staged=True)
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
        prompt = item["progress"]["prompt"]
        self.assertEqual(prompt["context"]["album_id"], row["album_key"])
        self.assertEqual(prompt["context"]["kind"], "musicbrainz-results")
        self.queue.action(
            "decision", job["id"],
            json.dumps({"action": "use-existing", "number": 1}),
            prompt["nonce"],
        )
        for _ in range(120):
            item = next(
                value for value in self.queue.list() if value["id"] == job["id"]
            )
            if (
                item["state"] == "waiting"
                and item["progress"]["prompt"]["context"].get("kind")
                != "musicbrainz-results"
            ):
                break
            time.sleep(0.05)
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
        with closing(self.db.connect()) as db:
            recorded = db.execute(
                "SELECT recording_mbid,artist_mbid,artwork_sha256,outcome "
                "FROM compilation_track_artwork WHERE track_path=?",
                (str(target),),
            ).fetchone()
        self.assertIsNotNone(recorded)
        self.assertEqual(recorded["outcome"], "embedded-replaced")
        self.assertEqual(
            recorded["artwork_sha256"],
            hashlib.sha256(embedded_track_artwork_bytes(target)).hexdigest(),
        )
        self.assertEqual(self.cover.read_bytes(), original_cover)
        self.assertNotIn("musicbrainz_albumid", FLAC(target))

    def test_title_suggested_comp_write_without_apply_ids_stays_blue(self):
        row, track, target = self.make_comp_track(
            "Title Suggested Compilation", recording_id=False
        )
        self.queue = JobQueue(self.services)
        resolution = self.title_suggestion_resolution()
        with (
            patch.object(
                core,
                "compilation_credential_options",
                return_value=compilation.MusicBrainzOptions(1, 0.0, 1.0),
            ),
            patch.object(
                core,
                "compilation_resolve_interactive_identity",
                return_value=resolution,
            ),
            patch("splined_scan._manual_remote_candidates", return_value=([], [])),
        ):
            item = self.drive_comp_candidate(row, track)

        self.assertEqual(item["state"], "done", item.get("error"))
        self.assertEqual(item["result"]["installed"], 1)
        self.assertEqual(item["result"]["unresolved"], 1)
        self.assertEqual(item["result"]["resolved"], 0)
        artwork = embedded_track_artwork_bytes(target)
        self.assertIsNotNone(artwork)
        with Image.open(BytesIO(artwork)) as image:
            self.assertEqual(image.size, (300, 300))
        with closing(self.db.connect()) as db:
            self.assertIsNone(
                db.execute(
                    "SELECT 1 FROM compilation_track_artwork WHERE track_path=?",
                    (str(target),),
                ).fetchone()
            )
        self.assertEqual(
            self.services.catalog.comp_track_status(
                row["album_key"], track["track_key"]
            )["status"],
            "blue",
        )
        self.assertNotIn("musicbrainz_trackid", FLAC(target))
        self.assertEqual(
            FLAC(target)["musicbrainz_artistid"],
            ["22222222-2222-4222-8222-222222222222"],
        )

    def test_queued_comp_verified_readback_records_completion(self):
        row, track, target = self.make_comp_track(
            "Verified Readback Compilation", recording_id=True
        )
        self.queue = JobQueue(self.services)
        suggestion = self.title_suggestion_resolution()
        exact = replace(
            suggestion.candidates[0],
            score=100,
            match_basis="exact-recording",
        )
        resolution = replace(
            suggestion,
            candidates=(exact,),
            identity_status="valid",
            identity_message="Recording and Artist IDs verified",
            recording_status="valid",
            used_fuzzy_search=False,
        )
        with (
            patch.object(
                core,
                "compilation_credential_options",
                return_value=compilation.MusicBrainzOptions(1, 0.0, 1.0),
            ),
            patch.object(
                core,
                "compilation_resolve_interactive_identity",
                return_value=resolution,
            ),
            patch("splined_scan._manual_remote_candidates", return_value=([], [])),
        ):
            item = self.drive_comp_candidate(row, track)

        self.assertEqual(item["state"], "done", item.get("error"))
        self.assertEqual(item["result"]["installed"], 1)
        self.assertEqual(item["result"]["unresolved"], 0)
        self.assertEqual(item["result"]["resolved"], 1)
        artwork = embedded_track_artwork_bytes(target)
        with Image.open(BytesIO(artwork)) as image:
            self.assertEqual(image.size, (300, 300))
        with closing(self.db.connect()) as db:
            recorded = db.execute(
                "SELECT recording_mbid,artist_mbid,artwork_sha256 "
                "FROM compilation_track_artwork WHERE track_path=?",
                (str(target),),
            ).fetchone()
        self.assertEqual(
            recorded["recording_mbid"],
            "44444444-4444-4444-8444-000000000001",
        )
        self.assertEqual(
            recorded["artwork_sha256"], hashlib.sha256(artwork).hexdigest()
        )
        self.assertEqual(
            self.services.catalog.comp_track_status(
                row["album_key"], track["track_key"]
            )["status"],
            "green",
        )

    def test_queued_comp_failed_readback_never_records_completion(self):
        row, track, target = self.make_comp_track(
            "Failed Readback Compilation", recording_id=True
        )
        self.queue = JobQueue(self.services)
        resolution = self.title_suggestion_resolution()
        with (
            patch.object(
                core,
                "compilation_credential_options",
                return_value=compilation.MusicBrainzOptions(1, 0.0, 1.0),
            ),
            patch.object(
                core,
                "compilation_resolve_interactive_identity",
                return_value=resolution,
            ),
            patch("splined_scan._manual_remote_candidates", return_value=([], [])),
            patch("webgui.services.embedded_track_artwork_bytes", return_value=None),
        ):
            item = self.drive_comp_candidate(row, track)

        self.assertEqual(item["state"], "failed")
        self.assertIn("verification failed", item["error"])
        self.assertIsNotNone(embedded_track_artwork_bytes(target))
        with closing(self.db.connect()) as db:
            self.assertIsNone(
                db.execute(
                    "SELECT 1 FROM compilation_track_artwork WHERE track_path=?",
                    (str(target),),
                ).fetchone()
            )
        self.assertEqual(
            self.services.catalog.comp_track_status(
                row["album_key"], track["track_key"]
            )["status"],
            "blue",
        )

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

    def test_rendered_browser_comp_workflows_and_persistent_album_queue(self):
        """Drive the real app in Chrome; only external provider replies are simulated."""
        try:
            from playwright.sync_api import expect, sync_playwright
        except ImportError as error:  # pragma: no cover - CI installs test requirements.
            self.skipTest(f"Playwright is unavailable: {error}")

        comp = (
            self.root
            / "music"
            / "Various Artists"
            / "Billboard Hot 100 Singles of 1994"
        )
        comp.mkdir(parents=True)
        target = comp / "01 10,000 Maniacs - Because the Night.flac"
        other = comp / "02 Other Performer - Other Song.flac"

        def comp_track(path, title, artist, artist_id, recording_id):
            sf.write(path, np.zeros((4410, 2)), 44100)
            audio = FLAC(path)
            for key, value in {
                "title": title,
                "artist": artist,
                "albumartist": "Various Artists",
                "album": "Billboard Hot 100 Singles of 1994",
                "musicbrainz_albumartistid": "89ad4ac3-39f7-470e-963a-56509c546377",
                "musicbrainz_artistid": artist_id,
                "musicbrainz_trackid": recording_id,
            }.items():
                audio[key] = value
            audio.save()

        comp_track(
            target,
            "Because the Night",
            "10,000 Maniacs",
            "6c8b9855-ba8b-48f9-ac1d-42167f7f7b18",
            "11111111-1111-4111-8111-111111111111",
        )
        comp_track(
            other,
            "Other Song",
            "Other Performer",
            "77777777-7777-4777-8777-777777777777",
            "88888888-8888-4888-8888-888888888888",
        )
        # The reported library selects 10,000 Maniacs from its normal Artist
        # entry, then projects that Artist's compilation appearances.  Keep a
        # regular reference Album in the fixture so the rendered interaction
        # follows the same route instead of manufacturing a track-credit-only
        # sidebar identity that Windows does not expose.
        artist_album = self.root / "music" / "10,000 Maniacs" / "Reference Album"
        artist_album.mkdir(parents=True)
        target_release_id = "12345678-1234-4234-8234-123456789abc"
        other_release_id = "99999999-9999-4999-8999-999999999999"
        artist_track = artist_album / "01 Because the Night.flac"
        sf.write(artist_track, np.zeros((4410, 2)), 44100)
        audio = FLAC(artist_track)
        for key, value in {
            "title": "Because the Night",
            "artist": "10,000 Maniacs",
            "albumartist": "10,000 Maniacs",
            "album": "Reference Album",
            "musicbrainz_albumid": target_release_id,
            "musicbrainz_artistid": "6c8b9855-ba8b-48f9-ac1d-42167f7f7b18",
            "musicbrainz_trackid": "11111111-1111-4111-8111-111111111111",
        }.items():
            audio[key] = value
        audio.save()
        Image.new("RGB", (900, 600), "orange").save(artist_album / "cover.png")
        other_album = self.root / "music" / "Other Performer" / "Other Reference"
        other_album.mkdir(parents=True)
        other_reference = other_album / "01 Other Song.flac"
        sf.write(other_reference, np.zeros((4410, 2)), 44100)
        audio = FLAC(other_reference)
        for key, value in {
            "title": "Other Song",
            "artist": "Other Performer",
            "albumartist": "Other Performer",
            "album": "Other Reference",
            "musicbrainz_albumid": other_release_id,
            "musicbrainz_artistid": "77777777-7777-4777-8777-777777777777",
            "musicbrainz_trackid": "88888888-8888-4888-8888-888888888888",
        }.items():
            audio[key] = value
        audio.save()
        Image.new("RGB", (900, 600), "cyan").save(other_album / "cover.png")
        Image.new("RGB", (700, 700), "purple").save(comp / "cover.jpg")
        credential = self.root / "credentials" / "musicbrainz.json"
        credential.parent.mkdir(parents=True, exist_ok=True)
        credential.write_text(json.dumps({"options": {
            "retry_max": 1, "min_delay": 0.001, "recording_timeout": 2,
        }}), encoding="utf-8")

        def wait_for_embedded_art(path: Path, timeout: float = 15.0) -> bytes:
            """Wait for verified audio readback, not a transient progress banner."""
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                try:
                    body = embedded_track_artwork_bytes(path)
                except Exception:
                    body = None
                if body:
                    return body
                time.sleep(0.1)
            self.fail(f"embedded artwork was not readable from {path.name}")

        self.services.refresh_index(mode="full")
        with closing(self.db.connect()) as db:
            comp_row = db.execute(
                "SELECT * FROM albums WHERE path=?", (str(comp),)
            ).fetchone()
        self.runtime.close()

        app = None
        http_calls: list[str] = []

        class MusicBrainzResponse:
            def __init__(self, payload, status_code=200):
                self.status_code = status_code
                self._payload = payload

            def json(self):
                return self._payload

        identities = {
            "6c8b9855-ba8b-48f9-ac1d-42167f7f7b18": {
                "recording": "11111111-1111-4111-8111-111111111111",
                "artist": "10,000 Maniacs",
                "title": "Because the Night",
                "release": target_release_id,
                "album": "Reference Album",
                "group": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            },
            "77777777-7777-4777-8777-777777777777": {
                "recording": "88888888-8888-4888-8888-888888888888",
                "artist": "Other Performer",
                "title": "Other Song",
                "release": other_release_id,
                "album": "Other Reference",
                "group": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            },
        }

        def recording_payload(artist_id, identity):
            credit = [{"artist": {"id": artist_id, "name": identity["artist"]}}]
            return {
                "id": identity["recording"],
                "score": 100,
                "title": identity["title"],
                "artist-credit": credit,
                "relations": [],
                "releases": [{
                    "id": identity["release"],
                    "title": identity["album"],
                    "status": "Official",
                    "date": "1994",
                    "country": "US",
                    "artist-credit": credit,
                    "release-group": {
                        "id": identity["group"],
                        "title": identity["album"],
                        "primary-type": "Album",
                        "secondary-types": [],
                    },
                }],
            }

        def fixture_http_get(_http, url, **kwargs):
            http_calls.append(url)
            for artist_id, identity in identities.items():
                if url.endswith("/artist/" + artist_id):
                    return MusicBrainzResponse({
                        "id": artist_id, "name": identity["artist"]
                    })
                if url.endswith("/recording/" + identity["recording"]):
                    return MusicBrainzResponse(
                        recording_payload(artist_id, identity)
                    )
            if url.rstrip("/").endswith("/recording"):
                query = str(kwargs.get("params", {}).get("query", ""))
                identity = next(
                    (value for key, value in identities.items() if key in query),
                    None,
                )
                artist_id = next(
                    (key for key in identities if key in query), ""
                )
                return MusicBrainzResponse({
                    "recordings": [recording_payload(artist_id, identity)]
                    if identity else []
                })
            return MusicBrainzResponse({}, 503)

        provider_fixture_dir = self.runtime.temporary_cache / "provider-candidates"
        provider_fixture_dir.mkdir()
        provider_candidates = []
        for number, (side, source, color) in enumerate(
            [
                (1200, "itunes", "navy"),
                (1400, "amazon", "orange"),
                (1600, "coverartarchive", "teal"),
                (1800, "fanarttv", "purple"),
                (2000, "discogs", "green"),
            ],
            1,
        ):
            path = provider_fixture_dir / f"candidate-{number}.jpg"
            Image.new("RGB", (side, side), color).save(path)
            provider_candidates.append(
                core.Candidate(
                    core.Ref(
                        source,
                        f"fixture-{number}",
                        f"https://fixtures.invalid/{number}",
                        width=side,
                        height=side,
                    ),
                    path,
                    side,
                    side,
                    "jpeg",
                    number,
                )
            )

        def fixture_remote_candidates(*_args, **_kwargs):
            return list(provider_candidates), [
                ("amazon", "Fixture policy diagnostic retained for review"),
                ("amazon", "Fixture policy diagnostic retained for review"),
                ("itunes", "Fixture range diagnostic retained for review"),
                ("musicbrainz", "policy-filtered: Reference is not primary/front under active source policy"),
                ("deezer", "request failed (ConnectionError)"),
            ]

        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]

        def launch_test_server():
            application = create_app(
                self.config,
                self.url,
                token="rendered-browser-token",
                start_worker=True,
            )
            running_server = uvicorn.Server(
                uvicorn.Config(
                    application,
                    host="127.0.0.1",
                    port=port,
                    log_level="warning",
                )
            )
            running_server.install_signal_handlers = lambda: None
            running_thread = threading.Thread(
                target=running_server.run, daemon=True
            )
            running_thread.start()
            for _ in range(100):
                if running_server.started:
                    break
                time.sleep(0.05)
            self.assertTrue(
                running_server.started, "temporary WebGUI server did not start"
            )
            return application, running_server, running_thread

        server = None
        server_thread = None
        with (
            patch.object(
                core,
                "mb_headers",
                return_value=({"User-Agent": "SPLINED-test"}, "fixture"),
            ),
            patch.object(core.Http, "get", new=fixture_http_get),
            patch(
                "splined_scan._manual_remote_candidates",
                side_effect=fixture_remote_candidates,
            ),
        ):
            app, server, server_thread = launch_test_server()

            try:
                with sync_playwright() as playwright:
                    browser = playwright.chromium.launch(
                        channel=os.environ.get(
                            "SPLINED_TEST_BROWSER_CHANNEL", "chrome"
                        ),
                        headless=True,
                        args=["--no-sandbox"],
                    )
                    page = browser.new_page(viewport={"width": 1600, "height": 1000})
                    page.goto(f"http://127.0.0.1:{port}", wait_until="networkidle")
                    page.locator("#setupUsername").fill("browser-admin")
                    page.locator("#setupPassword").fill("RenderedBrowser123")
                    page.locator("#setupConfirmation").fill("RenderedBrowser123")
                    page.locator("#setupToken").fill("rendered-browser-token")
                    page.locator("#adminSetupForm button[type=submit]").click()
                    expect(page.locator("body")).to_have_class("authenticated")

                    # A regular Album stages through the rendered controls and
                    # its card survives a real browser reload.
                    page.locator("#artistSearch").fill("Test Artist")
                    page.locator("#artistList button", has_text="Test Artist").click()
                    page.locator("button.album-card", has_text="Test Album").click()
                    expect(page.locator("#embedBatchButton")).to_be_enabled()
                    page.locator("#embedBatchButton").click()
                    page.locator("#embedTab").click()
                    expect(page.locator(".queue-card", has_text="Test Album")).to_be_visible()

                    # Recreate the authenticated service against the same
                    # PostgreSQL schema and persistent cache, then sign in as
                    # a new server session and verify the staged preview card.
                    server.should_exit = True
                    server_thread.join(timeout=15)
                    self.assertFalse(server_thread.is_alive())
                    app, server, server_thread = launch_test_server()
                    page.reload(wait_until="networkidle")
                    expect(page.locator("#loginForm")).to_be_visible()
                    page.locator("#loginUsername").fill("browser-admin")
                    page.locator("#loginPassword").fill("RenderedBrowser123")
                    page.locator("#loginForm button[type=submit]").click()
                    expect(page.locator("body")).to_have_class("authenticated")
                    # Authentication becomes visible before the asynchronous
                    # library/bootstrap render completes.  Wait for that load
                    # so its default workspace render cannot race this click.
                    expect(page.locator("#statusText")).to_contain_text("Loaded")
                    page.locator("#embedTab").click()
                    expect(page.locator("#embedView")).to_be_visible()
                    expect(page.locator(".queue-card", has_text="Test Album")).to_be_visible()
                    expect(page.locator("#embedQueue")).not_to_contain_text(
                        "Billboard Hot 100 Singles of 1994"
                    )

                    # Artist report: the top LAUNCH COMP stays scoped to the
                    # one 10,000 Maniacs track and presents simulated MB 100%.
                    page.locator("#findTab").click()
                    page.locator("#artistsTab").click()
                    page.locator("#artistSearch").fill("10,000 Maniacs")
                    page.locator(
                        "#artistList button", has_text="10,000 Maniacs"
                    ).click()
                    expect(page.locator("#statusText")).to_contain_text(
                        "Loaded 10,000 Maniacs"
                    )
                    page.locator("#compReportButton").click()
                    expect(page.locator("#fallbackReport")).to_contain_text(
                        "Because the Night"
                    )
                    expect(page.locator("#fallbackReport")).not_to_contain_text(
                        "Other Song"
                    )
                    page.locator("#fallbackReport .fallback-launch").click()
                    expect(page.locator("#musicbrainzReview")).to_be_visible(
                        timeout=15000
                    )
                    expect(page.locator("#sourceHeading")).to_have_text(
                        "Find Releases Containing This Recording"
                    )
                    expect(page.locator("#musicbrainzReview")).to_contain_text(
                        "100% match · Release Family"
                    )
                    expect(page.locator("#musicbrainzReview")).to_contain_text(
                        "CURRENT ALBUM · SPLINED LIBRARY AUTHORITY"
                    )
                    expect(page.locator("#candidateDock")).to_be_hidden()
                    expect(page.locator("#sourceStrip button", has_text="MusicBrainz")).to_contain_text(
                        "0 found"
                    )
                    self.assertTrue(http_calls)
                    self.assertTrue(
                        all(url.startswith(core.MB_BASE) for url in http_calls),
                        "an artwork provider was queried before release selection",
                    )
                    page.locator(".musicbrainz-authority-editor summary").click()
                    expect(
                        page.locator(
                            '.musicbrainz-authority-fields input[placeholder^="Release MBID"]'
                        )
                    ).to_have_count(0)
                    page.locator(
                        '.musicbrainz-authority-fields input[placeholder="Recording MBID"]'
                    ).fill("11111111-1111-4111-8111-111111111111")
                    page.locator(
                        '.musicbrainz-authority-fields input[placeholder="Artist MBID"]'
                    ).fill("6c8b9855-ba8b-48f9-ac1d-42167f7f7b18")
                    page.locator(
                        ".musicbrainz-authority-fields button",
                        has_text="Apply IDs",
                    ).click()
                    expect(page.locator("#musicbrainzReview")).to_contain_text(
                        "Artist ID · valid · Recording ID · valid",
                        timeout=15000,
                    )
                    self.assertTrue(
                        all(url.startswith(core.MB_BASE) for url in http_calls),
                        "Apply IDs queried an artwork provider before release selection",
                    )
                    expect(
                        page.locator(
                            ".musicbrainz-review-actions button",
                            has_text="Back to Candidates",
                        )
                    ).to_be_disabled()
                    expect(page.locator(".musicbrainz-release-row").first).to_be_visible()
                    page.evaluate(
                        """() => {
                          document.querySelector('.musicbrainz-release-row').click();
                          const use = document.querySelector('.musicbrainz-review-actions button.primary');
                          if (!use || use.disabled) throw new Error('release selection did not enable Use Selected Album');
                          use.click();
                        }"""
                    )
                    expect(page.locator(".candidate-image-button").first).to_be_visible(
                        timeout=15000
                    )
                    cards = page.locator("#candidateGrid .candidate-card.queued")
                    self.assertGreaterEqual(cards.count(), 5)
                    first_card = cards.first
                    expect(first_card.locator(".candidate-source-label")).not_to_be_empty()
                    expect(first_card.locator(".candidate-resolution")).to_be_visible()
                    expect(first_card.locator(".candidate-quality")).to_be_visible()
                    first_image_url = first_card.locator("img").evaluate(
                        "image => image.currentSrc"
                    )
                    first_image_response = page.request.get(first_image_url)
                    self.assertTrue(first_image_response.ok)
                    with Image.open(BytesIO(first_image_response.body())) as actual:
                        displayed = first_card.locator(
                            ".candidate-resolution"
                        ).inner_text()
                        self.assertEqual(
                            displayed, f"{actual.width} × {actual.height}"
                        )
                    grid_box = page.locator("#candidateGrid").bounding_box()
                    visible_cards = 0
                    for index in range(cards.count()):
                        box = cards.nth(index).bounding_box()
                        if box and box["x"] + box["width"] <= grid_box["x"] + grid_box["width"]:
                            visible_cards += 1
                            self.assertLessEqual(
                                box["y"] + box["height"],
                                grid_box["y"] + grid_box["height"] + 1,
                            )
                    self.assertGreaterEqual(visible_cards, 5)
                    diagnostics = page.locator("#compDecisionTools .comp-diagnostics")
                    expect(diagnostics).to_contain_text("Diagnostics (4)")
                    diagnostics.locator("summary").click()
                    expect(diagnostics.locator(".comp-diagnostics-list")).to_be_visible()
                    expect(first_card).to_be_visible()
                    diagnostics.locator("summary").click()
                    unavailable = page.locator("#sourceStrip .source-card", has_text="Deezer")
                    expect(unavailable).to_have_class(re.compile(r"\bunavailable\b"))
                    expect(unavailable.locator(".source-state")).to_have_text("⚠ Unavailable")
                    filtered = page.locator("#sourceStrip .source-card", has_text="MusicBrainz")
                    expect(filtered).to_have_class(re.compile(r"\bfiltered\b"))
                    expect(filtered.locator(".source-state")).to_have_text("○ Filtered")

                    screenshot_dir = os.environ.get("SPLINED_TEST_SCREENSHOT_DIR")
                    if screenshot_dir:
                        screenshot_root = Path(screenshot_dir)
                        screenshot_root.mkdir(parents=True, exist_ok=True)
                        page.locator("#candidateDock").screenshot(
                            path=str(screenshot_root / "candidate-area-normal.png")
                        )
                        page.set_viewport_size({"width": 1920, "height": 1080})
                        expect(first_card).to_be_visible()
                        page.locator("#candidateDock").screenshot(
                            path=str(screenshot_root / "candidate-area-maximized.png")
                        )
                        page.set_viewport_size({"width": 1600, "height": 1000})
                    page.locator(
                        "#compDecisionTools button", has_text="MusicBrainz Results"
                    ).click()
                    expect(page.locator("#musicbrainzReview")).to_be_visible(
                        timeout=15000
                    )
                    expect(
                        page.locator(
                            ".musicbrainz-review-actions button",
                            has_text="Back to Candidates",
                        )
                    ).to_be_enabled()
                    page.locator(
                        ".musicbrainz-review-actions button",
                        has_text="Back to Candidates",
                    ).click()
                    expect(page.locator(".candidate-image-button").first).to_be_visible(
                        timeout=15000
                    )
                    page.locator(
                        "#compDecisionTools button", has_text="MusicBrainz Results"
                    ).click()
                    expect(page.locator("#musicbrainzReview")).to_be_visible(
                        timeout=15000
                    )
                    page.locator(".musicbrainz-release-row").click()
                    page.locator(".musicbrainz-review-actions button.primary").click()
                    page.locator(".candidate-image-button").first.click()
                    expect(page.locator("#writeButton")).to_have_text("EMBED Track")
                    page.locator("#writeButton").click()
                    target_art = wait_for_embedded_art(target)
                    expect(page.locator("#statusText")).to_have_text(
                        re.compile(r"COMP (track artwork work complete|report ready)"),
                        timeout=15000,
                    )
                    self.assertIsNone(embedded_track_artwork_bytes(other))

                    # Full Fallback: the lower LAUNCH processes the remaining
                    # performer independently and remains in the full report.
                    page.locator("#fallbackTab").click()
                    page.locator(
                        "#artistList button", has_text="Billboard Hot 100 Singles of 1994"
                    ).click()
                    expect(page.locator("#fallbackReport")).to_contain_text(
                        "Because the Night"
                    )
                    expect(page.locator("#fallbackReport")).to_contain_text(
                        "Other Song"
                    )
                    full_launch_call_start = len(http_calls)
                    page.locator("#searchButton").click()
                    expect(page.locator("#musicbrainzReview")).to_contain_text(
                        "100% match · Release Family"
                    )
                    expect(page.locator("#candidateDock")).to_be_hidden()
                    self.assertTrue(
                        all(
                            url.startswith(core.MB_BASE)
                            for url in http_calls[full_launch_call_start:]
                        ),
                        "an artwork provider was queried before full-Fallback release selection",
                    )
                    page.locator(".musicbrainz-release-row").click()
                    page.locator(".musicbrainz-review-actions button.primary").click()
                    page.locator(".candidate-image-button").first.click()
                    page.locator("#writeButton").click()
                    wait_for_embedded_art(other)
                    expect(page.locator("#statusText")).to_have_text(
                        re.compile(r"COMP (track artwork work complete|report ready)"),
                        timeout=15000,
                    )
                    self.assertEqual(embedded_track_artwork_bytes(target), target_art)
                    self.assertIsNotNone(embedded_track_artwork_bytes(other))

                    # Reopen the Artist report and prove full-album work did
                    # not leak the other performer into the Artist scope.
                    page.locator("#artistsTab").click()
                    page.locator("#artistSearch").fill("10,000 Maniacs")
                    page.locator(
                        "#artistList button", has_text="10,000 Maniacs"
                    ).click()
                    expect(page.locator("#statusText")).to_contain_text(
                        "Loaded 10,000 Maniacs"
                    )
                    page.locator("#compReportButton").click()
                    expect(page.locator("#fallbackReport")).to_contain_text(
                        "Because the Night"
                    )
                    expect(page.locator("#fallbackReport")).not_to_contain_text(
                        "Other Song"
                    )
                    browser.close()
            finally:
                server.should_exit = True
                server_thread.join(timeout=15)
                self.assertFalse(server_thread.is_alive())

        self.runtime = Runtime(self.config, self.db)
        self.services = Services(self.runtime)


if __name__ == "__main__":
    unittest.main()
