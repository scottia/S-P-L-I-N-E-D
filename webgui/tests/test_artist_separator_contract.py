"""Shared Artist credit separator behavior for PostgreSQL projection."""

from __future__ import annotations

import unittest

from splined_media_tags import normalize_artist_credit_text
from webgui.catalog import explicit_artist_credits


class ArtistSeparatorContractTests(unittest.TestCase):
    def test_navidrome_default_splits_only_configured_joined_values(self):
        self.assertEqual(
            normalize_artist_credit_text("Owner feat. Guest"),
            "Owner\0Guest",
        )
        self.assertEqual(
            explicit_artist_credits("Owner feat. Guest"),
            ("Owner", "Guest"),
        )
        self.assertEqual(
            explicit_artist_credits("Earth, Wind & Fire"),
            ("Earth, Wind & Fire",),
        )

    def test_native_multivalue_tags_ignore_the_custom_joined_separator(self):
        value = "Rob Wasserman\0Aaron Neville"
        self.assertEqual(normalize_artist_credit_text(value, r"\\+"), value)
        self.assertEqual(
            explicit_artist_credits(value, r"\\+"),
            ("Rob Wasserman", "Aaron Neville"),
        )

    def test_custom_backslash_separator_matches_single_joined_value(self):
        self.assertEqual(
            normalize_artist_credit_text(
                r"Rob Wasserman\\Aaron Neville", r"\\+"
            ),
            "Rob Wasserman\0Aaron Neville",
        )


if __name__ == "__main__":
    unittest.main()
