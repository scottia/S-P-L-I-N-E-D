from __future__ import annotations

import io
import json
import sys
import tempfile
import tomllib
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

import requests

import splined
import splined_oauth_validation as validation
import splined_scan


REPOSITORY = Path(__file__).resolve().parent.parent


class FakeResponse:
    def __init__(self, status_code: int, payload: object) -> None:
        self.status_code = status_code
        self.payload = payload
        self.closed = False

    def json(self) -> object:
        return self.payload

    def close(self) -> None:
        self.closed = True


class OAuthValidationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.config_file = self.root / "config" / "config.toml"
        self.config_file.parent.mkdir()
        self.credential_dir = self.root / "private-credentials"
        self.credential_dir.mkdir()
        self.cfg = self._example_config()
        self.cfg["credentials"]["credential_dir"] = "../private-credentials"

    def tearDown(self) -> None:
        self.temporary.cleanup()

    @staticmethod
    def _example_config() -> dict:
        with (REPOSITORY / "docker" / "config.example.toml").open("rb") as handle:
            return tomllib.load(handle)

    def _write_credential(self, provider: str, payload: dict) -> Path:
        path = self.credential_dir / f"{provider}.json"
        path.write_text(json.dumps(payload), encoding="utf-8")
        return path

    @staticmethod
    def _capture(callable_object, *args):
        stdout = io.StringIO()
        stderr = io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            result = callable_object(*args)
        return result, stdout.getvalue(), stderr.getvalue()

    def test_configured_credential_directory_and_fixed_filename_are_used(self) -> None:
        expected = self.credential_dir.resolve()
        self.assertEqual(
            splined.runtime_credential_dir(self.config_file, self.cfg),
            expected,
        )
        self.assertEqual(
            splined.credential_file(self.config_file, self.cfg, "discogs"),
            expected / "discogs.json",
        )

    def test_missing_credentials_are_skip_and_exit_zero(self) -> None:
        result, stdout, stderr = self._capture(
            validation.run_oauth_validation,
            self.config_file,
            self.cfg,
        )
        self.assertEqual(result, 0)
        self.assertEqual(stdout.count("SKIP"), 8)
        self.assertIn("SPLINED API/OAuth Validation Summary", stdout)
        self.assertEqual(stderr, "")

    def test_parser_and_authoritative_help_include_oauth_validation(self) -> None:
        args = splined.parser().parse_args(["--oauth-validation"])
        self.assertTrue(args.oauth_validation)

        _, stdout, stderr = self._capture(
            splined.print_help,
            self.config_file,
            self.cfg,
        )
        self.assertIn("API/OAuth:", stdout)
        self.assertIn("--oauth-validation", stdout)
        self.assertIn("Test saved credential tokens", stdout)
        self.assertEqual(stderr, "")

    def test_scan_entrypoint_dispatches_without_creating_runtime_directories(self) -> None:
        with (
            patch.object(sys, "argv", ["splined", "--oauth-validation"]),
            patch.object(splined, "load_config", return_value=(self.config_file, self.cfg)),
            patch.object(splined, "run_oauth_validation_command", return_value=0) as run,
            patch.object(splined, "ensure_runtime_directories") as ensure,
            patch.object(splined, "init_debug_log") as debug,
        ):
            result = splined_scan.main()

        self.assertEqual(result, 0)
        run.assert_called_once_with(self.config_file, self.cfg)
        ensure.assert_not_called()
        debug.assert_not_called()

    def test_discogs_personal_token_pass_and_request_contract(self) -> None:
        token = "discogs-secret-token"
        self._write_credential("discogs", {"token": token})
        response = FakeResponse(
            200,
            {"results": [{"id": 42, "title": "Nirvana - Nevermind", "year": 1991}]},
        )
        with (
            patch.object(validation, "_randomized", side_effect=lambda values: list(values)),
            patch.object(validation.requests, "request", return_value=response) as request,
        ):
            result, stdout, stderr = self._capture(
                validation._validate_discogs,
                self.config_file,
                self.cfg,
            )

        self.assertEqual(result, validation.PASS)
        self.assertIn("Random test:", stdout)
        self.assertIn("PASS: Discogs personal token accepted", stdout)
        self.assertIn("Nirvana - Nevermind", stdout)
        self.assertNotIn(token, stdout + stderr)
        self.assertTrue(response.closed)
        _, url = request.call_args.args
        kwargs = request.call_args.kwargs
        self.assertEqual(url, "https://api.discogs.com/database/search")
        self.assertEqual(kwargs["headers"]["Authorization"], f"Discogs token={token}")
        self.assertEqual(kwargs["params"]["type"], "release")

    def test_authentication_rejection_is_fail(self) -> None:
        self._write_credential("discogs", {"token": "rejected-token"})
        with patch.object(
            validation.requests,
            "request",
            return_value=FakeResponse(401, {"message": "Unauthorized"}),
        ):
            result, stdout, _stderr = self._capture(
                validation._validate_discogs,
                self.config_file,
                self.cfg,
            )
        self.assertEqual(result, validation.FAIL)
        self.assertIn("token was rejected", stdout)

    def test_valid_credential_with_no_provider_result_is_not_auth_failure(self) -> None:
        self._write_credential("discogs", {"token": "valid-token"})
        with patch.object(
            validation.requests,
            "request",
            side_effect=[FakeResponse(200, {"results": []}) for _ in validation.DISCOGS_TARGETS],
        ):
            result, stdout, _stderr = self._capture(
                validation._validate_discogs,
                self.config_file,
                self.cfg,
            )
        self.assertEqual(result, validation.PASS)
        self.assertIn("no representative search result", stdout)
        self.assertNotIn("rejected", stdout)

    def test_fanarttv_uses_v32_release_group_endpoint_and_album_array(self) -> None:
        api_key = "fanart-api-secret"
        client_key = "fanart-client-secret"
        self._write_credential(
            "fanarttv",
            {"api_version": "v3.2", "api_key": api_key, "client_key": client_key},
        )
        target = validation.FANARTTV_TARGETS[0]
        response = FakeResponse(
            200,
            {
                "albums": [
                    {
                        "release_group_id": target[2],
                        "albumcover": [
                            {
                                "id": "cover-1",
                                "url": "https://assets.fanart.tv/fixture.jpg",
                            }
                        ],
                    }
                ]
            },
        )
        with (
            patch.object(validation, "_randomized", return_value=[target]),
            patch.object(validation.requests, "request", return_value=response) as request,
        ):
            result, stdout, stderr = self._capture(
                validation._validate_fanarttv,
                self.config_file,
                self.cfg,
            )

        self.assertEqual(result, validation.PASS)
        _, url = request.call_args.args
        self.assertEqual(
            url,
            f"https://webservice.fanart.tv/v3.2/music/albums/{target[2]}",
        )
        self.assertIn("release_group_mbid=", stdout)
        self.assertNotIn(api_key, stdout + stderr)
        self.assertNotIn(client_key, stdout + stderr)

    def test_musicbrainz_validates_userinfo_then_release_metadata(self) -> None:
        access_token = "musicbrainz-access-secret"
        self._write_credential(
            "musicbrainz",
            {"access_token": access_token, "refresh_token": "refresh-secret"},
        )
        target = validation.MUSICBRAINZ_TARGETS[0]
        userinfo = FakeResponse(200, {"sub": "public-user-id"})
        metadata = FakeResponse(
            200,
            {
                "id": target[2],
                "title": target[1],
                "artist-credit": [{"name": target[0]}],
                "release-group": {"id": "a4d2a86c-bbd6-352b-b9fa-f9da86df842c"},
            },
        )
        with (
            patch.object(validation, "_randomized", return_value=[target]),
            patch.object(
                validation.requests,
                "request",
                side_effect=[userinfo, metadata],
            ) as request,
        ):
            result, stdout, stderr = self._capture(
                validation._validate_musicbrainz,
                self.config_file,
                self.cfg,
            )

        self.assertEqual(result, validation.PASS)
        self.assertEqual(request.call_count, 2)
        self.assertEqual(
            request.call_args_list[0].args[1],
            "https://musicbrainz.org/oauth2/userinfo",
        )
        self.assertTrue(request.call_args_list[1].args[1].endswith(f"/release/{target[2]}"))
        self.assertEqual(
            request.call_args_list[1].kwargs["params"]["inc"],
            "artist-credits+release-groups",
        )
        self.assertIn("OAuth bearer token was accepted", stdout)
        self.assertIn("release_mbid=", stdout)
        self.assertIn("release_group_mbid=", stdout)
        self.assertNotIn(access_token, stdout + stderr)

    def test_lastfm_uses_album_getinfo_api_key_without_session_fields(self) -> None:
        api_key = "lastfm-api-secret"
        self._write_credential(
            "lastfm",
            {
                "api_key": api_key,
                "shared_secret": "unused-shared-secret",
                "session_key": "unused-session-secret",
            },
        )
        response = FakeResponse(
            200,
            {
                "album": {
                    "artist": "Nirvana",
                    "name": "Nevermind",
                    "image": [{"#text": "https://lastfm.example/cover.jpg"}],
                }
            },
        )
        with patch.object(
            validation.requests,
            "request",
            return_value=response,
        ) as request:
            result, stdout, stderr = self._capture(
                validation._validate_lastfm,
                self.config_file,
                self.cfg,
            )

        self.assertEqual(result, validation.PASS)
        params = request.call_args.kwargs["params"]
        self.assertEqual(params["method"], "album.getInfo")
        self.assertEqual(params["api_key"], api_key)
        self.assertNotIn("session_key", params)
        self.assertNotIn("shared_secret", params)
        self.assertNotIn(api_key, stdout + stderr)

    def test_all_secret_fields_are_absent_from_stdout_and_stderr(self) -> None:
        secrets = {
            "discogs": "discogs-secret",
            "fanart_api": "fanart-api-secret",
            "fanart_client": "fanart-client-secret",
            "mb_access": "mb-access-secret",
            "mb_refresh": "mb-refresh-secret",
            "mb_client": "mb-client-secret",
            "lastfm_api": "lastfm-api-secret",
            "lastfm_shared": "lastfm-shared-secret",
            "lastfm_session": "lastfm-session-secret",
        }
        self._write_credential("discogs", {"token": secrets["discogs"]})
        self._write_credential(
            "fanarttv",
            {
                "api_version": "v3.2",
                "api_key": secrets["fanart_api"],
                "client_key": secrets["fanart_client"],
            },
        )
        self._write_credential(
            "musicbrainz",
            {
                "access_token": secrets["mb_access"],
                "refresh_token": secrets["mb_refresh"],
                "client_secret": secrets["mb_client"],
            },
        )
        self._write_credential(
            "lastfm",
            {
                "api_key": secrets["lastfm_api"],
                "shared_secret": secrets["lastfm_shared"],
                "session_key": secrets["lastfm_session"],
            },
        )

        discogs = (200, {"results": [{"id": 1, "title": "Artist - Album"}]})
        fanart_target = validation.FANARTTV_TARGETS[0]
        fanart = (
            200,
            {
                "albums": [
                    {
                        "release_group_id": fanart_target[2],
                        "albumcover": [{"id": "1", "url": "https://example/cover.jpg"}],
                    }
                ]
            },
        )
        userinfo = (200, {"sub": "public-id"})
        mb_target = validation.MUSICBRAINZ_TARGETS[0]
        metadata = (
            200,
            {
                "id": mb_target[2],
                "title": mb_target[1],
                "artist-credit": [{"name": mb_target[0]}],
                "release-group": {"id": fanart_target[2]},
            },
        )
        lastfm = (200, {"album": {"artist": "Artist", "name": "Album", "image": []}})

        with (
            patch.object(validation, "_randomized", side_effect=lambda values: list(values)),
            patch.object(
                validation,
                "_request_json",
                side_effect=[discogs, fanart, userinfo, metadata, lastfm],
            ),
        ):
            result, stdout, stderr = self._capture(
                validation.run_oauth_validation,
                self.config_file,
                self.cfg,
            )

        self.assertEqual(result, 0)
        combined = stdout + stderr
        for secret in secrets.values():
            self.assertNotIn(secret, combined)
        self.assertIn("Discogs       PASS", stdout)
        self.assertIn("Fanart.tv     PASS", stdout)
        self.assertIn("MusicBrainz   PASS", stdout)
        self.assertIn("Last.fm       PASS", stdout)

    def test_network_exception_message_cannot_leak_secret(self) -> None:
        secret = "do-not-print-this-token"
        self._write_credential("discogs", {"token": secret})
        with patch.object(
            validation.requests,
            "request",
            side_effect=requests.RequestException(f"request URL contained {secret}"),
        ):
            result, stdout, stderr = self._capture(
                validation._validate_discogs,
                self.config_file,
                self.cfg,
            )
        self.assertEqual(result, validation.FAIL)
        self.assertIn("RequestException", stdout)
        self.assertNotIn(secret, stdout + stderr)

    def test_any_configured_failure_makes_summary_nonzero(self) -> None:
        self._write_credential("fanarttv", {"api_version": "v3.2"})
        result, stdout, _stderr = self._capture(
            validation.run_oauth_validation,
            self.config_file,
            self.cfg,
        )
        self.assertEqual(result, 1)
        self.assertIn("Fanart.tv     FAIL", stdout)


if __name__ == "__main__":
    unittest.main()
