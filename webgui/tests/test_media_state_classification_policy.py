from __future__ import annotations

import unittest

from webgui.media_state_policy import classify_media_state, media_state_policy


class MediaStateClassificationPolicyTests(unittest.TestCase):
    def test_artist_name_priority_and_forbidden_colors_are_shared_policy(self) -> None:
        policy = media_state_policy()

        self.assertEqual(
            classify_media_state(
                "artist", {"missing_artist_art", "embedding_issues", "incomplete_or_error"}
            )["color_name"],
            "yellow",
        )
        self.assertEqual(
            classify_media_state(
                "artist", {"embedding_issues", "incomplete_or_error", "opposing_override"}
            )["color_name"],
            "purple",
        )
        self.assertEqual(
            classify_media_state("artist", {"incomplete_or_error", "opposing_override"})[
                "color_name"
            ],
            "red",
        )
        self.assertEqual(
            policy["artist"]["forbidden_name_colors"], ["orange", "gold", "blue"]
        )


    def test_album_red_is_track_only_and_override_presentation_is_complete(self) -> None:
        policy = media_state_policy()

        self.assertEqual(
            classify_media_state("album", {"missing_art"})["color_name"], "yellow"
        )
        self.assertEqual(
            classify_media_state("album", {"incomplete_tracks"})["color_name"], "red"
        )
        self.assertEqual(
            classify_media_state("album", {"missing_art", "opposing_override"})[
                "color_name"
            ],
            "dark_grey",
        )
        self.assertEqual(policy["album"]["red_definition"], "Incomplete album tracks")
        self.assertEqual(
            policy["override_presentation"],
            {
                "condition": "opposing_override",
                "banner_kind": "override",
                "grayscale_thumbnail": True,
                "frame_color": "dark_grey",
                "status_dot_color": "dark_grey",
                "preserve_album_classification": True,
            },
        )


    def test_policy_supports_standalone_and_optional_cross_client_evidence(self) -> None:
        self.assertEqual(classify_media_state("artist", set())["ui_status_key"], "unprocessed")
        self.assertEqual(
            classify_media_state("album", {"processed"})["ui_status_key"], "processed"
        )
        self.assertEqual(
            classify_media_state("artist", {"opposing_override"})["ui_status_key"],
            "overridden",
        )
        self.assertEqual(
            classify_media_state("album", {"opposing_override"})["ui_status_key"],
            "overridden",
        )
