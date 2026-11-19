from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace
import tempfile
import tomllib
import unittest
from unittest.mock import patch

from PIL import Image

import splined as core
from webgui.services import Services


REPOSITORY = Path(__file__).resolve().parents[2]


class AlbumSearchCandidateParityTests(unittest.TestCase):
    def test_search_payload_preserves_windows_candidate_filter_evidence(self) -> None:
        with (REPOSITORY / "docker/config.example.toml").open("rb") as handle:
            config = tomllib.load(handle)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            image_path = root / "candidate.png"
            Image.new("RGB", (1600, 1600), "gold").save(image_path)
            candidate = core.Candidate(
                core.Ref("itunes", "candidate-1", "https://example.invalid/front"),
                image_path,
                1600,
                1600,
                "png",
                0,
            )
            service = Services.__new__(Services)
            service.runtime = SimpleNamespace(
                cfg=config,
                config_file=root / "config.toml",
                temporary_cache=root,
            )
            service.catalog = SimpleNamespace(image=lambda _path: "/api/image/test")
            service.album_row = lambda _album_id: {
                "musicbrainz_albumid": "00000000-0000-0000-0000-000000000001"
            }

            with (
                patch.object(core, "Http", return_value=object()),
                patch.object(core, "lookup_release", return_value=object()),
                patch.object(core, "discover_all", return_value=([], [])),
                patch.object(
                    core, "download_candidates", return_value=([candidate], [])
                ),
            ):
                result = service.search_album("album-1")

        payload = result["candidates"][0]
        self.assertTrue(payload["recommended"])
        self.assertTrue(payload["upscaled"])
        self.assertTrue(payload["acceptable"])
        self.assertEqual(payload["source_range_class"], "LowerRange")
        self.assertEqual(payload["range_class"], "Ideal")
        self.assertEqual(payload["projected_width"], 1800)
        self.assertEqual(payload["projected_height"], 1800)
        self.assertEqual(payload["policy_status"], "accept")
        self.assertFalse(payload["strict_override_active"])
        self.assertEqual(payload["strict_status"], "not-applicable")


if __name__ == "__main__":
    unittest.main()
