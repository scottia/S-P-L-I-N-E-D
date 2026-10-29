"""PostgreSQL catalog access and compatibility for the existing policy queries.

The server owns a PostgreSQL database. SQLite is accepted only by the explicit
import operation; it is never used as a silent server fallback.
"""

from __future__ import annotations

from collections.abc import Mapping
from contextlib import closing
from pathlib import Path
import re
import sqlite3
from typing import Any

import psycopg
from psycopg.rows import dict_row


class Row(dict):
    def __getitem__(self, key):
        return (
            list(self.values())[key]
            if isinstance(key, int)
            else super().__getitem__(key)
        )


class Cursor:
    def __init__(self, cursor):
        self.cursor = cursor

    def fetchone(self):
        row = self.cursor.fetchone()
        return Row(row) if row is not None else None

    def fetchall(self):
        return [Row(row) for row in self.cursor.fetchall()]

    def __iter__(self):
        for row in self.cursor:
            yield Row(row)

    @property
    def rowcount(self):
        return self.cursor.rowcount

    @property
    def description(self):
        return self.cursor.description


def policy_sql(statement: str, params: Any = ()) -> tuple[str, Any]:
    """Adapt parameter syntax used by the established Python artwork policies."""
    statement = re.sub(r"\s+COLLATE\s+NOCASE\b", "", statement, flags=re.I)
    statement = re.sub(r"\bLIKE\b", "ILIKE", statement, flags=re.I)
    statement = re.sub(
        r"\bsqlite_master\b",
        "(SELECT table_name AS name, 'table' AS type FROM information_schema.tables WHERE table_schema=current_schema()) AS sqlite_master",
        statement,
        flags=re.I,
    )
    if isinstance(params, Mapping):
        statement = re.sub(r"(?<!:):([A-Za-z_]\w*)", r"%(\1)s", statement)
    else:
        # Existing policy SQL uses bound parameters and has no question marks
        # in string literals. PostgreSQL percent literals must remain escaped.
        statement = statement.replace("%", "%%").replace("?", "%s")
    return statement, params


class Connection:
    def __init__(self, raw):
        self.raw = raw

    def execute(self, statement: str, params=()):
        if statement.strip().upper().startswith("PRAGMA"):
            return Cursor(self.raw.execute("SELECT 1 AS ignored"))
        statement, params = policy_sql(statement, params)
        return Cursor(self.raw.execute(statement, params or None))

    def executemany(self, statement: str, rows):
        rows = list(rows)
        if not rows:
            return
        statement, _ = policy_sql(statement, rows[0])
        with self.raw.cursor() as cursor:
            cursor.executemany(statement, rows)

    def commit(self):
        self.raw.commit()

    def rollback(self):
        self.raw.rollback()

    def close(self):
        self.raw.close()

    def __enter__(self):
        return self

    def __exit__(self, exception_type, exception, traceback):
        self.rollback() if exception_type else self.commit()


class Database:
    def __init__(self, url: str):
        if not url.startswith(("postgresql://", "postgres://")):
            raise ValueError("SPLINED_DATABASE_URL must be a PostgreSQL connection URL")
        self._url = url

    def __repr__(self):
        return "PostgreSQL catalog"

    def connect(self):
        return Connection(
            psycopg.connect(self._url, row_factory=dict_row, connect_timeout=10)
        )

    def migrate(self):
        with closing(self.connect()) as db, db:
            # One schema owner at a time, including simultaneous container starts.
            db.raw.execute("SELECT pg_advisory_xact_lock(741520204)")
            db.raw.execute(
                "CREATE TABLE IF NOT EXISTS web_schema_versions (version INTEGER PRIMARY KEY, applied_at TIMESTAMPTZ NOT NULL DEFAULT now())"
            )
            for migration in sorted(
                Path(__file__).with_name("migrations").glob("*.sql")
            ):
                version = int(migration.name.split("_", 1)[0])
                if db.raw.execute(
                    "SELECT 1 FROM web_schema_versions WHERE version=%s", (version,)
                ).fetchone():
                    continue
                db.raw.execute(migration.read_text(encoding="utf-8"))
                db.raw.execute(
                    "INSERT INTO web_schema_versions(version) VALUES(%s)", (version,)
                )

    def import_sqlite(self, source: Path):
        """Copy a named legacy snapshot into an empty PostgreSQL catalog.

        The source opens read-only and remains untouched. Import never replaces
        an existing server catalog or persistent job queue.
        """
        if not source.is_file():
            raise ValueError("Choose an existing SQLite catalog to import")
        with (
            sqlite3.connect(source.resolve().as_uri() + "?mode=ro", uri=True) as legacy,
            closing(self.connect()) as db,
            db,
        ):
            if db.raw.execute("SELECT count(*) AS n FROM albums").fetchone()["n"]:
                raise ValueError("The PostgreSQL catalog must be empty before import")
            legacy.row_factory = sqlite3.Row
            tables = (
                "artists",
                "albums",
                "tracks",
                "recording_release_lookups",
                "recording_release_candidates",
                "compilation_track_artwork",
                "compilation_album_progress",
                "picker_inventory",
                "cache_entries",
            )
            available = {
                row[0]
                for row in legacy.execute(
                    "SELECT name FROM sqlite_master WHERE type='table'"
                )
            }
            counts = {}
            for table in tables:
                if table not in available:
                    continue
                columns = {
                    row["column_name"]
                    for row in db.raw.execute(
                        "SELECT column_name FROM information_schema.columns WHERE table_schema=current_schema() AND table_name=%s",
                        (table,),
                    )
                }
                rows = [dict(row) for row in legacy.execute(f'SELECT * FROM "{table}"')]
                for row in rows:
                    row = {key: value for key, value in row.items() if key in columns}
                    names = list(row)
                    sql = psycopg.sql.SQL(
                        "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT DO NOTHING"
                    ).format(
                        psycopg.sql.Identifier(table),
                        psycopg.sql.SQL(",").join(map(psycopg.sql.Identifier, names)),
                        psycopg.sql.SQL(",").join(
                            psycopg.sql.Placeholder() for _ in names
                        ),
                    )
                    db.raw.execute(sql, list(row.values()))
                counts[table] = len(rows)
            return counts
