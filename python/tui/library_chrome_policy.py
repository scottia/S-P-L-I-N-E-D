"""Final Select Media chrome and retained-session presentation fixes.

This policy is intentionally Advanced-only. It keeps the spectral SPLINED
wordmark attached to the Album Status panel, preserves the yellow operational
frame used by LAUNCH, removes the duplicate standalone library-header wordmark,
and clears the temporary return message as soon as the retained picker is ready.
"""

from __future__ import annotations

from typing import Any

import splined_media_build_policy as build_policy


_INSTALLED = False
_RETURNING_MESSAGE = "Returning to the retained Select Media session"


def _control_panels(module: Any, area: Any) -> list[Any]:
    """Mirror the authoritative Select Media control-panel geometry."""
    if int(area.width) < 96:
        return module._split_vertical(
            area,
            [
                module.Constraint.length(8),
                module.Constraint.length(6),
                module.Constraint.length(5),
                module.Constraint.length(5),
            ],
        )
    return module._split_horizontal(
        area,
        [
            module.Constraint.percentage(25),
            module.Constraint.percentage(25),
            module.Constraint.percentage(25),
            module.Constraint.fill(1),
        ],
    )


def _render_album_status_title(
    module: Any,
    frame: Any,
    panel: Any,
    theme: Any,
) -> None:
    """Overlay ``S:P:L:I:N:E:D ALBUM STATUS`` into the yellow frame."""
    panel_width = int(panel.width)
    title_x = int(panel.x) + 2
    title_y = int(panel.y)
    spectral_width = len(module.SPLINED_TITLE) + 1
    label = " ALBUM STATUS "

    if panel_width > spectral_width + len(label) + 3:
        frame.render_widget(
            module.Paragraph(
                module.Text(
                    [
                        module.spectral_title(
                            theme,
                            title=module.SPLINED_TITLE,
                        )
                    ]
                )
            ),
            module.Rect(
                title_x,
                title_y,
                spectral_width,
                1,
            ),
        )
        frame.render_widget(
            module.Paragraph.from_string(label).style(
                module.style(
                    theme,
                    module.Semantic.WARNING,
                    bold=True,
                )
            ),
            module.Rect(
                title_x + len(module.SPLINED_TITLE),
                title_y,
                min(
                    len(label),
                    max(
                        1,
                        int(panel.x + panel.width)
                        - title_x
                        - len(module.SPLINED_TITLE)
                        - 1,
                    ),
                ),
                1,
            ),
        )
        return

    # Compact fallback: keep the frame/title yellow even when the full
    # spectral wordmark cannot fit in the available title rail.
    frame.render_widget(
        module.Paragraph.from_string(" ALBUM STATUS ").style(
            module.style(
                theme,
                module.Semantic.WARNING,
                bold=True,
            )
        ),
        module.Rect(
            title_x,
            title_y,
            max(1, panel_width - 4),
            1,
        ),
    )


def _patch_tui(module: Any) -> None:
    if getattr(module, "_splined_library_chrome_policy", False):
        return

    original_apply = module.TuiState.apply
    original_card = module.card
    original_render_header = module._render_header
    original_render_controls = module._render_library_controls

    def apply(state: Any, event: str, payload: dict[str, Any]) -> None:
        original_apply(state, event, payload)

        # The report screen sets this message before the engine resumes the
        # retained picker. Clear it on the first authoritative library/input
        # event so the normal key/menu footer is visible again.
        request = getattr(state, "input_request", None)
        request_kind = str(getattr(request, "kind", ""))
        if (
            str(getattr(state, "transient", "")).startswith(
                _RETURNING_MESSAGE
            )
            and str(getattr(state, "workflow", "")) == "library"
            and (
                event in {"library", "library_update"}
                or request_kind == "library-selection"
            )
        ):
            state.transient = ""

    def card(
        theme: Any,
        title: str,
        semantic: Any,
        *args: Any,
        **kwargs: Any,
    ) -> Any:
        if str(title).strip().casefold() in {
            "folder status",
            "album status",
        }:
            # The title is drawn as two separately styled overlays below.
            # WARNING matches the established yellow LAUNCH frame.
            return original_card(
                theme,
                "",
                module.Semantic.WARNING,
                *args,
                **kwargs,
            )
        return original_card(theme, title, semantic, *args, **kwargs)

    def render_header(
        frame: Any,
        area: Any,
        state: Any,
        theme: Any,
        point: Any,
    ) -> None:
        # Select Media carries the wordmark inside ALBUM STATUS. Suppress the
        # duplicate one-line header, but preserve all other operational headers
        # and the Source Policy workspace header.
        if (
            str(getattr(state, "workflow", "")) == "library"
            and str(getattr(state, "workspace", "")) == "library"
        ):
            return
        original_render_header(frame, area, state, theme, point)

    def render_library_controls(
        frame: Any,
        area: Any,
        state: Any,
        theme: Any,
    ) -> None:
        original_render_controls(frame, area, state, theme)
        panels = _control_panels(module, area)
        if panels:
            _render_album_status_title(
                module,
                frame,
                panels[0],
                theme,
            )

    module.TuiState.apply = apply
    module.card = card
    module._render_header = render_header
    module._render_library_controls = render_library_controls
    module._splined_library_chrome_policy = True


def install() -> None:
    """Install after the other Advanced TUI policies have wrapped Ratatui."""
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True

    original_patch_tui = build_policy._patch_tui

    def patch_tui(module: Any) -> None:
        original_patch_tui(module)
        _patch_tui(module)

    build_policy._patch_tui = patch_tui
