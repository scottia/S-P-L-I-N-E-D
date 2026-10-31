from __future__ import annotations

import unittest

from final_validation_gate import evaluate_final_gate
from validation_scope import LANES


BASE = "1" * 40
HEAD = "2" * 40


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

    def test_unselected_lane_must_be_skipped_after_selection(self):
        selected = selection("windows")
        job_results = results(selected)
        job_results["ubuntu_webgui"] = "success"
        errors = errors_for(selected, job_results)
        self.assertTrue(any("unexpectedly ran" in error for error in errors))

    def test_boundary_failure_blocks_even_when_lanes_succeed(self):
        selected = selection("root_native")
        errors = evaluate_final_gate(
            boundary(),
            selected,
            results(selected),
            boundary_job_result="failure",
            expected_base=BASE,
            expected_head=HEAD,
        )
        self.assertTrue(any("Boundary authorization job" in error for error in errors))

    def test_rejected_boundary_and_bootstrap_change_block(self):
        selected = selection("infrastructure")
        self.assertTrue(errors_for(selected, allowed=False))
        self.assertTrue(errors_for(selected, bootstrap=True))

    def test_updated_head_or_target_invalidates_stale_result(self):
        selected = selection("windows")
        stale_head = errors_for(selected, head="3" * 40)
        stale_base = errors_for(selected, base="4" * 40)
        self.assertTrue(any("proposed head" in error for error in stale_head))
        self.assertTrue(any("target/base" in error for error in stale_base))

    def test_missing_lane_or_fingerprint_blocks(self):
        selected = selection("windows")
        selected.pop("documentation")
        self.assertTrue(errors_for(selected))
        value = boundary()
        value["authorization_fingerprint"] = ""
        errors = evaluate_final_gate(
            value,
            selection("windows"),
            results(selection("windows")),
            boundary_job_result="success",
            expected_base=BASE,
            expected_head=HEAD,
        )
        self.assertTrue(any("fingerprint" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
