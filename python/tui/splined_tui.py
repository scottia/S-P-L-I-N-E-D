"""Interactive Ratatui application around the authoritative SPLINED engine."""

from __future__ import annotations

import contextlib
import io
import queue
import re
import signal
import sys
import threading
import time
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Callable

from pyratatui import (
    Block,
    Cell,
    Clear,
    Color,
    Constraint,
    Direction,
    Gauge,
    Layout,
    Line,
    Paragraph,
    Rect,
    Row,
    Span,
    Style,
    Table,
    TableState,
    Terminal,
    Text,
)

from .animation import STARTUP_SECONDS, fit_phrase, startup_frame
from .dialogs import BYPASS_DIALOG, confirm_key
from .keys import Action, map_key, picker_response
from .layout import Breakpoint, layout_spec
from .semantic import Semantic, log_semantic, outcome_semantic, range_semantic
from .status import use_adapter
from .theme import Theme, select_theme
from .widgets import card, panel_style, spectral_title, style, title_paragraph


ANSI_RE = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
OSC_RE = re.compile(r"\x1b\].*?(?:\x07|\x1b\\)")


class TuiInitializationError(RuntimeError):
    """Raised only when terminal setup fails before engine work starts."""


@dataclass
class CandidateView:
    number: int
    source: str
    width: int
    height: int
    format: str
    range_type: str
    distance: int
    square: bool
    acceptable: bool
    approved: bool
    identifier: str
    selected: bool = False
    suggested: bool = False

    @classmethod
    def from_payload(cls, payload: dict[str, Any]) -> "CandidateView":
        return cls(
            number=int(payload.get("number", 0)),
            source=str(payload.get("source", "")),
            width=int(payload.get("width", 0)),
            height=int(payload.get("height", 0)),
            format=str(payload.get("format", "")),
            range_type=str(payload.get("range_type", "")),
            distance=int(payload.get("distance", 0)),
            square=bool(payload.get("square", False)),
            acceptable=bool(payload.get("acceptable", False)),
            approved=bool(payload.get("approved", False)),
            identifier=str(payload.get("id", "")),
            selected=bool(payload.get("selected", False)),
            suggested=bool(payload.get("suggested", False)),
        )


@dataclass
class InputRequest:
    prompt: str
    kind: str
    context: dict[str, Any]


@dataclass
class HistoryEntry:
    timestamp: str
    album: str
    outcome: str


@dataclass
class TuiState:
    started_at: float = field(default_factory=time.monotonic)
    workflow: str = "startup"
    tab: str = "main"
    album_index: int = 0
    album_total: int = 0
    album_path: str = "Preparing scan inventory"
    artist: str = ""
    album: str = ""
    authority: str = ""
    fallback_reason: str = ""
    candidates: list[CandidateView] = field(default_factory=list)
    release_options: list[dict[str, str]] = field(default_factory=list)
    selected_index: int = 0
    input_request: InputRequest | None = None
    input_buffer: str = ""
    logs: list[tuple[str, str]] = field(default_factory=list)
    history: list[HistoryEntry] = field(default_factory=list)
    summary: dict[str, Any] = field(default_factory=dict)
    diagnostics: list[tuple[str, str]] = field(default_factory=list)
    transient: str = ""
    help_open: bool = False
    dialog_open: bool = False
    finished: bool = False
    exit_requested: bool = False
    exit_code: int = 0
    exception: BaseException | None = None

    def apply(self, event: str, payload: dict[str, Any]) -> None:
        if event == "scan_start":
            self.workflow = "overview"
            self.album_total = int(payload.get("total", 0))
            self.album_path = str(payload.get("root", self.album_path))
        elif event == "album":
            self.workflow = "processing"
            self.album_index = int(payload.get("index", 0))
            self.album_total = int(payload.get("total", self.album_total))
            self.album_path = str(payload.get("path", ""))
            self.artist = str(payload.get("artist", ""))
            self.album = str(payload.get("album", ""))
            self.authority = str(payload.get("authority", ""))
            self.fallback_reason = str(payload.get("fallback_reason", ""))
            self.candidates.clear()
            self.release_options.clear()
            self.diagnostics.clear()
        elif event == "candidates":
            self.workflow = "candidates"
            self.candidates = [
                CandidateView.from_payload(item)
                for item in payload.get("items", [])
                if isinstance(item, dict)
            ]
            selected = next(
                (
                    index
                    for index, item in enumerate(self.candidates)
                    if item.suggested or item.selected
                ),
                0,
            )
            self.selected_index = selected
        elif event == "diagnostics":
            self.diagnostics = [
                (str(source), str(message))
                for source, message in payload.get("items", [])
            ]
        elif event == "input":
            self.workflow = "picker"
            context = dict(payload.get("context") or {})
            self.input_request = InputRequest(
                str(payload.get("prompt", "")),
                str(context.get("kind", "picker")),
                context,
            )
            if self.input_request.kind == "musicbrainz":
                self.release_options = [
                    {str(key): str(value) for key, value in item.items()}
                    for item in context.get("options", [])
                    if isinstance(item, dict)
                ]
                self.selected_index = 0
            else:
                self.release_options.clear()
            self.input_buffer = ""
        elif event == "history":
            self.history.append(
                HistoryEntry(
                    datetime.now().strftime("%H:%M"),
                    str(payload.get("album", self.album_path)),
                    str(payload.get("outcome", "processed")),
                )
            )
            self.history = self.history[-200:]
        elif event == "summary":
            self.summary = dict(payload)
            self.workflow = "summary"
        elif event == "log":
            level = str(payload.get("level", "INFO")).upper()
            message = str(payload.get("message", "")).strip()
            if message:
                self.logs.append((level, message))
                self.logs = self.logs[-500:]
        elif event == "worker_done":
            self.finished = True
            self.exit_code = int(payload.get("exit_code", 0))
            self.exception = payload.get("exception")
            self.input_request = None
            if not self.summary:
                self.summary = {"exit_code": self.exit_code}
                self.workflow = "summary"


class TuiAdapter:
    def __init__(self) -> None:
        self.events: queue.Queue[tuple[str, dict[str, Any]]] = queue.Queue()
        self.responses: queue.Queue[str] = queue.Queue()
        self.waiting = threading.Event()

    def emit(self, event: str, payload: dict[str, Any]) -> None:
        self.events.put((event, payload))

    def read(self, prompt: str, context: dict[str, Any]) -> str:
        self.waiting.set()
        self.emit("input", {"prompt": prompt, "context": context})
        try:
            return self.responses.get()
        finally:
            self.waiting.clear()

    def respond(self, response: str) -> None:
        if self.waiting.is_set():
            self.responses.put(response)

    def cancel_wait(self) -> None:
        self.respond("b")


class EventWriter(io.TextIOBase):
    def __init__(self, adapter: TuiAdapter, level: str) -> None:
        self.adapter = adapter
        self.level = level
        self.pending = ""

    def writable(self) -> bool:
        return True

    def isatty(self) -> bool:
        return False

    def write(self, value: str) -> int:
        self.pending += value
        while "\n" in self.pending:
            line, self.pending = self.pending.split("\n", 1)
            self._publish(line)
        return len(value)

    def flush(self) -> None:
        if self.pending:
            self._publish(self.pending)
            self.pending = ""

    def _publish(self, line: str) -> None:
        clean = OSC_RE.sub("", ANSI_RE.sub("", line)).strip()
        if clean:
            level = self.level
            upper = clean.upper()
            if "ERROR" in upper or "FAILED" in upper:
                level = "ERROR"
            elif "WARN" in upper or "FALLBACK" in upper:
                level = "WARN"
            self.adapter.emit("log", {"level": level, "message": clean})


def _run_worker(adapter: TuiAdapter, worker: Callable[[], int]) -> None:
    output = EventWriter(adapter, "INFO")
    errors = EventWriter(adapter, "ERROR")
    result = 1
    failure: BaseException | None = None
    try:
        with use_adapter(adapter), contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            result = int(worker())
    except BaseException as exc:  # carried to the main thread after cleanup
        failure = exc
        errors.write(f"{type(exc).__name__}: {exc}\n")
    finally:
        output.flush()
        errors.flush()
        adapter.emit(
            "worker_done",
            {"exit_code": result if failure is None else 1, "exception": failure},
        )


def _background(theme: Theme) -> Block:
    return Block().style(
        Style()
        .fg(Color.rgb(*theme.color("text")))
        .bg(Color.rgb(*theme.color("background")))
    )


def _split_vertical(area: Rect, constraints: list[Constraint]) -> list[Rect]:
    return Layout().direction(Direction.Vertical).constraints(constraints).split(area)


def _split_horizontal(area: Rect, constraints: list[Constraint]) -> list[Rect]:
    return Layout().direction(Direction.Horizontal).constraints(constraints).split(area)


def _truncate(value: str, width: int) -> str:
    if width <= 0:
        return ""
    return value if len(value) <= width else value[: max(0, width - 1)] + "…"


def _render_startup(frame: Any, state: TuiState, theme: Theme) -> None:
    area = frame.area
    frame.render_widget(_background(theme), area)
    brand = startup_frame(time.monotonic() - state.started_at)
    rows = _split_vertical(
        area,
        [Constraint.fill(1), Constraint.length(1), Constraint.length(1), Constraint.fill(1)],
    )
    frame.render_widget(title_paragraph(theme, centered=True), rows[1])
    phrase = fit_phrase(brand.phrase, max(0, area.width - 4))
    line = Line(
        [Span((" " * brand.offset) + phrase, style(theme, Semantic.MUTED))]
    ).centered()
    frame.render_widget(Paragraph(Text([line])).alignment("center"), rows[2])


def _render_header(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    point = layout_spec(area.width, area.height).breakpoint
    if point is Breakpoint.WIDE:
        left, right = _split_horizontal(
            area, [Constraint.length(18), Constraint.fill(1)]
        )
        frame.render_widget(title_paragraph(theme), left)
        status = f"{state.album_index} / {state.album_total}" if state.album_total else "READY"
        frame.render_widget(
            Paragraph.from_string(status).right_aligned().style(style(theme, Semantic.ACTIVE)),
            right,
        )
    else:
        frame.render_widget(title_paragraph(theme), area)


def _render_overview(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    rows = _split_vertical(
        area,
        [Constraint.fill(1), Constraint.length(2), Constraint.length(3), Constraint.length(3), Constraint.fill(1)],
    )
    frame.render_widget(
        Paragraph.from_string("SCANNING").centered().style(style(theme, Semantic.ACTIVE, bold=True)),
        rows[1],
    )
    if state.album_total:
        ratio = min(1.0, state.album_index / state.album_total)
        gauge = (
            Gauge()
            .ratio(ratio)
            .label(f"{state.album_index:,} / {state.album_total:,} albums")
            .gauge_style(style(theme, Semantic.ACCEPTED, bold=True))
            .style(panel_style(theme))
            .use_unicode(True)
        )
        frame.render_widget(gauge, rows[2])
    else:
        frame.render_widget(
            Paragraph.from_string("Building album inventory…").centered().style(style(theme, Semantic.MUTED)),
            rows[2],
        )
    current = state.album or state.album_path
    frame.render_widget(
        Paragraph.from_string(_truncate(current, max(1, area.width - 6)))
        .centered()
        .block(card(theme, "CURRENT ALBUM", Semantic.ACTIVE)),
        rows[3],
    )


def _candidate_text(candidate: CandidateView | None) -> str:
    if candidate is None:
        return "N/A"
    return (
        f"{candidate.source}\n"
        f"{candidate.width} × {candidate.height}  {candidate.format.upper()}\n"
        f"{candidate.range_type}  distance {candidate.distance}"
    )


def _render_processing(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    spec = layout_spec(area.width, area.height)
    rows = _split_vertical(area, [Constraint.length(4), Constraint.fill(1)])
    album_label = " • ".join(part for part in (state.artist, state.album) if part) or state.album_path
    frame.render_widget(
        Paragraph.from_string(_truncate(album_label, max(1, area.width - 5)))
        .block(card(theme, "CURRENT ALBUM", Semantic.ACTIVE)),
        rows[0],
    )
    authority_semantic = Semantic.FALLBACK if state.fallback_reason else Semantic.ACCEPTED
    authority_text = state.fallback_reason or state.authority or "Evaluating authority and artwork sources…"
    if spec.stack_cards:
        cards = _split_vertical(rows[1], [Constraint.percentage(50), Constraint.percentage(50)])
    else:
        cards = _split_horizontal(rows[1], [Constraint.percentage(50), Constraint.percentage(50)])
    frame.render_widget(
        Paragraph.from_string(authority_text)
        .wrap(True, True)
        .block(card(theme, "AUTHORITY", authority_semantic)),
        cards[0],
    )
    activity = "Provider and image work continues on the engine worker."
    frame.render_widget(
        Paragraph.from_string(activity)
        .wrap(True, True)
        .block(card(theme, "STATUS", Semantic.ACTIVE)),
        cards[1],
    )


COLUMN_WIDTHS = {
    "#": 4,
    "source": 10,
    "resolution": 13,
    "format": 7,
    "range": 14,
    "distance": 10,
    "square": 8,
    "acceptable": 12,
    "approved": 10,
    "id": 24,
}


def _candidate_cell(candidate: CandidateView, column: str) -> str:
    return {
        "#": "★" if candidate.suggested else str(candidate.number),
        "source": candidate.source,
        "resolution": f"{candidate.width}×{candidate.height}",
        "format": candidate.format.upper(),
        "range": candidate.range_type,
        "distance": str(candidate.distance),
        "square": "✓ yes" if candidate.square else "✕ no",
        "acceptable": "✓ yes" if candidate.acceptable else "✕ no",
        "approved": "✓ yes" if candidate.approved else "✕ no",
        "id": candidate.identifier,
    }[column]


def _render_candidate_table(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    spec = layout_spec(area.width, area.height)
    columns = spec.candidate_columns
    headers = {
        "#": "#",
        "source": "SOURCE",
        "resolution": "RESOLUTION",
        "format": "FORMAT",
        "range": "RANGE TYPE",
        "distance": "DISTANCE",
        "square": "SQUARE",
        "acceptable": "ACCEPTABLE",
        "approved": "APPROVED",
        "id": "ID",
    }
    rows: list[Row] = []
    for candidate in state.candidates:
        semantic = range_semantic(candidate.range_type)
        cells = [
            Cell(_candidate_cell(candidate, column), style(theme, semantic if column == "range" else Semantic.TEXT))
            for column in columns
        ]
        rows.append(Row(cells))
    header = Row(
        [Cell(headers[column], style(theme, Semantic.ACTIVE, bold=True)) for column in columns]
    )
    widths = [Constraint.length(COLUMN_WIDTHS[column]) for column in columns[:-1]]
    widths.append(Constraint.fill(1))
    table = (
        Table(rows, widths, header)
        .block(card(theme, "SOURCE CANDIDATES", Semantic.FALLBACK if state.input_request else Semantic.ACTIVE))
        .column_spacing(1)
        .highlight_symbol("› ")
        .highlight_style(style(theme, Semantic.ACTIVE, bold=True))
    )
    table_state = TableState()
    if state.candidates:
        table_state.select(max(0, min(state.selected_index, len(state.candidates) - 1)))
    frame.render_stateful_table(table, area, table_state)


def _render_musicbrainz(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    spec = layout_spec(area.width, area.height)
    columns = ["#", "artist", "release"]
    widths = [Constraint.length(4), Constraint.percentage(28), Constraint.fill(1)]
    if spec.breakpoint in {Breakpoint.NORMAL, Breakpoint.WIDE}:
        columns.extend(["date", "country", "mbid"])
        widths = [
            Constraint.length(4),
            Constraint.percentage(20),
            Constraint.percentage(25),
            Constraint.length(11),
            Constraint.length(9),
            Constraint.fill(1),
        ]
    header = Row(
        [Cell(value.upper(), style(theme, Semantic.ACTIVE, bold=True)) for value in columns]
    )
    rows: list[Row] = []
    for index, option in enumerate(state.release_options, 1):
        values = {
            "#": str(index),
            "artist": option.get("artist", ""),
            "release": option.get("title", ""),
            "date": option.get("date", ""),
            "country": option.get("country", ""),
            "mbid": option.get("id", ""),
        }
        rows.append(
            Row([Cell(values[column], style(theme, Semantic.TEXT)) for column in columns])
        )
    table = (
        Table(rows, widths, header)
        .block(card(theme, "MUSICBRAINZ RELEASE SEARCH", Semantic.SPECIAL))
        .column_spacing(1)
        .highlight_symbol("› ")
        .highlight_style(style(theme, Semantic.SPECIAL, bold=True))
    )
    table_state = TableState()
    if rows:
        table_state.select(max(0, min(state.selected_index, len(rows) - 1)))
    frame.render_stateful_table(table, area, table_state)


def _render_candidates(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    spec = layout_spec(area.width, area.height)
    if spec.breakpoint is Breakpoint.COMPACT:
        rows = _split_vertical(area, [Constraint.length(4), Constraint.fill(1)])
        label = " • ".join(part for part in (state.artist, state.album) if part) or state.album_path
        frame.render_widget(
            Paragraph.from_string(_truncate(label, max(1, area.width - 5))).block(
                card(theme, "CURRENT ALBUM", Semantic.ACTIVE)
            ),
            rows[0],
        )
        _render_candidate_table(frame, rows[1], state, theme)
        return

    rows = _split_vertical(
        area, [Constraint.length(4), Constraint.length(6), Constraint.fill(1)]
    )
    label = " • ".join(part for part in (state.artist, state.album) if part) or state.album_path
    frame.render_widget(
        Paragraph.from_string(_truncate(label, max(1, area.width - 5))).block(
            card(theme, "CURRENT ALBUM", Semantic.ACTIVE)
        ),
        rows[0],
    )
    local = next((item for item in state.candidates if item.source.lower() in {"local", "embedded", "webp"}), None)
    target = next((item for item in state.candidates if item.selected or item.suggested), None)
    if target is None and state.candidates:
        target = state.candidates[0]
    left, middle, right = _split_horizontal(
        rows[1], [Constraint.percentage(42), Constraint.percentage(16), Constraint.percentage(42)]
    )
    frame.render_widget(
        Paragraph.from_string(_candidate_text(local)).block(card(theme, "LOCAL ART", Semantic.ACTIVE)),
        left,
    )
    result = "EQUAL\nDISTANCE" if local and target and local.distance == target.distance else "COMPARE"
    frame.render_widget(
        Paragraph.from_string(result).centered().block(card(theme, "RESULT", Semantic.HISTORY)),
        middle,
    )
    frame.render_widget(
        Paragraph.from_string(_candidate_text(target)).block(card(theme, "TARGET", Semantic.ACCEPTED if target and target.acceptable else Semantic.FALLBACK)),
        right,
    )
    _render_candidate_table(frame, rows[2], state, theme)


def _render_history(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    lines: list[Line] = []
    entries = state.history[-max(1, area.height - 3) :]
    if not entries:
        lines.append(Line([Span("No completion events in this session yet.", style(theme, Semantic.MUTED))]))
    for index, entry in enumerate(reversed(entries)):
        marker = "●" if index == 0 else "│"
        semantic = outcome_semantic(entry.outcome)
        lines.append(
            Line(
                [
                    Span(f"{marker} {entry.timestamp}  ", style(theme, semantic, bold=index == 0)),
                    Span(_truncate(entry.album, max(8, area.width - 32)), style(theme, Semantic.TEXT)),
                    Span(f"  {entry.outcome.upper()}", style(theme, semantic)),
                ]
            )
        )
    frame.render_widget(
        Paragraph(Text(lines)).block(card(theme, "HISTORY / STATUS", Semantic.HISTORY)),
        area,
    )


def _render_logs(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    lines: list[Line] = []
    for level, message in state.logs[-max(1, area.height - 3) :]:
        semantic = log_semantic(level)
        lines.append(
            Line(
                [
                    Span(f"{level:<5} ", style(theme, semantic, bold=level == "ERROR")),
                    Span(_truncate(message, max(1, area.width - 9)), style(theme, Semantic.TEXT)),
                ]
            )
        )
    frame.render_widget(
        Paragraph(Text(lines)).block(card(theme, "LOGS / DIAGNOSTICS", Semantic.ACTIVE)),
        area,
    )


def _render_summary(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    rows = _split_vertical(area, [Constraint.fill(1), Constraint.length(9), Constraint.fill(1)])
    values = state.summary
    summary_lines = [
        f"Albums     {values.get('albums', '—')}     Resolved   {values.get('resolved', '—')}",
        f"Selected   {values.get('selected', '—')}     Postponed  {values.get('postponed', '—')}",
        f"Unresolved {values.get('unresolved', '—')}     Failed     {values.get('failed', '—')}",
        f"Installed  {values.get('installed', '—')}     Unchanged  {values.get('unchanged', '—')}",
        "",
        f"Exit code: {state.exit_code}   Press Enter or q to close",
    ]
    semantic = Semantic.ACCEPTED if state.exit_code == 0 else Semantic.REJECTED
    frame.render_widget(
        Paragraph.from_string("\n".join(summary_lines))
        .centered()
        .block(card(theme, "FINAL SCAN SUMMARY", semantic)),
        rows[1],
    )


def _footer_text(state: TuiState) -> str:
    if state.transient:
        return state.transient
    if state.input_request:
        kind = state.input_request.kind
        if kind in {"artist", "album", "text"}:
            return f"{state.input_request.prompt}{state.input_buffer}   Enter confirm · Esc keep current"
        if kind == "local-comparison":
            return "↑/↓ choose · Enter exact · S suggested · K keep local · M MusicBrainz · B bypass · ? help"
        if kind == "musicbrainz":
            return "↑/↓ choose release · Enter select · B/Esc back · ? help"
        if kind == "fallback-picker":
            return "↑/↓ choose · Enter exact · S suggested · F edit · M MusicBrainz · B bypass · ? help"
        return "↑/↓ choose · Enter exact · S suggested · B bypass · ? help"
    if state.finished:
        return "Enter / q close · Tab history/logs · ? help"
    return "Tab views · Shift+Tab previous · ? help · Ctrl+C stop"


def _render_help(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    width = min(max(44, area.width - 8), 82)
    height = min(max(12, area.height - 6), 20)
    popup = Rect(
        area.x + max(0, (area.width - width) // 2),
        area.y + max(0, (area.height - height) // 2),
        min(width, area.width),
        min(height, area.height),
    )
    text = (
        "↑ / ↓          navigate\n"
        "← / →          change context\n"
        "Tab / Shift+Tab switch main, history, logs\n"
        "Enter          activate exact selection\n"
        "Esc            close / back\n"
        "S              use suggested candidate\n"
        "K              keep local artwork\n"
        "F              edit fallback artist/album\n"
        "M              MusicBrainz retry/search/pick\n"
        "B              confirm bypass\n"
        "?              close this help\n"
        "Ctrl+C         stop and restore terminal"
    )
    frame.render_widget(Clear(), popup)
    frame.render_widget(
        Paragraph.from_string(text)
        .block(card(theme, "HELP / KEY HINTS", Semantic.ACTIVE))
        .wrap(True, True),
        popup,
    )


def _render_dialog(frame: Any, area: Rect, theme: Theme) -> None:
    width = min(42, area.width)
    height = min(7, area.height)
    popup = Rect(
        area.x + max(0, (area.width - width) // 2),
        area.y + max(0, (area.height - height) // 2),
        width,
        height,
    )
    body = f"{BYPASS_DIALOG.question}\n\n[Y] {BYPASS_DIALOG.accept_label}          [N] {BYPASS_DIALOG.reject_label}"
    frame.render_widget(Clear(), popup)
    frame.render_widget(
        Paragraph.from_string(body)
        .centered()
        .block(card(theme, BYPASS_DIALOG.title, Semantic.FALLBACK)),
        popup,
    )


def render(frame: Any, state: TuiState, theme: Theme) -> None:
    area = frame.area
    spec = layout_spec(area.width, area.height)
    frame.render_widget(_background(theme), area)
    if spec.breakpoint is Breakpoint.MINIMUM:
        frame.render_widget(
            Paragraph.from_string(
                f"S:P:L:I:N:E:D needs at least 48×12 cells.\nCurrent: {area.width}×{area.height}\nResize the terminal to continue."
            )
            .centered()
            .style(style(theme, Semantic.WARNING, bold=True)),
            area,
        )
        return
    if state.workflow == "startup" and time.monotonic() - state.started_at < STARTUP_SECONDS:
        _render_startup(frame, state, theme)
        return
    if state.workflow == "startup":
        state.workflow = "overview"

    rows = _split_vertical(
        area, [Constraint.length(1), Constraint.fill(1), Constraint.length(1)]
    )
    _render_header(frame, rows[0], state, theme)
    content = rows[1]
    if state.tab == "history":
        _render_history(frame, content, state, theme)
    elif state.tab == "logs":
        _render_logs(frame, content, state, theme)
    elif state.workflow == "summary":
        _render_summary(frame, content, state, theme)
    elif state.input_request and state.input_request.kind == "musicbrainz":
        _render_musicbrainz(frame, content, state, theme)
    elif state.workflow in {"candidates", "picker"} and state.candidates:
        _render_candidates(frame, content, state, theme)
    elif state.workflow == "processing":
        _render_processing(frame, content, state, theme)
    else:
        _render_overview(frame, content, state, theme)
    frame.render_widget(
        Paragraph.from_string(_truncate(_footer_text(state), area.width))
        .style(style(theme, Semantic.MUTED)),
        rows[2],
    )
    if state.help_open:
        _render_help(frame, area, state, theme)
    if state.dialog_open:
        _render_dialog(frame, area, theme)


def _submit(state: TuiState, adapter: TuiAdapter, response: str) -> None:
    adapter.respond(response)
    state.input_request = None
    state.input_buffer = ""
    state.dialog_open = False
    state.workflow = "processing"


def handle_key(state: TuiState, adapter: TuiAdapter, event: Any) -> None:
    code = str(event.code)
    if state.dialog_open:
        decision = confirm_key(code)
        if decision is True:
            _submit(state, adapter, "b")
        elif decision is False:
            state.dialog_open = False
        return

    action = map_key(
        code,
        ctrl=bool(getattr(event, "ctrl", False)),
        shift=bool(getattr(event, "shift", False)),
    )
    if state.help_open:
        if action in {Action.HELP, Action.BACK}:
            state.help_open = False
        return
    if action is Action.HELP:
        state.help_open = True
        return
    if state.finished and action in {Action.ACTIVATE, Action.QUIT, Action.BACK}:
        state.exit_requested = True
        return
    if action in {Action.NEXT_REGION, Action.PREVIOUS_REGION}:
        tabs = ["main", "history", "logs"]
        step = -1 if action is Action.PREVIOUS_REGION else 1
        state.tab = tabs[(tabs.index(state.tab) + step) % len(tabs)]
        return

    request = state.input_request
    if request is None:
        if action is Action.QUIT:
            state.transient = "Scan is active; use Ctrl+C to stop safely."
        return

    if request.kind in {"artist", "album", "text"}:
        if action is Action.ACTIVATE:
            _submit(state, adapter, state.input_buffer)
        elif action is Action.BACK:
            _submit(state, adapter, "")
        elif action is Action.DELETE:
            state.input_buffer = state.input_buffer[:-1]
        elif len(code) == 1 and code.isprintable() and not getattr(event, "ctrl", False):
            state.input_buffer += code
        return

    selection_count = (
        len(state.release_options)
        if request.kind == "musicbrainz"
        else len(state.candidates)
    )
    if action is Action.UP and selection_count:
        state.selected_index = (state.selected_index - 1) % selection_count
        state.input_buffer = str(state.selected_index + 1)
        return
    if action is Action.DOWN and selection_count:
        state.selected_index = (state.selected_index + 1) % selection_count
        state.input_buffer = str(state.selected_index + 1)
        return
    if action is Action.DIGIT and selection_count:
        proposed = (state.input_buffer + code)[-2:]
        number = int(proposed)
        if 1 <= number <= selection_count:
            state.input_buffer = proposed
            state.selected_index = number - 1
        elif code != "0" and int(code) <= selection_count:
            state.input_buffer = code
            state.selected_index = int(code) - 1
        return
    if action is Action.BYPASS:
        state.dialog_open = True
        return
    if action is Action.QUIT:
        state.transient = "q is disabled during a decision; choose an engine action or Ctrl+C."
        return
    response = picker_response(action, state.selected_index)
    if response is not None:
        _submit(state, adapter, response)


def _drain(adapter: TuiAdapter, state: TuiState) -> None:
    while True:
        try:
            event, payload = adapter.events.get_nowait()
        except queue.Empty:
            return
        state.apply(event, payload)


def run_tui(worker: Callable[[], int], theme_name: str = "OLED") -> int:
    """Run ``worker`` behind Ratatui and return the engine's exit code.

    ``Terminal`` is always used as a context manager; all exits pass through
    its restore path before an engine exception is re-raised.
    """
    theme = select_theme(theme_name)
    state = TuiState()
    adapter = TuiAdapter()
    thread = threading.Thread(
        target=_run_worker,
        args=(adapter, worker),
        name="splined-engine",
        daemon=True,
    )
    terminated = False
    previous_sigterm: Any = None

    def on_sigterm(signum: int, frame: Any) -> None:
        nonlocal terminated
        terminated = True
        adapter.cancel_wait()

    if threading.current_thread() is threading.main_thread() and hasattr(signal, "SIGTERM"):
        previous_sigterm = signal.getsignal(signal.SIGTERM)
        signal.signal(signal.SIGTERM, on_sigterm)

    try:
        try:
            terminal = Terminal()
            with terminal:
                thread.start()
                while not state.exit_requested and not terminated:
                    _drain(adapter, state)
                    terminal.draw(lambda frame: render(frame, state, theme))
                    key = terminal.poll_event(timeout_ms=80)
                    if key is not None:
                        handle_key(state, adapter, key)
        except BaseException as exc:
            if thread.ident is None:
                raise TuiInitializationError(
                    f"Ratatui terminal initialization failed: {exc}"
                ) from exc
            raise
    finally:
        adapter.cancel_wait()
        if previous_sigterm is not None:
            signal.signal(signal.SIGTERM, previous_sigterm)

    if terminated:
        return 143
    if state.exception is not None:
        raise state.exception
    return state.exit_code
