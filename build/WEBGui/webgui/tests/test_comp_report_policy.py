import json
import unittest

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
