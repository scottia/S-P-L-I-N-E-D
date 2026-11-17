from __future__ import annotations

import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from webgui.media_state_policy import (
    POLICY_FILENAME,
    classify_media_state,
    database_monitor_presentation,
    editable_policy_path,
    media_state_policy,
)


class MediaStateClassificationPolicyTests(unittest.TestCase):
    def test_editable_policy_uses_configured_credential_directory(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            root = Path(raw_root)
            config_file = root / "config.toml"
            config_file.write_text(
                '[credentials]\ncredential_dir = "private-credentials"\n',
                encoding="utf-8",
            )
            with patch.dict(os.environ, {"SPLINED_CONFIG": str(config_file)}, clear=False):
                self.assertEqual(
                    editable_policy_path(),
                    root / "private-credentials" / POLICY_FILENAME,
                )

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

    def test_database_monitor_uses_policy_labels_and_colors(self) -> None:
        self.assertEqual(
            database_monitor_presentation("current", "current"),
            {
                "backend": "current",
                "health": "current",
                "text": "Current",
                "description": "Current",
                "color_name": "green",
                "color": "#32dda0",
                "poll_seconds": 180,
            },
        )
        broken = database_monitor_presentation("postgres", "broken")
        self.assertEqual(broken["text"], "Postgres")
        self.assertEqual(broken["color_name"], "red")
        self.assertEqual(broken["poll_seconds"], 180)
