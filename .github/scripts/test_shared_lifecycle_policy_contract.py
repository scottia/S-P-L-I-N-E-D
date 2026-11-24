"""Structural coverage for the declarative shared lifecycle policy.

These tests do not open SQLite/PostgreSQL, exercise audio writers, or assert
that Windows/WebGUI have implemented a runtime lifecycle JSON loader.
"""
from __future__ import annotations

import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / "schema" / "lifecycle-policy.json"
REGISTER = ROOT / "docs" / "POLICY-REGISTER.md"
INDEX = ROOT / "docs" / "POLICY-INDEX.md"
AGENTS = ROOT / "AGENTS.md"
DOCS = ROOT / "docs" / "lifecycle-policy.md"
DOCS_README = ROOT / "docs" / "README.md"


class SharedLifecyclePolicyContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.policy = json.loads(POLICY_PATH.read_text(encoding="utf-8"))

    def test_contract_identity_and_client_scope(self):
        self.assertEqual(self.policy["schema_version"], 1)
        self.assertEqual(self.policy["policy_id"], "splined-shared-lifecycle-v1")
        self.assertEqual(set(self.policy["runtimes"]), {"windows", "webgui"})
        self.assertEqual(self.policy["enforcement_state"]["automatic_sqlite_postgresql_sync"],
                         "not provided by JSON")

    def test_exactly_nine_shared_rules_have_both_clients_and_refinements(self):
        rules = self.policy["requirements"]
        expected = [f"LIFE-{i:03d}" for i in range(1, 10)]
        self.assertEqual([entry["id"] for entry in rules], expected)
        for rule in rules:
            with self.subTest(rule=rule["id"]):
                self.assertEqual(set(rule["applies_to"]), {"windows", "webgui"})
                self.assertEqual(rule["webgui_refinement"],
                                 rule["id"].replace("LIFE-", "WEB-LIFE-"))
                self.assertTrue(rule["title"].strip())
                self.assertTrue(rule["rule"].strip())
                self.assertEqual(len(rule["acceptance"]), 3)
                self.assertTrue(all(isinstance(text, str) and text.strip()
                                    for text in rule["acceptance"]))

    def test_all_rule_ids_are_in_normative_register_and_webgui_refinements_remain(self):
        register = REGISTER.read_text(encoding="utf-8")
        for number in range(1, 10):
            with self.subTest(number=number):
                self.assertIn(f"| `LIFE-{number:03d}` |", register)
                self.assertIn(f"| `WEB-LIFE-{number:03d}` |", register)

    def test_documentation_and_agent_map_shared_contract_separately(self):
        index = INDEX.read_text(encoding="utf-8")
        agents = AGENTS.read_text(encoding="utf-8")
        doc = DOCS.read_text(encoding="utf-8")
        readme = DOCS_README.read_text(encoding="utf-8")
        self.assertIn("lifecycle-policy.json", index)
        self.assertIn("LIFE-001..009", index)
        self.assertIn("WEB-LIFE-001..009", index)
        self.assertIn("LIFE-001..009", agents)
        self.assertIn("WEB-LIFE-001..009", agents)
        self.assertIn("SQLite", doc)
        self.assertIn("PostgreSQL", doc)
        self.assertIn("lifecycle-policy.md", readme)

    def test_activation_contract_covers_native_consumers_and_protection(self):
        requirements = self.policy["activation_requirements"]
        self.assertEqual(
            set(requirements),
            {
                "runtime_consumers",
                "source_binding",
                "protection_gate",
                "performance_and_authority",
                "activation_evidence",
                "downstream_prs",
            },
        )
        self.assertTrue(all(isinstance(value, str) and value.strip()
                            for value in requirements.values()))
        self.assertIn("Windows Rust/Slint", requirements["runtime_consumers"])
        self.assertIn("WebGUI Python/browser", requirements["runtime_consumers"])
        self.assertIn("schema/lifecycle-policy.json", requirements["protection_gate"])
        self.assertIn("SQLite/PostgreSQL", requirements["performance_and_authority"])

    def test_enforcement_state_must_be_explicit_but_may_advance_with_evidence(self):
        # No false positive at this checkpoint. The acceptance validator must
        # NOT pin the runtime status to "not implemented" forever: a future
        # reviewed loader/gate PR must be able to record verified activation.
        enforcement = self.policy["enforcement_state"]
        for key in ("windows_runtime_loader", "webgui_runtime_loader",
                    "independent_json_protection_gate"):
            with self.subTest(key=key):
                self.assertIsInstance(enforcement[key], str)
                self.assertTrue(enforcement[key].strip())
        self.assertEqual(enforcement["automatic_sqlite_postgresql_sync"],
                         "not provided by JSON")


if __name__ == "__main__":
    unittest.main()
