"""Locked SPLINED TUI palettes.

Only semantic RGB values live here.  Rendering code asks for a meaning such as
``accepted`` or ``fallback`` and never chooses an ad-hoc color.
"""

from __future__ import annotations

from types import MappingProxyType
from typing import Final, Mapping

RGB = tuple[int, int, int]


OLED: Final[Mapping[str, RGB]] = MappingProxyType(
    {
        "background": (0, 0, 0),
        "panel": (7, 9, 15),
        "text": (225, 230, 235),
        "muted": (145, 150, 165),
        "accepted": (60, 255, 135),
        "active": (55, 225, 255),
        "fallback": (255, 145, 35),
        "warning": (255, 235, 60),
        "rejected": (255, 70, 95),
        "history": (175, 95, 255),
        "special": (255, 80, 220),
        "disabled": (105, 110, 125),
        "info": (118, 255, 180),
        "debug": (80, 170, 255),
    }
)


CHALK: Final[Mapping[str, RGB]] = MappingProxyType(
    {
        "background": (24, 25, 27),
        "panel": (34, 35, 38),
        "text": (232, 228, 216),
        "muted": (165, 157, 145),
        "accepted": (144, 177, 137),
        "active": (112, 164, 170),
        "fallback": (190, 145, 74),
        "warning": (202, 165, 91),
        "rejected": (174, 91, 77),
        "history": (145, 121, 161),
        "special": (164, 116, 145),
        "disabled": (101, 99, 96),
        "info": (177, 184, 163),
        "debug": (105, 145, 153),
    }
)


OLED_SPECTRUM: Final[tuple[RGB, ...]] = (
    (255, 70, 95),
    (255, 145, 35),
    (255, 235, 60),
    (60, 255, 135),
    (55, 225, 255),
    (70, 135, 255),
    (175, 95, 255),
    (255, 80, 220),
)

CHALK_SPECTRUM: Final[tuple[RGB, ...]] = (
    (174, 91, 77),
    (190, 145, 74),
    (202, 165, 91),
    (144, 177, 137),
    (112, 164, 170),
    (105, 145, 153),
    (145, 121, 161),
    (164, 116, 145),
)
