from __future__ import annotations

import inspect
import unittest
from unittest import mock

import splined_scan


class LazyAlbumBatchTests(unittest.TestCase):
    def test_album_authority_is_prepared_only_when_iteration_advances(self) -> None:
        prepared: list[tuple[int, str]] = []

        def prepare(album: str, index: int) -> dict[str, object]:
            prepared.append((index, album))
            return {"album": album, "fallback_reason": album == "fallback"}

        records = splined_scan._iter_lazy_album_records(
            ["normal-one", "fallback", "normal-two"],
            prepare,
        )

        self.assertEqual(prepared, [])
        self.assertEqual(next(records), (1, {"album": "normal-one", "fallback_reason": False}))
        self.assertEqual(prepared, [(1, "normal-one")])
        self.assertEqual(next(records), (2, {"album": "fallback", "fallback_reason": True}))
        self.assertEqual(prepared, [(1, "normal-one"), (2, "fallback")])
        self.assertEqual(next(records), (3, {"album": "normal-two", "fallback_reason": False}))
        self.assertEqual(prepared, [(1, "normal-one"), (2, "fallback"), (3, "normal-two")])

    def test_scan_batch_does_not_materialize_or_reorder_authority_records(self) -> None:
        source = inspect.getsource(splined_scan._run_scan_dir_batch)

        self.assertIn("_iter_lazy_album_records", source)
        self.assertNotIn("fallback_records", source)
        self.assertNotIn("normal_records", source)
        self.assertNotIn("ordered_records", source)

    def test_auto_selection_accepts_only_ideal_candidates(self) -> None:
        lower = object()
        ideal = object()
        projected = {
            lower: {"acceptable": True, "range_type": "LowerRange"},
            ideal: {"acceptable": True, "range_type": "Ideal"},
        }

        with (
            mock.patch.object(
                splined_scan,
                "project_candidate",
                side_effect=lambda candidate, _cfg, _formats: projected[candidate],
            ),
            mock.patch.object(
                splined_scan,
                "candidate_key",
                side_effect=lambda candidate, _cfg, _formats: 0 if candidate is ideal else 1,
            ),
        ):
            self.assertIs(
                splined_scan.select_auto_ideal([lower, ideal], {}, ["jpeg"]),
                ideal,
            )
            self.assertIsNone(
                splined_scan.select_auto_ideal([lower], {}, ["jpeg"])
            )


if __name__ == "__main__":
    unittest.main()
