"""Restore the compact multicolor bar for SPLINED database builds.

During Album-folder discovery there is no honest denominator, so the banner
uses a moving red→orange→yellow→green activity pulse and omits a misleading
0.0% label. Once discovery completes, the same rail becomes a determinate
multicolor bar; the existing ``Albums x / y`` line is the numeric progress
indicator.
"""

from __future__ import annotations

from pathlib import Path
import sqlite3
import time
from typing import Any

import splined_media_build_policy as build_policy


_INSTALLED = False
_STAGE_TYPE = "media-index-stage"


def _saved_checkpoint_count(database_path: str) -> int:
    """Return durable staged Album rows without blocking startup on failure."""
    path = Path(str(database_path).strip())
    if not path.is_file():
        return 0
    connection: sqlite3.Connection | None = None
    try:
        connection = sqlite3.connect(
            f"file:{path}?mode=ro",
            uri=True,
            timeout=1.0,
        )
        row = connection.execute(
            "SELECT COUNT(*) FROM cache_entries WHERE cache_type=?",
            (_STAGE_TYPE,),
        ).fetchone()
        return int(row[0] or 0) if row is not None else 0
    except (OSError, sqlite3.Error, TypeError, ValueError):
        return 0
    finally:
        if connection is not None:
            connection.close()


def _patch_build_presentation(module: Any) -> None:
    if getattr(module, "_splined_database_gradient_progress", False):
        return

    original_apply = module.TuiState.apply
    original_render = module._render_startup

    def apply(state: Any, event: str, payload: dict[str, Any]) -> None:
        original_apply(state, event, payload)
        if event == "cache_build_start":
            saved = _saved_checkpoint_count(
                str(payload.get("database", ""))
            )
            state.cache_saved_before_inventory = saved
            state.cache_staged = max(
                int(getattr(state, "cache_staged", 0) or 0),
                saved,
            )
        elif event == "cache_progress":
            saved = int(
                getattr(state, "cache_saved_before_inventory", 0) or 0
            )
            # Discovery reports staged=0 because no new tag rows are created in
            # that phase. Preserve checkpoints from the interrupted prior run.
            state.cache_staged = max(
                int(getattr(state, "cache_staged", 0) or 0),
                saved,
            )

    def render_startup(frame: Any, state: Any, theme: Any) -> None:
        original_render(frame, state, theme)
        if getattr(state, "cache_kind", "") != "media-index":
            return

        area = frame.area
        brand_height = min(
            int(area.height),
            module.startup_brand_height(area.width, area.height),
        )
        _brand, inventory_area = module._split_vertical(
            area,
            [module.Constraint.length(brand_height), module.Constraint.fill(1)],
        )
        panel_width = min(max(60, int(inventory_area.width) - 8), 96)
        panel_height = min(13, int(inventory_area.height))
        panel = module.Rect(
            int(inventory_area.x)
            + max(0, (int(inventory_area.width) - panel_width) // 2),
            int(inventory_area.y)
            + max(0, (int(inventory_area.height) - panel_height) // 2),
            panel_width,
            panel_height,
        )
        inner = module.Rect(
            int(panel.x) + 2,
            int(panel.y) + 1,
            max(1, int(panel.width) - 4),
            max(1, int(panel.height) - 2),
        )

        # Preserve the database banner exactly. Replace only the three rows
        # below the Album-count line, removing the redundant percentage row.
        progress_area = module.Rect(
            int(inner.x),
            int(inner.y) + 3,
            int(inner.width),
            min(3, max(1, int(inner.height) - 3)),
        )

        total = int(getattr(state, "cache_total", 0) or 0)
        percent = float(getattr(state, "cache_percent", 0.0) or 0.0)
        ratio = max(0.0, min(1.0, percent / 100.0))
        bar_width = min(64, max(24, int(progress_area.width) - 10))

        bar_spans: list[Any] = [
            module.Span("[", module.style(theme, module.Semantic.MUTED))
        ]
        if total > 0:
            filled = min(bar_width, round(bar_width * ratio))
            for index in range(bar_width):
                if index < filled:
                    red, green, blue = module._status_gradient_rgb(
                        index / max(1, bar_width - 1)
                    )
                    bar_spans.append(
                        module.Span(
                            "█",
                            module.Style().fg(
                                module.Color.rgb(red, green, blue)
                            ),
                        )
                    )
                else:
                    bar_spans.append(
                        module.Span(
                            "░",
                            module.style(theme, module.Semantic.MUTED),
                        )
                    )
        else:
            # No total exists during topology discovery. Animate a compact
            # spectral pulse instead of presenting a frozen zero-percent bar.
            pulse_width = max(8, min(16, bar_width // 4))
            elapsed = max(
                0.0,
                time.monotonic()
                - float(getattr(state, "started_at", time.monotonic())),
            )
            head = (
                int(elapsed * 12.0) % (bar_width + pulse_width)
            ) - pulse_width
            for index in range(bar_width):
                offset = index - head
                if 0 <= offset < pulse_width:
                    red, green, blue = module._status_gradient_rgb(
                        offset / max(1, pulse_width - 1)
                    )
                    bar_spans.append(
                        module.Span(
                            "█",
                            module.Style().fg(
                                module.Color.rgb(red, green, blue)
                            ),
                        )
                    )
                else:
                    bar_spans.append(
                        module.Span(
                            "░",
                            module.style(theme, module.Semantic.MUTED),
                        )
                    )
        bar_spans.append(
            module.Span("]", module.style(theme, module.Semantic.MUTED))
        )

        progress = module.Text(
            [
                module.Line(bar_spans).centered(),
                module.Line([]),
                module.Line([]),
            ]
        )
        frame.render_widget(
            module.Paragraph(progress).style(module.panel_style(theme)),
            progress_area,
        )

    module.TuiState.apply = apply
    module._render_startup = render_startup
    module._splined_database_gradient_progress = True


def install() -> None:
    """Extend the SQLite build presentation before its runtime patch executes."""
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True

    original_patch = build_policy._patch_tui

    def patch_tui(module: Any) -> None:
        original_patch(module)
        _patch_build_presentation(module)

    build_policy._patch_tui = patch_tui
