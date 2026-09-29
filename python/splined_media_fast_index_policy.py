"""Fast, tag-identified media-index construction for SPLINED.

The Select Media database needs one representative audio file per Album for
Mutagen identity.  It does not need to retain every track path.  This policy
uses a dedicated index discovery pass that keeps only:

- one deterministic representative audio path;
- an integer track count;
- local artwork/sidecar paths;
- full audio metadata only for the small timeout-fingerprint subset.

Representative tag and cover inspection is then parallelized while logical
Album-key collision handling and SQLite checkpoint commits remain deterministic
and serialized.
"""

from __future__ import annotations

from collections import deque
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import time
from typing import Any, Callable

from PIL import Image

import splined_media_build_policy as build_policy
import splined_media_database as database
import splined_media_tags as media_tags


_INSTALLED = False
_INDEX_WORKERS_MAX = 8


def _index_track_count(album: Any) -> int:
    value = getattr(album, "_splined_index_track_count", None)
    if isinstance(value, int) and value >= 0:
        return value
    return len(getattr(album, "audio_files", []))


def _index_sidecars(album: Any) -> list[Path]:
    saved = getattr(album, "_splined_index_sidecars", None)
    if isinstance(saved, (list, tuple)):
        return [Path(value) for value in saved]

    audio_extensions = {
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
    sidecars: list[Path] = []
    try:
        with os.scandir(album.path) as iterator:
            for item in iterator:
                try:
                    if item.is_symlink() or not item.is_file(follow_symlinks=False):
                        continue
                except OSError:
                    continue
                path = Path(item.path)
                if path.suffix.casefold() not in audio_extensions:
                    sidecars.append(path)
    except OSError:
        sidecars = [Path(value) for value in getattr(album, "local_art_files", [])]
    return sorted(sidecars, key=lambda value: value.name.casefold())


def _cover_statistics(album: Any, cover_name: str) -> dict[str, Any]:
    """Inspect sidecars without requiring a complete track-path catalogue."""
    sidecars = _index_sidecars(album)
    prefix = cover_name.strip().casefold() or "cover"
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
        (covers if path.stem.casefold().startswith(prefix) else others).append(path)

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

    local_art = sorted(
        {
            str(path)
            for path in (
                *getattr(album, "local_art_files", []),
                *covers,
            )
        },
        key=str.casefold,
    )
    return {
        "artwork": artwork,
        "root_files": len(sidecars),
        "cover_files": len(covers),
        "cover_names": [path.name for path in covers],
        "local_art": local_art,
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


def _discover_index_albums(
    core: Any,
    root: Path,
    ignored_subs: list[str],
    configured_file_name: str,
    *,
    fingerprint_paths: set[str] | None = None,
    progress: Callable[[int, int], None] | None = None,
    workers: int = 8,
    cancelled: Callable[[], bool] | None = None,
) -> tuple[list[Any], list[Path]]:
    """Discover Albums while retaining one representative track path only."""
    root = Path(root)
    if not root.exists():
        raise core.SplinedError(f"SPLINED scan directory does not exist: {root}")
    if not root.is_dir():
        raise core.SplinedError(f"SPLINED scan path is not a directory: {root}")

    audio_extensions = {
        str(value).casefold() for value in getattr(core, "AUDIO_EXTENSIONS", set())
    }
    ignored: list[Path] = []
    albums: list[Any] = []
    fingerprint_required = fingerprint_paths or set()
    worker_count = max(1, min(32, int(workers)))
    directories_seen = 0

    def inspect_directory(directory: Path) -> tuple[Any | None, list[Path]]:
        try:
            iterator = os.scandir(directory)
        except OSError as exc:
            raise core.SplinedError(
                f"Unable to read SPLINED scan directory {directory}: {exc}"
            ) from exc

        representative: Path | None = None
        representative_sort = ""
        track_count = 0
        fingerprint_rows: list[tuple[Path, int | None, int | None]] = []
        local_art: list[Path] = []
        sidecars: list[Path] = []
        children: list[Path] = []
        collect_fingerprint = str(directory) in fingerprint_required

        with iterator:
            for entry in iterator:
                if entry.is_symlink():
                    continue
                path = Path(entry.path)
                try:
                    if entry.is_dir(follow_symlinks=False):
                        children.append(path)
                        continue
                    if not entry.is_file(follow_symlinks=False):
                        continue
                except OSError:
                    continue

                suffix = path.suffix.casefold()
                if suffix in audio_extensions:
                    track_count += 1
                    sort_key = path.name.casefold()
                    if representative is None or sort_key < representative_sort:
                        representative = path
                        representative_sort = sort_key
                    if collect_fingerprint:
                        try:
                            stat = entry.stat(follow_symlinks=False)
                            fingerprint_rows.append(
                                (path, int(stat.st_size), int(stat.st_mtime_ns))
                            )
                        except OSError:
                            fingerprint_rows.append((path, None, None))
                    continue

                sidecars.append(path)
                if core._is_inventory_local_art(path, configured_file_name):
                    local_art.append(path)

        album = None
        if representative is not None:
            fingerprint = (
                core._fingerprint_from_metadata(
                    sorted(fingerprint_rows, key=lambda row: str(row[0]).casefold())
                )
                if collect_fingerprint
                else None
            )
            album = core.AlbumDir(
                directory,
                [representative],
                sorted(local_art, key=lambda value: value.name.casefold()),
                fingerprint,
            )
            album._splined_index_track_count = track_count
            album._splined_index_sidecars = sorted(
                sidecars,
                key=lambda value: value.name.casefold(),
            )
        return album, children

    queued: deque[Path] = deque([root])
    pending: dict[concurrent.futures.Future[Any], Path] = {}
    with concurrent.futures.ThreadPoolExecutor(
        max_workers=worker_count,
        thread_name_prefix="splined-index-discovery",
    ) as executor:
        while queued or pending:
            if cancelled is not None and cancelled():
                raise core.TuiSessionExit()
            while queued and len(pending) < worker_count * 2:
                directory = queued.popleft()
                pending[executor.submit(inspect_directory, directory)] = directory
            done, _remaining = concurrent.futures.wait(
                pending,
                return_when=concurrent.futures.FIRST_COMPLETED,
            )
            for future in done:
                pending.pop(future, None)
                album, children = future.result()
                directories_seen += 1
                if album is not None:
                    albums.append(album)
                for child in children:
                    if core.should_ignore(child.name, ignored_subs):
                        ignored.append(child)
                    else:
                        queued.append(child)
                if progress is not None and directories_seen % 250 == 0:
                    progress(directories_seen, len(albums))

    if progress is not None:
        progress(directories_seen, len(albums))
    albums.sort(key=lambda value: str(value.path).casefold())
    ignored.sort(key=lambda value: str(value).casefold())
    return albums, ignored


def _assign_unique_album_key(
    row: dict[str, Any],
    review: dict[str, Any] | None,
    *,
    path: str,
    library_root: Path,
    existing_by_path: dict[str, Any],
    used_keys: dict[str, str],
) -> tuple[dict[str, Any], dict[str, Any] | None]:
    key = str(row["album_key"])
    base_key = key.split(":copy:", 1)[0]
    prior_path = used_keys.get(key) or used_keys.get(base_key)
    if prior_path is None or prior_path == path:
        used_keys[key] = path
        used_keys.setdefault(base_key, path)
        return row, review

    previous = existing_by_path.get(path)
    previous_key = str(previous["album_key"]) if previous is not None else ""
    if previous_key.startswith(base_key + ":copy:"):
        copy_key = previous_key
    else:
        try:
            relative = str(Path(path).relative_to(library_root))
        except ValueError:
            relative = path
        copy_key = (
            base_key
            + ":copy:"
            + hashlib.sha256(relative.encode("utf-8")).hexdigest()[:12]
        )
        if previous is None:
            row["status"] = (
                "processed" if int(row.get("cover_found", 0) or 0) else "unprocessed"
            )
            row["processed_at"] = ""
            row["bypassed"] = 0
            row["timeout_until"] = ""
            row["selected_source"] = ""

    row["album_key"] = copy_key
    used_keys[copy_key] = path
    return row, {
        "album_key": copy_key,
        "source": "duplicate-tag-identity",
        "details": {
            "base_key": base_key,
            "existing_path": prior_path,
            "duplicate_path": path,
        },
    }


def _build_index(
    context: database.IndexContext,
    connection: Any,
    reason: str,
) -> None:
    core = context.core
    version = str(core.display_version())
    expected_signature = database.signature(
        context.library_root,
        context.ignored,
        context.cover_name,
    )
    signature_token = build_policy._signature_token(expected_signature)
    build_policy._prepare_stage_scope(connection, signature_token)

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
            status="Discovering Album folders · one representative track retained",
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
            build_policy._write_stage_batch(
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

    fingerprint_paths = core.timeout_fingerprint_paths(
        context.completion_history,
        context.cfg,
        context.sources,
        context.timeout_hours,
    )
    albums, _ignored_dirs = _discover_index_albums(
        core,
        context.library_root,
        context.ignored,
        context.cover_name,
        fingerprint_paths=fingerprint_paths,
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
    staged_by_path = build_policy._load_stage_payloads(
        connection,
        signature_token,
    )

    def inspect_one(album: Any) -> tuple[Any, dict[str, Any], dict[str, Any], dict[str, Any] | None, bool, bool]:
        path = str(album.path)
        staged = staged_by_path.get(path)
        if staged is not None and build_policy._stage_payload_matches(
            staged,
            signature_token,
            album,
        ):
            review_raw = staged.get("review")
            return (
                album,
                dict(staged["artist"]),
                dict(staged["album"]),
                dict(review_raw) if isinstance(review_raw, dict) else None,
                False,
                True,
            )
        artist, album_row, review, reused_tags = database.inspect_album(
            album,
            context.library_root,
            context.cover_name,
            existing,
            existing_by_path,
            {},
        )
        album_row["track_count"] = _index_track_count(album)
        return album, artist, album_row, review, reused_tags, False

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
    worker_count = max(
        1,
        min(_INDEX_WORKERS_MAX, int(getattr(core, "INVENTORY_WORKERS", 8))),
    )

    with concurrent.futures.ThreadPoolExecutor(
        max_workers=worker_count,
        thread_name_prefix="splined-index-tags",
    ) as executor:
        results = executor.map(inspect_one, albums)
        for number, result in enumerate(results, 1):
            if getattr(core, "tui_cancelled", lambda: False)():
                if pending_stage:
                    build_policy._write_stage_batch(
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

            album, artist, album_row, review, reused_tags, checkpoint = result
            path = str(album.path)
            album_row, review = _assign_unique_album_key(
                album_row,
                review,
                path=path,
                library_root=context.library_root,
                existing_by_path=existing_by_path,
                used_keys=used_keys,
            )
            if checkpoint:
                checkpoint_reuses += 1
            else:
                if reused_tags:
                    tag_reuses += 1
                else:
                    tag_reads += 1
                pending_stage.append(
                    {
                        "signature": signature_token,
                        "path": path,
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
                len(pending_stage) >= build_policy._STAGE_BATCH
                or number == total
            ):
                build_policy._write_stage_batch(
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

            if (
                number == 1
                or number % build_policy._STAGE_BATCH == 0
                or number == total
            ):
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
                        "Reading one representative track per Album · "
                        f"{tag_reads:,} read · {tag_reuses:,} DB reused · "
                        f"{checkpoint_reuses:,} checkpoint reused · "
                        f"{worker_count} workers"
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
                    eta_text=build_policy._format_eta(eta),
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
    build_policy._clear_stage(connection)
    core.emit_ui(
        "activity",
        category="inventory",
        state="done",
        source="splined-db",
        message=(
            f"SPLINED index ready · {len(artist_rows):,} Artist(s) · "
            f"{len(album_rows):,} Album(s) · "
            f"{tag_reads:,} representative tag read(s) · "
            f"{tag_reuses:,} DB reused · "
            f"{checkpoint_reuses:,} checkpoint reused"
        ),
    )


def install(core: Any) -> None:
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True

    import splined_media_index

    media_tags.cover_statistics = _cover_statistics
    original_inspect = database.inspect_album

    def inspect_album(*args: Any, **kwargs: Any):
        album = args[0] if args else kwargs.get("album")
        artist, row, review, reused = original_inspect(*args, **kwargs)
        if album is not None:
            row["track_count"] = _index_track_count(album)
        return artist, row, review, reused

    database.inspect_album = inspect_album
    splined_media_index.build_index = _build_index
    core._splined_media_fast_index_installed = True
