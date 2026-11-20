from __future__ import annotations

from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest

from PIL import Image

from interaction.album_workspace import AlbumWorkspace


class AlbumWorkspaceTests(unittest.TestCase):

    def test_adjustments_preview_then_explicit_write(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            album = root / "Album"
            cache = root / "cache"
            album.mkdir()
            cache.mkdir()
            cover = album / "cover.jpg"
            Image.new("RGB", (90, 60), (80, 100, 120)).save(cover, format="JPEG")
            before = cover.read_bytes()
            session = AlbumWorkspace.load(album, cache, writable=True)
            try:
                self.assertEqual(session.saved, cover)
                self.assertTrue((session.preview_dir / "original.png").is_file())
                with Image.open(session.preview_dir / "enhanced.png") as preview:
                    initial_pixel = preview.getpixel((20, 20))
                session.selected_adjustment = 5  # brightness
                session.adjust(20)
                with Image.open(session.preview_dir / "enhanced.png") as preview:
                    self.assertNotEqual(preview.getpixel((20, 20)), initial_pixel)
                self.assertIsNone(session.write_approved())
                self.assertEqual(cover.read_bytes(), before)
                self.assertEqual(session.write_approved(), cover)
                self.assertNotEqual(cover.read_bytes(), before)
                self.assertEqual(session.profile.brightness, 0)
            finally:
                preview = session.preview_dir
                session.close()
                self.assertFalse(preview.exists())

    def test_webp_preview_does_not_replace_animation(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            album = root / "Album"
            cache = root / "cache"
            album.mkdir()
            cache.mkdir()
            animation = album / "cover.webp"
            Image.new("RGB", (50, 50), "blue").save(animation, format="WEBP")
            original = animation.read_bytes()
            session = AlbumWorkspace.load(album, cache, writable=True)
            try:
                self.assertIsNone(session.write_approved())
                self.assertEqual(session.write_approved(), album / "cover.jpg")
                self.assertEqual(animation.read_bytes(), original)
            finally:
                session.close()


if __name__ == "__main__":
    unittest.main()
