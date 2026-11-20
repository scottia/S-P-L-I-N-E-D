import os
import unittest
from unittest.mock import patch

from webgui.database import Database


class DatabaseTargetTests(unittest.TestCase):
    def test_libpq_environment_identifies_placeholder_url_target(self):
        with patch.dict(
            os.environ,
            {
                "PGHOST": "splined-postgresql-dev",
                "PGPORT": "5432",
                "PGDATABASE": "splined_dev",
            },
            clear=False,
        ):
            database = Database("postgresql://")
            self.assertEqual(
                database.target(),
                "splined-postgresql-dev:5432/splined_dev",
            )

    def test_explicit_url_remains_authoritative_over_libpq_environment(self):
        with patch.dict(
            os.environ,
            {
                "PGHOST": "ignored-host",
                "PGPORT": "6543",
                "PGDATABASE": "ignored_database",
            },
            clear=False,
        ):
            database = Database("postgresql://user:password@db:5433/catalog")
            self.assertEqual(database.target(), "db:5433/catalog")


if __name__ == "__main__":
    unittest.main()
