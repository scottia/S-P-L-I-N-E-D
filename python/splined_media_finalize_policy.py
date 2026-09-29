"""Validated SQLite promotion and Select Media handoff.

The representative-tag/checkpoint phase is not the end of a first database
build. Completed stage rows must still be promoted into the active Artist/Album
snapshot, verified, projected into Folder Status, and loaded into Select Media.

For an initial build this module uses a marker-last, batched promotion:

* active rows are written in small committed batches;
* ``picker_inventory`` is written only after every Artist/Album row exists;
* staged Album payloads remain untouched until the active snapshot validates;
* an interrupted promotion can be retried from the completed checkpoints;
* a completed checkpoint set can be recovered after one lightweight topology
  pass without reopening Album tags.

A normal refresh with an already usable snapshot retains the existing atomic
replacement path so the previous index remains authoritative until the refresh
commits.
"""

from __future__ import annotations

import json
import sqlite3
import time
from pathlib import Path
from typing import Any, Iterable

import splined_media_build_policy as build_policy
import splined_media_database as database
import splined_media_fast_index_policy as fast_policy
import splined_media_index as media_index


_INSTALLED = False
_STAGE_TYPE = "media-index-stage"
_PROMOTION_BATCH = 256

_ARTIST_INSERT_SQL = (
    "INSERT INTO artists"
    "(artist_key, artist_name, artist_sort, musicbrainz_artistid, "
    "primary_path, status, album_count, unprocessed_count, "
    "processed_count, bypassed_count, timeout_count, created_at, "
    "updated_at, last_seen_at, splined_version) "
    "VALUES(:artist_key, :artist_name, :artist_sort, "
    ":musicbrainz_artistid, :primary_path, :status, 0, 0, 0, 0, 0, "
    ":created_at, :updated_at, :last_seen_at, :splined_version)"
)

_ALBUM_INSERT_SQL = (
    "INSERT INTO albums"
    "(album_key, artist_key, album_name, album_sort, "
    "musicbrainz_albumid, musicbrainz_releasegroupid, release_year, "
    "compilation, path, representative_file, representative_size, "
    "representative_mtime_ns, tag_signature, track_count, "
    "inventory_fingerprint, status, cover_required, cover_found, "
    "cover_path, cover_name, cover_format, cover_width, cover_height, "
    "artwork_jpeg, artwork_png, artwork_webp, artwork_other, "
    "root_files, cover_files, cover_names_json, local_art_json, "
    "other_filenames_json, webp_found, webp_size_mb, webp_resolution, "
    "webp_conversion, processed_at, bypassed, timeout_until, "
    "selected_source, created_at, updated_at, last_seen_at, "
    "splined_version) "
    "VALUES(:album_key, :artist_key, :album_name, :album_sort, "
    ":musicbrainz_albumid, :musicbrainz_releasegroupid, "
    ":release_year, :compilation, :path, :representative_file, "
    ":representative_size, :representative_mtime_ns, :tag_signature, "
    ":track_count, :inventory_fingerprint, :status, :cover_required, "
    ":cover_found, :cover_path, :cover_name, :cover_format, "
    ":cover_width, :cover_height, :artwork_jpeg, :artwork_png, "
    ":artwork_webp, :artwork_other, :root_files, :cover_files, "
    ":cover_names_json, :local_art_json, :other_filenames_json, "
    ":webp_found, :webp_size_mb, :webp_resolution, :webp_conversion, "
    ":processed_at, :bypassed, :timeout_until, :selected_source, "
    ":created_at, :updated_at, :last_seen_at, :splined_version)"
)


def _chunks(values: list[Any], size: int = _PROMOTION_BATCH) -> Iterable[list[Any]]:
    for start in range(0, len(values), size):
        yield values[start : start + size]


def _stage_count(connection: sqlite3.Connection) -> int:
    row = connection.execute(
        "SELECT COUNT(*) FROM cache_entries WHERE cache_type=?",
        (_STAGE_TYPE,),
    ).fetchone()
    return int(row[0] or 0) if row is not None else 0


def _picker_folder_count(connection: sqlite3.Connection) -> int:
    """Return physical Artist folders, not album-authority identities."""
    row = connection.execute(
        "SELECT COUNT(DISTINCT primary_path) FROM artists"
    ).fetchone()
    return int(row[0] or 0) if row is not None else 0


def _picker_exists(connection: sqlite3.Connection) -> bool:
    return connection.execute(
        "SELECT 1 FROM picker_inventory WHERE inventory_key=?",
        (database.INVENTORY_KEY,),
    ).fetchone() is not None


def _signature_token(value: dict[str, Any]) -> str:
    return build_policy._signature_token(value)


def _stage_payloads(
    connection: sqlite3.Connection,
    signature_token: str,
) -> dict[str, dict[str, Any]]:
    return build_policy._load_stage_payloads(connection, signature_token)


def _prune_stage_rows(
    connection: sqlite3.Connection,
    expected_paths: set[str],
) -> int:
    stale_keys: list[str] = []
    rows = connection.execute(
        "SELECT cache_key, payload_json FROM cache_entries WHERE cache_type=?",
        (_STAGE_TYPE,),
    )
    for row in rows:
        try:
            payload = json.loads(str(row["payload_json"]))
        except (TypeError, ValueError):
            stale_keys.append(str(row["cache_key"]))
            continue
        if not isinstance(payload, dict) or str(payload.get("path", "")) not in expected_paths:
            stale_keys.append(str(row["cache_key"]))
    if stale_keys:
        with connection:
            connection.executemany(
                "DELETE FROM cache_entries WHERE cache_key=?",
                [(value,) for value in stale_keys],
            )
    return _stage_count(connection)


def _mark_stage_complete(
    connection: sqlite3.Connection,
    *,
    expected_signature: dict[str, Any],
    reason: str,
    version: str,
    total: int,
) -> None:
    existing: dict[str, Any] = {}
    row = connection.execute(
        "SELECT details_json FROM db_maintenance_state "
        "WHERE action_name='media-index-build-progress'"
    ).fetchone()
    if row is not None:
        try:
            decoded = json.loads(str(row["details_json"]))
            if isinstance(decoded, dict):
                existing = decoded
        except (TypeError, ValueError):
            existing = {}
    existing.update(
        {
            "signature": _signature_token(expected_signature),
            "reason": reason,
            "phase": "tag-index-complete",
            "processed": int(total),
            "total": int(total),
            "discovered": int(total),
        }
    )
    with connection:
        connection.execute(
            "INSERT INTO db_maintenance_state"
            "(action_name, completed_at, details_json, splined_version) "
            "VALUES('media-index-build-progress', ?, ?, ?) "
            "ON CONFLICT(action_name) DO UPDATE SET "
            "completed_at=excluded.completed_at, "
            "details_json=excluded.details_json, "
            "splined_version=excluded.splined_version",
            (
                database.utc_now(),
                json.dumps(existing, separators=(",", ":")),
                version,
            ),
        )


def validate_snapshot(
    connection: sqlite3.Connection,
    *,
    expected_artists: int,
    expected_albums: int,
    expected_signature: dict[str, Any],
) -> dict[str, int]:
    """Prove the active snapshot is complete before checkpoints are discarded."""
    artist_count = int(
        connection.execute("SELECT COUNT(*) FROM artists").fetchone()[0]
    )
    album_count = int(
        connection.execute("SELECT COUNT(*) FROM albums").fetchone()[0]
    )
    inventory = connection.execute(
        "SELECT signature_json, folders_json FROM picker_inventory "
        "WHERE inventory_key=?",
        (database.INVENTORY_KEY,),
    ).fetchone()
    if inventory is None:
        raise RuntimeError("picker_inventory was not committed")

    try:
        saved_signature = json.loads(str(inventory["signature_json"]))
        folders = json.loads(str(inventory["folders_json"]))
    except (TypeError, ValueError) as exc:
        raise RuntimeError("picker_inventory contains invalid JSON") from exc

    if saved_signature != expected_signature:
        raise RuntimeError("picker_inventory signature does not match this library")
    if artist_count != int(expected_artists):
        raise RuntimeError(
            f"active Artist count mismatch: {artist_count} != {expected_artists}"
        )
    if album_count != int(expected_albums):
        raise RuntimeError(
            f"active Album count mismatch: {album_count} != {expected_albums}"
        )
    if int(folders.get("artists", -1)) != artist_count:
        raise RuntimeError("picker_inventory Artist total does not match artists table")
    if int(folders.get("albums", -1)) != album_count:
        raise RuntimeError("picker_inventory Album total does not match albums table")

    quick = connection.execute("PRAGMA quick_check").fetchone()
    if quick is None or str(quick[0]).casefold() != "ok":
        raise RuntimeError(
            "SQLite quick_check failed: "
            + (str(quick[0]) if quick is not None else "no result")
        )
    return {"artists": artist_count, "albums": album_count}


def _emit_commit_status(
    core: Any,
    *,
    reason: str,
    status: str,
    total: int,
    staged: int,
) -> None:
    core.emit_ui(
        "cache_progress",
        phase="commit",
        reason=reason,
        status=status,
        processed=total,
        total=total,
        percent=100.0,
        albums=total,
        staged=staged,
        recovered=staged,
        rate_per_second=0.0,
        eta_text="finalizing",
        current_artist="",
    )


def _promote_initial_snapshot(
    core: Any,
    connection: sqlite3.Connection,
    artists: dict[str, dict[str, Any]],
    albums: list[dict[str, Any]],
    reviews: list[dict[str, Any]],
    expected_signature: dict[str, Any],
    version: str,
    reason: str,
) -> None:
    """Write an unusable-until-marked initial snapshot in small commits."""
    now = database.utc_now()
    staged = _stage_count(connection)
    total = len(albums)

    _emit_commit_status(
        core,
        reason=reason,
        status="Preparing active SQLite tables from completed checkpoints",
        total=total,
        staged=staged,
    )
    # The final picker_inventory row is the authority marker. Removing it first
    # means a crash can leave partial rows but can never expose a partial index.
    with connection:
        connection.execute(
            "DELETE FROM picker_inventory WHERE inventory_key=?",
            (database.INVENTORY_KEY,),
        )
        connection.execute("DELETE FROM album_refresh_review_queue")
        connection.execute("DELETE FROM albums")
        connection.execute("DELETE FROM artists")

    artist_payloads = [
        dict(row, splined_version=version) for row in artists.values()
    ]
    for batch in _chunks(artist_payloads):
        with connection:
            connection.executemany(_ARTIST_INSERT_SQL, batch)

    album_payloads = [dict(row, splined_version=version) for row in albums]
    inserted = 0
    for batch in _chunks(album_payloads):
        with connection:
            connection.executemany(_ALBUM_INSERT_SQL, batch)
        inserted += len(batch)
        _emit_commit_status(
            core,
            reason=reason,
            status=f"Writing active Album rows · {inserted:,} / {total:,}",
            total=total,
            staged=staged,
        )

    review_payloads = [
        (
            str(review["album_key"]),
            now,
            str(review["source"]),
            json.dumps(review["details"], separators=(",", ":")),
            version,
        )
        for review in reviews
    ]
    for batch in _chunks(review_payloads):
        with connection:
            connection.executemany(
                "INSERT INTO album_refresh_review_queue"
                "(album_key, requested_at, source, details_json, "
                "splined_version) VALUES(?, ?, ?, ?, ?)",
                batch,
            )

    folders = {
        "artists": len(artists),
        "albums": total,
        "review_queue": len(reviews),
    }
    _emit_commit_status(
        core,
        reason=reason,
        status="Publishing validated picker_inventory marker",
        total=total,
        staged=staged,
    )
    with connection:
        connection.execute(
            "INSERT INTO picker_inventory"
            "(inventory_key, signature_json, folders_json, generated_at, "
            "splined_version) VALUES(?, ?, ?, ?, ?) "
            "ON CONFLICT(inventory_key) DO UPDATE SET "
            "signature_json=excluded.signature_json, "
            "folders_json=excluded.folders_json, "
            "generated_at=excluded.generated_at, "
            "splined_version=excluded.splined_version",
            (
                database.INVENTORY_KEY,
                json.dumps(
                    expected_signature,
                    sort_keys=True,
                    separators=(",", ":"),
                ),
                json.dumps(folders, sort_keys=True, separators=(",", ":")),
                now,
                version,
            ),
        )
        connection.execute(
            "INSERT INTO cache_history"
            "(cache_key, cache_type, album_key, action, payload_json, "
            "splined_version, event_at) "
            "VALUES(?, 'picker_inventory', NULL, ?, ?, ?, ?)",
            (
                database.INVENTORY_KEY,
                reason,
                json.dumps(folders, separators=(",", ":")),
                version,
                now,
            ),
        )
        connection.execute(
            "INSERT INTO db_maintenance_state"
            "(action_name, completed_at, details_json, splined_version) "
            "VALUES('media-index-refresh', ?, ?, ?) "
            "ON CONFLICT(action_name) DO UPDATE SET "
            "completed_at=excluded.completed_at, "
            "details_json=excluded.details_json, "
            "splined_version=excluded.splined_version",
            (
                now,
                json.dumps(folders, separators=(",", ":")),
                version,
            ),
        )


def _reconstruct_stage_rows(
    payloads: dict[str, dict[str, Any]],
    ordered_paths: list[str],
) -> tuple[
    dict[str, dict[str, Any]],
    list[dict[str, Any]],
    list[dict[str, Any]],
]:
    artists: dict[str, dict[str, Any]] = {}
    albums: list[dict[str, Any]] = []
    reviews: dict[str, dict[str, Any]] = {}
    for path in ordered_paths:
        payload = payloads[path]
        artist = dict(payload["artist"])
        album = dict(payload["album"])
        key = str(artist["artist_key"])
        current = artists.get(key)
        if current is None:
            artists[key] = artist
        elif (
            not str(current.get("musicbrainz_artistid", ""))
            and str(artist.get("musicbrainz_artistid", ""))
        ):
            current.update(
                artist_name=artist["artist_name"],
                artist_sort=artist["artist_sort"],
                musicbrainz_artistid=artist["musicbrainz_artistid"],
            )
        albums.append(album)
        review = payload.get("review")
        if isinstance(review, dict):
            reviews[str(review["album_key"])] = dict(review)
    return artists, albums, list(reviews.values())


def _recover_complete_stage(
    core: Any,
    context: database.IndexContext,
    connection: sqlite3.Connection,
    reason: str,
) -> bool:
    """Recover a complete checkpoint set after one topology-only verification."""
    if _picker_exists(connection) or _stage_count(connection) <= 0:
        return False

    expected_signature = database.signature(
        context.library_root,
        context.ignored,
        context.cover_name,
    )
    token = _signature_token(expected_signature)
    staged = _stage_payloads(connection, token)
    if not staged:
        return False

    core.emit_ui(
        "cache_build_start",
        cache_kind="media-index",
        reason=reason,
        database=str(context.db_path),
        resumable=True,
    )

    def progress(directories: int, album_count: int) -> None:
        core.emit_ui(
            "cache_progress",
            phase="inventory",
            reason=reason,
            database=str(context.db_path),
            status="Validating completed Album checkpoints against library topology",
            processed=directories,
            total=0,
            percent=None,
            albums=album_count,
            staged=len(staged),
            recovered=0,
            current_artist="",
        )

    fingerprint_paths = core.timeout_fingerprint_paths(
        context.completion_history,
        context.cfg,
        context.sources,
        context.timeout_hours,
    )
    albums, _ignored = fast_policy._discover_index_albums(
        core,
        context.library_root,
        context.ignored,
        context.cover_name,
        fingerprint_paths=fingerprint_paths,
        progress=progress,
        workers=getattr(core, "INVENTORY_WORKERS", 8),
        cancelled=getattr(core, "tui_cancelled", None),
    )
    ordered_paths = [str(album.path) for album in albums]
    if len(ordered_paths) != len(staged):
        return False
    for album in albums:
        payload = staged.get(str(album.path))
        if payload is None or not build_policy._stage_payload_matches(
            payload,
            token,
            album,
        ):
            return False

    artists, album_rows, reviews = _reconstruct_stage_rows(
        staged,
        ordered_paths,
    )
    core.emit_ui(
        "cache_progress",
        phase="commit",
        reason=reason,
        database=str(context.db_path),
        status=(
            f"Checkpoint set complete · promoting {len(album_rows):,} Albums "
            "without rereading tags"
        ),
        processed=len(album_rows),
        total=len(album_rows),
        percent=100.0,
        albums=len(album_rows),
        staged=len(album_rows),
        recovered=len(album_rows),
        current_artist="",
    )
    database._insert_snapshot(
        connection,
        artists,
        album_rows,
        reviews,
        expected_signature,
        str(core.display_version()),
        reason,
    )
    build_policy._clear_stage(connection)
    core.emit_ui(
        "activity",
        category="inventory",
        state="done",
        source="splined-db",
        message=(
            f"SPLINED checkpoint recovery ready · {len(artists):,} Artist(s) · "
            f"{len(album_rows):,} Album(s)"
        ),
    )
    return True


def install(core: Any) -> None:
    """Install finalization after the fast representative-track builder."""
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True

    original_insert = database._insert_snapshot
    original_build = media_index.build_index
    original_populate = media_index.populate_session

    def validated_insert_snapshot(
        connection: sqlite3.Connection,
        artists: dict[str, dict[str, Any]],
        albums: list[dict[str, Any]],
        reviews: list[dict[str, Any]],
        expected_signature: dict[str, Any],
        version: str,
        reason: str,
    ) -> None:
        expected_paths = {str(row["path"]) for row in albums}
        staged = _prune_stage_rows(connection, expected_paths)
        total = len(albums)
        if staged == total and total:
            _mark_stage_complete(
                connection,
                expected_signature=expected_signature,
                reason=reason,
                version=version,
                total=total,
            )

        started = time.perf_counter()
        try:
            if not _picker_exists(connection):
                _promote_initial_snapshot(
                    core,
                    connection,
                    artists,
                    albums,
                    reviews,
                    expected_signature,
                    version,
                    reason,
                )
            else:
                _emit_commit_status(
                    core,
                    reason=reason,
                    status="Atomically replacing the active SQLite snapshot",
                    total=total,
                    staged=staged,
                )
                original_insert(
                    connection,
                    artists,
                    albums,
                    reviews,
                    expected_signature,
                    version,
                    reason,
                )

            _emit_commit_status(
                core,
                reason=reason,
                status="Validating active SQLite snapshot",
                total=total,
                staged=staged,
            )
            counts = validate_snapshot(
                connection,
                expected_artists=len(artists),
                expected_albums=total,
                expected_signature=expected_signature,
            )
        except Exception as exc:
            message = f"SQLite promotion failed: {type(exc).__name__}: {exc}"
            core.debug_log(
                "splined.db.promotion_error "
                f"staged={staged} expected_artists={len(artists)} "
                f"expected_albums={total} error={type(exc).__name__}: {exc}"
            )
            core.emit_ui(
                "cache_progress",
                phase="error",
                reason=reason,
                status=message,
                processed=total,
                total=total,
                percent=100.0,
                albums=total,
                staged=staged,
                recovered=0,
                current_artist="",
            )
            core.emit_ui(
                "activity",
                category="inventory",
                state="error",
                source="splined-db",
                message=message,
            )
            raise

        elapsed = time.perf_counter() - started
        core.debug_log(
            "splined.db.promotion_done "
            f"artists={counts['artists']} albums={counts['albums']} "
            f"elapsed_seconds={elapsed:.6f}"
        )
        core.emit_ui(
            "cache_progress",
            phase="load",
            reason=reason,
            status=(
                f"Database committed · loading {counts['artists']:,} Artists / "
                f"{counts['albums']:,} Albums into Select Media"
            ),
            processed=counts["albums"],
            total=counts["albums"],
            percent=100.0,
            albums=counts["albums"],
            staged=staged,
            recovered=0,
            current_artist="",
        )

    def guarded_build_index(
        context: database.IndexContext,
        connection: sqlite3.Connection,
        reason: str,
    ) -> None:
        try:
            if _recover_complete_stage(core, context, connection, reason):
                return
            original_build(context, connection, reason)
        except Exception as exc:
            staged = _stage_count(connection)
            core.debug_log(
                "splined.db.build_error "
                f"reason={reason!r} staged={staged} "
                f"error={type(exc).__name__}: {exc}"
            )
            raise

    def validated_populate_session(
        context: database.IndexContext,
        connection: sqlite3.Connection,
    ) -> None:
        row = connection.execute(
            "SELECT folders_json FROM picker_inventory WHERE inventory_key=?",
            (database.INVENTORY_KEY,),
        ).fetchone()
        expected_artists = 0
        expected_albums = 0
        if row is not None:
            try:
                folders = json.loads(str(row["folders_json"]))
                expected_artists = int(folders.get("artists", 0) or 0)
                expected_albums = int(folders.get("albums", 0) or 0)
            except (TypeError, ValueError):
                pass
        expected_picker_folders = _picker_folder_count(connection)

        core.emit_ui(
            "cache_progress",
            phase="load",
            status="Loading indexed Folder Status and Select Media rows",
            processed=expected_albums,
            total=expected_albums,
            percent=100.0 if expected_albums else 0.0,
            albums=expected_albums,
            staged=_stage_count(connection),
            recovered=0,
            current_artist="",
        )
        original_populate(context, connection)

        actual_picker_folders = len(context.session.artists)
        actual_albums = len(context.session.album_records)
        if actual_picker_folders != expected_picker_folders:
            raise RuntimeError(
                "Select Media Artist folder load mismatch: "
                f"{actual_picker_folders} != {expected_picker_folders}"
            )
        if expected_albums and actual_albums != expected_albums:
            raise RuntimeError(
                f"Select Media Album load mismatch: {actual_albums} != "
                f"{expected_albums}"
            )

        # A crash after publishing picker_inventory but before stage cleanup is
        # harmless. Once the active snapshot and session both validate, remove
        # any residual stage payloads here as well.
        if _stage_count(connection):
            build_policy._clear_stage(connection)
        core.debug_log(
            "splined.db.session_ready "
            f"authority_artists={expected_artists} "
            f"artist_folders={actual_picker_folders} albums={actual_albums}"
        )
        core.emit_ui(
            "cache_progress",
            phase="ready",
            status=(
                f"Select Media ready · {actual_picker_folders:,} Artist folders · "
                f"{actual_albums:,} Albums"
            ),
            processed=actual_albums,
            total=actual_albums,
            percent=100.0 if actual_albums else 0.0,
            albums=actual_albums,
            staged=0,
            recovered=0,
            current_artist="",
        )

    database._insert_snapshot = validated_insert_snapshot
    media_index.build_index = guarded_build_index
    media_index.populate_session = validated_populate_session
    core._splined_media_finalize_policy_installed = True
