"""Restart-safe browser authentication sessions stored in PostgreSQL."""

from __future__ import annotations

from contextlib import closing
import hashlib
from typing import Any, Mapping

from psycopg.types.json import Jsonb

from webgui.database import Database


SESSION_MAX_AGE_SECONDS = 30 * 24 * 60 * 60


def _session_hash(session_id: str) -> str:
    return hashlib.sha256(session_id.encode("utf-8")).hexdigest()


class BrowserSessionStore:
    """Persist opaque browser sessions without storing their cookie value."""

    def __init__(self, database: Database):
        self.database = database

    def load(self, session_id: str) -> dict[str, Any] | None:
        if not session_id:
            return None
        with closing(self.database.connect()) as db:
            row = db.raw.execute(
                "SELECT data FROM web_browser_sessions "
                "WHERE session_hash=%s AND expires_at>now()",
                (_session_hash(session_id),),
            ).fetchone()
        if row is None or not isinstance(row["data"], dict):
            return None
        data = dict(row["data"])
        if not isinstance(data.get("csrf"), str):
            return None
        return data

    def save(self, session_id: str, data: Mapping[str, Any]) -> None:
        with closing(self.database.connect()) as db, db:
            db.raw.execute(
                "DELETE FROM web_browser_sessions WHERE expires_at<=now()"
            )
            db.raw.execute(
                "INSERT INTO web_browser_sessions(session_hash,data,expires_at) "
                "VALUES(%s,%s,now()+interval '30 days') "
                "ON CONFLICT(session_hash) DO UPDATE SET data=excluded.data,"
                "updated_at=now(),expires_at=excluded.expires_at",
                (_session_hash(session_id), Jsonb(dict(data))),
            )

    def delete_all(self) -> None:
        with closing(self.database.connect()) as db, db:
            db.raw.execute("DELETE FROM web_browser_sessions")
