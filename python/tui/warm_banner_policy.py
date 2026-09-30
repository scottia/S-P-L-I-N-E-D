"""Present warm SQLite startup as loading, never as a first database build.

The Advanced runtime has two materially different startup modes:

* an actual first build or explicit refresh, which may discover folders and
  read representative tags;
* a warm load of an already-published ``splined.db`` snapshot.

Both use the same spectral progress language, but they must never share the
same user-facing title.  This policy also records the authoritative index check
so a debug log proves whether a launch reused or rebuilt the database.
"""

from __future__ import annotations

import json
import time
from typing import Any

import splined_media_build_policy as build_policy
import splined_media_index as media_index


_INSTALLED = False


def _is_warm_progress(payload: dict[str, Any]) -> bool:
    phase = str(payload.get("phase", "")).strip().casefold()
    status = str(payload.get("status", "")).strip().casefold()
    if phase not in {"load", "status", "commit", "ready"}:
        return False
    return any(
        token in status
        for token in (
            "sqlite",
            "indexed",
            "select media",
            "folder status",
            "artist rows",
            "album rows",
        )
    )


def _spectral_spans(
    module: Any,
    theme: Any,
    width: int,
    ratio: float | None,
    started_at: float,
) -> list[Any]:
    bar_width = min(64, max(24, width - 10))
    spans: list[Any] = [
        module.Span("[", module.style(theme, module.Semantic.MUTED))
    ]

    if ratio is not None:
        filled = min(
            bar_width,
            round(bar_width * max(0.0, min(1.0, ratio))),
        )
        for index in range(bar_width):
            if index < filled:
                red, green, blue = module._status_gradient_rgb(
                    index / max(1, bar_width - 1)
                )
                spans.append(
                    module.Span(
                        "█",
                        module.Style().fg(
                            module.Color.rgb(red, green, blue)
                        ),
                    )
                )
            else:
                spans.append(
                    module.Span(
                        "░",
                        module.style(theme, module.Semantic.MUTED),
                    )
                )
    else:
        pulse_width = max(8, min(16, bar_width // 4))
        elapsed = max(0.0, time.monotonic() - started_at)
        head = (int(elapsed * 12.0) % (bar_width + pulse_width)) - pulse_width
        for index in range(bar_width):
            offset = index - head
            if 0 <= offset < pulse_width:
                red, green, blue = module._status_gradient_rgb(
                    offset / max(1, pulse_width - 1)
                )
                spans.append(
                    module.Span(
                        "█",
                        module.Style().fg(
                            module.Color.rgb(red, green, blue)
                        ),
                    )
                )
            else:
                spans.append(
                    module.Span(
                        "░",
                        module.style(theme, module.Semantic.MUTED),
                    )
                )

    spans.append(module.Span("]", module.style(theme, module.Semantic.MUTED)))
    return spans


def _patch_tui(module: Any) -> None:
    if getattr(module, "_splined_warm_banner_policy", False):
        return

    original_apply = module.TuiState.apply
    original_render = module._render_startup

    def apply(state: Any, event: str, payload: dict[str, Any]) -> None:
        original_apply(state, event, payload)
        if event != "cache_progress" or not _is_warm_progress(payload):
            return

        # Never mask a genuine first build or explicit refresh.  Warm starts do
        # not emit cache_build_start, so they have no authoritative build reason.
        reason = str(getattr(state, "cache_reason", "")).strip().casefold()
        if reason in {"initial-build", "explicit-refresh"}:
            return
        state.cache_kind = "sqlite-warm"
        state.cache_reason = "warm-start"
        state.cache_resumable = False

    def render_startup(frame: Any, state: Any, theme: Any) -> None:
        original_render(frame, state, theme)
        if (
            getattr(state, "cache_kind", "") != "sqlite-warm"
            or str(getattr(state, "cache_reason", "")).casefold()
            != "warm-start"
        ):
            return

        area = frame.area
        brand_height = min(
            int(area.height),
            module.startup_brand_height(area.width, area.height),
        )
        _brand_area, inventory_area = module._split_vertical(
            area,
            [module.Constraint.length(brand_height), module.Constraint.fill(1)],
        )
        panel_width = min(max(48, int(inventory_area.width) - 8), 86)
        panel_height = min(10, int(inventory_area.height))
        panel = module.Rect(
            int(inventory_area.x)
            + max(0, (int(inventory_area.width) - panel_width) // 2),
            int(inventory_area.y)
            + max(0, (int(inventory_area.height) - panel_height) // 2),
            panel_width,
            panel_height,
        )

        # Clear the complete startup card so no BUILDING/ONE-TIME wording can
        # bleed through from an earlier wrapper in the presentation chain.
        frame.render_widget(module.Clear(), panel)
        frame.render_widget(
            module.card(
                theme,
                "LOADING SPLINED DATABASE",
                module.Semantic.ACTIVE,
            ),
            panel,
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
                module.Constraint.length(3),
                module.Constraint.length(2),
                module.Constraint.fill(1),
            ],
        )

        status = str(
            getattr(state, "inventory_status", "Loading existing SQLite index")
        )
        indexed_albums = int(getattr(state, "cache_albums", 0) or 0)
        detail = (
            f"Existing index · {indexed_albums:,} Albums"
            if indexed_albums
            else "Existing index · warm start"
        )
        frame.render_widget(
            module.Paragraph(
                module.Text(
                    [
                        module.Line(
                            [
                                module.Span(
                                    status,
                                    module.style(
                                        theme,
                                        module.Semantic.ACTIVE,
                                        bold=True,
                                    ),
                                )
                            ]
                        ).centered(),
                        module.Line(
                            [
                                module.Span(
                                    detail,
                                    module.style(theme, module.Semantic.TEXT),
                                )
                            ]
                        ).centered(),
                    ]
                )
            ),
            sections[0],
        )

        total = int(getattr(state, "cache_total", 0) or 0)
        percent_value = getattr(state, "cache_percent", None)
        ratio = (
            float(percent_value) / 100.0
            if total and isinstance(percent_value, (int, float))
            else None
        )
        spans = _spectral_spans(
            module,
            theme,
            int(sections[1].width),
            ratio,
            float(getattr(state, "started_at", time.monotonic())),
        )
        frame.render_widget(
            module.Paragraph(
                module.Text(
                    [
                        module.Line(spans).centered(),
                        module.Line([]),
                    ]
                )
            ).style(module.panel_style(theme)),
            sections[1],
        )

        frame.render_widget(
            module.Paragraph(
                module.Text(
                    [
                        module.Line(
                            [
                                module.Span(
                                    "Existing splined.db · no folders or tags are being rebuilt",
                                    module.style(theme, module.Semantic.MUTED),
                                )
                            ]
                        ).centered()
                    ]
                )
            ),
            sections[2],
        )

    module.TuiState.apply = apply
    module._render_startup = render_startup
    module._splined_warm_banner_policy = True


def install(core: Any) -> None:
    """Install warm-banner truthfulness and index-check diagnostics."""
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True

    original_patch_tui = build_policy._patch_tui

    def patch_tui(module: Any) -> None:
        original_patch_tui(module)
        _patch_tui(module)

    build_policy._patch_tui = patch_tui

    original_index_is_usable = media_index.index_is_usable

    def index_is_usable(connection: Any, expected: dict[str, Any]) -> bool:
        usable = bool(original_index_is_usable(connection, expected))
        generated_at = ""
        inventory_totals = ""
        try:
            row = connection.execute(
                "SELECT generated_at, folders_json FROM picker_inventory "
                "WHERE inventory_key=?",
                (media_index.database.INVENTORY_KEY,),
            ).fetchone()
            if row is not None:
                generated_at = str(row["generated_at"] or "")
                try:
                    folders = json.loads(str(row["folders_json"]))
                except (TypeError, ValueError):
                    folders = {}
                if isinstance(folders, dict):
                    inventory_totals = (
                        f"artists={int(folders.get('artists', 0) or 0)} "
                        f"albums={int(folders.get('albums', 0) or 0)}"
                    )
        except Exception:
            pass

        core.debug_log(
            "splined.db.index_check "
            f"usable={str(usable).lower()} "
            f"action={'warm-start' if usable else 'initial-build'} "
            f"generated_at={generated_at!r} {inventory_totals}".rstrip()
        )
        return usable

    media_index.index_is_usable = index_is_usable
    core._splined_warm_banner_policy_installed = True
