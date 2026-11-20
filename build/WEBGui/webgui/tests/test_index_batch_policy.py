import unittest

from webgui.index_batch_policy import (
    AdaptiveBatchController,
    CatalogWorkload,
    TARGET_PAYLOAD_BYTES,
)


class AdaptiveBatchPolicyTests(unittest.TestCase):
    def workload(self):
        return CatalogWorkload(artists=100, albums=1000, tracks=12000, average_tracks_per_album=12)

    def test_default_and_manual_override(self):
        adaptive = AdaptiveBatchController(
            enabled=True, configured_size=3, workload=self.workload()
        )
        self.assertEqual(adaptive.current_size, 16)
        fixed = AdaptiveBatchController(
            enabled=False, configured_size=3, workload=self.workload()
        )
        for _ in range(4):
            fixed.observe_checkpoint(
                albums=3,
                tracks=3,
                payload_bytes=128,
                processing_seconds=0.002,
                checkpoint_seconds=0.001,
                interactive_pressure=False,
            )
        self.assertEqual(fixed.current_size, 3)
        self.assertEqual(fixed.decisions, [])

    def test_growth_requires_stable_checkpoint_boundaries(self):
        controller = AdaptiveBatchController(
            enabled=True, configured_size=16, workload=self.workload()
        )
        for expected in (16, 32):
            controller.observe_checkpoint(
                albums=16,
                tracks=160,
                payload_bytes=100_000,
                processing_seconds=0.2,
                checkpoint_seconds=0.01,
                interactive_pressure=False,
            )
            self.assertEqual(controller.current_size, expected)

    def test_memory_and_interactive_pressure_shrink_immediately(self):
        controller = AdaptiveBatchController(
            enabled=True, configured_size=16, workload=self.workload()
        )
        controller.observe_checkpoint(
            albums=16,
            tracks=160,
            payload_bytes=TARGET_PAYLOAD_BYTES + 1,
            processing_seconds=0.2,
            checkpoint_seconds=0.01,
            interactive_pressure=False,
        )
        self.assertEqual(controller.current_size, 8)
        controller.observe_checkpoint(
            albums=8,
            tracks=80,
            payload_bytes=100_000,
            processing_seconds=0.1,
            checkpoint_seconds=0.01,
            interactive_pressure=True,
        )
        self.assertEqual(controller.current_size, 4)

    def test_prior_published_measurement_seeds_next_generation(self):
        class Raw:
            def execute(self, statement):
                if "SELECT (SELECT count(*)" in statement:
                    return Cursor(
                        {
                            "artists": 100,
                            "albums": 1000,
                            "tracks": 100000,
                            "average_tracks": 100,
                            "maximum_tracks": 6400,
                        }
                    )
                return Cursor(
                    {
                        "timings": {
                            "albums": 1000,
                            "adaptive_batching": {
                                "enabled": True,
                                "catalog_albums": 0,
                                "final_size": 8,
                            },
                        }
                    }
                )

        class Cursor:
            def __init__(self, row):
                self.row = row

            def fetchone(self):
                return self.row

        class Connection:
            raw = Raw()

        controller = AdaptiveBatchController.from_database(
            Connection(), enabled=True, configured_size=16
        )
        self.assertEqual(controller.current_size, 8)


if __name__ == "__main__":
    unittest.main()
