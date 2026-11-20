from __future__ import annotations

import json
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
    validate_media_state_policy,
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
            "red",
        )
        self.assertEqual(
            classify_media_state(
                "artist", {"embedding_issues", "incomplete_or_error", "opposing_override"}
            )["color_name"],
            "red",
        )
        self.assertEqual(
            classify_media_state("artist", {"incomplete_or_error", "opposing_override"})[
                "color_name"
            ],
            "red",
        )
        self.assertEqual(
            policy["artist"]["forbidden_name_colors"],
            ["white", "dark_grey", "orange", "gold", "blue"],
        )


    def test_album_red_and_override_presentation_are_independent(self) -> None:
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
        self.assertEqual(policy["album"]["red_definition"], "Incomplete / Error")
        self.assertEqual(policy["album"]["status_dots"][1]["label"], "Incomplete / Error")
        self.assertEqual(policy["artist"]["ranked_states"][1]["label"], "Missing Artist or Album Art")
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
        self.assertEqual(classify_media_state("artist", set())["ui_status_key"], "incomplete")
        self.assertEqual(
            classify_media_state("album", {"processed"})["ui_status_key"], "processed"
        )
        self.assertEqual(
            classify_media_state("artist", {"opposing_override"})["ui_status_key"],
            "incomplete",
        )
        self.assertEqual(
            classify_media_state("album", {"opposing_override"})["ui_status_key"],
            "overridden",
        )

    def test_names_never_fall_back_to_white(self) -> None:
        policy = media_state_policy()
        self.assertEqual(policy["policy_id"], "splined-media-state-v2")
        self.assertEqual(policy["ui_status_colors"]["unprocessed"], "red")
        self.assertEqual(classify_media_state("artist", set())["color_name"], "red")
        self.assertEqual(classify_media_state("album", set())["color_name"], "red")
        self.assertIn("white", policy["artist"]["forbidden_name_colors"])
        self.assertIn("white", policy["album"]["forbidden_name_colors"])

    def test_approved_rankings_and_all_seven_dots_are_enforced(self) -> None:
        original = media_state_policy()
        for scope, key in (("artist", "ranked_states"), ("album", "ranked_name_states")):
            changed = json.loads(json.dumps(original))
            changed[scope][key][0], changed[scope][key][1] = (
                changed[scope][key][1], changed[scope][key][0]
            )
            with self.subTest(scope=scope), self.assertRaisesRegex(ValueError, "ranking"):
                validate_media_state_policy(changed)

        wrong_dots = json.loads(json.dumps(original))
        wrong_dots["album"]["status_dots"][0], wrong_dots["album"]["status_dots"][1] = (
            wrong_dots["album"]["status_dots"][1], wrong_dots["album"]["status_dots"][0]
        )
        with self.assertRaisesRegex(ValueError, "status-dot order"):
            validate_media_state_policy(wrong_dots)

    def test_webgui_library_names_are_not_overridden_by_theme_css(self) -> None:
        root = Path(__file__).resolve().parents[4]
        css = (root / "build/WEBGui/webgui/static/app.css").read_text(encoding="utf-8")
        js = (root / "build/WEBGui/webgui/static/app.js").read_text(encoding="utf-8")
        self.assertNotIn("data-media-status=unprocessed] .row-copy strong", css)
        self.assertIn("color:var(--status-color)", css)
        self.assertIn("color:var(--status-color-light)", css)
        self.assertIn("const processed = status === \"processed\" || status === \"artistComplete\";", js)
        self.assertNotIn('rawStatus === "unprocessed" && !Boolean(album.cover_found', js)

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
