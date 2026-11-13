"""PostgreSQL catalog access and compatibility for the existing policy queries.

The server owns a PostgreSQL database. SQLite is accepted only by the explicit
import operation; it is never used as a silent server fallback.
"""

from __future__ import annotations

from collections.abc import Mapping
from contextlib import closing
from pathlib import Path
from urllib.parse import urlsplit
import re
import sqlite3
import threading
from typing import Any

import psycopg
from psycopg_pool import ConnectionPool
from psycopg.pq import TransactionStatus
from psycopg.rows import dict_row


DEFAULT_POOL_MAX_SIZE = 8
DEFAULT_POOL_TIMEOUT_SECONDS = 5.0


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
    def __init__(self, raw, release=None):
        self.raw = raw
        self._release = release
        self._closed = False

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
        if self._closed:
            return
        self._closed = True
        if self._release is None:
            self.raw.close()
        else:
            if self.raw.info.transaction_status not in {
                TransactionStatus.IDLE,
                TransactionStatus.UNKNOWN,
            }:
                self.raw.rollback()
            self._release(self.raw)

    def __enter__(self):
        return self

    def __exit__(self, exception_type, exception, traceback):
        self.rollback() if exception_type else self.commit()


class Database:
    def __init__(
        self,
        url: str,
        *,
        pool_max_size: int = DEFAULT_POOL_MAX_SIZE,
        pool_timeout: float = DEFAULT_POOL_TIMEOUT_SECONDS,
    ):
        if not url.startswith(("postgresql://", "postgres://")):
            raise ValueError("SPLINED_DATABASE_URL must be a PostgreSQL connection URL")
        if not 1 <= int(pool_max_size) <= 32:
            raise ValueError("PostgreSQL pool_max_size must be from 1 through 32")
        if float(pool_timeout) <= 0:
            raise ValueError("PostgreSQL pool_timeout must be positive")
        self._url = url
        self._pool_max_size = int(pool_max_size)
        self._pool_timeout = float(pool_timeout)
        self._pool = None
        self._pool_lock = threading.Lock()

    def __repr__(self):
        return "PostgreSQL catalog"

    def connect(self):
        pool = self._connection_pool()
        raw = pool.getconn(timeout=self._pool_timeout)
        # Callers may deliberately use autocommit for read-only lookup work.
        # Never let that session mode leak through the pool into a later
        # generation publication, which must be one atomic transaction.
        if raw.info.transaction_status not in {
            TransactionStatus.IDLE,
            TransactionStatus.UNKNOWN,
        }:
            raw.rollback()
        raw.autocommit = False
        return Connection(
            raw,
            release=pool.putconn,
        )

    def _connection_pool(self):
        if self._pool is not None:
            return self._pool
        with self._pool_lock:
            if self._pool is None:
                # min_size=0 keeps application startup independent of database
                # connection establishment. The finite maximum reserves server
                # capacity for interactive work while an index generation runs.
                pool = ConnectionPool(
                    conninfo=self._url,
                    min_size=0,
                    max_size=self._pool_max_size,
                    timeout=self._pool_timeout,
                    kwargs={"row_factory": dict_row, "connect_timeout": 10},
                    open=False,
                    name="splined-webgui",
                )
                pool.open(wait=False)
                self._pool = pool
        return self._pool

    def pool_stats(self) -> dict[str, int]:
        pool = self._pool
        stats = pool.get_stats() if pool is not None else {}
        return {
            "max_size": self._pool_max_size,
            "pool_size": int(stats.get("pool_size", 0)),
            "pool_available": int(stats.get("pool_available", 0)),
            "requests_waiting": int(stats.get("requests_waiting", 0)),
        }

    def close(self):
        with self._pool_lock:
            pool, self._pool = self._pool, None
        if pool is not None:
            pool.close()

    def target(self) -> str:
        """Return a credential-free database identity for destructive confirms."""
        parsed = urlsplit(self._url)
        host = parsed.hostname or "localhost"
        port = f":{parsed.port}" if parsed.port else ""
        database = parsed.path.lstrip("/") or "postgres"
        return f"{host}{port}/{database}"

    def reset_application_data(self) -> dict[str, object]:
        """Atomically empty SPLINED data while preserving schema, indexes, and admin login.

        This deliberately names every table owned by the accepted schema. An
        unexpected future/reserved table is never discovered and truncated
        implicitly; a new foreign-key dependency therefore stops the reset
        transaction for explicit review instead of broadening its scope.
        """
        tables = (
            "catalog_generation_albums",
            "catalog_index_events",
            "catalog_change_manifest",
            "catalog_index_state",
            "catalog_generations",
            "artist_album_appearances",
            "artist_profiles",
            "comp_track_write_coordination",
            "album_artwork_write_coordination",
            "catalog_sync_authority",
            "compilation_track_artwork",
            "compilation_album_progress",
            "recording_release_candidates",
            "recording_release_lookups",
            "album_refresh_review_queue",
            "tracks",
            "albums",
            "artists",
            "cache_entries",
            "cache_history",
            "db_maintenance_state",
            "picker_inventory",
            "retired_album_paths",
            "web_jobs",
            "web_audit",
            "web_audit_sessions",
            "salite_diff",
            "sqlite_reconciliation_runs",
            "sqlite_reconciliation_settings",
        )
        with closing(self.connect()) as db, db:
            db.raw.execute("SELECT pg_advisory_xact_lock(741520205)")
            before = {
                name: int(
                    db.raw.execute(f'SELECT count(*) AS n FROM "{name}"').fetchone()["n"]
                )
                for name in ("artists", "albums", "tracks")
            }
            identifiers = psycopg.sql.SQL(", ").join(
                psycopg.sql.Identifier(name) for name in tables
            )
            db.raw.execute(
                psycopg.sql.SQL("TRUNCATE TABLE {} RESTART IDENTITY").format(
                    identifiers
                )
            )
            db.raw.execute(
                "INSERT INTO catalog_index_state(singleton,catalog_state) "
                "VALUES(TRUE,'initializing')"
            )
            db.raw.execute(
                "INSERT INTO sqlite_reconciliation_settings(singleton) VALUES(TRUE)"
            )
            for sequence in (
                "album_artwork_write_revision_seq",
                "comp_track_write_revision_seq",
            ):
                db.raw.execute(
                    psycopg.sql.SQL("ALTER SEQUENCE {} RESTART WITH 1").format(
                        psycopg.sql.Identifier(sequence)
                    )
                )
        return {
            "backend": "PostgreSQL",
            "target": self.target(),
            "removed": before,
            "artists": 0,
            "albums": 0,
            "tracks": 0,
            "schema_preserved": True,
            "admin_preserved": True,
        }

    def migrate(self):
        applied = []
        cleanup = {"pre_v3_audit": 0}
        migrations = sorted(Path(__file__).with_name("migrations").glob("*.sql"))
        target = max(
            (int(migration.name.split("_", 1)[0]) for migration in migrations),
            default=0,
        )
        with closing(self.connect()) as db, db:
            # One schema owner at a time, including simultaneous container starts.
            db.raw.execute("SELECT pg_advisory_xact_lock(741520204)")
            db.raw.execute(
                "CREATE TABLE IF NOT EXISTS web_schema_versions (version INTEGER PRIMARY KEY, applied_at TIMESTAMPTZ NOT NULL DEFAULT now())"
            )
            for migration in migrations:
                version = int(migration.name.split("_", 1)[0])
                if db.raw.execute(
                    "SELECT 1 FROM web_schema_versions WHERE version=%s", (version,)
                ).fetchone():
                    continue
                if version == 23:
                    cleanup["pre_v3_audit"] = int(
                        db.raw.execute(
                            "SELECT count(*) AS n FROM web_audit "
                            "WHERE application_version ~ '^[0-9]+\\.' "
                            "AND split_part(application_version,'.',1)::INTEGER < 3"
                        ).fetchone()["n"]
                    )
                db.raw.execute(migration.read_text(encoding="utf-8"))
                db.raw.execute(
                    "INSERT INTO web_schema_versions(version) VALUES(%s)", (version,)
                )
                applied.append(version)
            current = int(
                db.raw.execute(
                    "SELECT coalesce(max(version),0) AS version FROM web_schema_versions"
                ).fetchone()["version"]
            )
        return {
            "current": current,
            "target": target,
            "applied": applied,
            "automatic": True,
            "cleanup": cleanup,
        }

    def upgrade_status(self, application_version: str) -> dict[str, object]:
        """Report version drift that requires content revalidation, not relabeling."""
        major = application_version.split(".", 1)[0]
        with closing(self.connect()) as db:
            pattern = major + ".%"
            catalog_row = db.raw.execute(
                "SELECT "
                "(SELECT count(*) FROM artists WHERE splined_version<>'' "
                "AND splined_version NOT LIKE %s) AS artists,"
                "(SELECT count(*) FROM albums WHERE splined_version<>'' "
                "AND splined_version NOT LIKE %s) AS albums,"
                "(SELECT count(*) FROM tracks WHERE splined_version<>'' "
                "AND splined_version NOT LIKE %s) AS tracks",
                (pattern, pattern, pattern),
            ).fetchone()
            catalog_by_table = {
                name: int(catalog_row[name]) for name in ("artists", "albums", "tracks")
            }
            catalog = sum(catalog_by_table.values())
            audit = int(
                db.raw.execute(
                    "SELECT count(*) AS n FROM web_audit WHERE application_version<>'' "
                    "AND application_version NOT LIKE %s",
                    (major + ".%",),
                ).fetchone()["n"]
            )
        recommendations = []
        if catalog:
            recommendations.append(
                "Run Full Refresh Library Index to revalidate earlier-major catalog records"
            )
        if audit:
            recommendations.append(
                "Review diagnostic retention because earlier-major audit records remain"
            )
        return {
            "application_version": application_version,
            "catalog_older_major_records": catalog,
            "catalog_older_major_by_table": catalog_by_table,
            "audit_older_major_records": audit,
            "recommendations": recommendations,
        }

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
