"""SPLINED runtime hooks loaded automatically by Python's site module."""

from __future__ import annotations

import sys
from typing import Any

from splined_status_cache import install


def _requested_tui_theme() -> str:
    for index, argument in enumerate(sys.argv):
        value = str(argument).strip()
        if value.startswith("--tui-theme="):
            return value.split("=", 1)[1].strip().upper()
        if value == "--tui-theme" and index + 1 < len(sys.argv):
            return str(sys.argv[index + 1]).strip().upper()
    return "OLED"


def _install_theme_aware_image_clear() -> None:
    """Clear native image cells with the active TUI panel background."""
    try:
        import splined_pyratatui_input as native
    except ImportError:
        return

    original = getattr(native, "clear_image_area", None)
    if not callable(original) or getattr(original, "_splined_theme_aware", False):
        return

    panel_rgb = (34, 35, 38) if _requested_tui_theme() == "CHALK" else (0, 0, 0)

    def clear_image_area(x: int, y: int, width: int, height: int) -> Any:
        result = original(x, y, width, height)
        if width <= 0 or height <= 0:
            return result

        writer = getattr(sys, "__stdout__", None)
        if writer is None:
            return result

        red, green, blue = panel_rgb
        blank = " " * int(width)
        output = ["\x1b7"]
        for row in range(int(height)):
            output.append(
                f"\x1b[{int(y) + row + 1};{int(x) + 1}H"
                f"\x1b[48;2;{red};{green};{blue}m{blank}"
            )
        output.append("\x1b[0m\x1b8")
        writer.write("".join(output))
        writer.flush()
        return result

    clear_image_area._splined_theme_aware = True  # type: ignore[attr-defined]
    native.clear_image_area = clear_image_area


_install_theme_aware_image_clear()
install()
