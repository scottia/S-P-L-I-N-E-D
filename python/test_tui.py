from __future__ import annotations

import unittest

from tui.animation import EXPANSION, STYLIZED_EXPANSION, cell_width, startup_frame
from tui.dialogs import confirm_key
from tui.dispatch import decide_activation
from tui.keys import Action, map_key, picker_response
from tui.layout import Breakpoint, breakpoint, layout_spec
from tui.palette import CHALK, OLED
from tui.semantic import Semantic, range_semantic
from tui.splined_tui import CandidateView, TuiAdapter, TuiState, handle_key
from tui.theme import ThemeName, select_theme


class _Key:
    def __init__(self, code: str, *, ctrl: bool = False, shift: bool = False):
        self.code = code
        self.ctrl = ctrl
        self.shift = shift


class ThemeTests(unittest.TestCase):
    def test_oled_semantic_mapping_reuses_engine_identity(self):
        self.assertEqual(OLED["background"], (0, 0, 0))
        self.assertEqual(OLED["accepted"], (60, 255, 135))
        self.assertEqual(OLED["active"], (55, 225, 255))
        self.assertEqual(OLED["fallback"], (255, 145, 35))
        self.assertEqual(OLED["rejected"], (255, 70, 95))
        self.assertEqual(OLED["history"], (175, 95, 255))

    def test_chalk_is_a_distinct_mineral_palette(self):
        self.assertEqual(CHALK["background"], (24, 25, 27))
        self.assertEqual(CHALK["accepted"], (144, 177, 137))
        self.assertEqual(CHALK["fallback"], (190, 145, 74))
        self.assertEqual(CHALK["rejected"], (174, 91, 77))
        self.assertNotEqual(CHALK["active"], OLED["active"])

    def test_theme_selection_has_exactly_two_names(self):
        self.assertEqual(select_theme(None).name, ThemeName.OLED)
        self.assertEqual(select_theme("chalk").name, ThemeName.CHALK)
        with self.assertRaisesRegex(ValueError, "OLED or CHALK"):
            select_theme("rainbow")

    def test_range_semantics_are_theme_independent(self):
        self.assertEqual(range_semantic("Ideal"), Semantic.ACCEPTED)
        self.assertEqual(range_semantic("LowerRange"), Semantic.FALLBACK)
        self.assertEqual(range_semantic("BelowMinimum"), Semantic.REJECTED)


class LayoutAndBrandTests(unittest.TestCase):
    def test_responsive_breakpoints(self):
        self.assertEqual(breakpoint(47, 30), Breakpoint.MINIMUM)
        self.assertEqual(breakpoint(79, 30), Breakpoint.COMPACT)
        self.assertEqual(breakpoint(100, 30), Breakpoint.NORMAL)
        self.assertEqual(breakpoint(140, 40), Breakpoint.WIDE)
        self.assertTrue(layout_spec(70, 25).stack_cards)
        self.assertNotIn("id", layout_spec(90, 30).candidate_columns)
        self.assertIn("id", layout_spec(140, 40).candidate_columns)

    def test_unicode_brand_forms_have_equal_cell_width(self):
        self.assertEqual(cell_width(EXPANSION), cell_width(STYLIZED_EXPANSION))
        self.assertEqual(startup_frame(0).phrase, EXPANSION)
        self.assertEqual(startup_frame(99).phrase, STYLIZED_EXPANSION)
        self.assertTrue(startup_frame(99).complete)


class DispatchTests(unittest.TestCase):
    def test_interactive_operational_scan_defaults_to_tui(self):
        decision = decide_activation(
            tui=False,
            no_tui=False,
            operational=True,
            stdin_tty=True,
            stdout_tty=True,
        )
        self.assertTrue(decision.enabled)
        self.assertFalse(decision.explicit)

    def test_non_tty_and_no_tui_preserve_plain_cli(self):
        self.assertFalse(
            decide_activation(
                tui=False,
                no_tui=False,
                operational=True,
                stdin_tty=False,
                stdout_tty=True,
            ).enabled
        )
        self.assertFalse(
            decide_activation(
                tui=False,
                no_tui=True,
                operational=True,
                stdin_tty=True,
                stdout_tty=True,
            ).enabled
        )

    def test_explicit_tui_rejects_non_tty_and_non_scan_commands(self):
        with self.assertRaisesRegex(ValueError, "interactive"):
            decide_activation(
                tui=True,
                no_tui=False,
                operational=True,
                stdin_tty=False,
                stdout_tty=True,
            )
        with self.assertRaisesRegex(ValueError, "operational scan"):
            decide_activation(
                tui=True,
                no_tui=False,
                operational=False,
                stdin_tty=True,
                stdout_tty=True,
            )


class KeyAndActionTests(unittest.TestCase):
    def test_established_picker_letters_map_to_same_engine_answers(self):
        expected = {
            "s": (Action.SUGGESTED, "s"),
            "f": (Action.FALLBACK_EDIT, "f"),
            "m": (Action.MUSICBRAINZ, "m"),
            "b": (Action.BYPASS, "b"),
            "k": (Action.KEEP, "k"),
        }
        for key, (action, response) in expected.items():
            with self.subTest(key=key):
                self.assertEqual(map_key(key), action)
                self.assertEqual(picker_response(action), response)
        self.assertEqual(picker_response(Action.ACTIVATE, 2), "3")

    def test_navigation_and_dialog_mapping(self):
        self.assertEqual(map_key("up"), Action.UP)
        self.assertEqual(map_key("tab", shift=True), Action.PREVIOUS_REGION)
        self.assertEqual(map_key("c", ctrl=True), Action.QUIT)
        self.assertTrue(confirm_key("y"))
        self.assertFalse(confirm_key("esc"))
        self.assertIsNone(confirm_key("x"))

    def test_candidate_selection_and_confirmed_bypass_submit_engine_values(self):
        state = TuiState(workflow="picker")
        state.candidates = [
            CandidateView(1, "iTunes", 1800, 1800, "jpeg", "Ideal", 0, True, True, True, "one"),
            CandidateView(2, "Discogs", 1200, 1000, "jpeg", "BelowMinimum", 800, False, False, True, "two"),
        ]
        state.apply("input", {"prompt": "Choice: ", "context": {"kind": "fallback-picker"}})
        adapter = TuiAdapter()
        adapter.waiting.set()
        handle_key(state, adapter, _Key("down"))
        handle_key(state, adapter, _Key("enter"))
        self.assertEqual(adapter.responses.get_nowait(), "2")

        state.apply("input", {"prompt": "Choice: ", "context": {"kind": "fallback-picker"}})
        adapter.waiting.set()
        handle_key(state, adapter, _Key("b"))
        self.assertTrue(state.dialog_open)
        handle_key(state, adapter, _Key("y"))
        self.assertEqual(adapter.responses.get_nowait(), "b")


if __name__ == "__main__":
    unittest.main()
