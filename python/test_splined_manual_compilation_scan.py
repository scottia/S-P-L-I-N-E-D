from __future__ import annotations

import json
import unittest
from unittest import mock

import splined_scan as scan


class ManualCompilationDecisionTests(unittest.TestCase):
    def test_manual_escape_returns_album_exit_decision(self) -> None:
        with (
            mock.patch.object(scan, "render_candidate_table"),
            mock.patch.object(
                scan.core,
                "read_input",
                return_value="__manual_album_exit__",
            ),
        ):
            decision, candidate, changes = scan._manual_choose_candidate(
                [], {}, ["jpeg"]
            )
        self.assertEqual(decision, "exit-album")
        self.assertIsNone(candidate)
        self.assertEqual(changes, {})

    def test_structured_authority_edit_returns_requery_decision(self) -> None:
        payload = {
            "action": "manual-authority-query",
            "artist_id": "291dcfb8-b31c-496a-905b-9955509d75b6",
            "release_id": "",
            "recording_id": "59a0c68f-ec68-418d-a29a-fa54a7d9aea9",
            "edited": "recording",
        }
        with (
            mock.patch.object(scan, "render_candidate_table"),
            mock.patch.object(
                scan.core,
                "read_input",
                return_value=json.dumps(payload),
            ),
        ):
            decision, candidate, changes = scan._manual_choose_candidate(
                [], {}, ["jpeg"]
            )
        self.assertEqual(decision, "requery")
        self.assertIsNone(candidate)
        self.assertEqual(changes["edited"], "recording")
        self.assertEqual(changes["recording_id"], payload["recording_id"])


if __name__ == "__main__":
    unittest.main()
