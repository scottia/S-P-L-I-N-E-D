"""Fast read-only hydration for the persistent SPLINED media index.

The persistent database can live on a NAS/bind mount whose small random reads
are dramatically slower than a sequential copy.  Warm start therefore stages a
completed WAL-free ``splined.db`` into container-local temporary storage, reads
only the columns required to paint Select Media, and materializes full Album
statistics lazily by logical ``album_key`` when an Album is selected.

No SQL row is modified by this policy.  Persistent writes remain limited to
explicit refreshes and real SPLINED mutations.
"""

from __future__ import annotations

from contextlib import contextmanager
from datetime import datetime
import json
import os
from pathlib import Path
import shutil
import sqlite3
import tempfile
import threading
import time
from typing import Any, Iterator
from urllib.parse import quote

import splined_media_database as database
import splined_media_index as media_index
import splined_media_runtime as runtime
from splined_media_tags import json_list


_INSTALLED = False
_PROGRESS_BATCH = 512
_COPY_CHUNK_BYTES = 8 * 1024 * 1024


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


def _wal_size(path: Path) -> int:
    try:
        return int(Path(str(path) + "-wal").stat().st_size)
    except OSError:
        return 0


def _snapshot_mode() -> str:
    value = os.environ.get(
        "SPLINED_SQLITE_LOCAL_SNAPSHOT",
        "auto",
    ).strip().casefold()
    return value if value in {"auto", "always", "never"} else "auto"


def _source_is_already_local(path: Path) -> bool:
    """Avoid copying when the database already shares /tmp's local device."""
    try:
        return path.stat().st_dev == Path(tempfile.gettempdir()).stat().st_dev
    except OSError:
        return False


def _copy_database(source: Path, target: Path) -> None:
    with source.open("rb", buffering=0) as input_handle:
        with target.open("wb", buffering=0) as output_handle:
            shutil.copyfileobj(
                input_handle,
                output_handle,
                length=_COPY_CHUNK_BYTES,
            )
            output_handle.flush()
            os.fsync(output_handle.fileno())


@contextmanager
def _warm_read_connection(
    context: database.IndexContext,
    connection: sqlite3.Connection,
) -> Iterator[sqlite3.Connection]:
    """Yield a local immutable read snapshot when it is safe and worthwhile."""
    core = context.core
    source = Path(context.db_path)
    mode = _snapshot_mode()

    if (
        mode == "never"
        or not source.is_file()
        or _wal_size(source) > 0
        or (mode == "auto" and _source_is_already_local(source))
    ):
        core.debug_log(
            "splined.db.warm_load.snapshot "
            f"mode=direct configured={mode!r} wal_bytes={_wal_size(source)}"
        )
        yield connection
        return

    descriptor, temp_name = tempfile.mkstemp(
        prefix="splined-warm-",
        suffix=".db",
        dir=tempfile.gettempdir(),
    )
    os.close(descriptor)
    temp_path = Path(temp_name)
    local: sqlite3.Connection | None = None
    try:
        size = int(source.stat().st_size)
        core.emit_ui(
            "cache_progress",
            phase="load",
            status=(
                "Staging local SQLite read snapshot · "
                f"{size / 1_048_576:.1f} MiB"
            ),
            processed=0,
            total=size,
            percent=0.0,
            albums=0,
            staged=0,
            recovered=0,
            current_artist="",
        )
        started = time.perf_counter()
        _copy_database(source, temp_path)
        copy_seconds = time.perf_counter() - started
        copied_size = int(temp_path.stat().st_size)
        if copied_size != size:
            raise OSError(
                f"SQLite snapshot size mismatch: {copied_size} != {size}"
            )

        uri = f"file:{quote(str(temp_path))}?mode=ro&immutable=1"
        local = sqlite3.connect(uri, uri=True, timeout=30.0)
        local.row_factory = sqlite3.Row
        local.execute("PRAGMA query_only = ON")
        core.debug_log(
            "splined.db.warm_load.snapshot "
            f"mode=local bytes={copied_size} copy_seconds={copy_seconds:.6f}"
        )
        core.emit_ui(
            "cache_progress",
            phase="load",
            status=(
                "Local SQLite snapshot ready · "
                f"{copy_seconds:.2f}s"
            ),
            processed=size,
            total=size,
            percent=100.0,
            albums=0,
            staged=0,
            recovered=0,
            current_artist="",
        )
    except (OSError, sqlite3.Error) as exc:
        core.debug_log(
            "splined.db.warm_load.snapshot_error "
            f"error={type(exc).__name__}: {exc}"
        )
        if local is not None:
            try:
                local.close()
            except sqlite3.Error:
                pass
        try:
            temp_path.unlink()
        except OSError:
            pass
        yield connection
        return

    try:
        assert local is not None
        yield local
    finally:
        try:
            local.close()
        except sqlite3.Error:
            pass
        try:
            temp_path.unlink()
        except OSError:
            pass


def _project_statuses_in_memory(
    context: database.IndexContext,
    album_rows: list[dict[str, Any]],
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


def _selected_stats_from_connection(
    connection: sqlite3.Connection,
    album_keys: dict[str, str],
    paths: set[str],
) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for path in sorted(paths, key=str.casefold):
        album_key = album_keys.get(path)
        if not album_key:
            continue
        row = connection.execute(
            "SELECT albums.*, artists.artist_name "
            "FROM albums JOIN artists "
            "ON artists.artist_key=albums.artist_key "
            "WHERE albums.album_key=?",
            (album_key,),
        ).fetchone()
        if row is not None:
            result[path] = runtime.stats_from_row(row)
    return result


def cached_stats_readthrough(path: Path) -> dict[str, Any] | None:
    """Read one Album's materialized statistics by indexed logical key."""
    path_text = str(path)
    with runtime._LOCK:
        cached = runtime._STATS_BY_PATH.get(path_text)
        album_key = runtime._PATH_TO_ALBUM_KEY.get(path_text)
        database_path = runtime._ACTIVE_DB_PATH
    if cached is not None:
        return json.loads(json.dumps(cached))
    if not album_key or database_path is None:
        return None

    uri = f"file:{quote(str(database_path))}?mode=ro"
    connection: sqlite3.Connection | None = None
    try:
        connection = sqlite3.connect(uri, uri=True, timeout=30.0)
        connection.row_factory = sqlite3.Row
        connection.execute("PRAGMA query_only = ON")
        row = connection.execute(
            "SELECT albums.*, artists.artist_name "
            "FROM albums JOIN artists "
            "ON artists.artist_key=albums.artist_key "
            "WHERE albums.album_key=?",
            (album_key,),
        ).fetchone()
        if row is None:
            return None
        stats = runtime.stats_from_row(row)
    except sqlite3.Error:
        return None
    finally:
        if connection is not None:
            connection.close()

    with runtime._LOCK:
        runtime._STATS_BY_PATH[path_text] = stats
    return json.loads(json.dumps(stats))


def populate_session_readonly(
    context: database.IndexContext,
    connection: sqlite3.Connection,
) -> None:
    """Hydrate the complete picker from a compact local read snapshot."""
    core = context.core
    started = time.perf_counter()

    core.debug_log(
        "splined.db.warm_load.begin "
        f"files={_database_family_sizes(context.db_path)}"
    )

    with _warm_read_connection(context, connection) as read_connection:
        expected_authority_artists, expected_albums = _expected_inventory_counts(
            read_connection
        )
        core.emit_ui(
            "cache_progress",
            phase="load",
            status="Reading compact indexed Artist rows",
            processed=0,
            total=expected_albums,
            percent=0.0,
            albums=expected_albums,
            staged=0,
            recovered=0,
            current_artist="",
        )

        artist_started = time.perf_counter()
        artist_rows = [
            dict(row)
            for row in read_connection.execute(
                "SELECT artist_key, artist_name, artist_sort, primary_path, "
                "status FROM artists"
            )
        ]
        picker_artists = runtime.project_picker_artists(core, artist_rows)
        duplicate_artist_paths = len(artist_rows) - len(picker_artists)
        artist_query_seconds = time.perf_counter() - artist_started
        core.debug_log(
            "splined.db.warm_load.artist_rows "
            f"authority_artists={len(artist_rows)} "
            f"picker_artists={len(picker_artists)} "
            f"duplicate_paths={duplicate_artist_paths} "
            f"expected_authority_artists={expected_authority_artists} "
            f"elapsed_seconds={artist_query_seconds:.6f}"
        )

        artist_by_key = {
            str(row["artist_key"]): row for row in artist_rows
        }
        core.emit_ui(
            "cache_progress",
            phase="load",
            status=(
                "Reading compact indexed Album rows · "
                f"{len(picker_artists):,} Artist folders ready"
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
        album_rows: list[dict[str, Any]] = []
        cursor = read_connection.execute(
            "SELECT album_key, artist_key, album_name, album_sort, path, "
            "inventory_fingerprint, status, bypassed, cover_found, "
            "timeout_until, local_art_json FROM albums"
        )
        while True:
            batch = cursor.fetchmany(_PROGRESS_BATCH)
            if not batch:
                break
            for raw in batch:
                row = dict(raw)
                artist = artist_by_key.get(str(row["artist_key"]))
                if artist is None:
                    continue
                row["artist_path"] = str(artist["primary_path"])
                row["artist_name"] = str(artist["artist_name"])
                row["artist_sort"] = str(
                    artist["artist_sort"] or artist["artist_name"]
                )
                album_rows.append(row)
            processed = len(album_rows)
            denominator = expected_albums or processed
            core.emit_ui(
                "cache_progress",
                phase="load",
                status=(
                    f"Reading compact indexed Album rows · {processed:,} loaded"
                ),
                processed=processed,
                total=denominator,
                percent=(
                    processed / denominator * 100.0
                    if denominator
                    else 0.0
                ),
                albums=expected_albums or processed,
                staged=0,
                recovered=0,
                current_artist="",
            )
        album_query_seconds = time.perf_counter() - album_started
        core.debug_log(
            "splined.db.warm_load.album_rows "
            f"albums={len(album_rows)} "
            f"elapsed_seconds={album_query_seconds:.6f}"
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
            f"authority_artists={len(artist_rows)} "
            f"picker_artists={len(picker_artists)} albums={total} "
            f"artist_query_seconds={artist_query_seconds:.6f} "
            f"album_query_seconds={album_query_seconds:.6f} "
            f"sort_seconds={sort_seconds:.6f}"
        )
        core.emit_ui(
            "cache_progress",
            phase="load",
            status=(
                f"SQLite rows ready · {len(picker_artists):,} Artist folders / "
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

        path_to_album_key = {
            str(row["path"]): str(row["album_key"])
            for row in album_rows
        }
        artist_by_album_key = {
            str(row["album_key"]): str(row["artist_key"])
            for row in album_rows
        }
        existing_paths = set(path_to_album_key)
        selected = set(context.session.selected_paths)
        selected.intersection_update(existing_paths)
        selected_stats = _selected_stats_from_connection(
            read_connection,
            path_to_album_key,
            selected,
        )

    with runtime._LOCK:
        runtime._PATH_TO_ALBUM_KEY = path_to_album_key
        runtime._ARTIST_BY_ALBUM_KEY = artist_by_album_key
        runtime._STATUS_BY_PATH = dict(status_by_path)
        # Full statistics contain several JSON/artwork columns and are not
        # required for first paint. Cache only already-selected Albums; every
        # later selection is read by indexed album_key through
        # cached_stats_readthrough().
        runtime._STATS_BY_PATH = dict(selected_stats)

    session = context.session
    artist_path_by_key = {
        str(row["artist_key"]): str(row["primary_path"])
        for row in artist_rows
    }

    with session.lock:
        session.artists = list(picker_artists)
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
        session.loaded_artists = {artist.path for artist in picker_artists}
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
        f"authority_artists={len(artist_rows)} "
        f"picker_artists={len(picker_artists)} "
        f"duplicate_paths={duplicate_artist_paths} "
        f"albums={len(album_rows)} "
        f"elapsed_seconds={elapsed:.6f}"
    )
    core.emit_ui(
        "cache_progress",
        phase="ready",
        status=(
            f"Indexed Select Media model ready · "
            f"{len(picker_artists):,} Artist folders · "
            f"{len(album_rows):,} Albums"
        ),
        processed=len(album_rows),
        total=len(album_rows),
        percent=100.0 if album_rows else 0.0,
        albums=len(album_rows),
        staged=0,
        recovered=0,
        current_artist="",
    )


def install() -> None:
    """Replace startup hydration and install indexed lazy statistics."""
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True
    runtime.populate_session = populate_session_readonly
    media_index.populate_session = populate_session_readonly
    runtime.cached_stats = cached_stats_readthrough
    media_index.cached_stats = cached_stats_readthrough
