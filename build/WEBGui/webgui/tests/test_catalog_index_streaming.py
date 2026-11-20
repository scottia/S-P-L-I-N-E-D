"""Requirement 1 acceptance: filesystem inventory is lazy and bounded."""

from __future__ import annotations

import os
from pathlib import Path
import tempfile
import tracemalloc
import unittest
from unittest.mock import patch

import splined as core


class CatalogIndexStreamingTests(unittest.TestCase):
    def test_inventory_yields_before_later_directories_are_enumerated(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = root / "A Artist" / "A Album"
            later = root / "Z Artist" / "Z Album"
            first.mkdir(parents=True)
            later.mkdir(parents=True)
            (first / "01.mp3").touch()
            (later / "01.mp3").touch()
            original_scandir = os.scandir
            visited = []

            def monitored(path):
                visited.append(Path(path))
                return original_scandir(path)

            with patch.object(core.os, "scandir", side_effect=monitored):
                inventory = core.iter_inventory(root, [], "cover")
                yielded = next(inventory)

            self.assertEqual(yielded.path, first)
            self.assertNotIn(later, visited)

    def test_streaming_inventory_is_complete_unique_and_memory_bounded(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            expected = set()
            for group in range(20):
                for album in range(50):
                    path = root / f"Artist {group:03d}" / f"Album {album:03d}"
                    path.mkdir(parents=True)
                    (path / "01.mp3").touch()
                    expected.add(path)
            (root / "Artist 999" / "Empty Album").mkdir(parents=True)

            tracemalloc.start()
            discovered = set()
            duplicate = False
            for item in core.iter_inventory(root, [], "cover"):
                duplicate = duplicate or item.path in discovered
                discovered.add(item.path)
            _, peak = tracemalloc.get_traced_memory()
            tracemalloc.stop()

            self.assertFalse(duplicate)
            self.assertEqual(discovered, expected)
            self.assertLess(
                peak,
                8 * 1024 * 1024,
                f"streaming 1,000 Albums used {peak} bytes",
            )


if __name__ == "__main__":
    unittest.main()
