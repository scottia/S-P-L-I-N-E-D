"""Theme selection for the two locked SPLINED presentations."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Mapping

from .palette import CHALK, CHALK_SPECTRUM, OLED, OLED_SPECTRUM, RGB


class ThemeName(str, Enum):
    OLED = "OLED"
    CHALK = "CHALK"


@dataclass(frozen=True)
class Theme:
    name: ThemeName
    colors: Mapping[str, RGB]
    title_spectrum: tuple[RGB, ...]

    def color(self, semantic: str) -> RGB:
        return self.colors[semantic]


THEMES = {
    ThemeName.OLED: Theme(ThemeName.OLED, OLED, OLED_SPECTRUM),
    ThemeName.CHALK: Theme(ThemeName.CHALK, CHALK, CHALK_SPECTRUM),
}


def select_theme(value: str | ThemeName | None) -> Theme:
    if value is None:
        return THEMES[ThemeName.OLED]
    if isinstance(value, ThemeName):
        return THEMES[value]
    try:
        return THEMES[ThemeName(str(value).strip().upper())]
    except ValueError as exc:
        raise ValueError("TUI theme must be OLED or CHALK.") from exc
