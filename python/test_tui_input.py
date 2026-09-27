from __future__ import annotations

import json
import io
import time
import unittest
from dataclasses import dataclass
from unittest import mock

from pyratatui import Rect

from tui.aispline import AiCandidate, EnhancementSelection
from tui.keys import Action, map_key, picker_response
from tui.library import AlbumStatus, ArtistStatus
from tui.semantic import Semantic
from tui.splined_tui import (
    AlbumRunReport,
    CandidateView,
    HitRegion,
    InputRequest,
    STATUS_CONTROLS,
    TuiAdapter,
    _folder_status_count,
    TuiState,
    _status_control_semantic,
    handle_key,
    handle_mouse,
    hit_test,
    osc8_link,
    render,
    run_tui,
    write_terminal_links,
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


class ReportAndStatusAuthorityTests(unittest.TestCase):
    def test_final_album_result_is_not_overwritten_by_late_error_log(self) -> None:
        state = TuiState(started_at=time.monotonic() - 10)
        state.active_report = AlbumRunReport(
            1,
            1,
            "'Til Tuesday",
            "Voices Carry",
            "/music/'Til Tuesday/Voices Carry",
            time.monotonic() - 1,
        )
        state.apply(
            "album_material_result",
            {
                "outcome": "Unchanged",
                "file_action": "Unchanged",
                "destination": "/music/'Til Tuesday/Voices Carry/cover.jpg",
                "source": "Local",
                "width": 1800,
                "height": 1800,
                "format": "jpeg",
                "range_type": "Ideal",
                "distance": 0,
            },
        )
        state.apply(
            "log",
            {"level": "ERROR", "message": "late non-album diagnostic"},
        )
        self.assertEqual(state.active_report.outcome, "Unchanged")
        self.assertEqual(state.active_report.file_action, "Unchanged")

    def test_error_log_still_fails_an_unfinished_album(self) -> None:
        state = TuiState(started_at=time.monotonic() - 10)
        state.active_report = AlbumRunReport(
            1,
            1,
            "Artist",
            "Album",
            "/music/Artist/Album",
            time.monotonic() - 1,
        )
        state.apply("log", {"level": "ERROR", "message": "processing failed"})
        self.assertEqual(state.active_report.outcome, "Failed")

    def test_folder_status_order_and_semantics_match_windows(self) -> None:
        self.assertEqual(
            [label for label, _status in STATUS_CONTROLS],
            [
                "Unprocessed",
                "Processed",
                "Bypassed",
                "Partial / Timeout",
                "Artist Complete",
                "Artist Contains Bypass",
            ],
        )
        self.assertEqual(
            [_status_control_semantic(index) for index in range(6)],
            [
                Semantic.UNPROCESSED,
                Semantic.FALLBACK,
                Semantic.REJECTED,
                Semantic.HISTORY,
                Semantic.ACCEPTED,
                Semantic.DEBUG,
            ],
        )
        album_counts = {
            AlbumStatus.UNPROCESSED: 11,
            AlbumStatus.PROCESSED: 7,
            AlbumStatus.BYPASSED: 2,
            AlbumStatus.TIMEOUT: 3,
        }
        artist_counts = {
            ArtistStatus.UNPROCESSED: 5,
            ArtistStatus.PARTIAL: 4,
            ArtistStatus.COMPLETE: 6,
            ArtistStatus.CONTAINS_BYPASS: 1,
        }
        self.assertEqual(
            [_folder_status_count(i, album_counts, artist_counts) for i in range(6)],
            [16, 7, 2, 7, 6, 1],
        )


class CancelAndBypassSafetyTests(unittest.TestCase):
    def test_escape_is_never_serialized_as_bypass(self) -> None:
        self.assertIsNone(picker_response(Action.BACK, 0))

        state = TuiState(started_at=time.monotonic() - 10, workflow="picker")
        state.candidates = [
            CandidateView(
                1, "iTunes", 1800, 1800, "jpeg", "Ideal", 0,
                True, True, True, "remote",
                suggested=True,
                url="https://example.test/cover.jpg",
                provenance="[URL]",
            )
        ]
        state.input_request = InputRequest("Choice: ", "fallback-picker", {})
        adapter = TuiAdapter()
        adapter.waiting.set()
        handle_key(state, adapter, _Event("Esc"))
        self.assertEqual(adapter.responses.get_nowait(), "__cancel__")

    def test_ctrl_c_cancel_token_is_not_bypass(self) -> None:
        adapter = TuiAdapter()
        adapter.waiting.set()
        adapter.cancel_wait()
        self.assertEqual(adapter.responses.get_nowait(), "__cancel__")

    def test_selecting_bypassed_album_requests_persistent_removal(self) -> None:
        payload = _payload(1, 1)
        albums = payload["albums"]
        assert isinstance(albums, list)
        albums[0]["status"] = "bypassed"
        state = TuiState(started_at=time.monotonic() - 10)
        state.apply("library", payload)
        state.apply(
            "input",
            {"prompt": "", "context": {"kind": "library-selection"}},
        )
        adapter = TuiAdapter()
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))
        handle_mouse(
            state,
            adapter,
            _center(_region(state, "album-checkbox", 0)),
        )
        self.assertEqual(state.dialog_kind, "library-bypass-remove")
        render(_Frame(150, 44), state, select_theme("OLED"))
        handle_mouse(
            state,
            adapter,
            _center(_region(state, "dialog-yes")),
        )
        response = json.loads(adapter.responses.get_nowait())
        self.assertEqual(response["action"], "set-bypass")
        self.assertFalse(response["bypassed"])
        self.assertTrue(response["select_after"])

    def test_b_on_unprocessed_album_requests_persistent_bypass_add(self) -> None:
        state = _library_state(artists=1, albums_each=1)
        state.library_focus = 5
        adapter = TuiAdapter()
        adapter.waiting.set()
        handle_key(state, adapter, _Event("b"))
        self.assertEqual(state.dialog_kind, "library-bypass-add")
        render(_Frame(150, 44), state, select_theme("OLED"))
        handle_mouse(
            state,
            adapter,
            _center(_region(state, "dialog-yes")),
        )
        response = json.loads(adapter.responses.get_nowait())
        self.assertEqual(response["action"], "set-bypass")
        self.assertTrue(response["bypassed"])
        self.assertFalse(response["select_after"])


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
        self.assertTrue(
            any(item.selected for item in model.albums if item.artist == artist_one.value)
        )

        # The explicit checkbox follows the same Artist cascade path and
        # therefore toggles those normally eligible child Albums back off.
        render(_Frame(150, 44), self.state, select_theme("OLED"))
        self.adapter.waiting.set()
        artist_check = _region(self.state, "artist-checkbox", 1)
        handle_mouse(self.state, self.adapter, _center(artist_check))
        self.assertFalse(
            any(item.selected for item in model.albums if item.artist == artist_check.value)
        )

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

    def test_engine_artist_status_is_not_reinterpreted_by_tui(self) -> None:
        payload = _payload(0, 0)
        payload["artists"] = [
            {
                "path": "/music/Authority Artist",
                "name": "Authority Artist",
                "indexed": True,
                "loaded": True,
                "album_count": 1,
                "status": "complete",
            }
        ]
        payload["albums"] = [
            {
                "path": "/music/Authority Artist/New Album",
                "artist": "Authority Artist",
                "album": "New Album",
                "status": "unprocessed",
                "selected": False,
                "formats": [],
            }
        ]
        state = TuiState(started_at=time.monotonic() - 10)
        state.apply("library", payload)
        model = state.library
        assert model is not None
        artist = model.visible_artists()[0]
        self.assertEqual(artist.status, ArtistStatus.COMPLETE)

    def test_processed_album_selection_survives_authoritative_payload(self) -> None:
        payload = _payload(1, 1)
        albums = payload["albums"]
        assert isinstance(albums, list)
        albums[0]["status"] = "processed"
        albums[0]["selected"] = True

        state = TuiState(started_at=time.monotonic() - 10)
        state.apply("library", payload)
        model = state.library
        assert model is not None
        self.assertTrue(model.albums[0].selected)

        state.apply("library_update", payload)
        self.assertTrue(model.albums[0].selected)

    def test_busy_library_mouse_actions_do_not_mutate_or_launch(self) -> None:
        state = _library_state()
        adapter = TuiAdapter()
        render(_Frame(150, 44), state, select_theme("OLED"))
        model = state.library
        assert model is not None

        album = model.visible_albums(active_artist_only=True)[0]
        before = album.selected
        handle_mouse(
            state,
            adapter,
            _center(_region(state, "album-checkbox", 0)),
        )
        self.assertEqual(album.selected, before)
        self.assertTrue(adapter.responses.empty())
        self.assertTrue(state.transient.startswith("Input busy"))

        handle_mouse(
            state,
            adapter,
            _center(_region(state, "scan-control", 3)),
        )
        self.assertEqual(state.workflow, "library")
        self.assertTrue(adapter.responses.empty())

    def test_history_aware_status_counts_override_loaded_only_counts(self) -> None:
        payload = _payload(1, 1)
        payload["status_counts"] = {
            "unprocessed": 7,
            "processed": 41,
            "bypassed": 3,
            "timeout": 2,
        }
        payload["artist_status_counts"] = {
            "unprocessed": 5,
            "partial": 4,
            "complete": 9,
            "contains-bypass": 2,
        }
        state = TuiState(started_at=time.monotonic() - 10)
        state.apply("library", payload)
        model = state.library
        assert model is not None
        self.assertEqual(model.album_status_counts()[AlbumStatus.PROCESSED], 41)
        self.assertEqual(model.album_status_counts()[AlbumStatus.BYPASSED], 3)
        self.assertEqual(
            model.indexed_artist_status_counts()[ArtistStatus.COMPLETE], 9
        )
        self.assertEqual(
            model.indexed_artist_status_counts()[ArtistStatus.CONTAINS_BYPASS],
            2,
        )

    def test_async_selected_stats_apply_only_to_current_first_ten(self) -> None:
        state = _library_state(artists=1, albums_each=2)
        model = state.library
        assert model is not None
        selected = model.albums[0]
        selected.selected = True
        stat = {"path": selected.path, "album": selected.title}
        state.apply(
            "library_selected_stats",
            {
                "selected_paths": [selected.path],
                "selected_album_stats": [stat],
                "limit": 10,
            },
        )
        self.assertEqual(state.selected_album_stats, [stat])

        state.apply(
            "library_selected_stats",
            {
                "selected_paths": ["/music/stale/album"],
                "selected_album_stats": [{"path": "/music/stale/album"}],
                "limit": 10,
            },
        )
        self.assertEqual(state.selected_album_stats, [stat])

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

    def test_uncached_artist_tap_and_keyboard_open_request_one_lazy_inventory(self) -> None:
        payload = _payload(0, 0)
        payload["artists"] = [
            {
                "path": "/music/10,000 Maniacs",
                "name": "10,000 Maniacs",
                "indexed": False,
                "loaded": False,
                "album_count": 0,
            }
        ]
        state = TuiState(started_at=time.monotonic() - 10)
        state.apply("library", payload)
        state.apply("input", {"prompt": "", "context": {"kind": "library-selection"}})
        adapter = TuiAdapter()
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))
        handle_mouse(state, adapter, _center(_region(state, "artist-row", 0)))
        request = json.loads(adapter.responses.get_nowait())
        self.assertEqual(request["action"], "load-artist")
        self.assertEqual(request["artist_path"], "/music/10,000 Maniacs")
        self.assertTrue(request["select_after_load"])

        state.apply("input", {"prompt": "", "context": {"kind": "library-selection"}})
        adapter.waiting.set()
        state.library_focus = 3
        handle_key(state, adapter, _Event("Enter"))
        keyboard_request = json.loads(adapter.responses.get_nowait())
        self.assertEqual(keyboard_request["action"], "load-artist")
        self.assertFalse(keyboard_request["select_after_load"])

    def test_unindexed_artist_checkbox_requests_load_and_select(self) -> None:
        payload = _payload(0, 0)
        payload["artists"] = [
            {
                "path": "/music/Aerosmith",
                "name": "Aerosmith",
                "indexed": False,
                "loaded": False,
                "album_count": 0,
            }
        ]
        state = TuiState(started_at=time.monotonic() - 10)
        state.apply("library", payload)
        state.apply("input", {"prompt": "", "context": {"kind": "library-selection"}})
        adapter = TuiAdapter()
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))
        handle_mouse(state, adapter, _center(_region(state, "artist-checkbox", 0)))
        request = json.loads(adapter.responses.get_nowait())
        self.assertEqual(request["action"], "load-artist")
        self.assertTrue(request["select_after_load"])

    def test_refresh_is_explicit_and_filter_text_r_never_triggers_it(self) -> None:
        model = self.state.library
        assert model is not None
        self.state.library_focus = 4
        handle_key(self.state, self.adapter, _Event("r"))
        self.assertEqual(model.artist_filter, "r")
        self.assertTrue(self.adapter.responses.empty())
        self.state.library_focus = 3
        handle_key(self.state, self.adapter, _Event("r"))
        self.assertEqual(json.loads(self.adapter.responses.get_nowait())["action"], "refresh-index")

    def test_status_select_and_scan_controls_are_direct_actions(self) -> None:
        model = self.state.library
        assert model is not None
        for index in range(6):
            region = _region(self.state, "status-control", index)
            handle_mouse(self.state, self.adapter, _center(region))
            self.assertEqual(self.state.status_index, index)
            if index == 0:
                self.assertNotIn(
                    AlbumStatus.UNPROCESSED, model.status_filters
                )
                self.assertNotIn(
                    ArtistStatus.UNPROCESSED, model.artist_status_filters
                )
            if index == 3:
                self.assertNotIn(AlbumStatus.TIMEOUT, model.status_filters)
                self.assertNotIn(
                    ArtistStatus.PARTIAL, model.artist_status_filters
                )

        # Bulk Select controls act immediately but delegate filesystem work to
        # the engine. Select NONE remains an in-memory operation.
        for index, expected_action in ((0, "select-all"), (2, "select-filtered")):
            state = _library_state()
            adapter = TuiAdapter()
            adapter.waiting.set()
            render(_Frame(150, 44), state, select_theme("OLED"))
            handle_mouse(
                state,
                adapter,
                _center(_region(state, "select-control", index)),
            )
            self.assertEqual(state.select_index, index)
            response = json.loads(adapter.responses.get_nowait())
            self.assertEqual(response["action"], expected_action)
            if expected_action == "select-filtered":
                self.assertIn("artist_paths", response)
                self.assertIn("album_filter", response)
                self.assertIn("status_filters", response)

        state = _library_state()
        none_model = state.library
        assert none_model is not None
        for item in none_model.albums:
            item.selected = True
        adapter = TuiAdapter()
        render(_Frame(150, 44), state, select_theme("OLED"))
        handle_mouse(
            state,
            adapter,
            _center(_region(state, "select-control", 1)),
        )
        self.assertEqual(state.select_index, 1)
        self.assertFalse(any(item.selected for item in none_model.albums))
        self.assertTrue(adapter.responses.empty())

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

    def test_candidate_url_is_a_direct_terminal_link_and_ai_controls_stay_direct(self) -> None:
        state = self._candidate_state()
        adapter = TuiAdapter()
        render(_Frame(160, 44), state, select_theme("OLED"))
        candidate = _region(state, "candidate-row")
        handle_mouse(state, adapter, _center(candidate))
        self.assertEqual(state.selected_index, candidate.index)

        url = _region(state, "candidate-url")
        stream = io.StringIO()
        write_terminal_links(state, stream)
        encoded = stream.getvalue()
        self.assertIn(osc8_link("[URL]", url.value), encoded)
        self.assertNotIn("OPEN IN DEFAULT BROWSER", encoded)
        self.assertNotIn("\x1b[4m", encoded)

        with mock.patch("webbrowser.open", side_effect=AssertionError("server browser")):
            handle_mouse(state, adapter, _center(url))
        self.assertEqual(state.selected_index, url.index)
        self.assertFalse(state.dialog_open)

        render(_Frame(160, 44), state, select_theme("OLED"))
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


class HoverAndStatusLoadingTests(unittest.TestCase):
    def test_url_hover_starts_live_remote_preview_and_leave_clears_it(self) -> None:
        state = CandidateInteractionTests()._candidate_state()
        adapter = TuiAdapter()
        render(_Frame(160, 44), state, select_theme("OLED"))
        url = _region(state, "candidate-url")
        moved = _Event(
            "moved",
            kind="mouse",
            button="none",
            column=url.x + 1,
            row=url.y,
        )
        with mock.patch(
            "tui.splined_tui._start_remote_hover_preview"
        ) as start:
            handle_mouse(state, adapter, moved)
        start.assert_called_once_with(state, adapter, url.index)

        state.remote_hover_index = url.index
        state.remote_hover_loading = True
        handle_mouse(
            state,
            adapter,
            _Event("moved", kind="mouse", button="none", column=0, row=0),
        )
        self.assertEqual(state.remote_hover_index, -1)
        self.assertFalse(state.remote_hover_loading)

    def test_artist_status_loading_blocks_actions_but_not_ctrl_c(self) -> None:
        state = _library_state(artists=1, albums_each=1)
        state.apply(
            "folder_status_progress",
            {
                "processed": 25,
                "total": 100,
                "percent": 25.0,
                "albums": 50,
                "done": False,
            },
        )
        self.assertTrue(state.status_loading)
        adapter = TuiAdapter()
        adapter.waiting.set()

        handle_key(state, adapter, _Event("b"))
        self.assertTrue(adapter.responses.empty())
        self.assertFalse(state.dialog_open)

        handle_key(state, adapter, _Event("c", ctrl=True))
        self.assertTrue(state.exit_requested)
        self.assertEqual(adapter.responses.get_nowait(), "__cancel__")

        state.apply(
            "folder_status_progress",
            {
                "processed": 100,
                "total": 100,
                "percent": 100.0,
                "albums": 200,
                "done": True,
            },
        )
        self.assertFalse(state.status_loading)


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
