"""Ownership contract for Windows-synchronized PostgreSQL catalog rows."""

from __future__ import annotations

from pathlib import Path
import unittest

from webgui.catalog_authority import (
    DERIVED_PRESENTATION_FIELDS,
    POSTGRESQL_EXCLUSIVE_FIELDS,
    WINDOWS_SYNCHRONIZED_FIELDS,
    index_update_columns,
)


class CatalogAuthorityTests(unittest.TestCase):
    def test_managed_index_updates_are_limited_to_derived_or_postgresql_fields(self):
        for table in ("artists", "albums", "tracks"):
            row = {
                name: "authoritative"
                for name in WINDOWS_SYNCHRONIZED_FIELDS[table]
            }
            row.update(
                {
                    name: "local"
                    for name in (
                        DERIVED_PRESENTATION_FIELDS[table]
                        | POSTGRESQL_EXCLUSIVE_FIELDS[table]
                    )
                }
            )
            updates = set(index_update_columns(table, row, managed=True))
            self.assertFalse(updates & WINDOWS_SYNCHRONIZED_FIELDS[table])
            self.assertEqual(
                updates,
                (
                    DERIVED_PRESENTATION_FIELDS[table]
                    | POSTGRESQL_EXCLUSIVE_FIELDS[table]
                )
                - {"artist_key", "album_key", "track_key"},
            )

    def test_unmanaged_index_updates_retain_standalone_webgui_behavior(self):
        row = {
            "album_key": "album",
            "album_name": "Changed Album",
            "updated_at": "now",
            "embedded_validation_status": "verified",
        }
        self.assertEqual(
            index_update_columns("albums", row, managed=False),
            ["album_name", "updated_at", "embedded_validation_status"],
        )

    def test_authority_migration_is_explicit_and_core_catalog_only(self):
        migration = (
            Path(__file__).parents[1]
            / "migrations"
            / "016_catalog_sync_authority.sql"
        ).read_text(encoding="utf-8")
        self.assertIn("CREATE TABLE catalog_sync_authority", migration)
        self.assertIn("'artists', 'albums', 'tracks'", migration)
        self.assertIn("PRIMARY KEY(entity_table, entity_key)", migration)
        self.assertNotIn("DELETE FROM", migration.upper())


if __name__ == "__main__":
    unittest.main()
