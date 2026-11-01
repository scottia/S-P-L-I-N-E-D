from __future__ import annotations

import json
from pathlib import Path
import unittest
from types import SimpleNamespace
from unittest import mock

import splined_scan as scan


class ManualCompilationDecisionTests(unittest.TestCase):
    def test_webgui_comp_opens_release_results_before_artwork_provider_work(self) -> None:
        track = SimpleNamespace(
            path=Path("Because the Night.flac"),
            artist="10,000 Maniacs",
            title="Because the Night",
            album="Billboard Hot 100 Singles of 1994",
            album_artist="Various Artists",
            artist_mbid="b9a06530-1241-4162-836f-7b8e79deaa58",
            recording_mbid="2ee5c11c-9e1a-4812-844b-c06530e5e600",
        )
        release = SimpleNamespace(release_mbid="release-id")
        identity = SimpleNamespace(
            candidates=(release,), error="", warnings=(),
            identity_status="valid", identity_message="Recording verified",
            recording_status="valid", artist_status="valid",
            friendly_artist=track.artist, friendly_title=track.title,
            focused_artist_mbid=track.artist_mbid, used_fuzzy_search=False,
        )
        with (
            mock.patch.object(
                scan.core,
                "compilation_manual_album_eligible",
                return_value=True,
                create=True,
            ),
            mock.patch.object(
                scan.core,
                "compilation_manual_track_eligible",
                return_value=(True, ""),
                create=True,
            ),
            mock.patch.object(
                scan.core,
                "compilation_mbids",
                side_effect=lambda value: {value} if value else set(),
                create=True,
            ),
            mock.patch.object(scan.core, "ui_cancelled", return_value=False),
            mock.patch.object(scan, "_manual_musicbrainz_options", return_value=object()),
            mock.patch.object(
                scan.core,
                "compilation_resolve_interactive_identity",
                return_value=identity,
                create=True,
            ) as resolve,
            mock.patch.object(
                scan.core,
                "compilation_local_artwork_rows",
                return_value=[],
                create=True,
            ),
            mock.patch.object(
                scan, "_manual_choose_release", return_value=("exit-album", None, {})
            ) as choose_release,
            mock.patch.object(scan, "_manual_remote_candidates") as remote,
            mock.patch.object(scan.core, "emit_ui"),
            mock.patch.object(scan.core, "debug_log"),
        ):
            continued = scan.run_manual_compilation_album(
                SimpleNamespace(path=Path("Billboard Hot 100 Singles of 1994")),
                [track],
                http=object(),
                config_file=Path("config.toml"),
                cfg={"mode": "read"},
                sources=[],
                cache=Path("cache"),
                format_order=["jpeg"],
                summary=SimpleNamespace(unresolved=0),
                api_queried=set(),
                unresolved_items=[],
                state={},
                target_track_paths={str(track.path)},
                musicbrainz_results_first=True,
            )

        self.assertFalse(continued)
        resolve.assert_called_once()
        choose_release.assert_called_once_with(
            [release],
            current_release_id="",
            inspected_resolutions={},
            authority={
                "artist_id": track.artist_mbid,
                "release_id": "",
                "recording_id": track.recording_mbid,
            },
            friendly={
                "artist": track.artist,
                "album": "",
                "track": track.title,
            },
            identity=identity,
            current_album={
                "artist": "Various Artists",
                "title": "Billboard Hot 100 Singles of 1994",
                "release_id": "",
            },
            existing_rows=[],
            can_back=False,
        )
        remote.assert_not_called()

    def test_advanced_ids_preserve_rejected_text_then_persist_only_validated_ids(self) -> None:
        track = SimpleNamespace(
            path=Path("Because the Night.flac"), artist="10,000 Maniacs",
            title="Because the Night", album="Billboard 1994",
            album_artist="Various Artists", album_mbid=None, compilation="1",
            artist_mbid="b9a06530-1241-4162-836f-7b8e79deaa58",
            recording_mbid="2ee5c11c-9e1a-4812-844b-c06530e5e600",
        )
        corrected_artist = "33333333-3333-4333-8333-333333333333"
        corrected_recording = "44444444-4444-4444-8444-444444444444"
        release = SimpleNamespace(
            release_mbid="55555555-5555-4555-8555-555555555555",
            recording_mbid=corrected_recording,
            recording_artist=track.artist,
            release_artist=track.artist,
            recording_title=track.title,
            release_title="Campfire Songs",
            release_date="2004", country="US", release_class="album",
            release_group_mbid="66666666-6666-4666-8666-666666666666",
            release_group_title="Campfire Songs", artist_mbids_key=corrected_artist,
            class_rank=0, score=100,
        )
        valid = SimpleNamespace(
            candidates=(release,), error="", warnings=(),
            identity_status="valid", identity_message="validated",
            recording_status="valid", artist_status="valid",
            friendly_artist=track.artist, friendly_title=track.title,
            focused_artist_mbid=corrected_artist, used_fuzzy_search=False,
        )
        invalid = SimpleNamespace(
            candidates=(), error="Invalid Artist ID", warnings=(),
            identity_status="invalid", identity_message="Invalid Artist ID",
            recording_status="unresolved", artist_status="invalid",
            friendly_artist=track.artist, friendly_title=track.title,
            focused_artist_mbid="", used_fuzzy_search=False,
        )
        prompts = []
        responses = iter([
            json.dumps({
                "action": "manual-authority-query", "artist_id": "not-a-uuid",
                "release_id": "", "recording_id": track.recording_mbid,
                "edited": "all",
            }),
            json.dumps({
                "action": "manual-authority-query", "artist_id": corrected_artist,
                "release_id": "", "recording_id": corrected_recording,
                "edited": "all",
            }),
            "__manual_album_exit__",
        ])

        def answer(_prompt, **context):
            prompts.append(context)
            return next(responses)

        persisted = []
        with (
            mock.patch.object(scan.core, "compilation_manual_album_eligible", return_value=True, create=True),
            mock.patch.object(scan.core, "compilation_manual_track_eligible", return_value=(True, ""), create=True),
            mock.patch.object(
                scan.core, "compilation_mbids",
                side_effect=lambda value: {
                    part.strip().casefold()
                    for part in str(value or "").replace(",", ";").split(";")
                    if len(part.strip()) == 36 and part.count("-") == 4
                },
                create=True,
            ),
            mock.patch.object(scan.core, "ui_cancelled", return_value=False),
            mock.patch.object(scan, "_manual_musicbrainz_options", return_value=object()),
            mock.patch.object(scan.core, "compilation_local_artwork_rows", return_value=[], create=True),
            mock.patch.object(
                scan.core, "compilation_resolve_interactive_identity",
                side_effect=[valid, invalid, valid], create=True,
            ) as resolve,
            mock.patch.object(scan.core, "read_input", side_effect=answer),
            mock.patch.object(scan.core, "emit_ui"),
            mock.patch.object(scan.core, "debug_log"),
        ):
            continued = scan.run_manual_compilation_album(
                SimpleNamespace(path=Path("Billboard 1994")), [track],
                http=object(), config_file=Path("config.toml"),
                cfg={"mode": "read"}, sources=[], cache=Path("cache"),
                format_order=["jpeg"], summary=SimpleNamespace(unresolved=0),
                api_queried=set(), unresolved_items=[], state={},
                target_track_paths={str(track.path)},
                persist_track_authority=lambda _track, recording, artist: persisted.append((recording, artist)),
                musicbrainz_results_first=True,
            )

        self.assertFalse(continued)
        self.assertEqual(resolve.call_count, 3)
        self.assertEqual(prompts[1]["authority"]["artist_mbids"], "not-a-uuid")
        self.assertEqual(prompts[2]["authority"]["artist_mbids"], corrected_artist)
        self.assertEqual(prompts[2]["authority"]["recording_mbid"], corrected_recording)
        self.assertEqual(persisted, [(corrected_recording, corrected_artist)])

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
