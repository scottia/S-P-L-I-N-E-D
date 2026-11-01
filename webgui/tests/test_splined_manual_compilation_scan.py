from __future__ import annotations

import json
import unittest
from types import SimpleNamespace
from unittest import mock

import splined_scan as scan


class ManualCompilationDecisionTests(unittest.TestCase):
    def test_webgui_candidate_choice_retains_prepared_track_artwork_request(self) -> None:
        candidate = mock.Mock()
        payload = {
            "action": "webgui-embed-track",
            "number": 1,
            "artwork": {
                "image": "/api/image/candidate",
                "adjustments": {"Brightness": 12},
                "upscale": True,
            },
        }
        with (
            mock.patch.object(scan, "fallback_top_candidates", return_value=[candidate]),
            mock.patch.object(scan, "fallback_suggested", return_value=candidate),
            mock.patch.object(scan, "render_candidate_table"),
            mock.patch.object(scan.core, "read_input", return_value=json.dumps(payload)),
        ):
            decision, selected, changes = scan._manual_choose_candidate(
                [candidate], {}, ["jpeg"]
            )

        self.assertEqual(decision, "selected")
        self.assertIs(selected, candidate)
        self.assertEqual(
            changes["webgui_artwork"],
            {
                "image": "/api/image/candidate",
                "adjustments": {"Brightness": 12},
                "upscale": True,
            },
        )

    def test_musicbrainz_result_selection_preserves_comp_track_identity(self) -> None:
        target = SimpleNamespace(
            release_mbid="8f33a714-f8f6-4c8f-b6f3-8183f48c03bb",
            recording_mbid="e1efc38f-1f86-4c2c-bd90-df1e0796f789",
            recording_artist="10,000 Maniacs",
            release_artist="Various Artists",
            recording_title="Because the Night",
            release_title="Billboard Hot 100 Singles of 1994",
            release_date="1994",
            country="US",
            release_class="compilation",
            release_group_mbid="f65ab2b0-a008-4d26-86de-63e7dc4bfba8",
            score=100,
        )
        unrelated = SimpleNamespace(
            release_mbid="f4013a2e-7c96-492f-bf5a-8fa21af9cf02",
            recording_mbid="ff667552-2e7f-4d22-9168-7dcce06f9329",
            recording_artist="Other Performer",
            release_artist="Various Artists",
            recording_title="Other Track",
            release_title="Billboard Hot 100 Singles of 1994",
            release_date="1994",
            country="US",
            release_class="compilation",
            release_group_mbid="f65ab2b0-a008-4d26-86de-63e7dc4bfba8",
            score=85,
        )
        captured: dict[str, object] = {}

        def choose(_prompt: str, **context: object) -> str:
            captured.update(context)
            options = context["options"]
            assert isinstance(options, list)
            return str(
                next(
                    index
                    for index, option in enumerate(options, 1)
                    if option["recording_id"] == target.recording_mbid
                )
            )

        with mock.patch.object(scan.core, "read_input", side_effect=choose):
            decision, selected, changes = scan._manual_choose_release(
                [unrelated, target],
                authority={
                    "artist_id": "291dcfb8-b31c-496a-905b-9955509d75b6",
                    "release_id": "",
                    "recording_id": target.recording_mbid,
                },
                friendly={
                    "artist": "10,000 Maniacs",
                    "album": "Billboard Hot 100 Singles of 1994",
                    "track": "Because the Night",
                },
            )

        self.assertEqual(decision, "selected")
        self.assertIs(selected, target)
        self.assertEqual(changes, {})
        self.assertEqual(captured["kind"], "musicbrainz-results")
        self.assertEqual(captured["authority"]["artist"], "10,000 Maniacs")
        matching = next(
            option
            for option in captured["options"]
            if option["recording_id"] == target.recording_mbid
        )
        self.assertEqual(matching["recording_artist"], "10,000 Maniacs")
        self.assertEqual(matching["track"], "Because the Night")
        self.assertEqual(
            matching["title"], "Billboard Hot 100 Singles of 1994"
        )
        self.assertEqual(
            matching["musicbrainz_url"],
            "https://musicbrainz.org/release/" + target.release_mbid,
        )

    def test_musicbrainz_result_navigation_and_authority_actions_are_track_scoped(self) -> None:
        result = SimpleNamespace(
            release_mbid="8f33a714-f8f6-4c8f-b6f3-8183f48c03bb",
            recording_mbid="e1efc38f-1f86-4c2c-bd90-df1e0796f789",
            recording_artist="10,000 Maniacs",
            release_artist="Various Artists",
            recording_title="Because the Night",
            release_title="Billboard Hot 100 Singles of 1994",
            release_date="1994",
            country="US",
            release_class="compilation",
            release_group_mbid="f65ab2b0-a008-4d26-86de-63e7dc4bfba8",
            score=100,
        )
        edited = {
            "action": "manual-authority-query",
            "artist_id": "291dcfb8-b31c-496a-905b-9955509d75b6",
            "release_id": result.release_mbid,
            "recording_id": result.recording_mbid,
            "edited": "all",
        }
        for response, expected, expected_changes in (
            ("b", "unchanged", {}),
            ("__manual_candidates__", "back-candidates", {}),
            (json.dumps(edited), "requery", {
                "artist_id": edited["artist_id"],
                "release_id": edited["release_id"],
                "recording_id": edited["recording_id"],
                "edited": "all",
            }),
        ):
            with self.subTest(response=response), mock.patch.object(
                scan.core, "read_input", return_value=response
            ):
                decision, selected, changes = scan._manual_choose_release([result])
            self.assertEqual(decision, expected)
            self.assertIsNone(selected)
            self.assertEqual(changes, expected_changes)

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
