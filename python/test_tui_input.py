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
    _musicbrainz_grouped_rows,
    TuiState,
    _clear_stale_remote_overlay,
    _draw_remote_hover_overlay,
    _embedded_compilation_selected,
    _preferred_candidate_index,
    _scan_controls,
    _status_control_semantic,
    _submit_library,
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
    def test_compilation_artwork_routes_automatically_and_requires_launch(self) -> None:
        state = _library_state(artists=1, albums_each=1)
        assert state.library is not None
        album = state.library.albums[0]
        album.selected = True
        self.assertEqual(len(_scan_controls(state)), 4)

        state.selected_album_stats = [
            {
                "path": album.path,
                "album_mbid_missing": True,
                "compilation": True,
                "manual_compilation_eligible": True,
            }
        ]
        self.assertEqual(len(_scan_controls(state)), 4)
        self.assertTrue(_embedded_compilation_selected(state))

        adapter = TuiAdapter()
        adapter.waiting.set()
        state.scan_index = 3
        _submit_library(state, adapter)
        self.assertEqual(state.scan_scope, "auto-selected")
        self.assertTrue(adapter.responses.empty())
        self.assertIn("LAUNCH", state.transient)

        state.scan_index = 0
        _submit_library(state, adapter)
        payload = json.loads(adapter.responses.get_nowait())
        self.assertEqual(payload["scan_mode"], "filtered-read")
        self.assertEqual(payload["selected"], [album.path])

    def test_library_hit_geometry_is_rebuilt_at_all_responsive_sizes(self) -> None:
        required = {
            "status-control",
            "select-control",
            "scan-control",
            "artist-row",
            "album-row",
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

    def test_artist_and_album_rows_are_the_only_selection_hit_surfaces(self) -> None:
        state = _library_state()
        render(_Frame(150, 44), state, select_theme("OLED"))
        targets = {item.target for item in state.hit_regions}
        self.assertIn("artist-row", targets)
        self.assertIn("album-row", targets)
        self.assertNotIn("artist-checkbox", targets)
        self.assertNotIn("album-checkbox", targets)


class ReportAndStatusAuthorityTests(unittest.TestCase):
    def test_manual_progress_marks_and_completes_album(self) -> None:
        state = _library_state(artists=1, albums_each=1)
        assert state.library is not None
        album = state.library.albums[0]
        state.album_path = album.path

        state.apply(
            "album_material_result",
            {
                "outcome": "Embedded Art Replaced",
                "manual_compilation": True,
                "progress_completed": 1,
                "progress_total": 100,
                "progress_status": "incomplete",
            },
        )
        self.assertIs(album.status, AlbumStatus.INCOMPLETE)

        state.apply(
            "album_material_result",
            {
                "outcome": "Embedded Art Replaced",
                "manual_compilation": True,
                "progress_completed": 100,
                "progress_total": 100,
                "progress_status": "complete",
            },
        )
        self.assertIs(album.status, AlbumStatus.PROCESSED)

    def test_resumed_manual_progress_marks_album_incomplete_without_new_write(self) -> None:
        state = _library_state(artists=1, albums_each=1)
        assert state.library is not None
        album = state.library.albums[0]
        state.active_report = AlbumRunReport(
            1,
            1,
            "Various Artists",
            "Billboard Hot 100 Singles of 1960",
            album.path,
            time.monotonic(),
        )
        state.apply(
            "album_progress",
            {
                "path": album.path,
                "completed": 19,
                "total": 100,
                "status": "incomplete",
            },
        )
        self.assertIs(album.status, AlbumStatus.INCOMPLETE)
        self.assertEqual(state.active_report.outcome, "Incomplete (19/100)")

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
                "Incomplete",
                "Processed",
                "Bypassed",
                "Partial / Timeout",
                "Artist Complete",
                "Artist Contains Bypass",
            ],
        )
        self.assertEqual(
            [_status_control_semantic(index) for index in range(7)],
            [
                Semantic.UNPROCESSED,
                Semantic.DEBUG,
                Semantic.FALLBACK,
                Semantic.REJECTED,
                Semantic.HISTORY,
                Semantic.ACCEPTED,
                Semantic.DEBUG,
            ],
        )
        album_counts = {
            AlbumStatus.UNPROCESSED: 11,
            AlbumStatus.INCOMPLETE: 4,
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
            [_folder_status_count(i, album_counts, artist_counts) for i in range(7)],
            [16, 4, 7, 2, 7, 6, 1],
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

    def test_manual_escape_aborts_album_without_cancelling_session(self) -> None:
        state = TuiState(started_at=time.monotonic() - 10, workflow="picker")
        state.input_request = InputRequest("Choice: ", "fallback-picker", {})
        state.fallback_artist_id = "291dcfb8-b31c-496a-905b-9955509d75b6"
        state.fallback_track_id = "59a0c68f-ec68-418d-a29a-fa54a7d9aea9"
        adapter = TuiAdapter()
        adapter.waiting.set()
        handle_key(state, adapter, _Event("Esc"))
        self.assertEqual(
            adapter.responses.get_nowait(),
            "__manual_album_exit__",
        )

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
            _center(_region(state, "album-row", 0)),
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

    def test_plain_artist_and_album_clicks_replace_selection(self) -> None:
        model = self.state.library
        assert model is not None

        artist_one = _region(self.state, "artist-row", 1)
        handle_mouse(self.state, self.adapter, _center(artist_one))
        self.assertEqual(model.active_artist, artist_one.value)
        self.assertTrue(
            any(
                item.selected
                for item in model.albums
                if item.artist == artist_one.value
            )
        )

        # A repeated plain Artist click remains a replacement selection.
        self.state.apply(
            "input",
            {"prompt": "", "context": {"kind": "library-selection"}},
        )
        self.adapter.waiting.set()
        render(_Frame(150, 44), self.state, select_theme("OLED"))
        artist_one = _region(self.state, "artist-row", 1)
        handle_mouse(self.state, self.adapter, _center(artist_one))
        self.assertTrue(
            any(
                item.selected
                for item in model.albums
                if item.artist == artist_one.value
            )
        )

        self.state.apply(
            "input",
            {"prompt": "", "context": {"kind": "library-selection"}},
        )
        self.adapter.waiting.set()
        render(_Frame(150, 44), self.state, select_theme("OLED"))
        album_row = _region(self.state, "album-row", 1)
        albums = model.visible_albums(active_artist_only=True)
        handle_mouse(self.state, self.adapter, _center(album_row))
        self.assertEqual(self.state.album_index_cursor, 1)
        self.assertTrue(albums[1].selected)
        self.assertEqual(sum(item.selected for item in model.albums), 1)

    def test_ctrl_click_toggles_multiple_albums(self) -> None:
        state = _library_state(artists=1, albums_each=4)
        adapter = TuiAdapter()
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))
        model = state.library
        assert model is not None

        handle_mouse(state, adapter, _center(_region(state, "album-row", 0)))
        adapter.responses.get_nowait()
        state.apply(
            "input",
            {"prompt": "", "context": {"kind": "library-selection"}},
        )
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))

        second = _center(_region(state, "album-row", 1))
        second.ctrl = True
        handle_mouse(state, adapter, second)
        self.assertEqual(
            [item.title for item in model.albums if item.selected],
            ["Love Among the Ruins", "Album 01"],
        )

        adapter.responses.get_nowait()
        state.apply(
            "input",
            {"prompt": "", "context": {"kind": "library-selection"}},
        )
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))
        first = _center(_region(state, "album-row", 0))
        first.ctrl = True
        handle_mouse(state, adapter, first)
        self.assertEqual(
            [item.title for item in model.albums if item.selected],
            ["Album 01"],
        )

    def test_ctrl_click_adds_artists_and_selects_non_orange_albums(self) -> None:
        payload = _payload(artists=2, albums_each=5)
        albums = payload["albums"]
        assert isinstance(albums, list)
        for item, status in zip(
            albums[:5],
            ("unprocessed", "incomplete", "processed", "bypassed", "timeout"),
            strict=True,
        ):
            assert isinstance(item, dict)
            item["status"] = status

        state = TuiState(started_at=time.monotonic() - 10)
        state.apply("library", payload)
        state.apply(
            "input",
            {"prompt": "", "context": {"kind": "library-selection"}},
        )
        adapter = TuiAdapter()
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))
        model = state.library
        assert model is not None

        first_artist = model.visible_artists()[0].name
        handle_mouse(state, adapter, _center(_region(state, "artist-row", 0)))
        self.assertEqual(
            [
                item.status
                for item in model.albums
                if item.artist == first_artist and item.selected
            ],
            [AlbumStatus.UNPROCESSED, AlbumStatus.INCOMPLETE],
        )

        adapter.responses.get_nowait()
        state.apply(
            "input",
            {"prompt": "", "context": {"kind": "library-selection"}},
        )
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))
        second_artist = model.visible_artists()[1].name
        second = _center(_region(state, "artist-row", 1))
        second.ctrl = True
        handle_mouse(state, adapter, second)

        self.assertTrue(
            any(item.selected for item in model.albums if item.artist == first_artist)
        )
        self.assertTrue(
            all(
                item.selected
                for item in model.albums
                if item.artist == second_artist and item.artist_selectable
            )
        )
        self.assertFalse(
            any(
                item.selected
                for item in model.albums
                if item.artist == first_artist
                and item.status
                in {
                    AlbumStatus.PROCESSED,
                    AlbumStatus.BYPASSED,
                    AlbumStatus.TIMEOUT,
                }
            )
        )

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
            _center(_region(state, "album-row", 0)),
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
        with mock.patch("tui.splined_tui._runtime_trace") as trace:
            handle_mouse(self.state, self.adapter, event)
            handle_mouse(self.state, self.adapter, event)
        self.assertEqual(trace.call_count, 1)
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
        self.assertTrue(request["replace_selection"])

        state.apply("input", {"prompt": "", "context": {"kind": "library-selection"}})
        adapter.waiting.set()
        state.library_focus = 3
        handle_key(state, adapter, _Event("Enter"))
        keyboard_request = json.loads(adapter.responses.get_nowait())
        self.assertEqual(keyboard_request["action"], "load-artist")
        self.assertFalse(keyboard_request["select_after_load"])
        self.assertFalse(keyboard_request["replace_selection"])

    def test_ctrl_click_unindexed_artist_requests_additive_load_and_select(self) -> None:
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
        state.apply(
            "input",
            {"prompt": "", "context": {"kind": "library-selection"}},
        )
        adapter = TuiAdapter()
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))
        event = _center(_region(state, "artist-row", 0))
        event.ctrl = True
        handle_mouse(
            state,
            adapter,
            event,
        )
        request = json.loads(adapter.responses.get_nowait())
        self.assertEqual(request["action"], "load-artist")
        self.assertTrue(request["select_after_load"])
        self.assertFalse(request["replace_selection"])

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
        for index in range(7):
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
            if index == 4:
                self.assertNotIn(AlbumStatus.TIMEOUT, model.status_filters)
                self.assertNotIn(
                    ArtistStatus.PARTIAL, model.artist_status_filters
                )

        # Bulk Select controls act immediately but delegate filesystem work to
        # the engine. Select NONE remains an in-memory operation.
        for index, expected_action in ((0, "select-all"), (2, "select-filtered")):
            state = _library_state()
            if index == 2:
                assert state.library is not None
                state.library.artist_filter = "artist"
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
                self.assertNotIn("status_filters", response)

        state = _library_state()
        none_model = state.library
        assert none_model is not None
        for item in none_model.albums:
            item.selected = True
        adapter = TuiAdapter()
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))
        handle_mouse(
            state,
            adapter,
            _center(_region(state, "select-control", 1)),
        )
        self.assertEqual(state.select_index, 1)
        self.assertFalse(any(item.selected for item in none_model.albums))
        response = json.loads(adapter.responses.get_nowait())
        self.assertEqual(response["action"], "selection-change")
        self.assertEqual(response["selected"], [])

        for index, mode in enumerate(("filtered-read", "filtered-write")):
            state = _library_state()
            assert state.library is not None
            state.library.albums[0].selected = True
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

        for index in (2, 3):
            state = _library_state()
            adapter = TuiAdapter()
            adapter.waiting.set()
            render(_Frame(150, 44), state, select_theme("OLED"))
            handle_mouse(
                state,
                adapter,
                _center(_region(state, "scan-control", index)),
            )
            self.assertTrue(adapter.responses.empty())
            self.assertIn("Select SPLINED LAUNCH", state.transient)

    def test_empty_filtered_launch_is_blocked(self) -> None:
        state = _library_state()
        state.library_focus = 2
        state.scan_index = 0
        adapter = TuiAdapter()
        adapter.waiting.set()

        handle_key(state, adapter, _Event("enter"))

        self.assertTrue(adapter.responses.empty())
        self.assertEqual(state.workflow, "library")
        self.assertIn("Select at least one Album", state.transient)

        assert state.library is not None
        state.library.albums[0].selected = True
        state.library_activate_guard_until = time.monotonic() + 1.0
        render(_Frame(150, 44), state, select_theme("OLED"))
        handle_mouse(
            state,
            adapter,
            _center(_region(state, "scan-control", 0)),
        )
        response = json.loads(adapter.responses.get_nowait())
        self.assertEqual(response["scan_mode"], "filtered-read")

    def test_filtered_scan_mouse_launch_keeps_multiple_artist_row_scope(self) -> None:
        state = _library_state(artists=3, albums_each=2)
        adapter = TuiAdapter()
        adapter.waiting.set()
        render(_Frame(150, 44), state, select_theme("OLED"))

        for index in (0, 1):
            event = _center(_region(state, "artist-row", index))
            event.ctrl = index > 0
            handle_mouse(
                state,
                adapter,
                event,
            )
            response = json.loads(adapter.responses.get_nowait())
            self.assertEqual(response["action"], "selection-change")
            state.apply(
                "input",
                {"prompt": "", "context": {"kind": "library-selection"}},
            )
            adapter.waiting.set()
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
            {"policy-source", "policy-field", "policy-global-field"}
            <= {item.target for item in state.hit_regions}
        )
        self.assertNotIn(
            "policy-source-enabled",
            {item.target for item in state.hit_regions},
        )

        source_row = _region(state, "policy-source", 0)
        source = source_row.value
        handle_mouse(state, adapter, _center(source_row))
        before = state.policy.policies[source]["enabled"]  # type: ignore[union-attr]
        render(_Frame(150, 44), state, select_theme("CHALK"))
        enabled_field = _region(state, "policy-field", 0)
        handle_mouse(state, adapter, _center(enabled_field))
        self.assertNotEqual(
            state.policy.policies[source]["enabled"], before  # type: ignore[union-attr]
        )

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
        theme = select_theme("OLED")
        render(_Frame(160, 44), state, theme)
        candidate = _region(state, "candidate-row")
        handle_mouse(state, adapter, _center(candidate))
        self.assertEqual(state.selected_index, candidate.index)

        url = _region(state, "candidate-url")
        stream = io.StringIO()
        write_terminal_links(state, stream, theme)
        encoded = stream.getvalue()
        self.assertIn(osc8_link("URL", url.value), encoded)
        foreground = theme.color("debug")
        background = theme.color("background")
        color_prefix = (
            f"\x1b[1;38;2;{foreground[0]};{foreground[1]};{foreground[2]};"
            f"48;2;{background[0]};{background[1]};{background[2]}m"
        )
        self.assertIn(
            f"\x1b[{url.y + 1};{url.x + 2}H"
            f"{color_prefix}{osc8_link('URL', url.value)}\x1b[0m",
            encoded,
        )
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

    def test_fallback_authority_ids_are_visible_terminal_links(self) -> None:
        state = self._candidate_state()
        state.apply(
            "fallback_authority",
            {
                "artist": "Percy Faith",
                "artist_id": "291dcfb8-b31c-496a-905b-9955509d75b6",
                "album": "A Summer Place",
                "album_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "track": "Theme From A Summer Place",
                "track_id": "59a0c68f-ec68-418d-a29a-fa54a7d9aea9",
            },
        )
        render(_Frame(220, 50), state, select_theme("OLED"))
        links = [
            region
            for region in state.hit_regions
            if region.target == "musicbrainz-link"
        ]
        self.assertEqual(len(links), 3)
        edit_boxes = [
            region
            for region in state.hit_regions
            if region.target == "fallback-id-edit"
        ]
        self.assertEqual([region.index for region in edit_boxes], [0, 1, 2])
        stream = io.StringIO()
        write_terminal_links(state, stream)
        encoded = stream.getvalue()
        for region in links:
            mbid = region.value.rsplit("/", 1)[-1]
            self.assertIn(osc8_link(mbid, region.value), encoded)

    def test_click_edit_recording_id_requeries_and_invalidates_release(self) -> None:
        state = self._candidate_state()
        state.apply(
            "fallback_authority",
            {
                "artist": "Percy Faith",
                "artist_id": "291dcfb8-b31c-496a-905b-9955509d75b6",
                "album": "A Summer Place",
                "album_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "track": "Theme From A Summer Place",
                "track_id": "59a0c68f-ec68-418d-a29a-fa54a7d9aea9",
            },
        )
        adapter = TuiAdapter()
        adapter.waiting.set()
        render(_Frame(220, 50), state, select_theme("OLED"))
        edit = _region(state, "fallback-id-edit", 2)
        handle_mouse(state, adapter, _center(edit))
        self.assertEqual(state.fallback_edit_index, 2)
        replacement = "11111111-2222-4333-8444-555555555555"
        handle_key(state, adapter, _Event(replacement))
        handle_key(state, adapter, _Event("Enter"))
        response = json.loads(adapter.responses.get_nowait())
        self.assertEqual(response["action"], "manual-authority-query")
        self.assertEqual(response["edited"], "recording")
        self.assertEqual(response["recording_id"], replacement)
        self.assertEqual(response["release_id"], "")

    def test_manual_m_opens_artist_track_discovery(self) -> None:
        state = self._candidate_state()
        state.fallback_artist_id = "291dcfb8-b31c-496a-905b-9955509d75b6"
        state.fallback_album_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
        state.fallback_track_id = "59a0c68f-ec68-418d-a29a-fa54a7d9aea9"
        adapter = TuiAdapter()
        adapter.waiting.set()
        handle_key(state, adapter, _Event("m"))
        self.assertEqual(adapter.responses.get_nowait(), "m")

    def test_manual_source_escape_returns_to_cached_musicbrainz_results(self) -> None:
        state = self._candidate_state()
        state.input_request = InputRequest(
            "Choice: ",
            "fallback-picker",
            {"manual_fallback": True, "musicbrainz_back": True},
        )
        adapter = TuiAdapter()
        adapter.waiting.set()
        handle_key(state, adapter, _Event("Esc"))
        self.assertEqual(adapter.responses.get_nowait(), "__manual_mb_results__")

    def test_folder_art_decision_uses_same_musicbrainz_picker_and_back_path(self) -> None:
        state = self._candidate_state()
        state.input_request = InputRequest(
            "Choice: ",
            "fallback-picker",
            {"musicbrainz": True},
        )
        adapter = TuiAdapter()
        adapter.waiting.set()
        handle_key(state, adapter, _Event("m"))
        self.assertEqual(adapter.responses.get_nowait(), "m")

        state.input_request = InputRequest(
            "MusicBrainz release #: ",
            "musicbrainz-results",
            {"options": [], "back_action": "source-results"},
        )
        adapter.waiting.set()
        handle_key(state, adapter, _Event("Esc"))
        self.assertEqual(adapter.responses.get_nowait(), "b")

    def test_manual_musicbrainz_results_render_inside_candidate_decision_and_click(self) -> None:
        state = self._candidate_state()
        state.apply(
            "input",
            {
                "prompt": "MusicBrainz release #: ",
                "context": {
                    "kind": "musicbrainz-results",
                    "options": [
                        {
                            "id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                            "artist": "Percy Faith",
                            "track": "Theme From A Summer Place",
                            "title": "A Summer Place",
                            "group": "Percy Faith · 1960s · Album",
                            "decade": "1960s",
                            "release_class": "Album",
                            "selection_state": "current",
                            "resolution": "3000x3000",
                            "artwork_url": "https://coverartarchive.org/release/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa/front",
                        },
                        {
                            "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                            "artist": "Percy Faith",
                            "track": "Theme From A Summer Place",
                            "title": "Greatest Hits",
                            "group": "Percy Faith · 1980s · Compilation",
                            "decade": "1980s",
                            "release_class": "Compilation",
                            "selection_state": "visited",
                            "resolution": "1200x1200",
                            "artwork_url": "https://coverartarchive.org/release/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb/front",
                        },
                    ],
                },
            },
        )
        adapter = TuiAdapter()
        adapter.waiting.set()
        render(_Frame(220, 50), state, select_theme("OLED"))
        second = _region(state, "musicbrainz-result-row", 1)
        artwork_url = _region(state, "musicbrainz-artwork-url", 1)
        preferred_url = _region(state, "candidate-url")
        self.assertTrue(artwork_url.value.endswith("/front"))
        # Paragraph and Table renderers have different content origins even
        # though their painted URL columns align on screen.
        self.assertEqual(artwork_url.x, preferred_url.x + 2)
        stream = io.StringIO()
        write_terminal_links(state, stream)
        encoded = stream.getvalue()
        self.assertIn(osc8_link("URL", artwork_url.value), encoded)
        self.assertIn(
            f"\x1b[{artwork_url.y + 1};{artwork_url.x + 2}H"
            f"{osc8_link('URL', artwork_url.value)}",
            encoded,
        )
        with mock.patch(
            "tui.splined_tui._start_remote_url_preview"
        ) as preview:
            handle_mouse(state, adapter, _center(artwork_url))
        preview.assert_called_once_with(
            state,
            adapter,
            artwork_url.index,
            artwork_url.value,
            hover_active=True,
        )
        handle_mouse(state, adapter, _center(second))
        self.assertEqual(state.selected_index, 1)
        handle_key(state, adapter, _Event("Enter"))
        self.assertEqual(adapter.responses.get_nowait(), "2")

    def test_panel_help_opens_the_selected_context_and_consumes_close(self) -> None:
        state = self._candidate_state()
        state.apply(
            "input",
            {
                "prompt": "MusicBrainz release #: ",
                "context": {
                    "kind": "musicbrainz-results",
                    "options": [
                        {
                            "id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                            "artist": "Percy Faith",
                            "title": "A Summer Place",
                            "decade": "1960s",
                            "release_class": "Album",
                            "artwork_url": "https://coverartarchive.org/release-group/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb/front",
                        }
                    ],
                },
            },
        )
        adapter = TuiAdapter()
        render(_Frame(220, 50), state, select_theme("OLED"))
        marker = next(
            region
            for region in state.hit_regions
            if region.target == "panel-help"
            and region.value == "musicbrainz-matches"
        )
        handle_mouse(state, adapter, _center(marker))
        self.assertTrue(state.help_open)
        self.assertEqual(state.help_topic, "musicbrainz-matches")

        render(_Frame(220, 50), state, select_theme("OLED"))
        handle_mouse(state, adapter, _center(_region(state, "help-close")))
        self.assertFalse(state.help_open)
        self.assertTrue(adapter.responses.empty())

        handle_key(state, adapter, _Event("?"))
        self.assertTrue(state.help_open)
        self.assertEqual(state.help_topic, "musicbrainz-matches")
        handle_key(state, adapter, _Event("Enter"))
        self.assertFalse(state.help_open)
        self.assertTrue(adapter.responses.empty())

    def test_musicbrainz_rows_group_newest_decade_and_base_release_types(self) -> None:
        rows = _musicbrainz_grouped_rows(
            [
                {"decade": "2000s", "release_class": "EP"},
                {"decade": "2010s", "release_class": "Album"},
                {"decade": "Unknown", "release_class": "Compilation"},
            ]
        )
        decades = [label for kind, _index, label in rows if kind == "decade"]
        self.assertEqual(decades, ["2010s", "2000s", "Unknown"])
        decade_positions = [
            index for index, row in enumerate(rows) if row[0] == "decade"
        ]
        first_decade_end = decade_positions[1]
        first_types = [
            label for kind, _index, label in rows[:first_decade_end] if kind == "type"
        ]
        self.assertEqual(first_types, ["Album"])
        self.assertEqual(
            sum(1 for kind, _index, _label in rows if kind == "blank"),
            3,
        )

    def test_empty_manual_result_keeps_authority_edit_controls_visible(self) -> None:
        state = self._candidate_state()
        state.candidates = []
        state.apply(
            "fallback_authority",
            {
                "artist": "Jimmy Clanton",
                "artist_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "album": "",
                "album_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                "track": "Go Jimmy Go",
                "track_id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
            },
        )
        state.apply(
            "diagnostics",
            {
                "items": [
                    [
                        "musicbrainz",
                        "Release is not an official Album, Soundtrack, or Compilation",
                    ]
                ]
            },
        )
        render(_Frame(220, 50), state, select_theme("OLED"))
        edit_boxes = [
            region
            for region in state.hit_regions
            if region.target == "fallback-id-edit"
        ]
        self.assertEqual([region.index for region in edit_boxes], [0, 1, 2])

        adapter = TuiAdapter()
        adapter.waiting.set()
        handle_key(state, adapter, _Event("Enter"))
        self.assertTrue(adapter.responses.empty())
        self.assertIn("No candidate is selectable", state.transient)

        handle_key(state, adapter, _Event("b"))
        self.assertEqual(adapter.responses.get_nowait(), "b")
        self.assertFalse(state.dialog_open)

    def test_manual_requery_input_clears_processing_transient(self) -> None:
        state = self._candidate_state()
        state.transient = "Updating MusicBrainz authority and artwork candidates…"
        state.apply(
            "input",
            {"prompt": "Choice: ", "context": {"kind": "fallback-picker"}},
        )
        self.assertEqual(state.workflow, "picker")
        self.assertEqual(state.transient, "")

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
    def test_url_hover_starts_live_preview_and_leave_restores_preferred(self) -> None:
        state = PolicyAndCandidateMouseTests()._candidate_state()
        adapter = TuiAdapter()
        render(_Frame(160, 44), state, select_theme("OLED"))
        urls = [
            item for item in state.hit_regions
            if item.target == "candidate-url"
        ]
        alternate = next(item for item in urls if item.index != _preferred_candidate_index(state))
        moved = _Event(
            "moved",
            kind="mouse",
            button="none",
            column=alternate.x + 1,
            row=alternate.y,
        )
        with mock.patch(
            "tui.splined_tui._start_remote_hover_preview"
        ) as start:
            handle_mouse(state, adapter, moved)
        start.assert_called_once_with(state, adapter, alternate.index)

        state.remote_hover_index = alternate.index
        state.remote_hover_url = alternate.value
        state.remote_hover_active = True
        state.remote_hover_loading = True
        with mock.patch(
            "tui.splined_tui._start_remote_source_preview"
        ) as restore:
            handle_mouse(
                state,
                adapter,
                _Event("moved", kind="mouse", button="none", column=0, row=0),
            )
        restore.assert_called_once()
        self.assertFalse(state.remote_hover_active)

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


class NativeOverlayLifecycleTests(unittest.TestCase):
    def test_native_overlay_is_not_drawn_outside_main_candidate_view(self) -> None:
        class Overlay:
            def __init__(self) -> None:
                self.calls: list[tuple[int, int]] = []

            def draw(self, x: int, y: int) -> None:
                self.calls.append((x, y))

        state = TuiState(started_at=time.monotonic() - 1)
        overlay = Overlay()
        state.workflow = "picker"
        state.tab = "main"
        state.remote_hover_index = 0
        state.remote_hover_overlay = overlay
        state.remote_preview_rect = (10, 5, 20, 10)

        _draw_remote_hover_overlay(state)
        self.assertEqual(overlay.calls, [(10, 5)])

        state.remote_drawn_rect = (10, 5, 20, 10)
        state.tab = "logs"
        with mock.patch(
            "tui.splined_tui.native_clear_image_area"
        ) as clear:
            _clear_stale_remote_overlay(state)
        clear.assert_called_once_with(10, 5, 20, 10)
        self.assertIsNone(state.remote_drawn_rect)
        _draw_remote_hover_overlay(state)
        self.assertEqual(overlay.calls, [(10, 5)])

        state.tab = "main"
        state.workflow = "batch-report"
        _draw_remote_hover_overlay(state)
        self.assertEqual(overlay.calls, [(10, 5)])

    def test_tab_to_logs_clears_remote_hover_state(self) -> None:
        state = TuiState(started_at=time.monotonic() - 1)
        state.workflow = "picker"
        state.tab = "main"
        state.remote_hover_index = 2
        state.remote_hover_url = "https://example.test/image.jpg"
        state.remote_hover_overlay = object()
        state.remote_preview_rect = (10, 5, 20, 10)
        state.input_request = InputRequest(
            "",
            "local-comparison",
            {},
        )
        adapter = TuiAdapter()

        handle_key(state, adapter, _Event("tab"))

        self.assertEqual(state.tab, "history")
        self.assertEqual(state.remote_hover_index, -1)
        self.assertEqual(state.remote_hover_url, "")
        self.assertIsNone(state.remote_hover_overlay)
        self.assertIsNone(state.remote_preview_rect)


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
