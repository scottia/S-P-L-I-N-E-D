from __future__ import annotations

from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from splined_db_audit import close, configure, connect_sqlite


class DatabaseAuditTests(unittest.TestCase):
    def test_separate_log_identifies_database_and_never_records_sql_values(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            database = root / "splined.db"
            audit = configure(root / "logs", database, shared=False)
            try:
                connection = connect_sqlite(database)
                with connection:
                    connection.execute("CREATE TABLE fixture (secret TEXT)")
                    connection.execute("INSERT INTO fixture VALUES (?)", ("private-value",))
                connection.close()
            finally:
                close()
            content = audit.read_text(encoding="utf-8")
            self.assertIn("expected_db=", content)
            self.assertIn("write_attempt", content)
            self.assertIn("table='fixture'", content)
            self.assertNotIn("private-value", content)
            self.assertEqual(audit.parent.name, "db-audit")
            self.assertTrue(audit.name.startswith("["))


if __name__ == "__main__":
    unittest.main()
