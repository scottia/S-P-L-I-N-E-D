from types import SimpleNamespace
import unittest

from webgui.services import _prioritized_album_inventory


class IndexPrioritizationTests(unittest.TestCase):
    def test_large_albums_are_deferred_without_reordering_ordinary_albums(self):
        albums = [
            SimpleNamespace(path="a-large", audio_files=list(range(100))),
            SimpleNamespace(path="b-small", audio_files=list(range(12))),
            SimpleNamespace(path="c-large", audio_files=list(range(101))),
            SimpleNamespace(path="d-small", audio_files=list(range(9))),
        ]

        ordered = list(_prioritized_album_inventory(iter(albums), 100))

        self.assertEqual(
            [album.path for album in ordered],
            ["b-small", "d-small", "a-large", "c-large"],
        )
        self.assertFalse(ordered[0].deferred_large_album)
        self.assertTrue(ordered[-1].deferred_large_album)

    def test_zero_threshold_preserves_original_order(self):
        albums = [
            SimpleNamespace(path="first", audio_files=list(range(100))),
            SimpleNamespace(path="second", audio_files=[]),
        ]

        ordered = list(_prioritized_album_inventory(iter(albums), 0))

        self.assertEqual([album.path for album in ordered], ["first", "second"])


if __name__ == "__main__":
    unittest.main()
