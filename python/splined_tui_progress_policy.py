"""Render one compact multicolor bar for SPLINED database builds.

During Album-folder discovery there is no honest denominator, so the banner
uses a moving red→orange→yellow→green activity pulse and omits a misleading
percentage. Once discovery completes, the same rail becomes a determinate
multicolor bar; the existing ``Albums x / y`` line remains the numeric progress
indicator.

The banner's checkpoint count is read from committed SQLite rows instead of
assuming that every processed Album has already been durably saved.
"""

from __future__ import annotations

from pathlib import Path
import sqlite3
import time
from typing import Any

import splined_media_build_policy as build_policy


_INSTALLED = False
_STAGE_TYPE = "media-index-stage"


def _saved_checkpoint_count(database_path: str) -> int | None:
    """Return committed staged Album rows, or ``None`` when temporarily busy."""
    path = Path(str(database_path).strip())
    if not path.is_file():
        return 0
    connection: sqlite3.Connection | None = None
    try:
        # A normal connection participates in the live WAL/shm state. Setting
        # query_only immediately afterwards keeps this presentation probe from
        # mutating the database while still seeing committed WAL transactions.
        connection = sqlite3.connect(path, timeout=0.25)
        connection.execute("PRAGMA query_only = ON")
        row = connection.execute(
            "SELECT COUNT(*) FROM cache_entries WHERE cache_type=?",
            (_STAGE_TYPE,),
        ).fetchone()
        return int(row[0] or 0) if row is not None else 0
    except (OSError, sqlite3.Error, TypeError, ValueError):
        return None
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
            durable = int(saved or 0)
            state.cache_saved_before_inventory = durable
            state.cache_staged = durable
        elif event == "cache_progress":
            database_path = str(
                payload.get(
                    "database",
                    getattr(state, "cache_database", ""),
                )
            )
            durable = _saved_checkpoint_count(database_path)
            if durable is not None:
                state.cache_staged = durable
                state.cache_saved_before_inventory = max(
                    int(
                        getattr(
                            state,
                            "cache_saved_before_inventory",
                            0,
                        )
                        or 0
                    ),
                    durable,
                )
            elif str(payload.get("phase", "")) == "inventory":
                # Discovery creates no stage rows. Preserve the last proven
                # durable count if the short live-WAL probe was temporarily busy.
                state.cache_staged = int(
                    getattr(state, "cache_saved_before_inventory", 0) or 0
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
        sections = module._split_vertical(
            inner,
            [
                module.Constraint.length(4),
                module.Constraint.length(2),
                module.Constraint.fill(1),
            ],
        )
        progress_area = sections[1]

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

        # The base database renderer draws a two-row Gauge here. Clear that
        # exact rectangle before painting the custom rail; otherwise empty
        # overlay lines leave the old Gauge visible as a second progress bar.
        frame.render_widget(module.Clear(), progress_area)
        frame.render_widget(
            module.Paragraph(
                module.Text(
                    [
                        module.Line(bar_spans).centered(),
                        module.Line([]),
                    ]
                )
            ).style(module.panel_style(theme)),
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
