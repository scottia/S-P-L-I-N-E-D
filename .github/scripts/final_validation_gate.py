"""Fail closed unless trusted identity and every selected CI lane succeeded."""

from __future__ import annotations

import argparse
import json
import sys
from typing import Mapping, Sequence

from validation_scope import LANES


KNOWN_RESULTS = {"success", "failure", "cancelled", "skipped"}
SELECTOR_JOB_NAME = "Validation / scope selection"
SIGNED_IDENTITY_JOB_NAME = "Validation / signed workflow identity"
LANE_JOB_NAMES = {
    "infrastructure": "Validation / safeguard infrastructure",
    "root_native": "Validation / root-native Rust",
    "windows": "Validation / Windows native",
    "ubuntu_webgui": "Validation / Ubuntu WebGUI",
    "release_contract": "Validation / release contracts",
    "documentation": "Validation / documentation contract",
}
EXPECTED_JOB_NAMES = {
    SELECTOR_JOB_NAME,
    SIGNED_IDENTITY_JOB_NAME,
    *LANE_JOB_NAMES.values(),
}
IDENTITY_FIELDS = (
    "repository",
    "repository_id",
    "pull_request_number",
    "base_sha",
    "head_sha",
    "merge_sha",
    "workflow_id",
    "workflow_path",
    "workflow_name",
    "workflow_ref",
    "workflow_sha",
    "trigger_sha",
    "executed_workflow_blob",
)


def evaluate_final_gate(
    boundary: Mapping[str, object],
    selection: Mapping[str, object],
    results: Mapping[str, object],
    *,
    boundary_job_result: str,
    expected_base: str,
    expected_head: str,
) -> list[str]:
    errors: list[str] = []
    if boundary_job_result != "success":
        errors.append(f"Boundary authorization job result is {boundary_job_result or 'missing'}")
    if boundary.get("allowed") is not True:
        errors.append("Boundary authorization did not allow the proposal")
    if boundary.get("requires_bootstrap") is True:
        errors.append("Validation enforcement changed and requires trusted-branch bootstrap")

    proposal = boundary.get("proposal")
    if not isinstance(proposal, Mapping):
        errors.append("Boundary proposal identity is missing")
    else:
        if proposal.get("base_sha") != expected_base:
            errors.append("Boundary result is stale for the target/base commit")
        if proposal.get("head_sha") != expected_head:
            errors.append("Boundary result is stale for the proposed head commit")

    if not boundary.get("authorization_fingerprint"):
        errors.append("Verified authorization fingerprint is missing")

    for lane in LANES:
        if lane not in selection:
            errors.append(f"Selection is missing lane: {lane}")
            continue
        selected = selection[lane]
        if not isinstance(selected, bool):
            errors.append(f"Selection for {lane} is not boolean")
            continue
        result = results.get(lane)
        if result not in KNOWN_RESULTS:
            errors.append(f"Validation result for {lane} is missing or unknown: {result!r}")
            continue
        if selected and result != "success":
            errors.append(f"Selected validation {lane} did not succeed: {result}")
        if not selected and result != "skipped":
            errors.append(f"Unselected validation {lane} unexpectedly ran or reported {result}")
    return errors


def evidence_identity_errors(
    evidence: Mapping[str, object], expected: Mapping[str, object]
) -> list[str]:
    errors: list[str] = []
    for field in IDENTITY_FIELDS:
        if evidence.get(field) != expected.get(field):
            errors.append(
                f"Validation evidence {field} mismatch: "
                f"{evidence.get(field)!r} != {expected.get(field)!r}"
            )
    for field in ("run_id", "run_number", "run_attempt"):
        if not isinstance(evidence.get(field), int) or int(evidence[field]) <= 0:
            errors.append(f"Validation evidence has invalid {field}: {evidence.get(field)!r}")
    for field in ("identity_check_run_id", "artifact_id"):
        if not isinstance(evidence.get(field), int) or int(evidence[field]) <= 0:
            errors.append(f"Validation evidence has invalid {field}: {evidence.get(field)!r}")
    if not isinstance(evidence.get("identity_jti"), str) or not evidence["identity_jti"]:
        errors.append("Validation evidence has no signed identity token identifier")
    if not isinstance(evidence.get("artifact_digest"), str) or not str(
        evidence["artifact_digest"]
    ).startswith("sha256:"):
        errors.append("Validation evidence has no signed identity artifact digest")
    return errors


def job_results_from_evidence(
    evidence: Mapping[str, object],
) -> tuple[dict[str, str], list[str]]:
    errors: list[str] = []
    jobs = evidence.get("jobs")
    if not isinstance(jobs, Sequence) or isinstance(jobs, (str, bytes)):
        return {}, ["Validation evidence jobs are missing or malformed"]

    by_name: dict[str, list[Mapping[str, object]]] = {}
    for item in jobs:
        if not isinstance(item, Mapping):
            errors.append("Validation evidence contains a malformed job")
            continue
        by_name.setdefault(str(item.get("name", "")), []).append(item)

    duplicate = sorted(name for name, items in by_name.items() if len(items) != 1)
    if duplicate:
        errors.append("Duplicate validation job names: " + ", ".join(duplicate))
    missing = sorted(EXPECTED_JOB_NAMES - set(by_name))
    if missing:
        errors.append("Missing validation jobs: " + ", ".join(missing))
    unexpected = sorted(set(by_name) - EXPECTED_JOB_NAMES)
    if unexpected:
        errors.append("Unexpected validation jobs: " + ", ".join(unexpected))

    run_attempt = evidence.get("run_attempt")
    for name, items in by_name.items():
        if len(items) != 1:
            continue
        item = items[0]
        if item.get("run_attempt") != run_attempt:
            errors.append(f"Validation job {name} belongs to a different run attempt")
        if item.get("status") != "completed":
            errors.append(f"Validation job {name} is not completed: {item.get('status')!r}")

    selector = by_name.get(SELECTOR_JOB_NAME, [])
    if len(selector) == 1 and selector[0].get("conclusion") != "success":
        errors.append(
            "Validation scope selection did not succeed: "
            f"{selector[0].get('conclusion')!r}"
        )
    identity = by_name.get(SIGNED_IDENTITY_JOB_NAME, [])
    if len(identity) == 1 and identity[0].get("conclusion") != "success":
        errors.append(
            "Signed workflow identity job did not succeed: "
            f"{identity[0].get('conclusion')!r}"
        )

    results: dict[str, str] = {}
    for lane, name in LANE_JOB_NAMES.items():
        items = by_name.get(name, [])
        if len(items) == 1:
            results[lane] = str(items[0].get("conclusion", ""))
    return results, errors


def evaluate_trusted_gate(
    boundary: Mapping[str, object],
    selection: Mapping[str, object],
    evidence: Mapping[str, object],
    expected_identity: Mapping[str, object],
) -> tuple[str, list[str]]:
    """Return a commit-status state and errors for current, API-verified evidence."""

    proposal = boundary.get("proposal")
    base = str(proposal.get("base_sha", "")) if isinstance(proposal, Mapping) else ""
    head = str(proposal.get("head_sha", "")) if isinstance(proposal, Mapping) else ""
    preliminary = evaluate_final_gate(
        boundary,
        selection,
        dict.fromkeys(LANES, "skipped"),
        boundary_job_result="success",
        expected_base=str(expected_identity.get("base_sha", "")),
        expected_head=str(expected_identity.get("head_sha", "")),
    )
    # Lane conclusions are evaluated only after a current workflow run completes.
    preliminary = [
        error
        for error in preliminary
        if not error.startswith(("Selected validation ", "Unselected validation "))
    ]
    if preliminary:
        return "failure", preliminary

    availability = evidence.get("availability")
    if availability == "missing":
        return "pending", ["No validation workflow run exists for the current PR revision"]
    if availability != "present":
        return "error", [f"Validation evidence availability is invalid: {availability!r}"]

    status = evidence.get("status")
    if status in {"queued", "in_progress", "requested", "waiting", "pending"}:
        return "pending", [
            f"Validation run {evidence.get('run_id')} attempt "
            f"{evidence.get('run_attempt')} is {status}"
        ]
    if status != "completed":
        return "failure", [f"Validation run has unexpected status: {status!r}"]

    identity_errors = evidence_identity_errors(evidence, expected_identity)
    if identity_errors:
        return "failure", identity_errors

    results, job_errors = job_results_from_evidence(evidence)
    errors = list(job_errors)
    if evidence.get("conclusion") != "success":
        errors.append(
            "Validation workflow did not succeed: "
            f"{evidence.get('conclusion')!r}"
        )
    errors.extend(
        evaluate_final_gate(
            boundary,
            selection,
            results,
            boundary_job_result="success",
            expected_base=base,
            expected_head=head,
        )
    )
    return ("success", []) if not errors else ("failure", errors)


def parse_object(value: str, label: str) -> Mapping[str, object]:
    parsed = json.loads(value)
    if not isinstance(parsed, Mapping):
        raise ValueError(f"{label} must be a JSON object")
    return parsed


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--boundary-json", required=True)
    parser.add_argument("--selection-json", required=True)
    parser.add_argument("--results-json", required=True)
    parser.add_argument("--boundary-job-result", required=True)
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    args = parser.parse_args(argv)
    try:
        errors = evaluate_final_gate(
            parse_object(args.boundary_json, "boundary"),
            parse_object(args.selection_json, "selection"),
            parse_object(args.results_json, "results"),
            boundary_job_result=args.boundary_job_result,
            expected_base=args.base,
            expected_head=args.head,
        )
    except (ValueError, json.JSONDecodeError) as error:
        print(f"final validation gate failed closed: {error}", file=sys.stderr)
        return 2
    if errors:
        for error in errors:
            print(f"final validation gate: {error}", file=sys.stderr)
        return 2
    print("final validation gate: all selected validations succeeded")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
