from __future__ import annotations

import copy
import json
import unittest
from unittest.mock import patch

from webgui.lifecycle_policy import (
    DEFAULT_POLICY,
    REQUIREMENT_IDS,
    lifecycle_policy,
    lifecycle_requirement,
    validate_lifecycle_policy,
)
from webgui import server


class SharedLifecyclePolicyTests(unittest.TestCase):
    def setUp(self):
        lifecycle_policy.cache_clear()
        self.canonical = json.loads(DEFAULT_POLICY.read_text(encoding="utf-8"))

    def tearDown(self):
        lifecycle_policy.cache_clear()

    def test_runtime_loads_all_nine_native_rules(self):
        policy = lifecycle_policy()
        self.assertEqual(tuple(rule["id"] for rule in policy["requirements"]), REQUIREMENT_IDS)
        for rule_id in REQUIREMENT_IDS:
            with self.subTest(rule=rule_id):
                rule = lifecycle_requirement(rule_id)
                self.assertEqual(rule["webgui_refinement"], rule_id.replace("LIFE-", "WEB-LIFE-"))
                self.assertEqual(set(rule["applies_to"]), {"windows", "webgui"})

    def test_identity_scope_order_and_refinement_fail_closed(self):
        mutations = []
        changed = copy.deepcopy(self.canonical)
        changed["policy_id"] = "wrong"
        mutations.append(changed)
        changed = copy.deepcopy(self.canonical)
        changed["runtimes"] = ["webgui"]
        mutations.append(changed)
        changed = copy.deepcopy(self.canonical)
        changed["requirements"][0], changed["requirements"][1] = (
            changed["requirements"][1], changed["requirements"][0]
        )
        mutations.append(changed)
        changed = copy.deepcopy(self.canonical)
        changed["requirements"][0]["webgui_refinement"] = "wrong"
        mutations.append(changed)
        for value in mutations:
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    validate_lifecycle_policy(value)

    def test_webgui_fails_before_context_or_database_initialization(self):
        with (
            patch.object(server, "lifecycle_policy", side_effect=RuntimeError("invalid lifecycle")),
            patch.object(server, "Context") as context,
        ):
            with self.assertRaisesRegex(RuntimeError, "invalid lifecycle"):
                server.create_app("unused.toml", start_worker=False)
            context.assert_not_called()


if __name__ == "__main__":
    unittest.main()
