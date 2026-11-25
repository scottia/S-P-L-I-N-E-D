"""Shared Artist credit separator behavior for PostgreSQL projection."""

from __future__ import annotations

import unittest

from splined_media_tags import ARTIST_CREDIT_SEPARATOR, normalize_artist_credit_text
from webgui.catalog import explicit_artist_credits


class ArtistSeparatorContractTests(unittest.TestCase):
    def test_navidrome_default_splits_only_configured_joined_values(self):
        self.assertEqual(
            normalize_artist_credit_text("Owner feat. Guest"),
            f"Owner{ARTIST_CREDIT_SEPARATOR}Guest",
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
        value = f"Rob Wasserman{ARTIST_CREDIT_SEPARATOR}Aaron Neville"
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
            f"Rob Wasserman{ARTIST_CREDIT_SEPARATOR}Aaron Neville",
        )

    def test_id3v23_slash_multivalue_uses_ordered_mbid_cardinality(self):
        rob = "77b5f0ca-5c13-4348-8d4d-d6b15440e3ec"
        aaron = "2f53411b-ec29-4979-aa46-62786a2bb377"
        self.assertEqual(
            normalize_artist_credit_text(
                "Rob Wasserman/Aaron Neville",
                artist_mbids=f"{rob}; {aaron}",
            ),
            f"Rob Wasserman{ARTIST_CREDIT_SEPARATOR}Aaron Neville",
        )
        self.assertEqual(
            normalize_artist_credit_text("AC/DC", artist_mbids=rob),
            "AC/DC",
        )

    def test_one_mbid_preserves_a_real_name_containing_configured_separator(self):
        identity = "3df0d7b5-79e8-4a3f-8f7b-2ea66b5ed106"
        self.assertEqual(
            normalize_artist_credit_text(
                "Ike & Tina Turner", r"\s+&\s+", artist_mbids=identity
            ),
            "Ike & Tina Turner",
        )
        self.assertEqual(
            normalize_artist_credit_text(
                "Owner & Guest",
                r"\s+&\s+",
                artist_mbids=f"{identity}; 2f53411b-ec29-4979-aa46-62786a2bb377",
            ),
            f"Owner{ARTIST_CREDIT_SEPARATOR}Guest",
        )


if __name__ == "__main__":
    unittest.main()
