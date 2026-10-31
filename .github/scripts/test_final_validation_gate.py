from __future__ import annotations

import copy
import unittest

from final_validation_gate import (
    LANE_JOB_NAMES,
    SELECTOR_JOB_NAME,
    evaluate_final_gate,
    evaluate_trusted_gate,
)
from validation_scope import LANES


BASE = "1" * 40
HEAD = "2" * 40
BLOB = "3" * 40


def boundary(*, allowed=True, bootstrap=False, base=BASE, head=HEAD):
    return {
        "allowed": allowed,
        "requires_bootstrap": bootstrap,
        "authorization_fingerprint": "verified",
        "proposal": {"base_sha": base, "head_sha": head},
    }


def selection(*selected):
    return {lane: lane in selected for lane in LANES}


def results(selected):
    return {lane: "success" if selected.get(lane, False) else "skipped" for lane in LANES}


def expected_identity():
    return {
        "repository": "owner/splined",
        "repository_id": 91,
        "pull_request_number": 17,
        "base_sha": BASE,
        "head_sha": HEAD,
        "workflow_id": 300,
        "workflow_path": ".github/workflows/validate.yml",
        "workflow_name": "Validate supported runtimes",
        "target_workflow_blob": BLOB,
    }


def evidence(selected):
    jobs = [
        {
            "id": 1,
            "name": SELECTOR_JOB_NAME,
            "status": "completed",
            "conclusion": "success",
            "run_attempt": 2,
        }
    ]
    for index, (lane, name) in enumerate(LANE_JOB_NAMES.items(), start=2):
        jobs.append(
            {
                "id": index,
                "name": name,
                "status": "completed",
                "conclusion": "success" if selected[lane] else "skipped",
                "run_attempt": 2,
            }
        )
    return {
        "availability": "present",
        **expected_identity(),
        "run_id": 500,
        "run_number": 12,
        "run_attempt": 2,
        "status": "completed",
        "conclusion": "success",
        "check_suite_id": 700,
        "jobs": jobs,
    }


def errors_for(selected, job_results=None, **kwargs):
    return evaluate_final_gate(
        boundary(**kwargs),
        selected,
        job_results or results(selected),
        boundary_job_result="success",
        expected_base=BASE,
        expected_head=HEAD,
    )


class FinalValidationGateTests(unittest.TestCase):
    def test_selected_success_and_unselected_skip_pass(self):
        selected = selection("windows", "documentation")
        self.assertEqual(errors_for(selected), [])
        state, errors = evaluate_trusted_gate(
            boundary(), selected, evidence(selected), expected_identity()
        )
        self.assertEqual((state, errors), ("success", []))

    def test_selected_failure_cancellation_skip_or_missing_blocks(self):
        for bad in ("failure", "cancelled", "skipped", None, "neutral"):
            with self.subTest(result=bad):
                selected = selection("windows")
                job_results = results(selected)
                if bad is None:
                    job_results.pop("windows")
                else:
                    job_results["windows"] = bad
                self.assertTrue(errors_for(selected, job_results))

    def test_trusted_gate_blocks_failed_cancelled_missing_or_unexpectedly_skipped_lane(self):
        for conclusion in ("failure", "cancelled", "skipped", None):
            with self.subTest(conclusion=conclusion):
                selected = selection("windows")
                value = evidence(selected)
                jobs = value["jobs"]
                windows = next(job for job in jobs if job["name"] == LANE_JOB_NAMES["windows"])
                if conclusion is None:
                    jobs.remove(windows)
                else:
                    windows["conclusion"] = conclusion
                value["conclusion"] = "failure"
                state, errors = evaluate_trusted_gate(
                    boundary(), selected, value, expected_identity()
                )
                self.assertEqual(state, "failure")
                self.assertTrue(errors)

    def test_unselected_lane_must_be_skipped_after_selection(self):
        selected = selection("windows")
        job_results = results(selected)
        job_results["ubuntu_webgui"] = "success"
        self.assertTrue(any("unexpectedly ran" in error for error in errors_for(selected, job_results)))

        value = evidence(selected)
        webgui = next(
            job for job in value["jobs"] if job["name"] == LANE_JOB_NAMES["ubuntu_webgui"]
        )
        webgui["conclusion"] = "success"
        state, errors = evaluate_trusted_gate(
            boundary(), selected, value, expected_identity()
        )
        self.assertEqual(state, "failure")
        self.assertTrue(any("unexpectedly ran" in error for error in errors))

    def test_boundary_failure_and_bootstrap_block_even_when_validation_passes(self):
        selected = selection("infrastructure")
        for value in (boundary(allowed=False), boundary(bootstrap=True)):
            state, errors = evaluate_trusted_gate(
                value, selected, evidence(selected), expected_identity()
            )
            self.assertEqual(state, "failure")
            self.assertTrue(errors)

    def test_updated_head_or_target_invalidates_stale_result(self):
        selected = selection("windows")
        stale_head = errors_for(selected, head="3" * 40)
        stale_base = errors_for(selected, base="4" * 40)
        self.assertTrue(any("proposed head" in error for error in stale_head))
        self.assertTrue(any("target/base" in error for error in stale_base))

    def test_wrong_repository_workflow_run_or_revision_blocks(self):
        selected = selection("windows")
        for field, wrong in (
            ("repository", "attacker/spoof"),
            ("repository_id", 999),
            ("pull_request_number", 18),
            ("base_sha", "8" * 40),
            ("head_sha", "9" * 40),
            ("workflow_id", 999),
            ("workflow_path", ".github/workflows/spoof.yml"),
            ("workflow_name", "Spoofed validation"),
            ("target_workflow_blob", "a" * 40),
        ):
            with self.subTest(field=field):
                value = evidence(selected)
                value[field] = wrong
                state, errors = evaluate_trusted_gate(
                    boundary(), selected, value, expected_identity()
                )
                self.assertEqual(state, "failure")
                self.assertTrue(any(field in error for error in errors))

        value = evidence(selected)
        value["run_id"] = 0
        state, errors = evaluate_trusted_gate(
            boundary(), selected, value, expected_identity()
        )
        self.assertEqual(state, "failure")
        self.assertTrue(any("run_id" in error for error in errors))

    def test_duplicate_or_unexpected_job_name_blocks_spoofed_evidence(self):
        selected = selection("windows")
        duplicate = evidence(selected)
        duplicate["jobs"].append(copy.deepcopy(duplicate["jobs"][1]))
        state, errors = evaluate_trusted_gate(
            boundary(), selected, duplicate, expected_identity()
        )
        self.assertEqual(state, "failure")
        self.assertTrue(any("Duplicate" in error for error in errors))

        unexpected = evidence(selected)
        unexpected["jobs"].append(
            {
                "id": 99,
                "name": "Validation / spoof",
                "status": "completed",
                "conclusion": "success",
                "run_attempt": 2,
            }
        )
        state, errors = evaluate_trusted_gate(
            boundary(), selected, unexpected, expected_identity()
        )
        self.assertEqual(state, "failure")
        self.assertTrue(any("Unexpected" in error for error in errors))

    def test_pending_or_missing_current_run_never_passes(self):
        selected = selection("windows")
        pending = evidence(selected)
        pending["status"] = "in_progress"
        pending["conclusion"] = None
        pending["jobs"] = []
        state, _ = evaluate_trusted_gate(
            boundary(), selected, pending, expected_identity()
        )
        self.assertEqual(state, "pending")

        state, _ = evaluate_trusted_gate(
            boundary(), selected, {"availability": "missing"}, expected_identity()
        )
        self.assertEqual(state, "pending")

    def test_job_from_wrong_rerun_attempt_blocks(self):
        selected = selection("windows")
        value = evidence(selected)
        value["jobs"][1]["run_attempt"] = 1
        state, errors = evaluate_trusted_gate(
            boundary(), selected, value, expected_identity()
        )
        self.assertEqual(state, "failure")
        self.assertTrue(any("different run attempt" in error for error in errors))

    def test_missing_lane_or_fingerprint_blocks(self):
        selected = selection("windows")
        selected.pop("documentation")
        self.assertTrue(errors_for(selected))
        value = boundary()
        value["authorization_fingerprint"] = ""
        state, errors = evaluate_trusted_gate(
            value,
            selection("windows"),
            evidence(selection("windows")),
            expected_identity(),
        )
        self.assertEqual(state, "failure")
        self.assertTrue(any("fingerprint" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
