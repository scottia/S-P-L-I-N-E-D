"""Native artwork-overlay lifecycle and local candidate preview policy.

Ratatui redraws its cell buffer independently from terminal-native image
protocols.  This policy keeps unchanged overlays in place, clears stale native
pixels only when content/geometry changes, and renders local/embedded candidate
artwork through ratatui-image instead of the low-detail Unicode half-block
fallback whenever the terminal supports a native image protocol.
"""

from __future__ import annotations

from pathlib import Path
import sys
from typing import Any


def _local_cover_key(state: Any) -> tuple[str, Any, int]:
    return (
        str(getattr(state, "local_cover_path", "")),
        getattr(state, "local_cover_rect", None),
        id(getattr(state, "local_cover_overlay", None)),
    )


def _remote_overlay_key(state: Any) -> tuple[Any, int]:
    return (
        getattr(state, "remote_preview_rect", None),
        id(getattr(state, "remote_hover_overlay", None)),
    )


def _candidate_file_identity(path: Path) -> tuple[int, int]:
    try:
        stat = path.stat()
        return int(stat.st_size), int(stat.st_mtime_ns)
    except OSError:
        return -1, -1


def _patch_tui(module: Any) -> None:
    if getattr(module, "_splined_stable_local_overlay", False):
        return

    original_apply = module.TuiState.apply
    original_render_candidate = module._render_candidate_preview
    original_clear_local = module._clear_local_cover_overlay
    original_draw_local = module._draw_local_cover_overlay
    original_clear_remote = module._clear_stale_remote_overlay
    original_draw_remote = module._draw_remote_hover_overlay

    def apply(state: Any, event: str, payload: dict[str, Any]) -> None:
        original_apply(state, event, payload)
        if event in {"scan_start", "album", "candidates"} and str(
            getattr(state, "transient", "")
        ).startswith("Returning to the retained Select Media session"):
            state.transient = ""

    def clear_local_cover_overlay(state: Any) -> None:
        drawn = getattr(state, "local_cover_drawn_rect", None)
        if drawn is None:
            return

        visible = (
            getattr(state, "tab", "") == "main"
            and getattr(state, "workflow", "") == "library"
            and getattr(state, "local_cover_overlay", None) is not None
            and getattr(state, "local_cover_rect", None) is not None
        )
        current_key = _local_cover_key(state)
        drawn_key = getattr(state, "_splined_local_cover_drawn_key", None)

        if visible and drawn == current_key[1] and drawn_key == current_key:
            return

        original_clear_local(state)
        setattr(state, "_splined_local_cover_drawn_key", None)

    def draw_local_cover_overlay(state: Any) -> None:
        drawn = getattr(state, "local_cover_drawn_rect", None)
        current_key = _local_cover_key(state)
        drawn_key = getattr(state, "_splined_local_cover_drawn_key", None)

        if drawn is not None and (
            drawn != current_key[1] or drawn_key != current_key
        ):
            original_clear_local(state)
            setattr(state, "_splined_local_cover_drawn_key", None)

        original_draw_local(state)
        if getattr(state, "local_cover_drawn_rect", None) is not None:
            setattr(
                state,
                "_splined_local_cover_drawn_key",
                _local_cover_key(state),
            )

    def clear_remote_overlay(state: Any) -> None:
        drawn = getattr(state, "remote_drawn_rect", None)
        if drawn is None:
            return
        current_key = _remote_overlay_key(state)
        drawn_key = getattr(state, "_splined_remote_drawn_key", None)
        visible = bool(module._remote_overlay_is_visible(state))
        if visible and drawn == current_key[0] and drawn_key == current_key:
            return
        original_clear_remote(state)
        setattr(state, "_splined_remote_drawn_key", None)

    def draw_remote_overlay(state: Any) -> None:
        if not module._remote_overlay_is_visible(state):
            return
        drawn = getattr(state, "remote_drawn_rect", None)
        current_key = _remote_overlay_key(state)
        drawn_key = getattr(state, "_splined_remote_drawn_key", None)
        if drawn is not None and (
            drawn != current_key[0] or drawn_key != current_key
        ):
            original_clear_remote(state)
            setattr(state, "_splined_remote_drawn_key", None)
            drawn = None
        if drawn is not None and drawn_key == current_key:
            return
        original_draw_remote(state)
        if getattr(state, "remote_drawn_rect", None) is not None:
            setattr(state, "_splined_remote_drawn_key", current_key)

    def render_candidate_preview(
        frame: Any,
        area: Any,
        state: Any,
        theme: Any,
        candidate: Any,
        *,
        title: str = "ARTWORK",
    ) -> None:
        source = Path(str(getattr(candidate, "path", "") or ""))
        native = getattr(module, "native_prepare_image_overlay", None)
        if native is None or not source.is_file() or source.is_symlink():
            original_render_candidate(
                frame,
                area,
                state,
                theme,
                candidate,
                title=title,
            )
            return

        width = max(1, int(area.width) - 2)
        height = max(1, int(area.height) - 2)
        identity = _candidate_file_identity(source)
        cache_key = (
            str(source),
            identity[0],
            identity[1],
            width,
            height,
        )
        cache = getattr(state, "_splined_local_candidate_overlays", None)
        if not isinstance(cache, dict):
            cache = {}
            setattr(state, "_splined_local_candidate_overlays", cache)

        overlay = cache.get(cache_key)
        if overlay is None:
            try:
                overlay = native(
                    source.read_bytes(),
                    width,
                    height,
                    4096,
                )
                cache.clear()
                cache[cache_key] = overlay
            except Exception:
                original_render_candidate(
                    frame,
                    area,
                    state,
                    theme,
                    candidate,
                    title=title,
                )
                return

        try:
            candidate_index = state.candidates.index(candidate)
        except (AttributeError, ValueError):
            candidate_index = max(0, int(getattr(state, "selected_index", 0)))

        rect = (
            int(area.x) + 1,
            int(area.y) + 1,
            width,
            height,
        )
        state.remote_hover_token += 1
        state.remote_hover_index = candidate_index
        state.remote_hover_url = f"local:{source}:{identity[0]}:{identity[1]}"
        state.remote_hover_overlay = overlay
        state.remote_hover_loading = False
        state.remote_hover_error = ""
        state.remote_hover_active = False
        state.remote_preview_width = width
        state.remote_preview_height = height
        state.remote_preview_rect = rect
        frame.render_widget(
            module.Paragraph.from_string("").block(
                module.card(
                    theme,
                    title,
                    module.Semantic.ACTIVE,
                )
            ),
            area,
        )

    module.TuiState.apply = apply
    module._render_candidate_preview = render_candidate_preview
    module._clear_local_cover_overlay = clear_local_cover_overlay
    module._draw_local_cover_overlay = draw_local_cover_overlay
    module._clear_stale_remote_overlay = clear_remote_overlay
    module._draw_remote_hover_overlay = draw_remote_overlay
    module._splined_stable_local_overlay = True


def install(core: Any) -> None:
    """Patch the TUI immediately before an interactive operational run."""
    if getattr(core, "_splined_tui_overlay_policy_installed", False):
        return

    original = core.run_operational_interface

    def run_operational_interface(args: Any, worker: Any) -> int:
        likely_tui = (
            not bool(getattr(args, "no_tui", False))
            and bool(getattr(sys.stdin, "isatty", lambda: False)())
            and bool(getattr(sys.stdout, "isatty", lambda: False)())
        )
        if likely_tui:
            try:
                from tui import splined_tui

                _patch_tui(splined_tui)
            except ImportError:
                pass
        return int(original(args, worker))

    core.run_operational_interface = run_operational_interface
    core._splined_tui_overlay_policy_installed = True
