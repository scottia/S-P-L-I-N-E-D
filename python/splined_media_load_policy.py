"""Fast warm-start projection for the persistent SPLINED media index.

The active SQLite snapshot already contains Album material state and prior
Folder Status. Warm startup must not rewrite every Album and Artist merely to
prove that most values are unchanged. This policy computes the same status
projection as the original implementation, but persists only actual deltas.

The TUI receives explicit sub-phase progress while the projection is running so
"Loading indexed Folder Status" cannot look frozen on slower storage.
"""

from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path
import time
from typing import Any

import splined_media_database as database
import splined_media_runtime as runtime
from splined_media_tags import json_list


_INSTALLED = False
_PROGRESS_BATCH = 512


def _artist_aggregate(statuses: list[str]) -> tuple[str, dict[str, int]]:
    counts = {name: statuses.count(name) for name in database.VALID_ALBUM_STATUSES}
    return database.aggregate_artist(statuses), counts


def project_statuses_delta(
    context: database.IndexContext,
    connection: Any,
) -> dict[str, int | float]:
    """Project Folder Status and write only rows whose values changed."""
    core = context.core
    started = time.perf_counter()
    history_albums = context.completion_history.get("albums", {})
    if not isinstance(history_albums, dict):
        history_albums = {}

    policy_fingerprint = core.scan_policy_fingerprint(context.cfg, context.sources)
    now_epoch = time.time()
    now_text = database.utc_now()

    album_rows = list(
        connection.execute(
            "SELECT album_key, artist_key, path, local_art_json, "
            "inventory_fingerprint, status, bypassed, cover_found, "
            "timeout_until FROM albums"
        )
    )
    artist_rows = {
        str(row["artist_key"]): row
        for row in connection.execute(
            "SELECT artist_key, status, album_count, unprocessed_count, "
            "processed_count, bypassed_count, timeout_count FROM artists"
        )
    }

    total = len(album_rows)
    core.emit_ui(
        "cache_progress",
        phase="status",
        status="Projecting indexed Folder Status",
        processed=0,
        total=total,
        percent=0.0 if total else 100.0,
        albums=total,
        staged=0,
        recovered=0,
        current_artist="",
    )

    album_updates: list[tuple[str, int, str, str, str]] = []
    by_artist: dict[str, list[str]] = {}

    for number, row in enumerate(album_rows, 1):
        path = str(row["path"])
        history_entry = history_albums.get(path)
        outcome = (
            str(history_entry.get("outcome", ""))
            if isinstance(history_entry, dict)
            else ""
        )
        current_status = str(row["status"])
        current_bypassed = int(row["bypassed"] or 0)
        current_timeout = str(row["timeout_until"] or "")
        timeout_until = current_timeout

        if (
            path in context.bypassed_paths
            or "bypass" in outcome.casefold()
            or bool(current_bypassed)
            or current_status == "bypassed"
        ):
            status = "bypassed"
            timeout_until = ""
        elif isinstance(history_entry, dict):
            album = core.AlbumDir(
                Path(path),
                [],
                [Path(value) for value in json_list(row["local_art_json"])],
                str(row["inventory_fingerprint"] or "") or None,
            )
            postponed, age_hours = core.scan_completion_status(
                context.completion_history,
                album,
                context.cfg,
                context.sources,
                context.timeout_hours,
                now=now_epoch,
                policy_fingerprint=policy_fingerprint,
            )
            if postponed:
                status = "timeout"
                timeout_until = datetime.fromtimestamp(
                    now_epoch
                    + max(0.0, context.timeout_hours - age_hours) * 3600,
                    timezone.utc,
                ).isoformat(timespec="seconds")
            else:
                status = "processed"
                timeout_until = ""
        elif current_status == "timeout" and current_timeout:
            try:
                active = datetime.fromisoformat(current_timeout).timestamp() > now_epoch
            except ValueError:
                active = False
            status = "timeout" if active else "processed"
            if not active:
                timeout_until = ""
        elif (
            current_status == "processed"
            or bool(row["cover_found"])
            or json_list(row["local_art_json"])
        ):
            status = "processed"
        else:
            status = "unprocessed"

        desired_bypassed = int(status == "bypassed")
        by_artist.setdefault(str(row["artist_key"]), []).append(status)

        if (
            status != current_status
            or desired_bypassed != current_bypassed
            or timeout_until != current_timeout
        ):
            album_updates.append(
                (
                    status,
                    desired_bypassed,
                    timeout_until,
                    now_text,
                    str(row["album_key"]),
                )
            )

        if number % _PROGRESS_BATCH == 0 or number == total:
            core.emit_ui(
                "cache_progress",
                phase="status",
                status=(
                    f"Projecting indexed Folder Status · "
                    f"{len(album_updates):,} Album change(s)"
                ),
                processed=number,
                total=total,
                percent=(number / total * 100.0 if total else 100.0),
                albums=total,
                staged=0,
                recovered=0,
                current_artist="",
            )

    artist_updates: list[tuple[str, int, int, int, int, int, str, str]] = []
    for artist_key, statuses in by_artist.items():
        aggregate, counts = _artist_aggregate(statuses)
        current = artist_rows.get(artist_key)
        desired = (
            aggregate,
            len(statuses),
            counts["unprocessed"],
            counts["processed"],
            counts["bypassed"],
            counts["timeout"],
        )
        current_values = (
            str(current["status"]) if current is not None else "",
            int(current["album_count"] or 0) if current is not None else -1,
            int(current["unprocessed_count"] or 0) if current is not None else -1,
            int(current["processed_count"] or 0) if current is not None else -1,
            int(current["bypassed_count"] or 0) if current is not None else -1,
            int(current["timeout_count"] or 0) if current is not None else -1,
        )
        if desired != current_values:
            artist_updates.append((*desired, now_text, artist_key))

    if album_updates or artist_updates:
        with connection:
            if album_updates:
                connection.executemany(
                    "UPDATE albums SET status=?, bypassed=?, timeout_until=?, "
                    "updated_at=? WHERE album_key=?",
                    album_updates,
                )
            if artist_updates:
                connection.executemany(
                    "UPDATE artists SET status=?, album_count=?, "
                    "unprocessed_count=?, processed_count=?, bypassed_count=?, "
                    "timeout_count=?, updated_at=? WHERE artist_key=?",
                    artist_updates,
                )

    elapsed = time.perf_counter() - started
    core.debug_log(
        "splined.db.status_projection_done "
        f"albums={total} album_updates={len(album_updates)} "
        f"artists={len(by_artist)} artist_updates={len(artist_updates)} "
        f"elapsed_seconds={elapsed:.6f}"
    )
    core.emit_ui(
        "cache_progress",
        phase="load",
        status=(
            f"Folder Status ready · {len(album_updates):,} Album change(s) · "
            f"loading {len(by_artist):,} Artists / {total:,} Albums"
        ),
        processed=total,
        total=total,
        percent=100.0 if total else 0.0,
        albums=total,
        staged=0,
        recovered=0,
        current_artist="",
    )
    return {
        "albums": total,
        "album_updates": len(album_updates),
        "artists": len(by_artist),
        "artist_updates": len(artist_updates),
        "elapsed_seconds": elapsed,
    }


def install() -> None:
    """Replace the write-every-row projection used by warm startup."""
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True
    database.project_statuses = project_statuses_delta
    runtime.project_statuses = project_statuses_delta
