"""Persistent Navtagger-style SQLite store for SPLINED Select Media."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
import json
from pathlib import Path
import sqlite3
import time
from typing import Any

from splined_media_tags import inspect_album, json_list, utc_now


DB_NAME = "splined.db"
SCHEMA_VERSION = 1
INVENTORY_KEY = "splined-media-library"
VALID_ALBUM_STATUSES = {"unprocessed", "processed", "bypassed", "timeout"}
VALID_ARTIST_STATUSES = {
    "unprocessed",
    "partial",
    "complete",
    "contains-bypass",
}


@dataclass
class IndexContext:
    core: Any
    config_file: Path
    cfg: dict[str, Any]
    sources: list[str]
    root: Path
    library_root: Path
    cache: Path
    db_path: Path
    completion_history: dict[str, Any]
    timeout_hours: float
    ignored: list[str]
    cover_name: str
    bypassed_paths: set[str]
    session: Any
    original_inventory: Any


def schema_path() -> Path:
    return Path(__file__).with_name("splined_schema.sql")


def database_path(cache_dir: Path) -> Path:
    return Path(cache_dir) / DB_NAME


def signature(
    library_root: Path,
    ignored: list[str],
    cover_name: str,
) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "library_root": str(library_root),
        "ignored_subs": sorted(str(value).casefold() for value in ignored),
        "cover_name": cover_name.casefold(),
    }


def quarantine_database(path: Path, reason: str) -> None:
    if not path.exists():
        return
    stamp = time.strftime("%Y%m%d-%H%M%S")
    target = path.with_name(f"{path.name}.{reason}-{stamp}")
    counter = 1
    while target.exists():
        target = path.with_name(f"{path.name}.{reason}-{stamp}-{counter}")
        counter += 1
    path.replace(target)
    for suffix in ("-wal", "-shm"):
        sidecar = Path(str(path) + suffix)
        if sidecar.exists():
            sidecar.replace(Path(str(target) + suffix))


def connect(path: Path, version: str) -> sqlite3.Connection:
    path.parent.mkdir(parents=True, exist_ok=True)
    connection: sqlite3.Connection | None = None
    try:
        connection = sqlite3.connect(path, timeout=30.0)
        connection.row_factory = sqlite3.Row
        connection.execute("PRAGMA foreign_keys = ON")
        connection.execute("PRAGMA journal_mode = WAL")
        connection.execute("PRAGMA synchronous = NORMAL")
        current = int(connection.execute("PRAGMA user_version").fetchone()[0])
        if current not in {0, SCHEMA_VERSION}:
            connection.close()
            connection = None
            quarantine_database(path, f"schema-v{current}")
            connection = sqlite3.connect(path, timeout=30.0)
            connection.row_factory = sqlite3.Row
            connection.execute("PRAGMA foreign_keys = ON")
            connection.execute("PRAGMA journal_mode = WAL")
            connection.execute("PRAGMA synchronous = NORMAL")
        try:
            schema = schema_path().read_text(encoding="utf-8")
        except OSError as exc:
            raise RuntimeError(
                f"SPLINED database schema is unavailable: {exc}"
            ) from exc
        connection.executescript(schema)
        connection.execute(f"PRAGMA user_version = {SCHEMA_VERSION}")
        with connection:
            connection.execute(
                "INSERT INTO db_maintenance_state"
                "(action_name, completed_at, details_json, splined_version) "
                "VALUES('schema', ?, ?, ?) "
                "ON CONFLICT(action_name) DO UPDATE SET "
                "completed_at=excluded.completed_at, "
                "details_json=excluded.details_json, "
                "splined_version=excluded.splined_version",
                (
                    utc_now(),
                    json.dumps(
                        {"schema_version": SCHEMA_VERSION},
                        separators=(",", ":"),
                    ),
                    version,
                ),
            )
        return connection
    except (sqlite3.DatabaseError, OSError):
        if connection is not None:
            try:
                connection.close()
            except Exception:
                pass
        quarantine_database(path, "corrupt")
        connection = sqlite3.connect(path, timeout=30.0)
        connection.row_factory = sqlite3.Row
        connection.execute("PRAGMA foreign_keys = ON")
        connection.execute("PRAGMA journal_mode = WAL")
        connection.execute("PRAGMA synchronous = NORMAL")
        connection.executescript(schema_path().read_text(encoding="utf-8"))
        connection.execute(f"PRAGMA user_version = {SCHEMA_VERSION}")
        return connection


def index_is_usable(
    connection: sqlite3.Connection,
    expected: dict[str, Any],
) -> bool:
    row = connection.execute(
        "SELECT signature_json FROM picker_inventory WHERE inventory_key=?",
        (INVENTORY_KEY,),
    ).fetchone()
    if row is None:
        return False
    try:
        saved = json.loads(str(row["signature_json"]))
    except (TypeError, ValueError):
        return False
    return saved == expected


def aggregate_artist(statuses: list[str]) -> str:
    if "bypassed" in statuses:
        return "contains-bypass"
    if statuses and all(
        status in {"processed", "timeout"} for status in statuses
    ):
        return "complete"
    if any(status in {"processed", "timeout"} for status in statuses):
        return "partial"
    return "unprocessed"


def _insert_snapshot(
    connection: sqlite3.Connection,
    artists: dict[str, dict[str, Any]],
    albums: list[dict[str, Any]],
    reviews: list[dict[str, Any]],
    expected_signature: dict[str, Any],
    version: str,
    reason: str,
) -> None:
    now = utc_now()
    old_paths = {
        str(row["album_key"]): str(row["path"])
        for row in connection.execute("SELECT album_key, path FROM albums")
    }
    new_paths = {str(row["album_key"]): str(row["path"]) for row in albums}
    with connection:
        for album_key, old_path in old_paths.items():
            new_path = new_paths.get(album_key)
            if new_path == old_path:
                continue
            connection.execute(
                "INSERT INTO retired_album_paths"
                "(album_key, album_path, reason, retired_at, splined_version) "
                "VALUES(?, ?, ?, ?, ?) "
                "ON CONFLICT(album_key, album_path) DO UPDATE SET "
                "reason=excluded.reason, retired_at=excluded.retired_at, "
                "splined_version=excluded.splined_version",
                (
                    album_key,
                    old_path,
                    "moved" if new_path else "removed",
                    now,
                    version,
                ),
            )
        connection.execute("DELETE FROM albums")
        connection.execute("DELETE FROM artists")
        connection.execute("DELETE FROM album_refresh_review_queue")
        connection.executemany(
            "INSERT INTO artists"
            "(artist_key, artist_name, artist_sort, musicbrainz_artistid, "
            "primary_path, status, album_count, unprocessed_count, "
            "processed_count, bypassed_count, timeout_count, created_at, "
            "updated_at, last_seen_at, splined_version) "
            "VALUES(:artist_key, :artist_name, :artist_sort, "
            ":musicbrainz_artistid, :primary_path, :status, 0, 0, 0, 0, 0, "
            ":created_at, :updated_at, :last_seen_at, :splined_version)",
            [dict(row, splined_version=version) for row in artists.values()],
        )
        connection.executemany(
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
            ":created_at, :updated_at, :last_seen_at, :splined_version)",
            [dict(row, splined_version=version) for row in albums],
        )
        for review in reviews:
            connection.execute(
                "INSERT INTO album_refresh_review_queue"
                "(album_key, requested_at, source, details_json, "
                "splined_version) VALUES(?, ?, ?, ?, ?)",
                (
                    review["album_key"],
                    now,
                    review["source"],
                    json.dumps(review["details"], separators=(",", ":")),
                    version,
                ),
            )
        folders = {
            "artists": len(artists),
            "albums": len(albums),
            "review_queue": len(reviews),
        }
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
                INVENTORY_KEY,
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
                INVENTORY_KEY,
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
    connection.execute("ANALYZE")
    connection.execute("PRAGMA optimize")


def build_index(
    context: IndexContext,
    connection: sqlite3.Connection,
    reason: str,
) -> None:
    core = context.core
    version = str(core.display_version())
    core.emit_ui(
        "cache_build_start",
        cache_kind="media-index",
        reason=reason,
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

    def progress(directories: int, album_count: int) -> None:
        core.emit_ui(
            "cache_progress",
            phase="inventory",
            status="Discovering Artist / Album folders",
            processed=directories,
            total=0,
            percent=None,
            albums=album_count,
            current_artist="",
        )

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
    existing = {
        str(row["album_key"]): row for row in existing_rows
    }
    existing_by_path = {
        str(row["path"]): row for row in existing_rows
    }
    artist_rows: dict[str, dict[str, Any]] = {}
    album_rows: list[dict[str, Any]] = []
    reviews: list[dict[str, Any]] = []
    used_keys: dict[str, str] = {}
    tag_reads = 0
    tag_reuses = 0
    total = len(albums)
    for number, album in enumerate(albums, 1):
        if getattr(core, "tui_cancelled", lambda: False)():
            raise core.TuiSessionExit()
        if not album.audio_files:
            continue
        artist, album_row, review, reused_tags = inspect_album(
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
        if number == 1 or number % 16 == 0 or number == total:
            core.emit_ui(
                "cache_progress",
                phase="tag-index",
                status=(
                    "Reading representative Album tags · "
                    f"{tag_reads:,} read · {tag_reuses:,} reused"
                ),
                processed=number,
                total=total,
                percent=(number / total * 100.0 if total else 100.0),
                albums=number,
                current_artist=artist["artist_name"],
            )

    _insert_snapshot(
        connection,
        artist_rows,
        album_rows,
        reviews,
        signature(context.library_root, context.ignored, context.cover_name),
        version,
        reason,
    )
    core.emit_ui(
        "activity",
        category="inventory",
        state="done",
        source="splined-db",
        message=(
            f"SPLINED index ready · {len(artist_rows):,} Artist(s) · "
            f"{len(album_rows):,} Album(s) · "
            f"{tag_reads:,} tag read(s) · {tag_reuses:,} reused"
        ),
    )


def project_statuses(
    context: IndexContext,
    connection: sqlite3.Connection,
) -> None:
    core = context.core
    history_albums = context.completion_history.get("albums", {})
    if not isinstance(history_albums, dict):
        history_albums = {}
    policy_fingerprint = core.scan_policy_fingerprint(context.cfg, context.sources)
    now_epoch = time.time()
    updates: list[tuple[str, int, str, str, str]] = []
    by_artist: dict[str, list[str]] = {}
    rows = connection.execute(
        "SELECT album_key, artist_key, path, local_art_json, "
        "inventory_fingerprint, status, bypassed, cover_found, "
        "timeout_until FROM albums"
    )
    for row in rows:
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
        elif current == "timeout" and timeout_until:
            try:
                active = datetime.fromisoformat(timeout_until).timestamp() > now_epoch
            except ValueError:
                active = False
            status = "timeout" if active else "processed"
            if not active:
                timeout_until = ""
        elif (
            current == "processed"
            or bool(row["cover_found"])
            or json_list(row["local_art_json"])
        ):
            status = "processed"
        else:
            status = "unprocessed"
        by_artist.setdefault(str(row["artist_key"]), []).append(status)
        updates.append(
            (
                status,
                int(status == "bypassed"),
                timeout_until,
                utc_now(),
                str(row["album_key"]),
            )
        )

    with connection:
        connection.executemany(
            "UPDATE albums SET status=?, bypassed=?, timeout_until=?, "
            "updated_at=? WHERE album_key=?",
            updates,
        )
        for artist_key, statuses in by_artist.items():
            update_artist_aggregate(connection, artist_key, statuses)


def update_artist_aggregate(
    connection: sqlite3.Connection,
    artist_key: str,
    statuses: list[str] | None = None,
) -> None:
    if statuses is None:
        statuses = [
            str(row[0])
            for row in connection.execute(
                "SELECT status FROM albums WHERE artist_key=?",
                (artist_key,),
            )
        ]
    counts = {name: statuses.count(name) for name in VALID_ALBUM_STATUSES}
    connection.execute(
        "UPDATE artists SET status=?, album_count=?, unprocessed_count=?, "
        "processed_count=?, bypassed_count=?, timeout_count=?, updated_at=? "
        "WHERE artist_key=?",
        (
            aggregate_artist(statuses),
            len(statuses),
            counts["unprocessed"],
            counts["processed"],
            counts["bypassed"],
            counts["timeout"],
            utc_now(),
            artist_key,
        ),
    )
