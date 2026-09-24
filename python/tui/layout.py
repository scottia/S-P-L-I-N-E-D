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


@dataclass(frozen=True)
class CandidateColumnLayout:
    """One frame-level terminal-cell grid shared by every source group."""

    columns: tuple[str, ...]
    widths: tuple[int, ...]
    starts: tuple[int, ...]
    spacing: int = 1

    def start(self, column: str) -> int:
        return self.starts[self.columns.index(column)]

    def width(self, column: str) -> int:
        return self.widths[self.columns.index(column)]


def candidate_column_layout(
    width: int,
    height: int,
    *,
    ai_enabled: bool,
) -> CandidateColumnLayout:
    """Allocate explicit Ratatui columns in terminal display cells.

    ``width`` is the complete framed table width.  Widths and starts are
    computed once for the frame, then reused by Preferred, Local, Enhanced,
    and every provider group.
    """
    point = breakpoint(width, height)
    if point is Breakpoint.WIDE:
        columns = (
            ("#", "ai_enhanced", "ai_splined") if ai_enabled else ("#",)
        ) + (
            "source",
            "resolution",
            "format",
            "range",
            "distance",
            "square",
            "acceptable",
            "approved",
            "url",
        )
    elif point is Breakpoint.NORMAL:
        columns = (
            ("#", "ai_enhanced", "ai_splined") if ai_enabled else ("#",)
        ) + (
            "source",
            "resolution",
            "range",
            "distance",
            "square",
            "acceptable",
            "url",
        )
    else:
        columns = (
            ("#", "ai_enhanced") if ai_enabled else ("#",)
        ) + ("source", "resolution", "range", "acceptable", "url")

    minimums = {
        "#": 3,
        "ai_enhanced": 10,
        "ai_splined": 9,
        "source": 10,
        "resolution": 10,
        "format": 6,
        "range": 11,
        "distance": 7,
        "square": 7,
        "acceptable": 9,
        "approved": 8,
        "url": 5,
    }
    preferred = {
        "#": 4,
        "ai_enhanced": 13,
        "ai_splined": 11,
        "source": 20,
        "resolution": 13,
        "format": 8,
        "range": 15,
        "distance": 9,
        "square": 9,
        "acceptable": 12,
        "approved": 10,
        "url": 7,
    }
    spacing = 1
    inner = max(1, int(width) - 2)
    widths = {column: minimums[column] for column in columns}
    floors = {
        "#": 2,
        "ai_enhanced": 7,
        "ai_splined": 7,
        "source": 5,
        "resolution": 7,
        "format": 4,
        "range": 7,
        "distance": 5,
        "square": 5,
        "acceptable": 5,
        "approved": 5,
        "url": 5,
    }
    excess = max(
        0,
        sum(widths.values()) + spacing * (len(columns) - 1) - inner,
    )
    shrink_order = (
        "source",
        "range",
        "resolution",
        "ai_enhanced",
        "ai_splined",
        "acceptable",
        "approved",
        "distance",
        "square",
        "format",
        "#",
    )
    while excess:
        changed = False
        for column in shrink_order:
            if column in widths and widths[column] > floors[column]:
                widths[column] -= 1
                excess -= 1
                changed = True
                if not excess:
                    break
        if not changed:
            break
    available = max(0, inner - sum(widths.values()) - spacing * (len(columns) - 1))
    # Grow information-bearing columns first rather than leaving a large dead
    # area while source/range labels are truncated.
    growth_order = (
        "source",
        "range",
        "resolution",
        "ai_enhanced",
        "ai_splined",
        "acceptable",
        "approved",
        "distance",
        "square",
        "format",
        "#",
        "url",
    )
    while available:
        changed = False
        for column in growth_order:
            if column in widths and widths[column] < preferred[column]:
                widths[column] += 1
                available -= 1
                changed = True
                if not available:
                    break
        if not changed:
            widths["url"] += available
            available = 0

    starts: list[int] = []
    offset = 0
    for column in columns:
        starts.append(offset)
        offset += widths[column] + spacing
    return CandidateColumnLayout(
        columns,
        tuple(widths[column] for column in columns),
        tuple(starts),
        spacing,
    )


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
        columns = ("#", "source", "resolution", "format", "range", "distance", "square", "acceptable", "approved", "url")
    elif point is Breakpoint.NORMAL:
        columns = ("#", "source", "resolution", "format", "range", "distance", "acceptable", "url")
    else:
        columns = ("#", "resolution", "range", "acceptable", "url")
    return LayoutSpec(
        breakpoint=point,
        stack_cards=point in {Breakpoint.COMPACT, Breakpoint.MINIMUM},
        compact_header=point is not Breakpoint.WIDE,
        candidate_columns=columns,
    )
