"""PostgreSQL catalog access and compatibility for the existing policy queries.

The server owns a PostgreSQL database. SQLite is accepted only by the explicit
import operation; it is never used as a silent server fallback.
"""

from __future__ import annotations

from collections.abc import Mapping
from contextlib import closing, contextmanager
from pathlib import Path
import re
import sqlite3
from typing import Any

import psycopg
from psycopg import sql
from psycopg.rows import dict_row


APPLICATION_DATA_TABLES = (
    "comp_track_write_coordination",
    "album_artwork_write_coordination",
    "catalog_sync_authority",
    "web_audit_sessions",
    "sqlite_reconciliation_runs",
    "salite_diff",
    "artist_album_appearances",
    "artist_profiles",
    "web_audit",
    "web_jobs",
    "retired_album_paths",
    "picker_inventory",
    "db_maintenance_state",
    "cache_history",
    "cache_entries",
    "album_refresh_review_queue",
    "compilation_album_progress",
    "compilation_track_artwork",
    "recording_release_candidates",
    "recording_release_lookups",
    "tracks",
    "albums",
    "artists",
)
REQUIRED_CATALOG_TABLES = ("artists", "albums", "tracks")


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

    def target(self) -> dict[str, str]:
        with closing(self.connect()) as db:
            row = db.raw.execute(
                "SELECT current_database() AS database,current_schema() AS schema,"
                "coalesce(inet_server_addr()::text,'local') AS server"
            ).fetchone()
        return {key: str(row[key]) for key in ("server", "database", "schema")}

    @contextmanager
    def application_operation(self):
        """Fence one logical client operation against a database reset.

        The session-level shared advisory lock spans every transaction opened by
        the operation. Reset takes the matching exclusive transaction lock, so
        it waits for active work and new work waits until the reset commits.
        """
        db = self.connect()
        try:
            db.raw.execute("SELECT pg_advisory_lock_shared(741520205)")
            yield
        finally:
            try:
                db.raw.execute("SELECT pg_advisory_unlock_shared(741520205)")
            finally:
                db.close()

    def reset_application_data(self) -> dict[str, object]:
        """Atomically empty SPLINED's data tables while retaining schema/auth."""
        target = self.target()
        with closing(self.connect()) as db, db:
            db.raw.execute("SELECT pg_advisory_xact_lock(741520205)")
            present = {
                row["table_name"]
                for row in db.raw.execute(
                    "SELECT table_name FROM information_schema.tables "
                    "WHERE table_schema=current_schema()"
                )
            }
            missing = sorted(set(REQUIRED_CATALOG_TABLES) - present)
            if missing:
                raise RuntimeError(
                    "Database reset stopped because required tables are missing: "
                    + ", ".join(missing)
                )
            tables = [name for name in APPLICATION_DATA_TABLES if name in present]
            for table in tables:
                db.raw.execute(
                    sql.SQL("LOCK TABLE {} IN ACCESS EXCLUSIVE MODE").format(
                        sql.Identifier(table)
                    )
                )
            if tables:
                db.raw.execute(
                    sql.SQL("TRUNCATE {} RESTART IDENTITY CASCADE").format(
                        sql.SQL(",").join(map(sql.Identifier, tables))
                    )
                )
            if "sqlite_reconciliation_settings" in present:
                db.raw.execute(
                    "UPDATE sqlite_reconciliation_settings "
                    "SET enabled=FALSE,auto_apply_safe=FALSE,startup_mode='off',"
                    "interval_minutes=1440,source_path='',source_root='',target_root='',"
                    "last_run_at=NULL,next_run_at=NULL,updated_at=now()"
                )
            invalid_indexes = db.raw.execute(
                "SELECT indexrelid::regclass::text AS name FROM pg_index "
                "WHERE indrelid IN (SELECT c.oid FROM pg_class c "
                "JOIN pg_namespace n ON n.oid=c.relnamespace "
                "WHERE n.nspname=current_schema()) AND NOT indisvalid"
            ).fetchall()
            if invalid_indexes:
                raise RuntimeError(
                    "Database reset stopped because invalid indexes remain: "
                    + ", ".join(row["name"] for row in invalid_indexes)
                )
            counts = {}
            for table in REQUIRED_CATALOG_TABLES:
                counts[table] = int(
                    db.raw.execute(
                        sql.SQL("SELECT count(*) AS count FROM {}").format(
                            sql.Identifier(table)
                        )
                    ).fetchone()["count"]
                )
            if any(counts.values()):
                raise RuntimeError("Database reset verification found library rows")
        return {"backend": "PostgreSQL", "target": target, "counts": counts}

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
