"""Install the tag-identified SQLite Select Media index into SPLINED."""

from __future__ import annotations

import json
from pathlib import Path
import sqlite3
from typing import Any, Callable

from splined_media_database import (
    IndexContext,
    build_index,
    connect,
    database_path,
    index_is_usable,
    signature,
)
from splined_media_runtime import (
    cached_stats,
    clean_transient_cache,
    migrate_legacy_json,
    populate_session,
    set_active_database,
    set_current_album,
    sync_library_payload,
    sync_material_result,
)


_INSTALLED = False
_ACTIVE_CONTEXT: IndexContext | None = None


def _resident_session_usable(
    session: Any | None,
    *,
    library_root: Path,
    database: Path,
) -> bool:
    """Return whether Select Media is already resident for this library/DB.

    The authoritative picker loop intentionally passes the same
    ``PickerSessionState`` back after an Album Run Report. Reopening SQLite and
    rebuilding 1,000+ Artist / 3,000+ Album objects at that boundary defeats the
    retained-session contract and leaves the report screen apparently stuck.
    """
    return bool(
        session is not None
        and bool(getattr(session, "ready", False))
        and str(getattr(session, "library_root", "")) == str(library_root)
        and str(getattr(session, "picker_path", "")) == str(database)
    )


def _wal_bytes(database: Path) -> int:
    try:
        return int(Path(str(database) + "-wal").stat().st_size)
    except OSError:
        return 0


def _checkpoint_wal(
    core: Any,
    connection: sqlite3.Connection,
    *,
    reason: str,
) -> None:
    """Merge completed build/refresh WAL pages into ``splined.db``.

    ``splined.db-wal`` and ``splined.db-shm`` are normal SQLite sidecars, not
    separate databases. A final TRUNCATE checkpoint keeps their retained size
    small after a large first build or explicit refresh and avoids carrying a
    large completed WAL into the next launch.
    """
    try:
        row = connection.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
        busy, log_pages, checkpointed = (
            (int(row[0]), int(row[1]), int(row[2]))
            if row is not None and len(row) >= 3
            else (0, 0, 0)
        )
        core.debug_log(
            "splined.db.wal_checkpoint "
            f"reason={reason!r} busy={busy} log_pages={log_pages} "
            f"checkpointed={checkpointed}"
        )
    except sqlite3.Error as exc:
        # The active snapshot is already committed. A failed maintenance
        # checkpoint must not invalidate it or prevent Select Media startup.
        core.debug_log(
            "splined.db.wal_checkpoint_error "
            f"reason={reason!r} error={type(exc).__name__}: {exc}"
        )


def install(core: Any, scan: Any | None = None) -> None:
    """Install ``splined.db`` without changing plain CLI scan authority."""
    del scan
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True

    original_prepare_selection = core.prepare_tui_library_selection
    original_read_input = core.read_input
    original_emit_ui = core.emit_ui
    original_selected_stats = core.selected_album_statistics
    original_inventory = core.inventory

    def prepare_run_cache(cache: Path) -> None:
        clean_transient_cache(core, cache)

    def selected_album_statistics(
        path: Path,
        *,
        cover_name: str = "cover",
    ) -> dict[str, Any]:
        cached = cached_stats(path)
        if cached is not None:
            return cached
        return original_selected_stats(path, cover_name=cover_name)

    def emit_ui(event: str, **payload: Any) -> None:
        if event == "album":
            set_current_album(str(payload.get("path", "")))
        elif event == "album_material_result":
            # READ evaluates a possible result but does not install cover.* or
            # create durable processed authority. The database remains a
            # materialized view of actual local artwork and Write outcomes.
            outcome = str(payload.get("outcome", "")).casefold()
            if "read-only" not in outcome:
                try:
                    sync_material_result(payload)
                except Exception as exc:
                    core.debug_log(
                        "splined.db.material_sync_error "
                        f"error={type(exc).__name__}: {exc}"
                    )
        elif event == "library_update":
            # The first library payload must paint immediately. Warm-start
            # projection already populated the in-memory status authority, so
            # there is no reason to perform a whole-library SQLite write before
            # Select Media becomes visible. Later explicit UI changes may sync
            # only genuine status deltas.
            try:
                sync_library_payload(payload)
            except Exception as exc:
                core.debug_log(
                    "splined.db.status_sync_error "
                    f"error={type(exc).__name__}: {exc}"
                )
        original_emit_ui(event, **payload)

    def read_input(prompt: str, **context_payload: Any) -> str:
        raw = original_read_input(prompt, **context_payload)
        context = _ACTIVE_CONTEXT
        if (
            context is None
            or str(context_payload.get("kind", "")) != "library-selection"
        ):
            return raw
        try:
            response = json.loads(raw)
        except (TypeError, ValueError):
            return raw
        if (
            not isinstance(response, dict)
            or response.get("action") != "refresh-index"
        ):
            return raw

        connection = connect(
            context.db_path,
            str(core.display_version()),
        )
        try:
            build_index(context, connection, "explicit-refresh")
            populate_session(context, connection)
            migrate_legacy_json(context)
            _checkpoint_wal(core, connection, reason="explicit-refresh")
        finally:
            connection.close()
        response["action"] = "selection-change"
        response["selected"] = sorted(
            context.session.selected_paths,
            key=str.casefold,
        )
        core.emit_ui(
            "activity",
            category="inventory",
            state="done",
            source="splined-db",
            message="SPLINED media index refreshed atomically",
        )
        return json.dumps(response, separators=(",", ":"))

    def prepare_tui_library_selection(
        config_file: Path,
        cfg: dict[str, Any],
        sources: list[str],
        root: Path,
        completion_history: dict[str, Any],
        timeout_hours: float,
        *,
        cache: Path,
        library_root: Path | None = None,
        bypassed_paths: set[str] | None = None,
        bypass_update: Callable[[str, bool], None] | None = None,
        picker_session: Any | None = None,
        initial_event: str = "library",
    ):
        global _ACTIVE_CONTEXT
        session = picker_session or core.PickerSessionState()
        actual_library_root = Path(library_root or root)
        library = core.section(cfg, "library")
        output = core.section(cfg, "output")
        ignored = [str(value) for value in library.get("ignored_subs", [])]
        cover_name = str(output.get("file_name", "cover")).strip() or "cover"
        db = database_path(Path(cache))
        fingerprint_paths = core.timeout_fingerprint_paths(
            completion_history,
            cfg,
            sources,
            timeout_hours,
        )

        def indexed_inventory(
            inventory_root: Path,
            ignored_subs: list[str],
            configured_file_name: str = "cover",
            **kwargs: Any,
        ):
            kwargs.setdefault("fingerprint_paths", fingerprint_paths)
            return original_inventory(
                inventory_root,
                ignored_subs,
                configured_file_name,
                **kwargs,
            )

        context = IndexContext(
            core=core,
            config_file=Path(config_file),
            cfg=cfg,
            sources=sources,
            root=Path(root),
            library_root=actual_library_root,
            cache=Path(cache),
            db_path=db,
            completion_history=completion_history,
            timeout_hours=timeout_hours,
            ignored=ignored,
            cover_name=cover_name,
            bypassed_paths=(
                bypassed_paths if bypassed_paths is not None else set()
            ),
            session=session,
            original_inventory=indexed_inventory,
        )

        reused_resident = _resident_session_usable(
            picker_session,
            library_root=actual_library_root,
            database=db,
        )
        if reused_resident:
            core.debug_log(
                "splined.db.retained_session_reuse "
                f"artists={len(session.artists)} "
                f"albums={len(session.album_records)} "
                f"selected={len(session.selected_paths)}"
            )
        else:
            connection = connect(db, str(core.display_version()))
            rebuilt = False
            try:
                expected = signature(
                    actual_library_root,
                    ignored,
                    cover_name,
                )
                usable = index_is_usable(connection, expected)
                if not usable:
                    build_index(context, connection, "initial-build")
                    rebuilt = True
                else:
                    pending_wal = _wal_bytes(db)
                    if pending_wal:
                        core.emit_ui(
                            "cache_progress",
                            phase="load",
                            status=(
                                "Consolidating completed SQLite write log · "
                                f"{pending_wal / 1_048_576:.1f} MiB"
                            ),
                            processed=0,
                            total=0,
                            percent=0.0,
                            albums=0,
                            staged=0,
                            recovered=0,
                            current_artist="",
                        )
                        _checkpoint_wal(core, connection, reason="warm-start")
                populate_session(context, connection)
                migrate_legacy_json(context)
                if rebuilt:
                    _checkpoint_wal(core, connection, reason="initial-build")
            finally:
                connection.close()

        _ACTIVE_CONTEXT = context
        set_active_database(db, cover_name)
        core.debug_log(
            f"splined.db.ready path={str(db)!r} "
            f"artists={len(session.artists)} "
            f"albums={len(session.album_records)} "
            f"retained={reused_resident}"
        )
        try:
            return original_prepare_selection(
                config_file,
                cfg,
                sources,
                root,
                completion_history,
                timeout_hours,
                cache=cache,
                library_root=library_root,
                bypassed_paths=bypassed_paths,
                bypass_update=bypass_update,
                picker_session=session,
                initial_event=initial_event,
            )
        finally:
            _ACTIVE_CONTEXT = None

    core.prepare_run_cache = prepare_run_cache
    core.prepare_tui_library_selection = prepare_tui_library_selection
    core.read_input = read_input
    core.emit_ui = emit_ui
    core.selected_album_statistics = selected_album_statistics
    core._splined_media_index_installed = True
