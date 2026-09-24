from __future__ import annotations

import json
import time
import unittest
from dataclasses import dataclass
from unittest import mock

from pyratatui import Rect

from tui.aispline import AiCandidate, EnhancementSelection
from tui.keys import Action, map_key
from tui.library import AlbumStatus, ArtistStatus
from tui.splined_tui import (
    CandidateView,
    HitRegion,
    InputRequest,
    TuiAdapter,
    TuiState,
    handle_key,
    handle_mouse,
    hit_test,
    render,
    run_tui,
)
from tui.theme import select_theme


@dataclass
class _Event:
    code: str
    kind: str = "key"
    button: str = "none"
    row: int = 0
    column: int = 0
    ctrl: bool = False
    alt: bool = False
    shift: bool = False


class _Frame:
    def __init__(self, width: int, height: int):
        self.area = Rect(0, 0, width, height)

    def render_widget(self, _widget, _area):
        pass

    def render_stateful_table(self, _widget, _area, _state):
        pass


def _center(region: HitRegion) -> _Event:
    return _Event(
        "down",
        kind="mouse",
        button="left",
        column=region.x + max(0, region.width // 2),
        row=region.y + max(0, region.height // 2),
    )


def _payload(artists: int = 3, albums_each: int = 4) -> dict[str, object]:
    albums = []
    for artist_index in range(artists):
        artist = "10,000 Maniacs" if artist_index == 0 else f"Artist {artist_index:02d}"
        for album_index in range(albums_each):
            title = "Love Among the Ruins" if artist_index == 0 and album_index == 0 else f"Album {album_index:02d}"
            albums.append(
                {
                    "path": f"/music/{artist}/{title}",
                    "artist": artist,
                    "album": title,
                    "status": "unprocessed",
                    "selected": False,
                    "formats": ["JPEG"] if album_index == 0 else [],
                }
            )
    config = {
        "range": {"min": 1200, "ideal": 1800, "max": 2400, "ladder": 3600},
        "output": {"square_round_to": 16, "upscale_below_ideal": True},
        "sources": {"cover_sources": ["itunes", "lastfm", "discogs"]},
        "source_policies": {},
        "aisplined": {"enabled": False},
    }
    return {"root": "/music", "albums": albums, "config": config}


def _library_state(artists: int = 3, albums_each: int = 4) -> TuiState:
    state = TuiState(started_at=time.monotonic() - 10)
    state.apply("library", _payload(artists, albums_each))
    state.apply(
        "input",
        {"prompt": "", "context": {"kind": "library-selection"}},
    )
    return state


def _region(state: TuiState, target: str, index: int | None = None) -> HitRegion:
    return next(
        item
        for item in state.hit_regions
        if item.target == target and (index is None or item.index == index)
    )


class RenderHitMapTests(unittest.TestCase):
    def test_library_hit_geometry_is_rebuilt_at_all_responsive_sizes(self) -> None:
        required = {
            "status-control",
            "select-control",
            "scan-control",
            "artist-row",
            "artist-checkbox",
            "album-row",
            "album-checkbox",
            "artist-filter",
            "album-filter",
            "artist-scroll",
            "album-scroll",
            "source-policy-open",
        }
        for width, height in ((150, 44), (108, 36), (72, 40)):
            state = _library_state()
            render(_Frame(width, height), state, select_theme("OLED"))
            targets = {region.target for region in state.hit_regions}
            with self.subTest(size=(width, height)):
                self.assertTrue(required <= targets)
                self.assertTrue(
                    all(
                        0 <= region.x < width
                        and 0 <= region.y < height
                        and region.x + region.width <= width
                        and region.y + region.height <= height
                        for region in state.hit_regions
                    )
                )

    def test_hit_test_prefers_checkbox_over_containing_row(self) -> None:
        state = _library_state()
        render(_Frame(150, 44), state, select_theme("OLED"))
        checkbox = _region(state, "artist-checkbox", 0)
        hit = hit_test(state, checkbox.x, checkbox.y)
        self.assertIsNotNone(hit)
        self.assertEqual(hit.target, "artist-checkbox")


class LibraryMouseAndFilterTests(unittest.TestCase):
    def setUp(self) -> None:
        self.state = _library_state(artists=40, albums_each=18)
        self.adapter = TuiAdapter()
        self.adapter.waiting.set()
        render(_Frame(150, 44), self.state, select_theme("OLED"))

    def test_artist_and_album_row_vs_checkbox_contract(self) -> None:
        model = self.state.library
        assert model is not None
        artist_one = _region(self.state, "artist-row", 1)
        handle_mouse(self.state, self.adapter, _center(artist_one))
        self.assertEqual(model.active_artist, artist_one.value)
        self.assertFalse(any(item.selected for item in model.albums if item.artist == artist_one.value))

        render(_Frame(150, 44), self.state, select_theme("OLED"))
        artist_check = _region(self.state, "artist-checkbox", 1)
        handle_mouse(self.state, self.adapter, _center(artist_check))
        self.assertTrue(any(item.selected for item in model.albums if item.artist == artist_check.value))

        render(_Frame(150, 44), self.state, select_theme("OLED"))
        album_row = _region(self.state, "album-row", 1)
        albums = model.visible_albums(active_artist_only=True)
        before = albums[1].selected
        handle_mouse(self.state, self.adapter, _center(album_row))
        self.assertEqual(self.state.album_index_cursor, 1)
        self.assertEqual(albums[1].selected, before)
        album_check = _region(self.state, "album-checkbox", 1)
        handle_mouse(self.state, self.adapter, _center(album_check))
        self.assertNotEqual(albums[1].selected, before)

    def test_filter_click_and_real_key_path_update_visible_rows_immediately(self) -> None:
        artist_filter = _region(self.state, "artist-filter")
        handle_mouse(self.state, self.adapter, _center(artist_filter))
        self.assertEqual(self.state.library_focus, 4)
        for letter in "maniacs":
            handle_key(self.state, self.adapter, _Event(letter))
        model = self.state.library
        assert model is not None
        self.assertEqual(model.artist_filter, "maniacs")
        self.assertEqual([row.name for row in model.visible_artists()], ["10,000 Maniacs"])
        handle_key(self.state, self.adapter, _Event("Backspace"))
        self.assertEqual(model.artist_filter, "maniac")
        handle_key(self.state, self.adapter, _Event("Esc"))
        self.assertEqual(self.state.library_focus, 3)

        render(_Frame(150, 44), self.state, select_theme("OLED"))
        album_filter = _region(self.state, "album-filter")
        handle_mouse(self.state, self.adapter, _center(album_filter))
        for letter in "ruins":
            handle_key(self.state, self.adapter, _Event(letter))
        self.assertEqual(model.album_filter, "ruins")
        self.assertEqual(
            [row.title for row in model.visible_albums(active_artist_only=True)],
            ["Love Among the Ruins"],
        )

    def test_slash_page_home_and_end_keyboard_fallback(self) -> None:
        self.state.library_focus = 3
        handle_key(self.state, self.adapter, _Event("/"))
        self.assertEqual(self.state.library_focus, 4)
        handle_key(self.state, self.adapter, _Event("Esc"))
        handle_key(self.state, self.adapter, _Event("End"))
        self.assertEqual(
            self.state.artist_index,
            len(self.state.library.visible_artists()) - 1,  # type: ignore[union-attr]
        )
        handle_key(self.state, self.adapter, _Event("Home"))
        self.assertEqual(self.state.artist_index, 0)
        self.assertEqual(map_key("PageUp"), Action.PAGE_UP)
        self.assertEqual(map_key("PageDown"), Action.PAGE_DOWN)

    def test_wheel_scrolls_viewport_without_changing_focus_or_selection(self) -> None:
        model = self.state.library
        assert model is not None
        initial_artist = model.active_artist
        initial_selection = [item.selected for item in model.albums]
        scroll = _region(self.state, "artist-scroll")
        event = _Event(
            "scroll_down",
            kind="mouse",
            column=scroll.x + 1,
            row=scroll.y + 1,
        )
        handle_mouse(self.state, self.adapter, event)
        self.assertGreater(self.state.artist_scroll, 0)
        self.assertEqual(model.active_artist, initial_artist)
        self.assertEqual([item.selected for item in model.albums], initial_selection)
        render(_Frame(150, 44), self.state, select_theme("OLED"))
        self.assertGreater(self.state.artist_scroll, 0)

        # Album scrolling is an independent viewport operation too. Grow the
        # active artist beyond its visible page without rebuilding the model.
        album_state = _library_state(artists=2, albums_each=60)
        render(_Frame(150, 44), album_state, select_theme("OLED"))
        album_model = album_state.library
        assert album_model is not None
        album_selection = [item.selected for item in album_model.albums]
        album_scroll = _region(album_state, "album-scroll")
        handle_mouse(
            album_state,
            self.adapter,
            _Event(
                "scroll_down",
                kind="mouse",
                column=album_scroll.x + 1,
                row=album_scroll.y + 1,
            ),
        )
        self.assertGreater(album_state.album_scroll, 0)
        self.assertEqual(
            [item.selected for item in album_model.albums], album_selection
        )

    def test_status_select_and_scan_controls_are_direct_actions(self) -> None:
        model = self.state.library
        assert model is not None
        for index in range(6):
            region = _region(self.state, "status-control", index)
            handle_mouse(self.state, self.adapter, _center(region))
            self.assertEqual(self.state.status_index, index)

        # Every Select control acts immediately; none requires Enter.
        model.status_filters = set(AlbumStatus)
        model.artist_status_filters = set(ArtistStatus)
        for index in (0, 1, 2):
            render(_Frame(150, 44), self.state, select_theme("OLED"))
            handle_mouse(
                self.state,
                self.adapter,
                _center(_region(self.state, "select-control", index)),
            )
            self.assertEqual(self.state.select_index, index)
        self.assertTrue(any(item.selected for item in model.albums))

        for index, mode in enumerate(
            ("filtered-read", "filtered-write", "auto-all", "auto-selected")
        ):
            state = _library_state()
            adapter = TuiAdapter()
            adapter.waiting.set()
            render(_Frame(150, 44), state, select_theme("OLED"))
            handle_mouse(
                state,
                adapter,
                _center(_region(state, "scan-control", index)),
            )
            response = json.loads(adapter.responses.get_nowait())
            self.assertEqual(response["scan_mode"], mode)

    def test_filtered_scan_mouse_launch_keeps_multiple_artist_checkbox_scope(self) -> None:
        state = _library_state(artists=3, albums_each=2)
        adapter = TuiAdapter()
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))

        for index in (0, 1):
            handle_mouse(
                state,
                adapter,
                _center(_region(state, "artist-checkbox", index)),
            )
            render(_Frame(150, 44), state, select_theme("OLED"))

        handle_mouse(
            state,
            adapter,
            _center(_region(state, "scan-control", 0)),
        )
        response = json.loads(adapter.responses.get_nowait())
        self.assertEqual(response["scan_mode"], "filtered-read")
        selected = set(response["selected"])
        self.assertEqual(len(selected), 4)
        self.assertTrue(all("Artist 02" not in path for path in selected))
        self.assertEqual(
            selected,
            {
                "/music/10,000 Maniacs/Love Among the Ruins",
                "/music/10,000 Maniacs/Album 01",
                "/music/Artist 01/Album 00",
                "/music/Artist 01/Album 01",
            },
        )


class PolicyAndCandidateMouseTests(unittest.TestCase):
    def test_source_policy_controls_have_direct_hit_actions(self) -> None:
        state = _library_state()
        adapter = TuiAdapter()
        render(_Frame(150, 44), state, select_theme("CHALK"))
        handle_mouse(state, adapter, _center(_region(state, "source-policy-open")))
        self.assertEqual(state.workspace, "policy")
        render(_Frame(150, 44), state, select_theme("CHALK"))
        self.assertTrue(
            {"policy-source", "policy-source-enabled", "policy-field", "policy-global-field"}
            <= {item.target for item in state.hit_regions}
        )
        enabled = _region(state, "policy-source-enabled", 0)
        source = enabled.value
        before = state.policy.policies[source]["enabled"]  # type: ignore[union-attr]
        handle_mouse(state, adapter, _center(enabled))
        self.assertNotEqual(state.policy.policies[source]["enabled"], before)  # type: ignore[union-attr]

        render(_Frame(150, 44), state, select_theme("CHALK"))
        source_row = _region(state, "policy-source", 2)
        handle_mouse(state, adapter, _center(source_row))
        self.assertEqual(state.artist_index, 2)

        render(_Frame(150, 44), state, select_theme("CHALK"))
        minimum_range = _region(state, "policy-field", 2)
        source = state.policy.source_order[state.artist_index]  # type: ignore[union-attr]
        old_range = state.policy.policies[source]["minimum_range_type"]  # type: ignore[union-attr]
        handle_mouse(state, adapter, _center(minimum_range))
        self.assertNotEqual(
            state.policy.policies[source]["minimum_range_type"], old_range  # type: ignore[union-attr]
        )
        render(_Frame(150, 44), state, select_theme("CHALK"))
        global_field = _region(state, "policy-global-field", 1)
        handle_mouse(state, adapter, _center(global_field))
        self.assertEqual((state.library_focus, state.status_index), (2, 1))

    def test_policy_and_candidate_wheels_scroll_the_region_under_pointer(self) -> None:
        policy_state = _library_state()
        policy_state.workspace = "policy"
        render(_Frame(72, 40), policy_state, select_theme("OLED"))
        source_scroll = _region(policy_state, "policy-source-scroll")
        handle_mouse(
            policy_state,
            TuiAdapter(),
            _Event(
                "scroll_down",
                kind="mouse",
                column=source_scroll.x + 1,
                row=source_scroll.y + 1,
            ),
        )
        self.assertGreater(policy_state.policy_source_scroll, 0)

        candidate_state = self._candidate_state()
        render(_Frame(160, 44), candidate_state, select_theme("OLED"))
        selected = candidate_state.selected_index
        group_scroll = _region(candidate_state, "candidate-scroll", 0)
        handle_mouse(
            candidate_state,
            TuiAdapter(),
            _Event(
                "scroll_down",
                kind="mouse",
                column=group_scroll.x + 1,
                row=group_scroll.y + 1,
            ),
        )
        self.assertGreater(candidate_state.group_scroll, 0)
        self.assertEqual(candidate_state.selected_index, selected)

    def _candidate_state(self) -> TuiState:
        state = TuiState(started_at=time.monotonic() - 10, workflow="picker")
        state.candidates = [
            CandidateView(1, "Local", 1500, 1500, "jpeg", "LowerRange", 300, True, True, True, "local", provenance="[LOCAL]", ai_review=True, ai_key="local"),
            CandidateView(2, "iTunes", 1600, 1600, "jpeg", "LowerRange", 200, True, True, True, "remote", suggested=True, url="https://example.test/cover.jpg", provenance="[URL]", ai_review=True, ai_key="remote"),
            CandidateView(3, "Discogs", 1200, 1000, "jpeg", "BelowMinimum", 800, False, False, True, "discogs", url="https://example.test/discogs.jpg", provenance="[URL]", ai_review=False, ai_key="discogs"),
        ]
        state.input_request = InputRequest("Choice: ", "fallback-picker", {})
        state.ai_enabled = True
        state.ai_runtime_available = True
        state.upscale_below_ideal = True
        selection = EnhancementSelection(True, True, 600, False, 1800)
        for candidate in state.candidates:
            selection.register(
                AiCandidate(
                    candidate.ai_key,
                    min(candidate.width, candidate.height),
                    candidate.provenance,
                    candidate.ai_review,
                )
            )
        state.ai_selection = selection
        return state

    def test_candidate_url_and_ai_controls_use_same_state_actions(self) -> None:
        state = self._candidate_state()
        adapter = TuiAdapter()
        render(_Frame(160, 44), state, select_theme("OLED"))
        candidate = _region(state, "candidate-row")
        handle_mouse(state, adapter, _center(candidate))
        self.assertEqual(state.selected_index, candidate.index)

        url = _region(state, "candidate-url")
        with mock.patch("tui.splined_tui.webbrowser.open", return_value=True) as opened:
            handle_mouse(state, adapter, _center(url))
        opened.assert_called_once_with(url.value, new=2, autoraise=False)

        ai = _region(state, "candidate-ai", 0)
        handle_mouse(state, adapter, _center(ai))
        self.assertEqual(state.ai_selection.selected_key, "local")
        self.assertTrue(state.ai_selection.disabled("remote"))

    def test_dialog_yes_and_no_are_direct(self) -> None:
        for target, expected in (("dialog-yes", "b"), ("dialog-no", None)):
            state = self._candidate_state()
            state.dialog_open = True
            state.dialog_kind = "bypass"
            adapter = TuiAdapter()
            adapter.waiting.set()
            render(_Frame(100, 30), state, select_theme("OLED"))
            handle_mouse(state, adapter, _center(_region(state, target)))
            if expected is None:
                self.assertFalse(state.dialog_open)
                self.assertTrue(adapter.responses.empty())
            else:
                self.assertEqual(adapter.responses.get_nowait(), expected)


class CaptureLifecycleTests(unittest.TestCase):
    class FakeTerminal:
        entered = 0
        restored = 0

        def __enter__(self):
            type(self).entered += 1
            return self

        def __exit__(self, *_args):
            type(self).restored += 1
            return False

        def restore(self):
            type(self).restored += 1

        def draw(self, _callback):
            pass

    @staticmethod
    def reader(event: _Event, *, fail_enter: bool = False):
        class FakeReader:
            enabled = 0
            disabled = 0

            def __enter__(self):
                type(self).enabled += 1
                if fail_enter:
                    raise OSError("capture failed")
                return self

            def __exit__(self, *_args):
                type(self).disabled += 1
                return False

            def disable_mouse_capture(self):
                type(self).disabled += 1

            def poll_event(self, timeout_ms=0):
                del timeout_ms
                time.sleep(0.01)
                return event

        return FakeReader

    def setUp(self) -> None:
        self.FakeTerminal.entered = 0
        self.FakeTerminal.restored = 0

    def test_capture_disabled_on_ctrl_c(self) -> None:
        reader = self.reader(_Event("c", ctrl=True))
        with mock.patch("tui.splined_tui.Terminal", self.FakeTerminal), mock.patch(
            "tui.splined_tui.InputEventReader", reader
        ):
            self.assertEqual(run_tui(lambda: 0), 130)
        self.assertEqual(reader.enabled, 1)
        self.assertEqual(reader.disabled, 1)
        self.assertEqual(self.FakeTerminal.restored, 1)

    def test_capture_enabled_and_disabled_on_normal_completion(self) -> None:
        reader = self.reader(_Event("Enter"))
        with mock.patch("tui.splined_tui.Terminal", self.FakeTerminal), mock.patch(
            "tui.splined_tui.InputEventReader", reader
        ):
            self.assertEqual(run_tui(lambda: 0), 0)
        self.assertEqual(reader.enabled, 1)
        self.assertEqual(reader.disabled, 1)
        self.assertEqual(self.FakeTerminal.restored, 1)

    def test_capture_disabled_when_worker_raises(self) -> None:
        reader = self.reader(_Event("Enter"))

        def worker():
            raise RuntimeError("engine failed")

        with mock.patch("tui.splined_tui.Terminal", self.FakeTerminal), mock.patch(
            "tui.splined_tui.InputEventReader", reader
        ):
            with self.assertRaisesRegex(RuntimeError, "engine failed"):
                run_tui(worker)
        self.assertEqual(reader.disabled, 1)
        self.assertEqual(self.FakeTerminal.restored, 1)

    def test_partial_capture_initialization_is_cleaned_up(self) -> None:
        reader = self.reader(_Event("c", ctrl=True), fail_enter=True)
        with mock.patch("tui.splined_tui.Terminal", self.FakeTerminal), mock.patch(
            "tui.splined_tui.InputEventReader", reader
        ):
            with self.assertRaisesRegex(Exception, "initialization failed"):
                run_tui(lambda: 0)
        self.assertEqual(reader.disabled, 1)
        self.assertGreaterEqual(self.FakeTerminal.restored, 1)


if __name__ == "__main__":
    unittest.main()
