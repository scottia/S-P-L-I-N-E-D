"""Safe, incremental SQLite-to-PostgreSQL reconciliation.

SQLite is always opened read-only.  Windows SQLite is authoritative for the
catalog, identity, metadata, and processing fields shared by both databases.
PostgreSQL-only fields are preserved, and ambiguous entity associations remain
reviewable conflicts in ``salite_diff``.
"""

from __future__ import annotations

import json
import re
import sqlite3
from contextlib import closing
from datetime import datetime, timedelta, timezone
from hashlib import sha256
from pathlib import Path
from threading import Event, Lock, Thread
from typing import Any, Callable
from uuid import uuid4

from psycopg import sql
from psycopg.types.json import Jsonb

from webgui.audit import sanitize_text
from webgui.catalog_authority import (
    TABLE_KEYS as AUTHORITY_TABLE_KEYS,
    WINDOWS_MANAGED_TABLES,
    mark_windows_authority,
)

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
WINDOWS_AUTHORITY_TABLES = {
    "artists",
    "albums",
    "tracks",
    "recording_release_lookups",
    "recording_release_candidates",
    "compilation_track_artwork",
    "cache_entries",
    "cache_history",
}
REQUIRED_INSERT_FIELDS = {
    "artists": {
        "artist_key",
        "artist_name",
        "primary_path",
        "created_at",
        "updated_at",
        "last_seen_at",
        "splined_version",
    },
    "albums": {
        "album_key",
        "artist_key",
        "album_name",
        "path",
        "tag_signature",
        "created_at",
        "updated_at",
        "last_seen_at",
        "splined_version",
    },
    "tracks": {
        "track_key",
        "album_key",
        "path",
        "title",
        "artist_name",
        "file_size",
        "file_mtime_ns",
        "updated_at",
        "splined_version",
    },
}


def _normalize_sqlite_value(value: Any) -> Any:
    """Translate SQLite multi-value text into PostgreSQL-safe text.

    Windows tag readers preserve multiple values with NUL separators. SQLite
    accepts those strings, but PostgreSQL text and JSONB reject U+0000. Keep
    every non-empty value and use the application's existing ``; `` display
    separator before matching, hashing, recording, or applying differences.
    """
    if not isinstance(value, str) or "\0" not in value:
        return value
    return "; ".join(part.strip() for part in value.split("\0") if part.strip())


def _identity_column(name: str) -> bool:
    """Return whether a shared field participates in catalog identity."""
    return name.endswith(("_key", "_mbid")) or (
        name.startswith("musicbrainz_") and name.endswith("id")
    )


def _normalize_sqlite_row(row: dict[str, Any]) -> dict[str, Any]:
    """Normalize transport-only characters without hiding source collisions.

    Some Windows tag values contain U+FEFF at the start of MusicBrainz IDs.
    Windows can consequently persist both ``mbid:<uuid>`` and
    ``mbid:\ufeff<uuid>`` as distinct SQLite keys. PostgreSQL must receive the
    canonical identity, while reconciliation must retain the original values
    long enough to reject two Windows rows that converge on one canonical key.
    """
    normalized: dict[str, Any] = {}
    originals: dict[str, str] = {}
    for name, raw_value in row.items():
        value = _normalize_sqlite_value(raw_value)
        if _identity_column(name) and isinstance(value, str) and "\ufeff" in value:
            originals[name] = value
            value = value.replace("\ufeff", "")
        normalized[name] = value
    if originals:
        normalized["_reconciliation_original_identity_values"] = originals
    return normalized


def _identity_text(value: Any) -> str:
    """Canonicalize only the transport BOM; preserve all real key syntax."""
    return str(value or "").replace("\ufeff", "")


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
    """Classify a deterministically associated Windows-authority difference."""
    identity = source.get("_reconciliation_identity") or {}
    if str(identity.get("status") or "") in {
        "ambiguous",
        "blocked",
        "conflict",
        "rejected",
    }:
        return "conflict"
    if table == "compilation_album_progress":
        return "derived"
    if target is None:
        return "safe" if table in WINDOWS_AUTHORITY_TABLES else "conflict"
    return "safe" if table in WINDOWS_AUTHORITY_TABLES else "conflict"


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
        snapshot_roots: tuple[Path, ...] = (),
        start_scheduler: bool = True,
        album_validator: Callable[[str], dict[str, Any]] | None = None,
    ):
        self.database = database
        roots: list[Path] = []
        for candidate in (allowed_root, *snapshot_roots):
            resolved = Path(candidate).expanduser().resolve()
            if resolved not in roots:
                roots.append(resolved)
        self.allowed_roots = tuple(roots)
        self.allowed_root = self.allowed_roots[0]
        self.target_root = target_root.resolve()
        self.audit = audit
        self.album_validator = album_validator
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

    def _processing_active(self) -> bool:
        """Keep catalog reconciliation invisible while Album work is active."""
        with closing(self.database.connect()) as db:
            return bool(
                db.raw.execute(
                    "SELECT 1 FROM web_jobs "
                    "WHERE state IN ('staged','queued','running','waiting') LIMIT 1"
                ).fetchone()
            )

    def _require_idle(self) -> None:
        if self._processing_active():
            raise RuntimeError(
                "SQLite reconciliation is deferred while Album processing is active"
            )

    def _authorized_root(self, path: Path) -> Path | None:
        for root in self.allowed_roots:
            try:
                path.relative_to(root)
                return root
            except ValueError:
                continue
        return None

    def _source_label(self, path: Path) -> str:
        root = self._authorized_root(path)
        if root is None:
            raise PermissionError("SQLite reconciliation source is outside the authorized snapshot directories")
        relative = path.relative_to(root)
        return str(relative) if root == self.allowed_root else str(Path(root.name) / relative)

    def _source(self, value: Any) -> Path:
        selected = Path(str(value or "")).expanduser()
        if selected.is_symlink():
            raise ValueError("SQLite reconciliation does not follow snapshot symlinks")
        path = selected.resolve(strict=True)
        if not path.is_file() or path.suffix.casefold() not in SOURCE_SUFFIXES:
            raise ValueError("Choose a .db, .sqlite, or .sqlite3 file from an authorized snapshot directory")
        if self._authorized_root(path) is None:
            raise PermissionError("SQLite reconciliation sources must be inside an authorized snapshot directory")
        return path

    def sources(self) -> list[dict[str, Any]]:
        result = []
        seen: set[Path] = set()
        inventory_roots = self.allowed_roots[1:] or self.allowed_roots
        for root in inventory_roots:
            if not root.is_dir():
                continue
            for path in root.rglob("*"):
                if len(result) >= 200:
                    break
                try:
                    resolved = path.resolve(strict=True)
                    if (
                        path.is_symlink()
                        or resolved in seen
                        or not resolved.is_file()
                        or resolved.suffix.casefold() not in SOURCE_SUFFIXES
                    ):
                        continue
                    resolved.relative_to(root)
                    stat = resolved.stat()
                except (OSError, ValueError):
                    continue
                seen.add(resolved)
                result.append(
                    {
                        "path": str(resolved),
                        "label": self._source_label(resolved),
                        "bytes": int(stat.st_size),
                        "modified_at": datetime.fromtimestamp(stat.st_mtime, timezone.utc).isoformat(),
                    }
                )
            if len(result) >= 200:
                break
        return sorted(result, key=lambda item: item["label"].casefold())

    @staticmethod
    def _fingerprint(path: Path) -> str:
        # Authorized reconciliation inputs are standalone snapshots produced
        # through SQLite's online-backup API.  Read and identify that immutable
        # artifact only; read-only SQLite connections must not create or make
        # the fingerprint depend on adjacent -wal/-shm sidecars.
        digest = sha256()
        digest.update(path.name.encode("utf-8", "surrogateescape"))
        digest.update(b"\0")
        with path.open("rb") as handle:
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
        return [
            _normalize_sqlite_row(dict(row))
            for row in db.execute(f'SELECT * FROM "{table}"')
        ]

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
        # Native SQLite uses the absolute media path as track_key.  The key is
        # part of the PostgreSQL identity, so map it with the track path when
        # both source values identify the same file.  Opaque/non-path keys are
        # deliberately left untouched.
        if table == "tracks" and row.get("track_key") and row.get("path"):
            source_key = str(row["track_key"]).replace("\\", "/").rstrip("/")
            source_path = str(row["path"]).replace("\\", "/").rstrip("/")
            if source_key.casefold() == source_path.casefold():
                mapped["track_key"] = mapped["path"]
        # Windows schema v2 records an embed-queue outcome only after all
        # tracks have completed and verified, but does not store the
        # WebGUI-only embedded_tracks counter.  Project that durable authority
        # so album completion renders consistently after reconciliation.
        if (
            table == "albums"
            and "embedded_tracks" not in row
            # A compilation tag alone does not make an identified release a
            # COMP fallback.  Only unidentified compilation folders use the
            # per-track COMP authority; MusicBrainz-identified compilations
            # remain normal Album embedding work.
            and (
                not bool(row.get("compilation"))
                or bool(str(row.get("musicbrainz_albumid") or "").strip())
            )
            and str(row.get("status") or "").strip().casefold() == "processed"
            and str(row.get("selected_source") or "").strip().casefold() == "embed-queue"
            and str(row.get("processed_at") or "").strip()
            and int(row.get("track_count") or 0) > 0
        ):
            mapped["embedded_tracks"] = int(row["track_count"])
        return mapped

    @staticmethod
    def _artist_path_identity(value: Any) -> str:
        return str(value or "").strip().replace("\\", "/").rstrip("/").casefold()

    def _insert_identity(
        self,
        table: str,
        row: dict[str, Any],
        *,
        parent_ready: bool = True,
    ) -> dict[str, Any]:
        """Prove that a source-only entity is complete and safely path-mapped."""
        if not parent_ready:
            return {
                "status": "blocked",
                "evidence": "parent-entity",
                "reason": "authoritative-parent-association-unresolved",
                "candidate_count": 0,
                "candidates": [],
            }
        field = {"artists": "primary_path", "albums": "path", "tracks": "path"}.get(
            table
        )
        if not field:
            return {
                "status": "insertable",
                "evidence": "authoritative-source-key",
                "candidate_count": 0,
            }
        missing = sorted(
            name
            for name in REQUIRED_INSERT_FIELDS.get(table, set())
            if name not in row or row.get(name) is None or row.get(name) == ""
        )
        if missing:
            return {
                "status": "rejected",
                "evidence": "source-schema",
                "reason": "required-insert-fields-missing",
                "missing_fields": missing,
                "candidate_count": 0,
                "candidates": [],
            }
        value = str(row.get(field) or "").strip()
        if not value:
            return {
                "status": "rejected",
                "evidence": "mapped-path",
                "reason": f"missing-{field.replace('_', '-')}",
                "candidate_count": 0,
                "candidates": [],
            }
        # ``map_library_path`` already rejects traversal and constructs this
        # value from the resolved target root. Keep the containment check
        # lexical: resolving tens of thousands of paths would perform remote
        # filesystem I/O during a Compare-only operation.
        candidate = Path(value)
        try:
            candidate.relative_to(self.target_root)
        except ValueError:
            return {
                "status": "rejected",
                "evidence": "mapped-path",
                "reason": "mapped-path-outside-target-library",
                "candidate_count": 0,
                "candidates": [],
            }
        expected = "file" if table == "tracks" else "directory"
        return {
            "status": "insertable",
            "evidence": f"mapped-{expected}-authority",
            "mapped_path": str(candidate),
            "candidate_count": 0,
        }

    @staticmethod
    def _artist_candidate_summary(rows: list[dict[str, Any]]) -> list[dict[str, str]]:
        return sorted(
            (
                {
                    "artist_key": str(row.get("artist_key") or ""),
                    "artist_name": str(row.get("artist_name") or ""),
                    "musicbrainz_artistid": str(row.get("musicbrainz_artistid") or ""),
                    "primary_path": str(row.get("primary_path") or ""),
                }
                for row in rows
            ),
            key=lambda row: (
                row["artist_key"],
                row["musicbrainz_artistid"],
                row["primary_path"],
                row["artist_name"],
            ),
        )

    @staticmethod
    def _match_maps(table: str, target_rows: list[dict[str, Any]]) -> dict[str, Any]:
        if table == "artists":
            # Artist identities require unique candidates.  Keeping buckets rather
            # than the first row prevents PostgreSQL row order from deciding which
            # Artist receives authoritative Windows state.
            artist_maps: dict[str, list[dict[str, Any]]] = {}

            def add(prefix: str, value: str, row: dict[str, Any]) -> None:
                if value:
                    artist_maps.setdefault(prefix + ":" + value, []).append(row)

            for row in target_rows:
                add("key", _identity_text(row.get("artist_key")), row)
                add(
                    "mbid",
                    _identity_text(row.get("musicbrainz_artistid")).strip().casefold(),
                    row,
                )
                add(
                    "primary-path",
                    SQLiteReconciler._artist_path_identity(row.get("primary_path")),
                    row,
                )
                add("name", _identity(row.get("artist_name")), row)
            return artist_maps

        maps: dict[str, list[dict[str, Any]]] = {}

        def add(prefix: str, value: Any, row: dict[str, Any]) -> None:
            text = str(value or "")
            if text:
                maps.setdefault(prefix + ":" + text, []).append(row)

        for row in target_rows:
            keys = TABLE_KEYS.get(table, ())
            if keys:
                add(
                    "entity",
                    _canonical([_identity_text(row.get(name)) for name in keys]),
                    row,
                )
                # Only a complete primary key proves entity identity.  Album
                # and track keys are single-column identities, while tables
                # such as recording_release_candidates have compound keys;
                # their first component alone must never select a row.
                if len(keys) == 1:
                    add("key", _identity_text(row.get(keys[0])), row)
            if table == "albums":
                add(
                    "mbid",
                    _identity_text(row.get("musicbrainz_albumid")).strip().casefold(),
                    row,
                )
                add("path", SQLiteReconciler._artist_path_identity(row.get("path")), row)
                if row.get("artist_key") and row.get("album_name"):
                    add(
                        "artist-album",
                        _canonical(
                            [_identity_text(row["artist_key"]), _identity(row["album_name"])]
                        ),
                        row,
                    )
            elif table == "tracks":
                add("path", SQLiteReconciler._artist_path_identity(row.get("path")), row)
                if row.get("musicbrainz_recordingid") and row.get("album_key"):
                    add(
                        "recording-album",
                        _canonical(
                            [
                                _identity_text(row["musicbrainz_recordingid"]).strip().casefold(),
                                _identity_text(row["album_key"]),
                            ]
                        ),
                        row,
                    )
            else:
                add(
                    "path",
                    SQLiteReconciler._artist_path_identity(
                        row.get("track_path") or row.get("album_path")
                    ),
                    row,
                )
        return maps

    @staticmethod
    def _find_artist_match(
        source: dict[str, Any], maps: dict[str, Any]
    ) -> tuple[dict[str, Any] | None, dict[str, Any]]:
        source_mbid = _identity_text(source.get("musicbrainz_artistid")).strip()
        values = (
            ("artist-key", "key", _identity_text(source.get("artist_key"))),
            ("musicbrainz-artist-id", "mbid", source_mbid.casefold()),
            (
                "primary-path",
                "primary-path",
                SQLiteReconciler._artist_path_identity(source.get("primary_path")),
            ),
            ("artist-name", "name", _identity(source.get("artist_name"))),
        )
        deferred_mbid_ambiguity: dict[str, Any] | None = None
        for evidence, prefix, value in values:
            if not value:
                continue
            if evidence == "artist-name" and deferred_mbid_ambiguity:
                return None, deferred_mbid_ambiguity
            candidates = maps.get(prefix + ":" + value, [])
            if not candidates:
                continue
            if len(candidates) != 1:
                ambiguity = {
                    "status": "ambiguous",
                    "evidence": evidence,
                    "candidate_count": len(candidates),
                    "candidates": SQLiteReconciler._artist_candidate_summary(candidates),
                }
                # A duplicated MBID is not itself authoritative enough to
                # choose a row, but a subsequent unique mapped path may still
                # independently prove the intended PostgreSQL Artist.
                if evidence == "musicbrainz-artist-id":
                    deferred_mbid_ambiguity = ambiguity
                    continue
                return None, ambiguity
            target = candidates[0]
            target_mbid = _identity_text(target.get("musicbrainz_artistid")).strip()
            mbids_conflict = bool(
                source_mbid
                and target_mbid
                and source_mbid.casefold() != target_mbid.casefold()
            )
            if evidence == "artist-key" and mbids_conflict:
                source_path = SQLiteReconciler._artist_path_identity(
                    source.get("primary_path")
                )
                path_candidates = (
                    maps.get("primary-path:" + source_path, []) if source_path else []
                )
                if len(path_candidates) == 1 and path_candidates[0] is target:
                    return target, {
                        "status": "matched",
                        "evidence": "artist-key+primary-path",
                        "candidate_count": 1,
                    }
                diagnostic_candidates = path_candidates or candidates
                return None, {
                    "status": "conflict",
                    "evidence": "artist-key",
                    "reason": "conflicting-non-empty-musicbrainz-artist-ids-require-unique-path",
                    "candidate_count": len(diagnostic_candidates),
                    "candidates": SQLiteReconciler._artist_candidate_summary(
                        diagnostic_candidates
                    ),
                }
            if (
                evidence == "artist-name"
                and mbids_conflict
            ):
                return None, {
                    "status": "conflict",
                    "evidence": evidence,
                    "reason": "conflicting-non-empty-musicbrainz-artist-ids",
                    "candidate_count": 1,
                    "candidates": SQLiteReconciler._artist_candidate_summary(candidates),
                }
            return target, {
                "status": "matched",
                "evidence": evidence,
                "candidate_count": 1,
            }
        return None, deferred_mbid_ambiguity or {
            "status": "unmatched",
            "evidence": "none",
            "reason": "no-unique-postgresql-artist-candidate",
            "candidate_count": 0,
            "candidates": [],
        }

    @staticmethod
    def _entity_candidate_summary(
        table: str, rows: list[dict[str, Any]]
    ) -> list[dict[str, str]]:
        names = {
            "albums": ("album_key", "artist_key", "album_name", "musicbrainz_albumid", "path"),
            "tracks": ("track_key", "album_key", "musicbrainz_recordingid", "path"),
        }.get(table, TABLE_KEYS.get(table, ()))
        return sorted(
            ({name: str(row.get(name) or "") for name in names} for row in rows),
            key=lambda item: tuple(item.values()),
        )

    @staticmethod
    def _reject_source_fan_in(
        table: str,
        rows: list[tuple[dict[str, Any], dict[str, Any] | None]],
    ) -> set[str]:
        """Reject multiple Windows entities converging on one target row."""
        key_name = TABLE_KEYS[table][0]
        groups: dict[str, list[dict[str, Any]]] = {}
        for source, target in rows:
            if target is not None:
                groups.setdefault(str(target.get(key_name) or ""), []).append(source)
        rejected: set[str] = set()
        for target_key, sources in groups.items():
            if len(sources) < 2:
                continue
            source_keys = sorted(
                str(item.get("_reconciliation_source_key") or "")
                for item in sources
            )
            diagnostic = {
                "status": "conflict",
                "evidence": "multiple-authoritative-source-entities",
                "reason": "multiple-windows-entities-map-to-one-postgresql-entity",
                "target_key": target_key,
                "source_keys": source_keys,
                "candidate_count": len(sources),
            }
            for source in sources:
                source["_reconciliation_identity"] = diagnostic
                rejected.add(str(source.get("_reconciliation_source_key") or ""))
        return rejected

    @staticmethod
    def _reject_normalized_source_collisions(
        table: str,
        rows: list[tuple[dict[str, Any], dict[str, Any] | None]],
    ) -> set[str]:
        """Reject Windows rows whose U+FEFF cleanup collapses a primary key."""
        keys = TABLE_KEYS.get(table, ())
        if not keys:
            return set()
        groups: dict[str, list[dict[str, Any]]] = {}
        for source, _target in rows:
            groups.setdefault(
                _canonical([_identity_text(source.get(name)) for name in keys]),
                [],
            ).append(source)
        rejected: set[str] = set()
        for normalized_key, sources in groups.items():
            if len(sources) < 2:
                continue
            original_keys = sorted(
                _canonical(
                    [
                        dict(
                            source.get("_reconciliation_original_identity_values")
                            or {}
                        ).get(name, source.get(name))
                        for name in keys
                    ]
                )
                for source in sources
            )
            diagnostic = {
                "status": "conflict",
                "evidence": "normalized-authoritative-source-identity",
                "reason": "multiple-windows-entities-share-one-canonical-key",
                "normalized_key": normalized_key,
                "source_keys": original_keys,
                "candidate_count": len(sources),
            }
            for source in sources:
                source["_reconciliation_identity"] = diagnostic
                rejected.add(str(source.get("_reconciliation_source_key") or ""))
        return rejected

    @staticmethod
    def _find_entity_match(
        table: str, source: dict[str, Any], maps: dict[str, Any]
    ) -> tuple[dict[str, Any] | None, dict[str, Any]]:
        keys = TABLE_KEYS.get(table, ())
        values: list[tuple[str, str, str]] = []
        if keys:
            values.append(
                (
                    "exact-entity-key",
                    "entity",
                    _canonical([_identity_text(source.get(name)) for name in keys]),
                )
            )
            if len(keys) == 1:
                values.append(
                    ("exact-primary-key", "key", _identity_text(source.get(keys[0])))
                )
        if table == "albums":
            values.extend(
                (
                    (
                        "musicbrainz-album-id",
                        "mbid",
                        _identity_text(source.get("musicbrainz_albumid")).strip().casefold(),
                    ),
                    (
                        "mapped-path",
                        "path",
                        SQLiteReconciler._artist_path_identity(source.get("path")),
                    ),
                    (
                        "artist-and-album-name",
                        "artist-album",
                        _canonical(
                            [
                                _identity_text(source.get("artist_key")),
                                _identity(source.get("album_name")),
                            ]
                        )
                        if source.get("artist_key") and source.get("album_name")
                        else "",
                    ),
                )
            )
        elif table == "tracks":
            values.extend(
                (
                    (
                        "mapped-path",
                        "path",
                        SQLiteReconciler._artist_path_identity(source.get("path")),
                    ),
                    (
                        "recording-and-album",
                        "recording-album",
                        _canonical(
                            [
                                _identity_text(source.get("musicbrainz_recordingid"))
                                .strip()
                                .casefold(),
                                _identity_text(source.get("album_key")),
                            ]
                        )
                        if source.get("musicbrainz_recordingid") and source.get("album_key")
                        else "",
                    ),
                )
            )
        else:
            values.append(
                (
                    "mapped-path",
                    "path",
                    SQLiteReconciler._artist_path_identity(
                        source.get("track_path") or source.get("album_path")
                    ),
                )
            )

        seen: set[tuple[str, str]] = set()
        for evidence, prefix, value in values:
            if not value or (prefix, value) in seen:
                continue
            seen.add((prefix, value))
            candidates = maps.get(prefix + ":" + value, [])
            if not candidates:
                continue
            if len(candidates) != 1:
                return None, {
                    "status": "ambiguous",
                    "evidence": evidence,
                    "candidate_count": len(candidates),
                    "candidates": SQLiteReconciler._entity_candidate_summary(
                        table, candidates
                    ),
                }
            target = candidates[0]
            if table == "albums" and evidence == "artist-and-album-name":
                source_mbid = _identity_text(source.get("musicbrainz_albumid")).strip()
                target_mbid = _identity_text(target.get("musicbrainz_albumid")).strip()
                if target_mbid and source_mbid.casefold() != target_mbid.casefold():
                    return None, {
                        "status": "conflict",
                        "evidence": evidence,
                        "reason": "album-name-only-evidence-cannot-replace-postgresql-mbid",
                        "candidate_count": 1,
                        "candidates": SQLiteReconciler._entity_candidate_summary(
                            table, candidates
                        ),
                    }
            return target, {
                "status": "matched",
                "evidence": evidence,
                "candidate_count": 1,
            }
        return None, {
            "status": "unmatched",
            "evidence": "none",
            "reason": "no-unique-postgresql-entity-candidate",
            "candidate_count": 0,
            "candidates": [],
        }

    @staticmethod
    def _find_match(table: str, source: dict[str, Any], maps: dict[str, Any]) -> dict[str, Any] | None:
        if table == "artists":
            return SQLiteReconciler._find_artist_match(source, maps)[0]
        return SQLiteReconciler._find_entity_match(table, source, maps)[0]

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
            "DO UPDATE SET last_run_id=excluded.last_run_id,source_label=excluded.source_label,"
            "source_value=excluded.source_value,target_value=excluded.target_value,"
            "resolution=excluded.resolution,application_outcome=excluded.application_outcome,"
            "error='',applied_at=NULL,last_seen_at=now()",
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
            self._require_idle()
            source = self._source(source_path)
            target = Path(str(target_root or self.target_root)).resolve()
            if target != self.target_root:
                raise PermissionError("The target mapping must be the configured WebGUI music library")
            before = self._fingerprint(source)
            run_id = uuid4().hex
            label = self._source_label(source)
            counts = {"differences": 0, "safe": 0, "conflicts": 0, "derived": 0}
            table_counts: dict[str, dict[str, int]] = {}
            active_table = ""
            self._event(
                "sqlite-reconciliation",
                {"run_id": run_id, "source": label, "trigger": trigger},
                stage="compare",
                request_status="running",
            )
            try:
                uri = source.as_uri() + "?mode=ro&immutable=1"
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
                    # A queued job must not start after the preliminary idle
                    # check and while catalog authority is being compared.
                    # SHARE blocks enqueue/claim updates for this transaction;
                    # the state check also refuses work already in the queue.
                    raw.execute("LOCK TABLE web_jobs IN SHARE MODE")
                    if raw.execute(
                        "SELECT 1 FROM web_jobs "
                        "WHERE state IN ('staged','queued','running','waiting') LIMIT 1"
                    ).fetchone():
                        raise RuntimeError(
                            "SQLite reconciliation is deferred while Album processing is active"
                        )
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
                    table_counts = {
                        name: {
                            "source_rows": len(source_rows[name]),
                            "target_rows": len(target_rows[name]),
                            "matched": 0,
                            "unmapped": 0,
                            "unchanged": 0,
                            "differences": 0,
                            "inserts": 0,
                            "updates": 0,
                            "safe": 0,
                            "conflicts": 0,
                            "derived": 0,
                            "rejected": 0,
                            "failed": 0,
                        }
                        for name in table_names
                    }
                    target_track_paths = {
                        str(row.get("path") or "").replace("\\", "/").casefold()
                        for row in target_rows["tracks"]
                        if row.get("path")
                    }

                    artist_maps = self._match_maps("artists", target_rows["artists"])
                    artist_keys: dict[str, str] = {}
                    unresolved_artist_keys: set[str] = set()
                    ready_artist_keys: set[str] = set()
                    projected_artists = []
                    active_table = "artists"
                    for row in source_rows["artists"]:
                        mapped = self._map_row("artists", row, str(source_root), str(target))
                        mapped["_reconciliation_source_key"] = str(
                            mapped.get("artist_key") or ""
                        )
                        match, identity = self._find_artist_match(mapped, artist_maps)
                        source_key = str(row.get("artist_key") or "")
                        mapped["_reconciliation_source_key"] = source_key
                        if match:
                            mapped["artist_key"] = match["artist_key"]
                            ready_artist_keys.add(source_key)
                        else:
                            if identity.get("status") == "unmatched":
                                identity = self._insert_identity("artists", mapped)
                            mapped["_reconciliation_identity"] = identity
                            if identity.get("status") == "insertable":
                                ready_artist_keys.add(source_key)
                            else:
                                unresolved_artist_keys.add(source_key)
                        artist_keys[source_key] = str(mapped.get("artist_key") or source_key)
                        projected_artists.append((mapped, match))
                    duplicate_artist_keys = self._reject_source_fan_in(
                        "artists", projected_artists
                    )
                    duplicate_artist_keys.update(
                        self._reject_normalized_source_collisions(
                            "artists", projected_artists
                        )
                    )
                    unresolved_artist_keys.update(duplicate_artist_keys)
                    ready_artist_keys.difference_update(duplicate_artist_keys)

                    album_maps = self._match_maps("albums", target_rows["albums"])
                    album_keys: dict[str, str] = {}
                    unresolved_album_keys: set[str] = set()
                    ready_album_keys: set[str] = set()
                    projected_albums = []
                    active_table = "albums"
                    for row in source_rows["albums"]:
                        mapped = self._map_row("albums", row, str(source_root), str(target))
                        mapped["_reconciliation_source_key"] = str(
                            mapped.get("album_key") or ""
                        )
                        source_artist_key = str(row.get("artist_key") or "")
                        source_key = str(row.get("album_key") or "")
                        mapped["_reconciliation_source_key"] = source_key
                        mapped["artist_key"] = artist_keys.get(source_artist_key, row.get("artist_key"))
                        if source_artist_key in unresolved_artist_keys:
                            match = None
                            identity = {
                                "status": "blocked",
                                "reason": "artist-association-unresolved",
                                "source_artist_key": source_artist_key,
                            }
                        else:
                            match, identity = self._find_entity_match(
                                "albums", mapped, album_maps
                            )
                            if not match and identity.get("status") == "unmatched":
                                identity = self._insert_identity(
                                    "albums",
                                    mapped,
                                    parent_ready=source_artist_key in ready_artist_keys,
                                )
                        if match:
                            mapped["album_key"] = match["album_key"]
                            ready_album_keys.add(source_key)
                        else:
                            mapped["_reconciliation_identity"] = identity
                            if identity.get("status") == "insertable":
                                ready_album_keys.add(source_key)
                            else:
                                unresolved_album_keys.add(source_key)
                        album_keys[source_key] = str(mapped.get("album_key") or source_key)
                        projected_albums.append((mapped, match))
                    duplicate_album_keys = self._reject_source_fan_in(
                        "albums", projected_albums
                    )
                    duplicate_album_keys.update(
                        self._reject_normalized_source_collisions(
                            "albums", projected_albums
                        )
                    )
                    unresolved_album_keys.update(duplicate_album_keys)
                    ready_album_keys.difference_update(duplicate_album_keys)

                    track_maps = self._match_maps("tracks", target_rows["tracks"])
                    projected_tracks = []
                    ready_track_paths: set[str] = set(target_track_paths)
                    active_table = "tracks"
                    for row in source_rows["tracks"]:
                        mapped = self._map_row("tracks", row, str(source_root), str(target))
                        mapped["_reconciliation_source_key"] = str(
                            mapped.get("track_key") or ""
                        )
                        source_album_key = str(row.get("album_key") or "")
                        mapped["_reconciliation_source_key"] = str(
                            row.get("track_key") or ""
                        )
                        mapped["album_key"] = album_keys.get(source_album_key, row.get("album_key"))
                        if source_album_key in unresolved_album_keys:
                            match = None
                            identity = {
                                "status": "blocked",
                                "reason": "artist-association-unresolved",
                                "source_album_key": source_album_key,
                            }
                        else:
                            match, identity = self._find_entity_match(
                                "tracks", mapped, track_maps
                            )
                            if not match and identity.get("status") == "unmatched":
                                identity = self._insert_identity(
                                    "tracks",
                                    mapped,
                                    parent_ready=source_album_key in ready_album_keys,
                                )
                        if match:
                            mapped["track_key"] = match["track_key"]
                            ready_track_paths.add(
                                str(mapped.get("path") or "")
                                .replace("\\", "/")
                                .casefold()
                            )
                        else:
                            mapped["_reconciliation_identity"] = identity
                            if identity.get("status") == "insertable":
                                ready_track_paths.add(
                                    str(mapped.get("path") or "")
                                    .replace("\\", "/")
                                    .casefold()
                                )
                        projected_tracks.append((mapped, match))
                    duplicate_track_keys = self._reject_source_fan_in(
                        "tracks", projected_tracks
                    )
                    duplicate_track_keys.update(
                        self._reject_normalized_source_collisions(
                            "tracks", projected_tracks
                        )
                    )
                    if duplicate_track_keys:
                        for source_row, _target_row in projected_tracks:
                            if str(
                                source_row.get("_reconciliation_source_key") or ""
                            ) in duplicate_track_keys:
                                ready_track_paths.discard(
                                    str(source_row.get("path") or "")
                                    .replace("\\", "/")
                                    .casefold()
                                )

                    prepared: dict[str, list[tuple[dict[str, Any], dict[str, Any] | None]]] = {
                        "artists": projected_artists,
                        "albums": projected_albums,
                        "tracks": projected_tracks,
                    }
                    for table in table_names[3:]:
                        active_table = table
                        maps = self._match_maps(table, target_rows[table]) if table != "cache_history" else {}
                        existing_history = {
                            _digest({key: value for key, value in row.items() if key != "history_id"})
                            for row in target_rows[table]
                        } if table == "cache_history" else set()
                        rows = []
                        for row in source_rows[table]:
                            mapped = self._map_row(table, row, str(source_root), str(target))
                            source_album_key = str(row.get("album_key") or "")
                            if table in {"cache_entries", "cache_history"} and source_album_key:
                                mapped["album_key"] = album_keys.get(
                                    source_album_key, row.get("album_key")
                                )
                                if source_album_key in unresolved_album_keys:
                                    mapped["_reconciliation_identity"] = {
                                        "status": "blocked",
                                        "reason": "album-association-unresolved",
                                        "source_album_key": source_album_key,
                                    }
                            if table == "cache_history":
                                mapped.pop("history_id", None)
                                match = mapped if _digest(mapped) in existing_history else None
                            else:
                                match, identity = self._find_entity_match(
                                    table, mapped, maps
                                )
                                if (
                                    match is None
                                    and identity.get("status") != "unmatched"
                                    and not mapped.get("_reconciliation_identity")
                                ):
                                    mapped["_reconciliation_identity"] = identity
                            rows.append((mapped, match))
                        self._reject_normalized_source_collisions(table, rows)
                        prepared[table] = rows

                    # Exact unchanged associations are Windows-managed even
                    # when this comparison produces no difference to Apply.
                    # Non-key identity matches are promoted only after a
                    # successful Apply, while ambiguous/blocked rows are never
                    # marked as authoritative.
                    for table in WINDOWS_MANAGED_TABLES:
                        key_name = AUTHORITY_TABLE_KEYS[table]
                        for source_row, target_row in prepared[table]:
                            identity = source_row.get("_reconciliation_identity") or {}
                            if (
                                target_row is not None
                                and str(identity.get("status") or "")
                                not in {"ambiguous", "blocked", "conflict", "rejected"}
                                and str(source_row.get("_reconciliation_source_key") or "")
                                == str(target_row.get(key_name) or "")
                            ):
                                mark_windows_authority(
                                    raw,
                                    table,
                                    target_row[key_name],
                                    before,
                                    label,
                                )

                    for table in table_names:
                        active_table = table
                        columns = target_columns[table]
                        stats = table_counts[table]
                        recorded_differences: set[tuple[str, str, str, str]] = set()
                        for source_row, target_row in prepared[table]:
                            stats["matched" if target_row is not None else "unmapped"] += 1
                            source_value = self._projection(table, source_row, columns)
                            if source_row.get("_reconciliation_identity"):
                                source_value["_reconciliation_identity"] = source_row[
                                    "_reconciliation_identity"
                                ]
                            target_value = self._projection(
                                table, target_row, list(source_value)
                            ) if target_row else None
                            if target_value is not None and _digest(source_value) == _digest(target_value):
                                stats["unchanged"] += 1
                                continue
                            resolution = classify_difference(table, source_value, target_value)
                            # A completion record may be inserted only for a
                            # track that the WebGUI catalog already indexes.
                            # Source-only/hidden Windows folders remain visible
                            # conflicts and cannot expand the target catalog.
                            if table == "compilation_track_artwork" and target_row is None:
                                track_path = str(source_value.get("track_path") or "").replace("\\", "/").casefold()
                                resolution = "safe" if track_path in ready_track_paths else "conflict"
                                if resolution == "conflict":
                                    source_value["_reconciliation_identity"] = {
                                        "status": "rejected",
                                        "evidence": "mapped-track-path",
                                        "reason": "track-not-present-in-authoritative-catalog",
                                        "mapped_path": source_value.get("track_path") or "",
                                    }
                            identity = source_value.get("_reconciliation_identity") or {}
                            difference_identity = (
                                self._key(table, source_value),
                                "derived"
                                if resolution == "derived"
                                else "insert"
                                if target_value is None
                                else "update",
                                _digest(target_value) if target_value is not None else "",
                                _digest(source_value),
                            )
                            if difference_identity in recorded_differences:
                                continue
                            recorded_differences.add(difference_identity)
                            if str(identity.get("status") or "") in {
                                "ambiguous",
                                "blocked",
                                "conflict",
                                "rejected",
                            }:
                                stats["rejected"] += 1
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
                            stats["differences"] += 1
                            stats["inserts" if target_row is None else "updates"] += 1
                            stats[
                                "safe"
                                if resolution == "safe"
                                else "derived"
                                if resolution == "derived"
                                else "conflicts"
                            ] += 1
                            counts["differences"] += 1
                            counts["safe" if resolution == "safe" else "derived" if resolution == "derived" else "conflicts"] += 1
                    after = self._fingerprint(source)
                    if after != before:
                        raise ValueError("SQLite snapshot changed during comparison; retry with a stable snapshot")
                    raw.execute(
                        "UPDATE sqlite_reconciliation_runs SET status='compared',differences=%s,safe_differences=%s,"
                        "conflicts=%s,table_counts=%s,finished_at=now() WHERE run_id=%s",
                        (
                            counts["differences"],
                            counts["safe"],
                            counts["conflicts"],
                            Jsonb(table_counts),
                            run_id,
                        ),
                    )
                result = {
                    "run_id": run_id,
                    "source_fingerprint": before,
                    "source": label,
                    **counts,
                    "tables": table_counts,
                }
                self._event(
                    "sqlite-reconciliation",
                    result,
                    stage="compare",
                    request_status="completed",
                )
                return result
            except Exception as error:
                message = sanitize_text(error, limit=1000)
                if active_table in table_counts:
                    table_counts[active_table]["failed"] += 1
                try:
                    with closing(self.database.connect()) as failure_db, failure_db:
                        failure_db.raw.execute(
                            "INSERT INTO sqlite_reconciliation_runs(run_id,source_fingerprint,source_label,trigger,status,failures,error,table_counts,finished_at) "
                            "VALUES(%s,%s,%s,%s,'failed',1,%s,%s,now()) ON CONFLICT(run_id) DO UPDATE SET "
                            "status='failed',failures=1,error=excluded.error,table_counts=excluded.table_counts,finished_at=now()",
                            (
                                run_id,
                                before,
                                label,
                                sanitize_text(trigger, limit=30),
                                message,
                                Jsonb(table_counts),
                            ),
                        )
                except Exception:
                    pass
                self._event(
                    "sqlite-reconciliation",
                    {"run_id": run_id, "source": label, "tables": table_counts},
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
        from splined_media_database import aggregate_artist

        rows = list(
            db.execute(
                "SELECT a.album_key,a.artist_key,a.path,a.compilation,a.musicbrainz_albumid,"
                "a.processed_at,a.selected_source,"
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
            elif timeout and timeout > now:
                status = "timeout"
            elif timeout:
                status = "processed"
            elif (
                bool(row["compilation"])
                and not str(row["musicbrainz_albumid"] or "").strip()
                and row["total_tracks"] is not None
            ):
                total = int(row["total_tracks"] or 0)
                completed = int(row["completed_tracks"] or 0)
                status = "processed" if total > 0 and completed >= total else "incomplete"
            elif str(row["processed_at"] or "").strip() or str(row["selected_source"] or "").strip():
                status = "processed"
            else:
                status = "unprocessed"
            db.execute(
                "UPDATE albums SET status=? WHERE album_key=?",
                (status, row["album_key"]),
            )
            by_artist.setdefault(str(row["artist_key"]), []).append(status)
        for artist_key, statuses in by_artist.items():
            counts = {name: statuses.count(name) for name in (
                "unprocessed",
                "incomplete",
                "processed",
                "bypassed",
                "timeout",
            )}
            # These counters are derived from authoritative Album state. Do
            # not stamp ``updated_at``: that shared timestamp belongs to the
            # Windows Artist row and must remain stable across repeated apply.
            db.execute(
                "UPDATE artists SET status=?,album_count=?,unprocessed_count=?,"
                "processed_count=?,bypassed_count=?,timeout_count=? WHERE artist_key=?",
                (
                    aggregate_artist(statuses),
                    len(statuses),
                    counts["unprocessed"] + counts["incomplete"],
                    counts["processed"],
                    counts["bypassed"],
                    counts["timeout"],
                    artist_key,
                ),
            )

    @staticmethod
    def _recalculate_comp_progress(db) -> None:
        albums = list(
            db.execute(
                "SELECT path FROM albums WHERE compilation<>0 "
                "AND TRIM(COALESCE(musicbrainz_albumid,''))=''"
            )
        )
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
        affected_album_keys: set[str] = set()
        with self._operation_lock, closing(self.database.connect()) as db, db:
            raw = db.raw
            # Hold the jobs table stable for the whole apply transaction. A
            # worker cannot transition a queued Album into running between the
            # idle check and status reconciliation.
            raw.execute("LOCK TABLE web_jobs IN SHARE MODE")
            if raw.execute(
                "SELECT 1 FROM web_jobs "
                "WHERE state IN ('staged','queued','running','waiting') LIMIT 1"
            ).fetchone():
                raise RuntimeError(
                    "SQLite reconciliation is deferred while Album processing is active"
                )
            raw.execute("SELECT pg_advisory_xact_lock(741520213)")
            run_row = raw.execute(
                "SELECT table_counts FROM sqlite_reconciliation_runs WHERE run_id=%s",
                (run,),
            ).fetchone()
            if not run_row:
                raise ValueError("The selected reconciliation run does not exist")
            apply_table_counts = {
                str(name): dict(values)
                for name, values in dict(run_row["table_counts"] or {}).items()
            }
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
            if any(row["entity_table"] == "cache_history" for row in rows):
                sequence = raw.execute(
                    "SELECT pg_get_serial_sequence(current_schema() || '.cache_history',"
                    "'history_id') AS sequence_name"
                ).fetchone()["sequence_name"]
                if sequence:
                    # Empty-catalog imports and older reconciliation builds may
                    # have copied explicit legacy IDs without advancing the
                    # PostgreSQL identity sequence. Advance it transactionally
                    # before inserting history rows without source IDs.
                    raw.execute(
                        "SELECT setval(%s::regclass,COALESCE((SELECT max(history_id) "
                        "FROM cache_history),0)+1,FALSE)",
                        (sequence,),
                    )
            applied = 0
            failures = 0
            failure_details: list[dict[str, str]] = []
            for difference in rows:
                table = str(difference["entity_table"])
                source = dict(difference["source_value"])
                application_value = {
                    name: value
                    for name, value in source.items()
                    if not name.startswith("_reconciliation_")
                }
                stats = apply_table_counts.setdefault(table, {})
                stats["apply_attempted"] = int(stats.get("apply_attempted", 0)) + 1
                try:
                    with raw.transaction():
                        if table == "cache_history":
                            self._insert(raw, table, application_value)
                        else:
                            current = self._row_by_key(raw, table, application_value)
                            columns = list(application_value)
                            projected = self._projection(table, current, columns) if current else None
                            current_digest = _digest(projected) if projected is not None else ""
                            if current_digest != str(difference["old_digest"]):
                                raise ValueError("PostgreSQL state changed after comparison")
                            if current is None:
                                insert_value = dict(application_value)
                                if table == "artists":
                                    insert_value.update(status="unprocessed", album_count=0, unprocessed_count=0, processed_count=0, bypassed_count=0, timeout_count=0)
                                elif table == "albums":
                                    insert_value["status"] = "unprocessed"
                                self._insert(raw, table, insert_value)
                            else:
                                self._update(raw, table, application_value)
                        raw.execute(
                            "UPDATE salite_diff SET resolution='applied',application_outcome='applied',applied_at=now(),error='' WHERE id=%s",
                            (difference["id"],),
                        )
                        if table in WINDOWS_MANAGED_TABLES:
                            mark_windows_authority(
                                raw,
                                table,
                                application_value[AUTHORITY_TABLE_KEYS[table]],
                                str(difference["source_fingerprint"]),
                                str(difference["source_label"]),
                            )
                    if table in {"albums", "tracks"} and application_value.get("album_key"):
                        affected_album_keys.add(str(application_value["album_key"]))
                    elif table == "compilation_track_artwork" and application_value.get("track_path"):
                        album = raw.execute(
                            "SELECT album_key FROM tracks WHERE path=%s",
                            (application_value["track_path"],),
                        ).fetchone()
                        if album:
                            affected_album_keys.add(str(album["album_key"]))
                    applied += 1
                    stats["applied"] = int(stats.get("applied", 0)) + 1
                except Exception as error:
                    message = sanitize_text(error, limit=1000)
                    raw.execute(
                        "UPDATE salite_diff SET resolution='skipped',application_outcome='failed',error=%s WHERE id=%s",
                        (message, difference["id"]),
                    )
                    failures += 1
                    stats["apply_failed"] = int(stats.get("apply_failed", 0)) + 1
                    if len(failure_details) < 100:
                        failure_details.append(
                            {
                                "table": table,
                                "entity_key": sanitize_text(
                                    difference["entity_key"], limit=300
                                ),
                                "error": message,
                            }
                        )
            self._recalculate_comp_progress(db)
            self._recalculate(db)
            raw.execute(
                "UPDATE sqlite_reconciliation_runs SET status=%s,applied=%s,"
                "failures=failures+%s,table_counts=%s,finished_at=now() WHERE run_id=%s",
                (
                    "applied" if failures == 0 else "applied-with-failures",
                    applied,
                    failures,
                    Jsonb(apply_table_counts),
                    run,
                ),
            )
        validations: list[dict[str, Any]] = []
        if self.album_validator:
            for album_key in sorted(affected_album_keys, key=str.casefold):
                try:
                    validations.append(dict(self.album_validator(album_key)))
                except Exception as error:
                    validations.append(
                        {
                            "album_id": album_key,
                            "status": "retry",
                            "error": sanitize_text(error, limit=500),
                        }
                    )
        validation_retries = sum(
            str(item.get("status") or "").casefold() != "verified"
            for item in validations
        )
        result = {
            "run_id": run,
            "attempted": len(rows),
            "applied": applied,
            "failures": failures,
            "complete": failures == 0 and applied == len(rows),
            "conflicts_applied": 0,
            "album_validations": validations,
            "album_validation_retries": validation_retries,
            "tables": apply_table_counts,
            "failure_details": failure_details,
        }
        self._event(
            "sqlite-reconciliation",
            result,
            severity="warning" if failures or validation_retries else "info",
            stage="apply-safe",
            request_status="failed" if failures else "retry" if validation_retries else "completed",
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
        startup_mode = str(payload.get("startup_mode", "off") or "off").strip().casefold()
        if startup_mode not in {"off", "compare", "apply-safe"}:
            raise ValueError("Startup SNAP mode must be Off, Compare only, or Apply safe")
        interval = int(payload.get("interval_minutes", 1440))
        if not 15 <= interval <= 10080:
            raise ValueError("Scheduled reconciliation interval must be 15 to 10080 minutes")
        source_path = str(payload.get("source_path", "")).strip()
        source_root = str(payload.get("source_root", "")).strip()
        if enabled or startup_mode != "off":
            self._source(source_path)
            if not source_root:
                raise ValueError("Automatic reconciliation requires the SQLite source library root")
        target = Path(str(payload.get("target_root") or self.target_root)).resolve()
        if target != self.target_root:
            raise PermissionError("Scheduled reconciliation target must be the configured music library")
        next_run = datetime.now(timezone.utc) + timedelta(minutes=interval) if enabled else None
        with closing(self.database.connect()) as db, db:
            db.raw.execute(
                "UPDATE sqlite_reconciliation_settings SET enabled=%s,auto_apply_safe=%s,startup_mode=%s,interval_minutes=%s,"
                "source_path=%s,source_root=%s,target_root=%s,updated_at=now(),next_run_at=%s WHERE singleton=TRUE",
                (enabled, auto_apply, startup_mode, interval, source_path, source_root, str(target), next_run),
            )
        return self.history(limit=1)["settings"]

    def _scheduler(self) -> None:
        state: dict[str, Any] = {}
        startup_pending = True
        deferred_reported = False
        delay = 5
        while not self._stop.wait(delay):
            delay = 30
            try:
                state = self.history(limit=1)["settings"]
                if startup_pending:
                    startup_mode = str(state.get("startup_mode") or "off")
                    if startup_mode == "off":
                        startup_pending = False
                    elif self._processing_active():
                        if not deferred_reported:
                            self._event(
                                "sqlite-reconciliation",
                                {"trigger": "startup", "startup_mode": startup_mode},
                                severity="info",
                                stage="startup",
                                request_status="waiting",
                            )
                            deferred_reported = True
                        continue
                    else:
                        try:
                            result = self.compare(
                                state.get("source_path", ""),
                                state.get("source_root", ""),
                                state.get("target_root", ""),
                                trigger="startup",
                            )
                            if startup_mode == "apply-safe":
                                self.apply(result["run_id"])
                            with closing(self.database.connect()) as db, db:
                                db.raw.execute(
                                    "UPDATE sqlite_reconciliation_settings SET last_run_at=now() WHERE singleton=TRUE"
                                )
                        except Exception as error:
                            self._event(
                                "sqlite-reconciliation",
                                {"trigger": "startup", "startup_mode": startup_mode},
                                severity="error",
                                stage="startup",
                                request_status="failed",
                                error=sanitize_text(error, limit=1000),
                            )
                        startup_pending = False
                        deferred_reported = False
                    continue
                if not state or not state.get("enabled"):
                    continue
                due = state.get("next_run_at")
                if due and _timestamp(due) and _timestamp(due) > datetime.now(timezone.utc):
                    continue
                if self._processing_active():
                    if not deferred_reported:
                        self._event(
                            "sqlite-reconciliation",
                            {"trigger": "scheduled"},
                            severity="info",
                            stage="schedule",
                            request_status="waiting",
                        )
                        deferred_reported = True
                    continue
                deferred_reported = False
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

