import json
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import splined_compilation_authority_policy as authority
import webgui.comp_report_policy as policy


class CompReportPolicyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.original = json.loads(policy.DEFAULT_POLICY.read_text(encoding="utf-8"))
        policy.activate_comp_report_policy(self.original, revision=0)

    def tearDown(self) -> None:
        policy.activate_comp_report_policy(self.original, revision=0)

    def test_repository_policy_is_valid(self) -> None:
        value = policy.validate_comp_report_policy(self.original)
        self.assertEqual(value["batch"]["completion_states"], [
            "completed", "unresolved", "failed",
        ])

    def test_selected_artist_precedes_slash_credit(self) -> None:
        self.assertEqual(
            policy.focused_artist_name("*NSYNC/Gloria Estefan", "*NSYNC"),
            "*NSYNC",
        )
        self.assertTrue(policy.artist_names_compatible("*NSYNC/Gloria Estefan", "NSYNC"))

    def test_comp_lookup_preserves_full_verified_artist_credit_with_slash(self) -> None:
        artist_id = "66c662b6-6e2f-4930-8610-912e24c63ed1"
        recording_id = "7578ce92-8b92-4232-b754-84bc1cb47bf4"
        track = SimpleNamespace(
            artist="AC/DC", title="Whole Lotta Rosie",
            artist_mbid=artist_id, recording_mbid=recording_id,
        )
        core = SimpleNamespace(
            mb_headers=lambda *_args: ({}, "oauth"),
            MB_BASE="https://musicbrainz.test/ws/2",
        )
        credit = [{"artist": {"id": artist_id, "name": "AC/DC"}}]
        recording = {
            "id": recording_id, "title": track.title, "artist-credit": credit,
            "releases": [{
                "id": "79f03cd7-03ae-4786-8bea-800aa8c32bf5",
                "title": "Let There Be Rock", "status": "Official",
                "artist-credit": credit,
                "release-group": {
                    "id": "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
                    "title": "Let There Be Rock", "primary-type": "Album",
                },
            }],
        }
        with patch.object(authority, "_request_musicbrainz_entity", side_effect=[
            (credit[0]["artist"], ""),
            (recording, ""),
            ({"recordings": []}, ""),
        ]) as lookup:
            result = authority.resolve_interactive_identity_matches(
                core, object(), Path("config.toml"), {}, track,
                authority.MusicBrainzOptions(1, 0.001, 7.0),
            )

        self.assertEqual(result.identity_status, "valid")
        self.assertEqual(result.artist_status, "valid")
        self.assertEqual(result.recording_status, "valid")
        self.assertEqual(result.friendly_artist, "AC/DC")
        self.assertEqual(result.candidates[0].match_basis, "exact-recording")
        self.assertTrue(lookup.call_args_list[1].args[1].endswith(f"/recording/{recording_id}"))

    def test_exact_alias_score_comes_from_the_shared_policy(self) -> None:
        self.assertEqual(
            self.original["artist_identity"]["local_artist_alias"]["exact_score"],
            100,
        )

    def test_batch_pins_policy_revision_until_context_exits(self) -> None:
        policy.activate_comp_report_policy(self.original, revision=7)
        replacement = json.loads(json.dumps(self.original))
        replacement["artist_identity"]["local_artist_alias"]["minimum_score"] = 84
        with policy.pin_comp_report_policy():
            policy.activate_comp_report_policy(replacement, revision=8)
            self.assertEqual(policy.active_policy_revision(), 7)
            self.assertEqual(
                policy.comp_report_policy()["artist_identity"]["local_artist_alias"]["minimum_score"],
                85,
            )
        self.assertEqual(policy.active_policy_revision(), 8)
        self.assertEqual(
            policy.comp_report_policy()["artist_identity"]["local_artist_alias"]["minimum_score"],
            84,
        )


if __name__ == "__main__":
    unittest.main()
