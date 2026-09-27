from __future__ import annotations

import contextlib
import io
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

from PIL import Image
from pyratatui import Rect

import splined
import splined_scan
from tui.aispline import AiCandidate, EnhancementSelection
from tui.layout import candidate_column_layout
from tui.preview import generate_preview
from tui.splined_tui import (
    CANDIDATE_HEADERS,
    CandidateView,
    TuiState,
    _candidate_preview,
    _candidate_value,
    _render_candidates,
)
from tui.theme import select_theme


class Frame:
    def __init__(self, width: int, height: int) -> None:
        self.area = Rect(0, 0, width, height)

    def render_widget(self, _widget, _area) -> None:
        pass

    def render_stateful_table(self, _widget, _area, _state) -> None:
        pass


def candidate(
    number: int,
    source: str,
    resolution: tuple[int, int],
    range_type: str,
    *,
    suggested: bool = False,
    provenance: str = "[URL]",
    path: str = "",
) -> CandidateView:
    return CandidateView(
        number,
        source,
        resolution[0],
        resolution[1],
        "jpeg",
        range_type,
        abs(1800 - min(resolution)),
        resolution[0] == resolution[1],
        min(resolution) >= 1200,
        True,
        f"internal-{number}",
        suggested=suggested,
        url=f"https://example.test/{number}.jpg" if provenance == "[URL]" else "",
        provenance=provenance,
        ai_review=True,
        ai_key=str(number),
        path=path,
    )


class CandidateGridTests(unittest.TestCase):
    def test_ai_columns_are_exact_and_disappear_when_disabled(self) -> None:
        disabled = candidate_column_layout(158, 44, ai_enabled=False)
        enabled = candidate_column_layout(158, 44, ai_enabled=True)
        self.assertNotIn("ai_enhanced", disabled.columns)
        self.assertNotIn("ai_splined", disabled.columns)
        self.assertEqual(
            enabled.columns[:3],
            ("#", "ai_enhanced", "ai_splined"),
        )
        self.assertEqual(CANDIDATE_HEADERS["ai_enhanced"], "AI ENHANCED")
        self.assertEqual(CANDIDATE_HEADERS["ai_splined"], "AI SPLINED")
        self.assertNotIn("id", enabled.columns)

    def test_shared_terminal_cell_starts_are_monotonic_at_all_widths(self) -> None:
        for width in (160, 119, 79, 48):
            for enabled in (False, True):
                with self.subTest(width=width, ai=enabled):
                    grid = candidate_column_layout(width, 40, ai_enabled=enabled)
                    self.assertEqual(grid.starts[0], 0)
                    for index in range(1, len(grid.columns)):
                        self.assertEqual(
                            grid.starts[index],
                            grid.starts[index - 1] + grid.widths[index - 1] + 1,
                        )
                    self.assertLessEqual(
                        grid.starts[-1] + grid.widths[-1],
                        max(width - 2, 1),
                    )

    def test_unicode_values_do_not_control_column_geometry(self) -> None:
        state = TuiState()
        state.candidates = [
            candidate(1, "Local", (34, 34), "BelowMinimum", provenance="[LOCAL]"),
            candidate(2, "Cover Art Archive", (600, 514), "BelowMinimum"),
            candidate(3, "iTunes", (1500, 1500), "LowerRange"),
            candidate(4, "Fanart.tv", (3600, 3600), "AboveLadder"),
        ]
        state.selected_index = 2
        grid = candidate_column_layout(158, 44, ai_enabled=False)
        starts = tuple(grid.start(column) for column in grid.columns)
        for item in state.candidates:
            values = [_candidate_value(item, column, state) for column in grid.columns]
            self.assertEqual(tuple(grid.start(column) for column in grid.columns), starts)
            self.assertIn(values[grid.columns.index("url")], {"[LOCAL]", "[URL]"})
        self.assertEqual(_candidate_value(state.candidates[2], "#", state), "›3")
        self.assertEqual(_candidate_value(state.candidates[0], "square", state), "✓ yes")

    def test_every_rendered_group_reuses_one_grid_and_url_x(self) -> None:
        state = TuiState(started_at=time.monotonic() - 10, workflow="candidates")
        state.candidates = [
            candidate(1, "Local", (1500, 1500), "LowerRange", provenance="[LOCAL]"),
            candidate(2, "iTunes", (1800, 1800), "Ideal", suggested=True),
            candidate(3, "LastFM", (34, 34), "BelowMinimum"),
            candidate(4, "Discogs", (600, 514), "BelowMinimum"),
        ]
        seen = []
        from tui import splined_tui

        original = splined_tui._render_candidate_table_group

        def recording(*args, **kwargs):
            seen.append(args[4])
            return original(*args, **kwargs)

        with mock.patch.object(
            splined_tui,
            "_render_candidate_table_group",
            side_effect=recording,
        ):
            frame = Frame(160, 50)
            _render_candidates(frame, frame.area, state, select_theme("OLED"))
        self.assertGreaterEqual(len(seen), 2)
        self.assertTrue(all(value == seen[0] for value in seen))
        url_regions = [region for region in state.hit_regions if region.target == "candidate-url"]
        self.assertGreaterEqual(len(url_regions), 2)
        self.assertEqual(len({region.x for region in url_regions}), 1)


class ArtworkPreviewTests(unittest.TestCase):
    def test_preview_uses_existing_artwork_and_quadrant_cells(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "cover.png"
            Image.new("RGB", (20, 20), (12, 34, 56)).save(path)
            preview = generate_preview(path, width=8, height=4)
        self.assertIsNotNone(preview)
        assert preview is not None
        self.assertEqual((preview.width, preview.height), (8, 4))
        self.assertEqual(preview.rows[0][0].foreground, (12, 34, 56))
        self.assertEqual(preview.rows[0][0].background, (12, 34, 56))

    def test_preview_retains_two_horizontal_samples_per_terminal_cell(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "split.png"
            image = Image.new("RGB", (2, 4), "white")
            for y in range(4):
                image.putpixel((0, y), (0, 0, 0))
            image.save(path)
            preview = generate_preview(path, width=1, height=1)
        self.assertIsNotNone(preview)
        assert preview is not None
        cell = preview.rows[0][0]
        self.assertEqual(cell.glyph, "▌")
        self.assertEqual({cell.foreground, cell.background}, {(0, 0, 0), (255, 255, 255)})

    def test_preview_is_cached_and_corrupt_art_is_nonfatal(self) -> None:
        state = TuiState()
        item = candidate(1, "iTunes", (1500, 1500), "LowerRange", path="/already/acquired.jpg")
        sentinel = mock.sentinel.preview
        with mock.patch("tui.splined_tui.generate_preview", return_value=sentinel) as generated:
            self.assertIs(_candidate_preview(state, item), sentinel)
            self.assertIs(_candidate_preview(state, item), sentinel)
        generated.assert_called_once_with(item.path, width=28, height=12)
        with tempfile.TemporaryDirectory() as directory:
            corrupt = Path(directory) / "broken.jpg"
            corrupt.write_bytes(b"not artwork")
            self.assertIsNone(generate_preview(corrupt))

    def test_preview_preserves_aspect_ratio_with_centered_letterboxing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "wide.png"
            Image.new("RGB", (600, 96), (200, 10, 20)).save(path)
            preview = generate_preview(path, width=20, height=10)
        self.assertIsNotNone(preview)
        assert preview is not None
        self.assertEqual((preview.width, preview.height), (20, 10))
        self.assertEqual(preview.rows[0][0].background, (0, 0, 0))
        center = preview.rows[5][10]
        self.assertIn((200, 10, 20), {center.foreground, center.background})

    def test_wide_renders_previews_and_compact_omits_them(self) -> None:
        state = TuiState(started_at=time.monotonic() - 10, workflow="candidates")
        state.candidates = [candidate(1, "iTunes", (1800, 1800), "Ideal", suggested=True)]
        with mock.patch("tui.splined_tui._render_candidate_preview") as rendered:
            frame = Frame(160, 44)
            _render_candidates(frame, frame.area, state, select_theme("OLED"))
            self.assertEqual(rendered.call_count, 1)
            self.assertGreaterEqual(int(rendered.call_args.args[1].width), 24)
            self.assertGreaterEqual(int(rendered.call_args.args[1].height), 10)
        state.hit_regions.clear()
        with mock.patch("tui.splined_tui._render_candidate_preview") as rendered:
            frame = Frame(78, 40)
            _render_candidates(frame, frame.area, state, select_theme("OLED"))
            rendered.assert_not_called()


class BrowserUrlProjectionTests(unittest.TestCase):
    def test_candidate_payload_prefers_resolved_url_and_falls_back_to_fetch_url(self) -> None:
        resolved = splined.Ref(
            "itunes",
            "resolved",
            "https://example.test/discovery-thumbnail.jpg",
            browser_url="https://cdn.example.test/resolved-3000.jpg",
        )
        fallback = splined.Ref(
            "itunes",
            "fallback",
            "https://example.test/original-3000.jpg",
        )
        candidates = [
            splined.Candidate(resolved, Path("resolved.jpg"), 3000, 3000, "jpeg", 0),
            splined.Candidate(fallback, Path("fallback.jpg"), 3000, 3000, "jpeg", 0),
        ]
        projected = {
            "range_type": "AboveLadder",
            "distance": 1200,
            "square": True,
            "acceptable": True,
        }
        with (
            mock.patch.object(splined_scan, "project_candidate", return_value=projected),
            mock.patch.object(splined_scan.core, "emit_ui") as emitted,
            contextlib.redirect_stdout(io.StringIO()),
        ):
            splined_scan.render_candidate_table(candidates, {}, ["jpeg"])

        items = emitted.call_args.kwargs["items"]
        self.assertEqual(items[0]["url"], resolved.browser_url)
        self.assertEqual(items[1]["url"], fallback.url)
        self.assertEqual(resolved.url, "https://example.test/discovery-thumbnail.jpg")

    def test_itunes_artwork_url_retains_high_resolution_suffix(self) -> None:
        thumbnail = (
            "https://is1-ssl.mzstatic.com/image/thumb/Music211/v4/aa/bb/cc/"
            "100x100bb.jpg"
        )
        high_resolution = splined.itunes_artwork_url(thumbnail, 3000)
        self.assertIn("/3000x3000bb.jpg", high_resolution)
        self.assertNotIn("/100x100bb.jpg", high_resolution)


if __name__ == "__main__":
    unittest.main()
