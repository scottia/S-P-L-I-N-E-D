"""Protocol and safety-contract tests for live catalog synchronization."""

from pathlib import Path
import unittest

from webgui.catalog_sync import ARTWORK_PROCESSING_COLUMNS, MODES


class CatalogSyncContractTests(unittest.TestCase):
    def test_migration_defaults_to_manual_snap_and_requires_a_baseline(self):
        migration = (
            Path(__file__).parents[1]
            / "migrations"
            / "018_live_catalog_synchronization.sql"
        ).read_text(encoding="utf-8")
        self.assertIn("DEFAULT 'manual-snap'", migration)
        self.assertIn("'live', 'manual-snap', 'off'", migration)
        self.assertIn("baseline_state", migration)
        self.assertIn("PRIMARY KEY(source_instance_id, local_revision)", migration)
        self.assertIn("catalog_sync_entity_state", migration)

    def test_mode_and_causal_barrier_contract_is_explicit(self):
        self.assertEqual(MODES, {"live", "manual-snap", "off"})
        self.assertIn("cover_path", ARTWORK_PROCESSING_COLUMNS)
        self.assertIn("processed_at", ARTWORK_PROCESSING_COLUMNS)
        self.assertNotIn("album_name", ARTWORK_PROCESSING_COLUMNS)


if __name__ == "__main__":
    unittest.main()
