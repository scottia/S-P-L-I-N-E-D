"""Responsive decisions kept independent of terminal rendering."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum


class Breakpoint(str, Enum):
    WIDE = "WIDE"
    NORMAL = "NORMAL"
    COMPACT = "COMPACT"
    MINIMUM = "MINIMUM"


@dataclass(frozen=True)
class LayoutSpec:
    breakpoint: Breakpoint
    stack_cards: bool
    compact_header: bool
    candidate_columns: tuple[str, ...]


def breakpoint(width: int, height: int) -> Breakpoint:
    if width < 48 or height < 12:
        return Breakpoint.MINIMUM
    if width < 80 or height < 24:
        return Breakpoint.COMPACT
    if width < 120 or height < 32:
        return Breakpoint.NORMAL
    return Breakpoint.WIDE


def layout_spec(width: int, height: int) -> LayoutSpec:
    point = breakpoint(width, height)
    if point is Breakpoint.WIDE:
        columns = ("#", "source", "resolution", "format", "range", "distance", "square", "acceptable", "approved", "id")
    elif point is Breakpoint.NORMAL:
        columns = ("#", "source", "resolution", "format", "range", "distance", "square", "acceptable")
    else:
        columns = ("#", "source", "resolution", "range", "acceptable")
    return LayoutSpec(
        breakpoint=point,
        stack_cards=point in {Breakpoint.COMPACT, Breakpoint.MINIMUM},
        compact_header=point is not Breakpoint.WIDE,
        candidate_columns=columns,
    )
