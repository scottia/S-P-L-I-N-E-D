"""Web operations use indexed identities, configured paths, and real writes."""

from __future__ import annotations

from contextlib import closing, contextmanager
import concurrent.futures
import logging
from dataclasses import asdict
from functools import wraps
from io import BytesIO
from pathlib import Path
from uuid import uuid4
import hashlib
import json
import os
import shutil
import tempfile
import threading
import time

from mutagen import File as MutagenFile
from mutagen.id3 import ID3, TXXX, UFID
from mutagen.mp4 import MP4
from PIL import Image, ImageOps
from psycopg import sql
from psycopg.types.json import Jsonb

import splined as core
import splined_scan as scan
import splined_strict_source_policy as strict_policy
import splined_media_database as media
import splined_media_tags as tags
import splined_compilation_authority_policy as compilation
from splined_artist_art import ArtistAlbum, artist_targets, write_image

from webgui.catalog import (
    Catalog,
    AUDIO_EXTENSIONS,
    inside,
    embedded_track_artwork_bytes,
    relative_coordination_path,
)
from webgui.catalog_authority import (
    index_update_columns,
    windows_managed,
    windows_managed_keys,
    windows_managed_track_keys,
)
from webgui.runtime import Runtime
from webgui.tempfiles import discard_committed_conversion, mark_conversion_backup
from webgui import imaging
from webgui import index_coordinator as indexer
from webgui import index_batch_policy
from webgui.audit import (
    application_revision,
    normalize_severity,
    sanitize,
    sanitize_text,
    utc_now,
)
from webgui.coordination import (
    current_rows,
    current_track_rows,
    new_event,
    new_track_event,
    normalized_album_key,
    normalized_coordination_relative,
    record_event,
    record_track_event,
)


LOGGER = logging.getLogger("uvicorn.error")

NAME_INVENTORY_PREFIX = "names-v1:"
TRACK_INVENTORY_PREFIX = "tracks-v1:"
# Increment when a corrected tag projection requires one deliberate Full
# Refresh to reopen Windows-managed tracks whose size/mtime did not change.
# Version 3 republishes positional Artist-credit MBIDs without the historical
# lexical sort and decodes ID3v2.3 slash-packed multi-value TPE1 frames using
# their ordered MBID cardinality. Both corrections preserve the Windows
# projection instead of manufacturing combined Featured Artist identities.
MANAGED_TRACK_CREDIT_REFRESH_VERSION = 3


@contextmanager
def _temporary_workspace(runtime, kind: str):
    """Use ownership-aware workspaces, retaining fixture compatibility."""
    manager = getattr(runtime, "temporary_files", None)
    if manager is not None:
        with manager.workspace(kind) as path:
            yield path
        return
    parent = Path(runtime.temporary_cache) / kind
    parent.mkdir(parents=True, exist_ok=True)
    path = Path(tempfile.mkdtemp(prefix="fixture-", dir=parent))
    try:
        yield path
    finally:
        shutil.rmtree(path, ignore_errors=True)


def _track_has_preferred_embedded_artwork(path: Path, preferred_side: int) -> bool:
    """Return physical thumbnail compliance while preserving track read failures."""
    artwork = embedded_track_artwork_bytes(path, strict=True)
    if not artwork:
        return False
    try:
        with Image.open(BytesIO(artwork)) as opened:
            width, height = opened.size
    except (OSError, ValueError):
        # The track was readable, so corrupt/unreadable artwork is confirmed
        # attention evidence rather than a transient media access failure.
        return False
    return width == height == preferred_side


def _album_inventory_fingerprint(album, fallback=False):
    del fallback
    # Per-track Artist credits now participate in every identified Album's
    # Artist projection. Include size/mtime for all tracks so changing a guest
    # credit on a non-representative track is visible to incremental refresh.
    metadata = _album_audio_metadata(album)
    return TRACK_INVENTORY_PREFIX + core._fingerprint_from_metadata(
        [
            (path, *metadata.get(str(path), (0, 0)))
            for path in album.audio_files
        ]
    )


def _album_audio_metadata(album):
    metadata = dict(getattr(album, "audio_metadata", {}) or {})
    for path in album.audio_files:
        key = str(path)
        if key in metadata:
            continue
        try:
            stat = path.stat()
            metadata[key] = (int(stat.st_size), int(stat.st_mtime_ns))
        except OSError:
            metadata[key] = (0, 0)
    album.audio_metadata = metadata
    return metadata


def _album_manifest_facts(album):
    """Return metadata-only manifest facts without opening tags or images."""
    total_size = 0
    newest_mtime_ns = 0
    track_parts = []
    metadata = _album_audio_metadata(album)
    for path in sorted(album.audio_files, key=lambda value: str(value).casefold()):
        size, modified = metadata.get(str(path), (0, 0))
        total_size += size
        newest_mtime_ns = max(newest_mtime_ns, modified)
        track_parts.append(f"{path.name.casefold()}:{size}:{modified}")
    art_parts = []
    for path in sorted(album.local_art_files, key=lambda value: str(value).casefold()):
        try:
            stat = path.stat()
            art_parts.append(
                f"{path.name.casefold()}:{int(stat.st_size)}:{int(stat.st_mtime_ns)}"
            )
        except OSError:
            art_parts.append(f"{path.name.casefold()}:0:0")
    return {
        "track_count": len(album.audio_files),
        "total_size": total_size,
        "newest_mtime_ns": newest_mtime_ns,
        "inventory_signature": hashlib.sha256(
            "\n".join(track_parts).encode("utf-8")
        ).hexdigest(),
        "artwork_signature": hashlib.sha256(
            "\n".join(art_parts).encode("utf-8")
        ).hexdigest(),
    }


def _indexed_album_unchanged(prior, album, fingerprint):
    if int(prior["track_count"] or 0) != len(album.audio_files):
        return False
    representative = sorted(
        album.audio_files, key=lambda value: str(value).casefold()
    )[0]
    size, mtime = _album_audio_metadata(album).get(str(representative), (0, 0))
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
        configured_artist_separator = str(
            runtime.cfg.get("scan", {}).get(
                "artist_separator_regex", core.DEFAULT_ARTIST_SEPARATOR_REGEX
            )
        )
        active_artist_separator = configured_artist_separator
        try:
            with closing(runtime.database.connect()) as db:
                separator_state = db.execute(
                    "SELECT details_json FROM db_maintenance_state "
                    "WHERE action_name=?",
                    ("artist-credit-separator",),
                ).fetchone()
                album_count = int(db.execute("SELECT count(*) FROM albums").fetchone()[0])
            if separator_state is not None:
                indexed = json.loads(str(separator_state["details_json"] or "{}"))
                active_artist_separator = str(
                    indexed.get("artist_separator_regex", "")
                    or core.DEFAULT_ARTIST_SEPARATOR_REGEX
                )
            elif album_count and configured_artist_separator != core.DEFAULT_ARTIST_SEPARATOR_REGEX:
                # A saved rule becomes active only after the reset/full-refresh
                # contract records it. Until then, preserve the rule that built
                # a pre-feature catalog instead of reinterpreting stored rows.
                active_artist_separator = core.DEFAULT_ARTIST_SEPARATOR_REGEX
        except (KeyError, TypeError, ValueError):
            active_artist_separator = configured_artist_separator
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
            preview_cache=runtime.temporary_files.session_workspace("web-images"),
            artist_separator_regex=active_artist_separator,
        )
        self.catalog.cache = runtime.cache
        self.catalog.artist_sources = tuple(core.resolve_artist_sources(runtime.cfg))
        self.lock = threading.RLock()
        self._audit_events_written = 0
        self.audit_session_id = uuid4().hex
        self.application_revision = application_revision()
        self._audit_lock = threading.RLock()
        self._audit_fallback_counter = 0
        log_path = Path(runtime.log_path) if runtime.log_path else runtime.cache / "runtime.log"
        self._audit_fallback_path = log_path.with_name("web-audit-fallback.jsonl")
        self._coordination_lock = threading.RLock()
        self._coordination_outbox_path = runtime.cache / "album-write-coordination-outbox.json"
        self._track_coordination_outbox_path = (
            runtime.cache / "comp-track-write-coordination-outbox.json"
        )
        # MusicBrainz discovery belongs to the service session, not to one
        # queued COMP job.  Keeping this state here lets a second review of the
        # same Recording/Artist identity reuse the validated release metadata
        # while artwork previews continue to load independently.
        self._compilation_discovery_state: dict[str, object] = {}

    def _coordination_guard(self):
        """Lazily restore coordination helpers for legacy lightweight fixtures."""
        lock = getattr(self, "_coordination_lock", None)
        if lock is None:
            lock = threading.RLock()
            self._coordination_lock = lock
        if not hasattr(self, "_coordination_outbox_path"):
            self._coordination_outbox_path = (
                self.runtime.cache / "album-write-coordination-outbox.json"
            )
        return lock

    def _coordination_outbox(self) -> list[dict[str, object]]:
        try:
            value = json.loads(self._coordination_outbox_path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            return []
        except (OSError, ValueError, TypeError):
            LOGGER.warning("Album coordination outbox could not be read; preserving it for review")
            return []
        return [item for item in value if isinstance(item, dict)] if isinstance(value, list) else []

    def _save_coordination_outbox(self, events: list[dict[str, object]]):
        path = self._coordination_outbox_path
        if not events:
            path.unlink(missing_ok=True)
            return
        path.parent.mkdir(parents=True, exist_ok=True)
        descriptor, temporary = tempfile.mkstemp(
            prefix=".album-write-coordination-", suffix=".json", dir=path.parent
        )
        try:
            with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
                json.dump(events, handle, ensure_ascii=False, separators=(",", ":"))
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(temporary, path)
        finally:
            Path(temporary).unlink(missing_ok=True)

    def flush_album_write_coordination(self) -> dict[str, int]:
        """Replay durable notifications after a transient PostgreSQL failure."""
        with self._coordination_guard():
            pending = self._coordination_outbox()
            remaining = []
            applied = 0
            for event in pending:
                try:
                    record_event(self.runtime.database, event)
                    applied += 1
                except Exception:
                    remaining.append(event)
            self._save_coordination_outbox(remaining)
            return {"applied": applied, "pending": len(remaining)}

    def record_album_write(self, album, operation: str) -> dict[str, object]:
        """Record only a verified successful Album write; never block later work."""
        event = new_event(album, self.runtime.music, "webgui", operation)
        with self._coordination_guard():
            try:
                self.flush_album_write_coordination()
                durable = record_event(self.runtime.database, event)
                return {"durable": True, **durable}
            except Exception as error:
                pending = self._coordination_outbox()
                if not any(item.get("event_id") == event["event_id"] for item in pending):
                    pending.append(event)
                self._save_coordination_outbox(pending)
                LOGGER.warning(
                    "Album artwork write notification queued for retry: album=%s operation=%s error=%s",
                    event["album_key"], operation, sanitize_text(str(error)),
                )
                return {"durable": False, **event}

    def album_write_coordination(self) -> list[dict[str, object]]:
        self.flush_album_write_coordination()
        return self.catalog.album_write_coordination_rows(
            current_rows(self.runtime.database)
        )

    def _track_coordination_outbox(self) -> list[dict[str, object]]:
        path = getattr(
            self,
            "_track_coordination_outbox_path",
            self.runtime.cache / "comp-track-write-coordination-outbox.json",
        )
        self._track_coordination_outbox_path = path
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            return []
        except (OSError, ValueError, TypeError):
            LOGGER.warning(
                "COMP track coordination outbox could not be read; preserving it for review"
            )
            return []
        return [item for item in value if isinstance(item, dict)] if isinstance(value, list) else []

    def _save_track_coordination_outbox(self, events: list[dict[str, object]]):
        path = getattr(
            self,
            "_track_coordination_outbox_path",
            self.runtime.cache / "comp-track-write-coordination-outbox.json",
        )
        self._track_coordination_outbox_path = path
        if not events:
            path.unlink(missing_ok=True)
            return
        path.parent.mkdir(parents=True, exist_ok=True)
        descriptor, temporary = tempfile.mkstemp(
            prefix=".comp-track-write-coordination-", suffix=".json", dir=path.parent
        )
        try:
            with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
                json.dump(events, handle, ensure_ascii=False, separators=(",", ":"))
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(temporary, path)
        finally:
            Path(temporary).unlink(missing_ok=True)

    def flush_comp_track_write_coordination(self) -> dict[str, int]:
        with self._coordination_guard():
            pending = self._track_coordination_outbox()
            remaining = []
            applied = 0
            for event in pending:
                try:
                    record_track_event(self.runtime.database, event)
                    applied += 1
                except Exception:
                    remaining.append(event)
            self._save_track_coordination_outbox(remaining)
            return {"applied": applied, "pending": len(remaining)}

    def record_comp_track_write(
        self, album, track, operation: str, artwork_sha256: str
    ) -> dict[str, object]:
        event = new_track_event(
            album,
            track,
            self.runtime.music,
            "webgui",
            operation,
            artwork_sha256,
        )
        with self._coordination_guard():
            try:
                self.flush_comp_track_write_coordination()
                durable = record_track_event(self.runtime.database, event)
                return {"durable": True, **durable}
            except Exception as error:
                pending = self._track_coordination_outbox()
                if not any(item.get("event_id") == event["event_id"] for item in pending):
                    pending.append(event)
                self._save_track_coordination_outbox(pending)
                LOGGER.warning(
                    "COMP track write notification queued for retry: track=%s operation=%s error=%s",
                    event["relative_track_path"],
                    operation,
                    sanitize_text(str(error)),
                )
                return {"durable": False, **event}

    def comp_track_write_coordination(self) -> list[dict[str, object]]:
        self.flush_comp_track_write_coordination()
        rows = current_track_rows(self.runtime.database)
        for row in rows:
            if str(row.get("writer") or "").casefold() == "windows":
                row["readable"] = self._persist_opposing_comp_track(row)
        return rows

    def _persist_opposing_comp_track(self, event: dict[str, object]) -> bool:
        """Project one verified Windows track write into the exact PG ledger row."""
        relative = normalized_coordination_relative(event.get("relative_track_path"))
        album_key = normalized_album_key(event.get("album_key"))
        if not relative or not album_key:
            return False
        with closing(self.runtime.database.connect()) as db, db:
            candidates = db.execute(
                "SELECT tracks.album_key,tracks.path,tracks.musicbrainz_recordingid,"
                "tracks.musicbrainz_artistid,albums.path AS album_path "
                "FROM tracks JOIN albums ON albums.album_key=tracks.album_key "
                "WHERE lower(albums.album_key)=lower(?)",
                (album_key,),
            ).fetchall()
            matches = [
                track
                for track in candidates
                if normalized_coordination_relative(
                    relative_coordination_path(Path(track["path"]), self.runtime.music)
                )
                == relative
            ]
            # Never guess through a legacy collision. The event must identify
            # exactly one indexed physical path inside its selected Album.
            if len(matches) != 1:
                return False
            track = matches[0]
            track_path = Path(track["path"])
            if not inside(track_path, self.runtime.music) or not track_path.is_file():
                return False
            try:
                artwork = embedded_track_artwork_bytes(track_path, strict=True)
            except (OSError, ValueError):
                return False
            expected = str(event.get("artwork_sha256") or "").strip().casefold()
            if not artwork or hashlib.sha256(artwork).hexdigest() != expected:
                return False
            now = tags.utc_now()
            db.execute(
                "INSERT INTO compilation_track_artwork"
                "(track_path,recording_mbid,artist_mbid,source_kind,source_locator,"
                "release_mbid,artwork_sha256,outcome,applied_at,splined_version) "
                "VALUES(?,?,?,?,?,NULL,?,'embedded-existing',?,?) "
                "ON CONFLICT(track_path) DO UPDATE SET "
                "recording_mbid=excluded.recording_mbid,artist_mbid=excluded.artist_mbid,"
                "source_kind=excluded.source_kind,source_locator=excluded.source_locator,"
                "artwork_sha256=excluded.artwork_sha256,outcome=excluded.outcome,"
                "applied_at=excluded.applied_at,splined_version=excluded.splined_version",
                (
                    str(track["path"]),
                    str(track["musicbrainz_recordingid"] or ""),
                    str(track["musicbrainz_artistid"] or ""),
                    "coordination-windows",
                    "comp-track-write-coordination",
                    expected,
                    now,
                    core.VERSION,
                ),
            )
            total = int(
                db.execute(
                    "SELECT count(*) FROM tracks WHERE album_key=?",
                    (track["album_key"],),
                ).fetchone()[0]
            )
            completed = int(
                db.execute(
                    "SELECT count(*) FROM tracks JOIN compilation_track_artwork "
                    "ON lower(compilation_track_artwork.track_path)=lower(tracks.path) "
                    "WHERE tracks.album_key=? AND compilation_track_artwork.outcome "
                    "IN ('embedded-replaced','embedded-existing')",
                    (track["album_key"],),
                ).fetchone()[0]
            )
            db.execute(
                "INSERT INTO compilation_album_progress"
                "(album_path,total_tracks,completed_tracks,status,updated_at,splined_version) "
                "VALUES(?,?,?,?,?,?) ON CONFLICT(album_path) DO UPDATE SET "
                "total_tracks=excluded.total_tracks,completed_tracks=excluded.completed_tracks,"
                "status=excluded.status,updated_at=excluded.updated_at,"
                "splined_version=excluded.splined_version",
                (
                    str(track["album_path"]),
                    total,
                    completed,
                    "complete" if total and completed >= total else "incomplete",
                    now,
                    core.VERSION,
                ),
            )
        return True

    def start_audit_session(self):
        return self.audit(
            "diagnostic-session-start",
            "WebGUI",
            {
                "application_version": core.VERSION,
                "application_revision": self.application_revision,
            },
            stage="startup",
            request_status="started",
        )

    def reload_settings(self):
        app = self.catalog
        r = self.runtime
        app.music = r.music
        app.artist_art = r.artist_art
        app.credentials = r.credentials
        app.cache = r.cache
        app.temporary_cache = r.temporary_cache
        app.preview_cache = r.temporary_files.session_workspace("web-images")
        app.comp_thumbnail_side = int(
            r.cfg.get("output", {}).get("embedded_thumbnail_size", 1200)
        )
        app.allow_comp_repairs = r.cfg.get("mode") == "write"
        app.cover_stem = str(r.cfg.get("output", {}).get("file_name", "cover"))
        app.artist_sources = tuple(core.resolve_artist_sources(r.cfg))
        app.metadata_cache.clear()
        app.appearance_cache.clear()
        app.portrait_cache.clear()

    def _audit_fallback_files(self):
        base = self._audit_fallback_path
        return [base.with_name(base.name + f".{number}") for number in range(4, 0, -1)] + [base]

    def _append_audit_fallback(self, event, insertion_error):
        self._audit_fallback_counter += 1
        fallback = {
            **event,
            "fallback_id": (
                f"{self.audit_session_id}:{self._audit_fallback_counter}:"
                f"{uuid4().hex}"
            ),
            "insertion_error": sanitize_text(insertion_error, limit=1000),
        }
        path = self._audit_fallback_path
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            if path.is_file() and path.stat().st_size >= 1024 * 1024:
                oldest = path.with_name(path.name + ".4")
                oldest.unlink(missing_ok=True)
                for number in range(3, 0, -1):
                    source = path.with_name(path.name + f".{number}")
                    if source.is_file():
                        os.replace(source, path.with_name(path.name + f".{number + 1}"))
                os.replace(path, path.with_name(path.name + ".1"))
            with path.open("a", encoding="utf-8", newline="\n") as handle:
                handle.write(json.dumps(fallback, ensure_ascii=False, sort_keys=True) + "\n")
                handle.flush()
                os.fsync(handle.fileno())
        except OSError as fallback_error:
            core.runtime_log(
                "critical",
                "webgui.audit fallback unavailable "
                + core._redact_log_message(sanitize_text(fallback_error)),
            )
        return -self._audit_fallback_counter

    def _load_audit_fallback(self):
        events = []
        files = []
        for path in self._audit_fallback_files():
            if not path.is_file():
                continue
            files.append(path)
            try:
                with path.open("r", encoding="utf-8") as handle:
                    for line in handle:
                        try:
                            value = json.loads(line)
                        except (TypeError, ValueError):
                            continue
                        if isinstance(value, dict) and value.get("fallback_id"):
                            events.append(sanitize(value))
            except OSError:
                continue
        return events, files

    @staticmethod
    def _insert_audit_event(raw, event, *, recovered=False):
        raw.execute(
            "INSERT INTO web_audit_sessions"
            "(session_id,application_version,application_revision,started_at) "
            "VALUES(%s,%s,%s,%s) ON CONFLICT(session_id) DO NOTHING",
            (
                event["session_id"],
                event["application_version"],
                event["application_revision"],
                event["created_at"],
            ),
        )
        return raw.execute(
            "INSERT INTO web_audit"
            "(action,subject,details,severity,action_id,trace_id,job_id,scope,stage,"
            "request_status,elapsed_ms,state_from,state_to,error,created_at,session_id,"
            "application_version,application_revision,recovered_from_fallback,fallback_id) "
            "VALUES(%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s) "
            "ON CONFLICT DO NOTHING RETURNING id",
            (
                event["action"],
                event["subject"],
                Jsonb(event["details"]),
                event["severity"],
                event["action_id"],
                event["trace_id"],
                event["job_id"],
                Jsonb(event["scope"]),
                event["stage"],
                event["request_status"],
                event["elapsed_ms"],
                event["state_from"],
                event["state_to"],
                event["error"],
                event["created_at"],
                event["session_id"],
                event["application_version"],
                event["application_revision"],
                recovered,
                str(event.get("fallback_id") or ""),
            ),
        ).fetchone()

    def audit(
        self,
        action,
        subject="",
        details=None,
        *,
        severity="info",
        action_id="",
        trace_id="",
        job_id="",
        scope=None,
        stage="",
        request_status="",
        elapsed_ms=None,
        state_from="",
        state_to="",
        error="",
    ):
        """Record one ordered, structured event using the existing audit authority."""
        action = sanitize_text(action, limit=120)
        subject = sanitize_text(subject, limit=500)
        details = sanitize(details or {})
        derived_scope = {
            key: details[key]
            for key in ("artist_id", "album_id", "track_key")
            if isinstance(details, dict) and details.get(key)
        }
        clean_scope = sanitize(scope or derived_scope)
        clean_error = sanitize_text(error)
        event = {
            "action": action,
            "subject": subject,
            "details": details,
            "severity": normalize_severity(severity),
            "action_id": sanitize_text(action_id, limit=100),
            "trace_id": sanitize_text(trace_id, limit=100),
            "job_id": sanitize_text(job_id, limit=100),
            "scope": clean_scope,
            "stage": sanitize_text(stage, limit=120),
            "request_status": sanitize_text(request_status, limit=120),
            "elapsed_ms": float(elapsed_ms) if elapsed_ms is not None else None,
            "state_from": sanitize_text(state_from, limit=40),
            "state_to": sanitize_text(state_to, limit=40),
            "error": clean_error,
            "created_at": utc_now(),
            "session_id": self.audit_session_id,
            "application_version": core.VERSION,
            "application_revision": self.application_revision,
            "fallback_id": "",
        }
        core.runtime_log(
            normalize_severity(severity),
            f"webgui.audit action={action} subject={core._redact_log_message(subject)}",
        )
        with self._audit_lock:
            pending, fallback_files = self._load_audit_fallback()
            try:
                with closing(self.runtime.database.connect()) as db, db:
                    for fallback in pending:
                        self._insert_audit_event(db.raw, fallback, recovered=True)
                    row = self._insert_audit_event(db.raw, event)
                    self._audit_events_written += 1
                    if self._audit_events_written % 256 == 0:
                        retention = max(
                            1,
                            min(
                                int(self.runtime.cfg.get("logging", {}).get("retention_days", 14)),
                                365,
                            ),
                        )
                        db.raw.execute(
                            "DELETE FROM web_audit WHERE created_at < now() - (%s * interval '1 day')",
                            (retention,),
                        )
                        db.raw.execute(
                            "DELETE FROM web_audit WHERE id IN "
                            "(SELECT id FROM web_audit ORDER BY id DESC OFFSET 50000)"
                        )
            except Exception as insertion_error:
                core.runtime_log(
                    "error",
                    "webgui.audit postgresql insertion failed; event retained locally "
                    + core._redact_log_message(sanitize_text(insertion_error)),
                )
                return self._append_audit_fallback(event, insertion_error)
            for path in fallback_files:
                try:
                    path.unlink(missing_ok=True)
                except OSError as cleanup_error:
                    # Recovery is idempotent through fallback_id. A cleanup
                    # failure must not misreport the already committed current
                    # event as lost or append a duplicate fallback record.
                    core.runtime_log(
                        "warning",
                        "webgui.audit recovered fallback cleanup deferred "
                        + core._redact_log_message(sanitize_text(cleanup_error)),
                    )
            return int(row["id"] if isinstance(row, dict) else row[0])

    def diagnostics(
        self,
        limit=250,
        *,
        severity="",
        action="",
        correlation="",
        since="",
        until="",
        maximum=500,
    ):
        """Return bounded, credential-free events in chronological order."""
        limit = max(1, min(int(limit), max(1, min(int(maximum), 50001))))
        clauses = []
        values = []
        if severity:
            clauses.append("severity=?")
            values.append(normalize_severity(severity))
        if action:
            # "Action contains" searches the readable event, not only its
            # action column. ILIKE keeps all text matching case-insensitive.
            clauses.append(
                "(action ILIKE ? OR subject ILIKE ? OR CAST(details AS TEXT) ILIKE ? "
                "OR error ILIKE ? OR action_id ILIKE ? OR trace_id ILIKE ? OR job_id ILIKE ?)"
            )
            value = "%" + sanitize_text(action, limit=100) + "%"
            values.extend((value, value, value, value, value, value, value))
        if correlation:
            clauses.append("(trace_id=? OR job_id=? OR action_id=?)")
            value = sanitize_text(correlation, limit=100)
            values.extend((value, value, value))
        if since:
            clauses.append("created_at>=?")
            values.append(sanitize_text(since, limit=80))
        if until:
            clauses.append("created_at<=?")
            values.append(sanitize_text(until, limit=80))
        where = " WHERE " + " AND ".join(clauses) if clauses else ""
        with closing(self.runtime.database.connect()) as db:
            rows = db.execute(
                "SELECT * FROM (SELECT id,action,subject,details,severity,action_id,"
                "trace_id,job_id,scope,stage,request_status,elapsed_ms,state_from,state_to,"
                "error,created_at,session_id,application_version,application_revision,"
                f"recovered_from_fallback FROM web_audit{where} ORDER BY id DESC LIMIT ?) recent "
                "ORDER BY id",
                (*values, limit),
            ).fetchall()
        return [
            {
                "sequence": int(row["id"]),
                "action": row["action"],
                "subject": row["subject"],
                "details": row["details"],
                "severity": row["severity"],
                "action_id": row["action_id"],
                "trace_id": row["trace_id"],
                "job_id": row["job_id"],
                "scope": row["scope"],
                "stage": row["stage"],
                "request_status": row["request_status"],
                "elapsed_ms": row["elapsed_ms"],
                "state_from": row["state_from"],
                "state_to": row["state_to"],
                "error": row["error"],
                "created_at": row["created_at"].isoformat(),
                "session_id": row["session_id"],
                "application_version": row["application_version"],
                "application_revision": row["application_revision"],
                "recovered_from_fallback": bool(row["recovered_from_fallback"]),
            }
            for row in rows
        ]

    def audit_bounds(self):
        """Describe retention and local fallback gaps for truthful exports."""
        with closing(self.runtime.database.connect()) as db:
            row = db.execute(
                "SELECT min(id) AS first_sequence,max(id) AS last_sequence,"
                "count(*) AS retained_records FROM web_audit"
            ).fetchone()
        with self._audit_lock:
            fallback_records = 0
            fallback_bytes = 0
            for path in self._audit_fallback_files():
                if not path.is_file():
                    continue
                try:
                    fallback_bytes += path.stat().st_size
                    with path.open("r", encoding="utf-8") as handle:
                        fallback_records += sum(1 for line in handle if line.strip())
                except OSError:
                    continue
        first = int(row["first_sequence"] or 0)
        return {
            "retained_first_sequence": first or None,
            "retained_last_sequence": int(row["last_sequence"] or 0) or None,
            "retained_records": int(row["retained_records"] or 0),
            "records_pruned_before_retained_window": max(0, first - 1),
            "fallback_records_pending_recovery": fallback_records,
            "fallback_bytes_pending_recovery": fallback_bytes,
        }

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
            "revision": self.application_revision,
            "audit_session_id": self.audit_session_id,
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
        managed = windows_managed(db.raw, table, row.get(key))
        updates = index_update_columns(table, row, managed)
        conflict = (
            sql.SQL("DO UPDATE SET {}").format(
                sql.SQL(",").join(
                    sql.SQL("{}=excluded.{}").format(
                        sql.Identifier(column), sql.Identifier(column)
                    )
                    for column in updates
                )
            )
            if updates
            else sql.SQL("DO NOTHING")
        )
        statement = sql.SQL(
            "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) {}"
        ).format(
            sql.Identifier(table),
            sql.SQL(",").join(map(sql.Identifier, columns)),
            sql.SQL(",").join(sql.Placeholder() for _ in columns),
            sql.Identifier(key),
            conflict,
        )
        db.raw.execute(statement, list(row.values()))

    @staticmethod
    def _update_artist_presentation(db, artist_key, managed):
        """Recalculate display counters without claiming Windows timestamps."""
        if not managed:
            media.update_artist_aggregate(db, artist_key)
            return
        statuses = media.artist_album_statuses(db, artist_key)
        counts = {
            name: statuses.count(name)
            for name in ("unprocessed", "incomplete", "processed", "bypassed", "timeout")
        }
        db.execute(
            "UPDATE artists SET status=?,album_count=?,unprocessed_count=?,"
            "processed_count=?,bypassed_count=?,timeout_count=? WHERE artist_key=?",
            (
                media.aggregate_artist(statuses),
                len(statuses),
                counts["unprocessed"] + counts["incomplete"],
                counts["processed"],
                counts["bypassed"],
                counts["timeout"],
                artist_key,
            ),
        )

    def index_changes(self, cancelled=None, *, refresh=True):
        # Direct service callers retain the established immediate detection
        # contract. The HTTP route passes refresh=False and schedules this
        # same detector on the background worker, so a browser status request
        # never walks the NAS or waits for publication.
        if refresh:
            return self.detect_index_changes(cancelled=cancelled)
        return indexer.change_summary(self.runtime.database)

    def detect_index_changes(self, progress=None, cancelled=None):
        """Refresh the stored change summary without opening tags or artwork."""
        r = self.runtime
        root = r.music
        if not root.is_dir():
            raise ValueError("Set an existing music library in Settings")
        cover_name = str(r.cfg.get("output", {}).get("file_name", "cover"))
        ignored = list(r.cfg.get("library", {}).get("ignored_subs", []))
        discovered = 0
        batch = []
        started = time.perf_counter()

        with closing(r.database.connect()) as db:
            db.raw.execute(
                "CREATE TEMP TABLE IF NOT EXISTS catalog_inventory_probe("
                "album_path TEXT PRIMARY KEY,inventory_signature TEXT NOT NULL,"
                "artwork_signature TEXT NOT NULL,track_count BIGINT NOT NULL,"
                "total_size BIGINT NOT NULL,newest_mtime_ns BIGINT NOT NULL) "
                "ON COMMIT PRESERVE ROWS"
            )
            db.raw.execute("TRUNCATE TABLE catalog_inventory_probe")
            db.commit()

            def flush_probe():
                nonlocal batch
                if not batch:
                    return
                with db.raw.cursor() as cursor:
                    cursor.executemany(
                        "INSERT INTO catalog_inventory_probe("
                        "album_path,inventory_signature,artwork_signature,track_count,"
                        "total_size,newest_mtime_ns) VALUES(%s,%s,%s,%s,%s,%s) "
                        "ON CONFLICT(album_path) DO UPDATE SET "
                        "inventory_signature=excluded.inventory_signature,"
                        "artwork_signature=excluded.artwork_signature,"
                        "track_count=excluded.track_count,"
                        "total_size=excluded.total_size,"
                        "newest_mtime_ns=excluded.newest_mtime_ns",
                        batch,
                    )
                db.commit()
                batch = []

            for album in core.iter_inventory(
                root, ignored, cover_name, cancelled=cancelled
            ):
                if cancelled and cancelled():
                    raise core.InteractionCancelled()
                facts = _album_manifest_facts(album)
                batch.append(
                    (
                        str(album.path),
                        _album_inventory_fingerprint(album),
                        facts["artwork_signature"],
                        facts["track_count"],
                        facts["total_size"],
                        facts["newest_mtime_ns"],
                    )
                )
                discovered += 1
                if len(batch) >= 64:
                    flush_probe()
                if progress and (discovered == 1 or discovered % 64 == 0):
                    progress(
                        {
                            "phase": "inventory",
                            "completed": discovered,
                            "total": 0,
                            "message": album.path.name,
                        }
                    )
            flush_probe()

            counts = db.raw.execute(
                "WITH authoritative AS ("
                " SELECT albums.path FROM albums JOIN catalog_sync_authority authority "
                " ON authority.entity_table='albums' "
                " AND authority.entity_key=albums.album_key "
                " AND authority.authority='windows'),"
                "added AS ("
                " SELECT probe.* FROM catalog_inventory_probe probe "
                " LEFT JOIN catalog_change_manifest saved "
                " ON saved.album_path=probe.album_path "
                " WHERE saved.album_path IS NULL),"
                "removed AS ("
                " SELECT saved.*,authoritative.path IS NOT NULL AS protected "
                " FROM catalog_change_manifest saved "
                " LEFT JOIN catalog_inventory_probe probe "
                " ON probe.album_path=saved.album_path "
                " LEFT JOIN authoritative ON authoritative.path=saved.album_path "
                " WHERE probe.album_path IS NULL),"
                "changed AS ("
                " SELECT probe.album_path,authoritative.path IS NOT NULL AS protected "
                " FROM catalog_inventory_probe probe "
                " JOIN catalog_change_manifest saved "
                " ON saved.album_path=probe.album_path "
                " LEFT JOIN authoritative ON authoritative.path=probe.album_path "
                " WHERE saved.inventory_signature<>probe.inventory_signature OR "
                " saved.artwork_signature<>probe.artwork_signature OR "
                " saved.track_count<>probe.track_count OR "
                " saved.total_size<>probe.total_size OR "
                " saved.newest_mtime_ns<>probe.newest_mtime_ns),"
                "moved AS ("
                " SELECT COALESCE(sum(LEAST(current_count,saved_count)),0) AS n "
                " FROM (SELECT inventory_signature,count(*) AS current_count "
                " FROM added GROUP BY inventory_signature) current_items "
                " JOIN (SELECT inventory_signature,count(*) AS saved_count "
                " FROM removed WHERE NOT protected "
                " GROUP BY inventory_signature) saved_items "
                " USING(inventory_signature)) "
                "SELECT (SELECT count(*) FROM added) AS raw_added,"
                "(SELECT count(*) FROM removed WHERE NOT protected) AS raw_removed,"
                "(SELECT n FROM moved) AS moved,"
                "(SELECT count(*) FROM changed WHERE NOT protected) AS changed,"
                "((SELECT count(*) FROM changed WHERE protected)+"
                "(SELECT count(*) FROM removed WHERE protected)) AS protected"
            ).fetchone()
            moved = int(counts["moved"] or 0)
            added = max(0, int(counts["raw_added"] or 0) - moved)
            removed = max(0, int(counts["raw_removed"] or 0) - moved)
            changed = int(counts["changed"] or 0)
            protected = int(counts["protected"] or 0)
            examples = [
                str(row["album_path"])
                for row in db.raw.execute(
                    "SELECT album_path FROM ("
                    " SELECT probe.album_path,0 AS ordering "
                    " FROM catalog_inventory_probe probe "
                    " LEFT JOIN catalog_change_manifest saved "
                    " ON saved.album_path=probe.album_path "
                    " WHERE saved.album_path IS NULL OR "
                    "saved.inventory_signature<>probe.inventory_signature OR "
                    "saved.artwork_signature<>probe.artwork_signature "
                    " UNION ALL SELECT saved.album_path,1 "
                    " FROM catalog_change_manifest saved "
                    " LEFT JOIN catalog_inventory_probe probe "
                    " ON probe.album_path=saved.album_path "
                    " WHERE probe.album_path IS NULL) changes "
                    "ORDER BY ordering,album_path LIMIT 10"
                )
            ]

        summary = {
            "count": added + changed + removed + moved,
            "added": added,
            "changed": changed,
            "removed": removed,
            "moved": moved,
            "protected": protected,
            "examples": examples,
            "scanned_albums": discovered,
            "scan_seconds": round(time.perf_counter() - started, 3),
        }
        indexer.update_change_summary(r.database, summary)
        return summary

    @serialized
    def refresh_index(self, progress=None, cancelled=None, mode="full"):
        r = self.runtime
        root = r.music
        if not root.is_dir():
            raise ValueError("Set an existing music library in Settings")
        if mode not in {"full", "incremental"}:
            raise ValueError("Library index mode must be full or incremental")
        separator_regex = str(
            r.cfg.get("scan", {}).get(
                "artist_separator_regex", core.DEFAULT_ARTIST_SEPARATOR_REGEX
            )
        ).strip()
        separator_profile = (
            str(
                r.cfg.get("scan", {}).get(
                    "artist_separator_profile", core.DEFAULT_ARTIST_SEPARATOR_PROFILE
                )
            ).strip()
            or core.DEFAULT_ARTIST_SEPARATOR_PROFILE
        )
        lease = indexer.claim(r.database, mode, str(root))
        started = time.perf_counter()
        with closing(r.database.connect()) as db:
            old_count = int(
                db.execute("SELECT count(*) AS n FROM albums").fetchone()["n"]
            )
            managed_artists = windows_managed_keys(db.raw, "artists")
            managed_albums = windows_managed_keys(db.raw, "albums")
            separator_state = db.execute(
                "SELECT details_json FROM db_maintenance_state WHERE action_name=?",
                ("artist-credit-separator",),
            ).fetchone()
        indexed_separator = ""
        separator_details = {}
        if separator_state is not None:
            try:
                separator_details = json.loads(
                    str(separator_state["details_json"] or "{}")
                )
                indexed_separator = str(
                    separator_details.get("artist_separator_regex", "")
                ).strip()
            except (TypeError, ValueError):
                indexed_separator = ""
                separator_details = {}
        # Earlier builds stored the separator after incremental refreshes and
        # did not update Windows-managed track credits.  Use a separate marker
        # for the one-time complete credit republish.  Afterwards full refresh
        # can use file size/mtime and reread only tracks whose tags changed.
        managed_credit_refresh = int(
            separator_details.get("managed_track_credit_refresh", 0) or 0
        )
        force_track_credit_read = (
            mode == "full"
            and managed_credit_refresh < MANAGED_TRACK_CREDIT_REFRESH_VERSION
        )
        if indexed_separator and indexed_separator != separator_regex:
            raise ValueError(
                "Artist separator settings changed. Reset the PostgreSQL Database, "
                "then use Full Refresh Library Index so every Artist identity is "
                "rebuilt with one rule."
            )
        if (
            not indexed_separator
            and old_count
            and separator_regex != core.DEFAULT_ARTIST_SEPARATOR_REGEX
        ):
            raise ValueError(
                "This PostgreSQL catalog predates the selected Artist separator. "
                "Reset the PostgreSQL Database, then use Full Refresh Library Index."
            )
        windows_authority_configured = bool(managed_artists or managed_albums)
        lookup_db = r.database.connect()
        # A single autocommit lookup connection avoids opening hundreds of
        # PostgreSQL sessions during an unchanged incremental generation. It
        # does not hold a long read transaction or participate in publication.
        lookup_db.raw.autocommit = True

        def existing_album(column, value):
            if not value:
                return None
            if column not in {"album_key", "path"}:
                raise ValueError("Unsupported catalog lookup")
            row = lookup_db.execute(
                f"SELECT albums.*,artists.artist_name,artists.artist_sort,"
                f"artists.musicbrainz_artistid,artists.primary_path "
                f"FROM albums JOIN artists ON artists.artist_key=albums.artist_key "
                f"WHERE albums.{column}=? LIMIT 1",
                (value,),
            ).fetchone()
            return dict(row) if row is not None else None

        class ExistingAlbums:
            def __init__(self, column):
                self.column = column
                self.aliases = {}

            def get(self, value, default=None):
                if value in self.aliases:
                    return self.aliases[value]
                return existing_album(self.column, value) or default

            def __contains__(self, value):
                return self.get(value) is not None

        old_keys = ExistingAlbums("album_key")
        old_paths = ExistingAlbums("path")
        claimed_moved_keys = set()

        def existing_moved_album(path, facts):
            """Match a new path to one missing published manifest row."""
            candidates = list(
                lookup_db.raw.execute(
                    "SELECT albums.*,artists.artist_name,artists.artist_sort,"
                    "artists.musicbrainz_artistid,artists.primary_path "
                    "FROM catalog_change_manifest manifest "
                    "JOIN albums ON albums.album_key=manifest.album_key "
                    "JOIN artists ON artists.artist_key=albums.artist_key "
                    "WHERE manifest.inventory_signature=%s "
                    "AND manifest.artwork_signature=%s "
                    "AND manifest.track_count=%s AND manifest.total_size=%s "
                    "AND manifest.newest_mtime_ns=%s "
                    "AND manifest.album_path<>%s ORDER BY manifest.album_path",
                    (
                        facts["inventory_signature"],
                        facts["artwork_signature"],
                        facts["track_count"],
                        facts["total_size"],
                        facts["newest_mtime_ns"],
                        path,
                    ),
                )
            )
            for candidate in candidates:
                key = str(candidate["album_key"])
                if key in claimed_moved_keys:
                    continue
                if Path(str(candidate["path"])).is_dir():
                    continue
                claimed_moved_keys.add(key)
                return dict(candidate)
            return None

        class GenerationKeys:
            def __init__(self):
                self.pending = {}

            def get(self, key, default=None):
                if key in self.pending:
                    return self.pending[key]
                staged = lookup_db.execute(
                    "SELECT album_path FROM catalog_generation_albums "
                    "WHERE generation_id=? AND scan_token=? AND album_key=? LIMIT 1",
                    (lease.generation_id, lease.owner_token, key),
                ).fetchone()
                if staged is not None:
                    return str(staged["album_path"])
                if mode == "incremental":
                    prior = existing_album("album_key", key)
                    # A published identity at a path which no longer exists
                    # is available for this generation to reuse as a move.
                    # Still-present paths remain reserved so a second physical
                    # copy receives its deterministic :copy: identity.
                    if prior is not None and Path(str(prior["path"])).is_dir():
                        return str(prior["path"])
                return default

            def __setitem__(self, key, path):
                self.pending[key] = path

        used = GenerationKeys()
        try:
            batch_controller = (
                index_batch_policy.AdaptiveBatchController.from_database(
                    lookup_db,
                    enabled=bool(
                        r.cfg.get("scan", {}).get("index_batch_adaptive", True)
                    ),
                    configured_size=int(
                        r.cfg.get("scan", {}).get("index_batch_size", 16)
                    ),
                )
            )
        except Exception:
            lookup_db.close()
            raise
        batch = []
        batch_payload_bytes = 0
        batch_started = time.perf_counter()
        discovered = 0
        completed = 0
        changed_albums = 0
        unchanged_albums = 0
        deferred_albums = 0
        protected_albums = 0
        tag_reads = 0
        tag_reuses = 0
        track_reads = 0
        track_reuses = 0
        inventory_seconds = 0.0
        filesystem_metadata_seconds = 0.0
        checkpoint_seconds = 0.0
        checkpoint_commits = 0
        cover_name = str(r.cfg.get("output", {}).get("file_name", "cover"))
        recovered_checkpoints = (
            indexer.completed_checkpoints(r.database, lease) if lease.resumed else {}
        )
        recovery_batch = []

        def report_progress(phase, message="", *, published=False):
            if not progress:
                return
            total_known = phase in {"validating", "published"}
            remaining_estimate = (
                0 if phase in {"validating", "published"} else batch_controller.current_size
            )
            total = max(old_count, discovered + remaining_estimate, completed, 1)
            percentage = 100 if published else min(99, int(completed * 100 / total))
            progress(
                {
                    "phase": phase,
                    "completed": completed,
                    "total": total,
                    "percentage": percentage,
                    "total_known": total_known,
                    "discovered": discovered,
                    "changed": changed_albums,
                    "unchanged": unchanged_albums,
                    "errors": 0,
                    "message": message,
                }
            )

        report_progress("starting", "Preparing the PostgreSQL catalog generation")

        def flush():
            nonlocal batch, batch_payload_bytes, batch_started
            nonlocal checkpoint_seconds, checkpoint_commits
            if not batch:
                return
            payload_bytes = batch_payload_bytes
            batch_tracks = sum(int(row.get("track_count", 0)) for row in batch)
            processing_seconds = time.perf_counter() - batch_started
            checkpoint_started = time.perf_counter()
            indexer.stage_batch(
                r.database,
                lease,
                batch,
                discovered=discovered,
                completed=completed,
                changed=changed_albums,
                unchanged=unchanged_albums,
                current_path=str(batch[-1]["album_path"]),
            )
            checkpoint_elapsed = time.perf_counter() - checkpoint_started
            checkpoint_seconds += checkpoint_elapsed
            checkpoint_commits += 1
            batch_controller.observe_checkpoint(
                albums=len(batch),
                tracks=batch_tracks,
                payload_bytes=payload_bytes,
                processing_seconds=processing_seconds,
                checkpoint_seconds=checkpoint_elapsed,
                interactive_pressure=index_batch_policy.competing_interactive_activity(
                    lookup_db
                ),
            )
            batch = []
            batch_payload_bytes = 0
            used.pending.clear()
            batch_started = time.perf_counter()

        def flush_recovery():
            nonlocal recovery_batch, checkpoint_seconds, checkpoint_commits
            if not recovery_batch:
                return
            checkpoint_started = time.perf_counter()
            indexer.touch_completed_batch(
                r.database,
                lease,
                recovery_batch,
                discovered=discovered,
                completed=completed,
                changed=changed_albums,
                unchanged=unchanged_albums,
                current_path=recovery_batch[-1][0],
            )
            checkpoint_seconds += time.perf_counter() - checkpoint_started
            checkpoint_commits += 1
            recovery_batch = []

        metadata_readers = max(
            1,
            min(32, int(r.cfg.get("scan", {}).get("index_metadata_readers", 16))),
        )
        metadata_executor = concurrent.futures.ThreadPoolExecutor(
            max_workers=metadata_readers,
            thread_name_prefix="splined-track-tags",
        )
        try:
            iterator = core.iter_inventory(
                root,
                list(r.cfg.get("library", {}).get("ignored_subs", [])),
                cover_name,
                cancelled=cancelled,
            )
            while True:
                inventory_started = time.perf_counter()
                try:
                    album = next(iterator)
                except StopIteration:
                    inventory_seconds += time.perf_counter() - inventory_started
                    break
                inventory_seconds += time.perf_counter() - inventory_started
                if cancelled and cancelled():
                    raise core.InteractionCancelled()
                discovered += 1
                path = str(album.path)
                prior_at_path = old_paths.get(path)
                fallback = (
                    prior_at_path is not None
                    and not str(prior_at_path["musicbrainz_albumid"] or "").strip()
                )
                filesystem_started = time.perf_counter()
                album.inventory_fingerprint = _album_inventory_fingerprint(album, fallback)
                facts = _album_manifest_facts(album)
                filesystem_metadata_seconds += time.perf_counter() - filesystem_started
                facts["inventory_signature"] = str(album.inventory_fingerprint)
                if prior_at_path is None:
                    moved_prior = existing_moved_album(path, facts)
                    if moved_prior is not None:
                        old_paths.aliases[path] = moved_prior
                        prior_at_path = moved_prior
                if lease.resumed:
                    recovered = recovered_checkpoints.get(path)
                    if recovered is not None and recovered[0] == facts["inventory_signature"]:
                        disposition = recovered[1]
                        completed += 1
                        if disposition == "changed":
                            changed_albums += 1
                        elif disposition == "unchanged":
                            unchanged_albums += 1
                        elif disposition == "protected":
                            protected_albums += 1
                        elif disposition == "deferred":
                            deferred_albums += 1
                        recovery_batch.append((path, facts["inventory_signature"]))
                        if len(recovery_batch) >= batch_controller.current_size:
                            flush_recovery()
                        if progress:
                            report_progress("recovering", album.path.name)
                        continue

                disposition = "changed"
                artist = row = review = None
                records = None
                managed = (
                    prior_at_path is not None
                    and str(prior_at_path["album_key"]) in managed_albums
                )
                unchanged = (
                    mode == "incremental"
                    and prior_at_path is not None
                    and _indexed_album_unchanged(
                        prior_at_path, album, str(album.inventory_fingerprint)
                    )
                )
                if managed:
                    disposition = "protected"
                    protected_albums += 1
                    if mode == "full" and force_track_credit_read:
                        with closing(r.database.connect()) as db:
                            old_tracks = {
                                str(item["path"]): item
                                for item in db.execute(
                                    "SELECT * FROM tracks WHERE album_key=?",
                                    (prior_at_path["album_key"],),
                                )
                            }
                        filesystem_started = time.perf_counter()
                        records, reads, reuses = tags.inspect_album_tracks(
                            album,
                            str(prior_at_path["album_key"]),
                            old_tracks,
                            force_read=True,
                            artist_separator_regex=separator_regex,
                            executor=metadata_executor,
                        )
                        filesystem_metadata_seconds += (
                            time.perf_counter() - filesystem_started
                        )
                        track_reads += reads
                        track_reuses += reuses
                elif unchanged:
                    disposition = "unchanged"
                    unchanged_albums += 1
                else:
                    filesystem_started = time.perf_counter()
                    artist, row, review, reused_tags = tags.inspect_album(
                        album, root, cover_name, old_keys, old_paths, used
                    )
                    filesystem_metadata_seconds += time.perf_counter() - filesystem_started
                    tag_reuses += int(reused_tags)
                    tag_reads += int(not reused_tags)
                    if str(row["album_key"]) in managed_albums:
                        authoritative = old_keys.get(str(row["album_key"]))
                        if authoritative is None:
                            raise ValueError(
                                "Windows-managed Album authority disappeared during indexing"
                            )
                        row["artist_key"] = str(authoritative["artist_key"])
                        artist["artist_key"] = str(authoritative["artist_key"])
                        disposition = "protected"
                        protected_albums += 1
                    elif (
                        windows_authority_configured
                        and prior_at_path is None
                        and str(row["album_key"]) not in old_keys
                    ):
                        disposition = "deferred"
                        deferred_albums += 1
                    if disposition == "changed":
                        now = tags.utc_now()
                        artist["splined_version"] = core.VERSION
                        row["splined_version"] = core.VERSION
                        row["last_seen_at"] = now
                        row["inventory_fingerprint"] = str(album.inventory_fingerprint)
                        with closing(r.database.connect()) as db:
                            old_tracks = {
                                str(item["path"]): item
                                for item in db.execute(
                                    "SELECT * FROM tracks WHERE album_key=?",
                                    (row["album_key"],),
                                )
                            }
                        filesystem_started = time.perf_counter()
                        records, reads, reuses = tags.inspect_album_tracks(
                            album,
                            str(row["album_key"]),
                            old_tracks,
                            force_read=force_track_credit_read,
                            artist_separator_regex=separator_regex,
                            executor=metadata_executor,
                        )
                        filesystem_metadata_seconds += (
                            time.perf_counter() - filesystem_started
                        )
                        track_reads += reads
                        track_reuses += reuses
                        for record in records:
                            record["splined_version"] = core.VERSION
                        changed_albums += 1

                completed += 1
                checkpoint = {
                    "album_path": path,
                    "album_key": str(
                        (row or prior_at_path or {}).get("album_key", "")
                    ),
                    "artist_key": str(
                        (row or prior_at_path or {}).get("artist_key", "")
                    ),
                    **facts,
                    "tag_signature": str(
                        (row or prior_at_path or {}).get("tag_signature", "")
                    ),
                    "disposition": disposition,
                    "artist_payload": artist if disposition == "changed" else None,
                    "album_payload": row if disposition == "changed" else None,
                    "tracks_payload": records,
                    "review_payload": review,
                }
                batch.append(checkpoint)
                batch_payload_bytes += index_batch_policy.checkpoint_payload_bytes(
                    [checkpoint]
                )
                if batch_controller.should_checkpoint(
                    len(batch), batch_payload_bytes
                ):
                    flush()
                if progress and (
                    completed == 1
                    or completed % batch_controller.current_size == 0
                ):
                    report_progress("indexing", album.path.name)
            flush_recovery()
            flush()
            report_progress("validating", "Validating the staged generation")
            publication = self._publish_index_generation(
                lease,
                discovered=discovered,
                separator_regex=separator_regex,
                separator_profile=separator_profile,
                managed_credit_refresh=managed_credit_refresh,
                managed_artists=managed_artists,
                managed_albums=managed_albums,
                timings={
                    "total_seconds": round(time.perf_counter() - started, 3),
                    "album_tag_reads": tag_reads,
                    "album_tag_reuses": tag_reuses,
                    "track_tag_reads": track_reads,
                    "track_tag_reuses": track_reuses,
                    "inventory_seconds": round(inventory_seconds, 3),
                    "filesystem_metadata_seconds": round(
                        filesystem_metadata_seconds, 3
                    ),
                    "checkpoint_seconds": round(checkpoint_seconds, 3),
                    "checkpoint_commits": checkpoint_commits,
                    "batch_size": batch_controller.initial_size,
                    "adaptive_batching": batch_controller.summary(),
                    "resource_budget": {
                        "index_workers": 1,
                        "metadata_readers": metadata_readers,
                        "maximum_batch_albums": 64,
                        "postgresql_connections": r.database.pool_stats()["max_size"],
                        "remote_provider_requests": 0,
                    },
                },
            )
            report_progress("published", "Published the validated generation", published=True)
        except core.InteractionCancelled:
            indexer.finish(r.database, lease, "cancelled", "Cancelled by operator")
            raise
        except Exception as error:
            indexer.finish(r.database, lease, "failed", str(error))
            raise
        finally:
            metadata_executor.shutdown(wait=True, cancel_futures=True)
            lookup_db.close()

        result = {
            "mode": lease.mode,
            "generation_id": lease.generation_id,
            "resumed": lease.resumed,
            "albums": discovered,
            "changed_albums": changed_albums,
            "unchanged_albums": unchanged_albums,
            "removed_albums": publication["removed_albums"],
            "moved_albums": publication["moved_albums"],
            "protected_albums": protected_albums,
            "deferred_albums": deferred_albums,
            "windows_managed_albums": len(managed_albums),
        }
        # Artist appearances come from the refreshed catalog plus Navidrome.
        # Never retain an empty or partial artist projection across a refresh.
        self.catalog.appearance_cache.clear()
        self.catalog.metadata_cache.clear()
        self.catalog.portrait_cache.clear()
        result.update(
            artists=len(self.catalog.artists("")),
            identified_albums=len(self.catalog.albums("", False)),
            fallback_albums=len(self.catalog.albums("", True)),
        )
        self.audit("index-refresh", str(root), result)
        return result

    def _publish_index_generation(
        self,
        lease,
        *,
        discovered,
        separator_regex,
        separator_profile,
        managed_credit_refresh,
        managed_artists,
        managed_albums,
        timings,
    ):
        """Validate and atomically publish one completely enumerated generation."""
        r = self.runtime
        touched_artists = set()
        removed = []
        moved = []
        with closing(r.database.connect()) as db, db:
            db.raw.execute("SELECT pg_advisory_xact_lock(741520206)")
            owner = db.raw.execute(
                "UPDATE catalog_generations SET state='validating',"
                "phase='validating',updated_at=now() WHERE generation_id=%s "
                "AND owner_token=%s AND state='building' "
                "AND lease_expires_at>now() RETURNING generation_id",
                (lease.generation_id, lease.owner_token),
            ).fetchone()
            if owner is None:
                raise indexer.GenerationLeaseLost(
                    "A stale catalog index worker cannot publish"
                )
            staged_count = int(
                db.raw.execute(
                    "SELECT count(*) AS n FROM catalog_generation_albums "
                    "WHERE generation_id=%s AND scan_token=%s "
                    "AND disposition<>'error'",
                    (lease.generation_id, lease.owner_token),
                ).fetchone()["n"]
            )
            error_count = int(
                db.raw.execute(
                    "SELECT count(*) AS n FROM catalog_generation_albums "
                    "WHERE generation_id=%s AND scan_token=%s "
                    "AND disposition='error'",
                    (lease.generation_id, lease.owner_token),
                ).fetchone()["n"]
            )
            if staged_count != discovered or error_count:
                raise ValueError(
                    "Catalog generation validation failed: "
                    f"discovered={discovered} staged={staged_count} errors={error_count}"
                )
            duplicate = db.raw.execute(
                "SELECT album_key FROM catalog_generation_albums "
                "WHERE generation_id=%s AND scan_token=%s "
                "AND disposition='changed' AND album_key<>'' "
                "GROUP BY album_key HAVING count(*)>1 LIMIT 1",
                (lease.generation_id, lease.owner_token),
            ).fetchone()
            if duplicate is not None:
                raise ValueError(
                    f"Catalog generation contains duplicate Album identity {duplicate['album_key']}"
                )
            self._catalog_publication_checkpoint("validated")
            publishing = db.raw.execute(
                "UPDATE catalog_generations SET state='publishing',"
                "phase='publishing',updated_at=now() WHERE generation_id=%s "
                "AND owner_token=%s AND state='validating' "
                "AND lease_expires_at>now() RETURNING generation_id",
                (lease.generation_id, lease.owner_token),
            ).fetchone()
            if publishing is None:
                raise indexer.GenerationLeaseLost(
                    "Catalog publication lease expired during validation"
                )
            stage_rows = db.raw.execute(
                "SELECT * FROM catalog_generation_albums "
                "WHERE generation_id=%s AND scan_token=%s "
                "ORDER BY album_path",
                (lease.generation_id, lease.owner_token),
            )
            for staged in stage_rows:
                disposition = str(staged["disposition"])
                album_key = str(staged["album_key"] or "")
                artist_key = str(staged["artist_key"] or "")
                if artist_key:
                    touched_artists.add(artist_key)
                if disposition == "changed":
                    artist = dict(staged["artist_payload"] or {})
                    album = dict(staged["album_payload"] or {})
                    tracks_payload = list(staged["tracks_payload"] or [])
                    if not artist or not album:
                        raise ValueError(
                            f"Changed Album checkpoint lacks payload: {staged['album_path']}"
                        )
                    if int(album.get("track_count", -1)) != len(tracks_payload):
                        raise ValueError(
                            f"Track coverage mismatch for {album.get('album_name', album_key)}"
                        )
                    previous_at_path = db.execute(
                        "SELECT album_key,artist_key FROM albums WHERE path=?",
                        (staged["album_path"],),
                    ).fetchone()
                    previous_by_key = db.execute(
                        "SELECT album_key,artist_key,path FROM albums "
                        "WHERE album_key=?",
                        (album_key,),
                    ).fetchone()
                    moved_from = (
                        str(previous_by_key["path"])
                        if previous_by_key is not None
                        and str(previous_by_key["path"]) != str(staged["album_path"])
                        else ""
                    )
                    if (
                        previous_at_path is not None
                        and str(previous_at_path["album_key"]) != album_key
                        and str(previous_at_path["album_key"]) not in managed_albums
                    ):
                        touched_artists.add(str(previous_at_path["artist_key"]))
                        db.execute(
                            "DELETE FROM tracks WHERE album_key=?",
                            (previous_at_path["album_key"],),
                        )
                        db.execute(
                            "DELETE FROM albums WHERE album_key=?",
                            (previous_at_path["album_key"],),
                        )
                    if db.execute(
                        "SELECT 1 FROM artists WHERE artist_key=?",
                        (artist["artist_key"],),
                    ).fetchone():
                        artist.pop("status", None)
                    self._upsert(db, "artists", artist, "artist_key")
                    self._upsert(db, "albums", album, "album_key")
                    db.execute("DELETE FROM tracks WHERE album_key=?", (album_key,))
                    for track in tracks_payload:
                        if str(track.get("album_key", "")) != album_key:
                            raise ValueError(
                                f"Track references the wrong Album: {track.get('path', '')}"
                            )
                        self._upsert(db, "tracks", dict(track), "track_key")
                    review = staged["review_payload"]
                    db.execute(
                        "DELETE FROM album_refresh_review_queue WHERE album_key=?",
                        (album_key,),
                    )
                    if review:
                        db.execute(
                            "INSERT INTO album_refresh_review_queue"
                            "(album_key,requested_at,source,details_json,splined_version) "
                            "VALUES(?,?,?,?,?) ON CONFLICT(album_key) DO UPDATE SET "
                            "requested_at=excluded.requested_at,source=excluded.source,"
                            "details_json=excluded.details_json,"
                            "splined_version=excluded.splined_version",
                            (
                                album_key,
                                tags.utc_now(),
                                review["source"],
                                json.dumps(review["details"], sort_keys=True),
                                core.VERSION,
                            ),
                        )
                    operation = "move" if moved_from else "upsert"
                    if moved_from:
                        moved.append(
                            {
                                "album_key": album_key,
                                "from": moved_from,
                                "to": str(staged["album_path"]),
                            }
                        )
                    db.raw.execute(
                        "INSERT INTO catalog_index_events("
                        "event_id,generation_id,album_key,album_path,operation,"
                        "inventory_signature) VALUES(%s,%s,%s,%s,%s,%s) "
                        "ON CONFLICT(generation_id,album_path,operation) DO NOTHING",
                        (
                            str(uuid4()),
                            lease.generation_id,
                            album_key,
                            staged["album_path"],
                            operation,
                            staged["inventory_signature"],
                        ),
                    )
                elif disposition == "protected" and staged["tracks_payload"]:
                    # Windows-owned identity and membership are immutable here;
                    # a Full Refresh may only refresh file-derived tag fields.
                    for track in staged["tracks_payload"]:
                        db.raw.execute(
                            "UPDATE tracks SET title=%s,artist_name=%s,"
                            "musicbrainz_recordingid=NULLIF(%s,''),"
                            "musicbrainz_artistid=NULLIF(%s,''),file_size=%s,"
                            "file_mtime_ns=%s,updated_at=%s WHERE track_key=%s "
                            "AND album_key=%s AND EXISTS("
                            "SELECT 1 FROM catalog_sync_authority authority "
                            "WHERE authority.entity_table='tracks' "
                            "AND authority.entity_key=tracks.track_key "
                            "AND authority.authority='windows')",
                            (
                                track["title"],
                                track["artist_name"],
                                track["musicbrainz_recordingid"],
                                track["musicbrainz_artistid"],
                                track["file_size"],
                                track["file_mtime_ns"],
                                track["updated_at"],
                                track["track_key"],
                                album_key,
                            ),
                        )

            removed = list(
                db.raw.execute(
                    "SELECT albums.album_key,albums.artist_key,albums.path "
                    "FROM albums WHERE NOT EXISTS("
                    "SELECT 1 FROM catalog_generation_albums staged "
                    "WHERE staged.generation_id=%s AND staged.scan_token=%s "
                    "AND staged.album_path=albums.path) AND NOT EXISTS("
                    "SELECT 1 FROM catalog_sync_authority authority "
                    "WHERE authority.entity_table='albums' "
                    "AND authority.entity_key=albums.album_key "
                    "AND authority.authority='windows')",
                    (lease.generation_id, lease.owner_token),
                )
            )
            for stale in removed:
                touched_artists.add(str(stale["artist_key"]))
                db.execute(
                    "DELETE FROM tracks WHERE album_key=?", (stale["album_key"],)
                )
                db.execute(
                    "DELETE FROM albums WHERE album_key=?", (stale["album_key"],)
                )
                db.raw.execute(
                    "INSERT INTO catalog_index_events("
                    "event_id,generation_id,album_key,album_path,operation) "
                    "VALUES(%s,%s,%s,%s,'remove') "
                    "ON CONFLICT(generation_id,album_path,operation) DO NOTHING",
                    (
                        str(uuid4()),
                        lease.generation_id,
                        stale["album_key"],
                        stale["path"],
                    ),
                )
            db.raw.execute(
                "DELETE FROM artists WHERE NOT EXISTS "
                "(SELECT 1 FROM albums WHERE albums.artist_key=artists.artist_key) "
                "AND NOT EXISTS (SELECT 1 FROM catalog_sync_authority authority "
                "WHERE authority.entity_table='artists' "
                "AND authority.entity_key=artists.artist_key "
                "AND authority.authority='windows')"
            )
            for artist in db.execute("SELECT artist_key FROM artists"):
                artist_key = str(artist["artist_key"])
                self._update_artist_presentation(
                    db, artist_key, artist_key in managed_artists
                )
            db.raw.execute(
                "DELETE FROM catalog_change_manifest WHERE NOT EXISTS("
                "SELECT 1 FROM catalog_generation_albums staged "
                "WHERE staged.generation_id=%s AND staged.scan_token=%s "
                "AND staged.album_path=catalog_change_manifest.album_path)",
                (lease.generation_id, lease.owner_token),
            )
            db.raw.execute(
                "INSERT INTO catalog_change_manifest("
                "album_path,album_key,inventory_signature,artwork_signature,"
                "tag_signature,track_count,total_size,newest_mtime_ns,"
                "last_seen_generation,updated_at) SELECT album_path,album_key,"
                "inventory_signature,artwork_signature,tag_signature,"
                "track_count,total_size,newest_mtime_ns,%s,now() "
                "FROM catalog_generation_albums WHERE generation_id=%s "
                "AND scan_token=%s ON CONFLICT(album_path) DO UPDATE SET "
                "album_key=excluded.album_key,"
                "inventory_signature=excluded.inventory_signature,"
                "artwork_signature=excluded.artwork_signature,"
                "tag_signature=excluded.tag_signature,"
                "track_count=excluded.track_count,total_size=excluded.total_size,"
                "newest_mtime_ns=excluded.newest_mtime_ns,"
                "last_seen_generation=excluded.last_seen_generation,"
                "updated_at=now()",
                (lease.generation_id, lease.generation_id, lease.owner_token),
            )
            db.execute(
                "INSERT INTO db_maintenance_state"
                "(action_name,completed_at,details_json,splined_version) "
                "VALUES(?,?,?,?) ON CONFLICT(action_name) DO UPDATE SET "
                "completed_at=excluded.completed_at,"
                "details_json=excluded.details_json,"
                "splined_version=excluded.splined_version",
                (
                    "artist-credit-separator",
                    tags.utc_now(),
                    json.dumps(
                        {
                            "artist_separator_regex": separator_regex,
                            "artist_separator_profile": separator_profile,
                            "managed_track_credit_refresh": (
                                MANAGED_TRACK_CREDIT_REFRESH_VERSION
                                if lease.mode == "full"
                                else managed_credit_refresh
                            ),
                        },
                        sort_keys=True,
                    ),
                    core.VERSION,
                ),
            )
            counts = db.raw.execute(
                "SELECT (SELECT count(*) FROM artists) AS artists,"
                "(SELECT count(*) FROM albums) AS albums,"
                "(SELECT count(*) FROM tracks) AS tracks"
            ).fetchone()
            publication_timings = dict(timings)
            publication_timings["artists"] = int(counts["artists"])
            publication_timings["albums"] = int(counts["albums"])
            publication_timings["tracks"] = int(counts["tracks"])
            publication_timings["moved_albums"] = len(moved)
            published = db.raw.execute(
                "UPDATE catalog_generations SET state='published',"
                "phase='complete',removed_albums=%s,timings=%s,"
                "published_at=now(),updated_at=now() WHERE generation_id=%s "
                "AND owner_token=%s AND state='publishing'",
                (
                    len(removed),
                    Jsonb(publication_timings),
                    lease.generation_id,
                    lease.owner_token,
                ),
            ).rowcount
            if published != 1:
                raise indexer.GenerationLeaseLost(
                    "Catalog publication rejected a stale generation owner"
                )
            activated = db.raw.execute(
                "UPDATE catalog_index_state SET active_generation_id=%s,"
                "maintenance_generation_id='',catalog_state='ready',"
                "change_summary=%s,updated_at=now() WHERE singleton=TRUE "
                "AND maintenance_generation_id=%s",
                (
                    lease.generation_id,
                    Jsonb(
                        {
                            "count": 0,
                            "added": 0,
                            "changed": 0,
                            "moved": 0,
                            "removed": 0,
                            "examples": [],
                        }
                    ),
                    lease.generation_id,
                ),
            ).rowcount
            if activated != 1:
                raise indexer.GenerationLeaseLost(
                    "Catalog generation could not atomically become active"
                )
            self._catalog_publication_checkpoint("before-commit")
        return {"removed_albums": len(removed), "moved_albums": len(moved)}

    def _catalog_publication_checkpoint(self, _phase):
        """Failure-injection seam for atomic publication recovery tests."""

    def hydrate_tracks(
        self,
        album_id,
        *,
        refresh_embedding_stats=True,
        force_tag_read=False,
        refresh_managed_track_tags=False,
    ):
        row = self.album_row(album_id)
        files = self.album_files(row)
        with closing(self.runtime.database.connect()) as db:
            old = {
                item["path"]: item
                for item in db.execute(
                    "SELECT * FROM tracks WHERE album_key=?", (album_id,)
                )
            }
            album_managed = windows_managed(db.raw, "albums", album_id)
            managed_tracks = windows_managed_track_keys(db.raw, album_id)
        records, _, _ = tags.inspect_album_tracks(
            core.AlbumDir(Path(row["path"]), files),
            album_id,
            old,
            force_read=force_tag_read,
            artist_separator_regex=self.catalog.artist_separator_regex,
        )
        with closing(self.runtime.database.connect()) as db, db:
            for record in records:
                if album_managed and str(record["track_key"]) not in managed_tracks:
                    continue
                record["splined_version"] = core.VERSION
                if (
                    refresh_managed_track_tags
                    and str(record["track_key"]) in managed_tracks
                ):
                    # Full refresh may revalidate only file-derived fields for
                    # an already matched Windows track.  Album/Artist identity,
                    # track membership, authority, and insertion/deletion stay
                    # Windows-owned.  This lets a corrected shared media tag
                    # update WebGUI's Featured Artist projection without
                    # opening a competing catalog identity path.
                    prior = old.get(str(record["path"]))
                    fields = (
                        "title",
                        "artist_name",
                        "musicbrainz_recordingid",
                        "musicbrainz_artistid",
                        "file_size",
                        "file_mtime_ns",
                    )
                    if prior is not None and all(
                        str(prior[field] or "") == str(record[field] or "")
                        for field in fields
                    ):
                        continue
                    db.execute(
                        "UPDATE tracks SET title=?,artist_name=?,"
                        "musicbrainz_recordingid=?,musicbrainz_artistid=?,"
                        "file_size=?,file_mtime_ns=?,updated_at=? "
                        "WHERE track_key=? AND album_key=?",
                        (
                            record["title"],
                            record["artist_name"],
                            record["musicbrainz_recordingid"],
                            record["musicbrainz_artistid"],
                            record["file_size"],
                            record["file_mtime_ns"],
                            record["updated_at"],
                            record["track_key"],
                            album_id,
                        ),
                    )
                else:
                    self._upsert(db, "tracks", record, "track_key")
            existing = {str(p) for p in files}
            for path in old:
                if path not in existing and path not in managed_tracks:
                    db.execute(
                        "DELETE FROM tracks WHERE album_key=? AND path=?",
                        (album_id, path),
                    )
        if refresh_embedding_stats:
            self.refresh_embedding_stats(row)
        return records

    def ensure_tracks(self, album_id):
        """Hydrate only when the durable catalog lacks indexed album tracks."""
        row = self.album_row(album_id)
        with closing(self.runtime.database.connect()) as db:
            indexed = int(
                db.execute(
                    "SELECT count(*) FROM tracks WHERE album_key=?", (album_id,)
                ).fetchone()[0]
            )
        expected = int(row["track_count"] or 0)
        return self.hydrate_tracks(album_id) if indexed < expected else []

    def indexed_tracks(self, album_id, track_keys):
        """Return only explicitly requested indexed tracks, in request order."""
        requested = [str(key) for key in track_keys]
        if not requested:
            return []
        placeholders = ",".join("?" for _ in requested)
        with closing(self.runtime.database.connect()) as db:
            rows = list(
                db.execute(
                    "SELECT * FROM tracks WHERE album_key=? "
                    f"AND track_key IN ({placeholders})",
                    (album_id, *requested),
                )
            )
        by_key = {str(row["track_key"]): row for row in rows}
        return [by_key[key] for key in requested if key in by_key]

    def hydrate_selected_tracks(self, album_id, track_keys):
        """Refresh tag metadata for selected tracks without scanning the album."""
        row = self.album_row(album_id)
        indexed = self.indexed_tracks(album_id, track_keys)
        if len(indexed) != len(track_keys):
            raise ValueError("Choose indexed COMP tracks")
        files = [Path(item["path"]) for item in indexed]
        old = {str(item["path"]): item for item in indexed}
        records, _, _ = tags.inspect_album_tracks(
            core.AlbumDir(Path(row["path"]), files),
            album_id,
            old,
            artist_separator_regex=self.catalog.artist_separator_regex,
        )
        with closing(self.runtime.database.connect()) as db, db:
            for record in records:
                record["splined_version"] = core.VERSION
                self._upsert(db, "tracks", record, "track_key")
        refreshed = {str(item["track_key"]): item for item in records}
        return [refreshed[key] for key in track_keys if key in refreshed]

    def refresh_embedding_stats(self, row):
        current = int(row["embedded_tracks"] or 0) if "embedded_tracks" in row else 0
        validation = "verified"
        error = ""
        preferred_side = int(self.catalog.comp_thumbnail_side)
        try:
            files = self.album_files(row)
            expected = int(row["track_count"] or 0)
            if expected > 0 and len(files) != expected:
                raise OSError(
                    f"Indexed track count {expected} does not match readable files {len(files)}"
                )
            count = sum(
                _track_has_preferred_embedded_artwork(path, preferred_side)
                for path in files
            )
        except (OSError, ValueError) as exception:
            count = current
            validation = "retry"
            error = sanitize_text(exception, limit=500)
        with closing(self.runtime.database.connect()) as db, db:
            db.execute(
                "UPDATE albums SET embedded_tracks=?,embedded_validation_status=?,"
                "embedded_validated_at=now() WHERE album_key=?",
                (count, validation, row["album_key"]),
            )
        if validation == "retry":
            self.audit(
                "embedded-artwork-validation",
                str(row["album_key"]),
                {
                    "status": validation,
                    "error": error,
                    "preferred_side": preferred_side,
                    "preserved_embedded_tracks": current,
                },
                severity="warning",
                stage="readback",
                request_status="retry",
            )
        return count

    @serialized
    def validate_album_embedding(self, album_id: str) -> dict[str, object]:
        """Verify one mapped Album without converting a read failure into missing art."""
        row = self.album_row(album_id)
        self.refresh_embedding_stats(row)
        with closing(self.runtime.database.connect()) as db:
            verified = db.execute(
                "SELECT embedded_tracks,track_count,embedded_validation_status,"
                "embedded_validated_at FROM albums WHERE album_key=?",
                (album_id,),
            ).fetchone()
        return {
            "album_id": album_id,
            "embedded_tracks": int(verified["embedded_tracks"] or 0),
            "track_count": int(verified["track_count"] or 0),
            "status": str(verified["embedded_validation_status"] or "unknown"),
            "validated_at": str(verified["embedded_validated_at"] or ""),
            "preferred_side": int(self.catalog.comp_thumbnail_side),
        }

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
        configured_cover = str(
            self.runtime.cfg.get("output", {}).get("file_name", "cover")
        )
        configured_stem = (configured_cover.strip() or "cover").casefold()
        local_art = sorted(
            path
            for path in folder.iterdir()
            if path.is_file()
            and not path.is_symlink()
            and path.suffix.casefold() in {".jpg", ".jpeg", ".png", ".webp"}
            and (
                path.stem.casefold().startswith("cover")
                or path.stem.casefold().startswith(configured_stem)
            )
        )
        album = core.AlbumDir(folder, files, local_art)
        cover = tags.cover_statistics(
            album,
            configured_cover,
        )
        representative = sorted(files, key=lambda value: str(value).casefold())[0]
        representative_stat = representative.stat()
        fingerprint = _album_inventory_fingerprint(
            album, not str(row["musicbrainz_albumid"] or "").strip()
        )
        manifest = _album_manifest_facts(album)
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
            # A verified SPLINED write becomes the new filesystem baseline.
            # Refresh only this Album's existing manifest row so a following
            # change probe does not report our own cover/tag/embed operation.
            # External mutations remain visible because no general-purpose
            # catalog read calls this write-boundary refresh.
            db.raw.execute(
                "UPDATE catalog_change_manifest SET "
                "inventory_signature=%s,artwork_signature=%s,track_count=%s,"
                "total_size=%s,newest_mtime_ns=%s,updated_at=now() "
                "WHERE album_path=%s",
                (
                    fingerprint,
                    manifest["artwork_signature"],
                    manifest["track_count"],
                    manifest["total_size"],
                    manifest["newest_mtime_ns"],
                    str(folder),
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
        policy_source_width, policy_source_height = image.size
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
            with _temporary_workspace(self.runtime, "prepared-previews") as directory:
                path = directory / (uuid4().hex + ".png")
                path.write_bytes(body)
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
        else:
            width, height = image.size
        policy_source_short = min(policy_source_width, policy_source_height)
        output_short = min(width, height)
        applied_percent = (
            output_short / policy_source_short * 100.0
            if policy_source_short > 0
            else 100.0
        )
        requested_percent = (
            ideal / policy_source_short * 100.0
            if 0 < policy_source_short < ideal
            else 100.0
        )
        upscale_limited = bool(upscaled and output_short < ideal)
        if upscaled:
            if upscale_limited:
                reason = (
                    f"Upscaled {policy_source_width} × {policy_source_height} → "
                    f"{width} × {height} ({applied_percent:.0f}% ceiling; "
                    f"below {ideal}px Ideal)"
                )
            else:
                reason = (
                    f"Upscaled {policy_source_width} × {policy_source_height} → "
                    f"{width} × {height} ({applied_percent:.0f}% to "
                    f"{ideal}px Ideal)"
                )
        elif upscale_requested and not upscale_enabled:
            reason = "Actual source resolution preserved because Upscale Below Ideal is off in Settings"
        elif upscale_requested and policy_source_short >= ideal:
            reason = f"Source already meets or exceeds {ideal}px Ideal; no enlargement applied"
        elif upscale_requested:
            reason = "The configured upscale ceiling prevents enlargement; source resolution preserved"
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
            "source_width": policy_source_width,
            "source_height": policy_source_height,
            "ideal": ideal,
            "upscale_percent": applied_percent,
            "upscale_requested_percent": requested_percent,
            "upscale_max_percent": max_percent,
            "upscale_limited": upscale_limited,
            "embedded_thumbnail_size": int(
                output.get("embedded_thumbnail_size", 1200)
            ),
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
        outputs = core.canonical_cover_outputs(destination, body, stem)
        output_bodies = {
            path.resolve(strict=False): output_body for path, output_body in outputs
        }
        exact_covers = sorted(
            (
                path
                for path in Path(row["path"]).iterdir()
                if path.is_file()
                and not path.is_symlink()
                and path.stem.casefold() == stem.casefold()
                and path.suffix.casefold() in {".jpg", ".jpeg", ".png", ".webp"}
            ),
            key=lambda path: path.name.casefold(),
        )
        preserved = []
        preserve_pairs = []
        preserve = bool(self.runtime.cfg.get("output", {}).get("preserve_file", True))
        removed = []
        if preserve:
            previous = {}
            try:
                for existing in exact_covers:
                    resolved = existing.resolve(strict=False)
                    if resolved in output_bodies and existing.read_bytes() == output_bodies[resolved]:
                        continue
                    # A retained WebP source is independent of a static write.
                    if existing.suffix.casefold() == ".webp" and resolved not in output_bodies:
                        continue
                    number = 2
                    backup = existing.with_name(f"{stem}-({number}){existing.suffix}")
                    while backup.exists():
                        number += 1
                        backup = existing.with_name(f"{stem}-({number}){existing.suffix}")
                    prior = existing.read_bytes()
                    self._atomic_artwork(backup, prior)
                    if backup.read_bytes() != prior:
                        backup.unlink(missing_ok=True)
                        raise scan.QueuedArtworkVerificationError(
                            "Preserved cover did not pass byte-for-byte verification"
                        )
                    preserve_pairs.append((existing, backup))
                    preserved.append(backup.name)

                previous = {
                    path: path.read_bytes() if path.is_file() else None
                    for path, _ in outputs
                }
                for path, output_body in outputs:
                    self._atomic_artwork(path, output_body)
                    if path.read_bytes() != output_body:
                        raise scan.QueuedArtworkVerificationError(
                            "Written cover did not pass byte-for-byte readback verification"
                        )
                    with Image.open(path) as opened:
                        opened.verify()
                for existing, _ in preserve_pairs:
                    if existing.resolve(strict=False) not in output_bodies:
                        existing.unlink()
            except Exception:
                for existing, backup in preserve_pairs:
                    if backup.is_file():
                        self._atomic_artwork(existing, backup.read_bytes())
                for path, prior in previous.items():
                    if prior is None:
                        path.unlink(missing_ok=True)
                    else:
                        self._atomic_artwork(path, prior)
                for _, backup in preserve_pairs:
                    backup.unlink(missing_ok=True)
                raise
        else:
            removed = [path.name for path in core.install_canonical_cover(destination, body, stem)]

        presentation = next(
            (path for path, _ in outputs if path.suffix.casefold() != ".webp"),
            destination,
        )
        written = [path.name for path, _ in outputs if path.is_file()]
        verified = presentation.read_bytes()
        with Image.open(BytesIO(verified)) as opened:
            verified_size = [int(opened.width), int(opened.height)]
        self.refresh_album(row["album_key"], source="webgui")
        self.audit(
            "write-cover",
            row["album_key"],
            {
                "album_id": row["album_key"],
                "file": presentation.name,
                "written": written,
                "preserved": preserved,
                "removed": removed,
                "sha256": hashlib.sha256(verified).hexdigest(),
                "verified_size": verified_size,
            },
            stage="write-readback",
            request_status="verified",
        )
        coordination = self.record_album_write(row, "write-cover")
        return {
            "written": written,
            "preserved": preserved,
            "removed": removed,
            "coordination": coordination,
            **rendered,
            # The verified canonical presentation, not a temporary candidate
            # handle or an arbitrary sibling cover.*, is the source of truth
            # for a following Album Track Embedding stage.
            "file": presentation.name,
            "sha256": hashlib.sha256(verified).hexdigest(),
        }

    @serialized
    def write_artist(self, payload):
        self.runtime.require_write()
        artist_id = str(payload.get("artist_id", ""))
        artist = self.catalog.artist(artist_id)
        with closing(self.runtime.database.connect()) as db:
            rows = list(
                db.execute(
                    "SELECT albums.path,albums.musicbrainz_albumid,artists.artist_name,artists.musicbrainz_artistid FROM albums JOIN artists ON artists.artist_key=albums.artist_key WHERE albums.artist_key=?",
                    (artist_id,),
                )
            )
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
            str(artist["name"]),
            self.runtime.artist_art,
            allow_managed_without_owned_album=bool(artist.get("albums")),
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
                # Artist Art is one shared, identity-scoped package. Unlike an
                # Album cover candidate, the last verified Artist Art save
                # must immediately become canonical for both clients.
                preserve_file=False,
            )
            written.append(path.name)
        self.audit("write-artist-art", artist_id, {"files": written})
        self.catalog.metadata_cache.pop(artist_id, None)
        self.catalog.portrait_cache.pop(artist_id, None)
        return {"written": written}

    def _persist_comp_track_authority(self, track, recording_mbid, artist_mbids):
        """Atomically save validated Recording/Artist IDs without Album identity."""
        recording = str(recording_mbid or "").strip().casefold()
        artists = "; ".join(sorted(compilation._mbids(artist_mbids)))
        if not core.valid_mbid(recording) or not artists:
            raise ValueError("Validated Recording and Artist IDs are required")
        path = Path(track.path).resolve()
        if not inside(path, self.runtime.music):
            raise ValueError("COMP track is outside the configured music library")
        descriptor, staged_name = tempfile.mkstemp(
            prefix=f".{path.name}.splined-identity-",
            suffix=path.suffix,
            dir=path.parent,
        )
        os.close(descriptor)
        staged = Path(staged_name)
        try:
            shutil.copyfile(path, staged)
            try:
                shutil.copymode(path, staged)
            except OSError:
                pass
            audio = MutagenFile(staged)
            if audio is None:
                raise ValueError("Unsupported COMP track format")
            if audio.tags is None:
                audio.add_tags()
            if isinstance(audio, MP4):
                assert audio.tags is not None
                audio.tags["----:com.apple.iTunes:MusicBrainz Track Id"] = [
                    recording.encode("utf-8")
                ]
                audio.tags["----:com.apple.iTunes:MusicBrainz Artist Id"] = [
                    artists.encode("utf-8")
                ]
            elif isinstance(audio.tags, ID3):
                retained = []
                for frame in audio.tags.getall("TXXX"):
                    description = str(getattr(frame, "desc", "") or "").strip().casefold()
                    if description not in {
                        "musicbrainz recording id",
                        "musicbrainz track id",
                        "musicbrainz_trackid",
                        "musicbrainz artist id",
                        "musicbrainz_artistid",
                    }:
                        retained.append(frame)
                audio.tags.delall("TXXX")
                for frame in retained:
                    audio.tags.add(frame)
                retained_ufid = [
                    frame
                    for frame in audio.tags.getall("UFID")
                    if str(getattr(frame, "owner", "") or "")
                    .rstrip("/")
                    .casefold()
                    != "http://musicbrainz.org"
                ]
                audio.tags.delall("UFID")
                for frame in retained_ufid:
                    audio.tags.add(frame)
                audio.tags.add(
                    TXXX(encoding=3, desc="MusicBrainz Track Id", text=[recording])
                )
                audio.tags.add(
                    TXXX(encoding=3, desc="MusicBrainz Artist Id", text=[artists])
                )
                audio.tags.add(
                    UFID(owner="http://musicbrainz.org", data=recording.encode("ascii"))
                )
            else:
                assert audio.tags is not None
                audio.tags["musicbrainz_trackid"] = [recording]
                audio.tags["musicbrainz_artistid"] = [artists]
            audio.save()
            verified = core.read_track(staged)
            if str(verified.recording_mbid or "").strip().casefold() != recording:
                raise ValueError("Staged track did not retain the Recording ID")
            if compilation._mbids(verified.artist_mbid) != compilation._mbids(
                artists
            ):
                raise ValueError("Staged track did not retain the Artist ID")
            os.replace(staged, path)
        finally:
            staged.unlink(missing_ok=True)

    def _comp_target(self, payload):
        artist_id = str(payload.get("artist_id", "")).strip()
        album_id = str(payload.get("album_id", "")).strip()
        track_key = str(payload.get("track_key", "")).strip()
        selection = {"album_id": album_id, "track_keys": [track_key]}
        if artist_id:
            selection["artist_id"] = artist_id
        normalized = self._validate_comp_payload(selection)
        row = self.album_row(normalized["album_id"])
        selected = self.indexed_tracks(row["album_key"], [track_key])
        record = selected[0] if selected else None
        if record is None:
            raise KeyError("COMP track not found")
        path, folder = Path(record["path"]), Path(row["path"])
        if (
            not inside(folder, self.runtime.music)
            or not inside(path, folder)
            or not path.is_file()
        ):
            raise ValueError("COMP track is outside the configured music library")
        return normalized, row, record, path

    def _prepared_comp_directory(self):
        manager = getattr(self.runtime, "temporary_files", None)
        if manager is not None:
            return manager.session_workspace("prepared-comp")
        # Compatibility for reduced fixtures which predate Runtime ownership.
        directory = self.runtime.cache / "prepared-comp"
        directory.mkdir(parents=True, exist_ok=True)
        return directory

    def _prepared_comp_target(self, payload, normalized):
        return {
            "artist_id": str(normalized.get("artist_id", "")),
            "album_id": str(normalized["album_id"]),
            "track_key": str(payload.get("track_key", "")),
        }

    def _load_prepared_comp(self, payload, normalized):
        prepared_id = str(payload.get("prepared_id", "")).strip().casefold()
        if not prepared_id:
            return None
        if len(prepared_id) != 32 or any(c not in "0123456789abcdef" for c in prepared_id):
            raise ValueError("Invalid prepared COMP thumbnail identity")
        directory = self._prepared_comp_directory()
        image_path = directory / (prepared_id + ".jpg")
        manifest_path = directory / (prepared_id + ".json")
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            body = image_path.read_bytes()
        except (OSError, ValueError, TypeError) as error:
            raise ValueError("Prepared COMP thumbnail is unavailable; prepare it again") from error
        if manifest.get("target") != self._prepared_comp_target(payload, normalized):
            raise ValueError("Prepared COMP thumbnail belongs to a different track")
        digest = hashlib.sha256(body).hexdigest()
        if digest != str(manifest.get("sha256", "")):
            raise ValueError("Prepared COMP thumbnail failed integrity validation")
        supplied = str(payload.get("prepared_sha256", "")).strip().casefold()
        if supplied and supplied != digest:
            raise ValueError("Prepared COMP thumbnail selection changed; prepare it again")
        try:
            with Image.open(BytesIO(body)) as opened:
                width, height = opened.size
                image_format = str(opened.format or "").casefold()
        except OSError as error:
            raise ValueError("Prepared COMP thumbnail is not a supported image") from error
        side = int(self.runtime.cfg.get("output", {}).get("embedded_thumbnail_size", 1200))
        if (width, height) != (side, side) or image_format != "jpeg":
            raise ValueError("Prepared COMP thumbnail no longer matches the configured size")
        return body, {"width": width, "height": height}, "jpeg", manifest

    def _remove_prepared_comp(self, prepared_id):
        token = str(prepared_id or "").strip().casefold()
        if len(token) != 32 or any(c not in "0123456789abcdef" for c in token):
            return
        directory = self._prepared_comp_directory()
        (directory / (token + ".jpg")).unlink(missing_ok=True)
        (directory / (token + ".json")).unlink(missing_ok=True)

    def _prepare_comp_embedded(self, payload, normalized=None):
        if payload.get("prepared_id"):
            normalized = normalized or self._comp_target(payload)[0]
            prepared = self._load_prepared_comp(payload, normalized)
            if prepared is not None:
                return prepared
        rendered = self.preview(dict(payload, kind="comp"))
        source = self.catalog.image_bytes(rendered["image"])
        side = int(
            self.runtime.cfg.get("output", {}).get("embedded_thumbnail_size", 1200)
        )
        with Image.open(BytesIO(source)) as opened:
            image = imaging.prepare_embedded_thumbnail(opened, side)
            body = imaging.encode(image, "jpeg")
        return body, {"width": side, "height": side}, "jpeg", rendered

    def prepare_comp_thumbnail(self, payload):
        normalized, row, record, _ = self._comp_target(payload)
        fresh = dict(payload)
        fresh.pop("prepared_id", None)
        fresh.pop("prepared_sha256", None)
        body, info, image_format, rendered = self._prepare_comp_embedded(
            fresh, normalized
        )
        prepared_id = uuid4().hex
        digest = hashlib.sha256(body).hexdigest()
        manifest = {
            "version": 1,
            "prepared_id": prepared_id,
            "sha256": digest,
            "format": image_format,
            "width": info["width"],
            "height": info["height"],
            "bytes": len(body),
            "upscaled": bool(rendered.get("upscaled")),
            "target": self._prepared_comp_target(payload, normalized),
        }
        directory = self._prepared_comp_directory()
        image_path = directory / (prepared_id + ".jpg")
        manifest_path = directory / (prepared_id + ".json")
        descriptor, temporary = tempfile.mkstemp(
            prefix=".prepared-", suffix=".jpg", dir=directory
        )
        try:
            with os.fdopen(descriptor, "wb") as handle:
                handle.write(body)
            os.replace(temporary, image_path)
        finally:
            Path(temporary).unlink(missing_ok=True)
        descriptor, temporary = tempfile.mkstemp(
            prefix=".prepared-", suffix=".json", dir=directory
        )
        try:
            with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
                json.dump(manifest, handle, sort_keys=True)
            os.replace(temporary, manifest_path)
        finally:
            Path(temporary).unlink(missing_ok=True)
        self.audit(
            "prepare-comp-thumbnail",
            record["track_key"],
            {"album_id": row["album_key"], "side": info["width"], "sha256": digest},
        )
        return {
            **manifest,
            "image": self.catalog.image(body),
            "upscaled": bool(manifest.get("upscaled")),
            "audio_written": False,
        }

    @staticmethod
    def _verify_comp_embedded(track, body, info):
        """Read back one queued COMP write before completion can be recorded."""
        raw_path = getattr(track, "path", None)
        if raw_path is None and hasattr(track, "get"):
            raw_path = track.get("path")
        path = Path(raw_path)
        verified = embedded_track_artwork_bytes(path)
        if (
            not verified
            or hashlib.sha256(verified).digest() != hashlib.sha256(body).digest()
        ):
            raise scan.QueuedArtworkVerificationError(
                "Embedded COMP artwork verification failed for " + path.name
            )
        expected = (int(info.get("width", 0)), int(info.get("height", 0)))
        if expected[0] <= 0 or expected[0] != expected[1]:
            raise scan.QueuedArtworkVerificationError(
                "Preferred COMP artwork dimensions are not square"
            )
        try:
            with Image.open(BytesIO(verified)) as opened:
                if opened.size != expected:
                    raise scan.QueuedArtworkVerificationError(
                        "Embedded COMP artwork did not verify at the preferred square size"
                    )
        except OSError as error:
            raise scan.QueuedArtworkVerificationError(
                "Embedded COMP artwork readback is not a supported image"
            ) from error
        return True

    def _comp_track_authority_validated(self, path):
        """Validate the file's current Recording/Artist relationship."""
        try:
            track = core.read_track(Path(path))
            options = core.compilation_credential_options(
                self.runtime.config_file, self.runtime.cfg
            )
            resolution = core.compilation_resolve_interactive_identity(
                core.Http(),
                self.runtime.config_file,
                self.runtime.cfg,
                track,
                options,
            )
        except Exception as error:
            core.debug_log(
                "compilation.direct.authority_validation_failed "
                f"file={str(path)!r} error={type(error).__name__!r}"
            )
            return False
        return bool(
            getattr(resolution, "identity_status", "") == "valid"
            and getattr(resolution, "recording_status", "") == "valid"
            and getattr(resolution, "artist_status", "") == "valid"
        )

    def _record_comp_track_artwork(
        self, row, record, body, outcome, *, authority_validated
    ):
        now = tags.utc_now()
        track_path = str(record["path"])
        recording = core.valid_mbid(
            str(record.get("musicbrainz_recordingid") or "")
        )
        artists = sorted(
            compilation._mbids(record.get("musicbrainz_artistid") or "")
        )
        authoritative = bool(authority_validated and recording and artists)
        with closing(self.runtime.database.connect()) as db, db:
            if authoritative:
                db.execute(
                    "INSERT INTO compilation_track_artwork"
                    "(track_path,recording_mbid,artist_mbid,source_kind,source_locator,"
                    "release_mbid,artwork_sha256,outcome,applied_at,splined_version) "
                    "VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(track_path) DO UPDATE SET "
                    "recording_mbid=excluded.recording_mbid,artist_mbid=excluded.artist_mbid,"
                    "source_kind=excluded.source_kind,source_locator=excluded.source_locator,"
                    "release_mbid=excluded.release_mbid,artwork_sha256=excluded.artwork_sha256,"
                    "outcome=excluded.outcome,applied_at=excluded.applied_at,"
                    "splined_version=excluded.splined_version",
                    (
                        track_path,
                        recording,
                        "; ".join(artists),
                        "webgui-selected-artwork",
                        "comp-report",
                        None,
                        hashlib.sha256(body).hexdigest(),
                        outcome,
                        now,
                        core.VERSION,
                    ),
                )
            else:
                # A verified artwork write is not authoritative completion.
                # Remove any stale row so refresh cannot leave the track green.
                db.execute(
                    "DELETE FROM compilation_track_artwork WHERE track_path=?",
                    (track_path,),
                )
            total = int(
                db.execute(
                    "SELECT count(*) FROM tracks WHERE album_key=?",
                    (row["album_key"],),
                ).fetchone()[0]
            )
            completed = int(
                db.execute(
                    "SELECT count(*) FROM tracks JOIN compilation_track_artwork "
                    "ON lower(compilation_track_artwork.track_path)=lower(tracks.path) "
                    "WHERE tracks.album_key=? AND compilation_track_artwork.outcome "
                    "IN ('embedded-replaced','embedded-existing')",
                    (row["album_key"],),
                ).fetchone()[0]
            )
            db.execute(
                "INSERT INTO compilation_album_progress"
                "(album_path,total_tracks,completed_tracks,status,updated_at,splined_version) "
                "VALUES(?,?,?,?,?,?) ON CONFLICT(album_path) DO UPDATE SET "
                "total_tracks=excluded.total_tracks,completed_tracks=excluded.completed_tracks,"
                "status=excluded.status,updated_at=excluded.updated_at,"
                "splined_version=excluded.splined_version",
                (
                    str(row["path"]),
                    total,
                    completed,
                    "complete" if total and completed >= total else "incomplete",
                    now,
                    core.VERSION,
                ),
            )
        return authoritative

    @serialized
    def embed_comp_track(self, payload):
        self.runtime.require_write()
        artist_id = str(payload.get("artist_id", "")).strip()
        track_key = str(payload.get("track_key", "")).strip()
        normalized, row, record, path = self._comp_target(payload)
        body, info, image_format, rendered = self._prepare_comp_embedded(
            payload, normalized
        )
        original = embedded_track_artwork_bytes(path)
        outcome = "embedded-existing" if original == body else "embedded-replaced"
        backup = None
        if original != body:
            backup_root = self.runtime.cache / "embedded-backups"
            backup_root.mkdir(parents=True, exist_ok=True)
            if original:
                backup = backup_root / (
                    hashlib.sha256(
                        (str(path) + hashlib.sha256(original).hexdigest()).encode()
                    ).hexdigest()
                    + ".image"
                )
                if not backup.exists():
                    backup.write_bytes(original)
                mark_conversion_backup(backup, committed=False)
            compilation.replace_embedded_artwork(path, body, image_format, info)
        self._verify_comp_embedded(record, body, info)
        verified_body = embedded_track_artwork_bytes(path)
        refreshed_rows = self.hydrate_selected_tracks(
            row["album_key"], [track_key]
        )
        refreshed = refreshed_rows[0] if refreshed_rows else record
        authority_validated = self._comp_track_authority_validated(path)
        completed = self._record_comp_track_artwork(
            row,
            refreshed,
            body,
            outcome,
            authority_validated=authority_validated,
        )
        self.refresh_embedding_stats(row)
        self.audit(
            "embed-comp-track",
            track_key,
            {
                "artist_id": artist_id,
                "album_id": row["album_key"],
                "side": info["width"],
                "outcome": outcome,
                "completion_recorded": completed,
            },
        )
        artwork_sha256 = hashlib.sha256(verified_body).hexdigest()
        coordination = self.record_comp_track_write(
            row, refreshed, "embed-comp-track", artwork_sha256
        )
        if backup is not None:
            mark_conversion_backup(backup, committed=True)
            discard_committed_conversion(backup)
        self._remove_prepared_comp(payload.get("prepared_id"))
        return {
            "album_id": row["album_key"],
            "track_key": track_key,
            "file": path.name,
            "status": "green" if completed else "blue",
            "completed": completed,
            "authority_required": not completed,
            "width": info["width"],
            "height": info["height"],
            "bytes": len(body),
            "image": self.catalog.image(verified_body),
            "artwork_sha256": artwork_sha256,
            "upscaled": bool(rendered.get("upscaled")),
            "outcome": outcome,
            "coordination": coordination,
        }

    @serialized
    def embed_album(self, payload, progress=None, cancelled=None):
        self.runtime.require_write()
        if payload.get("prepared_file"):
            row, files, source = self.validate_staged_album(payload)
        else:
            row = self.album_row(str(payload.get("album_id", "")))
            if not core.valid_mbid(str(row["musicbrainz_albumid"] or "")):
                raise ValueError(
                    "Album Track Embedding requires a valid MusicBrainz Release ID; "
                    "COMP albums require individual track processing"
                )
            files = self.album_files(row)
            if not files:
                raise ValueError("Album has no supported tracks")
            rendered = self.preview(dict(payload, kind="cover"))
            source = self.catalog.image_bytes(rendered["image"])
        side = int(
            payload.get("embedded_side")
            or self.runtime.cfg.get("output", {}).get(
                "embedded_thumbnail_size", 1200
            )
        )
        if payload.get("prepared_kind") == "thumbnail-v1":
            with Image.open(BytesIO(source)) as image:
                if image.size != (side, side):
                    raise ValueError(
                        "Staged artwork does not match its preferred embedded size"
                    )
            body = source
        else:
            # Backward compatibility for jobs staged before thumbnail snapshots
            # were persisted separately from the full-resolution cover.
            with Image.open(BytesIO(source)) as image:
                image = imaging.prepare_embedded_thumbnail(image, side)
                body = imaging.encode(image, "jpeg")
        self.runtime.cache.mkdir(parents=True, exist_ok=True)
        backup_root = self.runtime.cache / "embedded-backups"
        backup_root.mkdir(exist_ok=True)
        backups = []
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
            if original:
                mark_conversion_backup(backup, committed=False)
                backups.append(backup)
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
        for path in files:
            verified = embedded_track_artwork_bytes(path)
            if not verified or hashlib.sha256(verified).digest() != hashlib.sha256(
                body
            ).digest():
                raise ValueError(
                    "Completed Album artwork verification failed for " + path.name
                )
            with Image.open(BytesIO(verified)) as opened:
                if opened.size != (side, side):
                    raise ValueError(
                        "Completed Album artwork has the wrong preferred size for "
                        + path.name
                    )
        self.refresh_album(row["album_key"], source="webgui-embed")
        self.refresh_embedding_stats(row)
        self.audit(
            "embed-album", row["album_key"], {"tracks": len(files), "side": side}
        )
        coordination = self.record_album_write(row, "embed-album")
        for backup in backups:
            mark_conversion_backup(backup, committed=True)
            discard_committed_conversion(backup)
        return {
            "tracks": len(files),
            "side": side,
            "verified": True,
            "coordination": coordination,
        }

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
        with _temporary_workspace(r, "web-candidates") as cache:
            found, download_warnings = core.download_candidates(
                http, refs, sources, cache, r.cfg, clean_first=False
            )
            warnings.extend(download_warnings)
            strict_policy.apply(found, r.cfg, core.source_policy)
            format_order = core.formats(r.cfg)
            suggested = core.select_best(found, r.cfg, format_order)
            candidates = []
            for candidate in found:
                target_format = core.target_format_for_candidate(candidate, format_order)
                projected = core.project_candidate(candidate, r.cfg, target_format)
                policy = core.source_policy(r.cfg, candidate.source)
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
                        "source_range_class": core.classify_range(
                            candidate.short_side, r.cfg
                        ),
                        "projected_width": projected["width"],
                        "projected_height": projected["height"],
                        "recommended": candidate is suggested,
                        "upscaled": projected["upscaled"],
                        "policy_status": projected["policy_status"],
                        "policy_reason": projected["policy_reason"],
                        "strict_override_active": policy["strict_override"],
                        "strict_status": candidate.strict_status,
                        "strict_reason": candidate.strict_reason,
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
        album_mbid = str(row["musicbrainz_albumid"] or "").strip()
        if not core.valid_mbid(album_mbid):
            raise ValueError(
                "Album Track Embedding requires a valid MusicBrainz Release ID; "
                "COMP Fallback Albums use the separate track-scoped EMBED Comp workflow"
            )
        files = self.album_files(row)
        if not files:
            raise ValueError("Album has no supported tracks to stage")
        # EMBED Batch uses the same configured canonical-cover transaction as
        # WRITE Cover. Its verified readback—not whichever cover.* happens to
        # be discovered later—is snapshotted for every track in this job.
        result = self.write_cover(payload)
        authoritative_cover = Path(row["path"]) / str(result["file"])
        cover_body = authoritative_cover.read_bytes()
        if hashlib.sha256(cover_body).hexdigest() != result["sha256"]:
            raise ValueError("The written Album cover changed before embedding was staged")
        side = int(
            self.runtime.cfg.get("output", {}).get(
                "embedded_thumbnail_size", 1200
            )
        )
        with Image.open(BytesIO(cover_body)) as opened:
            thumbnail = imaging.prepare_embedded_thumbnail(opened, side)
            prepared_body = imaging.encode(thumbnail, "jpeg")
        directory = self.runtime.cache / "staged-artwork"
        directory.mkdir(parents=True, exist_ok=True)
        cover_suffix = authoritative_cover.suffix.casefold()
        if cover_suffix not in {".jpg", ".jpeg", ".png"}:
            raise ValueError("The verified Album presentation is not a static cover")
        cover_name = uuid4().hex + cover_suffix
        prepared_name = uuid4().hex + ".jpg"
        try:
            self._atomic_artwork(directory / cover_name, cover_body)
            self._atomic_artwork(directory / prepared_name, prepared_body)
        except Exception:
            (directory / cover_name).unlink(missing_ok=True)
            (directory / prepared_name).unlink(missing_ok=True)
            raise
        return {
            "album_id": row["album_key"],
            "album_path": row["path"],
            "album_mbid": album_mbid,
            "library_root": str(self.runtime.music),
            "name": row["album_name"],
            "artist": row["artist_name"],
            # Persist filenames and hashes, never a process-signed image URL.
            # Queue reads issue a fresh bounded handle for the current server.
            "cover": None,
            "cover_width": result["width"],
            "cover_height": result["height"],
            "embedded_side": side,
            "embedded_tracks": int(row["embedded_tracks"] or 0),
            "prepared_file": prepared_name,
            "prepared_kind": "thumbnail-v1",
            "sha256": hashlib.sha256(prepared_body).hexdigest(),
            "cover_file": cover_name,
            "cover_sha256": hashlib.sha256(cover_body).hexdigest(),
            "preferred_size": self.album_has_preferred_artwork(files, side),
            "tracks": len(files),
        }

    def _staged_artwork_bytes(self, payload, file_field, hash_field):
        name = str(payload.get(file_field, "")).strip()
        directory = self.runtime.cache / "staged-artwork"
        path = directory / name
        if not name or path.name != name or not inside(path, directory):
            raise ValueError("Staged artwork is unavailable")
        try:
            body = path.read_bytes()
        except OSError as error:
            raise ValueError("Staged artwork is unavailable") from error
        expected = str(payload.get(hash_field, "")).strip()
        if not expected or hashlib.sha256(body).hexdigest() != expected:
            raise ValueError("Staged artwork failed verification")
        return path, body

    @staticmethod
    def album_has_preferred_artwork(files, side):
        if not files:
            return False
        for path in files:
            body = embedded_track_artwork_bytes(path)
            if not body:
                return False
            try:
                with Image.open(BytesIO(body)) as opened:
                    if opened.size != (side, side):
                        return False
            except (OSError, ValueError):
                return False
        return True

    def validate_staged_album(self, payload):
        album_id = str(payload.get("album_id", "")).strip()
        if payload.get("library_root") != str(self.runtime.music):
            raise ValueError("The staged music library changed; stage this Album again")
        try:
            row = self.album_row(album_id)
        except KeyError as error:
            raise ValueError(
                "The staged Album is no longer indexed; restore it or remove this job"
            ) from error
        current_mbid = str(row["musicbrainz_albumid"] or "").strip()
        if not current_mbid:
            raise ValueError(
                "The staged Album is now COMP Fallback; remove this Album job "
                "and use track-scoped COMP work"
            )
        if not core.valid_mbid(current_mbid):
            raise ValueError(
                "The staged Album no longer has a valid MusicBrainz Release ID; "
                "remove this Album job and use track-scoped COMP work when applicable"
            )
        staged_mbid = str(payload.get("album_mbid", "")).strip()
        if not core.valid_mbid(staged_mbid):
            raise ValueError(
                "The staged Album identity is invalid; remove it and stage the current Album"
            )
        if current_mbid.casefold() != staged_mbid.casefold():
            raise ValueError(
                "The staged Album identity changed; remove it and stage the current Album"
            )
        if payload.get("album_path") != row["path"]:
            raise ValueError(
                "The staged Album location changed; restore it or stage the relocated Album again"
            )
        files = self.album_files(row)
        if not files:
            raise ValueError("The staged Album no longer has supported tracks")
        _, source = self._staged_artwork_bytes(payload, "prepared_file", "sha256")
        side = int(payload.get("embedded_side") or 0)
        if not 300 <= side <= 4000:
            raise ValueError("The staged embedded-thumbnail preference is invalid")
        if payload.get("prepared_kind") == "thumbnail-v1":
            try:
                with Image.open(BytesIO(source)) as opened:
                    if opened.size != (side, side):
                        raise ValueError(
                            "Staged artwork does not match its preferred embedded size"
                        )
            except OSError as error:
                raise ValueError("Staged artwork is unreadable") from error
        return row, files, source

    def staged_album_projection(self, payload, *, validate=True):
        projected = dict(payload)
        projected["cover"] = None
        try:
            if payload.get("cover_file"):
                preview, _ = self._staged_artwork_bytes(
                    payload, "cover_file", "cover_sha256"
                )
            else:
                preview, _ = self._staged_artwork_bytes(
                    payload, "prepared_file", "sha256"
                )
            projected["cover"] = self.catalog.image(preview)
            if validate:
                self.validate_staged_album(payload)
            projected["staged_valid"] = True
            projected["staged_error"] = ""
        except (KeyError, OSError, ValueError) as error:
            projected["staged_valid"] = not validate
            projected["staged_error"] = str(error) if validate else ""
        return projected

    def remove_staged_artwork(self, payload):
        directory = self.runtime.cache / "staged-artwork"
        for field in ("prepared_file", "cover_file"):
            name = str(payload.get(field, "")).strip()
            path = directory / name
            if name and path.name == name and inside(path, directory):
                path.unlink(missing_ok=True)

    def _validate_comp_payload(self, payload):
        """Resolve an explicit COMP selection without widening its scope."""
        album_id = str(payload.get("album_id", "")).strip()
        row = self.album_row(album_id)
        if str(row["musicbrainz_albumid"] or "").strip():
            raise ValueError("This album is not a COMP fallback album")

        requested = payload.get("track_keys")
        if (
            not isinstance(requested, list)
            or not requested
            or any(not isinstance(key, str) or not key for key in requested)
            or len(set(requested)) != len(requested)
        ):
            raise ValueError("Choose explicit indexed COMP tracks")

        records = self.indexed_tracks(album_id, requested)
        indexed = {item["track_key"] for item in records}
        if any(key not in indexed for key in requested):
            raise ValueError("Choose indexed COMP tracks")

        normalized = dict(payload)
        normalized.update(
            album_id=album_id,
            track_keys=list(requested),
            name=row["album_name"],
        )
        artist_id = str(payload.get("artist_id", "")).strip()
        if artist_id:
            report = self.catalog.artist_comp_report(artist_id)
            group = next((item for item in report if item["id"] == album_id), None)
            scoped = {track["key"] for track in group["tracks"]} if group else set()
            if any(key not in scoped for key in requested):
                raise ValueError("Track is not in this Artist's COMP report")
            normalized["artist_id"] = artist_id
            normalized["artist_name"] = self.catalog.artist(artist_id)["name"]
        else:
            normalized.pop("artist_id", None)
            normalized.pop("artist_name", None)
        return normalized

    @serialized
    def validate_comp_payload(self, payload):
        return self._validate_comp_payload(payload)

    @serialized
    def run_comp(self, payload, adapter):
        self.runtime.require_write()
        r = self.runtime
        payload = self.validate_comp_payload(payload)
        row = self.album_row(payload["album_id"])
        scope_artist_id = str(payload.get("artist_id", ""))
        adapter.context = {
            "album_id": row["album_key"],
            "album": row["album_name"],
            "artist": row["artist_name"],
            "scope_artist_id": scope_artist_id,
            "scope_artist": payload.get("artist_name", ""),
        }
        requested = payload["track_keys"]
        records = self.indexed_tracks(row["album_key"], requested)
        allowed = {item["track_key"]: item["path"] for item in records}
        selected_files = [Path(allowed[key]) for key in requested]
        progress = getattr(adapter, "progress", None)
        if callable(progress):
            progress({
                "completed": 0,
                "total": len(requested),
                "message": (
                    f"Loading {len(requested)} explicitly selected COMP "
                    f"track{'s' if len(requested) != 1 else ''} · "
                    "no other compilation tracks are included"
                ),
            })
        tracks = [core.read_track(path) for path in selected_files]
        keys_by_path = {str(Path(allowed[key])): key for key in requested}
        positions_by_path = {
            str(Path(allowed[key])): position
            for position, key in enumerate(requested, 1)
        }

        def update_track_context(track):
            position = positions_by_path.get(str(track.path), 1)
            adapter.context.update(
                track_key=keys_by_path.get(str(track.path), ""),
                track_file=track.path.name,
                track_title=str(track.title or track.path.name),
                track_artist=str(track.artist or ""),
            )
            track_progress = getattr(adapter, "progress", None)
            if callable(track_progress):
                focused_artist = str(
                    payload.get("artist_name") or track.artist or "Unknown Artist"
                )
                track_progress({
                    "completed": position - 1,
                    "total": len(requested),
                    "message": (
                        "MusicBrainz identity lookup · "
                        f"{focused_artist} — {track.title or track.path.name} · "
                        f"track {position} of {len(requested)}"
                    ),
                })

        def prepare_selected_artwork(request):
            request = dict(
                request,
                artist_id=scope_artist_id,
                album_id=row["album_key"],
                track_key=str(adapter.context.get("track_key", "")),
            )
            normalized = {
                "artist_id": scope_artist_id,
                "album_id": row["album_key"],
            }
            try:
                body, info, image_format, _ = self._prepare_comp_embedded(
                    request, normalized
                )
            except ValueError:
                # A prepared thumbnail is integrity-bound to this exact track
                # and digest. Never hide a stale or tampered prepared artifact
                # by silently replacing it with a remote fallback URL.
                if request.get("prepared_id"):
                    raise
                fallback = str(request.get("fallback_image") or "").strip()
                if not fallback or fallback == str(request.get("image") or "").strip():
                    raise
                request["image"] = fallback
                body, info, image_format, _ = self._prepare_comp_embedded(
                    request, normalized
                )
            adapter.context["prepared_id"] = str(request.get("prepared_id", ""))
            return body, info, image_format

        def verify_selected_artwork(track, body, info):
            result = self._verify_comp_embedded(track, body, info)
            self._remove_prepared_comp(adapter.context.pop("prepared_id", ""))
            return result

        def persist_track_authority(track, recording_mbid, artist_mbids):
            self._persist_comp_track_authority(
                track, recording_mbid, artist_mbids
            )

        def publish_verified_track_write(track, artwork):
            """Publish the queued COMP path after read-back verification.

            Direct Selected Artwork writes already call ``embed_comp_track``.
            The queued Artist/Fallback workflow writes through
            ``run_manual_compilation_album`` instead, so it previously updated
            the PostgreSQL completion ledger without emitting the cross-client
            coordination event.  Keep the event at the verified write boundary
            and use the indexed track identity selected for this exact job.
            """
            path = str(Path(track.path))
            track_key = keys_by_path.get(path, "")
            refreshed = self.hydrate_selected_tracks(
                row["album_key"], [track_key]
            )
            record = refreshed[0] if refreshed else {
                "track_key": track_key,
                "path": path,
            }
            return self.record_comp_track_write(
                row,
                record,
                "embed-comp-track",
                hashlib.sha256(artwork).hexdigest(),
            )

        discovery_state = getattr(self, "_compilation_discovery_state", None)
        if discovery_state is None:
            discovery_state = {}
            self._compilation_discovery_state = discovery_state
        summary = core.Summary()
        with _temporary_workspace(r, "comp-work") as cache:
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
                state=discovery_state,
                target_track_paths={allowed[key] for key in requested},
                track_context=update_track_context,
                prepare_selected_artwork=prepare_selected_artwork,
                persist_track_authority=persist_track_authority,
                verify_embedded_artwork=verify_selected_artwork,
                publish_verified_write=publish_verified_track_write,
                musicbrainz_results_first=True,
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

    def musicbrainz_artwork_preview(self, payload):
        """Import one explicitly identified CAA image into the bounded UI cache."""
        release_id = core.valid_mbid(str(payload.get("release_id") or ""))
        group_id = core.valid_mbid(str(payload.get("release_group_id") or ""))
        if not release_id and not group_id:
            raise ValueError("Choose a valid MusicBrainz release")
        locations = []
        if release_id:
            locations.append(("release", release_id))
        if group_id:
            locations.append(("release-group", group_id))
        http = core.Http()
        for entity, identity in locations:
            response = http.get(
                f"https://coverartarchive.org/{entity}/{identity}/front",
                headers={"Accept": "image/*"},
            )
            if int(getattr(response, "status_code", 500)) >= 400:
                continue
            body = bytes(getattr(response, "content", b"") or b"")
            if not body or len(body) > 25 * 1024 * 1024:
                continue
            try:
                with Image.open(BytesIO(body)) as opened:
                    width, height = opened.size
                    if width * height > 64 * 1024 * 1024:
                        continue
                    opened.verify()
            except (OSError, ValueError):
                continue
            return {
                "image": self.catalog.image(body),
                "width": int(width),
                "height": int(height),
                "bytes": len(body),
                "source": "MusicBrainz / Cover Art Archive",
                "entity": entity,
                "identity": identity,
            }
        raise ValueError("MusicBrainz artwork is unavailable for this release")

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
