"""Brief launch-only SPLINED brand animation."""

from __future__ import annotations

import unicodedata
from dataclasses import dataclass


TITLE = "S:P:L:I:N:E:D"
EXPANSION = "SEARCHABLE:PIXEL:LINKS:IDENTIFIED:NORMALIZED:ENRICHED:DEFINED"
STYLIZED_EXPANSION = "SƎARCHABLƎ:PІXƎL:LІNKS:ІDƎNTІFІƎD:NORMALІZƎD:ƎNRІCHƎD:DƎFІNƎD"
STARTUP_SECONDS = 1.25


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
    progress = min(1.0, max(0.0, elapsed / STARTUP_SECONDS))
    # A one-cell oscillation suggests a fold without moving readable text for
    # more than the short launch signature.
    offset = 1 if 0.32 <= progress < 0.68 else 0
    return BrandFrame(TITLE, _folded(progress), offset, progress >= 1.0)


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
