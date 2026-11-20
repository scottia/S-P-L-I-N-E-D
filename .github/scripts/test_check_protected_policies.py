"""Test protection of both canonical behavioral policy JSON files."""
import unittest
from check_protected_policies import PROTECTED_POLICIES, protected_changes

def changed(*entries):
    return ("\0".join(entries) + "\0").encode("utf-8") if entries else b""

class CanonicalPolicyPrGuardTests(unittest.TestCase):
    def test_exact_two_policy_files_are_guarded(self):
        self.assertEqual(PROTECTED_POLICIES, frozenset({
            "schema/media-state-classification.json",
            "schema/comp-report-policy.json",
        }))

    def test_unrelated_edits_allowed(self):
        self.assertEqual(protected_changes(changed("M","schema/media.sql","M","build/Windows/ui/app.slint")), [])
        self.assertEqual(protected_changes(b""), [])

    def test_media_state_change_blocked(self):
        self.assertEqual(protected_changes(changed("M","schema/media-state-classification.json")),
                         ["schema/media-state-classification.json"])

    def test_comp_report_change_blocked(self):
        self.assertEqual(protected_changes(changed("M","schema/comp-report-policy.json")),
                         ["schema/comp-report-policy.json"])

    def test_renames_and_deletions_blocked(self):
        self.assertEqual(protected_changes(changed("R100","schema/comp-report-policy.json","policy/comp-report-policy.json")),
                         ["schema/comp-report-policy.json"])
        self.assertEqual(protected_changes(changed("R098","other.json","schema/media-state-classification.json")),
                         ["schema/media-state-classification.json"])
        self.assertEqual(protected_changes(changed("D","schema/comp-report-policy.json")),
                         ["schema/comp-report-policy.json"])

    def test_fail_closed_on_corrupt_diff(self):
        for payload in (b"M\0", b"R100\0only-one-path\0", b"bogus\0foo\0"):
            with self.subTest(payload=payload), self.assertRaises(ValueError):
                protected_changes(payload)

if __name__ == "__main__":
    unittest.main()
