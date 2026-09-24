"""Binding capability facts verified against the pinned pyratatui source."""

from __future__ import annotations


PYRATATUI_VERSION = "0.3.0"
RATATUI_VERSION = "0.30.2"

# pyratatui 0.3.0's Terminal.poll_event returns only PyKeyEvent and its Rust
# terminal lifecycle does not issue crossterm EnableMouseCapture. Treating key
# events as clicks would be false support and could corrupt escape sequences.
MOUSE_EVENTS_AVAILABLE = False
MOUSE_LIMITATION = (
    "pyratatui 0.3.0 exposes keyboard events only; crossterm mouse capture "
    "and MouseEvent are not part of the published Python wheel API"
)

