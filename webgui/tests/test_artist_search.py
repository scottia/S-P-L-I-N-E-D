from __future__ import annotations

from io import BytesIO
from pathlib import Path
import json
import tempfile
import unittest

from PIL import Image

from interaction.artist_search import (
    _allowed_image_url,
    discover_artist_images,
    discover_artist_profile,
)


def _png(color: str) -> bytes:
    output = BytesIO()
    Image.new("RGB", (40, 40), color).save(output, format="PNG")
    return output.getvalue()


class _Response:
    def __init__(self, payload=None, content=b"", status_code=200):
        self.payload, self.content, self.status_code = payload, content, status_code

    def raise_for_status(self):
        if self.status_code >= 400:
            raise ValueError(f"HTTP {self.status_code}")

    def json(self):
        return self.payload


class _Http:
    def __init__(self):
        self.headers = {}
        self.calls = []

    def get(self, url, **kwargs):
        self.calls.append((url, kwargs))
        if url.startswith("https://webservice.fanart.tv/"):
            return _Response({"artistbackground": [{"url": "https://assets.fanart.tv/banner.png"}],
                              "artistthumb": [{"url": "https://assets.fanart.tv/portrait.png"}]})
        if url == "https://api.deezer.com/search/artist":
            return _Response({"data": [
                {"name": "3 Doors Down", "picture_xl": "https://e-cdns-images.dzcdn.net/deezer.jpg"},
                {"name": "Another Artist", "picture_xl": "https://e-cdns-images.dzcdn.net/wrong.jpg"},
            ]})
        if url.endswith("banner.png"):
            return _Response(content=_png("blue"))
        if url.endswith("portrait.png"):
            return _Response(content=_png("red"))
        if url.endswith("deezer.jpg"):
            return _Response(content=_png("green"))
        raise AssertionError(f"Unexpected request: {url}")


class _ProfileHttp:
    def __init__(self):
        self.headers = {}
        self.calls = []

    def get(self, url, **kwargs):
        self.calls.append((url, kwargs))
        if url.endswith("/artist"):
            return _Response({"artists": [{
                "id": "0330a83d-f61e-4989-adb3-afce3943c3ba",
                "name": ".38 Special",
                "score": 100,
            }]})
        if url == "https://ws.audioscrobbler.com/2.0/":
            return _Response({"artist": {
                "name": ".38 Special",
                "url": "https://www.last.fm/music/.38+Special",
                "bio": {"content": "Southern &amp; classic rock. <a>Read more</a>"},
            }})
        raise AssertionError(f"Unexpected request: {url}")


class _AdditionalProviderHttp:
    def __init__(self):
        self.headers = {}
        self.calls = []

    def get(self, url, **kwargs):
        self.calls.append((url, kwargs))
        if url == "https://ws.audioscrobbler.com/2.0/":
            return _Response({"artist": {"name": "3 Doors Down", "image": [
                {"size": "extralarge", "#text": "https://lastfm.freetls.fastly.net/i/u/artist.png"}
            ]}})
        if url == "https://api.discogs.com/database/search":
            return _Response({"results": [
                {"title": "3 Doors Down", "cover_image": "https://i.discogs.com/artist.jpg"},
                {"title": "Wrong Artist", "cover_image": "https://i.discogs.com/wrong.jpg"},
            ]})
        if url.endswith("artist.png") or url.endswith("artist.jpg"):
            return _Response(content=_png("purple"))
        raise AssertionError(f"Unexpected request: {url}")


class ArtistSearchTests(unittest.TestCase):
    def test_image_downloads_are_limited_to_provider_hosts(self):
        self.assertTrue(_allowed_image_url("https://assets.fanart.tv/art.png"))
        self.assertTrue(_allowed_image_url("https://e-cdns-images.dzcdn.net/art.jpg"))
        self.assertTrue(_allowed_image_url("https://lastfm.freetls.fastly.net/i/u/art.jpg"))
        self.assertTrue(_allowed_image_url("https://i.discogs.com/art.jpg"))
        self.assertFalse(_allowed_image_url("https://fanart.tv.evil.example/art.png"))
        self.assertFalse(_allowed_image_url("http://assets.fanart.tv/art.png"))

    def test_search_combines_verified_portraits_and_banner_without_wrong_artist(self):
        with tempfile.TemporaryDirectory() as temporary:
            credentials = Path(temporary)
            (credentials / "fanarttv.json").write_text(json.dumps({"api_key": "fixture-key"}), encoding="utf-8")
            http = _Http()
            found, warnings = discover_artist_images(
                "3 Doors Down", "03cd9e6d-0a23-4c52-876f-0e909c84c653", credentials, http=http,
            )
            self.assertEqual([label for label, _ in found],
                             ["Fanart banner", "Fanart portrait", "Deezer portrait"])
            self.assertEqual(warnings, [])
            self.assertNotIn("wrong.jpg", [url for url, _ in http.calls])
            self.assertEqual(http.calls[0][1]["headers"]["api-key"], "fixture-key")
            self.assertTrue(all(Image.open(BytesIO(data)).format == "PNG" for _, data in found))

    def test_search_without_credentials_still_uses_deezer(self):
        with tempfile.TemporaryDirectory() as temporary:
            found, warnings = discover_artist_images("3 Doors Down", "", Path(temporary), http=_Http())
            self.assertEqual([label for label, _ in found], ["Deezer portrait"])
            self.assertEqual(warnings, [])

    def test_bad_fanart_credentials_do_not_block_deezer(self):
        with tempfile.TemporaryDirectory() as temporary:
            credentials = Path(temporary)
            (credentials / "fanarttv.json").write_text("[]", encoding="utf-8")
            found, warnings = discover_artist_images(
                "3 Doors Down", "03cd9e6d-0a23-4c52-876f-0e909c84c653", credentials, http=_Http(),
            )
            self.assertEqual([label for label, _ in found], ["Deezer portrait"])
            self.assertEqual(len(warnings), 1)
            self.assertTrue(warnings[0].startswith("Fanart.tv:"))

    def test_configured_lastfm_and_discogs_artist_images_are_included(self):
        with tempfile.TemporaryDirectory() as temporary:
            credentials = Path(temporary)
            (credentials / "lastfm.json").write_text(
                json.dumps({"api_key": "lastfm-secret"}), encoding="utf-8"
            )
            (credentials / "discogs.json").write_text(
                json.dumps({"token": "discogs-secret"}), encoding="utf-8"
            )
            http = _AdditionalProviderHttp()
            found, warnings = discover_artist_images(
                "3 Doors Down", "", credentials, http=http,
                enabled=("lastfm", "discogs"),
            )
            self.assertEqual(
                [label for label, _ in found],
                ["Last.fm portrait", "Discogs portrait"],
            )
            self.assertEqual(warnings, [])
            discogs = next(call for call in http.calls if call[0].endswith("database/search"))
            self.assertEqual(
                discogs[1]["headers"]["Authorization"],
                "Discogs token=discogs-secret",
            )
            self.assertNotIn("wrong.jpg", [url for url, _ in http.calls])

    def test_profile_matches_musicbrainz_then_uses_lastfm_when_navidrome_is_missing(self):
        with tempfile.TemporaryDirectory() as temporary:
            credentials = Path(temporary)
            (credentials / "lastfm.json").write_text(
                json.dumps({"api_key": "fixture-key"}), encoding="utf-8"
            )
            http = _ProfileHttp()
            profile = discover_artist_profile(
                "38 Special", "", credentials, http=http
            )
            self.assertEqual(profile["lookup_name"], ".38 Special")
            self.assertEqual(
                profile["musicbrainz_id"],
                "0330a83d-f61e-4989-adb3-afce3943c3ba",
            )
            self.assertEqual(
                profile["biography"], "Southern & classic rock. Read more"
            )
            self.assertEqual(
                http.calls[-1][1]["params"]["mbid"],
                "0330a83d-f61e-4989-adb3-afce3943c3ba",
            )
