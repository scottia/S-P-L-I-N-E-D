"""Readiness-driven SPLINED brand animation."""

from __future__ import annotations

import unicodedata
from dataclasses import dataclass


TITLE = "S:P:L:I:N:E:D"
EXPANSION = "SEARCHABLE:PIXEL:LINKS:IDENTIFIED:NORMALIZED:ENRICHED:DEFINED"
STYLIZED_EXPANSION = "SƎARCHABLƎ:PІXƎL:LІNKS:ІDƎNTІFІƎD:NORMALІZƎD:ƎNRІCHƎD:DƎFІNƎD"
BRAND_ANIMATION_FPS = 6
BRAND_ANIMATION_PERIOD = 4.0


def cell_width(text: str) -> int:
    """Return a conservative terminal-cell width without a font dependency."""
    width = 0
    for character in text:
        if unicodedata.combining(character):
            continue
        width += 2 if unicodedata.east_asian_width(character) in {"W", "F"} else 1
    return width


def _folded(progress: float) -> str:
    changed = [
        index
        for index, (normal, alternate) in enumerate(zip(EXPANSION, STYLIZED_EXPANSION))
        if normal != alternate
    ]
    count = min(len(changed), max(0, round(progress * len(changed))))
    characters = list(EXPANSION)
    for index in changed[:count]:
        characters[index] = STYLIZED_EXPANSION[index]
    return "".join(characters)


@dataclass(frozen=True)
class BrandFrame:
    title: str
    phrase: str
    offset: int
    complete: bool


def startup_frame(elapsed: float) -> BrandFrame:
    """Return a restrained looping frame until application readiness.

    The animation has a period, but deliberately has no completion time.  Its
    lifecycle is owned by the TUI's readiness state rather than wall-clock
    time.
    """
    phase = (max(0.0, elapsed) % BRAND_ANIMATION_PERIOD) / BRAND_ANIMATION_PERIOD
    progress = phase * 2.0 if phase <= 0.5 else (1.0 - phase) * 2.0
    offset = 1 if 0.22 <= phase < 0.32 or 0.72 <= phase < 0.82 else 0
    return BrandFrame(TITLE, _folded(progress), offset, False)


def animation_step(elapsed: float) -> int:
    """Return the low-rate redraw step for a still-loading startup screen."""
    return max(0, int(elapsed * BRAND_ANIMATION_FPS))


def fit_phrase(text: str, width: int) -> str:
    if width <= 0:
        return ""
    if cell_width(text) <= width:
        return text
    if width <= 3:
        return "." * width
    result = ""
    for character in text:
        if cell_width(result + character) > width - 3:
            break
        result += character
    return result + "..."
