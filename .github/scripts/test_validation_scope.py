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

    def test_rejected_boundary_cannot_select_validation(self):
        with self.assertRaisesRegex(ValueError, "must succeed"):
            select_lanes({"allowed": False})

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

    def test_workflow_keeps_stable_gate_and_read_only_permissions(self):
        workflow = (Path(__file__).parents[1] / "workflows" / "validate.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("name: Safeguard / boundary authorization", workflow)
        self.assertIn("name: Safeguard / final validation gate", workflow)
        self.assertIn("if: ${{ always() }}", workflow)
        self.assertIn("permissions:\n  contents: read\n  pull-requests: read", workflow)
        self.assertNotIn("pull_request_target", workflow)
        self.assertNotIn("allow-unsafe-pr-checkout", workflow)
        self.assertIn("persist-credentials: false", workflow)
        self.assertIn('test "$(git -C trusted-base rev-parse HEAD)" = "$BASE_SHA"', workflow)
        self.assertIn('test "$(git -C proposal-data rev-parse HEAD)" = "$HEAD_SHA"', workflow)
        self.assertIn('git -C proposal-data cat-file -e "$BASE_SHA^{commit}"', workflow)


if __name__ == "__main__":
    unittest.main()
