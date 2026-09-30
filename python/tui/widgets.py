"""Small Ratatui widget builders used by workflow renderers."""

from __future__ import annotations

from typing import Iterable

from pyratatui import (
    Alignment,
    Block,
    BorderType,
    Color,
    Line,
    Modifier,
    Paragraph,
    Span,
    Style,
    Text,
)

from .animation import TITLE
from .semantic import Semantic
from .theme import Theme


def style(theme: Theme, semantic: Semantic | str, *, bold: bool = False) -> Style:
    name = semantic.value if isinstance(semantic, Semantic) else semantic
    result = Style().fg(Color.rgb(*theme.color(name))).bg(
        Color.rgb(*theme.color("background"))
    )
    if bold:
        result = result.add_modifier(Modifier.bold())
    return result


def panel_style(theme: Theme) -> Style:
    return Style().fg(Color.rgb(*theme.color("text"))).bg(
        Color.rgb(*theme.color("panel"))
    )


def card(theme: Theme, title: str, semantic: Semantic | str = Semantic.ACTIVE) -> Block:
    return (
        Block()
        .bordered()
        .border_type(BorderType.Rounded)
        .title(f" {title} ")
        .border_style(style(theme, semantic))
        .title_style(style(theme, semantic, bold=True))
        .style(panel_style(theme))
        .padding(left=1, right=1)
    )


def spectral_title(
    theme: Theme, *, centered: bool = False, title: str = TITLE
) -> Line:
    spans: list[Span] = []
    color_index = 0
    for character in title:
        if character == ":":
            spans.append(Span(character, style(theme, Semantic.MUTED)))
        else:
            rgb = theme.title_spectrum[color_index % len(theme.title_spectrum)]
            spans.append(
                Span(
                    character,
                    Style()
                    .fg(Color.rgb(*rgb))
                    .bg(Color.rgb(*theme.color("background")))
                    .add_modifier(Modifier.bold()),
                )
            )
            color_index += 1
    line = Line(spans)
    return line.centered() if centered else line


def text_lines(
    lines: Iterable[tuple[str, Semantic | str]], theme: Theme,
) -> Text:
    return Text(
        [Line([Span(value, style(theme, semantic))]) for value, semantic in lines]
    )


def centered_message(theme: Theme, message: str, semantic: Semantic | str) -> Paragraph:
    return Paragraph(
        Text([Line([Span(message, style(theme, semantic, bold=True))]).centered()])
    ).alignment("center")


def title_paragraph(
    theme: Theme, *, centered: bool = False, title: str = TITLE
) -> Paragraph:
    line = spectral_title(theme, centered=centered, title=title)
    return Paragraph(Text([line])).alignment(
        "center" if centered else "left"
    )
