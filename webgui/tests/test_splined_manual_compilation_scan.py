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

    def test_m_opens_text_discovery_and_escape_can_restore_cached_results(self) -> None:
        for response, has_results, expected in (
            ("m", False, "discover"),
            ("m", True, "mb-results"),
            ("__manual_mb_results__", True, "mb-results"),
        ):
            with (
                mock.patch.object(scan, "render_candidate_table"),
                mock.patch.object(scan.core, "read_input", return_value=response),
            ):
                decision, candidate, changes = scan._manual_choose_candidate(
                    [], {}, ["jpeg"], has_musicbrainz_results=has_results
                )
            self.assertEqual(decision, expected)
            self.assertIsNone(candidate)
            self.assertEqual(changes, {})

    def test_revisited_release_restores_candidates_without_provider_work(self) -> None:
        release = scan.core.Release(
            "5d05694f-2b0f-427e-9df8-78dbc0983681",
            "Greatest Hits 19...",
            "Jimmy Clanton",
            None,
            None,
        )
        candidate = mock.Mock(width=1200, height=1200)
        state: dict[str, object] = {}
        diagnostics = [("amazon", "temporary diagnostic")]
        with (
            mock.patch.object(
                scan.core,
                "discover_all",
                return_value=([mock.Mock()], list(diagnostics)),
            ) as discover,
            mock.patch.object(
                scan.core,
                "download_candidates",
                return_value=([candidate], []),
            ) as download,
            mock.patch.object(
                scan,
                "_candidate_resolution_label",
                return_value="1200x1200",
            ),
            mock.patch.object(scan.core, "formats", return_value=["jpeg"]),
        ):
            first = scan._manual_remote_candidates(
                mock.Mock(),
                mock.Mock(),
                {},
                release,
                ["amazon"],
                mock.Mock(),
                set(),
                state,
            )
            second = scan._manual_remote_candidates(
                mock.Mock(),
                mock.Mock(),
                {},
                release,
                ["amazon"],
                mock.Mock(),
                set(),
                state,
            )

        self.assertEqual(first, second)
        self.assertEqual(first, ([candidate], diagnostics))
        discover.assert_called_once()
        download.assert_called_once()
        self.assertTrue(download.call_args.kwargs["clean_first"])
        self.assertEqual(
            state["release_resolutions"][release.mbid],
            "1200x1200",
        )


if __name__ == "__main__":
    unittest.main()
