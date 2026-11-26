"""Read-only, exact-source acceleration for Workflow 20; never release authority."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

WF20 = ".github/workflows/20-quick-portable-windows.yml"
VALIDATE = ".github/workflows/validate.yml"
ARTIFACTS = {"windows": "splined-quick-portable-windows", "webgui": "splined-webgui-release"}
SOURCE = "release-assets/splined-webgui-python.tar.gz"
IMAGE = "release-assets/splined-webgui-container-image.tar.gz"
WEB_MANIFEST = "webgui-build-manifest.json"
WEB_SUMS = "SHA256SUMS-WEBGUI-BUILD.txt"
WIN_MANIFEST = "quick-portable-manifest.json"
WIN_SUMS = "SHA256SUMS.txt"
WEB_VALIDATIONS = [
    "python-postgresql-node", "container-health-database-smoke",
    "exact-smoke-tested-image-retained", "python-source-archive",
]
WEB_JOBS = {
    "Select relevant validation": (
        "Check validation selection rules", "Select checks for changed files",
    ),
    "Canonical policy JSON — review gate": (
        "Reject changed protected behavioural JSON",
        "Validate canonical policy, SQLite DDL and settings contracts",
    ),
    "Audio integrity — WebGUI real artwork writer": (
        "Enforce actual MP3 artwork-only integrity and mutation rejection",
    ),
    "webgui": (
        "Run python -m unittest discover -s build/WEBGui/webgui/tests -v",
        "Run node --check build/WEBGui/webgui/static/app.js && node --check build/WEBGui/webgui/static/operations.js",
        "Build and start browser container",
    ),
}
STAMP_FILES = (
    "build/Windows/Cargo.toml",
    "build/Windows/Cargo.lock",
    "build/Windows/gui/app.manifest",
    "build/Windows/package/Package.appxmanifest",
    "build/Windows/package/SPLINED.appinstaller.template",
    "build/WEBGui/webgui/backend/splined.py",
)
SHA = re.compile(r"[a-f0-9]{40}")
VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)")


def api(path: str) -> dict:
    return json.loads(subprocess.check_output(["gh", "api", path], timeout=30))


def git_show(commit: str, path: str) -> bytes:
    if not SHA.fullmatch(commit):
        raise ValueError("Invalid Git identity")
    return subprocess.check_output(["git", "show", f"{commit}:{path}"], timeout=20)


def hash_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def positive(value: object) -> bool:
    return type(value) is int and value > 0


def source_identity(env: dict[str, str]) -> tuple[str, str, str, str]:
    source, version, mode, pr = (env[k] for k in (
        "SOURCE_COMMIT", "SOURCE_VERSION", "SOURCE_MODE", "EVALUATION_PR",
    ))
    if not SHA.fullmatch(source) or not VERSION.fullmatch(version):
        raise ValueError("Invalid source identity")
    if mode == "pr-evaluation":
        if not re.fullmatch(r"[1-9][0-9]*", pr):
            raise ValueError("Invalid PR identity")
    elif mode != "dev" or pr:
        raise ValueError("Invalid dev source identity")
    return source, version, mode, pr


def eligible_run(run: dict, env: dict[str, str], path: str, event: str) -> bool:
    return (
        positive(run.get("id")) and positive(run.get("run_attempt"))
        and run.get("status") == "completed" and run.get("conclusion") == "success"
        and run.get("event") == event and run.get("path") == path
        and (run.get("repository") or {}).get("full_name") == env["GITHUB_REPOSITORY"]
        and (run.get("head_repository") or {}).get("full_name") == env["GITHUB_REPOSITORY"]
    )


def verify_bundle(root: Path, kind: str, env: dict[str, str], run_id: int) -> dict:
    source, version, mode, pr = source_identity(env)
    definition = env["GITHUB_SHA"]
    if not SHA.fullmatch(definition) or not positive(run_id):
        raise ValueError("Invalid build provenance")
    manifest_name = WIN_MANIFEST if kind == "windows" else WEB_MANIFEST
    paths = (WIN_MANIFEST, WIN_SUMS, "splined.exe") if kind == "windows" else (
        WEB_MANIFEST, WEB_SUMS, SOURCE, IMAGE,
    )
    found = set()
    for path in root.rglob("*"):
        if path.is_symlink():
            raise ValueError("Artifact contains a symlink")
        if path.is_file():
            found.add(path.relative_to(root).as_posix())
    if found != set(paths):
        raise ValueError("Artifact layout mismatch")
    m = json.loads((root / manifest_name).read_text(encoding="utf-8-sig"))
    if (
        m.get("source_commit") != source or m.get("source_mode") != mode
        or m.get("workflow_definition_commit") != definition
        or m.get("workflow_run_id") != run_id
        or m.get("release_eligible") is not False
    ):
        raise ValueError("Artifact is not the exact non-publishable source/recipe/run")
    if kind == "windows":
        if (
            m.get("schema") != "splined-quick-portable-v1"
            or m.get("source_version") != version or m.get("evaluation_pr") != pr
            or m.get("architecture") != env["WINDOWS_TARGET"]
            or m.get("rust_toolchain") != env["RUST_TOOLCHAIN"]
            or m.get("build_profile") != "release-optimized-incremental-v1"
        ):
            raise ValueError("Incompatible Windows build identity")
        exe = root / "splined.exe"
        digest = hash_file(exe)
        if exe.stat().st_size <= 0 or exe.stat().st_size != m.get("size_bytes") or digest != m.get("sha256"):
            raise ValueError("Executable size/hash mismatch")
        if (root / WIN_SUMS).read_text(encoding="ascii").splitlines() != [f"{digest}  splined.exe"]:
            raise ValueError("Executable checksum manifest mismatch")
    else:
        if (
            m.get("schema") != "splined-release-build-v1" or m.get("kind") != "webgui"
            or m.get("repository") != env["GITHUB_REPOSITORY"]
            or m.get("release_version") != version
            or m.get("evaluation_pr") != (int(pr) if pr else None)
            or m.get("evaluation_only") is not True
            or m.get("candidate_kind") != "workflow20-evaluation"
            or m.get("candidate_run_id") != run_id
            or m.get("validations") != WEB_VALIDATIONS
            or not isinstance(m.get("source_validation"), dict)
            or not SHA.fullmatch(str(m.get("candidate_commit", "")))
            or (m["candidate_commit"] == source) != (mode == "dev")
            or (root / "candidate").exists()
        ):
            raise ValueError("Incompatible WebGUI build identity")
        evidence = m["source_validation"]
        if not positive(evidence.get("run_id")) or evidence.get("kind") not in (
            "executed-in-evaluation", "reused-committed-source-pr-validation",
        ):
            raise ValueError("Invalid validation provenance")
        if evidence["kind"] == "reused-committed-source-pr-validation" and (
            evidence.get("source_commit") != source
            or not positive(evidence.get("run_attempt"))
            or not SHA.fullmatch(str(evidence.get("base_commit", "")))
            or evidence.get("runner_stamp_verified") is not True
        ):
            raise ValueError("Invalid committed-source validation provenance")
        digests = {name: hash_file(root / name) for name in (SOURCE, IMAGE)}
        if any((root / name).stat().st_size <= 0 for name in digests) or m.get("files") != digests:
            raise ValueError("WebGUI artifact hash mismatch")
        if (root / WEB_SUMS).read_text(encoding="ascii").splitlines() != [
            f"{value}  {name}" for name, value in sorted(digests.items())
        ]:
            raise ValueError("WebGUI checksum manifest mismatch")
    return m


def artifact_run_candidates(runs: list[dict], env: dict[str, str]) -> list[dict]:
    _, version, mode, pr = source_identity(env)
    title = f"WF20 PR {pr} v{version}" if mode == "pr-evaluation" else "WF20 origin/dev"
    return [
        r for r in runs if eligible_run(r, env, WF20, "workflow_dispatch")
        and r["id"] != int(env["GITHUB_RUN_ID"])
        and r.get("head_sha") == env["GITHUB_SHA"] and r.get("display_title") == title
    ]


def reuse_artifact(kind: str, root: Path, env: dict[str, str]) -> dict[str, str]:
    if root.exists():
        raise ValueError("Reuse destination must be empty")
    repo = env["GITHUB_REPOSITORY"]
    definition = env["GITHUB_SHA"]
    runs = api(
        f"repos/{repo}/actions/workflows/{WF20.rsplit('/', 1)[-1]}/runs"
        f"?head_sha={definition}&event=workflow_dispatch&status=success&per_page=30"
    )["workflow_runs"]
    for run in artifact_run_candidates(runs, env):
        run_id = run["id"]
        available = api(f"repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100")["artifacts"]
        matches = [a for a in available if a.get("name") == ARTIFACTS[kind] and not a.get("expired")]
        if len(matches) != 1:
            continue
        # Download at most one matching prior package per client. A stale
        # source or corrupt artifact falls through to a normal fresh build.
        with tempfile.TemporaryDirectory(dir=env["RUNNER_TEMP"]) as temp:
            bundle = Path(temp) / "bundle"
            subprocess.run(
                ["gh", "run", "download", str(run_id), "--repo", repo,
                 "--name", ARTIFACTS[kind], "--dir", str(bundle)],
                check=True, timeout=120, capture_output=True,
            )
            m = verify_bundle(bundle, kind, env, run_id)
            if kind == "webgui":
                origin = m.get("original_build_run_id", run_id)
                if not positive(origin):
                    raise ValueError("Invalid original WebGUI build run")
                m.update(
                    original_build_run_id=origin, reused_from_run_id=run_id,
                    workflow_run_id=int(env["GITHUB_RUN_ID"]),
                    workflow_run_attempt=int(env["GITHUB_RUN_ATTEMPT"]),
                    candidate_run_id=int(env["GITHUB_RUN_ID"]),
                    created_at=datetime.now(timezone.utc).isoformat(),
                )
                (bundle / WEB_MANIFEST).write_text(
                    json.dumps(m, indent=2, sort_keys=True) + "\n", encoding="utf-8"
                )
            shutil.move(str(bundle), root)
        return {"reused": "true", "run_id": str(run_id)}
    return {"reused": "false"}


def workflow_contract(contents: bytes) -> tuple[str, ...]:
    text = contents.decode("utf-8").replace("\r\n", "\n")
    header, _, jobs = text.partition("\njobs:\n")
    if not jobs:
        raise ValueError("Validation workflow is missing jobs")
    blocks = {
        match.group(1): match.group(0).strip()
        for match in re.finditer(r"(?ms)^  ([a-z][a-z0-9-]*):\n.*?(?=^  [a-z][a-z0-9-]*:\n|\Z)", jobs)
    }
    # The trusted checkout, permissions, scope, policy and WebGUI recipes
    # must match exactly. Unrelated Windows filters are not WebGUI evidence.
    return (header.strip(), *(blocks[name] for name in (
        "scope", "policy-review", "audio-integrity-webgui", "webgui",
    )))


def verify_runner_stamp(source: str, candidate: str, version: str) -> bool:
    # Import the trusted stamper copied from the workflow definition, never
    # from the selected product PR. Reuse its exact eight version-field
    # transformations, including WebGUI USER_AGENT and all Windows manifests.
    from source_version import stamp_release_version

    if not SHA.fullmatch(source) or not SHA.fullmatch(candidate):
        return False
    parent = subprocess.check_output(
        ["git", "rev-parse", f"{candidate}^"], text=True
    ).strip()
    if parent != source:
        return False
    changed = subprocess.check_output(
        ["git", "diff", "--name-only", source, candidate], text=True
    ).splitlines()
    if len(changed) != len(STAMP_FILES) or set(changed) != set(STAMP_FILES):
        return False
    with tempfile.TemporaryDirectory() as temp:
        root = Path(temp)
        for relative in STAMP_FILES:
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(git_show(source, relative))
        stamp_release_version(root, version)
        return all(
            (root / relative).read_bytes() == git_show(candidate, relative)
            for relative in STAMP_FILES
        )


def matching_pr_run(run: dict, env: dict[str, str], base: str) -> bool:
    source, _, mode, pr = source_identity(env)
    if mode != "pr-evaluation" or run.get("head_sha") != source:
        return False
    return (
        (run.get("repository") or {}).get("full_name") == env["GITHUB_REPOSITORY"]
        and (run.get("head_repository") or {}).get("full_name") == env["GITHUB_REPOSITORY"]
        and run.get("path") == VALIDATE and run.get("event") == "pull_request"
        and any(
            p.get("number") == int(pr)
            and (p.get("head") or {}).get("sha") == source
            and (p.get("base") or {}).get("sha") == base
            and (p.get("base") or {}).get("ref") == "dev"
            for p in run.get("pull_requests") or []
        )
    )


def web_jobs_passed(jobs: list[dict], source: str) -> bool:
    for name, required_steps in WEB_JOBS.items():
        matches = [j for j in jobs if j.get("name") == name]
        if len(matches) != 1:
            return False
        job = matches[0]
        if job.get("head_sha") != source or job.get("conclusion") != "success" or job.get("status") != "completed":
            return False
        for step_name in required_steps:
            steps = [s for s in job.get("steps") or [] if s.get("name") == step_name]
            if len(steps) != 1 or steps[0].get("conclusion") != "success":
                return False
    return True


def reuse_validation(env: dict[str, str]) -> dict[str, str]:
    source, version, mode, pr = source_identity(env)
    if mode != "pr-evaluation":
        return {"reused": "false"}
    repo = env["GITHUB_REPOSITORY"]
    pull = api(f"repos/{repo}/pulls/{pr}")
    if (
        pull.get("state") != "open" or pull.get("head", {}).get("sha") != source
        or pull.get("base", {}).get("ref") != "dev"
        or (pull.get("head", {}).get("repo") or {}).get("full_name") != repo
        or (pull.get("base", {}).get("repo") or {}).get("full_name") != repo
    ):
        return {"reused": "false"}
    base = pull["base"]["sha"]
    trusted = workflow_contract(git_show(env["GITHUB_SHA"], VALIDATE))
    if any(workflow_contract(git_show(commit, VALIDATE)) != trusted for commit in (source, base)):
        return {"reused": "false"}
    if not verify_runner_stamp(source, env["CANDIDATE_COMMIT"], version):
        return {"reused": "false"}
    runs = api(
        f"repos/{repo}/actions/workflows/validate.yml/runs"
        f"?head_sha={source}&event=pull_request&per_page=50"
    )["workflow_runs"]
    matching = [r for r in runs if matching_pr_run(r, env, base)]
    if not matching:
        return {"reused": "false"}
    # A newer failed, cancelled or running validation cannot be bypassed.
    newest = max(matching, key=lambda r: (r.get("created_at", ""), r.get("id", 0)))
    if not eligible_run(newest, env, VALIDATE, "pull_request"):
        return {"reused": "false"}
    run_id, attempt = newest["id"], newest["run_attempt"]
    jobs = api(f"repos/{repo}/actions/runs/{run_id}/attempts/{attempt}/jobs?per_page=100")
    if jobs.get("total_count", 0) != len(jobs.get("jobs", [])) or not web_jobs_passed(jobs["jobs"], source):
        return {"reused": "false"}
    return {"reused": "true", "run_id": str(run_id), "run_attempt": str(attempt), "base_sha": base}


def main() -> None:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    artifact = sub.add_parser("artifact")
    artifact.add_argument("--kind", choices=ARTIFACTS, required=True)
    artifact.add_argument("--root", type=Path, required=True)
    sub.add_parser("validation")
    args = parser.parse_args()
    env = dict(os.environ)
    result = {"reused": "false"}
    if env.get("FORCE_REBUILD") != "true":
        try:
            if args.command == "artifact":
                result = reuse_artifact(args.kind, args.root, env)
            else:
                result = reuse_validation(env)
        except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.SubprocessError) as exc:
            print(f"::warning::Evaluation reuse unavailable ({type(exc).__name__}); executing normal validation/build.")
    print("Evaluation reuse: " + json.dumps(result, sort_keys=True))
    with open(env["GITHUB_OUTPUT"], "a", encoding="utf-8") as out:
        for key, value in result.items():
            out.write(f"{key}={value}\n")


if __name__ == "__main__":
    main()
