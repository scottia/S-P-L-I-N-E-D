"""Binding capability facts verified against the pinned pyratatui source."""

from __future__ import annotations

import importlib.util


PYRATATUI_VERSION = "0.3.0"
RATATUI_VERSION = "0.30.2"

# pyratatui 0.3.0 remains the renderer. SPLINED's small ABI3 PyO3 extension
# supplies the crossterm event/capture surface missing from the published wheel.
MOUSE_EVENTS_AVAILABLE = importlib.util.find_spec("splined_pyratatui_input") is not None
MOUSE_LIMITATION = (
    "crossterm mouse capture/events are provided by splined-pyratatui-input"
    if MOUSE_EVENTS_AVAILABLE
    else "install the splined-pyratatui-input wheel for mouse/touch support"
)
