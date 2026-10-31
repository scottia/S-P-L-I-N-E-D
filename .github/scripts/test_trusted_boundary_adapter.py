from __future__ import annotations

from pathlib import Path
import subprocess
import tempfile
import unittest

from trusted_boundary_adapter import (
    AdapterError,
    BOUNDARY_WORKFLOW_PATH,
    REQUIRED_TRUSTED_FILES,
    apply_enforcement_isolation,
    authorization_fingerprint,
    event_identity,
    proposal_from_event,
    proposal_from_pull_request,
    require_trusted_files,
    repository_role,
    run_checker,
    select_canonical_validation_run,
    verify_authorizations,
)


BASE = "1" * 40
HEAD = "2" * 40
REPOSITORY = "owner/splined"
REPOSITORY_ID = 91
HEAD_REPOSITORY = "contributor/splined"
HEAD_REPOSITORY_ID = 92


def event(labels=("scope:windows",), *, base=BASE, head=HEAD, base_ref="main"):
    return {
        "number": 17,
        "pull_request": {
            "number": 17,
            "base": {"sha": base, "ref": base_ref},
            "head": {"sha": head, "ref": "feature"},
            "labels": [{"name": label} for label in labels],
        },
    }


def label_event(
    label,
    *,
    action="labeled",
    actor="maintainer",
    event_id=1001,
    created_at="2026-10-30T12:00:00Z",
):
    return {
        "id": event_id,
        "created_at": created_at,
        "event": action,
        "label": {"name": label},
        "actor": {"login": actor},
    }


def current_pull_request(*, base=BASE, head=HEAD, base_ref="main", labels=("scope:windows",)):
    return {
        "state": "open",
        "labels": [{"name": label} for label in labels],
        "base": {
            "sha": base,
            "ref": base_ref,
            "repo": {"full_name": REPOSITORY, "id": REPOSITORY_ID},
        },
        "head": {
            "sha": head,
            "ref": "feature",
            "repo": {"full_name": HEAD_REPOSITORY, "id": HEAD_REPOSITORY_ID},
        },
    }


def proposal(*, base=BASE, head=HEAD, base_ref="main"):
    return proposal_from_pull_request(
        17,
        current_pull_request(base=base, head=head, base_ref=base_ref),
        REPOSITORY,
        REPOSITORY_ID,
    )


def validation_run(
    *,
    run_id=200,
    run_number=10,
    run_attempt=1,
    base=BASE,
    head=HEAD,
    base_ref="main",
    workflow_id=300,
    repository=REPOSITORY,
    repository_id=REPOSITORY_ID,
    path=".github/workflows/validate.yml",
    name="Validate supported runtimes",
):
    return {
        "id": run_id,
        "run_number": run_number,
        "run_attempt": run_attempt,
        "workflow_id": workflow_id,
        "name": name,
        "path": path,
        "event": "pull_request",
        "repository": {"full_name": repository, "id": repository_id},
        "head_repository": {
            "full_name": HEAD_REPOSITORY,
            "id": HEAD_REPOSITORY_ID,
        },
        "pull_requests": [
            {
                "number": 17,
                "base": {"sha": base, "ref": base_ref},
                "head": {"sha": head, "ref": "feature"},
            }
        ],
    }


class TrustedBoundaryAdapterTests(unittest.TestCase):
    def test_single_platform_authorization_requires_verified_maintainer(self):
        scopes, records = verify_authorizations(
            event(),
            [label_event("scope:windows")],
            {"maintainer": "maintain"},
        )
        self.assertEqual(scopes, ["windows"])
        self.assertEqual(records[0]["actor"], "maintainer")
        self.assertEqual(records[0]["event_id"], 1001)

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
                    label_event("scope:windows", event_id=1001),
                    label_event("scope:windows", action="unlabeled", event_id=1002),
                ],
                {"maintainer": "maintain"},
            )

    def test_commit_retarget_and_label_event_change_fingerprints(self):
        first_proposal = proposal_from_event(event())
        _, records = verify_authorizations(
            event(),
            [label_event("scope:windows", event_id=1001)],
            {"maintainer": "maintain"},
        )
        first = authorization_fingerprint(first_proposal, records)
        changed_head = proposal_from_event(event(head="3" * 40))
        changed_base = proposal_from_event(event(base="4" * 40, base_ref="dev"))
        changed_record = [dict(records[0], event_id=1002)]
        self.assertNotEqual(first, authorization_fingerprint(changed_head, records))
        self.assertNotEqual(first, authorization_fingerprint(changed_base, records))
        self.assertNotEqual(first, authorization_fingerprint(first_proposal, changed_record))

    def test_incomplete_proposal_identity_fails_closed(self):
        with self.assertRaisesRegex(AdapterError, "full commit SHA"):
            proposal_from_event(event(head="short"))

    def test_main_and_dev_targets_are_distinct_and_supported(self):
        main = proposal(base_ref="main")
        dev = proposal(base_ref="dev", base="3" * 40)
        self.assertEqual(main["base_ref"], "main")
        self.assertEqual(dev["base_ref"], "dev")
        self.assertNotEqual(main["base_sha"], dev["base_sha"])
        with self.assertRaisesRegex(AdapterError, "unsupported branch"):
            proposal(base_ref="release")

    def test_workflow_run_event_binds_run_attempt_and_pr_revision(self):
        workflow_event = {
            "workflow_run": {
                "id": 500,
                "run_attempt": 2,
                "pull_requests": [
                    {
                        "number": 17,
                        "base": {"sha": BASE, "ref": "dev"},
                        "head": {"sha": HEAD, "ref": "feature"},
                    }
                ],
            }
        }
        identity = event_identity(workflow_event)
        self.assertEqual(identity["event_kind"], "workflow_run")
        self.assertEqual(identity["validation_run_id"], 500)
        self.assertEqual(identity["validation_run_attempt"], 2)
        self.assertEqual(identity["base_ref"], "dev")

    def test_each_missing_trusted_file_is_a_bootstrap_failure(self):
        self.assertIn(BOUNDARY_WORKFLOW_PATH, REQUIRED_TRUSTED_FILES)
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

    def test_boundary_workflow_is_enforcement_bootstrap_only(self):
        result = {
            "allowed": True,
            "errors": [],
            "classified_paths": {BOUNDARY_WORKFLOW_PATH: "shared-contract"},
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

    def test_canonical_run_prefers_new_run_over_late_old_rerun(self):
        old_rerun = validation_run(run_id=200, run_number=10, run_attempt=4)
        current = validation_run(run_id=201, run_number=11, run_attempt=1)
        selected = select_canonical_validation_run(
            [current, old_rerun], proposal(), REPOSITORY, REPOSITORY_ID, 300
        )
        self.assertEqual(selected["id"], 201)

    def test_canonical_run_uses_latest_attempt_for_current_run(self):
        current = validation_run(run_id=201, run_number=11, run_attempt=3)
        selected = select_canonical_validation_run(
            [current], proposal(), REPOSITORY, REPOSITORY_ID, 300
        )
        self.assertEqual(selected["run_attempt"], 3)

    def test_wrong_workflow_repository_or_revision_is_not_evidence(self):
        wrong = [
            validation_run(workflow_id=999),
            validation_run(repository="attacker/repo"),
            validation_run(head="9" * 40),
            validation_run(path=".github/workflows/spoof.yml"),
            validation_run(name="Spoofed validation"),
        ]
        selected = select_canonical_validation_run(
            wrong, proposal(), REPOSITORY, REPOSITORY_ID, 300
        )
        self.assertIsNone(selected)


if __name__ == "__main__":
    unittest.main()
