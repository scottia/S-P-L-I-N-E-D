from __future__ import annotations

from pathlib import Path
import unittest

from validation_scope import LANES, github_outputs, select_lanes


def boundary(*, categories, validations, paths):
    return {
        "allowed": True,
        "categories": categories,
        "validations": validations,
        "classified_paths": {path: "fixture" for path in paths},
    }


class ValidationScopeTests(unittest.TestCase):
    def test_windows_change_routes_only_to_windows_consumer(self):
        result = select_lanes(
            boundary(
                categories=["windows"],
                validations=["windows"],
                paths=["windows/src/main.rs"],
            )
        )
        self.assertTrue(result["windows"])
        self.assertFalse(result["root_native"])
        self.assertFalse(result["ubuntu_webgui"])

    def test_webgui_is_not_root_native(self):
        result = select_lanes(
            boundary(
                categories=["ubuntu-webgui"],
                validations=["ubuntu-webgui"],
                paths=["webgui/server.py"],
            )
        )
        self.assertTrue(result["ubuntu_webgui"])
        self.assertFalse(result["root_native"])
        self.assertFalse(result["windows"])

    def test_shared_contract_routes_all_actual_consumers(self):
        result = select_lanes(
            boundary(
                categories=["shared-contract"],
                validations=["root-native-other", "ubuntu-webgui", "windows"],
                paths=["config.example.toml"],
            )
        )
        self.assertTrue(result["root_native"])
        self.assertTrue(result["ubuntu_webgui"])
        self.assertTrue(result["windows"])

    def test_root_tests_remain_root_native(self):
        result = select_lanes(
            boundary(
                categories=["root-native-other"],
                validations=["root-native-other"],
                paths=["tests/history.rs"],
            )
        )
        self.assertTrue(result["root_native"])
        self.assertFalse(result["ubuntu_webgui"])

    def test_release_and_documentation_have_explicit_lanes(self):
        release = select_lanes(
            boundary(
                categories=["release-version"],
                validations=["release-version"],
                paths=["tests/release_automation.rs"],
            )
        )
        docs = select_lanes(
            boundary(
                categories=["terminology-docs"],
                validations=["terminology-docs"],
                paths=["README.md"],
            )
        )
        self.assertTrue(release["release_contract"])
        self.assertTrue(docs["documentation"])

    def test_validation_infrastructure_selects_its_tests_and_consumers(self):
        result = select_lanes(
            boundary(
                categories=["shared-contract"],
                validations=["root-native-other", "ubuntu-webgui", "windows"],
                paths=[".github/scripts/final_validation_gate.py"],
            )
        )
        self.assertTrue(result["infrastructure"])
        self.assertTrue(result["root_native"])
        self.assertTrue(result["ubuntu_webgui"])
        self.assertTrue(result["windows"])

    def test_validation_selection_is_independent_of_authorization(self):
        result = select_lanes(
            {
                "allowed": False,
                "categories": ["windows"],
                "validations": ["windows"],
                "classified_paths": {"windows/src/main.rs": "windows"},
            }
        )
        self.assertTrue(result["windows"])
        self.assertFalse(result["ubuntu_webgui"])

    def test_trusted_boundary_workflow_selects_infrastructure_and_consumers(self):
        result = select_lanes(
            boundary(
                categories=["shared-contract"],
                validations=["root-native-other", "ubuntu-webgui", "windows"],
                paths=[".github/workflows/boundary.yml"],
            )
        )
        self.assertTrue(result["infrastructure"])
        self.assertTrue(result["root_native"])
        self.assertTrue(result["ubuntu_webgui"])
        self.assertTrue(result["windows"])

    def test_outputs_account_for_every_lane(self):
        lines = github_outputs(
            boundary(
                categories=["windows"],
                validations=["windows"],
                paths=["windows/src/main.rs"],
            )
        )
        keys = {line.split("=", 1)[0] for line in lines}
        self.assertTrue(set(LANES).issubset(keys))
        self.assertIn("boundary_json", keys)
        self.assertIn("selection_json", keys)

    def test_workflows_separate_unprivileged_validation_from_trusted_control(self):
        workflow_root = Path(__file__).parents[1] / "workflows"
        validation = (workflow_root / "validate.yml").read_text(
            encoding="utf-8"
        )
        trusted = (workflow_root / "boundary.yml").read_text(encoding="utf-8")

        self.assertIn("pull_request:", validation)
        self.assertNotIn("pull_request_target", validation)
        self.assertIn("permissions:\n  contents: read", validation)
        self.assertNotIn("statuses: write", validation)
        self.assertIn("name: Validation / scope selection", validation)
        self.assertIn("name: Validation / signed workflow identity", validation)
        self.assertIn("id-token: write", validation)
        self.assertIn("actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02", validation)
        self.assertNotIn("Safeguard / final validation gate", validation)

        self.assertIn("pull_request_target:", trusted)
        self.assertIn("workflow_run:", trusted)
        self.assertIn("ref: ${{ github.workflow_sha }}", trusted)
        self.assertIn("name: Safeguard / trusted boundary evaluation", trusted)
        self.assertIn("name: Safeguard / trusted boundary decision", trusted)
        self.assertIn("statuses: write", trusted)
        self.assertIn("permissions: {}", trusted)
        self.assertIn("group: trusted-boundary-head-", trusted)
        self.assertIn("cancel-in-progress: false", trusted)
        self.assertNotIn("allow-unsafe-pr-checkout", trusted)
        self.assertNotIn("actions/download-artifact", trusted)
        self.assertNotIn("actions/cache", trusted)


if __name__ == "__main__":
    unittest.main()
