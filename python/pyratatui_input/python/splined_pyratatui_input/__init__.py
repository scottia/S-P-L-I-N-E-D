"""Crossterm event extension used alongside pyratatui 0.3.0."""

from ._native import EventReader, InputEvent, __version__, emergency_restore

__all__ = ["EventReader", "InputEvent", "emergency_restore", "__version__"]
