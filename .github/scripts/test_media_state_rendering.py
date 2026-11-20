"""Enforce mode-independent Artist/Album name-color routing at source boundaries."""
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class MediaStateRenderingContracts(unittest.TestCase):
    def test_canonical_policy_is_used_in_windows_and_webgui(self):
        rust = (ROOT / "build/Windows/src/media_state_policy.rs").read_text(encoding="utf-8")
        python = (ROOT / "build/WEBGui/webgui/media_state_policy.py").read_text(encoding="utf-8")
        manifest = (ROOT / "build/WEBGui/Dockerfile").read_text(encoding="utf-8")
        self.assertIn('include_str!("../../../schema/media-state-classification.json")', rust)
        self.assertIn('source_root() / "schema" / "media-state-classification.json"', python)
        self.assertIn('schema/media-state-classification.json', manifest)

        policy = json.loads((ROOT / "schema/media-state-classification.json").read_text(encoding="utf-8"))
        self.assertEqual([r["color"] for r in policy["artist"]["ranked_states"]],
                         ["red", "yellow", "purple", "green", "red"])
        self.assertEqual([r["color"] for r in policy["album"]["ranked_name_states"]],
                         ["dark_grey", "red", "yellow", "purple", "orange", "green", "red"])
        self.assertEqual([r["color"] for r in policy["album"]["status_dots"]],
                         ["dark_grey", "red", "yellow", "purple", "orange", "green", "blue"])

    def test_owner_confirmed_name_priorities_and_unprocessed_separation(self):
        policy = json.loads((ROOT / "schema/media-state-classification.json").read_text(encoding="utf-8"))
        self.assertEqual(
            [(rule["condition"], rule["color"]) for rule in policy["artist"]["ranked_states"]],
            [
                ("incomplete_or_error", "red"),
                ("missing_artist_art", "yellow"),
                ("embedding_issues", "purple"),
                ("all_albums_complete_or_overridden", "green"),
                ("default", "red"),
            ],
        )
        self.assertEqual(
            [(rule["condition"], rule["color"]) for rule in policy["album"]["ranked_name_states"]],
            [
                ("opposing_override", "dark_grey"),
                ("incomplete_tracks", "red"),
                ("missing_art", "yellow"),
                ("embedding_issues", "purple"),
                ("processed", "orange"),
                ("tracks_complete", "green"),
                ("default", "red"),
            ],
        )
        self.assertTrue(
            {"white", "dark_grey", "orange", "gold", "blue"}.issubset(
                set(policy["artist"]["forbidden_name_colors"])
            )
        )
        self.assertTrue(
            {"white", "blue"}.issubset(set(policy["album"]["forbidden_name_colors"]))
        )
        for scope, key in (("artist", "ranked_states"), ("album", "ranked_name_states")):
            self.assertNotIn("white", [rule["color"] for rule in policy[scope][key]])
            self.assertEqual(policy[scope][key][-1]["condition"], "default")
            self.assertEqual(policy[scope][key][-1]["color"], "red")
            self.assertNotIn("unprocessed", [rule["condition"] for rule in policy[scope][key]])
        self.assertEqual(
            [rule["color"] for rule in policy["album"]["status_dots"]],
            ["dark_grey", "red", "yellow", "purple", "orange", "green", "blue"],
        )
        self.assertEqual(policy["album"]["status_dots"][-1]["condition"], "animated_webp")
        self.assertTrue(policy["album"]["status_dots"][-1]["auxiliary"])
        self.assertEqual(policy["ui_status_colors"]["unprocessed"], "red")
        # This is a filter/legend palette entry, NOT an Artist/Album name category.

    def test_no_theme_override_of_library_names(self):
        slint = (ROOT / "build/Windows/ui/app.slint").read_text(encoding="utf-8")
        css = (ROOT / "build/WEBGui/webgui/static/app.css").read_text(encoding="utf-8")
        self.assertIn('Text { text: root.item.primary; color: root.item.status-color;', slint)
        self.assertNotIn('root.item.status == "Unprocessed" || root.item.status == "All"', slint)
        self.assertNotIn("data-media-status=unprocessed] .row-copy strong", css)
        self.assertIn("color:var(--status-color)", css)
        self.assertIn("color:var(--status-color-light)", css)


if __name__ == "__main__":
    unittest.main()
