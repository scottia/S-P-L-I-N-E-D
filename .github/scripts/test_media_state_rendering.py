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
