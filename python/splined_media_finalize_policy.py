"""Validated SQLite promotion and Select Media handoff.

The tag/checkpoint phase is not the end of a first database build.  A completed
set of staged Album rows still has to be promoted into the active Artist/Album
snapshot, verified, projected into Folder Status, and loaded into the retained
Select Media session.  This policy makes those phases explicit and refuses to
clear resumable checkpoints unless the promoted snapshot is internally valid.
"""

from __future__ import annotations

import json
import sqlite3
import time
from typing import Any

import splined_media_database as database
import splined_media_index as media_index


_INSTALLED = False
_STAGE_TYPE = "media-index-stage"


def _stage_count(connection: sqlite3.Connection) -> int:
    row = connection.execute(
        "SELECT COUNT(*) FROM cache_entries WHERE cache_type=?",
        (_STAGE_TYPE,),
    ).fetchone()
    return int(row[0] or 0) if row is not None else 0


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


def install(core: Any) -> None:
    """Install finalization validation after the fast index builder is selected."""
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
        staged = _stage_count(connection)
        total = len(albums)
        started = time.perf_counter()
        core.emit_ui(
            "cache_progress",
            phase="commit",
            reason=reason,
            status=(
                "Committing completed Artist / Album index to SQLite · "
                "do not close SPLINED"
            ),
            processed=total,
            total=total,
            percent=100.0,
            albums=total,
            staged=staged,
            recovered=0,
            current_artist="",
        )
        try:
            original_insert(
                connection,
                artists,
                albums,
                reviews,
                expected_signature,
                version,
                reason,
            )
            counts = validate_snapshot(
                connection,
                expected_artists=len(artists),
                expected_albums=total,
                expected_signature=expected_signature,
            )
        except Exception as exc:
            # The caller clears stage rows only after this function returns.
            # Raising here deliberately preserves every resumable checkpoint.
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
            original_build(context, connection, reason)
        except Exception as exc:
            # If failure happened before promotion, the stage count identifies
            # how much work is safe to resume.  If promotion itself failed, the
            # validated insert wrapper has already emitted the precise reason.
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

        actual_artists = len(context.session.artists)
        actual_albums = len(context.session.album_records)
        if expected_artists and actual_artists != expected_artists:
            raise RuntimeError(
                f"Select Media Artist load mismatch: {actual_artists} != "
                f"{expected_artists}"
            )
        if expected_albums and actual_albums != expected_albums:
            raise RuntimeError(
                f"Select Media Album load mismatch: {actual_albums} != "
                f"{expected_albums}"
            )
        core.debug_log(
            "splined.db.session_ready "
            f"artists={actual_artists} albums={actual_albums}"
        )
        core.emit_ui(
            "cache_progress",
            phase="ready",
            status=(
                f"Select Media ready · {actual_artists:,} Artists · "
                f"{actual_albums:,} Albums"
            ),
            processed=actual_albums,
            total=actual_alums if False else actual_albums,
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
