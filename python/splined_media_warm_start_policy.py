"""Read-only warm-start hydration for the persistent SPLINED media index.

An active ``picker_inventory`` already proves that the Artist/Album snapshot is
complete. Normal startup therefore must not rewrite thousands of Album and
Artist rows before Select Media can paint. This policy projects current
history/bypass/timeout state in memory, hydrates the resident picker directly
from SQLite, and leaves persistence to explicit SPLINED mutations and explicit
library refreshes.

The SQL read deliberately avoids sorting wide Album rows inside SQLite. The
Album table contains several JSON/statistics columns; an SQL ``ORDER BY`` over
the full joined row can spill a large temporary B-tree on slower mounted cache
storage. Rows are streamed without SQL sorting, progress is published in
batches, and the comparatively small Python row lists are sorted in memory.
"""

from __future__ import annotations

from datetime import datetime
import json
from pathlib import Path
import threading
import time
from typing import Any

import splined_media_database as database
import splined_media_index as media_index
import splined_media_runtime as runtime
from splined_media_tags import json_list


_INSTALLED = False
_PROGRESS_BATCH = 512


def _casefold(value: Any) -> str:
    return str(value or "").casefold()


def _expected_inventory_counts(connection: Any) -> tuple[int, int]:
    row = connection.execute(
        "SELECT folders_json FROM picker_inventory WHERE inventory_key=?",
        (database.INVENTORY_KEY,),
    ).fetchone()
    if row is None:
        return 0, 0
    try:
        folders = json.loads(str(row["folders_json"]))
    except (TypeError, ValueError):
        return 0, 0
    if not isinstance(folders, dict):
        return 0, 0
    return (
        int(folders.get("artists", 0) or 0),
        int(folders.get("albums", 0) or 0),
    )


def _database_family_sizes(path: Path) -> str:
    values: list[str] = []
    for suffix in ("", "-wal", "-shm"):
        item = Path(str(path) + suffix)
        try:
            size = item.stat().st_size
        except OSError:
            continue
        values.append(f"{item.name}={size}")
    return " ".join(values) or "unavailable"


def _project_statuses_in_memory(
    context: database.IndexContext,
    album_rows: list[Any],
) -> tuple[dict[str, str], dict[str, str]]:
    """Return Album path and Artist-key status maps without writing SQLite."""
    core = context.core
    history_albums = context.completion_history.get("albums", {})
    if not isinstance(history_albums, dict):
        history_albums = {}

    policy_fingerprint = core.scan_policy_fingerprint(
        context.cfg,
        context.sources,
    )
    now_epoch = time.time()
    by_artist: dict[str, list[str]] = {}
    by_path: dict[str, str] = {}
    total = len(album_rows)

    core.emit_ui(
        "cache_progress",
        phase="status",
        status="Projecting indexed Folder Status in memory",
        processed=0,
        total=total,
        percent=0.0 if total else 100.0,
        albums=total,
        staged=0,
        recovered=0,
        current_artist="",
    )

    for number, row in enumerate(album_rows, 1):
        path = str(row["path"])
        history_entry = history_albums.get(path)
        outcome = (
            str(history_entry.get("outcome", ""))
            if isinstance(history_entry, dict)
            else ""
        )
        current = str(row["status"])
        timeout_until = str(row["timeout_until"] or "")

        if (
            path in context.bypassed_paths
            or "bypass" in outcome.casefold()
            or bool(row["bypassed"])
            or current == "bypassed"
        ):
            status = "bypassed"
        elif isinstance(history_entry, dict):
            album = core.AlbumDir(
                Path(path),
                [],
                [Path(value) for value in json_list(row["local_art_json"])],
                str(row["inventory_fingerprint"] or "") or None,
            )
            postponed, _age_hours = core.scan_completion_status(
                context.completion_history,
                album,
                context.cfg,
                context.sources,
                context.timeout_hours,
                now=now_epoch,
                policy_fingerprint=policy_fingerprint,
            )
            status = "timeout" if postponed else "processed"
        elif current == "timeout" and timeout_until:
            try:
                active = datetime.fromisoformat(timeout_until).timestamp() > now_epoch
            except ValueError:
                active = False
            status = "timeout" if active else "processed"
        elif (
            current == "processed"
            or bool(row["cover_found"])
            or json_list(row["local_art_json"])
        ):
            status = "processed"
        else:
            status = "unprocessed"

        by_path[path] = status
        by_artist.setdefault(str(row["artist_key"]), []).append(status)

        if number % _PROGRESS_BATCH == 0 or number == total:
            core.emit_ui(
                "cache_progress",
                phase="status",
                status="Projecting indexed Folder Status in memory",
                processed=number,
                total=total,
                percent=(number / total * 100.0 if total else 100.0),
                albums=total,
                staged=0,
                recovered=0,
                current_artist="",
            )

    artist_status = {
        artist_key: database.aggregate_artist(statuses)
        for artist_key, statuses in by_artist.items()
    }
    return by_path, artist_status


def populate_session_readonly(
    context: database.IndexContext,
    connection: Any,
) -> None:
    """Hydrate the complete picker from the active snapshot with no SQL writes."""
    core = context.core
    started = time.perf_counter()
    expected_artists, expected_albums = _expected_inventory_counts(connection)

    core.debug_log(
        "splined.db.warm_load.begin "
        f"expected_artists={expected_artists} expected_albums={expected_albums} "
        f"files={_database_family_sizes(context.db_path)}"
    )
    core.emit_ui(
        "cache_progress",
        phase="load",
        status="Reading indexed Artist rows",
        processed=0,
        total=expected_albums,
        percent=0.0,
        albums=expected_albums,
        staged=0,
        recovered=0,
        current_artist="",
    )

    artist_started = time.perf_counter()
    artist_rows = list(connection.execute("SELECT * FROM artists"))
    artist_query_seconds = time.perf_counter() - artist_started
    core.debug_log(
        "splined.db.warm_load.artist_rows "
        f"artists={len(artist_rows)} elapsed_seconds={artist_query_seconds:.6f}"
    )

    core.emit_ui(
        "cache_progress",
        phase="load",
        status=(
            f"Reading indexed Album rows · {len(artist_rows):,} Artists ready"
        ),
        processed=0,
        total=expected_albums,
        percent=0.0,
        albums=expected_albums,
        staged=0,
        recovered=0,
        current_artist="",
    )

    album_started = time.perf_counter()
    album_rows: list[Any] = []
    cursor = connection.execute(
        "SELECT albums.*, artists.primary_path AS artist_path, "
        "artists.artist_name AS artist_name, "
        "artists.artist_sort AS artist_sort "
        "FROM albums JOIN artists "
        "ON artists.artist_key=albums.artist_key"
    )
    while True:
        batch = cursor.fetchmany(_PROGRESS_BATCH)
        if not batch:
            break
        album_rows.extend(batch)
        processed = len(album_rows)
        denominator = expected_albums or processed
        core.emit_ui(
            "cache_progress",
            phase="load",
            status=(
                f"Reading indexed Album rows · {processed:,} loaded"
            ),
            processed=processed,
            total=denominator,
            percent=(processed / denominator * 100.0 if denominator else 0.0),
            albums=expected_albums or processed,
            staged=0,
            recovered=0,
            current_artist="",
        )
    album_query_seconds = time.perf_counter() - album_started
    core.debug_log(
        "splined.db.warm_load.album_rows "
        f"albums={len(album_rows)} elapsed_seconds={album_query_seconds:.6f}"
    )

    sort_started = time.perf_counter()
    artist_rows.sort(
        key=lambda row: (
            _casefold(row["artist_sort"] or row["artist_name"]),
            _casefold(row["artist_name"]),
            _casefold(row["primary_path"]),
        )
    )
    album_rows.sort(
        key=lambda row: (
            _casefold(row["artist_sort"] or row["artist_name"]),
            _casefold(row["artist_name"]),
            _casefold(row["album_sort"] or row["album_name"]),
            _casefold(row["album_name"]),
            _casefold(row["path"]),
        )
    )
    sort_seconds = time.perf_counter() - sort_started
    total = len(album_rows)

    core.debug_log(
        "splined.db.warm_load.rows "
        f"artists={len(artist_rows)} albums={total} "
        f"artist_query_seconds={artist_query_seconds:.6f} "
        f"album_query_seconds={album_query_seconds:.6f} "
        f"sort_seconds={sort_seconds:.6f}"
    )
    core.emit_ui(
        "cache_progress",
        phase="load",
        status=(
            f"SQLite rows ready · {len(artist_rows):,} Artists / "
            f"{total:,} Albums"
        ),
        processed=total,
        total=total,
        percent=100.0 if total else 0.0,
        albums=total,
        staged=0,
        recovered=0,
        current_artist="",
    )

    status_by_path, status_by_artist = _project_statuses_in_memory(
        context,
        album_rows,
    )

    with runtime._LOCK:
        runtime._PATH_TO_ALBUM_KEY = {
            str(row["path"]): str(row["album_key"])
            for row in album_rows
        }
        runtime._ARTIST_BY_ALBUM_KEY = {
            str(row["album_key"]): str(row["artist_key"])
            for row in album_rows
        }
        # This map represents the current projected UI authority. It avoids a
        # whole-library write when the first library payload is emitted.
        runtime._STATUS_BY_PATH = dict(status_by_path)
        runtime._STATS_BY_PATH = {
            str(row["path"]): runtime.stats_from_row(row)
            for row in album_rows
        }

    session = context.session
    existing_paths = {str(row["path"]) for row in album_rows}
    selected = set(session.selected_paths)
    selected.intersection_update(existing_paths)
    selected_stats = {
        path: json.loads(json.dumps(runtime._STATS_BY_PATH[path]))
        for path in selected
        if path in runtime._STATS_BY_PATH
    }
    artist_path_by_key = {
        str(row["artist_key"]): str(row["primary_path"])
        for row in artist_rows
    }

    with session.lock:
        session.artists = [
            core.PickerArtist(
                str(row["primary_path"]),
                str(row["artist_name"]),
                True,
                time.time(),
            )
            for row in artist_rows
        ]
        session.album_records = [
            core.PickerAlbum(
                str(row["path"]),
                str(row["artist_path"]),
                str(row["album_name"]),
                tuple(json_list(row["local_art_json"])),
                str(row["inventory_fingerprint"] or "") or None,
            )
            for row in album_rows
        ]
        session.loaded_artists = {
            str(row["primary_path"]) for row in artist_rows
        }
        session.probed_artist_statuses = {
            artist_path_by_key[artist_key]: status
            for artist_key, status in status_by_artist.items()
            if artist_key in artist_path_by_key
            and status in database.VALID_ARTIST_STATUSES
        }
        session.selected_paths.clear()
        session.selected_paths.update(selected)
        session.selected_statistics.clear()
        session.selected_statistics.update(selected_stats)
        session.selected_statistics_pending.clear()
        session.status_probe_started = True
        session.status_probe_complete = True
        session.status_probe_thread = threading.Thread(
            name="splined-db-status-complete",
            daemon=True,
        )
        session.status_probe_cancel.clear()
        session.ready = True
        session.library_root = str(context.library_root)
        session.picker_path = str(context.db_path)
        session.initialized_paths.intersection_update(existing_paths)
        session.initialized_paths.update(existing_paths)

    elapsed = time.perf_counter() - started
    core.debug_log(
        "splined.db.warm_load.done "
        f"artists={len(artist_rows)} albums={total} "
        f"elapsed_seconds={elapsed:.6f}"
    )
    core.emit_ui(
        "cache_progress",
        phase="ready",
        status=(
            f"Indexed Select Media model ready · {len(artist_rows):,} Artists · "
            f"{total:,} Albums"
        ),
        processed=total,
        total=total,
        percent=100.0 if total else 0.0,
        albums=total,
        staged=0,
        recovered=0,
        current_artist="",
    )


def install() -> None:
    """Replace the startup hydrator before finalization wraps it."""
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True
    runtime.populate_session = populate_session_readonly
    media_index.populate_session = populate_session_readonly
