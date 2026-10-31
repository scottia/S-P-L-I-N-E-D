from __future__ import annotations

import json
from pathlib import Path
import unittest

from check_boundary import (
    Change,
    Policy,
    analyze_changes,
    evaluate,
    parse_name_status_z,
    select_validations,
)


HERE = Path(__file__).resolve().parent


class BoundaryCheckerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.policy = Policy.from_file(HERE / "policy.json")

    @staticmethod
    def contents(values):
        return lambda path: values.get(path, ("", ""))

    def test_policy_has_exact_required_categories(self):
        self.assertEqual(
            set(self.policy.categories),
            {
                "windows",
                "ubuntu-webgui",
                "shared-contract",
                "release-version",
                "terminology-docs",
                "root-native-other",
                "boundary-policy",
            },
        )

    def test_authorized_windows_change_is_allowed(self):
        result = evaluate(
            self.policy,
            [Change("M", "windows/src/application.rs")],
            ["windows"],
        )
        self.assertTrue(result["allowed"])
        self.assertEqual(result["validations"], ["windows"])

    def test_authorized_webgui_change_is_allowed(self):
        result = evaluate(
            self.policy,
            [Change("M", "webgui/catalog.py")],
            ["ubuntu-webgui"],
        )
        self.assertTrue(result["allowed"])
        self.assertEqual(result["validations"], ["ubuntu-webgui"])

    def test_wrong_platform_authorization_is_rejected(self):
        result = evaluate(
            self.policy,
            [Change("M", "webgui/server.py")],
            ["windows"],
        )
        self.assertFalse(result["allowed"])
        self.assertIn("Missing scope authorization: ubuntu-webgui", result["errors"])

    def test_cross_platform_change_requires_each_explicit_authorization(self):
        changes = [
            Change("M", "windows/src/application.rs"),
            Change("M", "webgui/server.py"),
        ]
        rejected = evaluate(self.policy, changes, ["windows"])
        allowed = evaluate(self.policy, changes, ["windows", "ubuntu-webgui"])
        self.assertFalse(rejected["allowed"])
        self.assertTrue(allowed["allowed"])
        self.assertEqual(allowed["validations"], ["ubuntu-webgui", "windows"])

    def test_unclassified_path_is_rejected_fail_closed(self):
        result = evaluate(
            self.policy,
            [Change("A", "unexpected/new-runtime.file")],
            [],
        )
        self.assertFalse(result["allowed"])
        self.assertIn("Unclassified path: unexpected/new-runtime.file", result["errors"])

    def test_shared_contract_requires_explicit_authorization(self):
        changes = [Change("M", "schema/media.sql")]
        rejected = evaluate(self.policy, changes, ["windows", "ubuntu-webgui"])
        allowed = evaluate(self.policy, changes, ["shared-contract"])
        self.assertFalse(rejected["allowed"])
        self.assertTrue(allowed["allowed"])
        self.assertEqual(
            allowed["validations"],
            ["root-native-other", "ubuntu-webgui", "windows"],
        )

    def test_trusted_boundary_workflow_requires_shared_authorization(self):
        changes = [Change("A", ".github/workflows/boundary.yml")]
        for authorization in ([], ["windows"], ["ubuntu-webgui"]):
            with self.subTest(authorization=authorization):
                rejected = evaluate(self.policy, changes, authorization)
                self.assertFalse(rejected["allowed"])
                self.assertIn(
                    "Missing scope authorization: shared-contract",
                    rejected["errors"],
                )

        allowed = evaluate(self.policy, changes, ["shared-contract"])
        self.assertTrue(allowed["allowed"])
        self.assertEqual(allowed["categories"], ["shared-contract"])
        self.assertEqual(
            allowed["classified_paths"],
            {".github/workflows/boundary.yml": "shared-contract"},
        )
        self.assertEqual(
            allowed["validations"],
            ["root-native-other", "ubuntu-webgui", "windows"],
        )

    def test_trusted_boundary_workflow_rule_is_exact(self):
        document = json.loads((HERE / "policy.json").read_text(encoding="utf-8"))
        patterns = {
            pattern
            for rule in document["rules"]
            for pattern in rule["patterns"]
            if pattern.startswith(".github/workflows/")
        }
        self.assertIn(".github/workflows/boundary.yml", patterns)
        self.assertNotIn(".github/workflows/**", patterns)

        arbitrary = evaluate(
            self.policy,
            [Change("A", ".github/workflows/unreviewed.yml")],
            ["shared-contract"],
        )
        self.assertFalse(arbitrary["allowed"])
        self.assertIn(
            "Unclassified path: .github/workflows/unreviewed.yml",
            arbitrary["errors"],
        )

    def test_cross_boundary_rename_is_rejected_even_when_both_sides_are_authorized(self):
        result = evaluate(
            self.policy,
            [Change("R100", "webgui/moved.py", "windows/src/old.rs")],
            ["windows", "ubuntu-webgui"],
        )
        self.assertFalse(result["allowed"])
        self.assertTrue(any("Cross-boundary rename/copy" in error for error in result["errors"]))

    def test_same_boundary_rename_is_allowed(self):
        result = evaluate(
            self.policy,
            [Change("R100", "webgui/new_name.py", "webgui/old_name.py")],
            ["ubuntu-webgui"],
        )
        self.assertTrue(result["allowed"])

    def test_webgui_version_requires_release_authorization(self):
        path = "webgui/backend/splined.py"
        loader = self.contents(
            {
                path: (
                    'USER_AGENT = "SPLINED/2.0.12 (example)"\nVERSION = "2.0.12"\n',
                    'USER_AGENT = "SPLINED/2.0.13 (example)"\nVERSION = "2.0.13"\n',
                )
            }
        )
        changes = [Change("M", path)]
        rejected = evaluate(self.policy, changes, ["ubuntu-webgui"], loader)
        allowed = evaluate(
            self.policy,
            changes,
            ["ubuntu-webgui", "release-version"],
            loader,
        )
        self.assertFalse(rejected["allowed"])
        self.assertEqual(
            {item["field"] for item in rejected["protected_fields"]},
            {"WebGUI USER_AGENT", "WebGUI VERSION"},
        )
        self.assertTrue(allowed["allowed"])
        self.assertEqual(allowed["validations"], ["release-version", "ubuntu-webgui"])

    def test_ordinary_splined_python_change_does_not_require_release_scope(self):
        path = "webgui/backend/splined.py"
        before = 'USER_AGENT = "SPLINED/2.0.12 (example)"\nVERSION = "2.0.12"\nvalue = 1\n'
        after = 'USER_AGENT = "SPLINED/2.0.12 (example)"\nVERSION = "2.0.12"\nvalue = 2\n'
        result = evaluate(
            self.policy,
            [Change("M", path)],
            ["ubuntu-webgui"],
            self.contents({path: (before, after)}),
        )
        self.assertTrue(result["allowed"])
        self.assertEqual(result["categories"], ["ubuntu-webgui"])

    def test_windows_manifest_version_requires_release_authorization(self):
        path = "windows/gui/app.manifest"
        before = '<assemblyIdentity version="2.0.12.0" name="SPLINED.Windows"/>'
        after = '<assemblyIdentity version="2.0.13.0" name="SPLINED.Windows"/>'
        result = evaluate(
            self.policy,
            [Change("M", path)],
            ["windows"],
            self.contents({path: (before, after)}),
        )
        self.assertFalse(result["allowed"])
        self.assertIn("release-version", result["categories"])

    def test_boundary_policy_change_must_be_isolated(self):
        policy_only = evaluate(
            self.policy,
            [Change("M", ".github/boundaries/policy.json")],
            ["boundary-policy"],
        )
        mixed = evaluate(
            self.policy,
            [
                Change("M", ".github/boundaries/policy.json"),
                Change("M", "webgui/server.py"),
            ],
            ["boundary-policy", "ubuntu-webgui"],
        )
        self.assertTrue(policy_only["allowed"])
        self.assertFalse(mixed["allowed"])
        self.assertTrue(any("must be isolated" in error for error in mixed["errors"]))

    def test_validation_selection_is_independent_of_authorization(self):
        analysis = analyze_changes(
            self.policy,
            [Change("M", "schema/settings-ui.json")],
        )
        self.assertEqual(
            select_validations(self.policy, analysis.categories),
            ["root-native-other", "ubuntu-webgui", "windows"],
        )
        rejected = evaluate(self.policy, [Change("M", "schema/settings-ui.json")], [])
        self.assertFalse(rejected["allowed"])
        self.assertEqual(
            rejected["validations"],
            ["root-native-other", "ubuntu-webgui", "windows"],
        )

    def test_release_automation_requires_release_authorization(self):
        result = evaluate(
            self.policy,
            [Change("M", ".github/workflows/01-full-release.yml")],
            ["ubuntu-webgui"],
        )
        self.assertFalse(result["allowed"])
        self.assertEqual(result["categories"], ["release-version"])

    def test_name_status_parser_preserves_both_rename_paths(self):
        changes = parse_name_status_z(
            b"M\0webgui/server.py\0R100\0windows/old.rs\0webgui/new.py\0"
        )
        self.assertEqual(
            changes,
            [
                Change("M", "webgui/server.py"),
                Change("R100", "webgui/new.py", "windows/old.rs"),
            ],
        )

    def test_policy_document_is_valid_json(self):
        document = json.loads((HERE / "policy.json").read_text(encoding="utf-8"))
        self.assertEqual(document["schema"], 1)


if __name__ == "__main__":
    unittest.main()
