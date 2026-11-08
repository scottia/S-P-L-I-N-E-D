from __future__ import annotations

import unittest

from webgui.database import (
    APPLICATION_DATA_TABLES,
    Database,
    REQUIRED_CATALOG_TABLES,
)


class _Cursor:
    def __init__(self, rows=()):
        self.rows = list(rows)

    def fetchone(self):
        return self.rows[0] if self.rows else None

    def fetchall(self):
        return self.rows

    def __iter__(self):
        return iter(self.rows)


class _Raw:
    def __init__(self):
        self.statements = []

    def execute(self, statement, parameters=None):
        rendered = str(statement)
        self.statements.append((rendered, parameters))
        if "information_schema.tables" in rendered:
            return _Cursor(
                {"table_name": name}
                for name in (
                    *APPLICATION_DATA_TABLES,
                    "sqlite_reconciliation_settings",
                    "web_admin_accounts",
                    "web_schema_versions",
                )
            )
        if "NOT indisvalid" in rendered:
            return _Cursor()
        if "SELECT count(*) AS count" in rendered:
            return _Cursor([{"count": 0}])
        return _Cursor([{"ignored": 1}])


class _Connection:
    def __init__(self, raw):
        self.raw = raw

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def close(self):
        pass


class _Database(Database):
    def __init__(self):
        self.raw = _Raw()

    def connect(self):
        return _Connection(self.raw)

    def target(self):
        return {"server": "fixture", "database": "splined", "schema": "public"}


class DatabaseResetTests(unittest.TestCase):
    def test_application_operation_uses_reset_fence_and_releases_it(self):
        database = _Database()
        with database.application_operation():
            pass
        statements = [statement for statement, _ in database.raw.statements]
        self.assertEqual(
            statements,
            [
                "SELECT pg_advisory_lock_shared(741520205)",
                "SELECT pg_advisory_unlock_shared(741520205)",
            ],
        )

    def test_reset_retains_only_schema_and_auth_tables(self):
        self.assertNotIn("web_admin_accounts", APPLICATION_DATA_TABLES)
        self.assertNotIn("web_schema_versions", APPLICATION_DATA_TABLES)
        self.assertIn("web_jobs", APPLICATION_DATA_TABLES)
        self.assertIn("compilation_album_progress", APPLICATION_DATA_TABLES)
        self.assertEqual(REQUIRED_CATALOG_TABLES, ("artists", "albums", "tracks"))
        result = _Database().reset_application_data()
        self.assertEqual(result["counts"], {"artists": 0, "albums": 0, "tracks": 0})
        self.assertEqual(result["backend"], "PostgreSQL")

    def test_reset_disables_stale_reconciliation_repopulation(self):
        database = _Database()
        database.reset_application_data()
        statements = "\n".join(statement for statement, _ in database.raw.statements)
        self.assertIn("enabled=FALSE", statements)
        self.assertIn("startup_mode='off'", statements)
        self.assertIn("source_path=''", statements)
        self.assertIn("next_run_at=NULL", statements)


if __name__ == "__main__":
    unittest.main()
