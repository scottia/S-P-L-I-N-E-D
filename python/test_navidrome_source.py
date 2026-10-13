from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import splined


class _Response:
    def __init__(self, payload=None, *, status_code: int = 200, body: bytes = b""):
        self.status_code = status_code
        self._payload = payload if payload is not None else {}
        self.body = body
        self.headers: dict[str, str] = {}
        self.closed = False
        self.url = "https://server.invalid/rest/result"

    def json(self):
        return self._payload

    def close(self) -> None:
        self.closed = True

    def __enter__(self):
        return self

    def __exit__(self, _type, _value, _traceback) -> None:
        self.close()

    def iter_content(self, chunk_size: int):
        del chunk_size
        yield self.body


class _Http:
    def __init__(self, *responses: _Response):
        self.responses = list(responses)
        self.calls: list[tuple[str, dict]] = []

    def get(self, url: str, **kwargs):
        self.calls.append((url, kwargs))
        return self.responses.pop(0)


def _envelope(albums: list[dict]) -> dict:
    return {
        "subsonic-response": {
            "status": "ok",
            "version": "1.16.1",
            "openSubsonic": True,
            "searchResult3": {"album": albums},
        }
    }


def _release(*, mbid: str = "release-mbid") -> splined.Release:
    return splined.Release(
        mbid=mbid,
        title="Renaissance",
        artist_credit="Beyoncé",
        release_group_id=None,
        release_group_title=None,
    )


class NavidromeSourceTests(unittest.TestCase):
    def setUp(self) -> None:
        splined._NAVIDROME_SESSIONS.clear()
        splined._NAVIDROME_NEXT_SESSION = 1
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.config_path = self.root / "config.toml"
        self.credentials = self.root / "credentials"
        self.credentials.mkdir()
        self.cfg = {"credentials": {"credential_dir": str(self.credentials)}}
        splined.save_json_atomic(
            self.credentials / "navidrome.json",
            {
                "server_url": "https://navidrome.invalid/",
                "auth_method": "api_key",
                "username": "listener",
                "password": "",
                "api_key": "api-secret",
                "bearer_token": "",
            },
        )

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_release_mbid_is_deterministic_authority(self) -> None:
        response = _Response(
            _envelope(
                [
                    {
                        "id": "wrong",
                        "artist": "Beyoncé",
                        "name": "Renaissance",
                        "coverArt": "wrong-cover",
                    },
                    {
                        "id": "right",
                        "musicBrainzId": "release-mbid",
                        "coverArt": "right-cover",
                    },
                ]
            )
        )
        http = _Http(response)

        refs = splined.discover_navidrome(
            http, self.config_path, self.cfg, _release()
        )

        self.assertEqual([ref.id for ref in refs], ["right"])
        self.assertTrue(refs[0].url.startswith("splined+navidrome://1/"))
        self.assertNotIn("api-secret", refs[0].url)
        self.assertNotIn("bearer-secret", refs[0].url)
        url, options = http.calls[0]
        self.assertEqual(url, "https://navidrome.invalid/rest/search3.view")
        self.assertEqual(options["params"]["apiKey"], "api-secret")
        self.assertNotIn("u", options["params"])
        self.assertNotIn("X-ND-Authorization", options["headers"])
        self.assertTrue(response.closed)

    def test_fallback_requires_one_exact_artist_album_identity(self) -> None:
        album = {
            "id": "album",
            "artist": "BEYONCÉ",
            "album": "Renaissance",
            "coverArt": "cover",
        }
        self.assertEqual(
            [item["id"] for item in splined.match_navidrome_albums(_release(mbid=""), [album])],
            ["album"],
        )
        self.assertEqual(
            splined.match_navidrome_albums(_release(mbid=""), [album, dict(album)]),
            [],
        )

    def test_password_auth_uses_challenge_not_password_or_bearer_in_url(self) -> None:
        splined.save_json_atomic(
            self.credentials / "navidrome.json",
            {
                "server_url": "https://navidrome.invalid",
                "auth_method": "password",
                "username": "listener",
                "password": "password-secret",
                "api_key": "",
                "bearer_token": "",
            },
        )
        http = _Http(_Response(_envelope([])))
        self.assertEqual(
            splined.discover_navidrome(
                http, self.config_path, self.cfg, _release(mbid="")
            ),
            [],
        )
        url, options = http.calls[0]
        self.assertNotIn("password-secret", url)
        self.assertNotIn("bearer-secret", url)
        self.assertEqual(options["params"]["u"], "listener")
        self.assertIn("s", options["params"])
        self.assertIn("t", options["params"])
        self.assertNotIn("p", options["params"])
        self.assertNotIn("X-ND-Authorization", options["headers"])

    def test_legacy_native_bearer_is_not_sent_to_opensubsonic(self) -> None:
        splined.save_json_atomic(
            self.credentials / "navidrome.json",
            {
                "server_url": "https://navidrome.invalid",
                "auth_method": "api_key",
                "api_key": "api-secret",
                "bearer_token": "bearer-secret",
            },
        )
        http = _Http(_Response(_envelope([])))
        self.assertEqual(
            splined.discover_navidrome(
                http, self.config_path, self.cfg, _release(mbid="")
            ),
            [],
        )
        _url, options = http.calls[0]
        self.assertEqual(options["params"]["apiKey"], "api-secret")
        self.assertNotIn("bearer-secret", repr(options["params"]))
        self.assertNotIn("X-ND-Authorization", options["headers"])

    def test_internal_cover_download_restores_only_in_memory_identity(self) -> None:
        search = _Response(
            _envelope(
                [
                    {
                        "id": "album",
                        "musicBrainzId": "release-mbid",
                        "coverArt": "cover/album",
                    }
                ]
            )
        )
        artwork = _Response(body=b"image")
        http = _Http(search, artwork)
        ref = splined.discover_navidrome(
            http, self.config_path, self.cfg, _release()
        )[0]

        response = splined.download_navidrome_cover(http, ref.url)
        response.close()

        url, options = http.calls[1]
        self.assertEqual(url, "https://navidrome.invalid/rest/getCoverArt.view")
        self.assertEqual(options["params"]["id"], "cover/album")
        self.assertNotIn("api-secret", ref.url)
        self.assertNotIn("bearer-secret", ref.url)

    def test_missing_compatible_authentication_is_rejected(self) -> None:
        splined.save_json_atomic(
            self.credentials / "navidrome.json",
            {
                "server_url": "https://navidrome.invalid",
                "auth_method": "password",
                "username": "listener",
                "password": "",
                "api_key": "",
                "bearer_token": "",
            },
        )
        with self.assertRaisesRegex(splined.SplinedError, "username and password"):
            splined.load_navidrome_credentials(self.config_path, self.cfg)


if __name__ == "__main__":
    unittest.main()
