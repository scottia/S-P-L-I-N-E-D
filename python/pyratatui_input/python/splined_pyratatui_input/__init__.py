"""Crossterm event extension used alongside pyratatui 0.3.0."""

from ._native import EventReader, InputEvent, __version__, emergency_restore

# MouseEvent is the public compatibility name requested by the TUI contract.
# The native value remains one compact event record because EventReader also
# carries keyboard and resize events through the same poll surface.
MouseEvent = InputEvent

__all__ = [
    "EventReader",
    "InputEvent",
    "MouseEvent",
    "emergency_restore",
    "__version__",
]
