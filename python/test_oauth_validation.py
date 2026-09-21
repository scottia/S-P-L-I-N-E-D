from __future__ import annotations

import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

import splined
import splined_oauth_validation as validation


class FakeResponse:
    def __init__(self, status_code: int, payload: dict):
        self.status_code = status_code
        self._payload = payload

    def json(self):
        return self._payload


class OAuthValidationTests(unittest.TestCase):
    def test_uses_configured_credential_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config_file = root / "config" / "config.toml"
            config_file.parent.mkdir()
            cfg = {"credentials": {"credential_dir": "../private"}}
            expected = (config_file.parent / "../private").resolve()
            self.assertEqual(splined.runtime_credential_dir(config_file, cfg), expected)

    def test_missing_credentials_are_skipped(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config_file = Path(directory) / "config.toml"
            cfg = {"credentials": {"credential_dir": str(Path(directory) / "credentials")}}
            out = io.StringIO()
            with redirect_stdout(out):
                rc = validation.run_oauth_validation(config_file, cfg)
            self.assertEqual(rc, 0)
            self.assertIn("SKIP", out.getvalue())
            self.assertNotIn("Authorization:", out.getvalue())

    def test_discogs_secret_is_not_printed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cred_dir = root / "credentials"
            cred_dir.mkdir()
            secret = "fixture-super-secret-token"
            (cred_dir / "discogs.json").write_text(json.dumps({"token": secret}), encoding="utf-8")
            cfg = {"credentials": {"credential_dir": str(cred_dir)}}
            config_file = root / "config.toml"
            response = FakeResponse(200, {"results": [{"id": 1, "title": "Artist - Album", "year": 2000}]})
            out = io.StringIO()
            with patch.object(validation.requests, "request", return_value=response):
                with redirect_stdout(out):
                    result = validation._validate_discogs(config_file, cfg)
            self.assertEqual(result, "pass")
            self.assertNotIn(secret, out.getvalue())

    def test_lastfm_auth_failure_is_distinct(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cred_dir = root / "credentials"
            cred_dir.mkdir()
            (cred_dir / "lastfm.json").write_text(json.dumps({"api_key": "fixture"}), encoding="utf-8")
            cfg = {"credentials": {"credential_dir": str(cred_dir)}}
            response = FakeResponse(200, {"error": 10, "message": "Invalid API key"})
            out = io.StringIO()
            with patch.object(validation.requests, "request", return_value=response):
                with redirect_stdout(out):
                    result = validation._validate_lastfm(root / "config.toml", cfg)
            self.assertEqual(result, "fail")
            self.assertIn("API key rejected", out.getvalue())


if __name__ == "__main__":
    unittest.main()
