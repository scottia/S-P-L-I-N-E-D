from __future__ import annotations

import unittest

from tui.layout import candidate_column_layout


class CandidateMinimumGridTests(unittest.TestCase):
    def test_ai_enabled_grid_fits_48_column_inner_frame_exactly(self) -> None:
        width = 48
        grid = candidate_column_layout(width, 40, ai_enabled=True)
        inner = width - 2

        self.assertTrue(all(column_width > 0 for column_width in grid.widths))
        self.assertEqual(
            sum(grid.widths) + grid.spacing * (len(grid.columns) - 1),
            inner,
        )
        self.assertEqual(grid.starts[-1] + grid.widths[-1], inner)


if __name__ == "__main__":
    unittest.main()
