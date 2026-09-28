"""Durable first-build checkpoints and explicit SQLite build presentation.

The tag-identified media index remains atomically promoted only after a complete
scan, but inspected Album rows are checkpointed inside ``splined.db`` while the
build is running. An interrupted first build can therefore resume without
re-reading every completed Album, and the database visibly records progress.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys
import time
from typing import Any

import splined_media_database as database


_STAGE_TYPE = "media-index-stage"
_PROGRESS_ACTION = "media-index-build-progress"
_STAGE_BATCH = 16
_INSTALLED = False


def _signature_token(value: dict[str, Any]) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def _stage_cache_key(path: str) -> str:
    digest = hashlib.sha256(path.encode("utf-8", "surrogateescape")).hexdigest()
    return f"{_STAGE_TYPE}:{digest}"


def _representative_state(album: Any) -> tuple[str, int, int]:
    audio = sorted(
        (Path(value) for value in getattr(album, "audio_files", [])),
        key=lambda value: str(value).casefold(),
    )
    if not audio:
        return "", 0, 0
    representative = audio[0]
    try:
        stat = representative.stat()
        return str(representative), int(stat.st_size), int(stat.st_mtime_ns)
    except OSError:
        return str(representative), 0, 0


def _load_stage_payloads(
    connection: Any,
    signature_token: str,
) -> dict[str, dict[str, Any]]:
    rows = connection.execute(
        "SELECT payload_json FROM cache_entries WHERE cache_type=?",
        (_STAGE_TYPE,),
    )
    result: dict[str, dict[str, Any]] = {}
    for row in rows:
        try:
            payload = json.loads(str(row["payload_json"]))
        except (TypeError, ValueError):
            continue
        if (
            isinstance(payload, dict)
            and payload.get("signature") == signature_token
            and str(payload.get("path", ""))
        ):
            result[str(payload["path"])] = payload
    return result


def _stage_payload_matches(
    payload: dict[str, Any],
    signature_token: str,
    album: Any,
) -> bool:
    representative, size, modified = _representative_state(album)
    return bool(
        payload.get("signature") == signature_token
        and str(payload.get("path", "")) == str(album.path)
        and str(payload.get("representative_file", "")) == representative
        and int(payload.get("representative_size", 0) or 0) == size
        and int(payload.get("representative_mtime_ns", 0) or 0) == modified
        and isinstance(payload.get("artist"), dict)
        and isinstance(payload.get("album"), dict)
    )


def _write_stage_batch(
    connection: Any,
    payloads: list[dict[str, Any]],
    *,
    signature_token: str,
    version: str,
    reason: str,
    phase: str,
    processed: int,
    total: int,
    discovered: int,
    tag_reads: int,
    tag_reuses: int,
    checkpoint_reuses: int,
) -> None:
    now = database.utc_now()
    details = {
        "signature": signature_token,
        "reason": reason,
        "phase": phase,
        "processed": int(processed),
        "total": int(total),
        "discovered": int(discovered),
        "tag_reads": int(tag_reads),
        "tag_reuses": int(tag_reuses),
        "checkpoint_reuses": int(checkpoint_reuses),
    }
    with connection:
        for payload in payloads:
            album = payload.get("album") or {}
            path = str(payload.get("path", ""))
            connection.execute(
                "INSERT INTO cache_entries"
                "(cache_key, cache_type, album_key, payload_json, "
                "splined_version, created_at, updated_at) "
                "VALUES(?, ?, ?, ?, ?, ?, ?) "
                "ON CONFLICT(cache_key) DO UPDATE SET "
                "cache_type=excluded.cache_type, "
                "album_key=excluded.album_key, "
                "payload_json=excluded.payload_json, "
                "splined_version=excluded.splined_version, "
                "updated_at=excluded.updated_at",
                (
                    _stage_cache_key(path),
                    _STAGE_TYPE,
                    str(album.get("album_key", "")) or None,
                    json.dumps(payload, default=str, separators=(",", ":")),
                    version,
                    now,
                    now,
                ),
            )
        connection.execute(
            "INSERT INTO db_maintenance_state"
            "(action_name, completed_at, details_json, splined_version) "
            "VALUES(?, ?, ?, ?) "
            "ON CONFLICT(action_name) DO UPDATE SET "
            "completed_at=excluded.completed_at, "
            "details_json=excluded.details_json, "
            "splined_version=excluded.splined_version",
            (
                _PROGRESS_ACTION,
                now,
                json.dumps(details, separators=(",", ":")),
                version,
            ),
        )


def _prepare_stage_scope(connection: Any, signature_token: str) -> None:
    row = connection.execute(
        "SELECT details_json FROM db_maintenance_state WHERE action_name=?",
        (_PROGRESS_ACTION,),
    ).fetchone()
    saved_signature = ""
    if row is not None:
        try:
            details = json.loads(str(row["details_json"]))
            if isinstance(details, dict):
                saved_signature = str(details.get("signature", ""))
        except (TypeError, ValueError):
            saved_signature = ""
    if saved_signature and saved_signature != signature_token:
        with connection:
            connection.execute(
                "DELETE FROM cache_entries WHERE cache_type=?",
                (_STAGE_TYPE,),
            )
            connection.execute(
                "DELETE FROM db_maintenance_state WHERE action_name=?",
                (_PROGRESS_ACTION,),
            )


def _clear_stage(connection: Any) -> None:
    with connection:
        connection.execute(
            "DELETE FROM cache_entries WHERE cache_type=?",
            (_STAGE_TYPE,),
        )
        connection.execute(
            "DELETE FROM db_maintenance_state WHERE action_name=?",
            (_PROGRESS_ACTION,),
        )


def _format_eta(seconds: float | None) -> str:
    if seconds is None or seconds < 0:
        return "calculating"
    rounded = int(seconds)
    hours, remainder = divmod(rounded, 3600)
    minutes, secs = divmod(remainder, 60)
    if hours:
        return f"{hours:d}h {minutes:02d}m"
    if minutes:
        return f"{minutes:d}m {secs:02d}s"
    return f"{secs:d}s"


def build_index(
    context: database.IndexContext,
    connection: Any,
    reason: str,
) -> None:
    """Build the SQLite read model with resumable Album checkpoints."""
    core = context.core
    version = str(core.display_version())
    expected_signature = database.signature(
        context.library_root,
        context.ignored,
        context.cover_name,
    )
    signature_token = _signature_token(expected_signature)
    _prepare_stage_scope(connection, signature_token)

    core.emit_ui(
        "cache_build_start",
        cache_kind="media-index",
        reason=reason,
        database=str(context.db_path),
        resumable=True,
    )
    core.emit_ui(
        "activity",
        category="inventory",
        state="start",
        source="splined-db",
        message=(
            "Building SPLINED tag index"
            if reason == "initial-build"
            else "Refreshing SPLINED tag index"
        ),
    )

    last_inventory_checkpoint = 0.0

    def progress(directories: int, album_count: int) -> None:
        nonlocal last_inventory_checkpoint
        core.emit_ui(
            "cache_progress",
            phase="inventory",
            reason=reason,
            database=str(context.db_path),
            status="Discovering Artist / Album folders",
            processed=directories,
            total=0,
            percent=None,
            albums=album_count,
            staged=0,
            recovered=0,
            current_artist="",
        )
        now = time.monotonic()
        if now - last_inventory_checkpoint >= 2.0:
            _write_stage_batch(
                connection,
                [],
                signature_token=signature_token,
                version=version,
                reason=reason,
                phase="inventory",
                processed=directories,
                total=0,
                discovered=album_count,
                tag_reads=0,
                tag_reuses=0,
                checkpoint_reuses=0,
            )
            last_inventory_checkpoint = now

    albums, _ignored_dirs = context.original_inventory(
        context.library_root,
        context.ignored,
        context.cover_name,
        progress=progress,
        workers=getattr(core, "INVENTORY_WORKERS", 8),
        cancelled=getattr(core, "tui_cancelled", None),
    )
    existing_rows = list(
        connection.execute(
            "SELECT albums.*, artists.artist_name, artists.artist_sort, "
            "artists.musicbrainz_artistid, artists.primary_path "
            "FROM albums JOIN artists "
            "ON artists.artist_key=albums.artist_key"
        )
    )
    existing = {str(row["album_key"]): row for row in existing_rows}
    existing_by_path = {str(row["path"]): row for row in existing_rows}
    staged_by_path = _load_stage_payloads(connection, signature_token)

    artist_rows: dict[str, dict[str, Any]] = {}
    album_rows: list[dict[str, Any]] = []
    reviews: list[dict[str, Any]] = []
    used_keys: dict[str, str] = {}
    pending_stage: list[dict[str, Any]] = []
    tag_reads = 0
    tag_reuses = 0
    checkpoint_reuses = 0
    total = len(albums)
    started = time.perf_counter()

    for number, album in enumerate(albums, 1):
        if getattr(core, "tui_cancelled", lambda: False)():
            if pending_stage:
                _write_stage_batch(
                    connection,
                    pending_stage,
                    signature_token=signature_token,
                    version=version,
                    reason=reason,
                    phase="tag-index",
                    processed=number - 1,
                    total=total,
                    discovered=total,
                    tag_reads=tag_reads,
                    tag_reuses=tag_reuses,
                    checkpoint_reuses=checkpoint_reuses,
                )
            raise core.TuiSessionExit()
        if not album.audio_files:
            continue

        path_text = str(album.path)
        staged = staged_by_path.get(path_text)
        if staged is not None and _stage_payload_matches(
            staged,
            signature_token,
            album,
        ):
            artist = dict(staged["artist"])
            album_row = dict(staged["album"])
            review_raw = staged.get("review")
            review = dict(review_raw) if isinstance(review_raw, dict) else None
            checkpoint_reuses += 1
            base_key = str(album_row.get("album_key", "")).split(
                ":copy:", 1
            )[0]
            if base_key:
                used_keys.setdefault(base_key, path_text)
        else:
            artist, album_row, review, reused_tags = database.inspect_album(
                album,
                context.library_root,
                context.cover_name,
                existing,
                existing_by_path,
                used_keys,
            )
            if reused_tags:
                tag_reuses += 1
            else:
                tag_reads += 1
            pending_stage.append(
                {
                    "signature": signature_token,
                    "path": path_text,
                    "representative_file": album_row["representative_file"],
                    "representative_size": album_row["representative_size"],
                    "representative_mtime_ns": album_row[
                        "representative_mtime_ns"
                    ],
                    "artist": artist,
                    "album": album_row,
                    "review": review,
                }
            )

        current = artist_rows.get(artist["artist_key"])
        if current is None:
            artist_rows[artist["artist_key"]] = artist
        elif (
            not current["musicbrainz_artistid"]
            and artist["musicbrainz_artistid"]
        ):
            current.update(
                artist_name=artist["artist_name"],
                artist_sort=artist["artist_sort"],
                musicbrainz_artistid=artist["musicbrainz_artistid"],
            )
        album_rows.append(album_row)
        if review is not None:
            reviews.append(review)

        if pending_stage and (
            len(pending_stage) >= _STAGE_BATCH or number == total
        ):
            _write_stage_batch(
                connection,
                pending_stage,
                signature_token=signature_token,
                version=version,
                reason=reason,
                phase="tag-index",
                processed=number,
                total=total,
                discovered=total,
                tag_reads=tag_reads,
                tag_reuses=tag_reuses,
                checkpoint_reuses=checkpoint_reuses,
            )
            pending_stage.clear()

        if number == 1 or number % _STAGE_BATCH == 0 or number == total:
            elapsed = max(0.001, time.perf_counter() - started)
            rate = number / elapsed
            remaining = max(0, total - number)
            eta = remaining / rate if rate > 0 else None
            core.emit_ui(
                "cache_progress",
                phase="tag-index",
                reason=reason,
                database=str(context.db_path),
                status=(
                    "Reading representative Album tags · "
                    f"{tag_reads:,} read · {tag_reuses:,} DB reused · "
                    f"{checkpoint_reuses:,} checkpoint reused"
                ),
                processed=number,
                total=total,
                percent=(number / total * 100.0 if total else 100.0),
                albums=number,
                staged=number,
                recovered=checkpoint_reuses,
                elapsed_seconds=elapsed,
                rate_per_second=rate,
                eta_seconds=eta,
                eta_text=_format_eta(eta),
                current_artist=artist["artist_name"],
            )

    database._insert_snapshot(
        connection,
        artist_rows,
        album_rows,
        reviews,
        expected_signature,
        version,
        reason,
    )
    _clear_stage(connection)
    core.emit_ui(
        "activity",
        category="inventory",
        state="done",
        source="splined-db",
        message=(
            f"SPLINED index ready · {len(artist_rows):,} Artist(s) · "
            f"{len(album_rows):,} Album(s) · "
            f"{tag_reads:,} tag read(s) · {tag_reuses:,} DB reused · "
            f"{checkpoint_reuses:,} checkpoint reused"
        ),
    )


def _patch_tui(module: Any) -> None:
    if getattr(module, "_splined_database_build_presentation", False):
        return

    original_apply = module.TuiState.apply
    original_render = module._render_startup

    def apply(state: Any, event: str, payload: dict[str, Any]) -> None:
        original_apply(state, event, payload)
        if event == "cache_build_start":
            state.cache_kind = str(payload.get("cache_kind", ""))
            state.cache_reason = str(payload.get("reason", ""))
            state.cache_database = str(payload.get("database", ""))
            state.cache_resumable = bool(payload.get("resumable", False))
            state.cache_staged = 0
            state.cache_recovered = 0
            state.cache_eta_text = "calculating"
            state.cache_rate = 0.0
        elif event == "cache_progress":
            state.cache_reason = str(
                payload.get("reason", getattr(state, "cache_reason", ""))
            )
            state.cache_database = str(
                payload.get(
                    "database",
                    getattr(state, "cache_database", ""),
                )
            )
            state.cache_staged = int(payload.get("staged", 0) or 0)
            state.cache_recovered = int(payload.get("recovered", 0) or 0)
            state.cache_eta_text = str(
                payload.get("eta_text", "calculating")
            )
            state.cache_rate = float(
                payload.get("rate_per_second", 0.0) or 0.0
            )

    def render_startup(
        frame: Any,
        state: Any,
        theme: Any,
    ) -> None:
        original_render(frame, state, theme)
        if getattr(state, "cache_kind", "") != "media-index":
            return

        area = frame.area
        brand_height = min(
            int(area.height),
            module.startup_brand_height(area.width, area.height),
        )
        _brand, inventory_area = module._split_vertical(
            area,
            [module.Constraint.length(brand_height), module.Constraint.fill(1)],
        )
        panel_width = min(max(60, int(inventory_area.width) - 8), 96)
        panel_height = min(13, int(inventory_area.height))
        panel = module.Rect(
            int(inventory_area.x)
            + max(0, (int(inventory_area.width) - panel_width) // 2),
            int(inventory_area.y)
            + max(0, (int(inventory_area.height) - panel_height) // 2),
            panel_width,
            panel_height,
        )
        frame.render_widget(module.Clear(), panel)

        reason = getattr(state, "cache_reason", "")
        first_build = reason == "initial-build"
        title = (
            "BUILDING SPLINED DATABASE"
            if first_build
            else "REFRESHING SPLINED DATABASE"
        )
        frame.render_widget(
            module.card(theme, title, module.Semantic.SPECIAL),
            panel,
        )
        inner = module.Rect(
            int(panel.x) + 2,
            int(panel.y) + 1,
            max(1, int(panel.width) - 4),
            max(1, int(panel.height) - 2),
        )
        sections = module._split_vertical(
            inner,
            [
                module.Constraint.length(4),
                module.Constraint.length(2),
                module.Constraint.fill(1),
            ],
        )

        processed = int(getattr(state, "cache_processed", 0) or 0)
        total = int(getattr(state, "cache_total", 0) or 0)
        percent = float(getattr(state, "cache_percent", 0.0) or 0.0)
        staged = int(getattr(state, "cache_staged", 0) or 0)
        recovered = int(getattr(state, "cache_recovered", 0) or 0)
        one_time = (
            "ONE-TIME PROCESS · PLEASE WAIT"
            if first_build
            else "EXPLICIT LIBRARY REFRESH · PLEASE WAIT"
        )
        detail = (
            f"Albums {processed:,} / {total:,}"
            if total
            else f"Albums discovered: {int(getattr(state, 'cache_albums', 0) or 0):,}"
        )
        header = module.Text(
            [
                module.Line(
                    [
                        module.Span(
                            one_time,
                            module.style(
                                theme,
                                module.Semantic.FALLBACK,
                                bold=True,
                            ),
                        )
                    ]
                ).centered(),
                module.Line(
                    [
                        module.Span(
                            str(getattr(state, "inventory_status", "")),
                            module.style(theme, module.Semantic.ACTIVE, bold=True),
                        )
                    ]
                ).centered(),
                module.Line(
                    [
                        module.Span(
                            detail,
                            module.style(theme, module.Semantic.TEXT, bold=True),
                        )
                    ]
                ).centered(),
            ]
        )
        frame.render_widget(module.Paragraph(header), sections[0])
        frame.render_widget(
            module.Gauge()
            .ratio(max(0.0, min(1.0, percent / 100.0)))
            .label(f"{percent:.1f}%" if total else "DISCOVERING")
            .gauge_style(
                module.style(theme, module.Semantic.ACCEPTED, bold=True)
            )
            .style(module.panel_style(theme))
            .use_unicode(True),
            sections[1],
        )

        database_path = str(
            getattr(state, "cache_database", "") or "/_cache/splined.db"
        )
        current_artist = str(getattr(state, "inventory_artist", ""))
        eta = str(getattr(state, "cache_eta_text", "calculating"))
        rate = float(getattr(state, "cache_rate", 0.0) or 0.0)
        checkpoint = (
            f"SQLite checkpoints: {staged:,} saved"
            + (f" · {recovered:,} resumed" if recovered else "")
        )
        performance = (
            f"Rate: {rate:.2f} Album/s · ETA: {eta}"
            if rate > 0
            else "Preparing durable SQLite checkpoints…"
        )
        footer_lines = [
            module.Line(
                [
                    module.Span(
                        checkpoint,
                        module.style(theme, module.Semantic.ACCEPTED, bold=True),
                    )
                ]
            ).centered(),
            module.Line(
                [module.Span(performance, module.style(theme, module.Semantic.TEXT))]
            ).centered(),
            module.Line(
                [
                    module.Span(
                        module._truncate(
                            (
                                f"{current_artist} · " if current_artist else ""
                            )
                            + database_path,
                            max(1, int(inner.width) - 2),
                        ),
                        module.style(theme, module.Semantic.MUTED),
                    )
                ]
            ).centered(),
            module.Line(
                [
                    module.Span(
                        "Ctrl+C is safe · the next launch resumes saved Albums",
                        module.style(theme, module.Semantic.MUTED),
                    )
                ]
            ).centered(),
        ]
        frame.render_widget(
            module.Paragraph(module.Text(footer_lines)),
            sections[2],
        )

    module.TuiState.apply = apply
    module._render_startup = render_startup
    module._splined_database_build_presentation = True


def install(core: Any) -> None:
    """Install resumable DB building before the media-index runtime is wired."""
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True

    import splined_media_index

    splined_media_index.build_index = build_index
    original = core.run_operational_interface

    def run_operational_interface(args: Any, worker: Any) -> int:
        likely_tui = (
            not bool(getattr(args, "no_tui", False))
            and bool(getattr(sys.stdin, "isatty", lambda: False)())
            and bool(getattr(sys.stdout, "isatty", lambda: False)())
        )
        if likely_tui:
            try:
                from tui import splined_tui

                _patch_tui(splined_tui)
            except ImportError:
                pass
        return int(original(args, worker))

    core.run_operational_interface = run_operational_interface
    core._splined_media_build_policy_installed = True
