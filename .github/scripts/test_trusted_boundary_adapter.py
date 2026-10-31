from __future__ import annotations

import base64
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

from trusted_boundary_adapter import (
    AdapterError,
    BOUNDARY_WORKFLOW_PATH,
    OIDC_AUDIENCE,
    OIDC_ISSUER,
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
    signed_identity_from_artifact,
    status_target,
    verify_signed_identity,
    verify_test_merge_commit,
    verify_unique_open_head,
    verify_authorizations,
)


BASE = "1" * 40
HEAD = "2" * 40
MERGE = "3" * 40
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


def current_pull_request(
    *,
    base=BASE,
    head=HEAD,
    merge=MERGE,
    base_ref="main",
    head_ref="feature",
    labels=("scope:windows",),
):
    return {
        "state": "open",
        "mergeable": True,
        "merge_commit_sha": merge,
        "labels": [{"name": label} for label in labels],
        "base": {
            "sha": base,
            "ref": base_ref,
            "repo": {"full_name": REPOSITORY, "id": REPOSITORY_ID},
        },
        "head": {
            "sha": head,
            "ref": head_ref,
            "repo": {"full_name": HEAD_REPOSITORY, "id": HEAD_REPOSITORY_ID},
        },
    }


def proposal(*, base=BASE, head=HEAD, merge=MERGE, base_ref="main"):
    return proposal_from_pull_request(
        17,
        current_pull_request(base=base, head=head, merge=merge, base_ref=base_ref),
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
        "head_sha": head,
        "head_branch": "feature",
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


TEST_RSA_N = "1MPvzXiuFIm7yXIV8YO0aG2JEdaPjXjVjvAIPzu9cTiTSTgek0WLQfPq6YFAHOmftuNS6MESlm4UaDTTUDRaepEZF9yYAqoz06-WnpOvAbB4d46VpdxNBsjcWtQTMVaA_Cd3xS_-Wmlf-08mCyVHVy6BRIEuAUoIo64rh9zUJFjcz6cx2xxAaSc404JxBApm7c4apJvODrt78o5ikO4eiZ_YgIabtG9Qbp8GqLW0xLUCzSI6M3IOXwpwv-OfIYRZEw0kz9KZy9YlhOCxXuzSuRJJlZxvjJ13-5Onk4aabSbHM1KhKLk-TrGh8CBv2asIvTtwNGbD9dfE1b5VKz3OtQ"
TEST_RSA_E = "AQAB"
TEST_RSA_D = "XMqPPQdStHZVRcfRtiEAf0K2MudmeAao7MvWbO1683P_GEG3Y_dCgqMEYo4SDgn9s990ezopBx4_HCtdtW7_vtClJvj3-sGMFNhRUhuKiawS_eFbdpibp0fWgm5XaH8XheEYpREHVxr51zpvlxDNCkKn8d581-cX2XrOVVSD2M102ymvkYFVB9_TPV7THT9m4X33tt-NyyVusbV1_6HxMKCBMbEZzeBf0Fzz5v560HT4gGzDcORy9kTBiuAox6w1J3JAFfm32X6ZgZTndOWpozf1NvUaZ7YSKseKthimROnSAPtiTrTXbh6s_mnW3vEBBvbg4VWnxbHv9136CDHoCQ"
TEST_KID = "fixture-key"
TEST_NOW = 1_800_000_000


def b64url(value: bytes) -> str:
    return base64.urlsafe_b64encode(value).rstrip(b"=").decode("ascii")


def signed_identity(overrides=None):
    claims = {
        "iss": OIDC_ISSUER,
        "aud": OIDC_AUDIENCE,
        "iat": TEST_NOW - 5,
        "nbf": TEST_NOW - 5,
        "exp": TEST_NOW + 300,
        "jti": "fixture-token",
        "repository": REPOSITORY,
        "repository_id": str(REPOSITORY_ID),
        "run_id": "200",
        "run_number": "10",
        "run_attempt": "2",
        "workflow": "Validate supported runtimes",
        "workflow_ref": f"{REPOSITORY}/.github/workflows/validate.yml@refs/pull/17/merge",
        "workflow_sha": MERGE,
        "ref": "refs/pull/17/merge",
        "sha": MERGE,
        "event_name": "pull_request",
        "base_ref": "main",
        "head_ref": "feature",
        "check_run_id": "800",
        "runner_environment": "github-hosted",
    }
    claims.update(overrides or {})
    header = {"typ": "JWT", "alg": "RS256", "kid": TEST_KID}
    encoded_header = b64url(json.dumps(header, separators=(",", ":")).encode())
    encoded_claims = b64url(json.dumps(claims, separators=(",", ":")).encode())
    signing_input = f"{encoded_header}.{encoded_claims}".encode("ascii")
    modulus = int.from_bytes(base64.urlsafe_b64decode(TEST_RSA_N + "=="), "big")
    private_exponent = int.from_bytes(base64.urlsafe_b64decode(TEST_RSA_D + "=="), "big")
    size = (modulus.bit_length() + 7) // 8
    digest_info = bytes.fromhex("3031300d060960864801650304020105000420") + hashlib.sha256(
        signing_input
    ).digest()
    encoded = b"\x00\x01" + b"\xff" * (size - len(digest_info) - 3) + b"\x00" + digest_info
    signature = pow(int.from_bytes(encoded, "big"), private_exponent, modulus).to_bytes(
        size, "big"
    )
    return f"{encoded_header}.{encoded_claims}.{b64url(signature)}"


def signed_identity_expected():
    return {
        "repository": REPOSITORY,
        "repository_id": REPOSITORY_ID,
        "run_id": 200,
        "run_number": 10,
        "run_attempt": 2,
        "workflow_ref": f"{REPOSITORY}/.github/workflows/validate.yml@refs/pull/17/merge",
        "merge_sha": MERGE,
        "pull_request_ref": "refs/pull/17/merge",
        "base_ref": "main",
        "head_ref": "feature",
        "identity_job_id": 800,
    }


def test_jwks():
    return {
        "keys": [
            {
                "kid": TEST_KID,
                "kty": "RSA",
                "alg": "RS256",
                "use": "sig",
                "n": TEST_RSA_N,
                "e": TEST_RSA_E,
            }
        ]
    }


def identity_archive(token=None):
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
        bundle.writestr("splined-validation-identity.jwt", token or signed_identity())
    return buffer.getvalue()


class ArtifactClient:
    def __init__(self, archive):
        self.archive = archive

    def download_artifact(self, artifact_id):
        if artifact_id != 900:
            raise AssertionError("unexpected artifact")
        return self.archive


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

    def test_names_and_expected_values_alone_do_not_attest_executed_workflow(self):
        legacy_candidate = validation_run(run_attempt=2)
        selected = select_canonical_validation_run(
            [legacy_candidate], proposal(), REPOSITORY, REPOSITORY_ID, 300
        )
        self.assertEqual(selected["id"], 200)
        self.assertNotIn("workflow_sha", selected)
        with self.assertRaisesRegex(AdapterError, "JWT"):
            verify_signed_identity(
                json.dumps(signed_identity_expected()),
                test_jwks(),
                signed_identity_expected(),
                now=TEST_NOW,
            )

    def test_github_signed_identity_binds_workflow_revision_and_run_attempt(self):
        claims = verify_signed_identity(
            signed_identity(), test_jwks(), signed_identity_expected(), now=TEST_NOW
        )
        self.assertEqual(claims["workflow_sha"], MERGE)
        self.assertEqual(claims["sha"], MERGE)
        for field, wrong in (
            ("repository", "attacker/repo"),
            ("repository_id", "999"),
            ("run_id", "201"),
            ("run_attempt", "1"),
            ("workflow_ref", f"{REPOSITORY}/.github/workflows/spoof.yml@refs/pull/17/merge"),
            ("workflow_sha", "9" * 40),
            ("sha", "8" * 40),
            ("ref", "refs/pull/18/merge"),
            ("base_ref", "dev"),
            ("check_run_id", "801"),
        ):
            with self.subTest(field=field), self.assertRaisesRegex(
                AdapterError, field
            ):
                verify_signed_identity(
                    signed_identity({field: wrong}),
                    test_jwks(),
                    signed_identity_expected(),
                    now=TEST_NOW,
                )

    def test_tampered_or_expired_signed_identity_fails_closed(self):
        token = signed_identity()
        replacement = "A" if token[-1] != "A" else "B"
        with self.assertRaisesRegex(AdapterError, "signature"):
            verify_signed_identity(
                token[:-1] + replacement,
                test_jwks(),
                signed_identity_expected(),
                now=TEST_NOW,
            )

    def test_identity_artifact_is_unique_run_bound_and_digest_verified(self):
        archive = identity_archive()
        artifact = {
            "id": 900,
            "name": "splined-validation-identity-2",
            "expired": False,
            "size_in_bytes": len(archive),
            "digest": "sha256:" + hashlib.sha256(archive).hexdigest(),
            "workflow_run": {
                "id": 200,
                "repository_id": REPOSITORY_ID,
                "head_repository_id": HEAD_REPOSITORY_ID,
                "head_sha": HEAD,
            },
        }
        token, evidence = signed_identity_from_artifact(
            ArtifactClient(archive),
            [artifact],
            proposal(),
            REPOSITORY_ID,
            200,
            2,
        )
        self.assertEqual(token, signed_identity())
        self.assertEqual(evidence["artifact_id"], 900)

        with self.assertRaisesRegex(AdapterError, "ambiguous"):
            signed_identity_from_artifact(
                ArtifactClient(archive),
                [artifact, dict(artifact)],
                proposal(),
                REPOSITORY_ID,
                200,
                2,
            )
        with self.assertRaisesRegex(AdapterError, "digest"):
            signed_identity_from_artifact(
                ArtifactClient(archive),
                [{**artifact, "digest": "sha256:" + "0" * 64}],
                proposal(),
                REPOSITORY_ID,
                200,
                2,
            )
        wrong_run = dict(artifact)
        wrong_run["workflow_run"] = {**artifact["workflow_run"], "id": 201}
        with self.assertRaisesRegex(AdapterError, "selected run"):
            signed_identity_from_artifact(
                ArtifactClient(archive),
                [wrong_run],
                proposal(),
                REPOSITORY_ID,
                200,
                2,
            )
        with self.assertRaisesRegex(AdapterError, "expired"):
            verify_signed_identity(
                signed_identity({"iat": TEST_NOW - 700, "nbf": TEST_NOW - 700, "exp": TEST_NOW - 100}),
                test_jwks(),
                signed_identity_expected(),
                now=TEST_NOW,
            )

    def test_test_merge_commit_must_bind_current_base_and_head(self):
        commit = {
            "sha": MERGE,
            "parents": [{"sha": BASE}, {"sha": HEAD}],
        }
        verify_test_merge_commit(commit, proposal())
        with self.assertRaisesRegex(AdapterError, "parents"):
            verify_test_merge_commit(
                {**commit, "parents": [{"sha": BASE}, {"sha": "9" * 40}]},
                proposal(),
            )

    def test_shared_head_prs_are_rejected_and_use_distinct_merge_statuses(self):
        first = proposal(merge=MERGE)
        second = proposal_from_pull_request(
            18,
            current_pull_request(
                base="4" * 40,
                head=HEAD,
                merge="5" * 40,
                base_ref="dev",
                head_ref="feature-copy",
                labels=("scope:ubuntu-webgui",),
            ),
            REPOSITORY,
            REPOSITORY_ID,
        )
        self.assertEqual(first["head_sha"], second["head_sha"])
        self.assertNotEqual(first["merge_sha"], second["merge_sha"])
        self.assertNotEqual(status_target(first), status_target(second))
        open_prs = [
            {"number": 17, "state": "open", "head": {"sha": HEAD}},
            {"number": 18, "state": "open", "head": {"sha": HEAD}},
        ]
        with self.assertRaisesRegex(AdapterError, "#17, #18"):
            verify_unique_open_head(open_prs, first)
        with self.assertRaisesRegex(AdapterError, "#17, #18"):
            verify_unique_open_head(open_prs, second)

    def test_ambiguous_pr_association_is_not_validation_evidence(self):
        ambiguous = validation_run()
        ambiguous["pull_requests"].append(
            {
                "number": 18,
                "base": {"sha": BASE, "ref": "main"},
                "head": {"sha": HEAD, "ref": "feature-copy"},
            }
        )
        selected = select_canonical_validation_run(
            [ambiguous], proposal(), REPOSITORY, REPOSITORY_ID, 300
        )
        self.assertIsNone(selected)


if __name__ == "__main__":
    unittest.main()
