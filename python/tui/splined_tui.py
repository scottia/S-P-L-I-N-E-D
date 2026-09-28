"""Interactive Ratatui application around the authoritative SPLINED engine."""

from __future__ import annotations

import contextlib
import io
import json
import queue
import re
import signal

import requests
import sys
import threading
import time
from pathlib import Path
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

try:
    from splined_pyratatui_input import EventReader as InputEventReader
    from splined_pyratatui_input import emergency_restore as emergency_terminal_restore
    from splined_pyratatui_input import prepare_image_overlay as native_prepare_image_overlay
    from splined_pyratatui_input import clear_image_area as native_clear_image_area
except ImportError:  # plain CLI and automatic fallback remain independently usable
    InputEventReader = None  # type: ignore[assignment,misc]
    emergency_terminal_restore = None  # type: ignore[assignment]
    native_prepare_image_overlay = None  # type: ignore[assignment]
    native_clear_image_area = None  # type: ignore[assignment]

from .animation import animation_step, fit_phrase, startup_frame
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
    AlbumStatus,
    ArtistStatus,
    LibraryModel,
    artist_status,
)
from .layout import (
    Breakpoint,
    CandidateColumnLayout,
    candidate_column_layout,
    layout_spec,
)
from .preview import ArtworkPreview, generate_preview
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
    path: str = ""

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
            path=str(payload.get("path", "")),
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
class AlbumRunReport:
    position: int
    total: int
    artist: str
    album: str
    path: str
    started_at: float
    outcome: str = "Incomplete"
    discovery: str = "Provider search"
    review_state: str = "Automatic"
    candidate_total: int = 0
    policy_hidden: int = 0
    provider_notes: list[str] = field(default_factory=list)
    duration_seconds: float = 0.0
    destination: str = ""
    file_action: str = ""
    selected_source: str = ""
    selected_resolution: str = ""
    selected_format: str = ""
    selected_range: str = ""
    selected_distance: int | None = None
    detail: str = ""

    def finish(self) -> None:
        if self.duration_seconds <= 0:
            self.duration_seconds = max(0.0, time.monotonic() - self.started_at)


@dataclass(frozen=True)
class HitRegion:
    """One interactive rectangle registered from the current render geometry."""

    target: str
    x: int
    y: int
    width: int
    height: int
    index: int = -1
    value: str = ""

    def contains(self, column: int, row: int) -> bool:
        return (
            self.x <= column < self.x + self.width
            and self.y <= row < self.y + self.height
        )


@dataclass
class TuiState:
    started_at: float = field(default_factory=time.monotonic)
    workflow: str = "startup"
    tab: str = "main"
    album_index: int = 0
    album_total: int = 0
    album_path: str = "Preparing scan inventory"
    phase: str = "inventory"
    inventory_root: str = ""
    inventory_index: str = ""
    inventory_status: str = "Starting Select Media inventory"
    root_artists: int = 0
    cached_artists: int = 0
    indexed_artists: int = 0
    albums_known: int = 0
    inventory_artist: str = ""
    inventory_recovered: bool = False
    cache_phase: str = ""
    cache_processed: int = 0
    cache_total: int = 0
    cache_percent: float | None = None
    cache_albums: int = 0
    cache_added: int = 0
    cache_removed: int = 0
    cache_changed: int = 0
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
    batch_reports: list[AlbumRunReport] = field(default_factory=list)
    active_report: AlbumRunReport | None = None
    album_started: dict[str, float] = field(default_factory=dict)
    report_scroll: int = 0
    report_page_size: int = 1
    diagnostics: list[tuple[str, str]] = field(default_factory=list)
    activity: list[ActivityEntry] = field(default_factory=list)
    library: LibraryModel | None = None
    policy: PolicyDraft | None = None
    workspace: str = "library"
    library_focus: int = 0
    status_index: int = 0
    select_index: int = 1
    scan_index: int = -1
    artist_index: int = 0
    album_index_cursor: int = 0
    group_scroll: int = 0
    candidate_row_scroll: int = 0
    artist_scroll: int = 0
    album_scroll: int = 0
    policy_source_scroll: int = 0
    artist_page_size: int = 1
    album_page_size: int = 1
    candidate_page_size: int = 1
    policy_source_page_size: int = 1
    selected_stats_scroll: int = 0
    selected_stats_page_size: int = 1
    selected_album_stats: list[dict[str, Any]] = field(default_factory=list)
    debug_library_geometry: str = ""
    filter_edit: str = ""
    hit_regions: list[HitRegion] = field(default_factory=list)
    ai_enabled: bool = False
    ai_runtime_available: bool = False
    ai_selection: EnhancementSelection | None = None
    ai_activity: AiActivityState = field(default_factory=AiActivityState)
    upscale_below_ideal: bool = False
    dialog_kind: str = ""
    preview_cache: dict[tuple[str, str, int, int, int, int], ArtworkPreview | None] = field(default_factory=dict)
    preview_identity: dict[str, tuple[int, int]] = field(default_factory=dict)
    preview_modal_open: bool = False
    preview_modal_index: int = 0
    remote_hover_index: int = -1
    remote_hover_url: str = ""
    remote_hover_overlay: Any | None = None
    remote_hover_loading: bool = False
    remote_hover_error: str = ""
    remote_hover_token: int = 0
    remote_hover_active: bool = False
    remote_overlay_cache: dict[str, Any] = field(default_factory=dict)
    image_protocol: str = "unknown"
    remote_preview_width: int = 48
    remote_preview_height: int = 24
    remote_preview_rect: tuple[int, int, int, int] | None = None
    remote_drawn_rect: tuple[int, int, int, int] | None = None
    hover_target: str = ""
    hover_index: int = -1
    status_loading: bool = False
    status_processed: int = 0
    status_total: int = 0
    status_percent: float = 0.0
    status_albums: int = 0
    status_first_build: bool = False
    status_cached_baseline: int = 0
    bypass_dialog_path: str = ""
    bypass_dialog_enable: bool | None = None
    bypass_dialog_select_after: bool = False
    transient: str = ""
    help_open: bool = False
    dialog_open: bool = False
    finished: bool = False
    exit_requested: bool = False
    exit_after_worker: bool = False
    exit_code: int = 0
    exception: BaseException | None = None

    def apply(self, event: str, payload: dict[str, Any]) -> None:
        if event == "cache_build_start":
            self.workflow = "startup"
            self.cache_phase = "build"
            self.cache_processed = 0
            self.cache_total = 0
            self.cache_percent = None
            self.cache_albums = 0
        elif event == "cache_progress":
            self.cache_phase = str(payload.get("phase", self.cache_phase))
            self.inventory_status = str(payload.get("status", self.inventory_status))
            self.cache_processed = int(payload.get("processed", self.cache_processed) or 0)
            self.cache_total = int(payload.get("total", self.cache_total) or 0)
            percent = payload.get("percent")
            self.cache_percent = float(percent) if isinstance(percent, (int, float)) else None
            self.cache_albums = int(payload.get("albums", self.cache_albums) or 0)
            self.inventory_artist = str(payload.get("current_artist", ""))
            self.cache_added = int(payload.get("added", self.cache_added) or 0)
            self.cache_removed = int(payload.get("removed", self.cache_removed) or 0)
            self.cache_changed = int(payload.get("changed", self.cache_changed) or 0)
        elif event == "inventory_state":
            self.inventory_root = str(payload.get("library_root", self.inventory_root))
            self.inventory_index = str(payload.get("picker_index", self.inventory_index))
            self.inventory_status = str(payload.get("status", self.inventory_status))
            self.root_artists = int(payload.get("root_artists", self.root_artists) or 0)
            self.cached_artists = int(payload.get("cached_artists", self.cached_artists) or 0)
            self.indexed_artists = int(payload.get("indexed_artists", self.indexed_artists) or 0)
            self.albums_known = int(payload.get("albums_known", self.albums_known) or 0)
            self.inventory_artist = str(payload.get("current_artist", self.inventory_artist))
            self.inventory_recovered = bool(
                payload.get("recovered", self.inventory_recovered)
            )
        elif event == "folder_status_progress":
            self.status_loading = not bool(payload.get("done", False))
            self.status_processed = int(payload.get("processed", 0) or 0)
            self.status_total = int(payload.get("total", 0) or 0)
            percent = payload.get("percent")
            self.status_percent = (
                float(percent)
                if isinstance(percent, (int, float))
                else (
                    (self.status_processed / self.status_total * 100.0)
                    if self.status_total
                    else 0.0
                )
            )
            self.status_albums = int(payload.get("albums", 0) or 0)
            self.status_first_build = bool(
                payload.get("first_status_build", self.status_first_build)
            )
            self.status_cached_baseline = int(
                payload.get("cached_baseline", self.status_cached_baseline) or 0
            )
        elif event == "remote_hover_preview":
            token = int(payload.get("token", -1))
            index = int(payload.get("index", -1))
            if token != self.remote_hover_token or index != self.remote_hover_index:
                return
            self.remote_hover_loading = False
            self.remote_hover_error = str(payload.get("error", ""))
            self.remote_hover_overlay = payload.get("overlay")
            if (
                self.remote_hover_overlay is not None
                and self.remote_hover_url
            ):
                self.remote_overlay_cache[
                    self.remote_hover_url
                ] = self.remote_hover_overlay
        elif event in {"library", "library_update"}:
            self.workflow = "library"
            self.workspace = "library"
            if self.transient.startswith(
                (
                    "Indexing Artist folder",
                    "Reading Album folders",
                    "Reading matching Artist folders",
                    "Refreshing Artist folder list",
                    "Input busy",
                )
            ):
                self.transient = ""
            if event == "library" or self.library is None:
                self.library = LibraryModel.from_payload(payload)
            else:
                prior_albums = self.library.visible_albums(active_artist_only=True)
                prior_album_path = (
                    prior_albums[self.album_index_cursor].path
                    if 0 <= self.album_index_cursor < len(prior_albums)
                    else ""
                )
                self.library.merge_payload(
                    payload,
                    preserve_selection=bool(payload.get("preserve_selection", False)),
                )
                visible_artists = self.library.visible_artists()
                self.artist_index = next(
                    (
                        index
                        for index, artist in enumerate(visible_artists)
                        if artist.name == self.library.active_artist
                    ),
                    0,
                )
                visible_albums = self.library.visible_albums(active_artist_only=True)
                self.album_index_cursor = next(
                    (
                        index
                        for index, album in enumerate(visible_albums)
                        if album.path == prior_album_path
                    ),
                    min(self.album_index_cursor, max(0, len(visible_albums) - 1)),
                )
            loaded_states = {
                item.name: (item.status.value if item.status is not None else "")
                for item in self.library.artists
                if item.loaded
            }
            _runtime_trace(
                f"library_apply event={event!r} payload_artists="
                f"{len(payload.get('artists', [])) if isinstance(payload.get('artists'), list) else 'invalid'} "
                f"payload_albums={len(payload.get('albums', [])) if isinstance(payload.get('albums'), list) else 'invalid'} "
                f"loaded_artist_states={loaded_states!r} "
                f"{_library_snapshot(self)}"
            )
            raw_selected_stats = payload.get("selected_album_stats", [])
            self.selected_album_stats = (
                [item for item in raw_selected_stats if isinstance(item, dict)]
                if isinstance(raw_selected_stats, list)
                else []
            )
            if not self.selected_album_stats:
                self.selected_stats_scroll = 0
            _runtime_trace(
                f"library_stats_apply selected_stats={len(self.selected_album_stats)} "
                f"{_library_snapshot(self)}"
            )
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
        elif event == "library_selected_stats":
            if self.library is None:
                return
            raw_paths = payload.get("selected_paths", [])
            payload_paths = (
                [str(value) for value in raw_paths if str(value)]
                if isinstance(raw_paths, list)
                else []
            )
            try:
                limit = max(1, int(payload.get("limit", 10) or 10))
            except (TypeError, ValueError):
                limit = 10
            current_paths = sorted(
                (
                    item.path
                    for item in self.library.albums
                    if item.selected
                ),
                key=str.casefold,
            )[:limit]
            if payload_paths != current_paths:
                _runtime_trace(
                    "library_stats_stale "
                    f"payload_paths={payload_paths!r} current_paths={current_paths!r}"
                )
                return
            raw_selected_stats = payload.get("selected_album_stats", [])
            self.selected_album_stats = (
                [item for item in raw_selected_stats if isinstance(item, dict)]
                if isinstance(raw_selected_stats, list)
                else []
            )
            _runtime_trace(
                f"library_stats_async_apply selected_stats={len(self.selected_album_stats)} "
                f"{_library_snapshot(self)}"
            )
        elif event == "scan_start":
            self.remote_hover_token += 1
            self.remote_hover_index = -1
            self.remote_hover_url = ""
            self.remote_hover_overlay = None
            self.remote_hover_loading = False
            self.remote_hover_error = ""
            self.remote_preview_rect = None
            self.workflow = "overview"
            self.album_total = int(payload.get("total", 0))
            self.album_path = str(payload.get("root", self.album_path))
            self.phase = str(payload.get("phase", "authority"))
            self.batch_reports.clear()
            self.active_report = None
            self.album_started.clear()
            self.report_scroll = 0
        elif event == "album":
            path = str(payload.get("path", ""))
            self.remote_hover_token += 1
            self.remote_hover_index = -1
            self.remote_hover_url = ""
            self.remote_hover_overlay = None
            self.remote_hover_loading = False
            self.remote_hover_error = ""
            self.remote_hover_active = False
            self.remote_overlay_cache.clear()
            # Candidate cache files can reuse names between Albums.  Identity
            # and decoded terminal previews therefore belong to one Album
            # decision only, while each frame inside that decision remains
            # free of repeated stat/decode work.
            self.preview_cache.clear()
            self.preview_identity.clear()
            self.album_started.setdefault(path, time.monotonic())
            event_phase = str(payload.get("phase", "processing"))
            if event_phase == "processing":
                if self.active_report is not None and self.active_report.path != path:
                    self.active_report.finish()
                report = AlbumRunReport(
                    int(payload.get("index", 0)),
                    int(payload.get("total", self.album_total) or 0),
                    str(payload.get("artist", "")) or "Unknown Artist",
                    str(payload.get("album", "")) or path,
                    path,
                    self.album_started.get(path, time.monotonic()),
                    discovery=(
                        "Fallback"
                        if payload.get("fallback_reason")
                        else "MusicBrainz"
                        if str(payload.get("authority", "")) == "ExactAlbumId"
                        else "Provider search"
                    ),
                )
                self.batch_reports.append(report)
                self.active_report = report
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
            self.remote_hover_token += 1
            self.remote_hover_index = -1
            self.remote_hover_url = ""
            self.remote_hover_overlay = None
            self.remote_hover_loading = False
            self.remote_hover_error = ""
            self.remote_hover_active = False
            self.remote_overlay_cache.clear()
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
            if self.active_report is not None:
                hidden = int(payload.get("hidden_by_source_policy", 0) or 0)
                self.active_report.policy_hidden = max(
                    self.active_report.policy_hidden, hidden
                )
                self.active_report.candidate_total = max(
                    self.active_report.candidate_total,
                    len(self.candidates) + max(hidden, self.active_report.policy_hidden),
                )
                chosen = self.candidates[selected] if self.candidates else None
                if chosen is not None:
                    self.active_report.selected_source = chosen.source
                    self.active_report.selected_resolution = f"{chosen.width}×{chosen.height}"
                    self.active_report.selected_format = chosen.format.upper()
                    self.active_report.selected_range = chosen.range_type
                    self.active_report.selected_distance = chosen.distance
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
            if self.active_report is not None:
                grouped_notes: dict[tuple[str, str], int] = {}
                for source, message in self.diagnostics:
                    key = (source, message)
                    grouped_notes[key] = grouped_notes.get(key, 0) + 1
                self.active_report.provider_notes = [
                    (
                        f"{source}: {message}"
                        + (f"  ×{count}" if count > 1 else "")
                    )
                    for (source, message), count in grouped_notes.items()
                ]
                hidden = sum(
                    "policy-filtered" in message.casefold()
                    for _source, message in self.diagnostics
                )
                self.active_report.policy_hidden = hidden
                self.active_report.candidate_total = max(
                    self.active_report.candidate_total,
                    len(self.candidates) + hidden,
                )
        elif event == "input":
            context = dict(payload.get("context") or {})
            input_kind = str(context.get("kind", "picker"))
            if input_kind == "library-selection":
                self.remote_hover_token += 1
                self.remote_hover_index = -1
                self.remote_hover_url = ""
                self.remote_hover_overlay = None
                self.remote_hover_loading = False
                self.remote_hover_error = ""
                self.remote_preview_rect = None
                self.workflow = "library"
            elif input_kind == "batch-summary":
                self.remote_hover_token += 1
                self.remote_hover_index = -1
                self.remote_hover_url = ""
                self.remote_hover_overlay = None
                self.remote_hover_loading = False
                self.remote_hover_error = ""
                self.remote_preview_rect = None
                self.workflow = "batch-report"
            else:
                self.workflow = "picker"
            self.input_request = InputRequest(
                str(payload.get("prompt", "")),
                input_kind,
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
            if self.active_report is not None and self.input_request.kind not in {
                "library-selection",
                "batch-summary",
            }:
                self.active_report.review_state = "Required"
            self.input_buffer = ""
        elif event == "history":
            outcome = str(payload.get("outcome", "processed"))
            self.history.append(
                HistoryEntry(
                    datetime.now().strftime("%H:%M"),
                    str(payload.get("album", self.album_path)),
                    outcome,
                )
            )
            self.history = self.history[-200:]
            if self.active_report is not None:
                lowered = outcome.casefold()
                if "bypass" in lowered:
                    self.active_report.outcome = "Bypassed"
                elif "no-candidates" in lowered:
                    self.active_report.outcome = "Skipped"
                    self.active_report.detail = "No artwork candidates returned"
                elif "local-kept" in lowered:
                    self.active_report.outcome = "Unchanged"
                    self.active_report.file_action = "Retained"
                elif self.active_report.outcome == "Incomplete":
                    self.active_report.outcome = "Completed"
                self.active_report.finish()
        elif event == "album_material_result":
            if self.active_report is not None:
                self.active_report.outcome = str(payload.get("outcome", "Completed"))
                self.active_report.destination = str(payload.get("destination", ""))
                self.active_report.file_action = str(payload.get("file_action", ""))
                self.active_report.selected_source = str(
                    payload.get("source", self.active_report.selected_source)
                )
                width = int(payload.get("width", 0) or 0)
                height = int(payload.get("height", 0) or 0)
                if width and height:
                    self.active_report.selected_resolution = f"{width}×{height}"
                self.active_report.selected_format = str(
                    payload.get("format", self.active_report.selected_format)
                ).upper()
                self.active_report.selected_range = str(
                    payload.get("range_type", self.active_report.selected_range)
                )
                distance = payload.get("distance")
                if isinstance(distance, int):
                    self.active_report.selected_distance = distance
                self.active_report.detail = str(payload.get("detail", ""))
                self.active_report.finish()
        elif event == "summary":
            self.remote_hover_token += 1
            self.remote_hover_index = -1
            self.remote_hover_url = ""
            self.remote_hover_overlay = None
            self.remote_hover_loading = False
            self.remote_hover_error = ""
            self.remote_preview_rect = None
            self.summary = dict(payload)
            self.exit_code = int(payload.get("exit_code", 0) or 0)
            if self.active_report is not None:
                self.active_report.finish()
                if self.active_report.outcome == "Incomplete" and self.exit_code:
                    self.active_report.outcome = "Failed"
            self.candidates.clear()
            self.activity.clear()
            self.workflow = "batch-report"
        elif event == "log":
            level = str(payload.get("level", "INFO")).upper()
            message = str(payload.get("message", "")).strip()
            if message:
                self.logs.append((level, message))
                self.logs = self.logs[-500:]
                if (
                    level == "ERROR"
                    and self.active_report is not None
                    and self.active_report.outcome == "Incomplete"
                ):
                    # Generic stderr/log lines may describe a provider,
                    # diagnostic, or post-result problem.  Once the engine has
                    # emitted album_material_result/history, that explicit
                    # Album outcome is authoritative and must not be replaced
                    # by a later generic ERROR line.
                    self.active_report.outcome = "Failed"
                    self.active_report.detail = message
                    self.active_report.finish()
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
            _runtime_trace(
                f"worker_done exit_code={payload.get('exit_code', 0)!r} "
                f"exception={type(payload.get('exception')).__name__ if payload.get('exception') is not None else 'none'}"
            )
            self.finished = True
            self.exit_code = int(payload.get("exit_code", 0))
            self.exception = payload.get("exception")
            self.input_request = None
            if (
                self.exception is not None
                and type(self.exception).__name__ == "TuiConfigEditRequested"
            ):
                # Config editing must occur after Ratatui restores the terminal.
                self.exit_requested = True
            if self.exit_after_worker:
                self.exit_requested = True
            if not self.summary:
                self.summary = {"exit_code": self.exit_code}
                self.workflow = "batch-report"


def _runtime_trace(message: str) -> None:
    """Write TUI interaction diagnostics only when runtime verbosity permits."""
    core = sys.modules.get("splined")
    logger = getattr(core, "runtime_log", None) if core is not None else None
    if callable(logger):
        logger("debug", f"tui.{message}")


def _response_summary(response: str) -> str:
    try:
        payload = json.loads(response)
    except (TypeError, ValueError):
        value = str(response)
        return f"raw={value[:120]!r}"
    if not isinstance(payload, dict):
        return f"json_type={type(payload).__name__}"
    selected = payload.get("selected", [])
    artist_paths = payload.get("artist_paths", [])
    return (
        f"action={str(payload.get('action', ''))!r} "
        f"scan_mode={str(payload.get('scan_mode', ''))!r} "
        f"selected={len(selected) if isinstance(selected, list) else 'invalid'} "
        f"artists={len(artist_paths) if isinstance(artist_paths, list) else 'invalid'} "
        f"artist_path={str(payload.get('artist_path', ''))!r}"
    )


def _library_snapshot(state: TuiState) -> str:
    model = state.library
    if model is None:
        return "library=none"
    selected = [item.path for item in model.albums if item.selected]
    return (
        f"active_artist={model.active_artist!r} "
        f"artists={len(model.artists)} albums={len(model.albums)} "
        f"selected={len(selected)} selected_paths={selected!r} "
        f"selected_stats={len(state.selected_album_stats)} "
        f"focus={state.library_focus}"
    )


class TuiAdapter:
    def __init__(self) -> None:
        self.events: queue.Queue[tuple[str, dict[str, Any]]] = queue.Queue()
        self.responses: queue.Queue[str] = queue.Queue()
        self.waiting = threading.Event()
        self.cancel_event = threading.Event()

    def emit(self, event: str, payload: dict[str, Any]) -> None:
        self.events.put((event, payload))

    def read(self, prompt: str, context: dict[str, Any]) -> str:
        kind = str(context.get("kind", ""))
        _runtime_trace(
            f"adapter.read.wait kind={kind!r} "
            f"queued_responses={self.responses.qsize()}"
        )
        self.waiting.set()
        self.emit("input", {"prompt": prompt, "context": context})
        try:
            response = self.responses.get()
            _runtime_trace(
                f"adapter.read.received kind={kind!r} "
                f"{_response_summary(response)}"
            )
            return response
        finally:
            self.waiting.clear()
            _runtime_trace(f"adapter.read.clear kind={kind!r}")

    def respond(self, response: str) -> bool:
        waiting = self.waiting.is_set()
        _runtime_trace(
            f"adapter.respond waiting={waiting} queue_before={self.responses.qsize()} "
            f"{_response_summary(response)}"
        )
        if waiting:
            self.responses.put(response)
            return True
        _runtime_trace(
            f"adapter.respond.DROPPED {_response_summary(response)}"
        )
        return False

    def cancelled(self) -> bool:
        return self.cancel_event.is_set()

    def cancel_wait(self) -> None:
        # Cancellation must never alias the engine's persistent bypass command.
        # Set a cooperative flag even when the engine is busy outside read().
        self.cancel_event.set()
        if self.waiting.is_set():
            self.responses.put("__cancel__")


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
            core = sys.modules.get("splined")
            logger = getattr(core, "runtime_log", None) if core is not None else None
            if callable(logger):
                logger(level, clean)
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


def _register_hit(
    state: TuiState,
    target: str,
    area: Rect,
    *,
    index: int = -1,
    value: str = "",
) -> None:
    width = max(0, int(area.width))
    height = max(0, int(area.height))
    if width and height:
        state.hit_regions.append(
            HitRegion(
                target,
                int(area.x),
                int(area.y),
                width,
                height,
                index,
                value,
            )
        )


def _row_rect(area: Rect, row: int, *, left: int = 1, right: int = 1) -> Rect:
    return Rect(
        int(area.x) + left,
        int(area.y) + 1 + row,
        max(0, int(area.width) - left - right),
        1,
    )


def _checkbox_rect(area: Rect, row: int) -> Rect:
    return Rect(int(area.x) + 2, int(area.y) + 1 + row, 3, 1)


_SOLID_WORDMARK_BITS = {
    "A": ("010", "101", "111", "101", "101"),
    "D": ("110", "101", "101", "101", "110"),
    "E": ("111", "100", "110", "100", "111"),
    "I": ("111", "010", "010", "010", "111"),
    "L": ("100", "100", "100", "100", "111"),
    "N": ("101", "111", "111", "111", "101"),
    "P": ("110", "101", "110", "100", "100"),
    "S": ("011", "100", "010", "001", "110"),
    ":": ("0", "1", "0", "1", "0"),
}


def startup_brand_height(width: int, height: int) -> int:
    point = layout_spec(width, height).breakpoint
    if point is Breakpoint.WIDE:
        return 7
    if point is Breakpoint.NORMAL:
        return 7
    if point is Breakpoint.COMPACT:
        return 4
    return 1


def context_header_height(width: int, height: int) -> int:
    point = layout_spec(width, height).breakpoint
    if point in {Breakpoint.WIDE, Breakpoint.NORMAL}:
        return 6
    return 1


def _solid_spectral_brand(theme: Theme, title: str, phase: int = 0) -> list[Line]:
    """Render a five-pixel solid wordmark in three terminal-cell rows."""
    rows: list[list[Span]] = [[] for _ in range(3)]
    color_index = 0
    for character in title:
        glyph = _SOLID_WORDMARK_BITS.get(character, ("1",) * 5)
        if character == ":":
            semantic_style = style(theme, Semantic.MUTED)
        else:
            rgb = theme.title_spectrum[(color_index + phase) % len(theme.title_spectrum)]
            semantic_style = (
                Style()
                .fg(Color.rgb(*rgb))
                .bg(Color.rgb(*theme.color("background")))
            )
            color_index += 1
        for terminal_row, top_row in enumerate((0, 2, 4)):
            bottom_row = top_row + 1
            cells: list[str] = []
            for column in range(len(glyph[top_row])):
                top = glyph[top_row][column] == "1"
                bottom = bottom_row < len(glyph) and glyph[bottom_row][column] == "1"
                cells.append(
                    "█" if top and bottom else "▀" if top else "▄" if bottom else " "
                )
            rows[terminal_row].append(Span("".join(cells) + " ", semantic_style))
    return [Line(spans).centered() for spans in rows]


def _spectral_phrase(
    theme: Theme,
    text: str,
    phase: int = 0,
    *,
    centered: bool = True,
) -> Line:
    spans: list[Span] = []
    color_index = 0
    for character in text:
        if character == ":":
            spans.append(Span(character, style(theme, Semantic.MUTED)))
        elif character.isspace():
            spans.append(Span(character, style(theme, Semantic.TEXT)))
        else:
            rgb = theme.title_spectrum[
                (color_index + phase) % len(theme.title_spectrum)
            ]
            spans.append(
                Span(
                    character,
                    Style()
                    .fg(Color.rgb(*rgb))
                    .bg(Color.rgb(*theme.color("background"))),
                )
            )
            color_index += 1
    line = Line(spans)
    return line.centered() if centered else line


def _render_startup(frame: Any, state: TuiState, theme: Theme) -> None:
    area = frame.area
    frame.render_widget(_background(theme), area)
    brand = startup_frame(time.monotonic() - state.started_at)
    phrase = fit_phrase(brand.phrase, max(0, area.width - 4))
    brand_height = min(int(area.height), startup_brand_height(area.width, area.height))
    brand_area, inventory_area = _split_vertical(
        area,
        [Constraint.length(brand_height), Constraint.fill(1)],
    )
    point = layout_spec(area.width, area.height).breakpoint
    if point in {Breakpoint.WIDE, Breakpoint.NORMAL}:
        brand_lines = _solid_spectral_brand(
            theme,
            SPLINED_TITLE,
            animation_step(time.monotonic() - state.started_at),
        )
        brand_lines.extend(
            [
                _spectral_phrase(
                    theme,
                    (" " * brand.offset) + phrase,
                    animation_step(time.monotonic() - state.started_at),
                ),
            ]
        )
        # Startup branding intentionally floats on the terminal background.
        # Only operational/status surfaces retain frames.
        brand_widget = Paragraph(Text(brand_lines))
    else:
        brand_widget = Paragraph(
            Text(
                [
                    spectral_title(theme, centered=True),
                    _spectral_phrase(
                        theme,
                        phrase,
                        animation_step(time.monotonic() - state.started_at),
                    ),
                ]
            )
        )
    frame.render_widget(brand_widget, brand_area)

    panel_width = min(max(48, int(inventory_area.width) - 8), 86)
    panel_height = min(10, int(inventory_area.height))
    panel = Rect(
        int(inventory_area.x) + max(0, (int(inventory_area.width) - panel_width) // 2),
        int(inventory_area.y) + max(0, (int(inventory_area.height) - panel_height) // 2),
        panel_width,
        panel_height,
    )
    if state.cache_total:
        headline = state.inventory_status or "BUILDING LIBRARY CACHE"
        detail = f"{state.cache_processed:,} / {state.cache_total:,} Artists"
        percent = state.cache_percent or 0.0
        status_rows = _split_vertical(
            panel,
            [Constraint.length(3), Constraint.length(1), Constraint.length(2), Constraint.fill(1)],
        )
        frame.render_widget(
            Paragraph.from_string(f"{headline}\n{detail}").centered().block(
                card(theme, "LIBRARY CACHE", Semantic.ACTIVE)
            ),
            status_rows[0],
        )
        frame.render_widget(
            Gauge()
            .ratio(max(0.0, min(1.0, percent / 100.0)))
            .label(f"{percent:.1f}%")
            .gauge_style(style(theme, Semantic.ACCEPTED, bold=True))
            .style(panel_style(theme))
            .use_unicode(True),
            status_rows[2],
        )
        current = (
            f"{state.cache_albums:,} Albums discovered"
            + (f" · {state.inventory_artist}" if state.inventory_artist else "")
        )
        frame.render_widget(
            Paragraph.from_string(_truncate(current, max(1, panel_width))).centered().style(
                style(theme, Semantic.MUTED)
            ),
            status_rows[3],
        )
    else:
        frame.render_widget(
            Paragraph.from_string(
                f"{state.inventory_status}\n\nDiscovering Artist folders…"
            )
            .centered()
            .block(card(theme, "LIBRARY CACHE", Semantic.ACTIVE)),
            panel,
        )


def _status_gradient_rgb(position: float) -> tuple[int, int, int]:
    stops = (
        (0.0, (255, 70, 70)),
        (0.33, (255, 145, 35)),
        (0.66, (255, 220, 50)),
        (1.0, (60, 255, 135)),
    )
    value = max(0.0, min(1.0, position))
    for index in range(len(stops) - 1):
        left_pos, left = stops[index]
        right_pos, right = stops[index + 1]
        if value <= right_pos:
            span = max(0.0001, right_pos - left_pos)
            ratio = (value - left_pos) / span
            return tuple(
                round(left[channel] + (right[channel] - left[channel]) * ratio)
                for channel in range(3)
            )  # type: ignore[return-value]
    return stops[-1][1]


def _render_artist_status_loading_banner(
    frame: Any,
    area: Rect,
    state: TuiState,
    theme: Theme,
) -> None:
    width = min(max(64, int(area.width) - 24), 78)
    height = min(11, int(area.height))
    popup = Rect(
        int(area.x) + max(0, (int(area.width) - width) // 2),
        int(area.y) + max(0, (int(area.height) - height) // 2),
        width,
        height,
    )
    frame.render_widget(Clear(), popup)

    inner_width = max(10, width - 8)
    ratio = max(0.0, min(1.0, state.status_percent / 100.0))
    filled = min(inner_width, round(inner_width * ratio))
    bar_spans: list[Span] = [Span("[", style(theme, Semantic.MUTED))]
    for index in range(inner_width):
        if index < filled:
            rgb = _status_gradient_rgb(
                index / max(1, inner_width - 1)
            )
            bar_spans.append(
                Span(
                    "█",
                    Style().fg(Color.rgb(*rgb)),
                )
            )
        else:
            bar_spans.append(Span("░", style(theme, Semantic.MUTED)))
    bar_spans.append(Span("]", style(theme, Semantic.MUTED)))

    progress = (
        f"Artists {state.status_processed:,} / {state.status_total:,}"
        f"   ·   {state.status_percent:.0f}%"
    )
    if state.status_first_build:
        title = "BUILDING ALBUM STATUS INDEX"
        headline = "One-time Select Media setup…"
        message_one = (
            "Please wait — SPLINED is building the persistent Album status index."
        )
        message_two = "Future runs will reuse this saved status inventory."
    else:
        title = "LOADING ALBUM STATUS"
        headline = "Checking saved Album status…"
        message_one = (
            "Please wait — SPLINED is validating saved Artist / Album status changes."
        )
        message_two = (
            f"Cached Artists: {state.status_cached_baseline:,}"
            if state.status_cached_baseline
            else "Using saved status inventory."
        )

    body = Text(
        [
            Line([Span(headline, style(theme, Semantic.ACTIVE, bold=True))]).centered(),
            Line([]),
            Line([Span(progress, style(theme, Semantic.TEXT, bold=True))]).centered(),
            Line(bar_spans).centered(),
            Line([]),
            Line([Span(message_one, style(theme, Semantic.TEXT))]).centered(),
            Line([Span(message_two, style(theme, Semantic.TEXT))]).centered(),
            Line([]),
            Line([Span("Ctrl+C to cancel startup", style(theme, Semantic.MUTED))]).centered(),
        ]
    )
    frame.render_widget(
        Paragraph(body).block(
            card(theme, title, Semantic.SPECIAL)
        ),
        popup,
    )


def _header_context(state: TuiState, ai_context: bool) -> str:
    if ai_context:
        return "AI ACTIVITY"
    return {
        "library": "SELECT MEDIA",
        "overview": "SCAN ACTIVITY",
        "processing": "CURRENT ALBUM / ACTIVITY",
        "candidates": "CANDIDATE DECISION",
        "picker": "CANDIDATE DECISION",
        "batch-report": "ALBUM RUN REPORT",
    }.get(state.workflow, state.phase.upper())


def ai_context_active(state: TuiState) -> bool:
    return bool(
        state.ai_activity.active
        or (state.ai_selection and state.ai_selection.selected_key)
    )


def _render_header(
    frame: Any,
    area: Rect,
    state: TuiState,
    theme: Theme,
    point: Breakpoint,
) -> None:
    ai_context = ai_context_active(state)
    title = (
        AISPLINE_TITLE
        if ai_context
        else SPLINED_TITLE
    )
    status = f"{state.album_index} / {state.album_total}" if state.album_total else "READY"
    if point in {Breakpoint.WIDE, Breakpoint.NORMAL}:
        context = _header_context(state, ai_context)
        if point is Breakpoint.WIDE:
            lines = _solid_spectral_brand(
                theme,
                title,
                animation_step(time.monotonic() - state.started_at),
            )
            lines.append(
                Line(
                    [
                        Span(context, style(theme, Semantic.SPECIAL if ai_context else Semantic.ACTIVE, bold=True)),
                        Span(f"  ·  {status}", style(theme, Semantic.MUTED)),
                    ]
                ).centered()
            )
        else:
            lines = _solid_spectral_brand(
                theme,
                title,
                animation_step(time.monotonic() - state.started_at),
            )
            lines.append(Line([Span(f"{context}  ·  {status}", style(theme, Semantic.ACTIVE))]).centered())
        frame.render_widget(
            Paragraph(Text(lines)).block(
                card(theme, context, Semantic.SPECIAL if ai_context else Semantic.ACTIVE)
            ),
            area,
        )
    else:
        frame.render_widget(title_paragraph(theme, title=title), area)


STATUS_CONTROLS: tuple[tuple[str, AlbumStatus | ArtistStatus], ...] = (
    ("Unprocessed", AlbumStatus.UNPROCESSED),
    ("Processed", AlbumStatus.PROCESSED),
    ("Bypassed", AlbumStatus.BYPASSED),
    ("Partial / Timeout", AlbumStatus.TIMEOUT),
    ("Artist Complete", ArtistStatus.COMPLETE),
    ("Artist Contains Bypass", ArtistStatus.CONTAINS_BYPASS),
)
SELECT_CONTROLS = ("Select [ALL]", "Select [NONE]", "Select [FILTERED]")
LAUNCH_CONTROLS = (
    "Launch [READ] Source Results",
    "Launch [LIVE WRITE] Choice Results",
)
AUTO_SCAN_CONTROLS = (
    "Auto Scan [ALL]",
    "Auto Scan [SELECTED]",
)
SCAN_CONTROLS = LAUNCH_CONTROLS + AUTO_SCAN_CONTROLS


def _album_status_semantic(status: AlbumStatus) -> Semantic:
    return {
        AlbumStatus.UNPROCESSED: Semantic.UNPROCESSED,
        AlbumStatus.PROCESSED: Semantic.FALLBACK,
        AlbumStatus.BYPASSED: Semantic.REJECTED,
        AlbumStatus.TIMEOUT: Semantic.HISTORY,
    }[status]


def _artist_status_semantic(status: ArtistStatus | None) -> Semantic:
    if status is None:
        return Semantic.MUTED
    return {
        ArtistStatus.UNPROCESSED: Semantic.UNPROCESSED,
        ArtistStatus.PARTIAL: Semantic.HISTORY,
        ArtistStatus.COMPLETE: Semantic.ACCEPTED,
        ArtistStatus.CONTAINS_BYPASS: Semantic.DEBUG,
    }[status]


def _interactive_row_style(
    theme: Theme,
    semantic: Semantic,
    *,
    hovered: bool = False,
    selected: bool = False,
    focused: bool = False,
) -> Style:
    result = style(
        theme,
        semantic,
        bold=selected,
    )
    if hovered:
        result = result.bg(Color.rgb(*theme.color("muted")))
    return result


def _control_lines(
    labels: tuple[str, ...],
    selected: int,
    active: Callable[[int], bool],
    theme: Theme,
    suffix: Callable[[int], str] | None = None,
    *,
    hover_index: int = -1,
    index_offset: int = 0,
) -> Text:
    lines: list[Line] = []
    for local_index, label in enumerate(labels):
        index = local_index + index_offset
        focused = index == selected
        is_active = active(index)
        hovered = index == hover_index
        semantic = (
            Semantic.ACTIVE
            if focused
            else Semantic.ACCEPTED
            if is_active
            else Semantic.MUTED
        )
        extra = suffix(index) if suffix is not None else ""
        lines.append(
            Line(
                [
                    Span(
                        f"{'›' if focused else ' '} • {label}{extra}",
                        _interactive_row_style(
                            theme,
                            semantic,
                            hovered=hovered,
                            selected=is_active,
                            focused=focused,
                        ),
                    )
                ]
            )
        )
    return Text(lines)


def _status_control_semantic(index: int) -> Semantic:
    return (
        Semantic.UNPROCESSED,
        Semantic.FALLBACK,
        Semantic.REJECTED,
        Semantic.HISTORY,
        Semantic.ACCEPTED,
        Semantic.DEBUG,
    )[index]


def _status_control_lines(
    selected: int,
    active: Callable[[int], bool],
    theme: Theme,
    suffix: Callable[[int], str] | None = None,
    *,
    hover_index: int = -1,
) -> Text:
    lines: list[Line] = []
    for index, (label, _status) in enumerate(STATUS_CONTROLS):
        focused = index == selected
        is_active = active(index)
        hovered = index == hover_index
        extra = suffix(index) if suffix is not None else ""
        # Keep the count as a stable right-justified second column inside the
        # Folder Status panel rather than letting label length move it around.
        label_column = 22
        if extra:
            padding = " " * max(1, label_column - len(label))
            extra = f"{padding}{extra.strip()}"
        state_semantic = _status_control_semantic(index)
        row_style = _interactive_row_style(
            theme,
            state_semantic,
            hovered=hovered,
            selected=is_active,
            focused=focused,
        )
        lines.append(
            Line(
                [
                    Span(f"{'›' if focused else ' '} ", row_style),
                    Span("●  ", _interactive_row_style(
                        theme,
                        state_semantic,
                        hovered=hovered,
                        selected=is_active,
                        focused=focused,
                    )),
                    Span(f"{label}{extra}", row_style),
                ]
            )
        )
    return Text(lines)


def _selection_control_lines(
    selected: int,
    selected_count: int,
    theme: Theme,
    *,
    hover_index: int = -1,
) -> Text:
    lines = [
        Line(
            [
                Span("• SELECTED [", style(theme, Semantic.ACTIVE, bold=True)),
                Span(str(selected_count), style(theme, Semantic.ACCEPTED, bold=True)),
                Span("]", style(theme, Semantic.ACTIVE, bold=True)),
            ]
        )
    ]
    for index, label in enumerate(SELECT_CONTROLS):
        focused = index == selected
        hovered = index == hover_index
        lines.append(
            Line(
                [
                    Span(
                        f"{'›' if focused else ' '} ● {label}",
                        _interactive_row_style(
                            theme,
                            Semantic.ACTIVE if focused else Semantic.MUTED,
                            hovered=hovered,
                            focused=focused,
                        ),
                    )
                ]
            )
        )
    return Text(lines)


def _folder_status_count(
    index: int,
    album_counts: dict[AlbumStatus, int],
    artist_counts: dict[ArtistStatus, int],
) -> int:
    """Return the row count across both Album and loaded Artist semantics."""
    if index == 0:
        return (
            album_counts[AlbumStatus.UNPROCESSED]
            + artist_counts[ArtistStatus.UNPROCESSED]
        )
    if index == 3:
        return (
            album_counts[AlbumStatus.TIMEOUT]
            + artist_counts[ArtistStatus.PARTIAL]
        )
    status = STATUS_CONTROLS[index][1]
    return (
        album_counts[status]
        if isinstance(status, AlbumStatus)
        else artist_counts[status]
    )


def _render_library_controls(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    model = state.library
    assert model is not None
    # WIDE/NORMAL layouts keep all four operational control surfaces on one
    # row. Narrow layouts stack them without changing action semantics.
    if int(area.width) < 96:
        panels = _split_vertical(
            area,
            [
                Constraint.length(8),
                Constraint.length(6),
                Constraint.length(5),
                Constraint.length(5),
            ],
        )
    else:
        panels = _split_horizontal(
            area,
            [
                # Four equal control panels across the full terminal width.
                Constraint.percentage(25),
                Constraint.percentage(25),
                Constraint.percentage(25),
                Constraint.fill(1),
            ],
        )

    def status_active(index: int) -> bool:
        if index == 0:
            return (
                AlbumStatus.UNPROCESSED in model.status_filters
                and ArtistStatus.UNPROCESSED in model.artist_status_filters
            )
        if index == 3:
            return (
                AlbumStatus.TIMEOUT in model.status_filters
                and ArtistStatus.PARTIAL in model.artist_status_filters
            )
        status = STATUS_CONTROLS[index][1]
        return (
            status in model.status_filters
            if isinstance(status, AlbumStatus)
            else status in model.artist_status_filters
        )

    album_counts = model.album_status_counts()
    artist_counts = model.indexed_artist_status_counts()

    def status_suffix(index: int) -> str:
        return f"  [{_folder_status_count(index, album_counts, artist_counts):,}]"

    selected_count = sum(item.selected for item in model.albums)
    status_hover = state.hover_index if state.hover_target == "status-control" else -1
    select_hover = state.hover_index if state.hover_target == "select-control" else -1
    scan_hover = state.hover_index if state.hover_target == "scan-control" else -1

    frame.render_widget(
        Paragraph(
            _status_control_lines(
                state.status_index,
                status_active,
                theme,
                status_suffix,
                hover_index=status_hover,
            )
        )
        .block(card(theme, "FOLDER STATUS", Semantic.ACTIVE)),
        panels[0],
    )
    frame.render_widget(
        Paragraph(
            _selection_control_lines(
                state.select_index,
                selected_count,
                theme,
                hover_index=select_hover,
            )
        )
        .block(card(theme, "ALBUM SELECTION", Semantic.SPECIAL)),
        panels[1],
    )

    scan_needs_launch = (
        state.scan_index in {2, 3}
        and state.transient.startswith("Select SPLINED LAUNCH")
    )
    frame.render_widget(
        Paragraph(
            _control_lines(
                AUTO_SCAN_CONTROLS,
                state.scan_index,
                lambda i: i == state.scan_index,
                theme,
                hover_index=scan_hover,
                index_offset=2,
            )
        )
        .block(card(theme, "ALBUM SCANNING", Semantic.SPECIAL)),
        panels[2],
    )
    if scan_needs_launch and int(panels[2].height) >= 4:
        prompt = "Select LAUNCH [READ] or [LIVE WRITE]"
        frame.render_widget(
            Paragraph.from_string(_truncate(prompt, max(1, int(panels[2].width) - 4)))
            .style(style(theme, Semantic.WARNING, bold=True)),
            Rect(
                int(panels[2].x) + 2,
                int(panels[2].y) + int(panels[2].height) - 2,
                max(1, int(panels[2].width) - 4),
                1,
            ),
        )

    # LAUNCH keeps the yellow operational frame but overlays a spectral
    # S:P:L:I:N:E:D wordmark into the frame title.
    frame.render_widget(
        Paragraph(
            _control_lines(
                LAUNCH_CONTROLS,
                state.scan_index,
                lambda i: i == state.scan_index,
                theme,
                hover_index=scan_hover,
                index_offset=0,
            )
        )
        .block(card(theme, "", Semantic.WARNING)),
        panels[3],
    )
    if int(panels[3].width) > len(SPLINED_TITLE) + 10:
        title_x = int(panels[3].x) + 2
        frame.render_widget(
            Paragraph(Text([spectral_title(theme, title=SPLINED_TITLE)])),
            Rect(
                title_x,
                int(panels[3].y),
                len(SPLINED_TITLE) + 1,
                1,
            ),
        )
        frame.render_widget(
            Paragraph.from_string(" LAUNCH ").style(
                style(theme, Semantic.WARNING, bold=True)
            ),
            Rect(
                title_x + len(SPLINED_TITLE),
                int(panels[3].y),
                min(
                    9,
                    max(
                        1,
                        int(panels[3].x + panels[3].width)
                        - title_x
                        - len(SPLINED_TITLE)
                        - 1,
                    ),
                ),
                1,
            ),
        )

    for index in range(
        min(len(STATUS_CONTROLS), max(0, int(panels[0].height) - 2))
    ):
        _register_hit(
            state,
            "status-control",
            _row_rect(panels[0], index),
            index=index,
        )
    for index in range(
        min(len(SELECT_CONTROLS), max(0, int(panels[1].height) - 3))
    ):
        _register_hit(
            state,
            "select-control",
            _row_rect(panels[1], index + 1),
            index=index,
        )
    for local_index in range(
        min(len(AUTO_SCAN_CONTROLS), max(0, int(panels[2].height) - 2))
    ):
        _register_hit(
            state,
            "scan-control",
            _row_rect(panels[2], local_index),
            index=local_index + 2,
        )
    for index in range(
        min(len(LAUNCH_CONTROLS), max(0, int(panels[3].height) - 2))
    ):
        _register_hit(
            state,
            "scan-control",
            _row_rect(panels[3], index),
            index=index,
        )

def _visible_window(
    items: list[Any], selected: int, height: int, scroll: int | None = None
) -> tuple[list[Any], int]:
    size = max(1, height)
    selected = max(0, min(selected, max(0, len(items) - 1)))
    if scroll is None:
        start = selected - size // 2
    else:
        start = scroll
    start = max(0, min(start, max(0, len(items) - size)))
    return items[start : start + size], start


def _keep_visible(selected: int, scroll: int, capacity: int, count: int) -> int:
    capacity = max(1, capacity)
    maximum = max(0, count - capacity)
    if selected < scroll:
        scroll = selected
    elif selected >= scroll + capacity:
        scroll = selected - capacity + 1
    return max(0, min(scroll, maximum))


def _render_artist_picker(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    model = state.library
    assert model is not None
    rows = model.visible_artists()
    body, filter_area = _split_vertical(area, [Constraint.fill(1), Constraint.length(3)])
    capacity = max(1, int(body.height) - 2)
    state.artist_page_size = capacity
    state.artist_scroll = max(
        0, min(state.artist_scroll, max(0, len(rows) - capacity))
    )
    visible, start = _visible_window(
        rows, state.artist_index, capacity, state.artist_scroll
    )
    lines: list[Line] = []
    for offset, artist in enumerate(visible):
        index = start + offset
        focused = index == state.artist_index
        selected = artist.selected_count > 0
        hovered = (
            state.hover_target == "artist-row"
            and state.hover_index == index
        )
        semantic = _artist_status_semantic(artist.status)
        row_style = _interactive_row_style(
            theme,
            semantic,
            hovered=hovered,
            selected=selected,
            focused=focused,
        )
        lines.append(
            Line(
                [
                    Span(f"{'›' if focused else ' '} ", row_style),
                    Span(
                        _truncate(
                            artist.name,
                            max(4, int(body.width) - 4),
                        ),
                        row_style,
                    ),
                ]
            )
        )
    if not lines:
        lines.append(Line([Span("No artists match the active filters.", style(theme, Semantic.MUTED))]))
    frame.render_widget(
        Paragraph(Text(lines)).block(card(theme, f"ARTIST PICKER · {len(rows)} VISIBLE", Semantic.SPECIAL if state.library_focus == 3 else Semantic.FALLBACK)),
        body,
    )
    _register_hit(state, "artist-scroll", body)
    for offset, artist in enumerate(visible):
        index = start + offset
        _register_hit(
            state,
            "artist-row",
            _row_rect(body, offset),
            index=index,
            value=artist.name,
        )
    filter_text = f"{model.artist_filter}{'▌' if state.library_focus == 4 else ''}"
    frame.render_widget(
        Paragraph.from_string(filter_text).block(card(theme, "ARTIST FILTER · TYPE TO FILTER", Semantic.SPECIAL if state.library_focus == 4 else Semantic.ACTIVE)),
        filter_area,
    )
    _register_hit(state, "artist-filter", filter_area)


def _render_album_picker(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    model = state.library
    assert model is not None
    rows = model.visible_albums(active_artist_only=True)
    body, filter_area = _split_vertical(area, [Constraint.fill(1), Constraint.length(3)])
    capacity = max(1, int(body.height) - 2)
    state.album_page_size = capacity
    state.album_scroll = max(
        0, min(state.album_scroll, max(0, len(rows) - capacity))
    )
    visible, start = _visible_window(
        rows, state.album_index_cursor, capacity, state.album_scroll
    )
    lines: list[Line] = []
    for offset, album in enumerate(visible):
        index = start + offset
        focused = index == state.album_index_cursor
        hovered = (
            state.hover_target == "album-row"
            and state.hover_index == index
        )
        semantic = _album_status_semantic(album.status)
        row_style = _interactive_row_style(
            theme,
            semantic,
            hovered=hovered,
            selected=album.selected,
            focused=focused,
        )
        lines.append(
            Line(
                [
                    Span(f"{'›' if focused else ' '} ", row_style),
                    Span(
                        _truncate(
                            album.title,
                            max(4, int(body.width) - 4),
                        ),
                        row_style,
                    ),
                ]
            )
        )
    if not lines:
        lines.append(Line([Span("No albums match the active filters.", style(theme, Semantic.MUTED))]))
    frame.render_widget(
        Paragraph(Text(lines)).block(card(theme, f"ALBUM PICKER · {len(rows)} VISIBLE", Semantic.SPECIAL if state.library_focus == 5 else Semantic.ACTIVE)),
        body,
    )
    _register_hit(state, "album-scroll", body)
    for offset, album in enumerate(visible):
        index = start + offset
        _register_hit(
            state,
            "album-row",
            _row_rect(body, offset),
            index=index,
            value=album.path,
        )
    filter_text = f"{model.album_filter}{'▌' if state.library_focus == 6 else ''}"
    frame.render_widget(
        Paragraph.from_string(filter_text).block(card(theme, "ALBUM FILTER · TYPE TO FILTER", Semantic.SPECIAL if state.library_focus == 6 else Semantic.ACTIVE)),
        filter_area,
    )
    _register_hit(state, "album-filter", filter_area)


def _selected_album_stat_lines(
    state: TuiState,
    theme: Theme,
    width: int,
) -> list[Line]:
    if not state.selected_album_stats:
        return [
            Line(
                [
                    Span(
                        "No Albums selected.",
                        style(theme, Semantic.MUTED),
                    )
                ]
            )
        ]

    lines: list[Line] = []
    usable = max(12, width - 4)
    separator = "─" * usable

    def field(label: str, value: Any, semantic: Semantic = Semantic.TEXT) -> Line:
        return Line(
            [
                Span(f"{label:<16}", style(theme, Semantic.MUTED)),
                Span(
                    str(value) if str(value) else "—",
                    style(theme, semantic),
                ),
            ]
        )

    for index, item in enumerate(state.selected_album_stats, 1):
        if lines:
            lines.append(Line([Span(separator, style(theme, Semantic.MUTED))]))
        heading = (
            f"Selected album {index} / {len(state.selected_album_stats)}  ↕"
            if len(state.selected_album_stats) > 1
            else "Selected album"
        )
        lines.append(Line([Span(heading, style(theme, Semantic.SPECIAL, bold=True))]))
        lines.append(Line([Span(separator, style(theme, Semantic.MUTED))]))
        lines.extend(
            [
                field("Path", item.get("path", ""), Semantic.DEBUG),
                field("Album", item.get("album", ""), Semantic.ACTIVE),
                field("Artist", item.get("artist", ""), Semantic.ACTIVE),
                field("Year", item.get("year", "")),
                field("Tracks", f"({int(item.get('tracks', 0) or 0)})"),
            ]
        )
        lines.append(Line([Span(separator, style(theme, Semantic.MUTED))]))
        artwork = item.get("artwork", {})
        if not isinstance(artwork, dict):
            artwork = {}
        artwork_text = "  ".join(
            f"{name} ({int(artwork.get(name, 0) or 0)})"
            for name in ("JPEG", "PNG", "WEBP", "OTHER")
        )
        if item.get("cover_names", []):
            cover_names = ", ".join(str(v) for v in item.get("cover_names", []))
            cover_resolution = str(item.get("cover_resolution", "") or "—")
            lines.append(field("ARTWORK", f"{cover_names} · {cover_resolution}", Semantic.ACCEPTED))
        lines.append(Line([Span(separator, style(theme, Semantic.MUTED))]))
        lines.extend(
            [
                field("Root files", f"({int(item.get('root_files', 0) or 0)})"),
                field("Cover* files", f"({int(item.get('cover_files', 0) or 0)})"),
                field(
                    "Cover* names",
                    ", ".join(str(v) for v in item.get("cover_names", [])) or "—",
                    Semantic.FALLBACK,
                ),
                field(
                    "Other filenames",
                    ", ".join(str(v) for v in item.get("other_filenames", [])) or "—",
                ),
            ]
        )
        lines.append(Line([Span(separator, style(theme, Semantic.MUTED))]))
        size = float(item.get("webp_size_mb", 0.0) or 0.0)
        size_text = f"{size:g}" if size else "0"
        lines.extend(
            [
                field("WebP found", str(bool(item.get("webp_found", False))), Semantic.SPECIAL),
                field("WebP size(mb)", size_text),
                field("WebP resolution", item.get("webp_resolution", "")),
                field(
                    "WebP conversion",
                    str(bool(item.get("webp_conversion", False))),
                    Semantic.ACCEPTED
                    if bool(item.get("webp_conversion", False))
                    else Semantic.MUTED,
                ),
            ]
        )
    return lines


def _render_selected_album_stats(
    frame: Any,
    area: Rect,
    state: TuiState,
    theme: Theme,
) -> None:
    lines = _selected_album_stat_lines(state, theme, int(area.width))
    capacity = max(1, int(area.height) - 2)
    state.selected_stats_page_size = capacity
    maximum = max(0, len(lines) - capacity)
    state.selected_stats_scroll = max(0, min(state.selected_stats_scroll, maximum))
    shown = lines[
        state.selected_stats_scroll : state.selected_stats_scroll + capacity
    ]
    frame.render_widget(
        Paragraph(Text(shown))
        .wrap(True, False)
        .block(
            card(
                theme,
                "MEDIA LIBRARY / SELECTED ALBUM STATISTICS",
                Semantic.SPECIAL,
            )
        ),
        area,
    )
    _register_hit(state, "selected-stats-scroll", area)


def _scan_directory_stat_lines(
    state: TuiState,
    theme: Theme,
    width: int,
) -> list[Line]:
    assert state.library is not None
    stats = state.library.statistics()
    usable = max(12, width - 4)
    separator = "─" * usable

    def field(label: str, value: str, semantic: Semantic = Semantic.TEXT) -> Line:
        return Line(
            [
                Span(f"{label:<12}", style(theme, Semantic.MUTED)),
                Span(
                    _truncate(value, max(1, usable - 12)),
                    style(theme, semantic),
                ),
            ]
        )

    formats = stats["formats"]
    artwork = "  ".join(
        f"{name} ({int(formats.get(name, 0) or 0)})"
        for name in ("JPEG", "PNG", "WEBP")
    )
    return [
        field("Path", str(stats["path"]), Semantic.DEBUG),
        field(
            "Artists",
            f"{stats['artists']:,} - {stats['visible_artists']:,} visible",
            Semantic.ACTIVE,
        ),
        field(
            "Albums",
            f"{stats['albums']:,} "
            f"{'complete' if stats['complete_inventory'] else 'loaded'}",
            Semantic.ACTIVE,
        ),
        field("Cache", "N/A · DIRECT / LAZY", Semantic.ACCEPTED),
        Line([Span(separator, style(theme, Semantic.MUTED))]),
        field("Artwork", artwork, Semantic.ACCEPTED),
        Line([Span(separator, style(theme, Semantic.MUTED))]),
        field(
            "Active",
            f"{stats['active_albums']:,} total • "
            f"{stats['active_visible_albums']:,} visible",
        ),
        Line([Span(separator, style(theme, Semantic.MUTED))]),
        field(
            "Selected",
            f"({stats['selected']:,})",
            Semantic.SPECIAL,
        ),
        field("Processed", f"({stats['processed']:,})", Semantic.FALLBACK),
        field("Bypassed", f"({stats['bypassed']:,})", Semantic.REJECTED),
    ]


def _render_library_scan_stats(
    frame: Any,
    area: Rect,
    state: TuiState,
    theme: Theme,
) -> None:
    lines = _scan_directory_stat_lines(state, theme, int(area.width))
    capacity = max(1, int(area.height) - 2)
    frame.render_widget(
        Paragraph(Text(lines[:capacity])).block(
            card(
                theme,
                "MEDIA LIBRARY / SCAN DIR STATISTICS",
                Semantic.ACTIVE,
            )
        ),
        area,
    )


def _render_library_tools(
    frame: Any,
    area: Rect,
    state: TuiState,
    theme: Theme,
) -> None:
    full = (
        ("[P] Source Policy", "source-policy-open"),
        ("[E] Edit Settings", "settings-edit"),
        ("[R] Refresh Library Index", "library-refresh"),
    )
    medium = (
        ("[P] Policy", "source-policy-open"),
        ("[E] Settings", "settings-edit"),
        ("[R] Refresh", "library-refresh"),
    )
    compact = (
        ("[P] Policy", "source-policy-open"),
        ("[E] Edit", "settings-edit"),
        ("[R] Refresh", "library-refresh"),
    )
    inner_width = max(1, int(area.width) - 2)
    separator = "  •  "

    def total_width(items: tuple[tuple[str, str], ...]) -> int:
        return sum(len(label) for label, _target in items) + len(separator) * (
            len(items) - 1
        )

    controls = (
        full
        if total_width(full) <= inner_width
        else medium
        if total_width(medium) <= inner_width
        else compact
    )
    spans: list[Span] = []
    semantics = (Semantic.SPECIAL, Semantic.ACTIVE, Semantic.ACCEPTED)
    for index, (label, _target) in enumerate(controls):
        if index:
            spans.append(Span(separator, style(theme, Semantic.MUTED)))
        spans.append(Span(label, style(theme, semantics[index], bold=True)))

    body_width = total_width(controls)
    frame.render_widget(
        Paragraph(Text([Line(spans)]))
        .centered()
        .block(card(theme, "POLICY, LIBRARY and SETTINGS", Semantic.DEBUG)),
        area,
    )
    if int(area.height) < 2:
        return

    start_x = int(area.x) + 1 + max(0, (inner_width - body_width) // 2)
    y = int(area.y) + 1
    cursor = start_x
    for index, (label, target) in enumerate(controls):
        if index:
            cursor += len(separator)
        _register_hit(
            state,
            target,
            Rect(cursor, y, min(len(label), max(1, int(area.x + area.width) - cursor - 1)), 1),
        )
        cursor += len(label)




def _render_library_side_panels(
    frame: Any,
    area: Rect,
    state: TuiState,
    theme: Theme,
) -> None:
    tools_height = 3
    scan_lines = _scan_directory_stat_lines(state, theme, int(area.width))
    # These two lower panels are fixed/affixed.  The selected-Album panel owns
    # every remaining row and becomes scrollable as selections accumulate.
    scan_height = min(
        len(scan_lines) + 2,
        max(3, int(area.height) - tools_height - 3),
    )
    panels = _split_vertical(
        area,
        [
            Constraint.fill(1),
            Constraint.length(scan_height),
            Constraint.length(tools_height),
        ],
    )
    geometry = (
        f"area={int(area.width)}x{int(area.height)} "
        f"selected={int(panels[0].width)}x{int(panels[0].height)} "
        f"scan={int(panels[1].width)}x{int(panels[1].height)} "
        f"tools={int(panels[2].width)}x{int(panels[2].height)} "
        f"selected_stats={len(state.selected_album_stats)}"
    )
    if geometry != state.debug_library_geometry:
        state.debug_library_geometry = geometry
        _runtime_trace(f"library_panels {geometry}")
    _render_selected_album_stats(frame, panels[0], state, theme)
    _render_library_scan_stats(frame, panels[1], state, theme)
    _render_library_tools(frame, panels[2], state, theme)



def _render_library(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    if state.library is None:
        _render_overview(frame, area, state, theme)
        return
    spec = layout_spec(area.width, area.height)
    if spec.stack_cards:
        controls, lower = _split_vertical(
            area, [Constraint.length(26), Constraint.fill(1)]
        )
        picker_height = max(6, int(lower.height) // 2)
        pickers, side = _split_vertical(
            lower,
            [Constraint.length(picker_height), Constraint.fill(1)],
        )
        artist, album = _split_vertical(
            pickers, [Constraint.percentage(50), Constraint.fill(1)]
        )
        _render_library_controls(frame, controls, state, theme)
        _render_artist_picker(frame, artist, state, theme)
        _render_album_picker(frame, album, state, theme)
        _render_library_side_panels(frame, side, state, theme)
    else:
        top, bottom = _split_vertical(area, [Constraint.length(8), Constraint.fill(1)])
        _render_library_controls(frame, top, state, theme)
        columns = _split_horizontal(
            bottom,
            [Constraint.percentage(25), Constraint.percentage(50), Constraint.fill(1)],
        )
        _render_artist_picker(frame, columns[0], state, theme)
        _render_album_picker(frame, columns[1], state, theme)
        _render_library_side_panels(frame, columns[2], state, theme)


def _render_library_embedded(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    """Keep the Select Media workspace visible above active processing."""
    top, bottom = _split_vertical(area, [Constraint.length(8), Constraint.fill(1)])
    _render_library_controls(frame, top, state, theme)
    columns = _split_horizontal(
        bottom,
        # Match the primary Select Media geometry exactly so processing does
        # not shift the control/picker/statistics column boundaries.
        [Constraint.percentage(25), Constraint.percentage(50), Constraint.fill(1)],
    )
    _render_artist_picker(frame, columns[0], state, theme)
    _render_album_picker(frame, columns[1], state, theme)
    # Embedded processing views are intentionally shallow; show the scan-dir
    # summary only rather than crushing three stacked Select Media panels.
    _render_library_scan_stats(frame, columns[2], state, theme)


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
    current = (
        state.activity[-1].message
        if state.activity and state.phase in {"inventory", "authority"}
        else (state.album or state.album_path)
    )
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
    # Authority is a compact full-width strip above the live activity region.
    # Keeping both horizontal prevents the short authority value from consuming
    # half of the available activity width on WIDE terminals.
    authority_area, activity_area = _split_vertical(
        rows[1],
        [Constraint.length(4), Constraint.fill(1)],
    )
    frame.render_widget(
        Paragraph.from_string(_truncate(authority_text, max(1, int(authority_area.width) - 4)))
        .block(card(theme, "AUTHORITY", authority_semantic)),
        authority_area,
    )
    _render_activity_region(frame, activity_area, state, theme)


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


CANDIDATE_HEADERS = {
    "#": "#",
    "ai_enhanced": "AI ENHANCED",
    "ai_splined": "AI SPLINED",
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


def _candidate_value(candidate: CandidateView, column: str, state: TuiState) -> str:
    selected = bool(
        state.candidates and state.candidates[state.selected_index] is candidate
    )
    enhancement = (
        state.ai_selection.label(candidate.ai_key)
        if state.ai_enabled and state.ai_selection is not None
        else "N/A"
    )
    return {
        "#": f"›{candidate.number}" if selected else "★" if candidate.suggested else str(candidate.number),
        "ai_enhanced": enhancement,
        "ai_splined": (
            "✓ yes" if candidate.ai_review is True else "✕ no" if candidate.ai_review is False else "N/A"
        ),
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


def _candidate_cell_style(
    candidate: CandidateView,
    column: str,
    state: TuiState,
    theme: Theme,
) -> Style:
    selected = bool(
        state.candidates and state.candidates[state.selected_index] is candidate
    )
    semantic = Semantic.TEXT
    if column == "range":
        semantic = range_semantic(candidate.range_type)
    elif column in {"square", "acceptable", "approved"}:
        truth = {
            "square": candidate.square,
            "acceptable": candidate.acceptable,
            "approved": candidate.approved,
        }[column]
        semantic = Semantic.ACCEPTED if truth else Semantic.REJECTED
    elif column in {"ai_enhanced", "ai_splined"}:
        unavailable = _candidate_value(candidate, column, state) == "N/A"
        disabled = bool(
            column == "ai_enhanced"
            and state.ai_selection
            and state.ai_selection.disabled(candidate.ai_key)
        )
        semantic = Semantic.DISABLED if unavailable or disabled else Semantic.SPECIAL
    elif column == "url":
        semantic = (
            Semantic.DEBUG
            if candidate.provenance == "[URL]"
            else Semantic.SPECIAL
            if candidate.provenance == "[Enhanced]"
            else Semantic.ACCEPTED
        )
    elif selected:
        semantic = Semantic.ACTIVE
    return style(theme, semantic, bold=selected or column == "url")


def _render_candidate_thumbnail(
    frame: Any,
    area: Rect,
    state: TuiState,
    candidate: CandidateView,
    grid: CandidateColumnLayout,
) -> None:
    if (
        "url" not in grid.columns
        or candidate.provenance != "[URL]"
        or not candidate.path
        or grid.width("url") < 8
    ):
        return
    width = min(3, max(1, grid.width("url") - 6))
    preview = _candidate_preview(
        state,
        candidate,
        width=width,
        height=1,
        mode="quadrant",
    )
    if preview is None or not preview.rows:
        return
    spans = [
        Span(
            cell.glyph,
            Style()
            .fg(Color.rgb(*cell.foreground))
            .bg(Color.rgb(*cell.background)),
        )
        for cell in preview.rows[0][:width]
    ]
    frame.render_widget(
        Paragraph(Text([Line(spans)])),
        Rect(
            int(area.x) + 1 + grid.start("url") + 6,
            int(area.y),
            width,
            1,
        ),
    )


def _register_candidate_hits(
    state: TuiState,
    area: Rect,
    data_row: int,
    candidate: CandidateView,
    grid: CandidateColumnLayout,
    *,
    preferred: bool = False,
) -> None:
    candidate_index = state.candidates.index(candidate)
    row_area = Rect(
        int(area.x) + 1,
        int(area.y) + 2 + data_row,
        max(1, int(area.width) - 2),
        1,
    )
    _register_hit(
        state,
        "preferred-candidate" if preferred else "candidate-row",
        row_area,
        index=candidate_index,
        value=candidate.ai_key,
    )
    if "ai_enhanced" in grid.columns:
        _register_hit(
            state,
            "candidate-ai",
            Rect(
                int(area.x) + 1 + grid.start("ai_enhanced"),
                int(row_area.y),
                grid.width("ai_enhanced"),
                1,
            ),
            index=candidate_index,
            value=candidate.ai_key,
        )
    if "url" in grid.columns:
        url_x = int(area.x) + grid.start("url")
        if candidate.provenance == "[URL]" and candidate.url:
            _register_hit(
                state,
                "candidate-url",
                Rect(
                    url_x,
                    int(row_area.y),
                    min(5, grid.width("url")),
                    1,
                ),
                index=candidate_index,
                value=candidate.url,
            )


def _render_candidate_table_group(
    frame: Any,
    area: Rect,
    state: TuiState,
    theme: Theme,
    grid: CandidateColumnLayout,
    candidates: list[CandidateView],
    *,
    title: str,
    semantic: Semantic,
    preferred: bool = False,
) -> None:
    header = Row(
        [
            Cell(CANDIDATE_HEADERS[column], style(theme, Semantic.ACTIVE, bold=True))
            for column in grid.columns
        ]
    )
    rows = [
        Row(
            [
                Cell(
                    _candidate_value(candidate, column, state),
                    _candidate_cell_style(candidate, column, state, theme),
                )
                for column in grid.columns
            ]
        )
        for candidate in candidates
    ]
    table = (
        Table(
            rows,
            [Constraint.length(value) for value in grid.widths],
            header,
        )
        .block(card(theme, title, semantic))
        .column_spacing(grid.spacing)
    )
    frame.render_widget(table, area)
    for row, candidate in enumerate(candidates):
        _register_candidate_hits(
            state,
            area,
            row,
            candidate,
            grid,
            preferred=preferred,
        )
        # Remote artwork preview is driven by hovering the [URL] cell.  Do not
        # render or persist a separate cached-thumbnail affordance here.


def _candidate_preview(
    state: TuiState,
    candidate: CandidateView,
    width: int = 28,
    height: int = 12,
    *,
    mode: str = "half",
) -> ArtworkPreview | None:
    source = candidate.path
    if not source:
        return None
    if source not in state.preview_identity:
        try:
            stat = Path(source).stat()
            state.preview_identity[source] = (int(stat.st_size), int(stat.st_mtime_ns))
        except OSError:
            state.preview_identity[source] = (-1, -1)
    size, modified = state.preview_identity[source]
    key = (source, mode, max(1, width), max(1, height), size, modified)
    if key not in state.preview_cache:
        state.preview_cache[key] = generate_preview(
            source,
            width=width,
            height=height,
            mode=mode,
        )
    return state.preview_cache[key]


def _preferred_candidate_index(state: TuiState) -> int:
    if not state.candidates:
        return -1
    return next(
        (
            index
            for index, candidate in enumerate(state.candidates)
            if candidate.suggested
        ),
        max(0, min(state.selected_index, len(state.candidates) - 1)),
    )


def _start_remote_source_preview(
    state: TuiState,
    adapter: TuiAdapter,
    candidate_index: int,
    *,
    hover_active: bool,
) -> None:
    if not 0 <= candidate_index < len(state.candidates):
        return
    candidate = state.candidates[candidate_index]
    if candidate.provenance != "[URL]" or not candidate.url:
        return
    if native_prepare_image_overlay is None:
        state.remote_hover_error = "ratatui-image renderer is unavailable"
        return

    state.remote_hover_token += 1
    token = state.remote_hover_token
    state.remote_hover_index = candidate_index
    state.remote_hover_url = candidate.url
    state.remote_hover_active = hover_active
    state.remote_hover_error = ""

    cached = state.remote_overlay_cache.get(candidate.url)
    if cached is not None:
        state.remote_hover_overlay = cached
        state.remote_hover_loading = False
        return

    state.remote_hover_overlay = None
    state.remote_hover_loading = True
    width = max(1, state.remote_preview_width)
    height = max(1, state.remote_preview_height)
    url = candidate.url

    def worker() -> None:
        error = ""
        overlay: Any | None = None
        try:
            with requests.get(
                url,
                stream=True,
                allow_redirects=True,
                timeout=(3.05, 12.0),
                headers={"User-Agent": "SPLINED/1.0.9 remote-preview"},
            ) as response:
                response.raise_for_status()
                length = response.headers.get("Content-Length")
                if length is not None and int(length) > 25 * 1024 * 1024:
                    raise RuntimeError("remote preview exceeds 25 MiB limit")
                body = bytearray()
                for chunk in response.iter_content(chunk_size=64 * 1024):
                    if not chunk:
                        continue
                    body.extend(chunk)
                    if len(body) > 25 * 1024 * 1024:
                        raise RuntimeError("remote preview exceeds 25 MiB limit")
            overlay = native_prepare_image_overlay(
                bytes(body),
                width,
                height,
                1000,
            )
        except Exception as exc:
            error = str(exc)

        adapter.emit(
            "remote_hover_preview",
            {
                "token": token,
                "index": candidate_index,
                "url": url,
                "overlay": overlay,
                "error": error,
            },
        )

    threading.Thread(
        target=worker,
        name="splined-url-preview",
        daemon=True,
    ).start()


def _start_remote_hover_preview(
    state: TuiState,
    adapter: TuiAdapter,
    candidate_index: int,
) -> None:
    _start_remote_source_preview(
        state,
        adapter,
        candidate_index,
        hover_active=True,
    )


def _start_preferred_preview(
    state: TuiState,
    adapter: TuiAdapter,
) -> None:
    index = _preferred_candidate_index(state)
    if not 0 <= index < len(state.candidates):
        return
    candidate = state.candidates[index]
    if candidate.provenance != "[URL]" or not candidate.url:
        return
    if (
        state.remote_hover_index == index
        and state.remote_hover_url == candidate.url
        and (
            state.remote_hover_overlay is not None
            or state.remote_hover_loading
        )
        and not state.remote_hover_active
    ):
        return
    _start_remote_source_preview(
        state,
        adapter,
        index,
        hover_active=False,
    )


def _clear_remote_hover_preview(
    state: TuiState,
    adapter: TuiAdapter | None = None,
) -> None:
    if adapter is not None and state.candidates:
        state.remote_hover_active = False
        preferred = _preferred_candidate_index(state)
        if 0 <= preferred < len(state.candidates):
            candidate = state.candidates[preferred]
            if candidate.provenance == "[URL]" and candidate.url:
                _start_remote_source_preview(
                    state,
                    adapter,
                    preferred,
                    hover_active=False,
                )
                return

    state.remote_hover_token += 1
    state.remote_hover_index = -1
    state.remote_hover_url = ""
    state.remote_hover_overlay = None
    state.remote_hover_loading = False
    state.remote_hover_error = ""
    state.remote_hover_active = False
    state.remote_preview_rect = None


def _remote_overlay_is_visible(state: TuiState) -> bool:
    return bool(
        state.tab == "main"
        and state.workflow in {"candidates", "picker"}
        and state.remote_hover_overlay is not None
        and state.remote_preview_rect is not None
        and state.remote_hover_index >= 0
    )


def _clear_stale_remote_overlay(state: TuiState) -> None:
    drawn = state.remote_drawn_rect
    if drawn is None:
        return
    if (
        _remote_overlay_is_visible(state)
        and state.remote_preview_rect == drawn
    ):
        return
    if native_clear_image_area is not None:
        try:
            native_clear_image_area(*drawn)
        except Exception:
            pass
    state.remote_drawn_rect = None


def _draw_remote_hover_overlay(state: TuiState) -> None:
    if not _remote_overlay_is_visible(state):
        return
    overlay = state.remote_hover_overlay
    rect = state.remote_preview_rect
    assert overlay is not None and rect is not None
    x, y, width, height = rect
    if width <= 0 or height <= 0:
        return
    try:
        overlay.draw(x, y)
        state.remote_drawn_rect = rect
    except Exception as exc:
        state.remote_hover_error = str(exc)
        state.remote_hover_overlay = None



def _render_prepared_preview(
    frame: Any,
    area: Rect,
    preview: ArtworkPreview,
    theme: Theme,
    *,
    title: str,
) -> None:
    lines = [
        Line(
            [
                Span(
                    cell.glyph,
                    Style()
                    .fg(Color.rgb(*cell.foreground))
                    .bg(Color.rgb(*cell.background)),
                )
                for cell in row[: max(0, int(area.width) - 2)]
            ]
        )
        for row in preview.rows[: max(0, int(area.height) - 2)]
    ]
    frame.render_widget(
        Paragraph(Text(lines))
        .centered()
        .block(card(theme, title, Semantic.ACTIVE)),
        area,
    )


def _render_candidate_preview(
    frame: Any,
    area: Rect,
    state: TuiState,
    theme: Theme,
    candidate: CandidateView,
    *,
    title: str = "ARTWORK",
) -> None:
    preview_width = max(1, int(area.width) - 2)
    preview_height = max(1, int(area.height) - 2)
    preview = _candidate_preview(
        state,
        candidate,
        preview_width,
        preview_height,
        mode="half",
    )
    if preview is None:
        frame.render_widget(
            Paragraph.from_string("NO PREVIEW")
            .centered()
            .style(style(theme, Semantic.MUTED))
            .block(card(theme, title, Semantic.MUTED)),
            area,
        )
        return
    _render_prepared_preview(
        frame,
        area,
        preview,
        theme,
        title=title,
    )


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
    preview_enabled = (
        (spec.breakpoint is Breakpoint.WIDE and int(area.width) >= 150)
        or (spec.breakpoint is Breakpoint.NORMAL and int(area.width) >= 105)
    )
    preview_area: Rect | None = None
    if preview_enabled:
        preview_width = 48 if spec.breakpoint is Breakpoint.WIDE else 30
        preview_height = 24 if spec.breakpoint is Breakpoint.WIDE else 16
        area, preview_column = _split_horizontal(
            area,
            [Constraint.fill(1), Constraint.length(preview_width)],
        )
        preview_area = _split_vertical(
            preview_column,
            [Constraint.length(min(preview_height, int(preview_column.height))), Constraint.fill(1)],
        )[0]
    table_width = int(area.width)
    grid = candidate_column_layout(
        table_width,
        int(area.height),
        ai_enabled=state.ai_enabled,
    )

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

    preferred_height = 4
    if spec.stack_cards:
        top = _split_vertical(area, [Constraint.length(3), Constraint.length(3), Constraint.length(3), Constraint.length(4), Constraint.length(preferred_height), Constraint.fill(1), Constraint.length(5)])
        summary_areas = [top[0], top[1], top[2]]
        current, preferred, groups_area, activity_area = top[3], top[4], top[5], top[6]
        label = " • ".join(part for part in (state.artist, state.album) if part) or state.album_path
        details = f"Tracks {state.track_count or '—'} · Compilation {state.compilation or '—'} · Authority {state.authority or '—'} · Tagged {state.tag_state or '—'}"
        current_text = _truncate(label, max(1, area.width - 5)) + "\n" + _truncate(details, max(1, area.width - 5))
        frame.render_widget(Paragraph.from_string(current_text).block(card(theme, "CURRENT ALBUM", Semantic.ACTIVE)), current)
        preferred_semantic = Semantic.ACCEPTED if target and target.range_type == "Ideal" else Semantic.FALLBACK
    else:
        top, current, preferred, groups_area, activity_area = _split_vertical(
            area,
            [Constraint.length(4), Constraint.length(4), Constraint.length(preferred_height), Constraint.fill(1), Constraint.length(6)],
        )
        summary_areas = _split_horizontal(top, [Constraint.percentage(27), Constraint.percentage(46), Constraint.fill(1)])
        label = " • ".join(part for part in (state.artist, state.album) if part) or state.album_path
        details = f"Tracks {state.track_count or '—'} · Compilation {state.compilation or '—'} · Authority {state.authority or '—'} · Tagged {state.tag_state or '—'}"
        current_text = _truncate(label, max(1, area.width - 5)) + "\n" + _truncate(details, max(1, area.width - 5))
        frame.render_widget(Paragraph.from_string(current_text).block(card(theme, "CURRENT ALBUM", Semantic.ACTIVE)), current)
        preferred_semantic = Semantic.ACCEPTED if target and target.range_type == "Ideal" else Semantic.FALLBACK

    if target is None:
        frame.render_widget(
            Paragraph.from_string("No suggested candidate").block(
                card(theme, "PREFERRED SOURCE CANDIDATE", preferred_semantic)
            ),
            preferred,
        )
    else:
        _render_candidate_table_group(
            frame,
            preferred,
            state,
            theme,
            grid,
            [target],
            title=f"★★ PREFERRED SOURCE CANDIDATE · (S) SUGGESTED {target.source}",
            semantic=preferred_semantic,
            preferred=True,
        )

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
        visible_groups: list[tuple[int, str, list[CandidateView], Semantic]] = []
        heights: list[int] = []
        remaining = max(4, groups_area.height)
        for group_index, group in enumerate(
            groups[state.group_scroll :], start=state.group_scroll
        ):
            wanted = min(8, max(4, len(group[1]) + 3))
            if visible_groups and remaining < 4:
                break
            height = min(wanted, remaining) if not visible_groups else min(wanted, max(4, remaining))
            visible_groups.append((group_index, *group))
            heights.append(max(4, height))
            remaining -= max(4, height)
            if remaining < 4:
                break
        panels = _split_vertical(
            groups_area, [Constraint.length(value) for value in heights]
        )
        for panel, (group_index, name, candidates, semantic) in zip(panels, visible_groups):
            table_panel = panel
            row_capacity = max(1, table_panel.height - 3)
            if group_index == state.group_scroll:
                state.candidate_page_size = int(row_capacity)
                state.candidate_row_scroll = max(
                    0,
                    min(
                        state.candidate_row_scroll,
                        max(0, len(candidates) - row_capacity),
                    ),
                )
                row_start = state.candidate_row_scroll
            else:
                row_start = 0
            shown = candidates[row_start : row_start + row_capacity]
            suffix = f" · {len(candidates)} CANDIDATE(S)"
            if len(candidates) > len(shown):
                suffix += f" · ROWS {row_start + 1}-{row_start + len(shown)}/{len(candidates)}"
            if len(groups) > len(visible_groups):
                suffix += f" · GROUP {state.group_scroll + 1}/{len(groups)}"
            _register_hit(
                state,
                "candidate-scroll",
                panel,
                index=group_index,
                value=name,
            )
            _render_candidate_table_group(
                frame,
                table_panel,
                state,
                theme,
                grid,
                shown,
                title=f"SOURCE CANDIDATES {name.upper()}{suffix}",
                semantic=semantic,
            )
    else:
        frame.render_widget(Paragraph.from_string("No candidates returned.").block(card(theme, "SOURCE CANDIDATES", Semantic.WARNING)), groups_area)
    _render_activity_region(frame, activity_area, state, theme)
    if preview_area is not None and state.candidates:
        state.remote_preview_width = max(1, int(preview_area.width) - 2)
        state.remote_preview_height = max(1, int(preview_area.height) - 2)
        state.remote_preview_rect = (
            int(preview_area.x) + 1,
            int(preview_area.y) + 1,
            state.remote_preview_width,
            state.remote_preview_height,
        )
        preferred_index = _preferred_candidate_index(state)
        preferred = (
            state.candidates[preferred_index]
            if 0 <= preferred_index < len(state.candidates)
            else None
        )

        if state.remote_hover_index >= 0:
            active = state.candidates[
                max(0, min(state.remote_hover_index, len(state.candidates) - 1))
            ]
            preview_title = (
                f"ARTWORK / URL PREVIEW · {active.source.upper()}"
                if state.remote_hover_active
                else "ARTWORK / ⭐️ (S) PREFERRED"
            )
            if state.remote_hover_overlay is not None:
                frame.render_widget(
                    Paragraph.from_string("")
                    .block(card(theme, preview_title, Semantic.ACTIVE)),
                    preview_area,
                )
            elif state.remote_hover_loading:
                frame.render_widget(
                    Paragraph.from_string(
                        "Loading image directly from source URL…"
                    )
                    .centered()
                    .block(
                        card(
                            theme,
                            (
                                "ARTWORK / URL PREVIEW"
                                if state.remote_hover_active
                                else "ARTWORK / ⭐️ (S) PREFERRED"
                            ),
                            Semantic.ACTIVE,
                        )
                    ),
                    preview_area,
                )
            else:
                message = state.remote_hover_error or "Remote preview unavailable."
                frame.render_widget(
                    Paragraph.from_string(
                        _truncate(message, max(1, int(preview_area.width) - 4))
                    )
                    .centered()
                    .block(
                        card(
                            theme,
                            "ARTWORK / ⭐️ (S) PREFERRED",
                            Semantic.WARNING,
                        )
                    ),
                    preview_area,
                )
        elif preferred is not None and preferred.provenance == "[LOCAL]":
            state.remote_preview_rect = None
            _render_candidate_preview(
                frame,
                preview_area,
                state,
                theme,
                preferred,
                title="ARTWORK / ⭐️ (S) PREFERRED",
            )
        else:
            frame.render_widget(
                Paragraph.from_string(
                    "Loading preferred source artwork…"
                )
                .centered()
                .style(style(theme, Semantic.MUTED))
                .block(
                    card(
                        theme,
                        "ARTWORK / ⭐️ (S) PREFERRED",
                        Semantic.ACTIVE,
                    )
                ),
                preview_area,
            )


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
    source_capacity = max(1, int(panels[0].height) - 5)
    state.policy_source_page_size = source_capacity
    state.policy_source_scroll = max(
        0,
        min(
            state.policy_source_scroll,
            max(0, len(draft.source_order) - source_capacity),
        ),
    )
    visible_sources = draft.source_order[
        state.policy_source_scroll : state.policy_source_scroll + source_capacity
    ]
    source_lines: list[Line] = []
    for offset, value in enumerate(visible_sources):
        index = state.policy_source_scroll + offset
        focused = index == source_index
        enabled = bool(draft.policies[value]["enabled"])
        hovered = (
            state.hover_target == "policy-source"
            and state.hover_index == index
        )
        semantic = (
            Semantic.ACTIVE
            if focused
            else Semantic.ACCEPTED
            if enabled
            else Semantic.DISABLED
        )
        source_lines.append(
            Line(
                [
                    Span(
                        f"{'›' if focused else ' '} • {value}",
                        _interactive_row_style(
                            theme,
                            semantic,
                            hovered=hovered,
                            selected=enabled,
                            focused=focused,
                        ),
                    )
                ]
            )
        )
    source_lines.extend([
        Line([Span("", style(theme, Semantic.MUTED))]),
        Line([Span("← Move Earlier   → Move Later", style(theme, Semantic.DEBUG))]),
    ])
    frame.render_widget(Paragraph(Text(source_lines)).block(card(theme, "ARTWORK SOURCE PRIORITY", Semantic.DEBUG)), panels[0])
    _register_hit(state, "policy-source-scroll", panels[0])
    for offset, value in enumerate(visible_sources):
        index = state.policy_source_scroll + offset
        _register_hit(
            state,
            "policy-source",
            _row_rect(panels[0], offset),
            index=index,
            value=value,
        )
    priority_row = len(visible_sources) + 1
    priority_area = _row_rect(panels[0], priority_row)
    half = max(1, int(priority_area.width) // 2)
    _register_hit(
        state,
        "policy-move-earlier",
        Rect(int(priority_area.x), int(priority_area.y), half, 1),
        index=source_index,
        value=source,
    )
    _register_hit(
        state,
        "policy-move-later",
        Rect(int(priority_area.x) + half, int(priority_area.y), max(1, int(priority_area.width) - half), 1),
        index=source_index,
        value=source,
    )

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
    _register_hit(state, "policy-field-scroll", panels[1])
    for index, key in enumerate(POLICY_FIELDS[: max(0, int(panels[1].height) - 2)]):
        _register_hit(state, "policy-field", _row_rect(panels[1], index), index=index, value=key)

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
    for index, key in enumerate(GLOBAL_POLICY_FIELDS):
        _register_hit(state, "policy-global-field", _row_rect(panels[2], index + 1), index=index, value=key)


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


def _render_batch_report(frame: Any, area: Rect, state: TuiState, theme: Theme) -> None:
    lines: list[Line] = [
        Line(
            [
                Span(
                    f"BATCH COMPLETE · {len(state.batch_reports):,} ALBUM(S)",
                    style(
                        theme,
                        Semantic.ACCEPTED if state.exit_code == 0 else Semantic.REJECTED,
                        bold=True,
                    ),
                )
            ]
        ).centered(),
        Line([]),
    ]
    for position, report in enumerate(state.batch_reports, 1):
        semantic = outcome_semantic(report.outcome)
        lines.append(
            Line(
                [
                    Span(f"[{position} / {len(state.batch_reports)}]  ", style(theme, Semantic.ACTIVE, bold=True)),
                    Span(f"{report.artist} · {report.album}", style(theme, semantic, bold=True)),
                ]
            )
        )
        fields: list[tuple[str, str, Semantic]] = [
            ("Album path", report.path, Semantic.MUTED),
            ("Outcome", report.outcome, semantic),
            ("Discovery", report.discovery, Semantic.ACTIVE),
            ("Review", report.review_state, Semantic.TEXT),
            ("Candidates", f"{report.candidate_total:,} total", Semantic.TEXT),
            ("Policy hidden", f"{report.policy_hidden:,}", Semantic.WARNING if report.policy_hidden else Semantic.MUTED),
            ("Duration", f"{report.duration_seconds:.2f}s", Semantic.MUTED),
        ]
        if report.selected_source:
            details = " · ".join(
                value
                for value in (
                    report.selected_source,
                    report.selected_resolution,
                    report.selected_format,
                    report.selected_range,
                    (
                        f"distance {report.selected_distance}"
                        if report.selected_distance is not None
                        else ""
                    ),
                )
                if value
            )
            fields.append(("Selected", details, Semantic.SPECIAL))
        if report.destination:
            fields.append(("Destination", report.destination, Semantic.ACCEPTED))
        if report.file_action:
            fields.append(("File action", report.file_action, semantic))
        if report.detail:
            fields.append(("Detail", report.detail, Semantic.WARNING))
        for label, value, field_semantic in fields:
            lines.append(
                Line(
                    [
                        Span(f"  {label:<14}", style(theme, Semantic.MUTED)),
                        Span(_truncate(value, max(1, int(area.width) - 20)), style(theme, field_semantic)),
                    ]
                )
            )
        if report.provider_notes:
            lines.append(Line([Span("  Provider notes", style(theme, Semantic.MUTED))]))
            for note in report.provider_notes:
                lines.append(
                    Line(
                        [
                            Span("    • ", style(theme, Semantic.WARNING)),
                            Span(_truncate(note, max(1, int(area.width) - 10)), style(theme, Semantic.WARNING)),
                        ]
                    )
                )
        lines.append(Line([]))
    lines.append(
        Line(
            [Span("Enter / Esc  Return to Select Media    q  Exit SPLINED", style(theme, Semantic.ACTIVE, bold=True))]
        ).centered()
    )
    capacity = max(1, int(area.height) - 2)
    state.report_page_size = capacity
    maximum = max(0, len(lines) - capacity)
    state.report_scroll = max(0, min(state.report_scroll, maximum))
    shown = lines[state.report_scroll : state.report_scroll + capacity]
    frame.render_widget(
        Paragraph(Text(shown)).block(
            card(theme, "S:P:L:I:N:E:D ALBUM RUN REPORT", Semantic.HISTORY)
        ),
        area,
    )
    _register_hit(state, "report-scroll", area)


def _footer_text(state: TuiState) -> str:
    if state.transient:
        return state.transient
    if state.input_request:
        kind = state.input_request.kind
        if kind == "batch-summary":
            return "Enter / Esc return to Select Media · q exit SPLINED · Tab history/logs · Ctrl+C stop"
        if kind == "library-selection":
            if state.workspace == "policy":
                return "↑/↓ settings · PgUp/PgDn scroll · Mouse/touch enabled · Ctrl+S Save/Apply · Esc library · ? help"
            return "↑/↓ move · Enter open Artist · Space select · / filter · P policy · E settings · R refresh folders · Ctrl+C stop"
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
        "/              focus current Artist/Album filter\n"
        "Type           live filtering when a filter is focused\n"
        "Enter / Space  select checkbox / activate\n"
        "Esc            close / back\n"
        "PgUp / PgDn    page-scroll focused list\n"
        "Home / End     first / last row\n"
        "Mouse / touch  rows, checkboxes, filters, links, dialogs, scrolling\n"
        "P / Ctrl+S     source policy / explicit Save & Apply\n"
        "E              edit Config v5 in micro; reload current scan on save\n"
        "R              refresh immediate Artist folder list\n"
        "S              use suggested candidate\n"
        "K              keep local artwork\n"
        "F              edit fallback artist/album\n"
        "M              MusicBrainz retry/search/pick\n"
        "B              confirm bypass\n"
        "0-9            exact candidate selection\n"
        "U              identify highlighted [URL] (browser navigation is terminal-owned)\n"
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
    elif state.dialog_kind in {"library-bypass-add", "library-bypass-remove"}:
        removing = state.dialog_kind == "library-bypass-remove"
        question = (
            "Remove the saved bypass for this album?"
            if removing
            else "Bypass this album persistently?"
        )
        accept, reject = (
            ("Yes · remove bypass", "No · keep bypass")
            if removing
            else ("Yes · bypass album", "No · cancel")
        )
        semantic = Semantic.FALLBACK
        title = "ALBUM BYPASS"
    elif state.dialog_kind == "bypass-remove":
        question = "Remove the saved bypass for this album?"
        accept, reject = "Yes · remove bypass", "No · keep bypass"
        semantic = Semantic.FALLBACK
        title = "ALBUM BYPASS"
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
    button_y = int(popup.y) + max(1, int(popup.height) - 3)
    half = max(1, int(popup.width) // 2)
    _register_hit(
        state,
        "dialog-yes",
        Rect(int(popup.x) + 1, button_y, max(1, half - 1), 1),
    )
    _register_hit(
        state,
        "dialog-no",
        Rect(
            int(popup.x) + half,
            button_y,
            max(1, int(popup.width) - half - 1),
            1,
        ),
    )


def _render_candidate_preview_modal(
    frame: Any,
    area: Rect,
    state: TuiState,
    theme: Theme,
) -> None:
    if not state.candidates:
        return
    index = max(0, min(state.preview_modal_index, len(state.candidates) - 1))
    candidate = state.candidates[index]
    width = min(max(64, int(area.width) - 8), 132)
    height = min(max(24, int(area.height) - 2), 48)
    popup = Rect(
        int(area.x) + max(0, (int(area.width) - width) // 2),
        int(area.y) + max(0, (int(area.height) - height) // 2),
        width,
        height,
    )
    image_area, path_area = _split_vertical(
        popup,
        [Constraint.fill(1), Constraint.length(3)],
    )
    frame.render_widget(Clear(), popup)
    _render_candidate_preview(frame, image_area, state, theme, candidate)
    frame.render_widget(
        Paragraph.from_string(
            _truncate(
                f"CACHED FILE · {candidate.path}",
                max(1, int(path_area.width) - 4),
            )
        )
        .centered()
        .block(card(theme, "CACHED THUMBNAIL · Enter/Esc CLOSE", Semantic.DEBUG)),
        path_area,
    )
    _register_hit(state, "preview-modal-close", popup)


def render(frame: Any, state: TuiState, theme: Theme) -> None:
    area = frame.area
    state.hit_regions.clear()
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
    if state.workflow == "startup":
        _render_startup(frame, state, theme)
        return

    library_header = state.workflow == "library"
    header_height = (
        1 if library_header else context_header_height(area.width, area.height)
    )
    rows = _split_vertical(
        area,
        [
            Constraint.length(header_height),
            Constraint.fill(1),
            Constraint.length(1),
        ],
    )
    _render_header(
        frame,
        rows[0],
        state,
        theme,
        Breakpoint.COMPACT if library_header else spec.breakpoint,
    )
    content = rows[1]
    if state.tab == "history":
        _render_history(frame, content, state, theme)
    elif state.tab == "logs":
        _render_logs(frame, content, state, theme)
    elif state.workflow == "batch-report":
        _render_batch_report(frame, content, state, theme)
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
    if state.status_loading and state.workflow == "library":
        _render_artist_status_loading_banner(frame, area, state, theme)
    if state.help_open:
        _render_help(frame, area, state, theme)
    if state.dialog_open:
        _render_dialog(frame, area, state, theme)
    if state.preview_modal_open:
        _render_candidate_preview_modal(frame, area, state, theme)


def _submit(state: TuiState, adapter: TuiAdapter, response: str) -> bool:
    if not adapter.respond(response):
        state.transient = (
            "Input busy · current engine action is still running; retry when complete."
        )
        return False
    state.input_request = None
    state.input_buffer = ""
    state.dialog_open = False
    state.workflow = "processing"
    return True


def _submit_library(state: TuiState, adapter: TuiAdapter) -> None:
    if state.library is None:
        return
    # ALBUM SCANNING defines scope only. A READ/WRITE LAUNCH choice is
    # mandatory so Auto Scan can never silently default to READ.
    if state.scan_index in {2, 3}:
        state.transient = (
            "Select SPLINED LAUNCH [READ] or [LIVE WRITE] before Auto Scan."
        )
        return
    if state.scan_index not in {0, 1}:
        state.transient = "Select a SPLINED LAUNCH option."
        return
    modes = ("filtered-read", "filtered-write")
    mode = modes[state.scan_index]
    payload = state.library.selection_payload(mode)
    _runtime_trace(
        f"library_launch.send mode={mode!r} {_library_snapshot(state)}"
    )
    _submit(state, adapter, json.dumps(payload, separators=(",", ":")))


def _respond_library_action(
    state: TuiState,
    adapter: TuiAdapter,
    action: str,
    **values: Any,
) -> None:
    """Answer one library request without leaving the Select Media workspace."""
    if state.library is None:
        return
    payload = {"action": action, **state.library.selection_state(), **values}
    encoded = json.dumps(payload, separators=(",", ":"))
    _runtime_trace(
        f"library_action.send action={action!r} waiting={adapter.waiting.is_set()} "
        f"{_library_snapshot(state)}"
    )
    accepted = adapter.respond(encoded)
    if accepted:
        state.input_request = None
    else:
        state.transient = "Input busy · action not sent; retry."
    state.workflow = "library"


def _library_input_ready(state: TuiState, adapter: TuiAdapter) -> bool:
    request = state.input_request
    ready = (
        adapter.waiting.is_set()
        and request is not None
        and request.kind == "library-selection"
    )
    if ready:
        return True
    state.transient = (
        "Input busy · current library action is still running; retry when complete."
    )
    _runtime_trace(
        f"library_action.busy workflow={state.workflow!r} "
        f"waiting={adapter.waiting.is_set()} {_library_snapshot(state)}"
    )
    return False


def _sync_library_selection(state: TuiState, adapter: TuiAdapter) -> None:
    if state.library is None:
        return
    _respond_library_action(state, adapter, "selection-change")


def _request_bulk_selection(
    state: TuiState,
    adapter: TuiAdapter,
    *,
    filtered: bool,
) -> None:
    model = state.library
    if model is None:
        return
    if not _library_input_ready(state, adapter):
        return
    _runtime_trace(
        f"bulk_select requested filtered={filtered} waiting={adapter.waiting.is_set()} "
        f"{_library_snapshot(state)}"
    )
    if not filtered:
        active = model.artist(model.active_artist) if model.active_artist else None
        if active is None:
            visible = model.visible_artists()
            active = visible[0] if visible else None
        if active is None:
            state.transient = "Select [ALL] requires an Artist."
            return
        state.transient = (
            f"Selecting all Unprocessed Albums for {active.name}…"
        )
        _respond_library_action(
            state,
            adapter,
            "select-all",
            artist_path=active.path,
        )
        return

    artist_filter = model.artist_filter.strip()
    album_filter = model.album_filter.strip()
    if not artist_filter and not album_filter:
        state.transient = (
            "Select [FILTERED] requires Artist or Album filter text."
        )
        return

    artist_paths = [
        artist.path
        for artist in model.artists
        if not artist_filter
        or artist_filter.casefold() in artist.name.casefold()
    ]
    state.transient = "Selecting Unprocessed Albums matching text filter…"
    _respond_library_action(
        state,
        adapter,
        "select-filtered",
        artist_paths=artist_paths,
        artist_filter=artist_filter,
        album_filter=album_filter,
    )


def _open_artist(
    state: TuiState,
    adapter: TuiAdapter,
    artist_name: str,
    *,
    select_after_load: bool = False,
) -> None:
    model = state.library
    if model is None:
        return
    _runtime_trace(
        f"artist.open requested={artist_name!r} select_after_load={select_after_load} "
        f"waiting={adapter.waiting.is_set()} {_library_snapshot(state)}"
    )
    artist = model.artist(artist_name)
    if artist is None:
        return
    if artist.loaded:
        model.active_artist = artist_name
        state.album_index_cursor = 0
        state.album_scroll = 0
        if select_after_load:
            if not _library_input_ready(state, adapter):
                return
            model.toggle_artist(artist_name)
            _sync_library_selection(state, adapter)
        return
    if not _library_input_ready(state, adapter):
        return
    model.active_artist = artist_name
    state.album_index_cursor = 0
    state.album_scroll = 0
    state.transient = ""
    _respond_library_action(
        state,
        adapter,
        "load-artist",
        artist_path=artist.path,
        select_after_load=select_after_load,
    )


def _handle_library_key(
    state: TuiState, adapter: TuiAdapter, event: Any, action: Action
) -> bool:
    model = state.library
    request = state.input_request
    if model is None or request is None or request.kind != "library-selection":
        return False
    code = str(event.code)
    if action is Action.QUIT:
        _respond_library_action(state, adapter, "exit")
        state.exit_after_worker = True
        state.transient = "Closing SPLINED after the engine session stops safely…"
        return True
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
                state.policy_source_scroll = _keep_visible(
                    state.artist_index,
                    state.policy_source_scroll,
                    state.policy_source_page_size,
                    len(draft.source_order),
                )
            elif state.library_focus == 1:
                state.album_index_cursor = (field_index - 1) % len(POLICY_FIELDS)
            else:
                state.status_index = (state.status_index - 1) % len(GLOBAL_POLICY_FIELDS)
        elif action is Action.DOWN:
            if state.library_focus == 0:
                state.artist_index = (source_index + 1) % len(draft.source_order)
                state.policy_source_scroll = _keep_visible(
                    state.artist_index,
                    state.policy_source_scroll,
                    state.policy_source_page_size,
                    len(draft.source_order),
                )
            elif state.library_focus == 1:
                state.album_index_cursor = (field_index + 1) % len(POLICY_FIELDS)
            else:
                state.status_index = (state.status_index + 1) % len(GLOBAL_POLICY_FIELDS)
        elif action in {Action.PAGE_UP, Action.PAGE_DOWN, Action.HOME, Action.END}:
            if state.library_focus == 0:
                if action is Action.HOME:
                    state.artist_index = 0
                elif action is Action.END:
                    state.artist_index = len(draft.source_order) - 1
                else:
                    step = -5 if action is Action.PAGE_UP else 5
                    state.artist_index = max(0, min(source_index + step, len(draft.source_order) - 1))
                state.policy_source_scroll = _keep_visible(
                    state.artist_index,
                    state.policy_source_scroll,
                    state.policy_source_page_size,
                    len(draft.source_order),
                )
            elif state.library_focus == 1:
                if action is Action.HOME:
                    state.album_index_cursor = 0
                elif action is Action.END:
                    state.album_index_cursor = len(POLICY_FIELDS) - 1
                else:
                    step = -5 if action is Action.PAGE_UP else 5
                    state.album_index_cursor = max(0, min(field_index + step, len(POLICY_FIELDS) - 1))
            else:
                if action is Action.HOME:
                    state.status_index = 0
                elif action is Action.END:
                    state.status_index = len(GLOBAL_POLICY_FIELDS) - 1
                else:
                    step = -3 if action is Action.PAGE_UP else 3
                    state.status_index = max(0, min(state.status_index + step, len(GLOBAL_POLICY_FIELDS) - 1))
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
        focus_order = (0, 1, 2, 3, 5)
        step = -1 if action is Action.PREVIOUS_REGION else 1
        current = state.library_focus if state.library_focus in focus_order else (3 if state.library_focus == 4 else 5)
        state.library_focus = focus_order[(focus_order.index(current) + step) % len(focus_order)]
        return True
    if state.library_focus in {4, 6}:
        if action is Action.DELETE:
            if state.library_focus == 4:
                model.set_filters(artist=model.artist_filter[:-1])
            else:
                model.set_filters(album=model.album_filter[:-1])
        elif action in {Action.BACK, Action.ACTIVATE}:
            state.library_focus = 3 if state.library_focus == 4 else 5
        elif len(code) == 1 and code.isprintable() and not bool(getattr(event, "ctrl", False)):
            if state.library_focus == 4:
                model.set_filters(artist=model.artist_filter + code)
            else:
                model.set_filters(album=model.album_filter + code)
            state.artist_index = 0
            state.album_index_cursor = 0
            state.artist_scroll = 0
            state.album_scroll = 0
        return True
    if action is Action.SETTINGS:
        state.workspace = "policy"
        state.library_focus = 0
        return True
    if action is Action.EDIT_SETTINGS:
        state.transient = "Opening Config v5 in micro…"
        _respond_library_action(state, adapter, "edit-config")
        return True
    if action is Action.REFRESH:
        state.transient = "Refreshing Artist folder list…"
        _respond_library_action(state, adapter, "refresh-index")
        return True
    if action is Action.FILTER:
        state.library_focus = 6 if state.library_focus in {5, 6} else 4
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
                state.album_scroll = 0
                state.artist_scroll = _keep_visible(
                    state.artist_index,
                    state.artist_scroll,
                    state.artist_page_size,
                    len(artists),
                )
        elif state.library_focus == 5:
            albums = model.visible_albums(active_artist_only=True)
            if albums:
                state.album_index_cursor = (state.album_index_cursor + delta) % len(albums)
                state.album_scroll = _keep_visible(
                    state.album_index_cursor,
                    state.album_scroll,
                    state.album_page_size,
                    len(albums),
                )
        elif state.library_focus == 0:
            state.status_index = (state.status_index + delta) % len(STATUS_CONTROLS)
        elif state.library_focus == 1:
            state.select_index = (state.select_index + delta) % len(SELECT_CONTROLS)
        elif state.library_focus == 2:
            state.scan_index = (state.scan_index + delta) % len(SCAN_CONTROLS)
        return True
    if action in {Action.PAGE_UP, Action.PAGE_DOWN, Action.HOME, Action.END}:
        if state.library_focus == 3:
            rows = model.visible_artists()
            if rows:
                if action is Action.HOME:
                    state.artist_index = 0
                elif action is Action.END:
                    state.artist_index = len(rows) - 1
                else:
                    step = -10 if action is Action.PAGE_UP else 10
                    state.artist_index = max(0, min(state.artist_index + step, len(rows) - 1))
                model.active_artist = rows[state.artist_index].name
                state.album_index_cursor = 0
                state.album_scroll = 0
                state.artist_scroll = _keep_visible(
                    state.artist_index,
                    state.artist_scroll,
                    state.artist_page_size,
                    len(rows),
                )
        elif state.library_focus == 5:
            rows = model.visible_albums(active_artist_only=True)
            if rows:
                if action is Action.HOME:
                    state.album_index_cursor = 0
                elif action is Action.END:
                    state.album_index_cursor = len(rows) - 1
                else:
                    step = -10 if action is Action.PAGE_UP else 10
                    state.album_index_cursor = max(0, min(state.album_index_cursor + step, len(rows) - 1))
                state.album_scroll = _keep_visible(
                    state.album_index_cursor,
                    state.album_scroll,
                    state.album_page_size,
                    len(rows),
                )
        return True
    if action in {Action.ACTIVATE, Action.TOGGLE}:
        if state.library_focus == 0:
            if state.status_index == 0:
                model.toggle_status(AlbumStatus.UNPROCESSED)
                model.toggle_artist_status(ArtistStatus.UNPROCESSED)
            elif state.status_index == 3:
                model.toggle_status(AlbumStatus.TIMEOUT)
                model.toggle_artist_status(ArtistStatus.PARTIAL)
            else:
                status = STATUS_CONTROLS[state.status_index][1]
                if isinstance(status, AlbumStatus):
                    model.toggle_status(status)
                else:
                    model.toggle_artist_status(status)
        elif state.library_focus == 1:
            if state.select_index == 0:
                _request_bulk_selection(state, adapter, filtered=False)
            elif state.select_index == 1:
                model.select_none()
                _sync_library_selection(state, adapter)
            else:
                _request_bulk_selection(state, adapter, filtered=True)
        elif state.library_focus == 2:
            _submit_library(state, adapter)
        elif state.library_focus == 3:
            artists = model.visible_artists()
            if artists:
                artist = artists[state.artist_index]
                if action is Action.ACTIVATE:
                    _open_artist(state, adapter, artist.name)
                elif artist.indexed:
                    before_selected = sum(item.selected for item in model.albums)
                    model.toggle_artist(artist.name)
                    _runtime_trace(
                        "key.artist_toggle "
                        f"artist={artist.name!r} before_selected={before_selected} "
                        f"after_selected={sum(item.selected for item in model.albums)} "
                        f"{_library_snapshot(state)}"
                    )
                    _sync_library_selection(state, adapter)
                else:
                    _open_artist(
                        state,
                        adapter,
                        artist.name,
                        select_after_load=True,
                    )
        elif state.library_focus == 5:
            albums = model.visible_albums(active_artist_only=True)
            if albums:
                album = albums[state.album_index_cursor]
                before = album.selected
                result = model.toggle_album(album)
                _runtime_trace(
                    "key.album_toggle "
                    f"path={album.path!r} before={before} after={album.selected} "
                    f"result={result!r} {_library_snapshot(state)}"
                )
                if result == "bypass-removal-required":
                    state.dialog_kind = "library-bypass-remove"
                    state.bypass_dialog_path = album.path
                    state.bypass_dialog_enable = False
                    state.bypass_dialog_select_after = True
                    state.dialog_open = True
                elif result == "timeout-active":
                    state.transient = "Timeout-active albums remain protected during automatic selection."
                else:
                    _sync_library_selection(state, adapter)
        return True
    if action is Action.BYPASS and state.library_focus == 5:
        albums = model.visible_albums(active_artist_only=True)
        if albums:
            album = albums[state.album_index_cursor]
            removing = album.status is AlbumStatus.BYPASSED
            state.dialog_kind = (
                "library-bypass-remove" if removing else "library-bypass-add"
            )
            state.bypass_dialog_path = album.path
            state.bypass_dialog_enable = not removing
            state.bypass_dialog_select_after = False
            state.dialog_open = True
        return True
    if action is Action.BACK:
        state.transient = "Library selection remains open; choose a Scan Mode or press Ctrl+C."
        return True
    return True


def hit_test(state: TuiState, column: int, row: int) -> HitRegion | None:
    """Return the top-most current render region at one terminal cell."""
    for region in reversed(state.hit_regions):
        if region.contains(column, row):
            return region
    return None


def _scroll_hit_test(state: TuiState, column: int, row: int) -> HitRegion | None:
    for region in reversed(state.hit_regions):
        if region.target.endswith("-scroll") and region.contains(column, row):
            return region
    return None


def _apply_dialog_decision(
    state: TuiState, adapter: TuiAdapter, decision: bool
) -> None:
    if decision:
        if (
            state.dialog_kind in {"library-bypass-add", "library-bypass-remove"}
            and state.library is not None
            and state.bypass_dialog_path
            and state.bypass_dialog_enable is not None
        ):
            _respond_library_action(
                state,
                adapter,
                "set-bypass",
                album_path=state.bypass_dialog_path,
                bypassed=state.bypass_dialog_enable,
                select_after=state.bypass_dialog_select_after,
            )
            state.dialog_open = False
            state.dialog_kind = ""
            state.bypass_dialog_path = ""
            state.bypass_dialog_enable = None
            state.bypass_dialog_select_after = False
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
        elif state.dialog_kind == "bypass-remove":
            _submit(state, adapter, "unbypass")
        else:
            _submit(state, adapter, "b")
    else:
        if state.dialog_kind == "upscale" and state.ai_selection is not None:
            state.ai_selection.confirm_upscale(False)
        state.dialog_open = False
        state.dialog_kind = ""
        state.bypass_dialog_path = ""
        state.bypass_dialog_enable = None
        state.bypass_dialog_select_after = False


def _open_candidate_preview(state: TuiState, candidate_index: int) -> None:
    if not 0 <= candidate_index < len(state.candidates):
        return
    state.selected_index = candidate_index
    state.preview_modal_index = candidate_index
    state.preview_modal_open = True
    candidate = state.candidates[candidate_index]
    state.transient = (
        f"Cached candidate preview · {candidate.path}"
        if candidate.path
        else "Cached candidate preview unavailable."
    )


def close_candidate_preview(state: TuiState) -> None:
    state.preview_modal_open = False
    state.transient = "Returned from cached candidate preview."


def _focus_candidate_url(state: TuiState, candidate_index: int) -> None:
    if not 0 <= candidate_index < len(state.candidates):
        return
    state.selected_index = candidate_index
    candidate = state.candidates[candidate_index]
    if candidate.provenance != "[URL]" or not candidate.url:
        state.transient = "The highlighted candidate has no remote URL."


def osc8_link(label: str, url: str) -> str:
    """Encode one terminal-client hyperlink without permitting control bytes."""
    safe_url = "".join(character for character in url if ord(character) >= 32 and character != "\x7f")
    safe_label = label.replace("\x1b", "").replace("\x07", "")
    return f"\x1b]8;;{safe_url}\x1b\\{safe_label}\x1b]8;;\x1b\\"


def write_terminal_links(state: TuiState, writer: Any) -> None:
    """Attach terminal-owned OSC-8 metadata only to visible ``[URL]`` cells."""
    targets = [
        region
        for region in state.hit_regions
        if region.target == "candidate-url" and region.value
    ]
    if not targets:
        return
    writer.write("\x1b7")
    for region in targets:
        writer.write(
            f"\x1b[{region.y + 1};{region.x + 2}H"
            f"{osc8_link('URL', region.value)}"
        )
    writer.write("\x1b8")
    writer.flush()


def _toggle_candidate_ai(state: TuiState, candidate_index: int) -> None:
    if (
        not state.ai_enabled
        or state.ai_selection is None
        or not 0 <= candidate_index < len(state.candidates)
    ):
        return
    state.selected_index = candidate_index
    candidate = state.candidates[candidate_index]
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


def _focus_candidate(state: TuiState, index: int, adapter: TuiAdapter | None = None) -> None:
    if not state.candidates:
        return
    state.selected_index = max(0, min(index, len(state.candidates) - 1))
    state.input_buffer = str(state.selected_index + 1)
    selected = state.candidates[state.selected_index]
    for group_index, (_name, items, _semantic) in enumerate(_candidate_groups(state)):
        if selected in items:
            state.group_scroll = group_index
            state.candidate_row_scroll = _keep_visible(
                items.index(selected),
                state.candidate_row_scroll,
                state.candidate_page_size,
                len(items),
            )
            break
    if adapter is not None:
        selected = state.candidates[state.selected_index]
        if selected.provenance == "[URL]" and selected.url:
            _start_remote_source_preview(
                state,
                adapter,
                state.selected_index,
                hover_active=False,
            )


def _scroll_region(state: TuiState, region: HitRegion, delta: int) -> None:
    model = state.library
    if region.target == "artist-scroll" and model is not None:
        count = len(model.visible_artists())
        state.artist_scroll = max(0, min(state.artist_scroll + delta * 3, max(0, count - 1)))
    elif region.target == "album-scroll" and model is not None:
        count = len(model.visible_albums(active_artist_only=True))
        state.album_scroll = max(0, min(state.album_scroll + delta * 3, max(0, count - 1)))
    elif region.target == "policy-source-scroll" and state.policy is not None:
        count = len(state.policy.source_order)
        state.policy_source_scroll = max(
            0, min(state.policy_source_scroll + delta * 3, max(0, count - 1))
        )
    elif region.target == "candidate-scroll":
        groups = _candidate_groups(state)
        if not groups:
            return
        group_index = max(0, min(region.index, len(groups) - 1))
        if group_index != state.group_scroll:
            state.group_scroll = group_index
            state.candidate_row_scroll = 0
        capacity = max(1, region.height - 3)
        maximum = max(0, len(groups[state.group_scroll][1]) - capacity)
        proposed = state.candidate_row_scroll + delta * 3
        if proposed < 0 and state.group_scroll > 0:
            state.group_scroll -= 1
            state.candidate_row_scroll = 0
        elif proposed > maximum and state.group_scroll < len(groups) - 1:
            state.group_scroll += 1
            state.candidate_row_scroll = 0
        else:
            state.candidate_row_scroll = max(0, min(proposed, maximum))
    elif region.target == "selected-stats-scroll":
        # Exact maximum is clamped by the renderer on the next frame. Keep
        # wheel/touch scrolling independent of the active OLED/CHALK theme.
        state.selected_stats_scroll = max(
            0,
            state.selected_stats_scroll + delta * 3,
        )
    elif region.target == "report-scroll":
        state.report_scroll = max(0, state.report_scroll + delta * 3)


def handle_mouse(state: TuiState, adapter: TuiAdapter, event: Any) -> None:
    """Map a crossterm mouse event through current render-time geometry."""
    code = str(getattr(event, "code", "")).lower()
    column = int(getattr(event, "column", -1))
    row = int(getattr(event, "row", -1))
    if state.status_loading:
        return
    if code == "moved":
        region = hit_test(state, column, row)
        state.hover_target = region.target if region is not None else ""
        state.hover_index = region.index if region is not None else -1
        if (
            region is not None
            and region.target == "candidate-url"
            and region.value
        ):
            if (
                state.remote_hover_index != region.index
                or state.remote_hover_url != region.value
                or not state.remote_hover_active
            ):
                _start_remote_hover_preview(state, adapter, region.index)
        elif state.remote_hover_active:
            _clear_remote_hover_preview(state, adapter)
        return
    if code in {"scroll_up", "scroll_down"}:
        region = _scroll_hit_test(state, column, row)
        target = repr(region.target) if region is not None else "'none'"
        _runtime_trace(
            f"mouse.scroll code={code!r} x={column} y={row} "
            f"target={target} workflow={state.workflow!r}"
        )
        if region is not None:
            _scroll_region(state, region, -1 if code == "scroll_up" else 1)
        return
    if code != "down" or str(getattr(event, "button", "")).lower() != "left":
        return
    region = hit_test(state, column, row)
    if region is None:
        _runtime_trace(
            f"mouse.click x={column} y={row} target='none' "
            f"workflow={state.workflow!r} waiting={adapter.waiting.is_set()}"
        )
        return
    _runtime_trace(
        f"mouse.click x={column} y={row} target={region.target!r} "
        f"index={region.index} value={region.value!r} "
        f"workflow={state.workflow!r} workspace={state.workspace!r} "
        f"waiting={adapter.waiting.is_set()} {_library_snapshot(state)}"
    )
    if state.dialog_open:
        if region.target == "dialog-yes":
            _apply_dialog_decision(state, adapter, True)
        elif region.target == "dialog-no":
            _apply_dialog_decision(state, adapter, False)
        return

    model = state.library
    if region.target == "status-control" and model is not None:
        state.library_focus = 0
        state.status_index = region.index
        if region.index == 0:
            model.toggle_status(AlbumStatus.UNPROCESSED)
            model.toggle_artist_status(ArtistStatus.UNPROCESSED)
        elif region.index == 3:
            model.toggle_status(AlbumStatus.TIMEOUT)
            model.toggle_artist_status(ArtistStatus.PARTIAL)
        else:
            status = STATUS_CONTROLS[region.index][1]
            if isinstance(status, AlbumStatus):
                model.toggle_status(status)
            else:
                model.toggle_artist_status(status)
    elif region.target == "select-control" and model is not None:
        if not _library_input_ready(state, adapter):
            return
        state.library_focus = 1
        state.select_index = region.index
        _runtime_trace(
            f"mouse.select_control index={region.index} {_library_snapshot(state)}"
        )
        if region.index == 0:
            _request_bulk_selection(state, adapter, filtered=False)
        elif region.index == 1:
            model.select_none()
            _sync_library_selection(state, adapter)
        else:
            _request_bulk_selection(state, adapter, filtered=True)
    elif region.target == "scan-control" and model is not None:
        if not _library_input_ready(state, adapter):
            return
        state.library_focus = 2
        state.scan_index = region.index
        _runtime_trace(
            f"mouse.scan_control index={region.index} {_library_snapshot(state)}"
        )
        _submit_library(state, adapter)
    elif region.target == "artist-row" and model is not None:
        state.library_focus = 3
        state.artist_index = region.index
        # Touch/click on an Artist row is the selection action. Keyboard
        # Enter remains the open-only navigation path.
        _open_artist(
            state,
            adapter,
            region.value,
            select_after_load=True,
        )
    elif region.target == "album-row" and model is not None:
        if not _library_input_ready(state, adapter):
            return
        state.library_focus = 5
        state.album_index_cursor = region.index
        albums = model.visible_albums(active_artist_only=True)
        if 0 <= region.index < len(albums):
            album = albums[region.index]
            before = album.selected
            result = model.toggle_album(album)
            _runtime_trace(
                "mouse.album_row_select "
                f"path={album.path!r} before={before} after={album.selected} "
                f"result={result!r} {_library_snapshot(state)}"
            )
            if result == "bypass-removal-required":
                state.dialog_kind = "library-bypass-remove"
                state.bypass_dialog_path = album.path
                state.bypass_dialog_enable = False
                state.bypass_dialog_select_after = True
                state.dialog_open = True
            elif result == "timeout-active":
                state.transient = (
                    "Timeout-active albums remain protected during automatic selection."
                )
            else:
                _sync_library_selection(state, adapter)
    elif region.target == "artist-filter":
        state.library_focus = 4
    elif region.target == "album-filter":
        state.library_focus = 6
    elif region.target == "source-policy-open":
        state.workspace = "policy"
        state.library_focus = 0
    elif region.target == "settings-edit":
        state.transient = "Opening Config v5 in micro…"
        _respond_library_action(state, adapter, "edit-config")
    elif region.target == "library-refresh":
        state.transient = "Refreshing Artist folder list…"
        _respond_library_action(state, adapter, "refresh-index")
    elif region.target == "policy-source" and state.policy is not None:
        state.library_focus = 0
        state.artist_index = region.index
    elif region.target in {"policy-move-earlier", "policy-move-later"} and state.policy is not None:
        delta = -1 if region.target.endswith("earlier") else 1
        state.policy.move(region.value, delta)
        state.artist_index = state.policy.source_order.index(region.value)
    elif region.target == "policy-field" and state.policy is not None:
        state.library_focus = 1
        state.album_index_cursor = region.index
        source = state.policy.source_order[state.artist_index]
        if region.value in {"enabled", "source_override", "allow_below_minimum_fallback"}:
            state.policy.toggle(source, region.value)
        elif region.value == "primary_image_only":
            if source in PRIMARY_METADATA_SOURCES:
                state.policy.toggle(source, region.value)
            else:
                state.transient = "This provider does not expose differentiated primary-image metadata."
        elif region.value == "minimum_range_type":
            state.policy.cycle_range(source, 1)
    elif region.target == "policy-global-field":
        state.library_focus = 2
        state.status_index = region.index
    elif region.target in {"candidate-row", "preferred-candidate"}:
        state.selected_index = region.index
    elif region.target == "candidate-url":
        _focus_candidate_url(state, region.index)
    elif region.target == "preview-modal-close":
        close_candidate_preview(state)
    elif region.target == "candidate-ai":
        _toggle_candidate_ai(state, region.index)


def handle_key(state: TuiState, adapter: TuiAdapter, event: Any) -> None:
    code = str(event.code)
    if bool(getattr(event, "ctrl", False)) and code.lower() == "c":
        state.exit_code = 130
        state.exit_requested = True
        adapter.cancel_wait()
        return
    if state.status_loading:
        return
    if state.preview_modal_open:
        if str(event.code).lower() in {"esc", "escape", "enter", "return", "space", " "}:
            close_candidate_preview(state)
        return
    if state.dialog_open:
        decision = confirm_key(code)
        if decision is not None:
            _apply_dialog_decision(state, adapter, decision)
        return

    action = map_key(
        code,
        ctrl=bool(getattr(event, "ctrl", False)),
        shift=bool(getattr(event, "shift", False)),
    )
    _runtime_trace(
        f"key code={code!r} action={action.value!r} "
        f"workflow={state.workflow!r} workspace={state.workspace!r} "
        f"waiting={adapter.waiting.is_set()} {_library_snapshot(state)}"
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
        next_tab = tabs[(tabs.index(state.tab) + step) % len(tabs)]
        if next_tab != "main":
            _clear_remote_hover_preview(state)
        state.tab = next_tab
        return

    request = state.input_request
    if request is None:
        if action is Action.QUIT:
            state.transient = "Scan is active; use Ctrl+C to stop safely."
        return

    if request.kind == "batch-summary":
        if action in {Action.UP, Action.DOWN, Action.PAGE_UP, Action.PAGE_DOWN, Action.HOME, Action.END}:
            if action is Action.HOME:
                state.report_scroll = 0
            elif action is Action.END:
                state.report_scroll = 1_000_000
            else:
                amount = 1 if action in {Action.UP, Action.DOWN} else max(1, state.report_page_size - 2)
                direction = -1 if action in {Action.UP, Action.PAGE_UP} else 1
                state.report_scroll = max(0, state.report_scroll + direction * amount)
        elif action in {Action.ACTIVATE, Action.BACK}:
            adapter.respond("continue")
            state.input_request = None
            state.transient = "Returning to the retained Select Media session…"
        elif action is Action.QUIT:
            adapter.respond("exit")
            state.input_request = None
            state.exit_after_worker = True
            state.transient = "Closing SPLINED after the engine session stops safely…"
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
        index = (state.selected_index - 1) % selection_count
        if state.candidates:
            _focus_candidate(state, index, adapter)
        else:
            state.selected_index = index
            state.input_buffer = str(index + 1)
        return
    if action is Action.DOWN and selection_count:
        index = (state.selected_index + 1) % selection_count
        if state.candidates:
            _focus_candidate(state, index, adapter)
        else:
            state.selected_index = index
            state.input_buffer = str(index + 1)
        return
    if action in {Action.PAGE_UP, Action.PAGE_DOWN, Action.HOME, Action.END} and selection_count:
        if action is Action.HOME:
            index = 0
        elif action is Action.END:
            index = selection_count - 1
        else:
            index = state.selected_index + (-10 if action is Action.PAGE_UP else 10)
        if state.candidates:
            _focus_candidate(state, index, adapter)
        else:
            state.selected_index = max(0, min(index, selection_count - 1))
            state.input_buffer = str(state.selected_index + 1)
        return
    if action is Action.DIGIT and selection_count:
        proposed = (state.input_buffer + code)[-2:]
        number = int(proposed)
        index: int | None = None
        if 1 <= number <= selection_count:
            state.input_buffer = proposed
            index = number - 1
        elif code != "0" and int(code) <= selection_count:
            state.input_buffer = code
            index = int(code) - 1
        if index is not None:
            if state.candidates:
                _focus_candidate(state, index, adapter)
            else:
                state.selected_index = index
        return
    if action is Action.BYPASS:
        current = (
            next(
                (
                    item
                    for item in state.library.albums
                    if item.path == state.album_path
                ),
                None,
            )
            if state.library is not None
            else None
        )
        state.dialog_kind = (
            "bypass-remove"
            if current is not None and current.status is AlbumStatus.BYPASSED
            else "bypass"
        )
        state.dialog_open = True
        return
    if action is Action.BACK:
        if request.kind == "musicbrainz":
            _submit(state, adapter, "b")
        else:
            _submit(state, adapter, "__cancel__")
        return
    if action is Action.URL and state.candidates:
        _focus_candidate_url(state, state.selected_index)
        return
    if action is Action.TOGGLE and state.ai_enabled and state.ai_selection and state.candidates:
        _toggle_candidate_ai(state, state.selected_index)
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
    if InputEventReader is None:
        raise TuiInitializationError(
            "SPLINED's pyratatui mouse/input extension is not installed; "
            "install the splined-pyratatui-input wheel."
        )
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
        input_reader: Any = None
        try:
            terminal = Terminal()
            input_reader = InputEventReader()
            with terminal, input_reader:
                state.image_protocol = str(
                    getattr(input_reader, "image_protocol", "unknown")
                )
                _runtime_trace(
                    "image_protocol "
                    f"protocol={state.image_protocol!r}"
                )
                thread.start()
                dirty = True
                last_animation_step = -1
                while not state.exit_requested and not terminated:
                    dirty = _drain(adapter, state) or dirty
                    current_animation_step = (
                        animation_step(time.monotonic() - state.started_at)
                        if state.workflow == "startup"
                        else -1
                    )
                    if current_animation_step != last_animation_step:
                        dirty = True
                    last_animation_step = current_animation_step
                    if dirty:
                        _clear_stale_remote_overlay(state)
                        terminal.draw(lambda frame: render(frame, state, theme))
                        if (
                            state.tab == "main"
                            and state.workflow in {"candidates", "picker"}
                            and state.candidates
                            and state.remote_hover_index < 0
                            and not state.remote_hover_loading
                        ):
                            _start_preferred_preview(state, adapter)
                        _draw_remote_hover_overlay(state)
                        writer = getattr(sys, "__stdout__", None)
                        if writer is not None:
                            write_terminal_links(state, writer)
                        dirty = False
                    event = input_reader.poll_event(timeout_ms=80)
                    if event is not None:
                        event_kind = str(getattr(event, "kind", "key"))
                        if event_kind == "mouse":
                            handle_mouse(state, adapter, event)
                        elif event_kind == "key":
                            handle_key(state, adapter, event)
                        elif event_kind == "resize":
                            if state.remote_drawn_rect is not None:
                                _clear_stale_remote_overlay(
                                    state
                                )
                                if state.remote_drawn_rect is not None:
                                    if native_clear_image_area is not None:
                                        try:
                                            native_clear_image_area(
                                                *state.remote_drawn_rect
                                            )
                                        except Exception:
                                            pass
                                    state.remote_drawn_rect = None
                            state.remote_preview_rect = None
                        dirty = True
        except BaseException as exc:
            if thread.ident is None:
                if input_reader is not None:
                    try:
                        input_reader.disable_mouse_capture()
                    except Exception:
                        pass
                if terminal is not None:
                    try:
                        terminal.restore()
                    except Exception:
                        pass
                if emergency_terminal_restore is not None:
                    try:
                        emergency_terminal_restore()
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
