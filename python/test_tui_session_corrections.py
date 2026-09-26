from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from time import monotonic
from unittest import mock

import splined
import splined_scan
from tui import animation
from tui import splined_tui as tui_module
from tui.aispline import AiActivityState, AiPhase
from tui.library import (
    AlbumItem,
    AlbumStatus,
    ArtistItem,
    ArtistStatus,
    LibraryModel,
)
from tui.splined_tui import (
    TuiAdapter,
    TuiState,
    ai_context_active,
    context_header_height,
    handle_key,
    render,
    startup_brand_height,
)
from tui.theme import select_theme
from pyratatui import Rect


class _Key:
    def __init__(self, code: str, *, ctrl: bool = False, shift: bool = False):
        self.code = code
        self.ctrl = ctrl
        self.shift = shift


class _Frame:
    def __init__(self, width: int, height: int):
        self.area = Rect(0, 0, width, height)
        self.areas: list[Rect] = []
        self.widgets: list[object] = []

    def render_widget(self, widget, area):
        self.widgets.append(widget)
        self.areas.append(area)

    def render_stateful_table(self, widget, area, _state):
        self.widgets.append(widget)
        self.areas.append(area)


def _library_payload(status: str = "unprocessed") -> dict[str, object]:
    return {
        "root": "/music",
        "artists": [
            {
                "path": "/music/Artist A",
                "name": "Artist A",
                "indexed": True,
                "loaded": True,
                "album_count": 1,
            }
        ],
        "albums": [
            {
                "path": "/music/Artist A/Album A",
                "artist": "Artist A",
                "album": "Album A",
                "status": status,
                "selected": status == "unprocessed",
            }
        ],
    }


class ReadinessBrandTests(unittest.TestCase):
    def test_startup_animation_has_no_expiration_and_tracks_readiness(self) -> None:
        self.assertFalse(hasattr(animation, "STARTUP_SECONDS"))
        self.assertEqual(animation.startup_frame(0).phrase, animation.EXPANSION)
        self.assertEqual(
            animation.startup_frame(animation.BRAND_ANIMATION_PERIOD / 2).phrase,
            animation.STYLIZED_EXPANSION,
        )
        self.assertFalse(animation.startup_frame(10_000).complete)

        state = TuiState(started_at=monotonic() - 10_000)
        state.apply(
            "inventory_state",
            {
                "library_root": "/music",
                "picker_index": "",
                "status": "Artist folders ready",
                "root_artists": 1033,
                "cached_artists": 0,
                "indexed_artists": 0,
                "albums_known": 0,
            },
        )
        self.assertEqual(state.workflow, "startup")
        frame = _Frame(140, 40)
        render(frame, state, select_theme("OLED"))
        brand_areas = [
            area
            for area in frame.areas
            if int(area.y) == 0
            and int(area.height) == startup_brand_height(140, 40)
        ]
        self.assertEqual(len(brand_areas), 1)
        self.assertFalse(
            any(type(widget).__name__ == "Gauge" for widget in frame.widgets)
        )

        state.apply("library", _library_payload())
        self.assertEqual(state.workflow, "library")

    def test_brand_geometry_is_responsive_and_ai_context_is_truthful(self) -> None:
        self.assertFalse(hasattr(tui_module, "_BRAND_GLYPHS"))
        self.assertEqual(
            len(tui_module._solid_spectral_brand(select_theme("OLED"), "S:P:L:I:N:E:D")),
            3,
        )
        self.assertGreater(startup_brand_height(140, 40), context_header_height(140, 40))
        self.assertGreater(context_header_height(140, 40), context_header_height(60, 18))
        self.assertGreater(context_header_height(100, 30), context_header_height(60, 18))

        state = TuiState(workflow="processing", ai_enabled=True)
        self.assertFalse(ai_context_active(state))
        state.ai_activity = AiActivityState(True, AiPhase.ASSESSING, "real event")
        self.assertTrue(ai_context_active(state))
        state.ai_activity.active = False
        self.assertFalse(ai_context_active(state))

        for workflow in ("processing", "candidates", "batch-report"):
            state.workflow = workflow
            state.candidates = []
            frame = _Frame(140, 40)
            render(frame, state, select_theme("CHALK"))
            # Background + a distinct responsive context band + content/footer.
            self.assertTrue(
                any(int(area.height) == context_header_height(140, 40) for area in frame.areas)
            )


class CountScopeTests(unittest.TestCase):
    def test_loaded_scope_counts_are_explicit_in_lazy_inventory(self) -> None:
        albums = [
            *[
                AlbumItem(f"/A/U{index}", "Artist A", f"A U{index}", AlbumStatus.UNPROCESSED)
                for index in range(10)
            ],
            *[
                AlbumItem(f"/A/P{index}", "Artist A", f"A P{index}", AlbumStatus.PROCESSED)
                for index in range(2)
            ],
            *[
                AlbumItem(f"/B/U{index}", "Artist B", f"B U{index}", AlbumStatus.UNPROCESSED)
                for index in range(3)
            ],
            *[
                AlbumItem(f"/B/P{index}", "Artist B", f"B P{index}", AlbumStatus.PROCESSED)
                for index in range(5)
            ],
            AlbumItem("/C/Bypass", "Artist C", "C Bypass", AlbumStatus.BYPASSED),
        ]
        artists = [
            ArtistItem("/A", "Artist A", ArtistStatus.PARTIAL, 12, 0, True, True),
            ArtistItem("/B", "Artist B", ArtistStatus.PARTIAL, 8, 0, True, True),
            ArtistItem("/C", "Artist C", ArtistStatus.CONTAINS_BYPASS, 1, 0, True, True),
        ]
        model = LibraryModel("/music", albums, artists, active_artist="Artist A")
        counts = model.album_status_counts()
        self.assertEqual(counts[AlbumStatus.UNPROCESSED], 13)
        self.assertEqual(counts[AlbumStatus.PROCESSED], 7)
        self.assertEqual(counts[AlbumStatus.BYPASSED], 1)
        self.assertEqual(sum(counts.values()), 21)
        stats = model.statistics()
        self.assertEqual(stats["artists"], 3)
        self.assertEqual(stats["albums"], 21)
        self.assertEqual(stats["inventory"], "DIRECT / LAZY")
        self.assertEqual(stats["loaded_artists"], 3)
        self.assertEqual(stats["active_albums"], 12)
        self.assertEqual(stats["active_visible_albums"], 12)
        self.assertEqual(
            model.indexed_artist_status_counts()[ArtistStatus.PARTIAL],
            2,
        )

        model.album_filter = "U1"
        model.select_all(filtered=True)
        self.assertTrue(any(item.selected for item in albums if item.artist == "Artist A"))
        self.assertTrue(any(item.selected for item in albums if item.artist == "Artist B"))
        self.assertFalse(any(item.selected for item in albums if item.artist == "Artist C"))


class HiddenDirectoryTests(unittest.TestCase):
    def test_dot_directories_never_enter_direct_root_or_nested_inventory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "music"
            for artist in (
                ".animatedartworkdownloader",
                ".nomifo",
                ".hidden",
                "10,000 Maniacs",
                "Aerosmith",
                "[videos]",
            ):
                album = root / artist / "Album"
                album.mkdir(parents=True)
                (album / "track.mp3").write_bytes(b"audio")
            nested = root / "10,000 Maniacs" / ".private" / "Hidden Album"
            nested.mkdir(parents=True)
            (nested / "track.flac").write_bytes(b"audio")

            with os.scandir(root) as iterator:
                names = sorted(
                    entry.name
                    for entry in iterator
                    if not entry.is_symlink()
                    and entry.is_dir(follow_symlinks=False)
                    and not splined.picker_should_ignore(entry.name, ["[videos]"])
                )
            self.assertEqual(names, ["10,000 Maniacs", "Aerosmith"])

            albums, ignored = splined.inventory(root, ["[videos]"])
            self.assertEqual(
                {album.path.parent.name for album in albums},
                {"10,000 Maniacs", "Aerosmith"},
            )
            self.assertTrue(any(path.name == ".private" for path in ignored))


class MultiBatchSessionTests(unittest.TestCase):
    def test_scan_session_runs_multiple_batches_with_one_picker_state(self) -> None:
        calls: list[tuple[object, str]] = []

        def batch(*_args, **kwargs):
            calls.append((kwargs["picker_session"], kwargs["initial_library_event"]))
            return (0, 1, 0)[len(calls) - 1]

        with (
            mock.patch.object(splined_scan.core, "tui_active", return_value=True),
            mock.patch.object(splined_scan, "_run_scan_dir_batch", side_effect=batch),
            mock.patch.object(
                splined_scan.core,
                "read_input",
                side_effect=["continue", "continue", "exit"],
            ),
        ):
            result = splined_scan.run_scan_dir(Path("config.toml"), {}, ["itunes"])
        self.assertEqual(result, 1)
        self.assertEqual([event for _session, event in calls], ["library", "library_update", "library_update"])
        self.assertIs(calls[0][0], calls[1][0])
        self.assertIs(calls[1][0], calls[2][0])

    def test_windows_style_report_returns_to_preserved_library_state(self) -> None:
        state = TuiState()
        state.apply("library", _library_payload())
        assert state.library is not None
        state.library.artist_filter = "artist"
        state.library.album_filter = "album"
        state.library.active_artist = "Artist A"
        state.apply(
            "album",
            {
                "index": 1,
                "total": 1,
                "path": "/music/Artist A/Album A",
                "artist": "Artist A",
                "album": "Album A",
                "phase": "processing",
            },
        )
        state.apply(
            "candidates",
            {
                "items": [
                    {
                        "number": 1,
                        "source": "iTunes",
                        "width": 1800,
                        "height": 1800,
                        "format": "jpeg",
                        "range_type": "Ideal",
                        "distance": 0,
                        "square": True,
                        "acceptable": True,
                        "approved": True,
                        "id": "internal",
                        "suggested": True,
                        "url": "https://example.test/cover.jpg",
                    }
                ]
            },
        )
        state.apply(
            "album_material_result",
            {
                "outcome": "Installed",
                "destination": "/music/Artist A/Album A/cover.jpg",
                "file_action": "Written",
                "source": "iTunes",
            },
        )
        state.apply("history", {"album": "Artist A / Album A", "outcome": "selected"})
        state.apply("summary", {"albums": 1, "resolved": 1, "failed": 0, "exit_code": 0})
        state.apply("input", {"prompt": "", "context": {"kind": "batch-summary"}})
        self.assertEqual(state.workflow, "batch-report")
        self.assertEqual(len(state.batch_reports), 1)
        self.assertEqual(state.batch_reports[0].outcome, "Installed")
        self.assertEqual(state.batch_reports[0].file_action, "Written")
        adapter = TuiAdapter()
        adapter.waiting.set()
        handle_key(state, adapter, _Key("enter"))
        self.assertEqual(adapter.responses.get_nowait(), "continue")
        state.apply("library_update", _library_payload("processed"))
        assert state.library is not None
        self.assertEqual(state.workflow, "library")
        self.assertEqual(state.library.artist_filter, "artist")
        self.assertEqual(state.library.album_filter, "album")
        self.assertEqual(state.library.active_artist, "Artist A")
        self.assertEqual(state.library.albums[0].status, AlbumStatus.PROCESSED)
        self.assertFalse(state.library.albums[0].selected)
        self.assertEqual(len(state.history), 1)

    def test_report_preserves_processing_order_and_authoritative_detail_classes(self) -> None:
        state = TuiState()
        state.apply("scan_start", {"total": 3, "root": "/music"})
        outcomes = (
            ("Artist A", "Album A", "Installed", "Written", "iTunes"),
            ("Artist B", "Album B", "Unchanged", "Retained", "Local"),
            ("Artist C", "Album C", "Failed", "Failed", "Discogs"),
        )
        for index, (artist, album, outcome, action, source) in enumerate(outcomes, 1):
            path = f"/music/{artist}/{album}"
            state.apply(
                "album",
                {
                    "index": index,
                    "total": 3,
                    "path": path,
                    "artist": artist,
                    "album": album,
                    "authority": "ExactAlbumId",
                    "phase": "processing",
                },
            )
            state.apply(
                "candidates",
                {
                    "hidden_by_source_policy": index,
                    "items": [
                        {
                            "number": 1,
                            "source": source,
                            "width": 1800,
                            "height": 1800,
                            "format": "jpeg",
                            "range_type": "Ideal",
                            "distance": 0,
                            "square": True,
                            "acceptable": outcome != "Failed",
                            "approved": True,
                            "id": f"internal-{index}",
                            "selected": True,
                        }
                    ],
                },
            )
            state.apply(
                "diagnostics",
                {"items": [[source, f"provider note {index}"]]},
            )
            state.apply(
                "album_material_result",
                {
                    "outcome": outcome,
                    "destination": f"{path}/cover.jpg" if outcome != "Failed" else "",
                    "file_action": action,
                    "source": source,
                    "width": 1800,
                    "height": 1800,
                    "format": "jpeg",
                    "range_type": "Ideal",
                    "distance": 0,
                    "detail": "No acceptable candidate" if outcome == "Failed" else "",
                },
            )
        state.apply("summary", {"albums": 3, "failed": 1, "exit_code": 1})
        state.apply("input", {"prompt": "", "context": {"kind": "batch-summary"}})
        self.assertEqual(state.workflow, "batch-report")
        self.assertEqual(
            [(item.position, item.artist, item.album) for item in state.batch_reports],
            [(1, "Artist A", "Album A"), (2, "Artist B", "Album B"), (3, "Artist C", "Album C")],
        )
        self.assertEqual([item.outcome for item in state.batch_reports], ["Installed", "Unchanged", "Failed"])
        self.assertEqual([item.file_action for item in state.batch_reports], ["Written", "Retained", "Failed"])
        self.assertTrue(all(item.candidate_total >= 1 for item in state.batch_reports))
        self.assertTrue(all(item.provider_notes for item in state.batch_reports))
        frame = _Frame(140, 40)
        render(frame, state, select_theme("OLED"))
        self.assertTrue(any(region.target == "report-scroll" for region in state.hit_regions))

    def test_report_pauses_scrolls_and_exit_waits_for_worker_result(self) -> None:
        state = TuiState(workflow="batch-report")
        state.apply("input", {"prompt": "", "context": {"kind": "batch-summary"}})
        adapter = TuiAdapter()
        adapter.waiting.set()
        handle_key(state, adapter, _Key("pagedown"))
        self.assertGreater(state.report_scroll, 0)
        self.assertTrue(adapter.responses.empty())
        handle_key(state, adapter, _Key("q"))
        self.assertEqual(adapter.responses.get_nowait(), "exit")
        self.assertFalse(state.exit_requested)
        self.assertTrue(state.exit_after_worker)
        state.apply("worker_done", {"exit_code": 1, "exception": None})
        self.assertTrue(state.exit_requested)
        self.assertEqual(state.exit_code, 1)


if __name__ == "__main__":
    unittest.main()
