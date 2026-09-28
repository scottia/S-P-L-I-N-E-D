"""Restore the multicolor percent bar for SPLINED database builds.

The resumable SQLite builder owns build semantics and text. This policy only
replaces its single-color Gauge with the established red→orange→yellow→green
progress treatment used by the prior Album-status banner.
"""

from __future__ import annotations

from typing import Any

import splined_media_build_policy as build_policy


_INSTALLED = False


def _patch_build_presentation(module: Any) -> None:
    if getattr(module, "_splined_database_gradient_progress", False):
        return

    original_render = module._render_startup

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
        bar_width = max(8, int(progress_area.width) - 4)
        filled = min(bar_width, round(bar_width * ratio))

        bar_spans: list[Any] = [
            module.Span("[", module.style(theme, module.Semantic.MUTED))
        ]
        for index in range(bar_width):
            if total and index < filled:
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
        bar_spans.append(
            module.Span("]", module.style(theme, module.Semantic.MUTED))
        )

        label = f"{percent:.1f}%" if total else "DISCOVERING"
        label_red, label_green, label_blue = module._status_gradient_rgb(
            ratio if total else 0.0
        )
        progress = module.Text(
            [
                module.Line(bar_spans).centered(),
                module.Line(
                    [
                        module.Span(
                            label,
                            module.Style().fg(
                                module.Color.rgb(
                                    label_red,
                                    label_green,
                                    label_blue,
                                )
                            ),
                        )
                    ]
                ).centered(),
            ]
        )
        frame.render_widget(
            module.Paragraph(progress).style(module.panel_style(theme)),
            progress_area,
        )

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
