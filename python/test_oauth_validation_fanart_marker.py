from __future__ import annotations

import io
import json
import tempfile
import tomllib
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

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


class FanartValidationMarkerTests(unittest.TestCase):
    def test_non_v32_marker_warns_but_live_v32_success_passes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config_file = root / "config" / "config.toml"
            config_file.parent.mkdir()
            credential_dir = root / "credentials"
            credential_dir.mkdir()

            with (REPOSITORY / "docker" / "config.example.toml").open("rb") as handle:
                cfg = tomllib.load(handle)
            cfg["credentials"]["credential_dir"] = str(credential_dir)

            (credential_dir / "fanarttv.json").write_text(
                json.dumps(
                    {
                        "api_version": "v3",
                        "api_key": "fixture-api-key",
                        "client_key": "fixture-client-key",
                    }
                ),
                encoding="utf-8",
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

            stdout = io.StringIO()
            with (
                redirect_stdout(stdout),
                patch.object(validation, "_randomized", return_value=[target]),
                patch.object(validation.requests, "request", return_value=response),
            ):
                result = validation._validate_fanarttv(config_file, cfg)

            output = stdout.getvalue()
            self.assertEqual(result, validation.PASS)
            self.assertIn("WARN: Fanart.tv", output)
            self.assertIn("testing the saved credentials against the live v3.2 endpoint", output)
            self.assertIn("PASS: Fanart.tv v3.2 credentials accepted", output)
            self.assertNotIn("fixture-api-key", output)
            self.assertNotIn("fixture-client-key", output)


if __name__ == "__main__":
    unittest.main()
