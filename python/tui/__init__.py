"""Ratatui presentation layer for the SPLINED Python engine.

The package deliberately keeps policy out of the UI.  Engine modules publish
state through :mod:`tui.status`; pyratatui is imported only when TUI mode is
actually selected so the plain CLI remains independently usable.
"""

from .theme import Theme, ThemeName, select_theme

__all__ = ["Theme", "ThemeName", "select_theme"]
