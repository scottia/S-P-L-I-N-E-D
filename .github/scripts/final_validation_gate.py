"""Fail closed unless every selected CI validation lane succeeded."""

from __future__ import annotations

import argparse
import json
import sys
from typing import Mapping

from validation_scope import LANES


KNOWN_RESULTS = {"success", "failure", "cancelled", "skipped"}


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
