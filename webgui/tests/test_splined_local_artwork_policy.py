from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest

from PIL import Image

from splined_scan import inspect_local_preflight


def _config(*, preserve: bool) -> dict[str, object]:
    return {
        "mode": "write",
        "output": {
            "file_name": "cover",
            "file_formats": ["jpeg", "png", "webp"],
            "preserve_file": preserve,
            "square": True,
            "square_mode": "crop",
            "square_round_to": 16,
            "upscale_below_ideal": False,
            "evaluate_final_image": True,
        },
        "range": {"min": 1200, "ideal": 1800, "max": 2400, "ladder": 3600},
        "source_policies": {},
    }


class LocalArtworkReplacementPolicyTests(unittest.TestCase):
    def _preflight(self, preserve: bool) -> dict[str, object]:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        album_path = root / "Artist" / "Album"
        album_path.mkdir(parents=True)
        cache = root / "cache"
        cache.mkdir()
        cover = album_path / "cover.jpg"
        Image.new("RGB", (1800, 1800), (4, 5, 6)).save(cover, format="JPEG")
        album = SimpleNamespace(
            path=album_path,
            audio_files=[],
            local_art_files=[cover],
            inventory_fingerprint=None,
        )
        return inspect_local_preflight(
            album,
            _config(preserve=preserve),
            cache,
            ["jpeg", "png", "webp"],
        )

    def test_preserve_true_accepts_ideal_existing_cover_before_discovery(self) -> None:
        preflight = self._preflight(True)
        self.assertEqual(preflight["action"], "local-ideal")
        self.assertEqual(preflight["fallback"], [])

    def test_preserve_false_routes_ideal_existing_cover_to_comparison(self) -> None:
        preflight = self._preflight(False)
        self.assertEqual(preflight["action"], "fallback")
        self.assertEqual(len(preflight["fallback"]), 1)
        self.assertEqual(preflight["fallback"][0].source, "local")


if __name__ == "__main__":
    unittest.main()
