"""Safe, incremental SQLite-to-PostgreSQL reconciliation.

SQLite is always opened read-only.  PostgreSQL remains authoritative: this
module inserts missing state, applies only demonstrably newer safe fields, and
retains every conflict in ``salite_diff`` for authenticated review.
"""

from __future__ import annotations

from contextlib import closing
from datetime import datetime, timedelta, timezone
from hashlib import sha256
from pathlib import Path
from threading import Event, Lock, Thread
from typing import Any, Callable
from uuid import uuid4
import json
import re
import sqlite3

from psycopg import sql
from psycopg.types.json import Jsonb

from webgui.audit import sanitize_text


SQLITE_SCHEMA_VERSION = 2
SOURCE_SUFFIXES = {".db", ".sqlite", ".sqlite3"}
DERIVED_COLUMNS = {
    "artists": {
        "status",
        "album_count",
        "unprocessed_count",
        "processed_count",
        "bypassed_count",
        "timeout_count",
    },
    "albums": {"status"},
}
TABLE_KEYS = {
    "artists": ("artist_key",),
    "albums": ("album_key",),
    "tracks": ("track_key",),
    "recording_release_lookups": ("recording_mbid",),
    "recording_release_candidates": ("recording_mbid", "release_mbid"),
    "compilation_track_artwork": ("track_path",),
    "compilation_album_progress": ("album_path",),
    "cache_entries": ("cache_key",),
}
TABLE_TIMESTAMPS = {
    "artists": "updated_at",
    "albums": "updated_at",
    "tracks": "updated_at",
    "recording_release_lookups": "fetched_at",
    "compilation_track_artwork": "applied_at",
    "cache_entries": "updated_at",
}
SAFE_INSERT_TABLES = {
    "artists",
    "albums",
    "tracks",
    "recording_release_lookups",
    "recording_release_candidates",
    "compilation_track_artwork",
    "cache_history",
}
SAFE_UPDATE_TABLES = {
    "artists",
    "albums",
    "tracks",
    "recording_release_lookups",
    "compilation_track_artwork",
}
def _canonical(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), default=str)


def _digest(value: Any) -> str:
    return sha256(_canonical(value).encode("utf-8", "surrogateescape")).hexdigest()


def _identity(value: Any) -> str:
    return re.sub(r"[^a-z0-9]+", "", str(value or "").casefold())


def _timestamp(value: Any) -> datetime | None:
    text = str(value or "").strip()
    if not text:
        return None
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
        return parsed.replace(tzinfo=parsed.tzinfo or timezone.utc).astimezone(timezone.utc)
    except ValueError:
        return None


def classify_difference(table: str, source: dict[str, Any], target: dict[str, Any] | None) -> str:
    """Return the only automatic resolution that is safe for this difference."""
    if table == "compilation_album_progress":
        return "derived"
    if target is None:
        return "safe" if table in SAFE_INSERT_TABLES else "conflict"
    if table not in SAFE_UPDATE_TABLES:
        return "conflict"
    field = TABLE_TIMESTAMPS.get(table)
    source_time = _timestamp(source.get(field)) if field else None
    target_time = _timestamp(target.get(field)) if field else None
    return "safe" if source_time and (not target_time or source_time > target_time) else "conflict"


def map_library_path(value: Any, source_root: str, target_root: str) -> str:
    """Map Windows or POSIX snapshot paths without assuming equal roots."""
    text = str(value or "").strip()
    if not text:
        return ""
    source = str(source_root or "").strip().replace("\\", "/").rstrip("/")
    target = str(target_root or "").strip()
    normalized = text.replace("\\", "/").rstrip("/")
    if not source or not target:
        raise ValueError("Source and target library roots are required for path reconciliation")
    source_folded = source.casefold()
    normalized_folded = normalized.casefold()
    if normalized_folded == source_folded:
        relative = ""
    elif normalized_folded.startswith(source_folded + "/"):
        relative = normalized[len(source) + 1 :]
    else:
        raise ValueError(f"SQLite path is outside the mapped source library: {text}")
    parts = [part for part in relative.split("/") if part not in {"", "."}]
    if any(part == ".." for part in parts):
        raise ValueError("SQLite path mapping cannot traverse outside the target library")
    return str(Path(target).joinpath(*parts))


class SQLiteReconciler:
    """Compare, record, and explicitly apply safe SQLite differences."""

    def __init__(
        self,
        database,
        allowed_root: Path,
        target_root: Path,
        audit: Callable[..., Any] | None = None,
        *,
        start_scheduler: bool = True,
    ):
        self.database = database
        self.allowed_root = allowed_root.resolve()
        self.target_root = target_root.resolve()
        self.audit = audit
        self._operation_lock = Lock()
        self._stop = Event()
        self._thread = None
        if start_scheduler:
            self._thread = Thread(target=self._scheduler, name="sqlite-reconciliation", daemon=True)
            self._thread.start()

    def close(self) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=3)

    def _event(self, action: str, details: dict[str, Any], **fields: Any) -> None:
        if self.audit:
            self.audit(action, "SQLite reconciliation", details, **fields)

    def _source(self, value: Any) -> Path:
        selected = Path(str(value or "")).expanduser()
        if selected.is_symlink():
            raise ValueError("SQLite reconciliation does not follow snapshot symlinks")
        path = selected.resolve(strict=True)
        if not path.is_file() or path.suffix.casefold() not in SOURCE_SUFFIXES:
            raise ValueError("Choose a .db, .sqlite, or .sqlite3 snapshot from the authorized cache directory")
        try:
            path.relative_to(self.allowed_root)
        except ValueError as error:
            raise PermissionError("SQLite reconciliation sources must be inside the configured persistent cache") from error
        return path

    def sources(self) -> list[dict[str, Any]]:
        result = []
        if not self.allowed_root.is_dir():
            return result
        for path in self.allowed_root.rglob("*"):
            if len(result) >= 200:
                break
            try:
                resolved = path.resolve(strict=True)
                if path.is_symlink() or not resolved.is_file() or resolved.suffix.casefold() not in SOURCE_SUFFIXES:
                    continue
                resolved.relative_to(self.allowed_root)
                stat = resolved.stat()
            except (OSError, ValueError):
                continue
            result.append(
                {
                    "path": str(resolved),
                    "label": str(resolved.relative_to(self.allowed_root)),
                    "bytes": int(stat.st_size),
                    "modified_at": datetime.fromtimestamp(stat.st_mtime, timezone.utc).isoformat(),
                }
            )
        return sorted(result, key=lambda item: item["label"].casefold())

    @staticmethod
    def _fingerprint(path: Path) -> str:
        digest = sha256()
        for candidate in (path, Path(str(path) + "-wal"), Path(str(path) + "-shm")):
            if not candidate.is_file():
                continue
            digest.update(candidate.name.encode("utf-8", "surrogateescape"))
            digest.update(b"\0")
            with candidate.open("rb") as handle:
                while chunk := handle.read(1024 * 1024):
                    digest.update(chunk)
        return digest.hexdigest()

    @staticmethod
    def _source_tables(db: sqlite3.Connection) -> set[str]:
        return {
            str(row[0])
            for row in db.execute("SELECT name FROM sqlite_master WHERE type='table'")
        }

    @staticmethod
    def _read_source(db: sqlite3.Connection, table: str, available: set[str]) -> list[dict[str, Any]]:
        if table not in available:
            return []
        return [dict(row) for row in db.execute(f'SELECT * FROM "{table}"')]

    @staticmethod
    def _target_columns(raw, table: str) -> list[str]:
        return [
            row["column_name"]
            for row in raw.execute(
                "SELECT column_name FROM information_schema.columns "
                "WHERE table_schema=current_schema() AND table_name=%s ORDER BY ordinal_position",
                (table,),
            )
        ]

    @staticmethod
    def _target_rows(raw, table: str) -> list[dict[str, Any]]:
        statement = sql.SQL("SELECT * FROM {}").format(sql.Identifier(table))
        return [dict(row) for row in raw.execute(statement)]

    @staticmethod
    def _key(table: str, row: dict[str, Any]) -> str:
        if table == "cache_history":
            return _digest({key: value for key, value in row.items() if key != "history_id"})
        return _canonical([row.get(name) for name in TABLE_KEYS[table]])

    @staticmethod
    def _projection(table: str, row: dict[str, Any], columns: list[str]) -> dict[str, Any]:
        ignored = DERIVED_COLUMNS.get(table, set())
        return {name: row.get(name) for name in columns if name in row and name not in ignored}

    @staticmethod
    def _map_row(table: str, row: dict[str, Any], source_root: str, target_root: str) -> dict[str, Any]:
        mapped = dict(row)
        path_fields = {
            "artists": ("primary_path",),
            "albums": ("path", "representative_file", "cover_path"),
            "tracks": ("path",),
            "compilation_track_artwork": ("track_path",),
            "compilation_album_progress": ("album_path",),
        }.get(table, ())
        for name in path_fields:
            if mapped.get(name):
                mapped[name] = map_library_path(mapped[name], source_root, target_root)
        return mapped

    @staticmethod
    def _match_maps(table: str, target_rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
        maps: dict[str, dict[str, Any]] = {}
        for row in target_rows:
            keys = TABLE_KEYS.get(table, ())
            if keys:
                maps.setdefault("entity:" + _identity(_canonical([row.get(name) for name in keys])), row)
            for prefix, value in (
                ("key", row.get(TABLE_KEYS.get(table, ("",))[0])),
                ("mbid", row.get("musicbrainz_artistid") or row.get("musicbrainz_albumid")),
                ("path", row.get("path") or row.get("track_path") or row.get("album_path")),
                ("name", row.get("artist_name")),
            ):
                if value:
                    maps.setdefault(prefix + ":" + _identity(value), row)
            if table == "tracks" and row.get("musicbrainz_recordingid") and row.get("album_key"):
                maps.setdefault(
                    "recording:" + _identity(str(row["musicbrainz_recordingid"]) + "\0" + str(row["album_key"])),
                    row,
                )
        return maps

    @staticmethod
    def _find_match(table: str, source: dict[str, Any], maps: dict[str, dict[str, Any]]) -> dict[str, Any] | None:
        keys = TABLE_KEYS.get(table, ())
        values = [
            ("entity", _canonical([source.get(name) for name in keys]) if keys else ""),
            ("key", source.get(TABLE_KEYS.get(table, ("",))[0])),
            ("mbid", source.get("musicbrainz_artistid") or source.get("musicbrainz_albumid")),
            ("path", source.get("path") or source.get("track_path") or source.get("album_path")),
            ("name", source.get("artist_name")),
        ]
        if table == "tracks" and source.get("musicbrainz_recordingid") and source.get("album_key"):
            values.insert(1, ("recording", str(source["musicbrainz_recordingid"]) + "\0" + str(source["album_key"])))
        return next((maps.get(prefix + ":" + _identity(value)) for prefix, value in values if value and maps.get(prefix + ":" + _identity(value))), None)

    def _record_difference(
        self,
        raw,
        *,
        run_id: str,
        fingerprint: str,
        label: str,
        table: str,
        source: dict[str, Any],
        target: dict[str, Any] | None,
        resolution: str,
    ) -> None:
        difference_type = "derived" if resolution == "derived" else "insert" if target is None else "update"
        old_digest = _digest(target) if target is not None else ""
        new_digest = _digest(source)
        outcome = "recalculate" if resolution == "derived" else "pending"
        raw.execute(
            "INSERT INTO salite_diff(run_id,last_run_id,source_fingerprint,source_label,"
            "entity_table,entity_key,difference_type,old_digest,new_digest,source_value,"
            "target_value,resolution,application_outcome) "
            "VALUES(%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s) "
            "ON CONFLICT(source_fingerprint,entity_table,entity_key,difference_type,old_digest,new_digest) "
            "DO UPDATE SET last_run_id=excluded.last_run_id,last_seen_at=now()",
            (
                run_id,
                run_id,
                fingerprint,
                label,
                table,
                self._key(table, source),
                difference_type,
                old_digest,
                new_digest,
                Jsonb(source),
                Jsonb(target or {}),
                resolution,
                outcome,
            ),
        )

    def compare(
        self,
        source_path: Any,
        source_root: Any = "",
        target_root: Any = "",
        *,
        trigger: str = "manual",
    ) -> dict[str, Any]:
        with self._operation_lock:
            source = self._source(source_path)
            target = Path(str(target_root or self.target_root)).resolve()
            if target != self.target_root:
                raise PermissionError("The target mapping must be the configured WebGUI music library")
            before = self._fingerprint(source)
            run_id = uuid4().hex
            label = str(source.relative_to(self.allowed_root))
            counts = {"differences": 0, "safe": 0, "conflicts": 0, "derived": 0}
            self._event(
                "sqlite-reconciliation",
                {"run_id": run_id, "source": label, "trigger": trigger},
                stage="compare",
                request_status="running",
            )
            try:
                uri = source.as_uri() + "?mode=ro"
                with closing(sqlite3.connect(uri, uri=True)) as legacy, legacy, closing(self.database.connect()) as db, db:
                    legacy.row_factory = sqlite3.Row
                    legacy.execute("BEGIN")
                    version = int(legacy.execute("PRAGMA user_version").fetchone()[0])
                    available = self._source_tables(legacy)
                    if version != SQLITE_SCHEMA_VERSION or not {"artists", "albums"}.issubset(available):
                        raise ValueError(
                            f"SQLite schema v{version} is incompatible; reconciliation requires v{SQLITE_SCHEMA_VERSION} with artists and albums"
                        )
                    if not source_root:
                        inventory = self._read_source(legacy, "picker_inventory", available)
                        if inventory:
                            try:
                                source_root = json.loads(str(inventory[0].get("signature_json") or "{}"))["library_root"]
                            except (KeyError, TypeError, ValueError):
                                source_root = ""
                    if not str(source_root or "").strip():
                        raise ValueError("Enter the SQLite snapshot's original library root")

                    raw = db.raw
                    raw.execute("SELECT pg_advisory_xact_lock(741520213)")
                    raw.execute(
                        "INSERT INTO sqlite_reconciliation_runs(run_id,source_fingerprint,source_label,trigger,status) "
                        "VALUES(%s,%s,%s,%s,'running')",
                        (run_id, before, label, sanitize_text(trigger, limit=30)),
                    )
                    table_names = [
                        "artists",
                        "albums",
                        "tracks",
                        "recording_release_lookups",
                        "recording_release_candidates",
                        "compilation_track_artwork",
                        "compilation_album_progress",
                        "cache_entries",
                        "cache_history",
                    ]
                    target_rows = {name: self._target_rows(raw, name) for name in table_names}
                    target_columns = {name: self._target_columns(raw, name) for name in table_names}
                    source_rows = {name: self._read_source(legacy, name, available) for name in table_names}

                    artist_maps = self._match_maps("artists", target_rows["artists"])
                    artist_keys: dict[str, str] = {}
                    projected_artists = []
                    for row in source_rows["artists"]:
                        mapped = self._map_row("artists", row, str(source_root), str(target))
                        match = self._find_match("artists", mapped, artist_maps)
                        source_key = str(row.get("artist_key") or "")
                        if match:
                            mapped["artist_key"] = match["artist_key"]
                        artist_keys[source_key] = str(mapped.get("artist_key") or source_key)
                        projected_artists.append((mapped, match))

                    album_maps = self._match_maps("albums", target_rows["albums"])
                    album_keys: dict[str, str] = {}
                    projected_albums = []
                    for row in source_rows["albums"]:
                        mapped = self._map_row("albums", row, str(source_root), str(target))
                        mapped["artist_key"] = artist_keys.get(str(row.get("artist_key") or ""), row.get("artist_key"))
                        match = self._find_match("albums", mapped, album_maps)
                        source_key = str(row.get("album_key") or "")
                        if match:
                            mapped["album_key"] = match["album_key"]
                        album_keys[source_key] = str(mapped.get("album_key") or source_key)
                        projected_albums.append((mapped, match))

                    track_maps = self._match_maps("tracks", target_rows["tracks"])
                    projected_tracks = []
                    for row in source_rows["tracks"]:
                        mapped = self._map_row("tracks", row, str(source_root), str(target))
                        mapped["album_key"] = album_keys.get(str(row.get("album_key") or ""), row.get("album_key"))
                        match = self._find_match("tracks", mapped, track_maps)
                        if match:
                            mapped["track_key"] = match["track_key"]
                        projected_tracks.append((mapped, match))

                    prepared: dict[str, list[tuple[dict[str, Any], dict[str, Any] | None]]] = {
                        "artists": projected_artists,
                        "albums": projected_albums,
                        "tracks": projected_tracks,
                    }
                    for table in table_names[3:]:
                        maps = self._match_maps(table, target_rows[table]) if table != "cache_history" else {}
                        existing_history = {
                            _digest({key: value for key, value in row.items() if key != "history_id"})
                            for row in target_rows[table]
                        } if table == "cache_history" else set()
                        rows = []
                        for row in source_rows[table]:
                            mapped = self._map_row(table, row, str(source_root), str(target))
                            if table == "cache_history":
                                mapped.pop("history_id", None)
                                match = mapped if _digest(mapped) in existing_history else None
                            else:
                                match = self._find_match(table, mapped, maps)
                            rows.append((mapped, match))
                        prepared[table] = rows

                    for table in table_names:
                        columns = target_columns[table]
                        for source_row, target_row in prepared[table]:
                            source_value = self._projection(table, source_row, columns)
                            target_value = self._projection(
                                table, target_row, list(source_value)
                            ) if target_row else None
                            if target_value is not None and _digest(source_value) == _digest(target_value):
                                continue
                            resolution = classify_difference(table, source_value, target_value)
                            self._record_difference(
                                raw,
                                run_id=run_id,
                                fingerprint=before,
                                label=label,
                                table=table,
                                source=source_value,
                                target=target_value,
                                resolution=resolution,
                            )
                            counts["differences"] += 1
                            counts["safe" if resolution == "safe" else "derived" if resolution == "derived" else "conflicts"] += 1
                    after = self._fingerprint(source)
                    if after != before:
                        raise ValueError("SQLite snapshot changed during comparison; retry with a stable snapshot")
                    raw.execute(
                        "UPDATE sqlite_reconciliation_runs SET status='compared',differences=%s,safe_differences=%s,"
                        "conflicts=%s,finished_at=now() WHERE run_id=%s",
                        (counts["differences"], counts["safe"], counts["conflicts"], run_id),
                    )
                result = {"run_id": run_id, "source_fingerprint": before, "source": label, **counts}
                self._event(
                    "sqlite-reconciliation",
                    result,
                    stage="compare",
                    request_status="completed",
                )
                return result
            except Exception as error:
                message = sanitize_text(error, limit=1000)
                try:
                    with closing(self.database.connect()) as failure_db, failure_db:
                        failure_db.raw.execute(
                            "INSERT INTO sqlite_reconciliation_runs(run_id,source_fingerprint,source_label,trigger,status,failures,error,finished_at) "
                            "VALUES(%s,%s,%s,%s,'failed',1,%s,now()) ON CONFLICT(run_id) DO UPDATE SET "
                            "status='failed',failures=1,error=excluded.error,finished_at=now()",
                            (run_id, before, label, sanitize_text(trigger, limit=30), message),
                        )
                except Exception:
                    pass
                self._event(
                    "sqlite-reconciliation",
                    {"run_id": run_id, "source": label},
                    severity="error",
                    stage="compare",
                    request_status="failed",
                    error=message,
                )
                raise

    @staticmethod
    def _row_by_key(raw, table: str, source: dict[str, Any]) -> dict[str, Any] | None:
        keys = TABLE_KEYS[table]
        statement = sql.SQL("SELECT * FROM {} WHERE {}").format(
            sql.Identifier(table),
            sql.SQL(" AND ").join(
                sql.SQL("{}={}").format(sql.Identifier(name), sql.Placeholder()) for name in keys
            ),
        )
        row = raw.execute(statement, [source.get(name) for name in keys]).fetchone()
        return dict(row) if row else None

    @staticmethod
    def _insert(raw, table: str, source: dict[str, Any]) -> None:
        names = list(source)
        statement = sql.SQL("INSERT INTO {} ({}) VALUES ({})").format(
            sql.Identifier(table),
            sql.SQL(",").join(map(sql.Identifier, names)),
            sql.SQL(",").join(sql.Placeholder() for _ in names),
        )
        raw.execute(statement, [source[name] for name in names])

    @staticmethod
    def _update(raw, table: str, source: dict[str, Any]) -> None:
        keys = TABLE_KEYS[table]
        names = [name for name in source if name not in keys and name not in DERIVED_COLUMNS.get(table, set())]
        statement = sql.SQL("UPDATE {} SET {} WHERE {}").format(
            sql.Identifier(table),
            sql.SQL(",").join(
                sql.SQL("{}={}").format(sql.Identifier(name), sql.Placeholder()) for name in names
            ),
            sql.SQL(" AND ").join(
                sql.SQL("{}={}").format(sql.Identifier(name), sql.Placeholder()) for name in keys
            ),
        )
        raw.execute(statement, [source[name] for name in names] + [source[name] for name in keys])

    @staticmethod
    def _recalculate(db) -> None:
        from splined_media_database import update_artist_aggregate

        rows = list(
            db.execute(
                "SELECT a.album_key,a.artist_key,a.path,a.compilation,a.processed_at,a.selected_source,"
                "a.bypassed,a.timeout_until,p.total_tracks,p.completed_tracks,p.status AS progress_status "
                "FROM albums a LEFT JOIN compilation_album_progress p ON p.album_path=a.path"
            )
        )
        by_artist: dict[str, list[str]] = {}
        now = datetime.now(timezone.utc)
        for row in rows:
            timeout = _timestamp(row["timeout_until"])
            if bool(row["bypassed"]):
                status = "bypassed"
                timeout_value = ""
            elif timeout and timeout > now:
                status = "timeout"
                timeout_value = str(row["timeout_until"] or "")
            elif timeout:
                status = "processed"
                timeout_value = ""
            elif bool(row["compilation"]) and row["total_tracks"] is not None:
                total = int(row["total_tracks"] or 0)
                completed = int(row["completed_tracks"] or 0)
                status = "processed" if total > 0 and completed >= total else "incomplete"
                timeout_value = ""
            elif str(row["processed_at"] or "").strip() or str(row["selected_source"] or "").strip():
                status = "processed"
                timeout_value = ""
            else:
                status = "unprocessed"
                timeout_value = ""
            db.execute(
                "UPDATE albums SET status=?,bypassed=?,timeout_until=? WHERE album_key=?",
                (status, int(status == "bypassed"), timeout_value, row["album_key"]),
            )
            by_artist.setdefault(str(row["artist_key"]), []).append(status)
        for artist_key, statuses in by_artist.items():
            update_artist_aggregate(db, artist_key, statuses)

    @staticmethod
    def _recalculate_comp_progress(db) -> None:
        albums = list(db.execute("SELECT path FROM albums WHERE compilation<>0"))
        for album in albums:
            album_path = str(album["path"])
            tracks = list(db.execute("SELECT path FROM tracks WHERE album_key=(SELECT album_key FROM albums WHERE path=?)", (album_path,)))
            total = len(tracks)
            completed = sum(
                1
                for track in tracks
                if db.execute(
                    "SELECT 1 FROM compilation_track_artwork WHERE track_path=? "
                    "AND outcome IN ('embedded-replaced','embedded-existing')",
                    (track["path"],),
                ).fetchone()
            )
            status = "complete" if total > 0 and completed >= total else "incomplete"
            db.execute(
                "INSERT INTO compilation_album_progress(album_path,total_tracks,completed_tracks,status,updated_at,splined_version) "
                "VALUES(?,?,?,?,?,?) ON CONFLICT(album_path) DO UPDATE SET total_tracks=excluded.total_tracks,"
                "completed_tracks=excluded.completed_tracks,status=excluded.status,updated_at=excluded.updated_at,"
                "splined_version=excluded.splined_version",
                (album_path, total, completed, status, datetime.now(timezone.utc).isoformat(), "web-reconcile"),
            )

    def apply(self, run_id: Any) -> dict[str, Any]:
        run = sanitize_text(run_id, limit=100)
        if not run:
            raise ValueError("Choose a reconciliation run")
        with self._operation_lock, closing(self.database.connect()) as db, db:
            raw = db.raw
            raw.execute("SELECT pg_advisory_xact_lock(741520213)")
            if not raw.execute(
                "SELECT 1 FROM sqlite_reconciliation_runs WHERE run_id=%s", (run,)
            ).fetchone():
                raise ValueError("The selected reconciliation run does not exist")
            rows = [
                dict(row)
                for row in raw.execute(
                    "SELECT * FROM salite_diff WHERE last_run_id=%s AND resolution='safe' "
                    "AND application_outcome='pending' ORDER BY CASE entity_table "
                    "WHEN 'artists' THEN 10 WHEN 'albums' THEN 20 WHEN 'tracks' THEN 30 "
                    "WHEN 'recording_release_lookups' THEN 40 WHEN 'recording_release_candidates' THEN 50 "
                    "WHEN 'compilation_track_artwork' THEN 60 WHEN 'cache_history' THEN 70 ELSE 100 END,id",
                    (run,),
                )
            ]
            applied = 0
            failures = 0
            for difference in rows:
                table = str(difference["entity_table"])
                source = dict(difference["source_value"])
                try:
                    with raw.transaction():
                        if table == "cache_history":
                            self._insert(raw, table, source)
                        else:
                            current = self._row_by_key(raw, table, source)
                            columns = list(source)
                            projected = self._projection(table, current, columns) if current else None
                            current_digest = _digest(projected) if projected is not None else ""
                            if current_digest != str(difference["old_digest"]):
                                raise ValueError("PostgreSQL state changed after comparison")
                            if current is None:
                                insert_value = dict(source)
                                if table == "artists":
                                    insert_value.update(status="unprocessed", album_count=0, unprocessed_count=0, processed_count=0, bypassed_count=0, timeout_count=0)
                                elif table == "albums":
                                    insert_value["status"] = "unprocessed"
                                self._insert(raw, table, insert_value)
                            else:
                                self._update(raw, table, source)
                        raw.execute(
                            "UPDATE salite_diff SET resolution='applied',application_outcome='applied',applied_at=now(),error='' WHERE id=%s",
                            (difference["id"],),
                        )
                    applied += 1
                except Exception as error:
                    raw.execute(
                        "UPDATE salite_diff SET resolution='skipped',application_outcome='failed',error=%s WHERE id=%s",
                        (sanitize_text(error, limit=1000), difference["id"]),
                    )
                    failures += 1
            self._recalculate_comp_progress(db)
            self._recalculate(db)
            raw.execute(
                "UPDATE sqlite_reconciliation_runs SET status=%s,applied=%s,failures=failures+%s,finished_at=now() WHERE run_id=%s",
                ("applied" if failures == 0 else "applied-with-failures", applied, failures, run),
            )
        result = {"run_id": run, "applied": applied, "failures": failures, "conflicts_applied": 0}
        self._event(
            "sqlite-reconciliation",
            result,
            severity="warning" if failures else "info",
            stage="apply-safe",
            request_status="failed" if failures else "completed",
        )
        return result

    def history(self, *, run_id: str = "", resolution: str = "", limit: int = 500) -> dict[str, Any]:
        clauses = []
        params: list[Any] = []
        if run_id:
            clauses.append("last_run_id=%s")
            params.append(sanitize_text(run_id, limit=100))
        if resolution:
            if resolution not in {"safe", "conflict", "derived", "applied", "skipped"}:
                raise ValueError("Unknown reconciliation resolution")
            clauses.append("resolution=%s")
            params.append(resolution)
        where = " WHERE " + " AND ".join(clauses) if clauses else ""
        size = max(1, min(int(limit), 2000))
        with closing(self.database.connect()) as db:
            differences = [
                dict(row)
                for row in db.raw.execute(
                    "SELECT id,last_run_id AS run_id,source_label,entity_table,entity_key,difference_type,"
                    "old_digest,new_digest,resolution,application_outcome,error,created_at,last_seen_at,applied_at "
                    f"FROM salite_diff{where} ORDER BY id DESC LIMIT %s",
                    params + [size],
                )
            ]
            last = db.raw.execute(
                "SELECT * FROM sqlite_reconciliation_runs WHERE (%s='' OR run_id=%s) "
                "ORDER BY started_at DESC LIMIT 1",
                (sanitize_text(run_id, limit=100), sanitize_text(run_id, limit=100)),
            ).fetchone()
            pending_safe = int(
                db.raw.execute(
                    "SELECT count(*) AS n FROM salite_diff WHERE last_run_id=%s "
                    "AND resolution='safe' AND application_outcome='pending'",
                    (str(last["run_id"]) if last else "",),
                ).fetchone()["n"]
            )
            settings = db.raw.execute("SELECT * FROM sqlite_reconciliation_settings WHERE singleton=TRUE").fetchone()
        return {
            "differences": differences,
            "last_run": dict(last) if last else None,
            "pending_safe": pending_safe,
            "settings": dict(settings) if settings else {},
        }

    def save_settings(self, payload: dict[str, Any]) -> dict[str, Any]:
        enabled = bool(payload.get("enabled", False))
        auto_apply = bool(payload.get("auto_apply_safe", False))
        interval = int(payload.get("interval_minutes", 1440))
        if not 15 <= interval <= 10080:
            raise ValueError("Scheduled reconciliation interval must be 15 to 10080 minutes")
        source_path = str(payload.get("source_path", "")).strip()
        source_root = str(payload.get("source_root", "")).strip()
        if enabled:
            self._source(source_path)
            if not source_root:
                raise ValueError("Scheduled reconciliation requires the SQLite source library root")
        target = Path(str(payload.get("target_root") or self.target_root)).resolve()
        if target != self.target_root:
            raise PermissionError("Scheduled reconciliation target must be the configured music library")
        next_run = datetime.now(timezone.utc) + timedelta(minutes=interval) if enabled else None
        with closing(self.database.connect()) as db, db:
            db.raw.execute(
                "UPDATE sqlite_reconciliation_settings SET enabled=%s,auto_apply_safe=%s,interval_minutes=%s,"
                "source_path=%s,source_root=%s,target_root=%s,updated_at=now(),next_run_at=%s WHERE singleton=TRUE",
                (enabled, auto_apply, interval, source_path, source_root, str(target), next_run),
            )
        return self.history(limit=1)["settings"]

    def _scheduler(self) -> None:
        state: dict[str, Any] = {}
        while not self._stop.wait(30):
            try:
                state = self.history(limit=1)["settings"]
                if not state or not state.get("enabled"):
                    continue
                due = state.get("next_run_at")
                if due and _timestamp(due) and _timestamp(due) > datetime.now(timezone.utc):
                    continue
                result = self.compare(
                    state.get("source_path", ""),
                    state.get("source_root", ""),
                    state.get("target_root", ""),
                    trigger="scheduled",
                )
                if state.get("auto_apply_safe"):
                    self.apply(result["run_id"])
                interval = int(state.get("interval_minutes") or 1440)
                with closing(self.database.connect()) as db, db:
                    db.raw.execute(
                        "UPDATE sqlite_reconciliation_settings SET last_run_at=now(),next_run_at=%s WHERE singleton=TRUE",
                        (datetime.now(timezone.utc) + timedelta(minutes=interval),),
                    )
            except Exception as error:
                try:
                    interval = int(state.get("interval_minutes") or 1440)
                    with closing(self.database.connect()) as db, db:
                        db.raw.execute(
                            "UPDATE sqlite_reconciliation_settings SET last_run_at=now(),next_run_at=%s WHERE singleton=TRUE",
                            (datetime.now(timezone.utc) + timedelta(minutes=interval),),
                        )
                except Exception:
                    pass
                self._event(
                    "sqlite-reconciliation",
                    {"trigger": "scheduled"},
                    severity="error",
                    stage="schedule",
                    request_status="failed",
                    error=sanitize_text(error, limit=1000),
                )

