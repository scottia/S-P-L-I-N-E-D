"""Durable cross-client Album artwork write provenance.

The coordination row is deliberately separate from catalog processing state.
It does not lock an Album, validate embedded art, or change Windows authority.
"""

from __future__ import annotations

from contextlib import closing
from datetime import datetime, timezone
from pathlib import Path
import time
import unicodedata
from uuid import uuid4


WRITERS = {"windows", "webgui"}


def normalized_album_key(value: object) -> str:
    normalized = unicodedata.normalize("NFKC", str(value or ""))
    visible = "".join(
        character for character in normalized if unicodedata.category(character) != "Cf"
    ).strip().casefold()
    if not visible:
        raise ValueError("Album coordination requires a canonical Album key")
    return visible


def _json_row(row) -> dict[str, object]:
    """Return a coordination row that is safe for jobs, APIs, and outboxes."""
    value = dict(row)
    for key, item in tuple(value.items()):
        if isinstance(item, datetime):
            value[key] = item.isoformat()
    return value


def canonical_album_identity(album_key: object) -> str:
    """Return the exact canonical Album key with invisible BOM characters removed.

    Punctuation and spacing remain significant.  This intentionally does not
    repeat the lossy display-name normalization that reconciliation hardened.
    """
    return "key:" + normalized_album_key(album_key)


def relative_album_path(album_path: object, library_root: Path) -> str:
    try:
        relative = Path(str(album_path or "")).resolve(strict=False).relative_to(
            library_root.resolve(strict=False)
        )
    except (OSError, ValueError):
        return ""
    return normalized_coordination_relative(relative)


def normalized_coordination_relative(value: object) -> str:
    return "".join(
        character
        for character in unicodedata.normalize("NFKC", str(value or ""))
        if unicodedata.category(character) != "Cf"
    ).replace("\\", "/").strip("/").casefold()


def normalized_relative_track_path(track_path: object, library_root: Path) -> str:
    relative = relative_album_path(track_path, library_root)
    visible = normalized_coordination_relative(relative)
    if not visible:
        raise ValueError("Track coordination requires a path inside the music library")
    return visible


def canonical_track_identity(album_key: object, relative_track_path: object) -> str:
    path = "".join(
        character
        for character in unicodedata.normalize("NFKC", str(relative_track_path or ""))
        if unicodedata.category(character) != "Cf"
    ).replace("\\", "/").strip("/").casefold()
    if not path:
        raise ValueError("Track coordination requires a relative track path")
    return canonical_album_identity(album_key) + "|track:" + path


def new_event(album, library_root: Path, writer: str, operation: str) -> dict[str, object]:
    writer = writer.strip().casefold()
    if writer not in WRITERS:
        raise ValueError("Unknown Album artwork writer")
    album_key = normalized_album_key(album["album_key"])
    completed_ms = time.time_ns() // 1_000_000
    return {
        "album_identity": canonical_album_identity(album_key),
        "album_key": album_key,
        "musicbrainz_albumid": (
            normalized_album_key(album.get("musicbrainz_albumid"))
            if str(album.get("musicbrainz_albumid") or "").strip()
            else ""
        ),
        "relative_path": relative_album_path(album.get("path"), library_root),
        "writer": writer,
        "operation": operation.strip() or "artwork-write",
        "event_id": uuid4().hex,
        "source_completed_ms": completed_ms,
        "completed_at": datetime.fromtimestamp(
            completed_ms / 1000, tz=timezone.utc
        ).isoformat(),
    }


def record_event(database, event: dict[str, object]) -> dict[str, object]:
    """Atomically retain the newest event and return the durable current row."""
    album_key = normalized_album_key(event.get("album_key"))
    identity = canonical_album_identity(album_key)
    if identity != str(event.get("album_identity") or ""):
        raise ValueError("Album coordination identity does not match its key")
    writer = str(event.get("writer") or "").casefold()
    if writer not in WRITERS:
        raise ValueError("Unknown Album artwork writer")
    with closing(database.connect()) as db, db:
        db.raw.execute(
            "SELECT pg_advisory_xact_lock(hashtextextended(%s, 0))", (identity,)
        )
        row = db.raw.execute(
            """
            INSERT INTO album_artwork_write_coordination(
                album_identity,album_key,musicbrainz_albumid,relative_path,
                writer,operation,event_id,source_completed_ms,completed_at
            ) VALUES(%s,%s,%s,%s,%s,%s,%s,%s,%s)
            ON CONFLICT(album_identity) DO UPDATE SET
                album_key=excluded.album_key,
                musicbrainz_albumid=excluded.musicbrainz_albumid,
                relative_path=excluded.relative_path,
                writer=excluded.writer,
                operation=excluded.operation,
                event_id=excluded.event_id,
                source_completed_ms=excluded.source_completed_ms,
                revision=nextval('album_artwork_write_revision_seq'),
                completed_at=excluded.completed_at,
                updated_at=now()
            WHERE excluded.source_completed_ms
                    > album_artwork_write_coordination.source_completed_ms
               OR (excluded.source_completed_ms
                    = album_artwork_write_coordination.source_completed_ms
                   AND excluded.event_id
                    > album_artwork_write_coordination.event_id)
            RETURNING *
            """,
            (
                identity,
                album_key,
                normalized_album_key(event.get("musicbrainz_albumid"))
                if str(event.get("musicbrainz_albumid") or "").strip()
                else "",
                str(event.get("relative_path") or ""),
                writer,
                str(event.get("operation") or "artwork-write"),
                str(event["event_id"]),
                int(event["source_completed_ms"]),
                str(event["completed_at"]),
            ),
        ).fetchone()
        if row is None:
            row = db.raw.execute(
                "SELECT * FROM album_artwork_write_coordination "
                "WHERE album_identity=%s",
                (identity,),
            ).fetchone()
        return _json_row(row)


def current_rows(database) -> list[dict[str, object]]:
    with closing(database.connect()) as db, db:
        rows = db.raw.execute(
            "SELECT album_identity,album_key,musicbrainz_albumid,relative_path,"
            "writer,operation,event_id,source_completed_ms,revision,completed_at "
            "FROM album_artwork_write_coordination ORDER BY revision"
        ).fetchall()
        groups: dict[str, list[dict[str, object]]] = {}
        for raw in rows:
            row = _json_row(raw)
            row["_stored_identity"] = str(row["album_identity"])
            row["_stored_key"] = str(row["album_key"])
            row["_stored_mbid"] = str(row.get("musicbrainz_albumid") or "")
            row["_stored_relative"] = str(row.get("relative_path") or "")
            key = normalized_album_key(row["album_key"])
            row["album_key"] = key
            row["musicbrainz_albumid"] = (
                normalized_album_key(row.get("musicbrainz_albumid"))
                if str(row.get("musicbrainz_albumid") or "").strip()
                else ""
            )
            row["album_identity"] = canonical_album_identity(key)
            row["relative_path"] = normalized_coordination_relative(
                row.get("relative_path")
            )
            groups.setdefault(str(row["album_identity"]), []).append(row)
        normalized_rows = []
        for identity, group in groups.items():
            mbids = {
                str(row["musicbrainz_albumid"])
                for row in group
                if str(row["musicbrainz_albumid"])
            }
            if len(mbids) > 1:
                raise ValueError(
                    f"Ambiguous Album coordination collision for {identity}; MusicBrainz identities differ"
                )
            selected = max(
                group,
                key=lambda row: (
                    int(row["source_completed_ms"]),
                    str(row["event_id"]),
                ),
            )
            dirty = len(group) > 1 or any(
                str(raw["_stored_identity"]) != identity
                or str(raw["_stored_key"]) != str(raw["album_key"])
                or str(raw["_stored_mbid"])
                != str(raw["musicbrainz_albumid"])
                or str(raw["_stored_relative"]) != str(raw["relative_path"])
                for raw in group
            )
            if dirty:
                db.raw.execute(
                    "SELECT pg_advisory_xact_lock(hashtextextended(%s, 0))",
                    (identity,),
                )
                for raw in group:
                    db.raw.execute(
                        "DELETE FROM album_artwork_write_coordination WHERE event_id=%s",
                        (str(raw["event_id"]),),
                    )
                db.raw.execute(
                    "INSERT INTO album_artwork_write_coordination("
                    "album_identity,album_key,musicbrainz_albumid,relative_path,writer,"
                    "operation,event_id,source_completed_ms,revision,completed_at,updated_at) "
                    "VALUES(%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,now())",
                    (
                        identity,
                        selected["album_key"],
                        selected["musicbrainz_albumid"],
                        selected["relative_path"],
                        selected["writer"],
                        selected["operation"],
                        selected["event_id"],
                        selected["source_completed_ms"],
                        max(int(row["revision"]) for row in group),
                        selected["completed_at"],
                    ),
                )
            normalized_rows.append(selected)
    for row in normalized_rows:
        row.pop("_stored_identity", None)
        row.pop("_stored_key", None)
        row.pop("_stored_mbid", None)
        row.pop("_stored_relative", None)
    return sorted(normalized_rows, key=lambda row: int(row["revision"]))


def new_track_event(
    album,
    track,
    library_root: Path,
    writer: str,
    operation: str,
    artwork_sha256: str,
) -> dict[str, object]:
    writer = writer.strip().casefold()
    if writer not in WRITERS:
        raise ValueError("Unknown COMP track artwork writer")
    album_key = normalized_album_key(album["album_key"])
    relative = normalized_relative_track_path(track["path"], library_root)
    completed_ms = time.time_ns() // 1_000_000
    return {
        "track_identity": canonical_track_identity(album_key, relative),
        "album_identity": canonical_album_identity(album_key),
        "album_key": album_key,
        "track_key": str(track.get("track_key") or ""),
        "relative_track_path": relative,
        "writer": writer,
        "operation": operation.strip() or "embed-comp-track",
        "event_id": uuid4().hex,
        "artwork_sha256": str(artwork_sha256 or "").strip().casefold(),
        "source_completed_ms": completed_ms,
        "completed_at": datetime.fromtimestamp(
            completed_ms / 1000, tz=timezone.utc
        ).isoformat(),
    }


def record_track_event(database, event: dict[str, object]) -> dict[str, object]:
    album_identity = canonical_album_identity(event.get("album_key"))
    relative = str(event.get("relative_track_path") or "")
    identity = canonical_track_identity(event.get("album_key"), relative)
    if identity != str(event.get("track_identity") or ""):
        raise ValueError("COMP track coordination identity does not match its path")
    writer = str(event.get("writer") or "").casefold()
    if writer not in WRITERS:
        raise ValueError("Unknown COMP track artwork writer")
    with closing(database.connect()) as db, db:
        db.raw.execute(
            "SELECT pg_advisory_xact_lock(hashtextextended(%s, 0))", (identity,)
        )
        row = db.raw.execute(
            """
            INSERT INTO comp_track_write_coordination(
                track_identity,album_identity,album_key,track_key,
                relative_track_path,writer,operation,event_id,
                artwork_sha256,source_completed_ms,completed_at
            ) VALUES(%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)
            ON CONFLICT(track_identity) DO UPDATE SET
                album_identity=excluded.album_identity,
                album_key=excluded.album_key,
                track_key=excluded.track_key,
                relative_track_path=excluded.relative_track_path,
                writer=excluded.writer,
                operation=excluded.operation,
                event_id=excluded.event_id,
                artwork_sha256=excluded.artwork_sha256,
                source_completed_ms=excluded.source_completed_ms,
                revision=nextval('comp_track_write_revision_seq'),
                completed_at=excluded.completed_at,
                updated_at=now()
            WHERE excluded.source_completed_ms
                    > comp_track_write_coordination.source_completed_ms
               OR (excluded.source_completed_ms
                    = comp_track_write_coordination.source_completed_ms
                   AND excluded.event_id
                    > comp_track_write_coordination.event_id)
            RETURNING *
            """,
            (
                identity,
                album_identity,
                str(event["album_key"]),
                str(event.get("track_key") or ""),
                relative,
                writer,
                str(event.get("operation") or "embed-comp-track"),
                str(event["event_id"]),
                str(event.get("artwork_sha256") or ""),
                int(event["source_completed_ms"]),
                str(event["completed_at"]),
            ),
        ).fetchone()
        if row is None:
            row = db.raw.execute(
                "SELECT * FROM comp_track_write_coordination WHERE track_identity=%s",
                (identity,),
            ).fetchone()
        return _json_row(row)


def current_track_rows(database) -> list[dict[str, object]]:
    with closing(database.connect()) as db, db:
        rows = db.raw.execute(
            "SELECT track_identity,album_identity,album_key,track_key,"
            "relative_track_path,writer,operation,event_id,artwork_sha256,"
            "source_completed_ms,revision,completed_at "
            "FROM comp_track_write_coordination ORDER BY revision"
        ).fetchall()
        groups: dict[str, list[dict[str, object]]] = {}
        for raw in rows:
            row = _json_row(raw)
            row["_stored_track_identity"] = str(row["track_identity"])
            row["_stored_album_identity"] = str(row["album_identity"])
            row["_stored_album_key"] = str(row["album_key"])
            row["_stored_relative"] = str(row["relative_track_path"])
            album_key = normalized_album_key(row["album_key"])
            relative = normalized_coordination_relative(
                row["relative_track_path"]
            )
            row["album_key"] = album_key
            row["album_identity"] = canonical_album_identity(album_key)
            row["relative_track_path"] = relative
            row["track_identity"] = canonical_track_identity(album_key, relative)
            groups.setdefault(str(row["track_identity"]), []).append(row)
        normalized = []
        for identity, group in groups.items():
            selected = max(
                group,
                key=lambda row: (
                    int(row["source_completed_ms"]),
                    str(row["event_id"]),
                ),
            )
            dirty = len(group) > 1 or any(
                str(row["_stored_track_identity"]) != identity
                or str(row["_stored_album_identity"])
                != str(row["album_identity"])
                or str(row["_stored_album_key"]) != str(row["album_key"])
                or str(row["_stored_relative"])
                != str(row["relative_track_path"])
                for row in group
            )
            if dirty:
                db.raw.execute(
                    "SELECT pg_advisory_xact_lock(hashtextextended(%s, 0))",
                    (identity,),
                )
                for row in group:
                    db.raw.execute(
                        "DELETE FROM comp_track_write_coordination WHERE event_id=%s",
                        (str(row["event_id"]),),
                    )
                db.raw.execute(
                    "INSERT INTO comp_track_write_coordination("
                    "track_identity,album_identity,album_key,track_key,"
                    "relative_track_path,writer,operation,event_id,artwork_sha256,"
                    "source_completed_ms,revision,completed_at,updated_at) "
                    "VALUES(%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,now())",
                    (
                        identity,
                        selected["album_identity"],
                        selected["album_key"],
                        selected["track_key"],
                        selected["relative_track_path"],
                        selected["writer"],
                        selected["operation"],
                        selected["event_id"],
                        selected["artwork_sha256"],
                        selected["source_completed_ms"],
                        max(int(row["revision"]) for row in group),
                        selected["completed_at"],
                    ),
                )
            normalized.append(selected)
    for row in normalized:
        for key in tuple(row):
            if key.startswith("_stored_"):
                row.pop(key, None)
    return sorted(normalized, key=lambda row: int(row["revision"]))
