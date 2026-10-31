"""Web operations use indexed identities, configured paths, and real writes."""

from __future__ import annotations

from contextlib import closing
import logging
from dataclasses import asdict
from functools import wraps
from io import BytesIO
from pathlib import Path
from uuid import uuid4
import hashlib
import os
import tempfile
import threading

from PIL import Image, ImageOps
from psycopg import sql
from psycopg.types.json import Jsonb

import splined as core
import splined_scan as scan
import splined_media_database as media
import splined_media_tags as tags
import splined_compilation_authority_policy as compilation
from splined_artist_art import ArtistAlbum, artist_targets, write_image

from webgui.catalog import (
    Catalog,
    AUDIO_EXTENSIONS,
    inside,
    embedded_track_artwork_bytes,
)
from webgui.runtime import Runtime
from webgui import imaging


LOGGER = logging.getLogger("uvicorn.error")

NAME_INVENTORY_PREFIX = "names-v1:"
TRACK_INVENTORY_PREFIX = "tracks-v1:"


def _album_inventory_fingerprint(album, fallback=False):
    if fallback:
        return TRACK_INVENTORY_PREFIX + core.album_scan_fingerprint(album)
    digest = hashlib.sha256()
    for path in sorted(album.audio_files, key=lambda value: str(value).casefold()):
        digest.update(path.name.casefold().encode("utf-8", "surrogateescape"))
        digest.update(b"\0")
    return NAME_INVENTORY_PREFIX + digest.hexdigest()


def _indexed_album_unchanged(prior, album, fingerprint):
    if int(prior["track_count"] or 0) != len(album.audio_files):
        return False
    representative = sorted(
        album.audio_files, key=lambda value: str(value).casefold()
    )[0]
    try:
        stat = representative.stat()
        size = int(stat.st_size)
        mtime = int(stat.st_mtime_ns)
    except OSError:
        size = 0
        mtime = 0
    if (
        str(prior["representative_file"] or "").casefold()
        != str(representative).casefold()
        or int(prior["representative_size"] or 0) != size
        or int(prior["representative_mtime_ns"] or 0) != mtime
    ):
        return False
    saved = str(prior["inventory_fingerprint"] or "")
    if saved.startswith((NAME_INVENTORY_PREFIX, TRACK_INVENTORY_PREFIX)):
        return saved == fingerprint
    if fingerprint.startswith(TRACK_INVENTORY_PREFIX) and saved:
        return saved == fingerprint.removeprefix(TRACK_INVENTORY_PREFIX)
    return True


def serialized(function):
    @wraps(function)
    def operation(self, *args, **kwargs):
        with self.runtime.processing:
            return function(self, *args, **kwargs)

    return operation


class Services:
    def __init__(self, runtime: Runtime):
        self.runtime = runtime
        self.catalog = Catalog(
            runtime.music,
            runtime.database,
            runtime.artist_art,
            runtime.credentials,
            comp_thumbnail_side=int(
                runtime.cfg.get("output", {}).get("embedded_thumbnail_size", 1200)
            ),
            allow_comp_repairs=runtime.cfg.get("mode") == "write",
            cover_stem=str(runtime.cfg.get("output", {}).get("file_name", "cover")),
            cache=runtime.cache,
            temporary_cache=runtime.temporary_cache,
        )
        self.catalog.cache = runtime.cache
        self.catalog.artist_sources = tuple(
            core.resolve_sources(runtime.cfg, None, None, [])
        )
        self.lock = threading.RLock()

    def reload_settings(self):
        app = self.catalog
        r = self.runtime
        app.music = r.music
        app.artist_art = r.artist_art
        app.credentials = r.credentials
        app.cache = r.cache
        app.temporary_cache = r.temporary_cache
        app.comp_thumbnail_side = int(
            r.cfg.get("output", {}).get("embedded_thumbnail_size", 1200)
        )
        app.allow_comp_repairs = r.cfg.get("mode") == "write"
        app.cover_stem = str(r.cfg.get("output", {}).get("file_name", "cover"))
        app.artist_sources = tuple(core.resolve_sources(r.cfg, None, None, []))
        app.metadata_cache.clear()
        app.appearance_cache.clear()
        app.portrait_cache.clear()

    def audit(self, action, subject, details=None):
        core.runtime_log(
            "info",
            f"webgui.audit action={action} subject={core._redact_log_message(str(subject))}",
        )
        with closing(self.runtime.database.connect()) as db, db:
            db.raw.execute(
                "INSERT INTO web_audit(action,subject,details) VALUES(%s,%s,%s)",
                (action, str(subject), Jsonb(details or {})),
            )

    def diagnostics(self, limit=250):
        """Return bounded, credential-free operational events for Settings."""
        limit = max(1, min(int(limit), 500))
        with closing(self.runtime.database.connect()) as db:
            rows = db.execute(
                "SELECT action,subject,details,created_at FROM web_audit ORDER BY id DESC LIMIT ?",
                (limit,),
            ).fetchall()
        return [
            {
                "action": row["action"],
                "subject": row["subject"],
                "details": row["details"],
                "created_at": row["created_at"].isoformat(),
            }
            for row in rows
        ]

    def storage_summary(self):
        """Describe persistent WebGUI storage without walking network folders."""
        with closing(self.runtime.database.connect()) as db:
            row = db.execute(
                "SELECT pg_database_size(current_database()) AS database_bytes, current_database() AS database_name"
            ).fetchone()
        return {
            "database": f"PostgreSQL · {row['database_name']}",
            "database_bytes": int(row["database_bytes"] or 0),
            "cache": str(self.runtime.cache),
            "artist_art": str(self.runtime.artist_art),
            "credentials": str(self.runtime.credentials),
            "configuration": str(self.runtime.config_file),
            "version": core.VERSION,
            "mode": str(self.runtime.cfg.get("mode", "read")),
        }

    def album_row(self, key):
        with closing(self.runtime.database.connect()) as db:
            row = db.execute(
                "SELECT albums.*,artists.artist_name,artists.musicbrainz_artistid FROM albums JOIN artists ON artists.artist_key=albums.artist_key WHERE album_key=?",
                (key,),
            ).fetchone()
        if not row:
            raise KeyError("Album not found")
        if not inside(Path(row["path"]), self.runtime.music):
            raise ValueError("Album is outside the configured music library")
        return row

    def album_files(self, row):
        folder = Path(row["path"])
        return sorted(
            (
                p
                for p in folder.iterdir()
                if p.suffix.lower() in AUDIO_EXTENSIONS
                and p.is_file()
                and not p.is_symlink()
            ),
            key=lambda p: p.name.casefold(),
        )

    def _upsert(self, db, table, row, key):
        columns = list(row)
        statement = sql.SQL(
            "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {}"
        ).format(
            sql.Identifier(table),
            sql.SQL(",").join(map(sql.Identifier, columns)),
            sql.SQL(",").join(sql.Placeholder() for _ in columns),
            sql.Identifier(key),
            sql.SQL(",").join(
                sql.SQL("{}=excluded.{}").format(sql.Identifier(c), sql.Identifier(c))
                for c in columns
                if c != key
            ),
        )
        db.raw.execute(statement, list(row.values()))

    @serialized
    def index_changes(self, cancelled=None):
        r = self.runtime
        root = r.music
        if not root.is_dir():
            raise ValueError("Set an existing music library in Settings")
        with closing(r.database.connect()) as db:
            old = list(
                db.execute(
                    "SELECT album_key,path,track_count,inventory_fingerprint,musicbrainz_albumid,representative_file,representative_size,representative_mtime_ns FROM albums"
                )
            )
        old_paths = {str(row["path"]): row for row in old}
        albums, _ = core.inventory(
            root,
            list(r.cfg.get("library", {}).get("ignored_subs", [])),
            str(r.cfg.get("output", {}).get("file_name", "cover")),
            cancelled=cancelled,
        )
        current_paths = {str(album.path) for album in albums if album.audio_files}
        added = 0
        changed = 0
        examples = []
        for album in albums:
            if not album.audio_files:
                continue
            path = str(album.path)
            prior = old_paths.get(path)
            if prior is None:
                added += 1
                if len(examples) < 3:
                    examples.append(path)
                continue
            fingerprint = _album_inventory_fingerprint(
                album, not str(prior["musicbrainz_albumid"] or "").strip()
            )
            if not _indexed_album_unchanged(prior, album, fingerprint):
                changed += 1
                if len(examples) < 3:
                    examples.append(path)
        removed_paths = sorted(set(old_paths) - current_paths, key=str.casefold)
        examples.extend(removed_paths[: max(0, 3 - len(examples))])
        return {
            "count": added + changed + len(removed_paths),
            "added": added,
            "changed": changed,
            "removed": len(removed_paths),
            "examples": examples,
        }

    @serialized
    def refresh_index(self, progress=None, cancelled=None, mode="full"):
        r = self.runtime
        root = r.music
        if not root.is_dir():
            raise ValueError("Set an existing music library in Settings")
        if mode not in {"full", "incremental"}:
            raise ValueError("Library index mode must be full or incremental")
        with closing(r.database.connect()) as db:
            old = list(
                db.execute(
                    "SELECT albums.*,artists.artist_name,artists.artist_sort,artists.musicbrainz_artistid,artists.primary_path FROM albums JOIN artists ON artists.artist_key=albums.artist_key"
                )
            )
        old_keys = {row["album_key"]: row for row in old}
        old_paths = {row["path"]: row for row in old}
        albums, _ = core.inventory(
            root,
            list(r.cfg.get("library", {}).get("ignored_subs", [])),
            str(r.cfg.get("output", {}).get("file_name", "cover")),
            cancelled=cancelled,
        )
        for album in albums:
            if album.audio_files:
                prior = old_paths.get(str(album.path))
                fallback = prior is not None and not str(
                    prior["musicbrainz_albumid"] or ""
                ).strip()
                album.inventory_fingerprint = _album_inventory_fingerprint(
                    album, fallback
                )
        current_paths = {
            str(album.path) for album in albums if album.audio_files
        }
        used = (
            {
                str(row["album_key"]): str(row["path"])
                for row in old
                if str(row["path"]) in current_paths
            }
            if mode == "incremental"
            else {}
        )
        seen = set()
        touched_artists = set()
        changed_albums = 0
        now = tags.utc_now()
        for number, album in enumerate(albums, 1):
            if cancelled and cancelled():
                raise core.InteractionCancelled()
            if not album.audio_files:
                continue
            prior_at_path = old_paths.get(str(album.path))
            if mode == "incremental" and prior_at_path is not None:
                if _indexed_album_unchanged(
                    prior_at_path, album, str(album.inventory_fingerprint or "")
                ):
                    seen.add(str(prior_at_path["album_key"]))
                    continue
            artist, row, _review, _reused = tags.inspect_album(
                album,
                root,
                str(r.cfg.get("output", {}).get("file_name", "cover")),
                old_keys,
                old_paths,
                used,
            )
            artist["splined_version"] = core.VERSION
            row["splined_version"] = core.VERSION
            row["last_seen_at"] = now
            if not str(row["musicbrainz_albumid"] or "").strip():
                album.inventory_fingerprint = _album_inventory_fingerprint(
                    album, True
                )
                row["inventory_fingerprint"] = album.inventory_fingerprint
            seen.add(row["album_key"])
            changed_albums += 1
            touched_artists.add(str(artist["artist_key"]))
            with closing(r.database.connect()) as db, db:
                if (
                    mode == "incremental"
                    and prior_at_path is not None
                    and str(prior_at_path["album_key"]) != str(row["album_key"])
                ):
                    touched_artists.add(str(prior_at_path["artist_key"]))
                    db.execute(
                        "DELETE FROM tracks WHERE album_key=?",
                        (prior_at_path["album_key"],),
                    )
                    db.execute(
                        "DELETE FROM albums WHERE album_key=?",
                        (prior_at_path["album_key"],),
                    )
                previous = db.execute(
                    "SELECT 1 FROM artists WHERE artist_key=?", (artist["artist_key"],)
                ).fetchone()
                if previous:
                    artist.pop("status", None)
                self._upsert(db, "artists", artist, "artist_key")
                self._upsert(db, "albums", row, "album_key")
            # Missing-ID COMP albums need every track identity in PostgreSQL.
            # Build that detail during an explicit index refresh so ordinary
            # artist navigation never turns into a library-wide filesystem scan.
            if not str(row["musicbrainz_albumid"] or "").strip():
                self.hydrate_tracks(row["album_key"])
            if progress:
                progress(
                    {
                        "completed": number,
                        "total": len(albums),
                        "message": row["album_name"],
                    }
                )
        with closing(r.database.connect()) as db, db:
            # Removal happens only after a complete inventory, never after cancellation.
            if mode == "full":
                stale = list(
                    db.execute(
                        "SELECT album_key,artist_key FROM albums WHERE last_seen_at<>?",
                        (now,),
                    )
                )
            else:
                stale = [
                    row for row in old if str(row["path"]) not in current_paths
                ]
            for row in stale:
                touched_artists.add(str(row["artist_key"]))
                db.execute(
                    "DELETE FROM tracks WHERE album_key=?", (row["album_key"],)
                )
                db.execute(
                    "DELETE FROM albums WHERE album_key=?", (row["album_key"],)
                )
            db.raw.execute(
                "DELETE FROM artists WHERE NOT EXISTS (SELECT 1 FROM albums WHERE albums.artist_key=artists.artist_key)"
            )
            artists = (
                list(db.execute("SELECT artist_key FROM artists"))
                if mode == "full"
                else [
                    {"artist_key": artist_key}
                    for artist_key in touched_artists
                    if db.execute(
                        "SELECT 1 FROM artists WHERE artist_key=?", (artist_key,)
                    ).fetchone()
                ]
            )
            for artist in artists:
                media.update_artist_aggregate(db, artist["artist_key"])
        result = {
            "mode": mode,
            "albums": len(current_paths),
            "changed_albums": changed_albums,
            "removed_albums": len(stale),
        }
        # Artist appearances come from the refreshed catalog plus Navidrome.
        # Never retain an empty or partial artist projection across a refresh.
        self.catalog.appearance_cache.clear()
        self.catalog.metadata_cache.clear()
        self.catalog.portrait_cache.clear()
        self.audit("index-refresh", str(root), result)
        return result

    def hydrate_tracks(self, album_id):
        row = self.album_row(album_id)
        files = self.album_files(row)
        with closing(self.runtime.database.connect()) as db:
            old = {
                item["path"]: item
                for item in db.execute(
                    "SELECT * FROM tracks WHERE album_key=?", (album_id,)
                )
            }
        records, _, _ = tags.inspect_album_tracks(
            core.AlbumDir(Path(row["path"]), files), album_id, old
        )
        with closing(self.runtime.database.connect()) as db, db:
            for record in records:
                record["splined_version"] = core.VERSION
                self._upsert(db, "tracks", record, "track_key")
            existing = {str(p) for p in files}
            for path in old:
                if path not in existing:
                    db.execute(
                        "DELETE FROM tracks WHERE album_key=? AND path=?",
                        (album_id, path),
                    )
        self.refresh_embedding_stats(row)
        return records

    def refresh_embedding_stats(self, row):
        count = sum(
            bool(embedded_track_artwork_bytes(path)) for path in self.album_files(row)
        )
        with closing(self.runtime.database.connect()) as db, db:
            db.execute(
                "UPDATE albums SET embedded_tracks=? WHERE album_key=?",
                (count, row["album_key"]),
            )
        return count

    def comp_report(self, artist_id):
        album_ids = self.catalog.artist_comp_album_ids(artist_id)
        if not album_ids:
            return self.catalog.artist_comp_report(artist_id)
        placeholders = ",".join("?" for _ in album_ids)
        with closing(self.runtime.database.connect()) as db:
            albums = list(
                db.execute(
                    "SELECT albums.album_key,albums.track_count,count(tracks.track_key) AS indexed_track_count "
                    "FROM albums LEFT JOIN tracks ON tracks.album_key=albums.album_key "
                    "WHERE trim(coalesce(albums.musicbrainz_albumid,''))='' "
                    f"AND albums.album_key IN ({placeholders}) "
                    "GROUP BY albums.album_key,albums.track_count",
                    tuple(album_ids),
                )
            )
        pending = [
            row for row in albums
            if int(row["indexed_track_count"] or 0) < int(row["track_count"] or 0)
        ]
        LOGGER.info(
            "COMP report requested for artist %s; %d of %d fallback albums require track indexing",
            artist_id,
            len(pending),
            len(album_ids),
        )
        for row in pending:
            self.hydrate_tracks(row["album_key"])
        report = self.catalog.artist_comp_report(artist_id)
        LOGGER.info(
            "COMP report ready for artist %s: %d albums, %d matching tracks",
            artist_id,
            len(report),
            sum(len(group["tracks"]) for group in report),
        )
        return report

    def refresh_album(self, album_id, *, source=None):
        row = self.album_row(album_id)
        folder = Path(row["path"])
        files = self.album_files(row)
        album = core.AlbumDir(folder, files)
        cover = tags.cover_statistics(
            album,
            str(self.runtime.cfg.get("output", {}).get("file_name", "cover")),
        )
        representative = sorted(files, key=lambda value: str(value).casefold())[0]
        representative_stat = representative.stat()
        fingerprint = _album_inventory_fingerprint(
            album, not str(row["musicbrainz_albumid"] or "").strip()
        )
        now = tags.utc_now()
        with closing(self.runtime.database.connect()) as db, db:
            db.execute(
                "UPDATE albums SET cover_found=?,cover_path=?,cover_width=?,cover_height=?,artwork_jpeg=?,artwork_png=?,artwork_webp=?,artwork_other=?,representative_file=?,representative_size=?,representative_mtime_ns=?,track_count=?,inventory_fingerprint=?,updated_at=?,status=CASE WHEN CAST(? AS TEXT) IS NOT NULL THEN 'processed' ELSE status END,processed_at=CASE WHEN CAST(? AS TEXT) IS NOT NULL THEN ? ELSE processed_at END,selected_source=coalesce(?,selected_source) WHERE album_key=?",
                (
                    int(cover["cover_found"]),
                    str(cover["cover_path"]),
                    cover["cover_width"],
                    cover["cover_height"],
                    int(cover["artwork"]["JPEG"]),
                    int(cover["artwork"]["PNG"]),
                    int(cover["artwork"]["WEBP"]),
                    int(cover["artwork"]["OTHER"]),
                    str(representative),
                    int(representative_stat.st_size),
                    int(representative_stat.st_mtime_ns),
                    len(files),
                    fingerprint,
                    now,
                    source,
                    source,
                    now,
                    source,
                    album_id,
                ),
            )
            for path in files:
                stat = path.stat()
                db.execute(
                    "UPDATE tracks SET file_size=?,file_mtime_ns=?,updated_at=? "
                    "WHERE album_key=? AND path=?",
                    (
                        int(stat.st_size),
                        int(stat.st_mtime_ns),
                        now,
                        album_id,
                        str(path),
                    ),
                )
            media.update_artist_aggregate(db, row["artist_key"])

    def preview(self, payload):
        source = self.catalog.image_source(str(payload.get("image", "")))
        kind = str(payload.get("kind", "cover"))
        output = self.runtime.cfg.get("output", {})
        ranges = self.runtime.cfg.get("range", {})
        upscale_requested = bool(payload.get("upscale"))
        upscale_enabled = bool(output.get("upscale_below_ideal", False))
        ideal = int(ranges.get("ideal", 1800))
        max_percent = int(output.get("upscale_max_percent", 200))
        source_format = ""
        try:
            with Image.open(source if isinstance(source, Path) else BytesIO(source)) as opened:
                source_format = str(opened.format or "").casefold().replace("jpg", "jpeg")
                source_width, source_height = opened.size
                animated_webp = source_format == "webp" and int(getattr(opened, "n_frames", 1)) > 1
        except (OSError, ValueError):
            source_width = source_height = 0
            animated_webp = False
        adjustments = payload.get("adjustments") or {}
        package_adjustments = payload.get("package_adjustments") or {}
        fit = payload.get("fit") or {}
        edited = any(float(value or 0) for value in adjustments.values() if isinstance(value, (int, float))) \
            or any(float(value or 0) for value in package_adjustments.values() if isinstance(value, (int, float))) \
            or any(float(fit.get(key, default) or default) != default for key, default in (("zoom", 1), ("x", 0), ("y", 0)))
        if kind == "cover" and animated_webp and not edited and not upscale_requested:
            body = source.read_bytes() if isinstance(source, Path) else source
            return {
                "image": self.catalog.image(body), "width": source_width,
                "height": source_height, "bytes": len(body), "kind": kind,
                "format": "webp", "upscaled": False,
                "reason": "Original animated WebP preserved",
            }
        image = imaging.render(
            source,
            values=payload.get("adjustments"),
            package_values=payload.get("package_adjustments"),
            fit=payload.get("fit"),
            kind=kind,
        )
        upscaled = False
        if upscale_requested and upscale_enabled and kind != "cover":
            image, upscaled, _ = imaging.upscale_to_policy(
                image, ideal, max_percent
            )
        target = "png"
        body = imaging.encode(image)
        if kind == "cover":
            configured = core.formats(self.runtime.cfg)
            target = source_format if source_format in configured else configured[0]
            directory = self.runtime.temporary_cache / "prepared-previews"
            directory.mkdir(parents=True, exist_ok=True)
            path = directory / (uuid4().hex + ".png")
            path.write_bytes(body)
            try:
                candidate = core.Candidate(
                    core.Ref("local", "preview", ""),
                    path,
                    image.width,
                    image.height,
                    "png",
                    0,
                )
                body, info = core.prepare_final(
                    candidate, self.runtime.cfg, target, allow_out_of_range=True
                )
                width, height = info["width"], info["height"]
                upscaled = bool(info.get("upscaled"))
            finally:
                path.unlink(missing_ok=True)
        else:
            width, height = image.size
        if upscaled:
            reason = f"Prepared toward {ideal}px Ideal within the {max_percent}% ceiling; original artwork is unchanged until written"
        elif upscale_requested and not upscale_enabled:
            reason = "Actual source resolution preserved because Upscale Below Ideal is off in Settings"
        elif upscale_requested:
            reason = "Actual source resolution already meets Ideal or the configured ceiling prevents enlargement"
        else:
            reason = "Prepared preview; original artwork is unchanged until written"
        return {
            "image": self.catalog.image(body),
            "width": width,
            "height": height,
            "bytes": len(body),
            "kind": kind,
            "format": target,
            "upscaled": upscaled,
            "reason": reason,
        }

    def _atomic_artwork(self, destination, body):
        with Image.open(BytesIO(body)) as opened:
            opened.verify()
        destination.parent.mkdir(parents=True, exist_ok=True)
        descriptor, temporary = tempfile.mkstemp(
            prefix=".splined-artwork-",
            suffix=destination.suffix,
            dir=destination.parent,
        )
        try:
            with os.fdopen(descriptor, "wb") as handle:
                handle.write(body)
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(temporary, destination)
        finally:
            Path(temporary).unlink(missing_ok=True)

    @serialized
    def write_cover(self, payload):
        self.runtime.require_write()
        row = self.album_row(str(payload.get("album_id", "")))
        if not str(row["musicbrainz_albumid"] or "").strip():
            raise ValueError("COMP artwork must be written to individual tracks")
        rendered = self.preview(dict(payload, kind="cover"))
        source = self.catalog.image_bytes(rendered["image"])
        fmt = rendered["format"]
        body = source
        stem = str(self.runtime.cfg.get("output", {}).get("file_name", "cover"))
        core.validate_filename(stem)
        extension = {"jpeg": ".jpg", "png": ".png", "webp": ".webp"}[fmt]
        destination = Path(row["path"]) / (stem + extension)
        if self.runtime.cfg.get("output", {}).get("preserve_file", True):
            number = 1
            while destination.exists() and destination.read_bytes() != body:
                number += 1
                destination = Path(row["path"]) / (
                    f"{stem}-({number})" + destination.suffix
                )
        self._atomic_artwork(destination, body)
        self.refresh_album(row["album_key"], source="webgui")
        self.audit("write-cover", row["album_key"], {"file": destination.name})
        return {"written": [destination.name], **rendered}

    @serialized
    def write_artist(self, payload):
        self.runtime.require_write()
        artist_id = str(payload.get("artist_id", ""))
        with closing(self.runtime.database.connect()) as db:
            rows = list(
                db.execute(
                    "SELECT albums.path,albums.musicbrainz_albumid,artists.artist_name,artists.musicbrainz_artistid FROM albums JOIN artists ON artists.artist_key=albums.artist_key WHERE albums.artist_key=?",
                    (artist_id,),
                )
            )
        if not rows:
            raise KeyError("Artist not found")
        targets = artist_targets(
            [
                ArtistAlbum(
                    row["artist_name"],
                    Path(row["path"]),
                    row["musicbrainz_albumid"] or "",
                    row["musicbrainz_artistid"] or "",
                )
                for row in rows
                if inside(Path(row["path"]), self.runtime.music)
            ],
            rows[0]["artist_name"],
            self.runtime.artist_art,
        )
        if not targets:
            raise ValueError("Artist has no eligible artwork destination")
        prepared = []
        for component in payload.get("components", []):
            kind = str(component.get("kind", ""))
            if kind not in {"artist", "banner"}:
                raise ValueError("Choose an artist image or banner")
            render = self.preview(dict(component, kind=kind))
            prepared.append((kind, self.catalog.image_bytes(render["image"])))
        if not prepared:
            raise ValueError("Choose artwork before writing")
        written = []
        for kind, body in prepared:
            target = targets[0].for_banner() if kind == "banner" else targets[0]
            path = write_image(
                target,
                body,
                preserve_file=bool(
                    self.runtime.cfg.get("output", {}).get("preserve_file", True)
                ),
            )
            written.append(path.name)
        self.audit("write-artist-art", artist_id, {"files": written})
        self.catalog.metadata_cache.pop(artist_id, None)
        self.catalog.portrait_cache.pop(artist_id, None)
        return {"written": written}

    @serialized
    def embed_album(self, payload, progress=None, cancelled=None):
        self.runtime.require_write()
        row = self.album_row(str(payload.get("album_id", "")))
        if not str(row["musicbrainz_albumid"] or "").strip():
            raise ValueError("COMP albums require individual track processing")
        if payload.get("prepared_file"):
            if (
                payload.get("library_root") != str(self.runtime.music)
                or payload.get("album_path") != row["path"]
            ):
                raise ValueError(
                    "The staged library location has changed; stage this album again"
                )
            path = self.runtime.cache / "staged-artwork" / str(payload["prepared_file"])
            if (
                not inside(path, self.runtime.cache / "staged-artwork")
                or path.name != payload["prepared_file"]
            ):
                raise ValueError("Staged artwork is unavailable")
            source = path.read_bytes()
            if hashlib.sha256(source).hexdigest() != payload.get("sha256"):
                raise ValueError("Staged artwork failed verification")
        else:
            rendered = self.preview(dict(payload, kind="cover"))
            source = self.catalog.image_bytes(rendered["image"])
        with Image.open(BytesIO(source)) as image:
            side = int(
                self.runtime.cfg.get("output", {}).get("embedded_thumbnail_size", 1200)
            )
            image = ImageOps.fit(image, (side, side), method=Image.Resampling.LANCZOS)
            body = imaging.encode(image, "jpeg")
        files = self.album_files(row)
        if not files:
            raise ValueError("Album has no supported tracks")
        self.runtime.cache.mkdir(parents=True, exist_ok=True)
        backup_root = self.runtime.cache / "embedded-backups"
        backup_root.mkdir(exist_ok=True)
        for index, path in enumerate(files, 1):
            if cancelled and cancelled():
                raise core.InteractionCancelled()
            original = embedded_track_artwork_bytes(path)
            if original == body:
                if progress:
                    progress(
                        {"completed": index, "total": len(files), "message": path.name}
                    )
                continue
            backup = backup_root / (
                hashlib.sha256(
                    (str(path) + hashlib.sha256(original or b"").hexdigest()).encode()
                ).hexdigest()
                + ".image"
            )
            if original and not backup.exists():
                backup.write_bytes(original)
            compilation.replace_embedded_artwork(
                path, body, "jpeg", {"width": side, "height": side}
            )
            verified = embedded_track_artwork_bytes(path)
            if (
                not verified
                or hashlib.sha256(verified).digest() != hashlib.sha256(body).digest()
            ):
                raise ValueError(
                    "Embedded artwork verification failed for " + path.name
                )
            if progress:
                progress(
                    {"completed": index, "total": len(files), "message": path.name}
                )
        self.refresh_album(row["album_key"], source="webgui-embed")
        self.refresh_embedding_stats(row)
        self.audit(
            "embed-album", row["album_key"], {"tracks": len(files), "side": side}
        )
        return {"tracks": len(files), "side": side}

    def search_album(self, album_id, release_id=None):
        row = self.album_row(album_id)
        mbid = release_id or str(row["musicbrainz_albumid"] or "")
        if not core.valid_mbid(mbid):
            raise ValueError(
                "Choose a MusicBrainz release before searching album artwork"
            )
        http = core.Http()
        r = self.runtime
        sources = core.resolve_sources(r.cfg, None, None, [])
        release = core.lookup_release(http, r.config_file, r.cfg, mbid)
        refs, warnings = core.discover_all(http, r.config_file, r.cfg, release, sources)
        cache = r.temporary_cache / "web-candidates" / uuid4().hex
        cache.mkdir(parents=True, exist_ok=True)
        found, download_warnings = core.download_candidates(
            http, refs, sources, cache, r.cfg, clean_first=False
        )
        warnings.extend(download_warnings)
        candidates = []
        for candidate in found:
            projected = core.project_candidate(candidate, r.cfg, core.formats(r.cfg)[0])
            candidates.append(
                {
                    "name": candidate.path.name,
                    "source": core.provider_label(candidate.source),
                    "kind": "cover",
                    "image": self.catalog.image(candidate.path),
                    "width": candidate.width,
                    "height": candidate.height,
                    "bytes": candidate.path.stat().st_size,
                    "acceptable": projected["acceptable"],
                    "range_class": projected["range_type"],
                }
            )
        return {
            "candidates": candidates,
            "warnings": [core._redact_log_message(str(item)) for item in warnings],
        }

    @serialized
    def stage_album(self, payload):
        self.runtime.require_write()
        row = self.album_row(str(payload.get("album_id", "")))
        result = self.write_cover(payload)
        body = self.catalog.image_bytes(result["image"])
        directory = self.runtime.cache / "staged-artwork"
        directory.mkdir(parents=True, exist_ok=True)
        name = uuid4().hex + {
            "jpeg": ".jpg",
            "png": ".png",
            "webp": ".webp",
        }[result["format"]]
        self._atomic_artwork(directory / name, body)
        return {
            "album_id": row["album_key"],
            "album_path": row["path"],
            "library_root": str(self.runtime.music),
            "name": row["album_name"],
            "artist": row["artist_name"],
            "prepared_file": name,
            "sha256": hashlib.sha256(body).hexdigest(),
            "tracks": row["track_count"],
        }

    @serialized
    def run_comp(self, payload, adapter):
        self.runtime.require_write()
        r = self.runtime
        row = self.album_row(str(payload.get("album_id", "")))
        adapter.context = {
            "album_id": row["album_key"],
            "album": row["album_name"],
            "artist": row["artist_name"],
        }
        if str(row["musicbrainz_albumid"] or "").strip():
            raise ValueError("This album is not a COMP fallback album")
        records = self.hydrate_tracks(row["album_key"])
        allowed = {item["track_key"]: item["path"] for item in records}
        requested = payload.get("track_keys") or list(allowed)
        if not isinstance(requested, list) or any(
            key not in allowed for key in requested
        ):
            raise ValueError("Choose indexed COMP tracks")
        selected_files = [Path(allowed[key]) for key in requested]
        tracks = [core.read_track(path) for path in selected_files]
        summary = core.Summary()
        cache = r.temporary_cache / "comp-work" / uuid4().hex
        cache.mkdir(parents=True, exist_ok=True)
        scan.run_manual_compilation_album(
            core.AlbumDir(Path(row["path"]), selected_files),
            tracks,
            http=core.Http(),
            config_file=r.config_file,
            cfg=r.cfg,
            sources=core.resolve_sources(r.cfg, None, None, []),
            cache=cache,
            format_order=core.formats(r.cfg),
            summary=summary,
            api_queried=set(),
            unresolved_items=[],
            state={},
            target_track_paths={allowed[key] for key in requested},
        )
        self.refresh_album(row["album_key"])
        self.refresh_embedding_stats(row)
        return asdict(summary)

    def run_scan(self, payload, adapter):
        album_ids = payload.get("album_ids")
        if not album_ids and payload.get("artist_id"):
            with closing(self.runtime.database.connect()) as db:
                album_ids = [
                    row["album_key"]
                    for row in db.execute(
                        "SELECT album_key FROM albums WHERE artist_key=? "
                        "AND trim(coalesce(musicbrainz_albumid,''))<>'' "
                        "AND bypassed=0 AND lower(coalesce(status,''))<>'timeout' "
                        "AND NOT (lower(coalesce(status,''))='processed' AND cover_found<>0 "
                        "AND embedded_tracks>=track_count)",
                        (payload["artist_id"],),
                    )
                ]
        if not isinstance(album_ids, list) or not album_ids:
            raise ValueError("Choose albums to process")
        reviewed = 0
        adapter.progress({"completed": 0, "total": len(album_ids), "message": "Ready"})
        for position, album_id in enumerate(album_ids, 1):
            if adapter.cancelled():
                raise core.InteractionCancelled()
            row = self.album_row(str(album_id))
            adapter.context = {
                "album_id": row["album_key"],
                "album": row["album_name"],
                "artist": row["artist_name"],
            }
            result = self.search_album(row["album_key"])
            candidates = result["candidates"]
            if not candidates:
                adapter.progress({"completed": position, "total": len(album_ids), "message": row["album_name"]})
                continue
            items = [dict(item, index=n) for n, item in enumerate(candidates, 1)]
            adapter.emit("candidates", {"items": items})
            answer = adapter.read(
                "Choose album artwork",
                {
                    "kind": "normal-picker",
                    "album": row["album_name"],
                    "artist": row["artist_name"],
                    "options": items,
                },
            )
            if answer == "b":
                self.bypass(row["album_key"], True)
                adapter.progress({"completed": position, "total": len(album_ids), "message": row["album_name"]})
                continue
            if answer in {"k", "__batch_return__"}:
                adapter.progress({"completed": position, "total": len(album_ids), "message": row["album_name"]})
                continue
            if answer == "__cancel__":
                raise core.InteractionCancelled()
            if answer == "s":
                eligible = [item for item in candidates if item.get("acceptable")]
                if not eligible:
                    raise ValueError("No candidate meets the configured policy")
                candidate = eligible[0]
            elif answer.isdigit() and 1 <= int(answer) <= len(candidates):
                candidate = candidates[int(answer) - 1]
            else:
                adapter.progress({"completed": position, "total": len(album_ids), "message": row["album_name"]})
                continue
            if self.runtime.cfg.get("mode") == "write":
                self.write_cover(
                    {"album_id": row["album_key"], "image": candidate["image"]}
                )
            reviewed += 1
            adapter.progress(
                {
                    "completed": position,
                    "total": len(album_ids),
                    "message": row["album_name"],
                }
            )
        return {
            "reviewed": reviewed,
            "total": len(album_ids),
            "mode": self.runtime.cfg.get("mode", "read"),
        }

    def musicbrainz(self, album_id, artist=None, title=None):
        row = self.album_row(album_id)
        r = self.runtime
        return core.decorate_musicbrainz_results(
            core.musicbrainz_search_releases(
                core.Http(),
                r.config_file,
                r.cfg,
                artist or row["artist_name"],
                title or row["album_name"],
            ),
            query_artist=artist or row["artist_name"],
            query_album=title or row["album_name"],
        )

    @serialized
    def bypass(self, album_id, enabled):
        row = self.album_row(album_id)
        with closing(self.runtime.database.connect()) as db, db:
            db.execute(
                "UPDATE albums SET bypassed=?,status=CASE WHEN ?=1 THEN 'bypassed' WHEN processed_at IS NOT NULL THEN 'processed' ELSE 'unprocessed' END,updated_at=? WHERE album_key=?",
                (int(enabled), int(enabled), tags.utc_now(), album_id),
            )
            media.update_artist_aggregate(db, row["artist_key"])
        self.audit("bypass" if enabled else "unbypass", album_id)
        return {"bypassed": bool(enabled)}
