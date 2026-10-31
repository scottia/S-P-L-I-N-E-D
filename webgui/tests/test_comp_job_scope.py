"""COMP job payloads retain explicit full-album or Artist scope."""

from __future__ import annotations

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import RLock
from types import SimpleNamespace
from unittest.mock import patch

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


if __name__ == "__main__":
    unittest.main()
