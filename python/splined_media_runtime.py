"""Runtime session and live-status integration for ``splined.db``.

The database is the stable Select Media read model.  Filesystem and Mutagen
work belongs to the initial/explicit index refresh; normal startup and normal
picker interaction stay database-only.
"""

from __future__ import annotations

import json
from pathlib import Path
import shutil
import sqlite3
import threading
import time
from typing import Any, Iterable

from PIL import Image

from splined_media_database import (
    DB_NAME,
    IndexContext,
    VALID_ALBUM_STATUSES,
    VALID_ARTIST_STATUSES,
    aggregate_artist,
    connect,
    project_statuses,
    update_artist_aggregate,
)
from splined_media_tags import json_list, utc_now


_AUDIO_EXTENSIONS = {
    ".mp3",
    ".flac",
    ".m4a",
    ".mp4",
    ".ogg",
    ".opus",
    ".wav",
    ".aiff",
    ".aif",
}
_LOCK = threading.RLock()
_ACTIVE_DB_PATH: Path | None = None
_ACTIVE_COVER_NAME = "cover"
_PATH_TO_ALBUM_KEY: dict[str, str] = {}
_ARTIST_BY_ALBUM_KEY: dict[str, str] = {}
_STATUS_BY_PATH: dict[str, str] = {}
_STATS_BY_PATH: dict[str, dict[str, Any]] = {}
_CURRENT_ALBUM_PATH = ""


def stats_from_row(row: Any) -> dict[str, Any]:
    cover_width = row["cover_width"]
    cover_height = row["cover_height"]
    compilation = bool(row["compilation"])
    album_mbid_missing = not bool(
        str(row["musicbrainz_albumid"] or "").strip()
    )
    return {
        "path": str(row["path"]),
        "album": str(row["album_name"]),
        "artist": str(row["artist_name"]),
        "year": str(row["release_year"] or ""),
        "tracks": int(row["track_count"] or 0),
        "compilation": compilation,
        "album_mbid_missing": album_mbid_missing,
        "manual_compilation_eligible": (
            compilation and album_mbid_missing
        ),
        "artwork": {
            "JPEG": int(row["artwork_jpeg"] or 0),
            "PNG": int(row["artwork_png"] or 0),
            "WEBP": int(row["artwork_webp"] or 0),
            "OTHER": int(row["artwork_other"] or 0),
        },
        "root_files": int(row["root_files"] or 0),
        "cover_files": int(row["cover_files"] or 0),
        "cover_names": json_list(row["cover_names_json"]),
        "cover_resolution": (
            f"{int(cover_width)}x{int(cover_height)}"
            if cover_width and cover_height
            else ""
        ),
        "cover_path": str(row["cover_path"] or ""),
        "other_filenames": json_list(row["other_filenames_json"]),
        "webp_found": bool(row["webp_found"]),
        "webp_size_mb": float(row["webp_size_mb"] or 0.0),
        "webp_resolution": str(row["webp_resolution"] or ""),
        "webp_conversion": bool(row["webp_conversion"]),
    }


def _joined_rows(connection: sqlite3.Connection) -> list[sqlite3.Row]:
    return list(
        connection.execute(
            "SELECT albums.*, artists.artist_name "
            "FROM albums JOIN artists "
            "ON artists.artist_key=albums.artist_key"
        )
    )


def refresh_maps(connection: sqlite3.Connection) -> None:
    global _PATH_TO_ALBUM_KEY, _ARTIST_BY_ALBUM_KEY
    global _STATUS_BY_PATH, _STATS_BY_PATH
    rows = _joined_rows(connection)
    with _LOCK:
        _PATH_TO_ALBUM_KEY = {
            str(row["path"]): str(row["album_key"]) for row in rows
        }
        _ARTIST_BY_ALBUM_KEY = {
            str(row["album_key"]): str(row["artist_key"]) for row in rows
        }
        _STATUS_BY_PATH = {
            str(row["path"]): str(row["status"]) for row in rows
        }
        _STATS_BY_PATH = {
            str(row["path"]): stats_from_row(row) for row in rows
        }


def cached_stats(path: Path) -> dict[str, Any] | None:
    with _LOCK:
        value = _STATS_BY_PATH.get(str(path))
        return json.loads(json.dumps(value)) if value is not None else None


def physical_artist_folder(library_root: Path | str, album_path: Any) -> str:
    """Return the first physical folder below the configured library root.

    Indexed paths come from the deployment host and can therefore use a
    different separator than the process reading them (for example, Linux
    paths in a database inspected by Windows tests).  Normalize only for the
    comparison and preserve the stored path style in the returned value.
    """
    root = str(library_root).rstrip("/\\")
    album = str(album_path or "").rstrip("/\\")
    if not root or not album:
        return ""

    normalized_root = root.replace("\\", "/")
    normalized_album = album.replace("\\", "/")
    prefix = f"{normalized_root}/"
    windows_style = "\\" in root or (
        len(normalized_root) >= 2 and normalized_root[1] == ":"
    )
    comparable_album = (
        normalized_album.casefold() if windows_style else normalized_album
    )
    comparable_prefix = prefix.casefold() if windows_style else prefix
    if not comparable_album.startswith(comparable_prefix):
        return ""

    relative = normalized_album[len(prefix) :]
    folder_name = relative.split("/", 1)[0]
    if not folder_name:
        return ""
    separator = "\\" if "\\" in root and "/" not in root else "/"
    return f"{root}{separator}{folder_name}"


def project_picker_artists(
    core: Any,
    library_root: Path | str,
    album_rows: Iterable[Any],
) -> list[Any]:
    """Project indexed Albums into unique physical library-root folders."""
    by_path: dict[str, Any] = {}
    indexed_at = time.time()
    for row in album_rows:
        path = physical_artist_folder(library_root, row["path"])
        if not path:
            continue
        if path in by_path:
            continue
        folder_name = path.rstrip("/\\").replace("\\", "/").rsplit("/", 1)[-1]
        by_path[path] = core.PickerArtist(path, folder_name, True, indexed_at)
    return sorted(
        by_path.values(),
        key=lambda artist: (artist.name.casefold(), artist.path.casefold()),
    )


def project_picker_folder_statuses(album_rows: Iterable[Any]) -> dict[str, str]:
    """Aggregate Album statuses independently for each physical folder."""
    grouped: dict[str, list[str]] = {}
    for row in album_rows:
        path = str(row["artist_path"] or "")
        status = str(row["status"] or "")
        if path and status in VALID_ALBUM_STATUSES:
            grouped.setdefault(path, []).append(status)
    return {
        path: aggregate_artist(statuses)
        for path, statuses in grouped.items()
    }


def populate_session(
    context: IndexContext,
    connection: sqlite3.Connection,
) -> None:
    """Hydrate the complete resident picker from SQLite before first paint."""
    project_statuses(context, connection)
    album_rows = [
        dict(row)
        for row in connection.execute(
            "SELECT albums.*, artists.artist_name AS artist_name, "
            "artists.artist_sort AS artist_sort "
            "FROM albums JOIN artists "
            "ON artists.artist_key=albums.artist_key"
        )
    ]
    for row in album_rows:
        row["artist_path"] = physical_artist_folder(
            context.library_root,
            row["path"],
        )
    album_rows.sort(
        key=lambda row: (
            str(row["artist_path"]).casefold(),
            str(row["album_sort"] or row["album_name"]).casefold(),
            str(row["album_name"]).casefold(),
            str(row["path"]).casefold(),
        )
    )
    picker_artists = project_picker_artists(
        context.core,
        context.library_root,
        album_rows,
    )

    global _PATH_TO_ALBUM_KEY, _ARTIST_BY_ALBUM_KEY
    global _STATUS_BY_PATH, _STATS_BY_PATH
    with _LOCK:
        _PATH_TO_ALBUM_KEY = {
            str(row["path"]): str(row["album_key"]) for row in album_rows
        }
        _ARTIST_BY_ALBUM_KEY = {
            str(row["album_key"]): str(row["artist_key"])
            for row in album_rows
        }
        _STATUS_BY_PATH = {
            str(row["path"]): str(row["status"]) for row in album_rows
        }
        _STATS_BY_PATH = {
            str(row["path"]): stats_from_row(row) for row in album_rows
        }

    session = context.session
    existing_paths = {str(row["path"]) for row in album_rows}
    selected = set(session.selected_paths)
    selected.intersection_update(existing_paths)
    selected_stats = {
        path: json.loads(json.dumps(_STATS_BY_PATH[path]))
        for path in selected
        if path in _STATS_BY_PATH
    }

    with session.lock:
        session.artists = list(picker_artists)
        session.album_records = [
            context.core.PickerAlbum(
                str(row["path"]),
                str(row["artist_path"]),
                str(row["album_name"]),
                tuple(json_list(row["local_art_json"])),
                str(row["inventory_fingerprint"] or "") or None,
            )
            for row in album_rows
        ]
        session.loaded_artists = {artist.path for artist in picker_artists}
        session.probed_artist_statuses = project_picker_folder_statuses(album_rows)
        session.selected_paths.clear()
        session.selected_paths.update(selected)
        session.selected_statistics.clear()
        session.selected_statistics.update(selected_stats)
        session.selected_statistics_pending.clear()
        session.status_probe_started = True
        session.status_probe_complete = True
        # The authoritative function checks this non-running placeholder and
        # exits immediately because status_probe_complete is true.
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


def migrate_legacy_json(context: IndexContext) -> None:
    """Retire the v2 JSON only after a usable SQLite index exists."""
    try:
        history_dir = context.core.runtime_history_dir(
            context.config_file,
            context.cfg,
        )
    except Exception:
        return
    legacy = Path(history_dir) / "select-media-status.json"
    target = legacy.with_name(legacy.name + ".legacy")
    if not legacy.exists() or target.exists():
        return
    try:
        legacy.replace(target)
    except OSError:
        pass


def set_active_database(path: Path | None, cover_name: str = "cover") -> None:
    global _ACTIVE_DB_PATH, _ACTIVE_COVER_NAME
    _ACTIVE_DB_PATH = path
    _ACTIVE_COVER_NAME = str(cover_name).strip() or "cover"


def set_current_album(path: str) -> None:
    global _CURRENT_ALBUM_PATH
    _CURRENT_ALBUM_PATH = path


def sync_library_payload(payload: dict[str, Any]) -> None:
    """Persist only Album statuses that actually changed in the UI model."""
    db_path = _ACTIVE_DB_PATH
    rows = payload.get("albums")
    if db_path is None or not isinstance(rows, list):
        return

    with _LOCK:
        path_map = dict(_PATH_TO_ALBUM_KEY)
        known_status = dict(_STATUS_BY_PATH)
        artist_map = dict(_ARTIST_BY_ALBUM_KEY)

    changes: list[tuple[str, int, str, str]] = []
    affected_artists: set[str] = set()
    changed_paths: dict[str, str] = {}
    now = utc_now()
    for raw in rows:
        if not isinstance(raw, dict):
            continue
        path = str(raw.get("path", ""))
        status = str(raw.get("status", "")).casefold()
        album_key = path_map.get(path)
        if (
            album_key is None
            or status not in VALID_ALBUM_STATUSES
            or known_status.get(path) == status
        ):
            continue
        changes.append((status, int(status == "bypassed"), now, album_key))
        changed_paths[path] = status
        artist_key = artist_map.get(album_key)
        if artist_key:
            affected_artists.add(artist_key)

    if not changes:
        return

    connection = connect(db_path, "runtime")
    try:
        with connection:
            connection.executemany(
                "UPDATE albums SET status=?, bypassed=?, updated_at=? "
                "WHERE album_key=?",
                changes,
            )
            for artist_key in affected_artists:
                update_artist_aggregate(connection, artist_key)
    finally:
        connection.close()

    with _LOCK:
        _STATUS_BY_PATH.update(changed_paths)


def _folder_statistics(album_path: Path, cover_name: str) -> dict[str, Any]:
    """Refresh one Album's materialized cover/statistics after a SPLINED write."""
    sidecars: list[Path] = []
    track_count = 0
    try:
        entries = sorted(album_path.iterdir(), key=lambda path: path.name.casefold())
    except OSError:
        entries = []
    for path in entries:
        if path.is_symlink() or not path.is_file():
            continue
        if path.suffix.casefold() in _AUDIO_EXTENSIONS:
            track_count += 1
        else:
            sidecars.append(path)

    prefix = cover_name.casefold()
    artwork = {"JPEG": 0, "PNG": 0, "WEBP": 0, "OTHER": 0}
    covers: list[Path] = []
    others: list[Path] = []
    webps: list[Path] = []
    for path in sidecars:
        suffix = path.suffix.casefold()
        if suffix in {".jpg", ".jpeg"}:
            artwork["JPEG"] += 1
        elif suffix == ".png":
            artwork["PNG"] += 1
        elif suffix == ".webp":
            artwork["WEBP"] += 1
            webps.append(path)
        else:
            artwork["OTHER"] += 1
        if path.stem.casefold().startswith(prefix):
            covers.append(path)
        else:
            others.append(path)

    canonical: Path | None = None
    width: int | None = None
    height: int | None = None
    image_format = ""
    if covers:
        canonical = sorted(
            covers,
            key=lambda path: (
                path.stem.casefold() != prefix,
                path.name.casefold(),
            ),
        )[0]
        try:
            with Image.open(canonical) as image:
                width = int(image.width)
                height = int(image.height)
                image_format = str(
                    image.format or canonical.suffix[1:]
                ).upper()
        except Exception:
            image_format = canonical.suffix[1:].upper()

    webp_size_mb = 0.0
    webp_resolution = ""
    webp_conversion = False
    if webps:
        def size(path: Path) -> int:
            try:
                return int(path.stat().st_size)
            except OSError:
                return 0

        webp = max(webps, key=size)
        webp_size_mb = round(size(webp) / 1_000_000, 2)
        try:
            with Image.open(webp) as image:
                webp_resolution = f"{int(image.width)}x{int(image.height)}"
                webp_conversion = True
        except Exception:
            pass

    return {
        "track_count": track_count,
        "artwork": artwork,
        "root_files": len(sidecars),
        "cover_files": len(covers),
        "cover_names": [path.name for path in covers],
        "local_art": [str(path) for path in covers],
        "other_filenames": [path.name for path in others],
        "cover_found": bool(canonical),
        "cover_path": str(canonical) if canonical else "",
        "cover_name": canonical.name if canonical else "",
        "cover_format": image_format,
        "cover_width": width,
        "cover_height": height,
        "webp_found": bool(webps),
        "webp_size_mb": webp_size_mb,
        "webp_resolution": webp_resolution,
        "webp_conversion": webp_conversion,
    }


def sync_material_result(payload: dict[str, Any]) -> None:
    db_path = _ACTIVE_DB_PATH
    path = _CURRENT_ALBUM_PATH
    if db_path is None or not path:
        return
    with _LOCK:
        album_key = _PATH_TO_ALBUM_KEY.get(path)
        artist_key = _ARTIST_BY_ALBUM_KEY.get(album_key or "")
    if album_key is None or artist_key is None:
        return

    outcome = str(payload.get("outcome", "")).casefold()
    if any(value in outcome for value in ("failed", "skipped", "bypass")):
        return

    album_path = Path(path)
    material = _folder_statistics(album_path, _ACTIVE_COVER_NAME)
    source = str(payload.get("source", ""))
    now = utc_now()
    connection = connect(db_path, "runtime")
    try:
        with connection:
            connection.execute(
                "UPDATE albums SET status='processed', processed_at=?, "
                "selected_source=?, track_count=?, cover_found=?, "
                "cover_path=?, cover_name=?, cover_format=?, cover_width=?, "
                "cover_height=?, artwork_jpeg=?, artwork_png=?, artwork_webp=?, "
                "artwork_other=?, root_files=?, cover_files=?, "
                "cover_names_json=?, local_art_json=?, other_filenames_json=?, "
                "webp_found=?, webp_size_mb=?, webp_resolution=?, "
                "webp_conversion=?, updated_at=? WHERE album_key=?",
                (
                    now,
                    source,
                    material["track_count"],
                    int(material["cover_found"]),
                    material["cover_path"],
                    material["cover_name"],
                    material["cover_format"],
                    material["cover_width"],
                    material["cover_height"],
                    material["artwork"]["JPEG"],
                    material["artwork"]["PNG"],
                    material["artwork"]["WEBP"],
                    material["artwork"]["OTHER"],
                    material["root_files"],
                    material["cover_files"],
                    json.dumps(material["cover_names"], separators=(",", ":")),
                    json.dumps(material["local_art"], separators=(",", ":")),
                    json.dumps(
                        material["other_filenames"], separators=(",", ":")
                    ),
                    int(material["webp_found"]),
                    material["webp_size_mb"],
                    material["webp_resolution"],
                    int(material["webp_conversion"]),
                    now,
                    album_key,
                ),
            )
            update_artist_aggregate(connection, artist_key)
            connection.execute(
                "INSERT INTO cache_history"
                "(cache_key, cache_type, album_key, action, payload_json, "
                "splined_version, event_at) "
                "VALUES(?, 'album_status', ?, 'processed', ?, 'runtime', ?)",
                (
                    album_key,
                    album_key,
                    json.dumps(payload, default=str, separators=(",", ":")),
                    now,
                ),
            )
        row = connection.execute(
            "SELECT albums.*, artists.artist_name FROM albums JOIN artists "
            "ON artists.artist_key=albums.artist_key "
            "WHERE albums.album_key=?",
            (album_key,),
        ).fetchone()
    finally:
        connection.close()

    if row is None:
        return
    with _LOCK:
        _STATUS_BY_PATH[path] = "processed"
        _STATS_BY_PATH[path] = stats_from_row(row)


def clean_transient_cache(core: Any, cache: Path) -> None:
    """Clear candidate/sample data while preserving the persistent SQLite index."""
    cache = Path(cache)
    if cache.exists():
        if cache.is_symlink() or not cache.is_dir():
            raise core.SplinedError(
                f"Refusing to clean unsafe SPLINED cache directory: {cache}"
            )
        if cache.parent == cache or not cache.name:
            raise core.SplinedError(
                f"Refusing to clean unsafe SPLINED cache path: {cache}"
            )
        preserved_prefixes = (DB_NAME + "-", DB_NAME + ".")
        for path in cache.iterdir():
            # Preserve the live DB, WAL/SHM/journal sidecars, and quarantined
            # recovery copies.  Everything else remains disposable run cache.
            if path.name == DB_NAME or path.name.startswith(preserved_prefixes):
                continue
            if path.is_symlink():
                path.unlink()
            elif path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink()
    cache.mkdir(parents=True, exist_ok=True)
