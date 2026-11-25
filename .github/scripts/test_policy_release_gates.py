"""Nonmutating tests of independent PR and pre-release policy gates."""
from __future__ import annotations

import copy
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / ".github" / "scripts"))

import check_protected_policies as pr_gate
import validate_release_policy_contracts as release_gate


class CanonicalReleaseContractTests(unittest.TestCase):
    def test_current_canonical_contracts_are_valid(self) -> None:
        release_gate.check_all()

    def test_policy_order_drift_is_rejected_by_actual_loader(self) -> None:
        media = release_gate._read_json("media-state-classification.json")
        media["artist"]["ranked_states"][0], media["artist"]["ranked_states"][1] = (
            media["artist"]["ranked_states"][1],
            media["artist"]["ranked_states"][0],
        )
        with self.assertRaisesRegex(ValueError, "ranking"):
            release_gate.validate_media_state_policy(media)

    def test_comp_policy_bad_score_is_rejected(self) -> None:
        comp = release_gate._read_json("comp-report-policy.json")
        comp["recording_search"]["minimum_score"] = -1
        with self.assertRaises(ValueError):
            release_gate.validate_comp_report_policy(comp)

    def test_settings_field_duplicates_rejected(self) -> None:
        value = release_gate._read_json("settings-ui.json")
        # Do not mutate the source: validate the copied test data only.
        value = copy.deepcopy(value)
        first = value["sections"][0]["fields"][0]
        value["sections"][0]["fields"].append(copy.deepcopy(first))
        with patch.object(release_gate, "_read_json", return_value=value):
            with self.assertRaisesRegex(ValueError, "duplicate"):
                release_gate.check_settings_ui_contract()

    def test_pr_gate_rejects_any_protected_diff(self) -> None:
        sha = "a" * 40
        with patch.object(pr_gate.subprocess, "run") as run:
            run.return_value.stdout = "M\tschema/comp-report-policy.json\n"
            with self.assertRaisesRegex(RuntimeError, "independent owner-policy"):
                pr_gate.check_diff(sha, sha)
            self.assertIn("--no-renames", run.call_args.args[0])
            self.assertIn("schema/media-state-classification.json", run.call_args.args[0])
            self.assertIn("schema/comp-report-policy.json", run.call_args.args[0])
            self.assertIn("schema/lifecycle-policy.json", run.call_args.args[0])

    def test_lifecycle_policy_add_change_delete_and_rename_fail_closed(self) -> None:
        sha = "a" * 40
        cases = {
            "add": "A\tschema/lifecycle-policy.json\n",
            "change": "M\tschema/lifecycle-policy.json\n",
            "delete": "D\tschema/lifecycle-policy.json\n",
            # --no-renames reports both sides of a rename independently.
            "rename": (
                "D\tschema/lifecycle-policy.json\n"
                "A\tschema/lifecycle-policy-renamed.json\n"
            ),
        }
        for operation, output in cases.items():
            with self.subTest(operation=operation), patch.object(
                pr_gate.subprocess, "run"
            ) as run:
                run.return_value.stdout = output
                with self.assertRaisesRegex(RuntimeError, "independent owner-policy"):
                    pr_gate.check_diff(sha, sha)
                self.assertIn("--no-renames", run.call_args.args[0])

    def test_pr_gate_accepts_unchanged_policies_and_rejects_invalid_sha(self) -> None:
        with patch.object(pr_gate.subprocess, "run") as run:
            run.return_value.stdout = ""
            pr_gate.check_diff("a" * 40, "b" * 40)
            run.assert_called_once()
        with self.assertRaisesRegex(ValueError, "invalid base"):
            pr_gate.check_diff("bad", "b" * 40)

    def test_candidate_preparation_fails_before_version_stamp_when_policy_invalid(self) -> None:
        release = (ROOT / ".github/workflows/21-release-preparation.yml").read_text(encoding="utf-8")
        self.assertLess(
            release.index("python3 .github/scripts/validate_release_policy_contracts.py"),
            release.index('source_version.py stamp-release "$RELEASE_VERSION"'),
        )
        self.assertIn("python3 .github/scripts/validate_release_policy_contracts.py", release)
        self.assertIn("Resolve successful required-validation provenance", release)
        self.assertIn('select(.name == "Validate supported runtimes"', release)
        self.assertIn("Required validation run contains an unsuccessful job", release)

    def test_pr_policy_check_is_independent_of_windows_advisory(self) -> None:
        content = (ROOT / ".github/workflows/validate.yml").read_text(encoding="utf-8")
        self.assertIn("Canonical policy JSON — review gate", content)
        self.assertIn("python3 .github/scripts/check_protected_policies.py", content)
        self.assertIn("python3 .github/scripts/validate_release_policy_contracts.py", content)
        self.assertNotIn("continue-on-error: true\n    steps:\n      - uses: actions/checkout@", content.split("  policy-review:", 1)[1].split("  webgui:", 1)[0])


if __name__ == "__main__":
    unittest.main()
