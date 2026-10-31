"""Artist thumbnail lookup remains recoverable and API access stays protected."""

from __future__ import annotations

from contextlib import closing
from io import BytesIO
import json
from pathlib import Path
import sqlite3
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest import mock

from fastapi.testclient import TestClient
from PIL import Image
import requests

from webgui.catalog import Catalog
from webgui.server import create_app


def _png() -> bytes:
    output = BytesIO()
    Image.new("RGB", (12, 12), "teal").save(output, format="PNG")
    return output.getvalue()


class ArtistThumbnailRecoveryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.music = self.root / "music"
        self.artist_path = self.music / "Test Artist"
        self.artist_path.mkdir(parents=True)
        self.credentials = self.root / "credentials"
        self.credentials.mkdir()
        self.database = self.root / "catalog.sqlite"
        with closing(sqlite3.connect(self.database)) as database:
            database.execute(
                "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,artist_name TEXT,"
                "musicbrainz_artistid TEXT,primary_path TEXT,status TEXT)"
            )
            database.execute(
                "INSERT INTO artists VALUES (?,?,?,?,?)",
                ("artist-1", "Test Artist", "", str(self.artist_path), "unprocessed"),
            )
            database.commit()
        self.catalog = Catalog(
            self.music,
            self.database,
            self.root / "artist-art",
            self.credentials,
            cache=self.root / "cache",
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def _save_credentials(self) -> None:
        (self.credentials / "navidrome.json").write_text(
            json.dumps(
                {
                    "server_url": "https://navidrome.invalid",
                    "auth_method": "api_key",
                    "api_key": "test-only",
                }
            ),
            encoding="utf-8",
        )

    @staticmethod
    def _directory(name: str = "Test Artist") -> dict:
        return {
            "artists": {
                "index": [
                    {
                        "artist": [
                            {"id": "remote-artist", "name": name, "coverArt": "cover-1"}
                        ]
                    }
                ]
            }
        }

    def test_failed_remote_thumbnail_does_not_hide_later_local_art(self) -> None:
        self._save_credentials()
        remote = {"id": "remote-artist", "name": "Test Artist", "coverArt": "cover-1"}
        with mock.patch.object(
            self.catalog, "_navidrome_artists_by_name", return_value={"testartist": remote}
        ), mock.patch.object(self.catalog, "_cache_navidrome_thumbnail", return_value=None):
            with self.assertRaisesRegex(KeyError, "unavailable"):
                self.catalog.artist_thumbnail("artist-1")

        self.assertNotIn("artist-1", self.catalog.portrait_cache)
        portrait = self.artist_path / "artist.png"
        portrait.write_bytes(_png())
        self.assertEqual(self.catalog.artist_thumbnail("artist-1"), portrait)

    def test_empty_remote_thumbnail_is_bounded_then_can_recover(self) -> None:
        self._save_credentials()
        remote = {"id": "remote-artist", "name": "Test Artist", "coverArt": "cover-1"}
        cached = self.root / "cache" / "navidrome-thumbnails" / "artists" / "cached.png"
        cached.parent.mkdir(parents=True)
        cached.write_bytes(_png())
        with mock.patch.object(
            self.catalog, "_navidrome_artists_by_name", return_value={"testartist": remote}
        ), mock.patch.object(
            self.catalog, "_cache_navidrome_thumbnail", side_effect=[None, cached]
        ) as download:
            with self.assertRaisesRegex(KeyError, "unavailable"):
                self.catalog.artist_thumbnail("artist-1")
            with self.assertRaisesRegex(KeyError, "unavailable"):
                self.catalog.artist_thumbnail("artist-1")
            self.assertEqual(download.call_count, 1, "retry backoff must suppress repeated downloads")
            self.catalog.portrait_retry_at["artist-1"] = 0
            self.assertEqual(self.catalog.artist_thumbnail("artist-1"), cached)
            self.assertEqual(self.catalog.artist_thumbnail("artist-1"), cached)
        self.assertEqual(download.call_count, 2)
        self.assertEqual(self.catalog.portrait_cache["artist-1"], cached)

    def test_missing_credentials_can_recover_without_restart(self) -> None:
        self.assertEqual(self.catalog._navidrome_artists_by_name(), {})
        self._save_credentials()
        with mock.patch.object(
            self.catalog, "_navidrome_call", return_value=self._directory()
        ) as call:
            result = self.catalog._navidrome_artists_by_name()
        self.assertEqual(result["testartist"]["id"], "remote-artist")
        call.assert_called_once()

    def test_transient_failure_is_bounded_then_can_retry(self) -> None:
        self._save_credentials()
        with mock.patch.object(
            self.catalog,
            "_navidrome_call",
            side_effect=[requests.RequestException("offline"), self._directory()],
        ) as call:
            self.assertEqual(self.catalog._navidrome_artists_by_name(), {})
            self.assertEqual(self.catalog._navidrome_artists_by_name(), {})
            self.assertEqual(call.call_count, 1, "retry backoff must suppress an immediate request loop")
            self.catalog.navidrome_directory_retry_at = 0
            result = self.catalog._navidrome_artists_by_name()
        self.assertEqual(result["testartist"]["id"], "remote-artist")
        self.assertEqual(call.call_count, 2)

    def test_empty_directory_result_can_refresh_later(self) -> None:
        self._save_credentials()
        with mock.patch.object(
            self.catalog,
            "_navidrome_call",
            side_effect=[{"artists": {"index": []}}, self._directory()],
        ) as call:
            self.assertEqual(self.catalog._navidrome_artists_by_name(), {})
            self.assertEqual(self.catalog._navidrome_artists_by_name(), {})
            self.assertEqual(call.call_count, 1)
            self.catalog.navidrome_directory_retry_at = 0
            result = self.catalog._navidrome_artists_by_name()
        self.assertEqual(result["testartist"]["id"], "remote-artist")
        self.assertEqual(call.call_count, 2)


class ArtistThumbnailEndpointTests(unittest.TestCase):
    def _app(self, root: Path, *, token: str):
        app = create_app(root / "missing.toml", token=token, start_worker=False)
        catalog = SimpleNamespace(
            artist_thumbnails=mock.Mock(
                return_value=[{"id": "artist-1", "portrait": "/api/artist/thumbnail?id=artist-1"}]
            )
        )
        app.state.context.services = SimpleNamespace(catalog=catalog)
        return app, catalog

    def test_thumbnail_list_endpoint_returns_catalog_projection(self) -> None:
        with TemporaryDirectory() as temporary:
            app, catalog = self._app(Path(temporary), token="")
            with TestClient(app) as client:
                response = client.get("/api/artists/thumbnails")
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json()[0]["id"], "artist-1")
        catalog.artist_thumbnails.assert_called_once_with()

    def test_thumbnail_list_endpoint_requires_authenticated_session(self) -> None:
        with TemporaryDirectory() as temporary:
            app, catalog = self._app(Path(temporary), token="owner-token")
            with TestClient(app) as client:
                response = client.get("/api/artists/thumbnails")
        self.assertEqual(response.status_code, 401)
        catalog.artist_thumbnails.assert_not_called()

    def test_thumbnail_list_endpoint_retains_local_network_restriction(self) -> None:
        with TemporaryDirectory() as temporary:
            app, catalog = self._app(Path(temporary), token="")
            with TestClient(app, base_url="http://pc.invalid") as client:
                response = client.get("/api/artists/thumbnails")
        self.assertEqual(response.status_code, 403)
        catalog.artist_thumbnails.assert_not_called()


if __name__ == "__main__":
    unittest.main()
