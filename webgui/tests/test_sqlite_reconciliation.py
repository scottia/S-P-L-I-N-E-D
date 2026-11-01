"""Safety and contract tests for incremental SQLite reconciliation."""

from __future__ import annotations

from contextlib import closing
from pathlib import Path
from tempfile import TemporaryDirectory
import sqlite3
import unittest

from webgui.reconciliation import (
    SQLiteReconciler,
    classify_difference,
    map_library_path,
)


class SQLiteReconciliationTests(unittest.TestCase):
    def test_windows_snapshot_paths_require_and_apply_explicit_root_mapping(self):
        mapped = map_library_path(
            r"D:\Music\Artist\Album\01.flac",
            r"D:\Music",
            r"E:\WebGUI Music",
        )
        normalized = mapped.replace("\\", "/")
        self.assertTrue(normalized.endswith("WebGUI Music/Artist/Album/01.flac"))
        with self.assertRaisesRegex(ValueError, "outside the mapped source"):
            map_library_path(r"D:\Other\01.flac", r"D:\Music", r"E:\Music")

    def test_only_missing_or_strictly_newer_supported_authority_is_safe(self):
        newer = {"updated_at": "2026-01-02T00:00:00+00:00"}
        older = {"updated_at": "2026-01-01T00:00:00+00:00"}
        self.assertEqual(classify_difference("albums", newer, older), "safe")
        self.assertEqual(classify_difference("albums", older, newer), "conflict")
        self.assertEqual(classify_difference("tracks", newer, None), "safe")
        self.assertEqual(classify_difference("cache_entries", newer, None), "conflict")
        self.assertEqual(
            classify_difference("compilation_album_progress", newer, older),
            "derived",
        )

    def test_authorized_source_inventory_is_bounded_to_cache_database_files(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            snapshot = root / "snapshot.sqlite"
            with closing(sqlite3.connect(snapshot)) as database, database:
                database.execute("PRAGMA user_version=2")
            (root / "notes.txt").write_text("not a database", encoding="utf-8")
            reconciler = SQLiteReconciler(
                object(), root, root / "music", start_scheduler=False
            )
            try:
                self.assertEqual(
                    [item["label"] for item in reconciler.sources()],
                    ["snapshot.sqlite"],
                )
                self.assertEqual(reconciler._source(snapshot), snapshot.resolve())
                with TemporaryDirectory() as outside:
                    other = Path(outside) / "outside.db"
                    other.touch()
                    with self.assertRaises(PermissionError):
                        reconciler._source(other)
            finally:
                reconciler.close()

    def test_migration_has_exact_history_name_idempotence_key_and_disabled_schedule(self):
        migration = (
            Path(__file__).parents[1] / "migrations" / "010_sqlite_reconciliation.sql"
        ).read_text(encoding="utf-8")
        self.assertIn("CREATE TABLE salite_diff", migration)
        self.assertIn("UNIQUE (\n        source_fingerprint", migration)
        self.assertIn("enabled BOOLEAN NOT NULL DEFAULT FALSE", migration)
        self.assertIn("auto_apply_safe BOOLEAN NOT NULL DEFAULT FALSE", migration)

    def test_ui_and_apply_contract_never_offer_conflict_application(self):
        root = Path(__file__).parents[1]
        operations = (root / "static" / "operations.js").read_text(encoding="utf-8")
        reconciliation = (root / "reconciliation.py").read_text(encoding="utf-8")
        self.assertIn("Compare / Dry Run", operations)
        self.assertIn("Apply Safe Differences", operations)
        self.assertIn("Automatically apply safe differences only", operations)
        self.assertIn("resolution='safe'", reconciliation)
        self.assertIn('"conflicts_applied": 0', reconciliation)
        self.assertNotIn("web_jobs", reconciliation)


if __name__ == "__main__":
    unittest.main()
