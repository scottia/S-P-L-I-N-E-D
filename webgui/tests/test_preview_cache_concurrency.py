from concurrent.futures import ThreadPoolExecutor
from io import BytesIO
import os
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Barrier, Lock
import time
import unittest
from unittest.mock import patch

from PIL import Image
from webgui.catalog import Catalog


class PreviewCacheTests(unittest.TestCase):
    def pixels(self):
        output = BytesIO()
        Image.new("RGB", (80, 80), "blue").save(output, "JPEG")
        return output.getvalue()

    def test_concurrent_requests_publish_identical_pixels_once(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            catalog = Catalog(root, root / "unused.sqlite", root, root,
                              cache=root / "cache", temporary_cache=root / "temporary")
            pixels = self.pixels()
            start = Barrier(12)
            publication = Lock()
            replace = os.replace

            def windows_publication(source, target):
                with publication:
                    if Path(target).exists():
                        raise PermissionError("Windows reader holds the published preview")
                    time.sleep(0.02)
                    replace(source, target)

            def request(_):
                start.wait()
                return catalog.image(pixels)

            with patch("webgui.catalog.os.replace", side_effect=windows_publication) as publish:
                with ThreadPoolExecutor(max_workers=12) as pool:
                    handles = list(pool.map(request, range(12)))
            self.assertEqual(len(set(handles)), 1)
            self.assertEqual(publish.call_count, 1)
            self.assertEqual(catalog.image_bytes(handles[0]), pixels)
            self.assertTrue(list((root / "temporary/web-images").glob("*.jpg")))
            self.assertFalse((root / "cache/web-images").exists())
            self.assertFalse(list((root / "temporary/web-images").glob(".prepared-*")))

    def test_existing_preview_is_not_replaced_while_reader_is_open(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            catalog = Catalog(root, root / "unused.sqlite", root, root,
                              cache=root / "cache", temporary_cache=root / "temporary")
            pixels = self.pixels()
            handle = catalog.image(pixels)
            with catalog.image_source(handle).open("rb") as reader:
                with patch("webgui.catalog.os.replace", side_effect=PermissionError("locked")) as replace:
                    self.assertEqual(catalog.image(pixels), handle)
                    replace.assert_not_called()
                self.assertEqual(reader.read(), pixels)

    def test_real_write_denial_reports_preview_cache_problem(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            catalog = Catalog(root, root / "unused.sqlite", root, root,
                              cache=root / "cache", temporary_cache=root / "temporary")
            with patch("webgui.catalog.os.replace", side_effect=PermissionError("private filesystem path")):
                with self.assertRaisesRegex(ValueError, "preview cache") as caught:
                    catalog.image(self.pixels())
            self.assertNotIn("private filesystem path", str(caught.exception))
            self.assertFalse(list((root / "temporary/web-images").glob(".prepared-*")))


if __name__ == "__main__":
    unittest.main()
