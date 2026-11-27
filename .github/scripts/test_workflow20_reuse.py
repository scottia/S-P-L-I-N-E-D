"""Test exact evaluation identity, retained artifact integrity and CI provenance."""
import hashlib
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

from source_version import current_source_version, stamp_release_version

from workflow20_reuse import (
    IMAGE, SOURCE, VALIDATE, WEB_JOBS, WEB_MANIFEST, WEB_SUMS, WEB_VALIDATIONS,
    WF20, WIN_MANIFEST, WIN_SUMS, STAMP_FILES, artifact_run_candidates, eligible_run,
    matching_pr_run, verify_bundle, verify_runner_stamp, web_jobs_passed,
    workflow_contract, reuse_artifact, reuse_validation,
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

    def test_exact_six_file_stamp_includes_user_agent_and_rejects_code_changes(self):
        repo_root = Path(__file__).resolve().parents[2]
        original, stamped = {}, {}
        for relative in STAMP_FILES:
            data = (repo_root / relative).read_bytes()
            original[relative] = data
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        major, minor, patch_number = map(int, current_source_version(self.root).split("."))
        version = f"{major}.{minor}.{patch_number + 1}"
        stamp_release_version(self.root, version)
        for relative in STAMP_FILES:
            stamped[relative] = (self.root / relative).read_bytes()
        source, candidate = ENV["SOURCE_COMMIT"], "c" * 40
        def show(commit, relative):
            return (original if commit == source else stamped)[relative]
        with patch("workflow20_reuse.git_show", side_effect=show), patch(
            "workflow20_reuse.subprocess.check_output",
            side_effect=lambda args, **kw: source + "\n" if args[1] == "rev-parse" else "\n".join(STAMP_FILES),
        ):
            self.assertTrue(verify_runner_stamp(source, candidate, version))
            # User-agent and VERSION both legitimately changed. Any other
            # byte, even in a Windows version file, invalidates evidence.
            for relative in STAMP_FILES:
                prior = stamped[relative]
                stamped[relative] += b"\nnot an authorized version change"
                self.assertFalse(verify_runner_stamp(source, candidate, version))
                stamped[relative] = prior

    def test_exact_retained_windows_artifact_download_and_stale_fallback(self):
        package = self.root
        self.windows()
        target = package / "new-evaluation"
        env = dict(ENV, RUNNER_TEMP=self.tmp.name)
        run = self.run_metadata()
        def download(args, **kw):
            destination = Path(args[args.index("--dir") + 1])
            destination.mkdir()
            for name in ("splined.exe", WIN_MANIFEST, WIN_SUMS):
                shutil.copyfile(package / name, destination / name)
        with patch("workflow20_reuse.api", side_effect=[
            {"workflow_runs": [run]},
            {"artifacts": [{"name": "splined-quick-portable-windows", "expired": False}]},
        ]), patch("workflow20_reuse.subprocess.run", side_effect=download):
            result = reuse_artifact("windows", target, env)
        self.assertEqual(result, {"reused": "true", "run_id": "200"})
        self.assertTrue((target / "splined.exe").is_file())
        shutil.rmtree(target)
        with patch("workflow20_reuse.api", side_effect=[
            {"workflow_runs": [run]},
            {"artifacts": [{"name": "splined-quick-portable-windows", "expired": False}]},
        ]), patch("workflow20_reuse.subprocess.run", side_effect=download):
            with self.assertRaises(ValueError):
                reuse_artifact("windows", target, dict(env, SOURCE_COMMIT="d" * 40))
        self.assertFalse(target.exists())

    def test_reuse_validation_rejects_newer_failure_and_missing_step(self):
        source, base = ENV["SOURCE_COMMIT"], "d" * 40
        env = dict(ENV, CANDIDATE_COMMIT="c" * 40)
        repo = {"full_name": ENV["GITHUB_REPOSITORY"]}
        pull = {"state": "open", "head": {"sha": source, "repo": repo},
                "base": {"ref": "dev", "sha": base, "repo": repo}}
        run = dict(self.run_metadata(), path=VALIDATE, event="pull_request", head_sha=source,
                   created_at="2026-01-01T00:00:00Z",
                   pull_requests=[{"number": 356, "head": {"sha": source},
                                   "base": {"sha": base, "ref": "dev"}}])
        jobs = [
            {"name": name, "head_sha": source, "status": "completed",
             "conclusion": "success",
             "steps": [{"name": step, "conclusion": "success"} for step in steps]}
            for name, steps in WEB_JOBS.items()
        ]
        def resolve(runs, steps):
            with patch("workflow20_reuse.api", side_effect=[
                pull, {"workflow_runs": runs}, {"total_count": len(steps), "jobs": steps},
            ]), patch("workflow20_reuse.git_show", return_value=b"trusted"), patch(
                "workflow20_reuse.workflow_contract", return_value=("trusted",)
            ), patch("workflow20_reuse.verify_runner_stamp", return_value=True):
                return reuse_validation(env)
        self.assertEqual(resolve([run], jobs)["run_id"], "200")
        self.assertEqual(resolve([run], jobs[1:]), {"reused": "false"})
        newer = dict(run, id=201, conclusion="failure", created_at="2026-01-02T00:00:00Z")
        self.assertEqual(resolve([newer, run], jobs), {"reused": "false"})

    def test_workflow_contract_preserves_relevant_validation(self):
        workflow = (Path(__file__).resolve().parents[1] / "workflows/validate.yml").read_bytes()
        original = workflow_contract(workflow)
        self.assertEqual(original, workflow_contract(workflow.replace(b"continue-on-error: true", b"continue-on-error: false")))
        self.assertNotEqual(original, workflow_contract(workflow.replace(b"Validate WebGUI without retired writer routines", b"Skip WebGUI validation")))

    def test_webgui_build_exposes_runtime_for_shared_gha_cache(self):
        workflow = (Path(__file__).resolve().parents[1] / "workflows/20-quick-portable-windows.yml").read_text(encoding="utf-8")
        self.assertIn(
            "actions/github-script@ed597411d8f924073f98dfc5c65a23a2325f34cd",
            workflow,
        )
        self.assertNotIn("crazy-max/ghaction-github-runtime", workflow)
        self.assertIn('core.exportVariable("ACTIONS_RUNTIME_TOKEN", token)', workflow)
        self.assertIn("--cache-to type=gha,mode=max,scope=splined-workflow20-webgui-${CACHE_SOURCE_SHA}", workflow)
        self.assertIn("--cache-from type=gha,scope=splined-workflow20-webgui-${CACHE_SOURCE_SHA}", workflow)
        self.assertIn("--cache-from type=gha,scope=splined-official-release-webgui", workflow)


if __name__ == "__main__":
    unittest.main()
