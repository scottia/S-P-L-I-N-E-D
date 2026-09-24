"""Interactive Ratatui application around the authoritative SPLINED engine."""

from __future__ import annotations

import contextlib
import io
import json
import queue
import re
import signal
import sys
import threading
import time
import webbrowser
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
from .aispline import (
    AISPLINE_TITLE,
    SPLINED_TITLE,
    AiActivityState,
    AiCandidate,
    EnhancementSelection,
)
from .dialogs import BYPASS_DIALOG, confirm_key
from .keys import Action, map_key, picker_response
from .library import (
    STATUS_LABELS,
    AlbumStatus,
    ArtistStatus,
    LibraryModel,
    artist_status,
)
from .layout import Breakpoint, layout_spec
from .semantic import Semantic, log_semantic, outcome_semantic, range_semantic
from .status import use_adapter
from .source_settings import PolicyDraft, PRIMARY_METADATA_SOURCES
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
    url: str = ""
    provenance: str = "[URL]"
    comparison: str = ""
    crop_risk: str = "equal"
    ai_review: bool | None = None
    ai_key: str = ""

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
            url=str(payload.get("url", "")),
            provenance=str(payload.get("provenance", "[URL]")),
            comparison=str(payload.get("comparison", "")),
            crop_risk=str(payload.get("crop_risk", "equal")),
            ai_review=(
                bool(payload["ai_splined"])
                if isinstance(payload.get("ai_splined"), bool)
                else None
            ),
            ai_key=str(payload.get("ai_key", payload.get("number", ""))),
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
class ActivityEntry:
    category: str
    state: str
    source: str
    message: str


@dataclass
class TuiState:
    started_at: float = field(default_factory=time.monotonic)
    workflow: str = "startup"
    tab: str = "main"
    album_index: int = 0
    album_total: int = 0
    album_path: str = "Preparing scan inventory"
    phase: str = "inventory"
    artist: str = ""
    album: str = ""
    authority: str = ""
    fallback_reason: str = ""
    track_count: int = 0
    compilation: str = ""
    mbid: str = ""
    tag_state: str = ""
    candidates: list[CandidateView] = field(default_factory=list)
    release_options: list[dict[str, str]] = field(default_factory=list)
    selected_index: int = 0
    input_request: InputRequest | None = None
    input_buffer: str = ""
    logs: list[tuple[str, str]] = field(default_factory=list)
    history: list[HistoryEntry] = field(default_factory=list)
    summary: dict[str, Any] = field(default_factory=dict)
    diagnostics: list[tuple[str, str]] = field(default_factory=list)
    activity: list[ActivityEntry] = field(default_factory=list)
    library: LibraryModel | None = None
    policy: PolicyDraft | None = None
    workspace: str = "library"
    library_focus: int = 0
    status_index: int = 0
    select_index: int = 0
    scan_index: int = 3
    artist_index: int = 0
    album_index_cursor: int = 0
    group_scroll: int = 0
    filter_edit: str = ""
    ai_enabled: bool = False
    ai_runtime_available: bool = False
    ai_selection: EnhancementSelection | None = None
    ai_activity: AiActivityState = field(default_factory=AiActivityState)
    upscale_below_ideal: bool = False
    dialog_kind: str = ""
    transient: str = ""
    help_open: bool = False
    dialog_open: bool = False
    finished: bool = False
    exit_requested: bool = False
    exit_code: int = 0
    exception: BaseException | None = None

    def apply(self, event: str, payload: dict[str, Any]) -> None:
        if event == "library":
            self.workflow = "library"
            self.workspace = "library"
            self.library = LibraryModel.from_payload(payload)
            config = payload.get("config", {})
            if isinstance(config, dict):
                self.policy = PolicyDraft.from_config(config)
                ai = payload.get("aisplined", config.get("aisplined", {}))
                output = config.get("output", {})
                if isinstance(ai, dict):
                    self.ai_enabled = bool(ai.get("enabled", False))
                    self.ai_runtime_available = bool(payload.get("ai_runtime_available", False))
                if isinstance(output, dict):
                    self.upscale_below_ideal = bool(output.get("upscale_below_ideal", False))
        elif event == "scan_start":
            self.workflow = "overview"
            self.album_total = int(payload.get("total", 0))
            self.album_path = str(payload.get("root", self.album_path))
            self.phase = str(payload.get("phase", "authority"))
        elif event == "album":
            self.workflow = "processing"
            self.album_index = int(payload.get("index", 0))
            self.album_total = int(payload.get("total", self.album_total))
            self.album_path = str(payload.get("path", ""))
            self.artist = str(payload.get("artist", ""))
            self.album = str(payload.get("album", ""))
            self.authority = str(payload.get("authority", ""))
            self.fallback_reason = str(payload.get("fallback_reason", ""))
            self.track_count = int(payload.get("track_count", 0) or 0)
            self.compilation = str(payload.get("compilation", ""))
            self.mbid = str(payload.get("mbid", ""))
            self.tag_state = str(payload.get("tag_state", ""))
            self.phase = str(payload.get("phase", "processing"))
            self.candidates.clear()
            self.release_options.clear()
            self.diagnostics.clear()
            self.activity.clear()
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
            ai = payload.get("aisplined", {})
            if isinstance(ai, dict):
                self.ai_enabled = bool(ai.get("enabled", self.ai_enabled))
                self.ai_runtime_available = bool(
                    payload.get("ai_runtime_available", self.ai_runtime_available)
                )
                if self.ai_enabled:
                    selection = EnhancementSelection(
                        enabled=True,
                        runtime_available=self.ai_runtime_available,
                        minimum_short_side=int(ai.get("minimum_short_side", 600)),
                        allow_below_minimum_override=bool(
                            ai.get("allow_below_minimum_override", False)
                        ),
                        ideal=int(payload.get("ideal", 1800)),
                    )
                    for candidate in self.candidates:
                        key = candidate.ai_key or str(candidate.number)
                        selection.register(
                            AiCandidate(
                                key,
                                min(candidate.width, candidate.height),
                                candidate.provenance,
                                candidate.ai_review,
                            )
                        )
                    self.ai_selection = selection
        elif event == "diagnostics":
            self.diagnostics = [
                (str(source), str(message))
                for source, message in payload.get("items", [])
            ]
        elif event == "input":
            context = dict(payload.get("context") or {})
            if str(context.get("kind", "picker")) == "library-selection":
                self.workflow = "library"
            else:
                self.workflow = "picker"
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
        elif event == "activity":
            message = str(payload.get("message", "")).strip()
            if message:
                self.activity.append(
                    ActivityEntry(
                        str(payload.get("category", "engine")),
                        str(payload.get("state", "info")),
                        str(payload.get("source", "")),
                        message,
                    )
                )
                self.activity = self.activity[-500:]
        elif event == "aispline_activity":
            # No event is synthesized here. A future real adapter is the only
            # authority allowed to publish this event.
            self.ai_activity.apply(payload)
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
    title = (
        AISPLINE_TITLE
        if state.ai_activity.active
        or bool(state.ai_selection and state.ai_selection.selected_key)
        else SPLINED_TITLE
    )
    if point is Breakpoint.WIDE:
        left, right = _split_horizontal(
            area, [Constraint.length(len(title) + 1), Constraint.fill(1)]
        )
        frame.render_widget(title_paragraph(theme, title=title), left)
        status = f"{state.album_index} / {state.album_total}" if state.album_total else "READY"
        frame.render_widget(
            Paragraph.from_string(status).right_aligned().style(style(theme, Semantic.ACTIVE)),
            right,
        )
    else:
        frame.render_widget(title_paragraph(theme, title=title), area)


STATUS_CONTROLS: tuple[tuple[str, AlbumStatus | ArtistStatus], ...] = (
    ("Unprocessed", AlbumStatus.UNPROCESSED),
    ("Processed", AlbumStatus.PROCESSED),
    ("Bypass", AlbumStatus.BYPASSED),
    ("Partial / Timeout", AlbumStatus.TIMEOUT),
    ("Artist Complete", ArtistStatus.COMPLETE),
    ("Artist Contains Bypass", ArtistStatus.CONTAINS_BYPASS),
)
SELECT_CONTROLS = ("Select [ALL]", "Select [NONE]", "Select [FILTERED]")
SCAN_CONTROLS = (
    "Filter Scan [READ]",
    "Filter Scan [WRITE]",
    "Auto Scan [ALL]",
    "Auto Scan [SELECTED]",
)


def _album_status_semantic(status: AlbumStatus) -> Semantic:
    return {
        AlbumStatus.UNPROCESSED: Semantic.TEXT,
        AlbumStatus.PROCESSED: Semantic.FALLBACK,
        AlbumStatus.BYPASSED: Semantic.REJECTED,
        AlbumStatus.TIMEOUT: Semantic.HISTORY,
    }[status]


def _artist_status_semantic(status: ArtistStatus) -> Semantic:
    return {
        ArtistStatus.UNPROCESSED: Semantic.TEXT,
        ArtistStatus.PARTIAL: Semantic.HISTORY,
        ArtistStatus.COMPLETE: Semantic.ACCEPTED,
        ArtistStatus.CONTAINS_BYPASS: Semantic.DEBUG,
    }[status]


def _control_lines(
    labels: tuple[str, ...],
    selected: int,
    active: Callable[[int], bool],
    theme: Theme,
    suffix: Callable[[int], str] | None = None,
) -> Text:
    lines: list[Line] = []
    for index, label in enumerate(labels):
        marker = "›" if index == selected else " "
        checked = "☑" if active(index) else "☐"
        semantic = Semantic.ACTIVE if index == selected else (
            Semantic.ACCEPTED if active(index) else Semantic.MUTED
        )
        extra = suffix(index) if suffix is not None else ""
        lines.append(Line([Span(f"{marker} {checked} {label}{extra}", style(theme, semantic, bold=index == selected))]))
    return Text(lines)


def _render_library_controls(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    model = state.library
    assert model is not None
    if layout_spec(area.width, area.height).stack_cards:
        panels = _split_vertical(
            area, [Constraint.length(8), Constraint.length(5), Constraint.length(6)]
        )
    else:
        panels = _split_horizontal(
            area, [Constraint.percentage(38), Constraint.percentage(27), Constraint.fill(1)]
        )

    def status_active(index: int) -> bool:
        status = STATUS_CONTROLS[index][1]
        return (
            status in model.status_filters
            if isinstance(status, AlbumStatus)
            else status in model.artist_status_filters
        )

    album_counts = {
        status: sum(item.status is status for item in model.albums)
        for status in AlbumStatus
    }
    grouped: dict[str, list[Any]] = {}
    for item in model.albums:
        grouped.setdefault(item.artist, []).append(item)
    artist_counts = {
        status: sum(artist_status(items) is status for items in grouped.values())
        for status in ArtistStatus
    }

    def status_suffix(index: int) -> str:
        status = STATUS_CONTROLS[index][1]
        count = album_counts[status] if isinstance(status, AlbumStatus) else artist_counts[status]
        return f"  [{count:,}]"

    selection_counts = (
        len(model.albums),
        0,
        len(model.visible_albums()),
    )

    frame.render_widget(
        Paragraph(_control_lines(tuple(x[0] for x in STATUS_CONTROLS), state.status_index, status_active, theme, status_suffix))
        .block(card(theme, "ALBUM STATUS MODE", Semantic.ACTIVE)),
        panels[0],
    )
    frame.render_widget(
        Paragraph(_control_lines(SELECT_CONTROLS, state.select_index, lambda _i: False, theme, lambda index: f"  [{selection_counts[index]:,}]"))
        .block(card(theme, "ALBUM SELECT MODE", Semantic.SPECIAL)),
        panels[1],
    )
    frame.render_widget(
        Paragraph(_control_lines(SCAN_CONTROLS, state.scan_index, lambda i: i == state.scan_index, theme))
        .block(card(theme, "SCAN MODE", Semantic.ACCEPTED)),
        panels[2],
    )


def _visible_window(items: list[Any], selected: int, height: int) -> tuple[list[Any], int]:
    size = max(1, height)
    selected = max(0, min(selected, max(0, len(items) - 1)))
    start = max(0, min(selected - size // 2, max(0, len(items) - size)))
    return items[start : start + size], start


def _render_artist_picker(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    model = state.library
    assert model is not None
    rows = model.visible_artists()
    body, filter_area = _split_vertical(area, [Constraint.fill(1), Constraint.length(3)])
    visible, start = _visible_window(rows, state.artist_index, max(1, body.height - 2))
    lines: list[Line] = []
    for offset, artist in enumerate(visible):
        index = start + offset
        marker = "›" if index == state.artist_index else " "
        checked = "☑" if artist.selected_count else "☐"
        semantic = _artist_status_semantic(artist.status)
        lines.append(Line([
            Span(f"{marker} {checked} ", style(theme, Semantic.ACTIVE if index == state.artist_index else semantic, bold=index == state.artist_index)),
            Span(_truncate(artist.name, max(4, body.width - 27)), style(theme, semantic)),
            Span(f"  {STATUS_LABELS[artist.status]} {artist.selected_count}/{artist.album_count}", style(theme, semantic)),
        ]))
    if not lines:
        lines.append(Line([Span("No artists match the active filters.", style(theme, Semantic.MUTED))]))
    frame.render_widget(
        Paragraph(Text(lines)).block(card(theme, f"ARTIST PICKER · {len(rows)} VISIBLE", Semantic.FALLBACK)),
        body,
    )
    filter_text = f"{model.artist_filter}{'▌' if state.library_focus == 4 else ''}"
    frame.render_widget(
        Paragraph.from_string(filter_text).block(card(theme, "ARTIST FILTER · TYPE TO FILTER", Semantic.SPECIAL if state.library_focus == 4 else Semantic.ACTIVE)),
        filter_area,
    )


def _render_album_picker(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    model = state.library
    assert model is not None
    rows = model.visible_albums(active_artist_only=True)
    body, filter_area = _split_vertical(area, [Constraint.fill(1), Constraint.length(3)])
    visible, start = _visible_window(rows, state.album_index_cursor, max(1, body.height - 2))
    lines: list[Line] = []
    for offset, album in enumerate(visible):
        index = start + offset
        marker = "›" if index == state.album_index_cursor else " "
        checked = "☑" if album.selected else "☐"
        semantic = _album_status_semantic(album.status)
        lines.append(Line([
            Span(f"{marker} {checked} ", style(theme, Semantic.ACTIVE if index == state.album_index_cursor else semantic, bold=index == state.album_index_cursor)),
            Span(_truncate(album.title, max(4, body.width - 23)), style(theme, semantic)),
            Span(f"  {STATUS_LABELS[album.status]}", style(theme, semantic)),
        ]))
    if not lines:
        lines.append(Line([Span("No albums match the active filters.", style(theme, Semantic.MUTED))]))
    frame.render_widget(
        Paragraph(Text(lines)).block(card(theme, f"ALBUM PICKER · {len(rows)} VISIBLE", Semantic.ACTIVE)),
        body,
    )
    filter_text = f"{model.album_filter}{'▌' if state.library_focus == 6 else ''}"
    frame.render_widget(
        Paragraph.from_string(filter_text).block(card(theme, "ALBUM FILTER · TYPE TO FILTER", Semantic.SPECIAL if state.library_focus == 6 else Semantic.ACTIVE)),
        filter_area,
    )


def _render_library_stats(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    assert state.library is not None
    stats = state.library.statistics()
    formats = "  ".join(f"{key} {value}" for key, value in sorted(stats["formats"].items())) or "none"
    text = (
        f"Path  {_truncate(str(stats['path']), max(8, area.width - 9))}\n"
        f"Artists  {stats['artists']} total · {stats['visible_artists']} visible\n"
        f"Albums   {stats['albums']} total · {stats['visible_albums']} visible\n"
        f"Selected {stats['selected']}\n"
        f"Artwork  {formats}\n"
        "[P] Source Policy Settings"
    )
    frame.render_widget(
        Paragraph.from_string(text).block(card(theme, "MEDIA LIBRARY STATISTICS", Semantic.ACTIVE)), area
    )


def _render_library(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    if state.library is None:
        _render_overview(frame, area, state, theme)
        return
    spec = layout_spec(area.width, area.height)
    if spec.stack_cards:
        sections = _split_vertical(
            area,
            [Constraint.length(19), Constraint.percentage(35), Constraint.percentage(35), Constraint.fill(1)],
        )
        _render_library_controls(frame, sections[0], state, theme)
        _render_artist_picker(frame, sections[1], state, theme)
        _render_album_picker(frame, sections[2], state, theme)
        _render_library_stats(frame, sections[3], state, theme)
    else:
        top, bottom = _split_vertical(area, [Constraint.length(8), Constraint.fill(1)])
        _render_library_controls(frame, top, state, theme)
        columns = _split_horizontal(
            bottom,
            [Constraint.percentage(34), Constraint.percentage(38), Constraint.fill(1)],
        )
        _render_artist_picker(frame, columns[0], state, theme)
        _render_album_picker(frame, columns[1], state, theme)
        _render_library_stats(frame, columns[2], state, theme)


def _render_library_embedded(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    """Keep the Select Media workspace visible above active processing."""
    top, bottom = _split_vertical(area, [Constraint.length(8), Constraint.fill(1)])
    _render_library_controls(frame, top, state, theme)
    columns = _split_horizontal(
        bottom,
        [Constraint.percentage(32), Constraint.percentage(39), Constraint.fill(1)],
    )
    _render_artist_picker(frame, columns[0], state, theme)
    _render_album_picker(frame, columns[1], state, theme)
    _render_library_stats(frame, columns[2], state, theme)


def _render_overview(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    rows = _split_vertical(
        area,
        [Constraint.fill(1), Constraint.length(2), Constraint.length(3), Constraint.length(3), Constraint.fill(1)],
    )
    frame.render_widget(
        Paragraph.from_string(f"SCANNING · {state.phase.upper()}").centered().style(style(theme, Semantic.ACTIVE, bold=True)),
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
    if state.library is not None and spec.breakpoint is Breakpoint.WIDE and area.height >= 30:
        library_area, area = _split_vertical(
            area, [Constraint.length(16), Constraint.fill(1)]
        )
        _render_library_embedded(frame, library_area, state, theme)
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
    _render_activity_region(frame, cards[1], state, theme)


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
    "url": 12,
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
        "url": candidate.provenance or ("[URL]" if candidate.url else "N/A"),
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
        "url": "URL",
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


def _candidate_line(candidate: CandidateView, state: TuiState, theme: Theme) -> Line:
    selected = state.candidates and state.candidates[state.selected_index] is candidate
    marker = "›" if selected else " "
    star = "★" if candidate.suggested else str(candidate.number)
    semantic = range_semantic(candidate.range_type)
    facts = (
        f"{marker}{star:>2} {candidate.width}×{candidate.height} "
        f"{candidate.format.upper():<5} {candidate.range_type:<13} "
        f"Δ{candidate.distance:<5} "
        f"Square {'yes' if candidate.square else 'no'} · "
        f"Accept {'yes' if candidate.acceptable else 'no'} · "
        f"Approved {'yes' if candidate.approved else 'no'} · "
    )
    spans = [Span(facts, style(theme, Semantic.ACTIVE if selected else semantic, bold=selected))]
    provenance = candidate.provenance or "[URL]"
    provenance_style = style(
        theme,
        Semantic.DEBUG if provenance == "[URL]" else Semantic.SPECIAL if provenance == "[Enhanced]" else Semantic.ACCEPTED,
        bold=True,
    )
    if provenance == "[URL]":
        provenance_style = provenance_style.underlined()
    spans.append(Span(provenance, provenance_style))
    if state.ai_enabled:
        review = "yes" if candidate.ai_review is True else "no" if candidate.ai_review is False else "unavailable"
        enhancement = state.ai_selection.label(candidate.ai_key) if state.ai_selection else "N/A"
        ai_semantic = (
            Semantic.DISABLED
            if enhancement == "N/A" or (state.ai_selection and state.ai_selection.disabled(candidate.ai_key))
            else Semantic.SPECIAL
        )
        spans.extend([
            Span(f" · AI SPLINED {review} · ", style(theme, Semantic.SPECIAL if candidate.ai_review is not None else Semantic.DISABLED)),
            Span(f"AI ENHANCED {enhancement}", style(theme, ai_semantic)),
        ])
    return Line(spans)


def _candidate_header(state: TuiState, theme: Theme) -> Line:
    prefix = " #  RESOLUTION   FORMAT RANGE TYPE    DISTANCE SQUARE · ACCEPTABLE · APPROVED · URL"
    if state.ai_enabled:
        prefix += " · AI SPLINED · AI ENHANCED"
    return Line([Span(prefix, style(theme, Semantic.ACTIVE, bold=True))])


def _candidate_groups(state: TuiState) -> list[tuple[str, list[CandidateView], Semantic]]:
    grouped: dict[str, list[CandidateView]] = {}
    order: list[str] = []
    for candidate in state.candidates:
        # The engine's suggested candidate has a dedicated always-visible
        # section. Keep it out of its provider group to avoid presenting the
        # same actionable row twice; other candidates from that source remain.
        if candidate.suggested:
            continue
        source_key = candidate.source.casefold()
        if candidate.provenance == "[Enhanced]" or source_key == "enhanced":
            name = "ENHANCED"
        elif candidate.provenance == "[LOCAL]" or source_key in {"local", "embedded", "webp", "webpstill"}:
            name = "LOCAL"
        else:
            name = candidate.source or "UNKNOWN"
        if name not in grouped:
            grouped[name] = []
            order.append(name)
        grouped[name].append(candidate)
    result: list[tuple[str, list[CandidateView], Semantic]] = []
    for name in order:
        semantic = Semantic.ACTIVE
        if name == "LOCAL":
            semantic = Semantic.DEBUG
        elif name == "ENHANCED":
            semantic = Semantic.SPECIAL
        elif not any(item.acceptable for item in grouped[name]):
            semantic = Semantic.REJECTED
        elif any(item.range_type == "Ideal" for item in grouped[name]):
            semantic = Semantic.ACCEPTED
        elif any(item.range_type in {"LowerRange", "BelowMinimum"} for item in grouped[name]):
            semantic = Semantic.FALLBACK
        result.append((name, grouped[name], semantic))
    return result


def _render_activity(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    lines: list[Line] = []
    for item in state.activity[-max(1, area.height - 2) :]:
        semantic = {
            "error": Semantic.REJECTED,
            "skipped": Semantic.WARNING,
            "done": Semantic.ACCEPTED,
            "start": Semantic.ACTIVE,
            "retry": Semantic.FALLBACK,
        }.get(item.state.lower(), Semantic.TEXT)
        prefix = f"{item.source}: " if item.source else ""
        lines.append(Line([Span("• ", style(theme, semantic)), Span(_truncate(prefix + item.message, max(1, area.width - 5)), style(theme, semantic))]))
    if not lines:
        lines.append(Line([Span("Waiting for engine activity…", style(theme, Semantic.MUTED))]))
    frame.render_widget(Paragraph(Text(lines)).block(card(theme, "LIVE ACTIVITY / STATUS", Semantic.ACTIVE)), area)


def _render_ai_activity(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    activity = state.ai_activity
    phase = activity.phase.value if activity.phase is not None else "WAITING"
    if phase == "SUCCESS":
        headline = "✓ AISPLINE SUCCESS"
        semantic = Semantic.ACCEPTED
    elif phase == "REJECTED":
        headline = "✕ AISPLINE REJECTED"
        semantic = Semantic.REJECTED
    else:
        headline = phase
        semantic = Semantic.SPECIAL
    lines = [headline]
    if activity.message:
        lines.append(activity.message)
    if activity.reason:
        lines.extend([f"reason: {activity.reason}", "original candidate retained"])
    frame.render_widget(
        Paragraph.from_string("\n".join(lines)).block(
            card(theme, "AISPLINE ACTIVITY", semantic)
        ),
        area,
    )


def _render_activity_region(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    if state.ai_enabled and state.ai_activity.active and area.width >= 80:
        left, right = _split_horizontal(
            area, [Constraint.percentage(62), Constraint.fill(1)]
        )
        _render_activity(frame, left, state, theme)
        _render_ai_activity(frame, right, state, theme)
    elif state.ai_enabled and state.ai_activity.active:
        top, bottom = _split_vertical(
            area, [Constraint.percentage(55), Constraint.fill(1)]
        )
        _render_activity(frame, top, state, theme)
        _render_ai_activity(frame, bottom, state, theme)
    else:
        _render_activity(frame, area, state, theme)


def _render_candidates(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    spec = layout_spec(area.width, area.height)
    if state.library is not None and spec.breakpoint is Breakpoint.WIDE and area.height >= 36:
        library_area, area = _split_vertical(
            area, [Constraint.length(16), Constraint.fill(1)]
        )
        _render_library_embedded(frame, library_area, state, theme)
    target = next((item for item in state.candidates if item.suggested or item.selected), None)
    if target is None and state.candidates:
        target = state.candidates[0]
    local = next((item for item in state.candidates if item.provenance == "[LOCAL]"), None)
    comparison = "No local comparison"
    if local and target:
        if target.comparison:
            comparison = target.comparison
        elif local.distance == target.distance:
            comparison = "Equal distance to Ideal · Shape equal · Crop risk equal"
        else:
            toward = local.distance - target.distance
            comparison = f"{target.source} {toward:+d}px toward Ideal · Shape {'equal' if local.square == target.square else 'changed'} · Crop risk {target.crop_risk}"

    if spec.stack_cards:
        top = _split_vertical(area, [Constraint.length(3), Constraint.length(3), Constraint.length(3), Constraint.length(4), Constraint.length(4), Constraint.fill(1), Constraint.length(5)])
        summary_areas = [top[0], top[1], top[2]]
        current, preferred, groups_area, activity_area = top[3], top[4], top[5], top[6]
        label = " • ".join(part for part in (state.artist, state.album) if part) or state.album_path
        details = f"Tracks {state.track_count or '—'} · Compilation {state.compilation or '—'} · Authority {state.authority or '—'} · Tagged {state.tag_state or '—'}"
        current_text = _truncate(label, max(1, area.width - 5)) + "\n" + _truncate(details, max(1, area.width - 5))
        frame.render_widget(Paragraph.from_string(current_text).block(card(theme, "CURRENT ALBUM", Semantic.ACTIVE)), current)
        preferred_lines = (
            [Line([Span("No suggested candidate", style(theme, Semantic.MUTED))])]
            if target is None
            else [
                Line([Span(f"★★ (S) Suggested {target.source}", style(theme, Semantic.FALLBACK, bold=True))]),
                _candidate_line(target, state, theme),
            ]
        )
        preferred_semantic = Semantic.ACCEPTED if target and target.range_type == "Ideal" else Semantic.FALLBACK
        frame.render_widget(Paragraph(Text(preferred_lines)).block(card(theme, "PREFERRED SOURCE CANDIDATE", preferred_semantic)), preferred)
    else:
        top, current, preferred, groups_area, activity_area = _split_vertical(
            area,
            [Constraint.length(4), Constraint.length(4), Constraint.length(4), Constraint.fill(1), Constraint.length(6)],
        )
        summary_areas = _split_horizontal(top, [Constraint.percentage(27), Constraint.percentage(46), Constraint.fill(1)])
        label = " • ".join(part for part in (state.artist, state.album) if part) or state.album_path
        details = f"Tracks {state.track_count or '—'} · Compilation {state.compilation or '—'} · Authority {state.authority or '—'} · Tagged {state.tag_state or '—'}"
        current_text = _truncate(label, max(1, area.width - 5)) + "\n" + _truncate(details, max(1, area.width - 5))
        frame.render_widget(Paragraph.from_string(current_text).block(card(theme, "CURRENT ALBUM", Semantic.ACTIVE)), current)
        preferred_lines = (
            [Line([Span("No suggested candidate", style(theme, Semantic.MUTED))])]
            if target is None
            else [
                Line([Span(f"★★ (S) Suggested {target.source}", style(theme, Semantic.FALLBACK, bold=True))]),
                _candidate_line(target, state, theme),
            ]
        )
        preferred_semantic = Semantic.ACCEPTED if target and target.range_type == "Ideal" else Semantic.FALLBACK
        frame.render_widget(Paragraph(Text(preferred_lines)).block(card(theme, "PREFERRED SOURCE CANDIDATE", preferred_semantic)), preferred)

    selected_albums = (
        [item for item in state.library.albums if item.selected]
        if state.library is not None
        else []
    )
    if selected_albums:
        selected_text = "\n".join(
            f"{'›' if item.title == state.album else ' '} {item.artist} · {item.title}"
            for item in selected_albums[: max(1, summary_areas[0].height - 2)]
        )
        selected_title = f"SELECTED ALBUM(S) · {state.album_index} OF {len(selected_albums)}"
    else:
        selected_text = f"{state.album_index} / {state.album_total}" if state.album_total else "None selected"
        selected_title = "SELECTED ALBUM(S)"
    frame.render_widget(Paragraph.from_string(selected_text).block(card(theme, selected_title, Semantic.SPECIAL)), summary_areas[0])
    frame.render_widget(Paragraph.from_string(_truncate(comparison, max(1, summary_areas[1].width - 4))).block(card(theme, "RESULT", Semantic.HISTORY)), summary_areas[1])
    target_text = "Ideal target" if target is None else f"Ideal · distance {target.distance} · {target.width}×{target.height}"
    frame.render_widget(Paragraph.from_string(target_text).block(card(theme, "TARGET", Semantic.ACCEPTED)), summary_areas[2])

    groups = _candidate_groups(state)
    if groups:
        state.group_scroll = max(0, min(state.group_scroll, len(groups) - 1))
        visible_groups: list[tuple[str, list[CandidateView], Semantic]] = []
        heights: list[int] = []
        remaining = max(4, groups_area.height)
        for group in groups[state.group_scroll :]:
            wanted = min(8, len(group[1]) + 3)
            if visible_groups and remaining < 4:
                break
            height = min(wanted, remaining) if not visible_groups else min(wanted, max(4, remaining))
            visible_groups.append(group)
            heights.append(max(4, height))
            remaining -= max(4, height)
            if remaining < 4:
                break
        panels = _split_vertical(
            groups_area, [Constraint.length(value) for value in heights]
        )
        for panel, (name, candidates, semantic) in zip(panels, visible_groups):
            selected_at = next(
                (
                    index
                    for index, item in enumerate(candidates)
                    if state.candidates.index(item) == state.selected_index
                ),
                0,
            )
            row_capacity = max(1, panel.height - 3)
            row_start = max(
                0,
                min(selected_at - row_capacity // 2, max(0, len(candidates) - row_capacity)),
            )
            shown = candidates[row_start : row_start + row_capacity]
            suffix = f" · {len(candidates)} CANDIDATE(S)"
            if len(candidates) > len(shown):
                suffix += f" · ROWS {row_start + 1}-{row_start + len(shown)}/{len(candidates)}"
            if len(groups) > len(visible_groups):
                suffix += f" · GROUP {state.group_scroll + 1}/{len(groups)}"
            frame.render_widget(
                Paragraph(Text([_candidate_header(state, theme)] + [_candidate_line(item, state, theme) for item in shown])).block(
                    card(theme, f"SOURCE CANDIDATES {name.upper()}{suffix}", semantic)
                ),
                panel,
            )
    else:
        frame.render_widget(Paragraph.from_string("No candidates returned.").block(card(theme, "SOURCE CANDIDATES", Semantic.WARNING)), groups_area)
    _render_activity_region(frame, activity_area, state, theme)


POLICY_FIELDS = (
    "enabled",
    "source_override",
    "minimum_range_type",
    "allow_below_minimum_fallback",
    "minimum_short_side",
    "maximum_short_side",
    "minimum_width",
    "minimum_height",
    "primary_image_only",
)
GLOBAL_POLICY_FIELDS = ("min", "ideal", "max", "ladder", "square_round_to")


def _render_policy(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    draft = state.policy
    if draft is None:
        frame.render_widget(Paragraph.from_string("Source policy is not available.").block(card(theme, "SOURCE POLICY SETTINGS", Semantic.WARNING)), area)
        return
    source_index = max(0, min(state.artist_index, len(draft.source_order) - 1))
    source = draft.source_order[source_index]
    policy = draft.policies[source]
    if layout_spec(area.width, area.height).stack_cards:
        panels = _split_vertical(area, [Constraint.length(10), Constraint.length(14), Constraint.fill(1)])
    else:
        panels = _split_horizontal(area, [Constraint.percentage(27), Constraint.percentage(42), Constraint.fill(1)])
    source_lines: list[Line] = []
    for index, value in enumerate(draft.source_order):
        marker = "›" if index == source_index else " "
        enabled = draft.policies[value]["enabled"]
        source_lines.append(Line([Span(f"{marker} {'☑' if enabled else '☐'} {value}", style(theme, Semantic.ACTIVE if index == source_index else Semantic.ACCEPTED if enabled else Semantic.DISABLED, bold=index == source_index))]))
    source_lines.extend([
        Line([Span("", style(theme, Semantic.MUTED))]),
        Line([Span("← Move Earlier   → Move Later", style(theme, Semantic.DEBUG))]),
    ])
    frame.render_widget(Paragraph(Text(source_lines)).block(card(theme, "ARTWORK SOURCE PRIORITY", Semantic.DEBUG)), panels[0])

    labels = {
        "enabled": "Source Enabled",
        "source_override": "Source Override",
        "minimum_range_type": "Minimum Range Type",
        "allow_below_minimum_fallback": "Fallback Range",
        "minimum_short_side": "Minimum short side",
        "maximum_short_side": "Maximum short side",
        "minimum_width": "Minimum width",
        "minimum_height": "Minimum height",
        "primary_image_only": "Primary image only",
    }
    setting_lines: list[Line] = []
    for index, key in enumerate(POLICY_FIELDS):
        raw = policy.get(key)
        value = "On" if raw is True else "Off" if raw is False else "—" if raw is None else str(raw)
        disabled = key == "primary_image_only" and source not in PRIMARY_METADATA_SOURCES
        if disabled:
            value = "Not differentiated by provider"
        semantic = Semantic.DISABLED if disabled else Semantic.ACTIVE if index == state.album_index_cursor else Semantic.TEXT
        setting_lines.append(Line([Span(f"{'›' if index == state.album_index_cursor else ' '} {labels[key]:<25} {value}", style(theme, semantic, bold=index == state.album_index_cursor))]))
    frame.render_widget(Paragraph(Text(setting_lines)).block(card(theme, f"SOURCE · {source.upper()}", Semantic.FALLBACK)), panels[1])

    global_lines: list[Line] = [Line([Span("GLOBAL ARTWORK RANGE", style(theme, Semantic.ACCEPTED, bold=True))])]
    for index, key in enumerate(GLOBAL_POLICY_FIELDS):
        value = draft.square_round_to if key == "square_round_to" else draft.ranges[key]
        label = "Square round-to" if key == "square_round_to" else key.capitalize()
        selected = state.library_focus == 2 and index == state.status_index
        global_lines.append(Line([Span(f"{'›' if selected else ' '} {label:<18} {value}", style(theme, Semantic.ACTIVE if selected else Semantic.TEXT, bold=selected))]))
    global_lines.extend([
        Line([Span("", style(theme, Semantic.MUTED))]),
        Line([Span("RANGE EFFECT / POLICY PREVIEW", style(theme, Semantic.SPECIAL, bold=True))]),
        Line([Span(draft.effective_preview(source), style(theme, Semantic.TEXT))]),
        Line([Span("Enter toggle/cycle · ←/→ adjust or reorder", style(theme, Semantic.DEBUG))]),
        Line([Span("Delete clears optional · Ctrl+S Save/Apply · Esc back", style(theme, Semantic.DEBUG))]),
    ])
    frame.render_widget(Paragraph(Text(global_lines)).wrap(True, True).block(card(theme, "RANGE EFFECT / POLICY PREVIEW", Semantic.SPECIAL if draft.dirty else Semantic.ACCEPTED)), panels[2])


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
        if kind == "library-selection":
            if state.workspace == "policy":
                return "↑/↓ settings · ←/→ priority/value · Enter toggle · Ctrl+S Save/Apply · Esc library · ? help"
            return "Tab/Shift+Tab regions · type in focused filter · Enter/Space select · P policy · Ctrl+C stop · ? help"
        if kind in {"artist", "album", "text"}:
            return f"{state.input_request.prompt}{state.input_buffer}   Enter confirm · Esc keep current"
        if kind == "local-comparison":
            return "↑/↓ choose · Enter exact · S suggested · K keep local · U URL · M MusicBrainz · B bypass · ? help"
        if kind == "musicbrainz":
            return "↑/↓ choose release · Enter select · B/Esc back · ? help"
        if kind == "fallback-picker":
            return "↑/↓ choose · Enter exact · S suggested · U URL · F edit · M MusicBrainz · B bypass · ? help"
        return "↑/↓ choose · Enter exact · S suggested · U URL · B bypass · ? help"
    if state.finished:
        return "Enter / q close · Tab history/logs · ? help"
    return "Tab views · Shift+Tab previous · ? help · Ctrl+C stop"


def _render_help(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    width = min(max(44, area.width - 8), 82)
    height = min(max(12, area.height - 4), 25)
    popup = Rect(
        area.x + max(0, (area.width - width) // 2),
        area.y + max(0, (area.height - height) // 2),
        min(width, area.width),
        min(height, area.height),
    )
    text = (
        "↑ / ↓          navigate\n"
        "← / →          change context\n"
        "Tab / Shift+Tab move workflow region\n"
        "Type           live Artist/Album filter when focused\n"
        "Enter / Space  select checkbox / activate\n"
        "Esc            close / back\n"
        "Mouse          unavailable in pyratatui 0.3.0 binding; keyboard remains complete\n"
        "P / Ctrl+S     source policy / explicit Save & Apply\n"
        "S              use suggested candidate\n"
        "K              keep local artwork\n"
        "F              edit fallback artist/album\n"
        "M              MusicBrainz retry/search/pick\n"
        "B              confirm bypass\n"
        "0-9            exact candidate selection\n"
        "U              open highlighted [URL] using the system URL handler\n"
        "AI ENHANCED    one candidate per album when real runtime is available\n"
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


def _render_dialog(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    width = min(42, area.width)
    height = min(7, area.height)
    popup = Rect(
        area.x + max(0, (area.width - width) // 2),
        area.y + max(0, (area.height - height) // 2),
        width,
        height,
    )
    if state.dialog_kind == "upscale":
        question = "Override upscale_below_ideal for this candidate attempt only?"
        accept, reject = "Yes · one attempt", "No · cancel"
        semantic = Semantic.REJECTED
        title = "AISPLINE RUNTIME OVERRIDE"
    elif state.dialog_kind == "floor":
        question = "This candidate is below the configured AISPLINE floor. Experiment once?"
        accept, reject = "Yes · deliberate exception", "No · cancel"
        semantic = Semantic.WARNING
        title = "BELOW-FLOOR EXPERIMENT"
    else:
        question = BYPASS_DIALOG.question
        accept, reject = BYPASS_DIALOG.accept_label, BYPASS_DIALOG.reject_label
        semantic = Semantic.FALLBACK
        title = BYPASS_DIALOG.title
    body = f"{question}\n\n[Y] {accept}          [N] {reject}"
    frame.render_widget(Clear(), popup)
    frame.render_widget(
        Paragraph.from_string(body)
        .centered()
        .block(card(theme, title, semantic)),
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
    if time.monotonic() - state.started_at < STARTUP_SECONDS:
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
    elif state.workflow == "library" and state.workspace == "policy":
        _render_policy(frame, content, state, theme)
    elif state.workflow == "library":
        _render_library(frame, content, state, theme)
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
        _render_dialog(frame, area, state, theme)


def _submit(state: TuiState, adapter: TuiAdapter, response: str) -> None:
    adapter.respond(response)
    state.input_request = None
    state.input_buffer = ""
    state.dialog_open = False
    state.workflow = "processing"


def _submit_library(state: TuiState, adapter: TuiAdapter) -> None:
    if state.library is None:
        return
    modes = ("filtered-read", "filtered-write", "auto-all", "auto-selected")
    mode = modes[state.scan_index]
    if mode == "auto-all":
        state.library.select_all()
    payload = state.library.selection_payload(mode)
    _submit(state, adapter, json.dumps(payload, separators=(",", ":")))


def _handle_library_key(
    state: TuiState, adapter: TuiAdapter, event: Any, action: Action
) -> bool:
    model = state.library
    request = state.input_request
    if model is None or request is None or request.kind != "library-selection":
        return False
    code = str(event.code)
    if state.workspace == "policy":
        draft = state.policy
        if draft is None:
            state.workspace = "library"
            return True
        source_index = max(0, min(state.artist_index, len(draft.source_order) - 1))
        source = draft.source_order[source_index]
        field_index = max(0, min(state.album_index_cursor, len(POLICY_FIELDS) - 1))
        key = POLICY_FIELDS[field_index]
        if action is Action.BACK:
            state.workspace = "library"
        elif action in {Action.NEXT_REGION, Action.PREVIOUS_REGION}:
            step = -1 if action is Action.PREVIOUS_REGION else 1
            state.library_focus = (state.library_focus + step) % 3
        elif action is Action.UP:
            if state.library_focus == 0:
                state.artist_index = (source_index - 1) % len(draft.source_order)
            elif state.library_focus == 1:
                state.album_index_cursor = (field_index - 1) % len(POLICY_FIELDS)
            else:
                state.status_index = (state.status_index - 1) % len(GLOBAL_POLICY_FIELDS)
        elif action is Action.DOWN:
            if state.library_focus == 0:
                state.artist_index = (source_index + 1) % len(draft.source_order)
            elif state.library_focus == 1:
                state.album_index_cursor = (field_index + 1) % len(POLICY_FIELDS)
            else:
                state.status_index = (state.status_index + 1) % len(GLOBAL_POLICY_FIELDS)
        elif action in {Action.LEFT, Action.RIGHT}:
            delta = -1 if action is Action.LEFT else 1
            if state.library_focus == 0:
                draft.move(source, delta)
                state.artist_index = draft.source_order.index(source)
            elif state.library_focus == 1 and key == "minimum_range_type":
                draft.cycle_range(source, delta)
            elif state.library_focus == 1 and key in {"minimum_short_side", "maximum_short_side", "minimum_width", "minimum_height"}:
                draft.adjust_number(source, key, delta * 100)
            elif state.library_focus == 2:
                global_key = GLOBAL_POLICY_FIELDS[state.status_index]
                draft.adjust_number(None, global_key, delta * (16 if global_key == "square_round_to" else 100))
        elif action in {Action.ACTIVATE, Action.TOGGLE}:
            if key in {"enabled", "source_override", "allow_below_minimum_fallback"}:
                draft.toggle(source, key)
            elif key == "primary_image_only":
                if source in PRIMARY_METADATA_SOURCES:
                    draft.toggle(source, key)
                else:
                    state.transient = "This provider does not expose differentiated primary-image metadata."
            elif key == "minimum_range_type":
                draft.cycle_range(source, 1)
        elif action is Action.DELETE and key in {"minimum_short_side", "maximum_short_side", "minimum_width", "minimum_height"}:
            draft.clear_optional(source, key)
        elif action is Action.SAVE:
            response = {"action": "save-settings", "policy": draft.as_payload()}
            _submit(state, adapter, json.dumps(response, separators=(",", ":")))
        return True

    if action in {Action.NEXT_REGION, Action.PREVIOUS_REGION}:
        step = -1 if action is Action.PREVIOUS_REGION else 1
        state.library_focus = (state.library_focus + step) % 7
        return True
    if action is Action.SETTINGS:
        state.workspace = "policy"
        state.library_focus = 0
        return True
    if state.library_focus in {4, 6}:
        if action is Action.DELETE:
            if state.library_focus == 4:
                model.set_filters(artist=model.artist_filter[:-1])
            else:
                model.set_filters(album=model.album_filter[:-1])
        elif action is Action.BACK:
            state.library_focus = 3 if state.library_focus == 4 else 5
        elif len(code) == 1 and code.isprintable() and not bool(getattr(event, "ctrl", False)):
            if state.library_focus == 4:
                model.set_filters(artist=model.artist_filter + code)
            else:
                model.set_filters(album=model.album_filter + code)
            state.artist_index = 0
            state.album_index_cursor = 0
        return True
    if action in {Action.LEFT, Action.RIGHT}:
        delta = -1 if action is Action.LEFT else 1
        if state.library_focus == 0:
            state.status_index = (state.status_index + delta) % len(STATUS_CONTROLS)
        elif state.library_focus == 1:
            state.select_index = (state.select_index + delta) % len(SELECT_CONTROLS)
        elif state.library_focus == 2:
            state.scan_index = (state.scan_index + delta) % len(SCAN_CONTROLS)
        return True
    if action in {Action.UP, Action.DOWN}:
        delta = -1 if action is Action.UP else 1
        if state.library_focus == 3:
            artists = model.visible_artists()
            if artists:
                state.artist_index = (state.artist_index + delta) % len(artists)
                model.active_artist = artists[state.artist_index].name
                state.album_index_cursor = 0
        elif state.library_focus == 5:
            albums = model.visible_albums(active_artist_only=True)
            if albums:
                state.album_index_cursor = (state.album_index_cursor + delta) % len(albums)
        elif state.library_focus == 0:
            state.status_index = (state.status_index + delta) % len(STATUS_CONTROLS)
        elif state.library_focus == 1:
            state.select_index = (state.select_index + delta) % len(SELECT_CONTROLS)
        elif state.library_focus == 2:
            state.scan_index = (state.scan_index + delta) % len(SCAN_CONTROLS)
        return True
    if action in {Action.ACTIVATE, Action.TOGGLE}:
        if state.library_focus == 0:
            status = STATUS_CONTROLS[state.status_index][1]
            if isinstance(status, AlbumStatus):
                model.toggle_status(status)
            else:
                model.toggle_artist_status(status)
        elif state.library_focus == 1:
            if state.select_index == 0:
                model.select_all()
            elif state.select_index == 1:
                model.select_none()
            else:
                model.select_all(filtered=True)
        elif state.library_focus == 2:
            _submit_library(state, adapter)
        elif state.library_focus == 3:
            artists = model.visible_artists()
            if artists:
                artist = artists[state.artist_index]
                model.active_artist = artist.name
                model.toggle_artist(artist.name)
        elif state.library_focus == 5:
            albums = model.visible_albums(active_artist_only=True)
            if albums:
                result = model.toggle_album(albums[state.album_index_cursor])
                if result == "bypass-confirmation-required":
                    state.dialog_kind = "library-bypass"
                    state.dialog_open = True
                elif result == "timeout-active":
                    state.transient = "Timeout-active albums remain protected during automatic selection."
        return True
    if action is Action.BACK:
        state.transient = "Library selection remains open; choose a Scan Mode or press Ctrl+C."
        return True
    return True


def handle_key(state: TuiState, adapter: TuiAdapter, event: Any) -> None:
    code = str(event.code)
    if bool(getattr(event, "ctrl", False)) and code.lower() == "c":
        state.exit_code = 130
        state.exit_requested = True
        adapter.cancel_wait()
        return
    if state.dialog_open:
        decision = confirm_key(code)
        if decision is True:
            if state.dialog_kind == "library-bypass" and state.library is not None:
                albums = state.library.visible_albums(active_artist_only=True)
                if albums:
                    state.library.toggle_album(
                        albums[state.album_index_cursor], bypass_override=True
                    )
                state.dialog_open = False
                state.dialog_kind = ""
            elif state.dialog_kind == "upscale" and state.ai_selection is not None:
                state.ai_selection.confirm_upscale(True)
                state.dialog_open = False
                state.dialog_kind = ""
            elif state.dialog_kind == "floor" and state.ai_selection is not None:
                key = state.ai_selection.pending_key
                if key is not None:
                    result = state.ai_selection.request(
                        key,
                        upscale_below_ideal=state.upscale_below_ideal,
                        below_floor_confirmed=True,
                    )
                    if result == "upscale-confirmation-required":
                        state.dialog_kind = "upscale"
                    else:
                        state.dialog_open = False
                        state.dialog_kind = ""
                        state.transient = result.replace("-", " ")
            else:
                _submit(state, adapter, "b")
        elif decision is False:
            if state.dialog_kind == "upscale" and state.ai_selection is not None:
                state.ai_selection.confirm_upscale(False)
            state.dialog_open = False
            state.dialog_kind = ""
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
    if _handle_library_key(state, adapter, event, action):
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
        selected = state.candidates[state.selected_index] if state.candidates else None
        if selected is not None:
            for index, (_name, items, _semantic) in enumerate(_candidate_groups(state)):
                if selected in items:
                    state.group_scroll = index
                    break
        return
    if action is Action.DOWN and selection_count:
        state.selected_index = (state.selected_index + 1) % selection_count
        state.input_buffer = str(state.selected_index + 1)
        selected = state.candidates[state.selected_index] if state.candidates else None
        if selected is not None:
            for index, (_name, items, _semantic) in enumerate(_candidate_groups(state)):
                if selected in items:
                    state.group_scroll = index
                    break
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
        state.dialog_kind = "bypass"
        state.dialog_open = True
        return
    if action is Action.URL and state.candidates:
        candidate = state.candidates[state.selected_index]
        if candidate.provenance == "[URL]" and candidate.url:
            opened = bool(webbrowser.open(candidate.url, new=2, autoraise=False))
            state.transient = "Opened highlighted [URL]." if opened else "No system URL handler is available; use --no-tui to copy the full URL."
        else:
            state.transient = "The highlighted candidate has no remote URL."
        return
    if action is Action.TOGGLE and state.ai_enabled and state.ai_selection and state.candidates:
        candidate = state.candidates[state.selected_index]
        key = candidate.ai_key or str(candidate.number)
        if state.ai_selection.selected_key == key:
            state.ai_selection.clear()
            return
        result = state.ai_selection.request(
            key, upscale_below_ideal=state.upscale_below_ideal
        )
        if result == "upscale-confirmation-required":
            state.dialog_kind = "upscale"
            state.dialog_open = True
        elif result == "below-floor-confirmation-required":
            state.dialog_kind = "floor"
            state.dialog_open = True
        else:
            state.transient = result.replace("-", " ")
        return
    if action is Action.QUIT:
        state.transient = "q is disabled during a decision; choose an engine action or Ctrl+C."
        return
    response = picker_response(action, state.selected_index)
    if response is not None:
        _submit(state, adapter, response)


def _drain(adapter: TuiAdapter, state: TuiState) -> bool:
    changed = False
    while True:
        try:
            event, payload = adapter.events.get_nowait()
        except queue.Empty:
            return changed
        state.apply(event, payload)
        changed = True


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
        terminal: Terminal | None = None
        try:
            terminal = Terminal()
            with terminal:
                thread.start()
                dirty = True
                startup_active = True
                while not state.exit_requested and not terminated:
                    dirty = _drain(adapter, state) or dirty
                    now_startup = time.monotonic() - state.started_at < STARTUP_SECONDS
                    if startup_active and not now_startup:
                        dirty = True
                    startup_active = now_startup
                    if dirty or startup_active:
                        terminal.draw(lambda frame: render(frame, state, theme))
                        dirty = False
                    key = terminal.poll_event(timeout_ms=80)
                    if key is not None:
                        handle_key(state, adapter, key)
                        dirty = True
        except BaseException as exc:
            if thread.ident is None:
                if terminal is not None:
                    try:
                        terminal.restore()
                    except Exception:
                        pass
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
