"""Native artwork-overlay lifecycle corrections for the Ratatui interface.

Ratatui redraws its cell buffer independently from terminal-native image
protocols.  Clearing and redrawing an unchanged local cover on every dirty
frame causes visible flicker, especially after theme-aware clearing made the
operation explicit under CHALK.  This policy keeps an unchanged overlay in
place and clears it only when its content, geometry, or workflow visibility
actually changes.
"""

from __future__ import annotations

import sys
from typing import Any


def _overlay_key(state: Any) -> tuple[str, Any, int]:
    return (
        str(getattr(state, "local_cover_path", "")),
        getattr(state, "local_cover_rect", None),
        id(getattr(state, "local_cover_overlay", None)),
    )


def _patch_tui(module: Any) -> None:
    if getattr(module, "_splined_stable_local_overlay", False):
        return

    original_clear = module._clear_local_cover_overlay
    original_draw = module._draw_local_cover_overlay

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
        current_key = _overlay_key(state)
        drawn_key = getattr(state, "_splined_local_cover_drawn_key", None)

        # Preserve the native image when Ratatui is merely repainting unrelated
        # text/status cells.  This restores the stable pre-72caed3 behavior.
        if visible and drawn == current_key[1] and drawn_key == current_key:
            return

        original_clear(state)
        setattr(state, "_splined_local_cover_drawn_key", None)

    def draw_local_cover_overlay(state: Any) -> None:
        drawn = getattr(state, "local_cover_drawn_rect", None)
        current_key = _overlay_key(state)
        drawn_key = getattr(state, "_splined_local_cover_drawn_key", None)

        # A resize or newly prepared cover can change geometry/content during
        # the Ratatui render callback.  Remove the old native image immediately
        # before drawing the replacement so no stale strip remains behind.
        if drawn is not None and (
            drawn != current_key[1] or drawn_key != current_key
        ):
            original_clear(state)
            setattr(state, "_splined_local_cover_drawn_key", None)

        original_draw(state)
        if getattr(state, "local_cover_drawn_rect", None) is not None:
            setattr(state, "_splined_local_cover_drawn_key", _overlay_key(state))

    module._clear_local_cover_overlay = clear_local_cover_overlay
    module._draw_local_cover_overlay = draw_local_cover_overlay
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
                # The authoritative activation path retains its existing
                # explicit-error/plain-CLI fallback behavior.
                pass
        return int(original(args, worker))

    core.run_operational_interface = run_operational_interface
    core._splined_tui_overlay_policy_installed = True
