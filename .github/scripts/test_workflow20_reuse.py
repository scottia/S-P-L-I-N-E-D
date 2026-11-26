"""Test exact evaluation identity, retained artifact integrity and CI provenance."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from workflow20_reuse import (
    IMAGE, SOURCE, VALIDATE, WEB_JOBS, WEB_MANIFEST, WEB_SUMS, WEB_VALIDATIONS,
    WF20, WIN_MANIFEST, WIN_SUMS, artifact_run_candidates, eligible_run,
    exact_stamp, matching_pr_run, verify_bundle, web_jobs_passed, workflow_contract,
)

ENV = {
    "GITHUB_REPOSITORY": "owner/repo", "GITHUB_RUN_ID": "300",
    "GITHUB_SHA": "a" * 40, "SOURCE_COMMIT": "b" * 40,
    "SOURCE_VERSION": "3.0.34", "SOURCE_MODE": "pr-evaluation",
    "EVALUATION_PR": "356", "WINDOWS_TARGET": "x86_64-pc-windows-msvc",
    "RUST_TOOLCHAIN": "1.99.0",
}


class ReuseTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def run_metadata(self):
        return {
            "id": 200, "run_attempt": 1, "status": "completed",
            "conclusion": "success", "event": "workflow_dispatch", "path": WF20,
            "repository": {"full_name": ENV["GITHUB_REPOSITORY"]},
            "head_repository": {"full_name": ENV["GITHUB_REPOSITORY"]},
            "head_sha": ENV["GITHUB_SHA"], "display_title": "WF20 PR 356 v3.0.34",
        }

    def common_manifest(self):
        return {
            "source_commit": ENV["SOURCE_COMMIT"], "source_mode": ENV["SOURCE_MODE"],
            "workflow_definition_commit": ENV["GITHUB_SHA"],
            "workflow_run_id": 200, "release_eligible": False,
        }

    def windows(self):
        exe = b"test executable bytes"
        (self.root / "splined.exe").write_bytes(exe)
        digest = hashlib.sha256(exe).hexdigest()
        m = dict(self.common_manifest(), schema="splined-quick-portable-v1",
                 source_version=ENV["SOURCE_VERSION"], evaluation_pr="356",
                 architecture=ENV["WINDOWS_TARGET"], rust_toolchain=ENV["RUST_TOOLCHAIN"],
                 build_profile="release-optimized-incremental-v1",
                 sha256=digest, size_bytes=len(exe))
        (self.root / WIN_MANIFEST).write_text(json.dumps(m), encoding="utf-8")
        (self.root / WIN_SUMS).write_text(f"{digest}  splined.exe\n", encoding="ascii")
        return m

    def test_windows_requires_exact_build_identity_and_checksum(self):
        m = self.windows()
        self.assertEqual(verify_bundle(self.root, "windows", ENV, 200), m)
        for field, value in (
            ("source_commit", "c" * 40), ("source_version", "3.0.33"),
            ("workflow_definition_commit", "c" * 40), ("workflow_run_id", 201),
            ("release_eligible", True), ("build_profile", "debug"),
            ("rust_toolchain", "1.98.0"), ("evaluation_pr", "355"),
        ):
            with self.subTest(field=field):
                (self.root / WIN_MANIFEST).write_text(json.dumps(dict(m, **{field: value})))
                with self.assertRaises(ValueError):
                    verify_bundle(self.root, "windows", ENV, 200)
        (self.root / WIN_MANIFEST).write_text(json.dumps(m))
        (self.root / "splined.exe").write_bytes(b"changed")
        with self.assertRaises(ValueError):
            verify_bundle(self.root, "windows", ENV, 200)

    def test_webgui_requires_exact_package_and_no_candidate_bundle(self):
        files = {}
        for name in (SOURCE, IMAGE):
            p = self.root / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(name.encode())
            files[name] = hashlib.sha256(name.encode()).hexdigest()
        m = dict(self.common_manifest(), schema="splined-release-build-v1", kind="webgui",
                 repository=ENV["GITHUB_REPOSITORY"], release_version=ENV["SOURCE_VERSION"],
                 evaluation_pr=356, evaluation_only=True, candidate_kind="workflow20-evaluation",
                 candidate_run_id=200, candidate_commit="c" * 40, validations=WEB_VALIDATIONS,
                 source_validation={"kind": "executed-in-evaluation", "run_id": 200}, files=files)
        (self.root / WEB_MANIFEST).write_text(json.dumps(m))
        (self.root / WEB_SUMS).write_text("".join(f"{v}  {k}\n" for k, v in sorted(files.items())))
        self.assertEqual(verify_bundle(self.root, "webgui", ENV, 200), m)
        (self.root / "candidate").mkdir()
        with self.assertRaises(ValueError):
            verify_bundle(self.root, "webgui", ENV, 200)

    def test_only_successful_same_definition_evaluation_runs_are_eligible(self):
        run = self.run_metadata()
        self.assertEqual(artifact_run_candidates([run], ENV), [run])
        for change in (
            {"conclusion": "failure"}, {"status": "in_progress"}, {"path": VALIDATE},
            {"head_sha": "c" * 40}, {"display_title": "WF20 PR 356 v3.0.33"},
            {"head_repository": {"full_name": "other/repo"}}, {"id": 300},
        ):
            self.assertEqual(artifact_run_candidates([dict(run, **change)], ENV), [])

    def test_pr_validation_and_all_required_steps(self):
        base = "d" * 40
        run = dict(self.run_metadata(), path=VALIDATE, event="pull_request",
                   head_sha=ENV["SOURCE_COMMIT"],
                   pull_requests=[{"number": 356, "head": {"sha": ENV["SOURCE_COMMIT"]},
                                   "base": {"sha": base, "ref": "dev"}}])
        self.assertTrue(matching_pr_run(run, ENV, base))
        self.assertTrue(eligible_run(run, ENV, VALIDATE, "pull_request"))
        self.assertFalse(matching_pr_run(run, ENV, "e" * 40))
        jobs = [
            {"name": name, "head_sha": ENV["SOURCE_COMMIT"], "status": "completed",
             "conclusion": "success",
             "steps": [{"name": step, "conclusion": "success"} for step in steps]}
            for name, steps in WEB_JOBS.items()
        ]
        self.assertTrue(web_jobs_passed(jobs, ENV["SOURCE_COMMIT"]))
        self.assertFalse(web_jobs_passed(jobs[1:], ENV["SOURCE_COMMIT"]))
        jobs[-1]["steps"][-1]["conclusion"] = "skipped"
        self.assertFalse(web_jobs_passed(jobs, ENV["SOURCE_COMMIT"]))

    def test_version_stamp_cannot_hide_webgui_code_changes(self):
        before = b'VERSION = "3.0.28"\nfunctionality = True\n'
        after = b'VERSION = "3.0.34"\nfunctionality = True\n'
        self.assertTrue(exact_stamp(before, after, "3.0.34"))
        self.assertFalse(exact_stamp(before, after.replace(b"True", b"False"), "3.0.34"))
        self.assertFalse(exact_stamp(before, before, "3.0.34"))

    def test_workflow_contract_preserves_relevant_validation(self):
        workflow = (Path(__file__).resolve().parents[1] / "workflows/validate.yml").read_bytes()
        original = workflow_contract(workflow)
        self.assertEqual(original, workflow_contract(workflow.replace(b"continue-on-error: true", b"continue-on-error: false")))
        self.assertNotEqual(original, workflow_contract(workflow.replace(b"python -m unittest discover -s build/WEBGui/webgui/tests -v", b"echo skipped")))


if __name__ == "__main__":
    unittest.main()
