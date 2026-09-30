from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace
import sqlite3
import tempfile
import unittest

import splined_media_checkpoint_policy as policy
import splined_media_index as media_index


class MediaCheckpointPolicyTests(unittest.TestCase):
    def test_build_wrapper_checkpoints_after_original_build(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "splined.db"
            connection = sqlite3.connect(path)
            connection.execute("PRAGMA journal_mode=WAL")
            connection.execute("CREATE TABLE values_table(value INTEGER)")
            connection.commit()

            calls: list[str] = []
            original = media_index.build_index
            original_installed = policy._INSTALLED
            try:
                media_index.build_index = lambda _context, conn, _reason: (
                    conn.execute("INSERT INTO values_table VALUES(1)"),
                    conn.commit(),
                )
                policy._INSTALLED = False
                core = SimpleNamespace(
                    emit_ui=lambda *_args, **_kwargs: None,
                    debug_log=lambda message: calls.append(message),
                )
                policy.install(core)
                context = SimpleNamespace(db_path=path)
                media_index.build_index(context, connection, "test")
                self.assertEqual(
                    connection.execute("SELECT COUNT(*) FROM values_table").fetchone()[0],
                    1,
                )
                self.assertTrue(any("postbuild_checkpoint" in value for value in calls))
            finally:
                media_index.build_index = original
                policy._INSTALLED = original_installed
                connection.close()


if __name__ == "__main__":
    unittest.main()
