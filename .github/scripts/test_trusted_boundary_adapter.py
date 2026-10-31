from __future__ import annotations

from pathlib import Path
import subprocess
import tempfile
import unittest

from trusted_boundary_adapter import (
    AdapterError,
    REQUIRED_TRUSTED_FILES,
    apply_enforcement_isolation,
    authorization_fingerprint,
    proposal_from_event,
    require_trusted_files,
    repository_role,
    run_checker,
    verify_authorizations,
)


BASE = "1" * 40
HEAD = "2" * 40


def event(labels=("scope:windows",), *, base=BASE, head=HEAD):
    return {
        "number": 17,
        "pull_request": {
            "base": {"sha": base, "ref": "main"},
            "head": {"sha": head, "ref": "feature"},
            "labels": [{"name": label} for label in labels],
        },
    }


def label_event(label, *, action="labeled", actor="maintainer"):
    return {"event": action, "label": {"name": label}, "actor": {"login": actor}}


class TrustedBoundaryAdapterTests(unittest.TestCase):
    def test_single_platform_authorization_requires_verified_maintainer(self):
        scopes, records = verify_authorizations(
            event(),
            [label_event("scope:windows")],
            {"maintainer": "maintain"},
        )
        self.assertEqual(scopes, ["windows"])
        self.assertEqual(records[0]["actor"], "maintainer")

    def test_explicit_shared_authorization_is_distinct(self):
        scopes, _ = verify_authorizations(
            event(("scope:shared-contract",)),
            [label_event("scope:shared-contract", actor="admin")],
            {"admin": "admin"},
        )
        self.assertEqual(scopes, ["shared-contract"])

    def test_missing_scope_authorization_fails_closed(self):
        with self.assertRaisesRegex(AdapterError, "No explicit"):
            verify_authorizations(event(()), [], {})

    def test_unknown_scope_label_fails_closed(self):
        with self.assertRaisesRegex(AdapterError, "Unknown"):
            verify_authorizations(event(("scope:everything",)), [], {})

    def test_unverified_label_issuer_fails_closed(self):
        with self.assertRaisesRegex(AdapterError, "verified maintainer"):
            verify_authorizations(
                event(),
                [label_event("scope:windows", actor="contributor")],
                {"contributor": "write"},
            )

    def test_github_maintain_role_is_not_collapsed_to_write(self):
        self.assertEqual(
            repository_role({"permission": "write", "role_name": "maintain"}),
            "maintain",
        )

    def test_removed_authorization_fails_closed(self):
        with self.assertRaisesRegex(AdapterError, "removed"):
            verify_authorizations(
                event(),
                [
                    label_event("scope:windows"),
                    label_event("scope:windows", action="unlabeled"),
                ],
                {"maintainer": "maintain"},
            )

    def test_updated_commit_and_authorization_change_fingerprints(self):
        proposal = proposal_from_event(event())
        _, records = verify_authorizations(
            event(),
            [label_event("scope:windows")],
            {"maintainer": "maintain"},
        )
        first = authorization_fingerprint(proposal, records)
        changed_head = proposal_from_event(event(head="3" * 40))
        changed_record = [dict(records[0], actor="other")]
        self.assertNotEqual(first, authorization_fingerprint(changed_head, records))
        self.assertNotEqual(first, authorization_fingerprint(proposal, changed_record))

    def test_incomplete_proposal_identity_fails_closed(self):
        with self.assertRaisesRegex(AdapterError, "full commit SHAs"):
            proposal_from_event(event(head="short"))

    def test_each_missing_trusted_file_is_a_bootstrap_failure(self):
        for missing in REQUIRED_TRUSTED_FILES:
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                for relative in REQUIRED_TRUSTED_FILES:
                    if relative == missing:
                        continue
                    path = root / relative
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text("fixture", encoding="utf-8")
                with self.assertRaisesRegex(AdapterError, "bootstrap required"):
                    require_trusted_files(root)

    def test_adapter_invokes_the_trusted_checker_without_policy_override(self):
        repository = Path(__file__).resolve().parents[2]
        head = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=repository, text=True, encoding="utf-8"
        ).strip()
        code, result = run_checker(
            repository,
            repository,
            {"base_sha": head, "head_sha": head},
            [],
        )
        self.assertEqual(code, 0)
        self.assertTrue(result["allowed"])
        self.assertEqual(result["categories"], [])

    def test_enforcement_change_is_bootstrap_only(self):
        result = {
            "allowed": True,
            "errors": [],
            "classified_paths": {
                ".github/workflows/validate.yml": "shared-contract",
                ".github/scripts/validation_scope.py": "shared-contract",
            },
        }
        apply_enforcement_isolation(result)
        self.assertTrue(result["allowed"])
        self.assertTrue(result["requires_bootstrap"])

    def test_enforcement_mixed_with_application_change_is_rejected(self):
        result = {
            "allowed": True,
            "errors": [],
            "classified_paths": {
                ".github/workflows/validate.yml": "shared-contract",
                "webgui/server.py": "ubuntu-webgui",
            },
        }
        apply_enforcement_isolation(result)
        self.assertFalse(result["allowed"])
        self.assertTrue(any("must be isolated" in error for error in result["errors"]))

    def test_policy_tampering_mixed_with_other_paths_is_rejected(self):
        result = {
            "allowed": False,
            "errors": ["Boundary-policy changes must be isolated"],
            "classified_paths": {
                ".github/boundaries/policy.json": "boundary-policy",
                "windows/src/main.rs": "windows",
            },
        }
        apply_enforcement_isolation(result)
        self.assertFalse(result["allowed"])
        self.assertTrue(result["requires_bootstrap"])


if __name__ == "__main__":
    unittest.main()
