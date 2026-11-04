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
    normalized = unicodedata.normalize("NFKC", str(album_key or ""))
    visible = "".join(
        character
        for character in normalized
        if unicodedata.category(character) != "Cf"
    ).strip()
    if not visible:
        raise ValueError("Album coordination requires a canonical Album key")
    return "key:" + visible.casefold()


def relative_album_path(album_path: object, library_root: Path) -> str:
    try:
        relative = Path(str(album_path or "")).resolve(strict=False).relative_to(
            library_root.resolve(strict=False)
        )
    except (OSError, ValueError):
        return ""
    return str(relative).replace("\\", "/").strip("/").casefold()


def new_event(album, library_root: Path, writer: str, operation: str) -> dict[str, object]:
    writer = writer.strip().casefold()
    if writer not in WRITERS:
        raise ValueError("Unknown Album artwork writer")
    album_key = str(album["album_key"])
    completed_ms = time.time_ns() // 1_000_000
    return {
        "album_identity": canonical_album_identity(album_key),
        "album_key": album_key,
        "musicbrainz_albumid": str(album.get("musicbrainz_albumid") or ""),
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
    identity = canonical_album_identity(event.get("album_key"))
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
                str(event["album_key"]),
                str(event.get("musicbrainz_albumid") or ""),
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
    with closing(database.connect()) as db:
        rows = db.raw.execute(
            "SELECT album_identity,album_key,musicbrainz_albumid,relative_path,"
            "writer,operation,event_id,source_completed_ms,revision,completed_at "
            "FROM album_artwork_write_coordination ORDER BY revision"
        ).fetchall()
    return [_json_row(row) for row in rows]
