from __future__ import annotations

import io
import tempfile
import tomllib
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

import splined
import splined_oauth_validation as validation


REPOSITORY = Path(__file__).resolve().parent.parent


class FakeResponse:
    def __init__(self, status_code: int, payload: object) -> None:
        self.status_code = status_code
        self.payload = payload

    def json(self) -> object:
        return self.payload

    def close(self) -> None:
        return None

    @property
    def ok(self) -> bool:
        return 200 <= self.status_code < 400


class OAuthValidationRecoveryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.config_file = self.root / "config" / "config.toml"
        self.config_file.parent.mkdir()
        self.credential_dir = self.root / "credentials"
        self.credential_dir.mkdir()
        with (REPOSITORY / "docker" / "config.example.toml").open("rb") as handle:
            self.cfg = tomllib.load(handle)
        self.cfg["credentials"]["credential_dir"] = "../credentials"
        self.credential_payloads: dict[str, dict] = {}

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def _write(self, provider: str, payload: dict) -> None:
        # Recovery tests need the credential file to exist, but credential-like
        # test values do not need to be persisted. Keep them in memory and write
        # only a non-sensitive placeholder document to the temporary fixture.
        self.credential_payloads[provider] = dict(payload)
        (self.credential_dir / f"{provider}.json").write_text(
            "{}\n", encoding="utf-8"
        )

    def _load_json(self, path: Path, _label: str) -> dict:
        return dict(self.credential_payloads[path.stem])

    def _capture(self, callable_object):
        output = io.StringIO()
        with (
            redirect_stdout(output),
            patch.object(validation.core, "load_json", side_effect=self._load_json),
        ):
            result = callable_object(self.config_file, self.cfg)
        return result, output.getvalue()

    def test_discogs_rejection_prints_repair_and_retest_steps(self) -> None:
        self._write("discogs", {"token": "secret"})
        with patch.object(
            validation.requests,
            "request",
            return_value=FakeResponse(401, {"message": "Unauthorized"}),
        ):
            result, output = self._capture(validation._validate_discogs)
        self.assertEqual(result, validation.FAIL)
        self.assertIn("Next steps:", output)
        self.assertIn("valid Discogs personal access token", output)
        self.assertIn("splined --oauth-validation", output)
        self.assertNotIn("secret", output)

    def test_fanart_rejection_prints_credentials_command(self) -> None:
        self._write("fanarttv", {"api_version": "v3.2", "api_key": "secret"})
        with patch.object(
            validation.requests,
            "request",
            return_value=FakeResponse(401, {"status": "error"}),
        ):
            result, output = self._capture(validation._validate_fanarttv)
        self.assertEqual(result, validation.FAIL)
        self.assertIn("splined --fanarttv-credentials", output)
        self.assertIn("splined --oauth-validation", output)
        self.assertNotIn("secret", output)

    def test_musicbrainz_rejection_prints_reauthorization_and_resolution_check(self) -> None:
        self._write(
            "musicbrainz",
            {
                "access_token": "access-secret",
                "refresh_token": "refresh-secret",
                "client_secret": "client-secret",
                "expires_at_unix": 1,
            },
        )
        with patch.object(
            validation.requests,
            "request",
            return_value=FakeResponse(401, {"error": "invalid_token"}),
        ):
            result, output = self._capture(validation._validate_musicbrainz)
        self.assertEqual(result, validation.FAIL)
        self.assertIn("splined --mb-oauth-login", output)
        self.assertIn("Resolution check:", output)
        self.assertIn("expires_at_unix", output)
        self.assertIn("automatically refresh an expired", output)
        self.assertIn("splined --oauth-validation", output)
        self.assertNotIn("access-secret", output)
        self.assertNotIn("refresh-secret", output)
        self.assertNotIn("client-secret", output)

    def test_musicbrainz_refresh_rejection_prints_reauthorization_steps(self) -> None:
        self._write(
            "musicbrainz",
            {
                "access_token": "expired-access-secret",
                "refresh_token": "refresh-secret",
                "client_id": "client-id-secret",
                "client_secret": "client-secret",
                "expires_at_unix": 1,
            },
        )
        with patch.object(
            splined.requests,
            "post",
            return_value=FakeResponse(400, {"error": "invalid_grant"}),
        ):
            result, output = self._capture(validation._validate_musicbrainz)

        self.assertEqual(result, validation.FAIL)
        self.assertIn("OAuth refresh was rejected with HTTP 400", output)
        self.assertIn("splined --mb-oauth-login", output)
        self.assertIn("splined --oauth-validation", output)
        self.assertNotIn("expired-access-secret", output)
        self.assertNotIn("refresh-secret", output)
        self.assertNotIn("client-secret", output)

    def test_lastfm_rejection_prints_credentials_and_optional_login_commands(self) -> None:
        api_key = "lastfm-api-key-do-not-print-7d91"
        shared_secret = "lastfm-shared-secret-do-not-print-4c28"
        self._write(
            "lastfm",
            {"api_key": api_key, "shared_secret": shared_secret},
        )
        with patch.object(
            validation.requests,
            "request",
            return_value=FakeResponse(200, {"error": 10, "message": "Invalid API key"}),
        ):
            result, output = self._capture(validation._validate_lastfm)
        self.assertEqual(result, validation.FAIL)
        self.assertIn("splined --lastfm-credentials", output)
        self.assertIn("splined --lastfm-login", output)
        self.assertIn("splined --oauth-validation", output)
        self.assertNotIn(api_key, output)
        self.assertNotIn(shared_secret, output)


if __name__ == "__main__":
    unittest.main()
