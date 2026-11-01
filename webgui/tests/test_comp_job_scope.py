"""COMP job payloads retain explicit full-album or Artist scope."""

from __future__ import annotations

import unittest
from io import BytesIO
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import RLock
from types import SimpleNamespace
from unittest.mock import patch

import numpy as np
import soundfile as sf
from mutagen.flac import FLAC, Picture
from PIL import Image

from webgui.catalog import embedded_track_artwork_bytes
from webgui.services import Services


class _Catalog:
    def artist_comp_report(self, artist_id):
        if artist_id != "artist-1":
            raise KeyError("Artist not found")
        return [{"id": "comp-1", "tracks": [{"key": "artist-track"}]}]

    def artist(self, artist_id):
        if artist_id != "artist-1":
            raise KeyError("Artist not found")
        return {"id": artist_id, "name": "Selected Artist"}


class CompJobScopeTests(unittest.TestCase):
    def setUp(self):
        self.services = Services.__new__(Services)
        self.services.runtime = SimpleNamespace(processing=RLock())
        self.services.catalog = _Catalog()
        self.services.album_row = lambda album_id: {
            "album_key": album_id,
            "album_name": "Large Compilation",
            "musicbrainz_albumid": "",
        }
        self.services.hydrate_tracks = lambda _album_id: [
            {"track_key": "artist-track", "path": "artist.flac"},
            {"track_key": "other-track", "path": "other.flac"},
        ]

    def test_artist_scope_accepts_only_tracks_in_that_artist_report(self):
        normalized = self.services.validate_comp_payload({
            "artist_id": "artist-1",
            "album_id": "comp-1",
            "track_keys": ["artist-track"],
        })

        self.assertEqual(normalized["track_keys"], ["artist-track"])
        self.assertEqual(normalized["artist_id"], "artist-1")
        self.assertEqual(normalized["artist_name"], "Selected Artist")
        with self.assertRaisesRegex(ValueError, "not in this Artist's COMP report"):
            self.services.validate_comp_payload({
                "artist_id": "artist-1",
                "album_id": "comp-1",
                "track_keys": ["other-track"],
            })

    def test_full_album_scope_remains_available_with_explicit_track_keys(self):
        normalized = self.services.validate_comp_payload({
            "album_id": "comp-1",
            "track_keys": ["artist-track", "other-track"],
            "artist_name": "untrusted label",
        })

        self.assertEqual(normalized["track_keys"], ["artist-track", "other-track"])
        self.assertNotIn("artist_id", normalized)
        self.assertNotIn("artist_name", normalized)

    def test_missing_empty_and_duplicate_track_selections_never_expand(self):
        for track_keys in (None, [], ["artist-track", "artist-track"]):
            with self.subTest(track_keys=track_keys):
                payload = {"album_id": "comp-1"}
                if track_keys is not None:
                    payload["track_keys"] = track_keys
                with self.assertRaisesRegex(ValueError, "explicit indexed COMP tracks"):
                    self.services.validate_comp_payload(payload)

    def test_worker_passes_only_validated_tracks_and_current_identity_to_review(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "artist.flac"
            other = root / "other.flac"
            normalized = {
                "artist_id": "artist-1",
                "artist_name": "Selected Artist",
                "album_id": "comp-1",
                "track_keys": ["artist-track"],
                "name": "Large Compilation",
            }
            self.services.runtime = SimpleNamespace(
                processing=RLock(),
                require_write=lambda: None,
                temporary_cache=root / "temporary",
                config_file=root / "config.toml",
                cfg={"mode": "write"},
            )
            self.services.album_row = lambda _album_id: {
                "album_key": "comp-1",
                "album_name": "Large Compilation",
                "artist_name": "Various Artists",
                "path": str(root),
            }
            self.services.hydrate_tracks = lambda _album_id: [
                {"track_key": "artist-track", "path": str(target)},
                {"track_key": "other-track", "path": str(other)},
            ]
            self.services.refresh_album = lambda _album_id: None
            self.services.refresh_embedding_stats = lambda _row: None
            adapter = SimpleNamespace(context={})
            captured = {}

            def manual_scan(album, tracks, **kwargs):
                captured["files"] = list(album.audio_files)
                captured["targets"] = kwargs["target_track_paths"]
                captured["persist_track_authority"] = kwargs[
                    "persist_track_authority"
                ]
                kwargs["track_context"](tracks[0])
                return True

            track = SimpleNamespace(
                path=target, title="Target Song", artist="Selected Artist",
            )
            with (
                patch.object(self.services, "validate_comp_payload", return_value=normalized),
                patch("webgui.services.core.read_track", return_value=track),
                patch("webgui.services.core.Http", return_value=object()),
                patch("webgui.services.core.resolve_sources", return_value=[]),
                patch("webgui.services.core.formats", return_value=["jpeg"]),
                patch("webgui.services.scan.run_manual_compilation_album", side_effect=manual_scan),
            ):
                self.services.run_comp(normalized, adapter)

            self.assertEqual(captured["files"], [target])
            self.assertEqual(captured["targets"], {str(target)})
            self.assertEqual(adapter.context["scope_artist_id"], "artist-1")
            self.assertEqual(adapter.context["scope_artist"], "Selected Artist")
            self.assertEqual(adapter.context["track_key"], "artist-track")
            self.assertEqual(adapter.context["track_title"], "Target Song")
            self.assertTrue(callable(captured.get("persist_track_authority")))

    def test_validated_authority_write_is_atomic_and_preserves_embedded_art(self):
        with TemporaryDirectory() as directory:
            music = Path(directory) / "music"
            music.mkdir()
            track_path = music / "Because the Night.flac"
            sf.write(track_path, np.zeros((4410, 2)), 44100)
            original_art = BytesIO()
            Image.new("RGB", (32, 32), (8, 16, 24)).save(original_art, "JPEG")
            audio = FLAC(track_path)
            audio["title"] = "Because the Night"
            audio["artist"] = "10,000 Maniacs"
            picture = Picture()
            picture.type = 3
            picture.mime = "image/jpeg"
            picture.width = picture.height = 32
            picture.depth = 24
            picture.data = original_art.getvalue()
            audio.add_picture(picture)
            audio.save()
            service = Services.__new__(Services)
            service.runtime = SimpleNamespace(music=music)
            track = SimpleNamespace(path=track_path)
            recording = "e1efc38f-1f86-4c2c-bd90-df1e0796f789"
            artist = "291dcfb8-b31c-496a-905b-9955509d75b6"

            service._persist_comp_track_authority(track, recording, artist)

            verified = FLAC(track_path)
            self.assertEqual(verified["musicbrainz_trackid"], [recording])
            self.assertEqual(verified["musicbrainz_artistid"], [artist])
            self.assertEqual(verified.pictures[0].data, original_art.getvalue())

    def test_artist_and_full_embed_prepare_exact_square_and_write_only_selected_file(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            album = root / "music" / "Various Artists" / "Billboard 1994"
            album.mkdir(parents=True)
            target = album / "10,000 Maniacs - Because the Night.flac"
            other = album / "Other Performer.flac"
            for path in (target, other):
                sf.write(path, np.zeros((4410, 2)), 44100)
                audio = FLAC(path)
                audio["title"] = path.stem
                audio["artist"] = "10,000 Maniacs" if path == target else "Other Performer"
                audio["albumartist"] = "Various Artists"
                audio["album"] = "Billboard Hot 100 Singles of 1994"
                audio.save()
            source = root / "candidate.png"
            Image.new("RGB", (1800, 900), (90, 30, 120)).save(source)

            class Images:
                rendered = b""

                def image_source(self, value):
                    if value != "/candidate":
                        raise KeyError(value)
                    return source

                def image(self, body):
                    self.rendered = body
                    return "/rendered"

                def image_bytes(self, value):
                    if value != "/rendered":
                        raise KeyError(value)
                    return self.rendered

            service = Services.__new__(Services)
            service.runtime = SimpleNamespace(
                processing=RLock(), require_write=lambda: None,
                music=root / "music", cache=root / "cache",
                cfg={
                    "output": {
                        "embedded_thumbnail_size": 1200,
                        "upscale_below_ideal": True,
                        "upscale_max_percent": 200,
                    },
                    "range": {"ideal": 1800},
                },
            )
            service.catalog = Images()
            service.validate_comp_payload = lambda payload: dict(payload)
            service.album_row = lambda _album_id: {
                "album_key": "billboard-1994", "path": str(album),
            }
            records = [
                {
                    "track_key": "because-the-night", "path": str(target),
                    "musicbrainz_recordingid": "recording", "musicbrainz_artistid": "artist",
                },
                {"track_key": "other", "path": str(other)},
            ]
            service.hydrate_tracks = lambda _album_id, **_kwargs: records
            recorded = []
            service._record_comp_track_artwork = lambda *args: recorded.append(args)
            service.refresh_embedding_stats = lambda _row: 1
            service.audit = lambda *_args, **_kwargs: None

            result = service.embed_comp_track({
                "artist_id": "10k-maniacs", "album_id": "billboard-1994",
                "track_key": "because-the-night", "image": "/candidate",
                "adjustments": {"Brightness": 12}, "upscale": True,
            })

            with Image.open(BytesIO(embedded_track_artwork_bytes(target))) as embedded:
                self.assertEqual(embedded.size, (1200, 1200))
                self.assertLess(max(embedded.getpixel((600, 50))), 12)
                self.assertGreater(max(embedded.getpixel((600, 600))), 60)
            self.assertIsNone(embedded_track_artwork_bytes(other))
            self.assertEqual((result["width"], result["height"]), (1200, 1200))
            self.assertTrue(result["upscaled"])
            self.assertEqual(recorded[0][1]["track_key"], "because-the-night")

            target_art = embedded_track_artwork_bytes(target)
            full_result = service.embed_comp_track({
                "album_id": "billboard-1994", "track_key": "other",
                "image": "/candidate", "adjustments": {}, "upscale": False,
            })
            self.assertEqual((full_result["width"], full_result["height"]), (1200, 1200))
            self.assertEqual(embedded_track_artwork_bytes(target), target_art)
            with Image.open(BytesIO(embedded_track_artwork_bytes(other))) as embedded:
                self.assertEqual(embedded.size, (1200, 1200))
            self.assertEqual(recorded[1][1]["track_key"], "other")

    def test_embed_track_keeps_write_permission_guard(self):
        service = Services.__new__(Services)
        service.runtime = SimpleNamespace(
            processing=RLock(),
            require_write=lambda: (_ for _ in ()).throw(PermissionError("read mode")),
        )
        with self.assertRaisesRegex(PermissionError, "read mode"):
            service.embed_comp_track({})


if __name__ == "__main__":
    unittest.main()
