"""Create and resolve reference-only Workflow 20 candidate provenance."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

from source_version import release_product_digest


WF20 = ".github/workflows/20-quick-portable-windows.yml"
ARTIFACT = "splined-workflow20-candidate-report"
REPORT = "workflow20-candidate-report.json"
PRODUCT_ROOTS = ("build/Windows", "build/WEBGui", "schema")
SCHEMA = "splined-workflow20-candidate-report-v1"
SHA = re.compile(r"[0-9a-f]{40}")
VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)")
LABEL = re.compile(r"((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))-rc\.([1-9][0-9]*)")
LEGACY_TITLE = re.compile(r"WF20 PR ([1-9][0-9]*) v(" + VERSION.pattern + r")")
TARGET_JOBS = {
    "windows": "Compile and validate standalone splined.exe",
    "webgui": "Validate and package WebGUI pre-release source",
}


def positive(value: object) -> bool:
    return type(value) is int and value > 0


def candidate_id(release_version: str, run_number: int) -> str:
    if not VERSION.fullmatch(release_version) or not positive(run_number):
        raise ValueError("Invalid candidate release identity")
    return f"{release_version}-rc.{run_number}"


def api(path: str) -> dict:
    return json.loads(subprocess.check_output(["gh", "api", path], text=True, timeout=30))


def selected(value: str) -> bool:
    if value not in ("true", "false"):
        raise ValueError("Invalid target selection")
    return value == "true"


def create_report(root: Path, env: dict[str, str]) -> dict:
    run_id = int(env["GITHUB_RUN_ID"])
    run_number = int(env["GITHUB_RUN_NUMBER"])
    run_attempt = int(env["GITHUB_RUN_ATTEMPT"])
    source = env["SOURCE_COMMIT"]
    source_version = env["SOURCE_VERSION"]
    committed_version = env["COMMITTED_SOURCE_VERSION"]
    release_version = env["OFFICIAL_RELEASE_VERSION"]
    definition = env["GITHUB_SHA"]
    mode = env["SOURCE_MODE"]
    pr = env.get("EVALUATION_PR", "")
    label = env["CANDIDATE_ID"]
    windows = selected(env["TARGET_WINDOWS"])
    webgui = selected(env["TARGET_WEBGUI"])
    if not all(positive(value) for value in (run_id, run_number, run_attempt)):
        raise ValueError("Invalid workflow run identity")
    if not all(SHA.fullmatch(value) for value in (source, definition)):
        raise ValueError("Invalid source identity")
    if not all(VERSION.fullmatch(value) for value in (source_version, committed_version, release_version)):
        raise ValueError("Invalid source or release version")
    if label != candidate_id(release_version, run_number):
        raise ValueError("Candidate ID does not match the release reference and run number")
    if not windows and not webgui:
        raise ValueError("Candidate must select at least one client")
    if mode == "pr-evaluation":
        if not re.fullmatch(r"[1-9][0-9]*", pr):
            raise ValueError("Invalid evaluation PR")
    elif mode != "dev" or pr:
        raise ValueError("Invalid source mode")
    results = {
        "windows": env["WINDOWS_RESULT"] if windows else "skipped",
        "webgui": env["WEBGUI_RESULT"] if webgui else "skipped",
    }
    if any(results[target] != "success" for target, enabled in (("windows", windows), ("webgui", webgui)) if enabled):
        raise ValueError("Selected candidate targets did not succeed")
    report = {
        "schema": SCHEMA,
        "candidate_id": label,
        "candidate_kind": "workflow20-evaluation",
        "official_release_version_reference": release_version,
        "evaluation_version": source_version,
        "committed_source_version": committed_version,
        "source_commit": source,
        "source_mode": mode,
        "evaluation_pr": int(pr) if pr else None,
        "product_digest": release_product_digest(root),
        "targets": {"windows": windows, "webgui": webgui},
        "results": results,
        "repository": env["GITHUB_REPOSITORY"],
        "workflow_definition_commit": definition,
        "workflow_run_id": run_id,
        "workflow_run_number": run_number,
        "workflow_run_attempt": run_attempt,
        "release_eligible": False,
        "reference_only": True,
    }
    (root / REPORT).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return report


def eligible_run(run: dict, repository: str) -> bool:
    return (
        positive(run.get("id"))
        and positive(run.get("run_number"))
        and run.get("status") == "completed"
        and run.get("conclusion") == "success"
        and run.get("event") == "workflow_dispatch"
        and run.get("path") == WF20
        and (run.get("repository") or {}).get("full_name") == repository
        and (run.get("head_repository") or {}).get("full_name") == repository
    )


def reference(report: dict, target: str, run: dict) -> dict | None:
    if report["targets"].get(target) is not True or report["results"].get(target) != "success":
        return None
    return {
        "candidate_id": report["candidate_id"],
        "evaluation_version": report["evaluation_version"],
        "source_commit": report["source_commit"],
        "source_mode": report["source_mode"],
        "evaluation_pr": report["evaluation_pr"],
        "workflow_run_id": report["workflow_run_id"],
        "workflow_run_number": report["workflow_run_number"],
        "target": target,
        "reference_only": True,
        "provenance": "workflow20-candidate-report",
        "url": run.get("html_url"),
    }


def validate_report(report: dict, run: dict, repository: str, release_version: str, digest: str) -> None:
    required = {
        "schema": SCHEMA,
        "candidate_kind": "workflow20-evaluation",
        "official_release_version_reference": release_version,
        "repository": repository,
        "workflow_run_id": run["id"],
        "workflow_run_number": run["run_number"],
        "release_eligible": False,
        "reference_only": True,
        "product_digest": digest,
    }
    if any(report.get(key) != value for key, value in required.items()):
        raise ValueError("Candidate report identity mismatch")
    if report.get("candidate_id") != candidate_id(release_version, run["run_number"]):
        raise ValueError("Candidate report label mismatch")
    if not SHA.fullmatch(str(report.get("source_commit", ""))) or not VERSION.fullmatch(str(report.get("evaluation_version", ""))):
        raise ValueError("Candidate report source identity is invalid")
    if not isinstance(report.get("targets"), dict) or not isinstance(report.get("results"), dict):
        raise ValueError("Candidate report target results are invalid")


def download_report(run: dict, repository: str, release_version: str, digest: str) -> dict | None:
    artifacts = api(f"repos/{repository}/actions/runs/{run['id']}/artifacts?per_page=100").get("artifacts", [])
    matches = [item for item in artifacts if item.get("name") == ARTIFACT and not item.get("expired")]
    if len(matches) != 1:
        return None
    with tempfile.TemporaryDirectory() as temp:
        subprocess.run(
            ["gh", "run", "download", str(run["id"]), "--repo", repository,
             "--name", ARTIFACT, "--dir", temp],
            check=True, timeout=60, capture_output=True,
        )
        files = list(Path(temp).rglob(REPORT))
        if len(files) != 1:
            raise ValueError("Candidate report artifact layout mismatch")
        report = json.loads(files[0].read_text(encoding="utf-8-sig"))
    validate_report(report, run, repository, release_version, digest)
    return report


def exact_legacy_product(source: str, main: str) -> bool:
    if not SHA.fullmatch(source) or not SHA.fullmatch(main):
        return False
    ancestor = subprocess.run(
        ["git", "merge-base", "--is-ancestor", source, main],
        timeout=20, capture_output=True,
    )
    if ancestor.returncode != 0:
        return False
    comparison = subprocess.run(
        ["git", "diff", "--quiet", source, main, "--", *PRODUCT_ROOTS],
        timeout=30, capture_output=True,
    )
    return comparison.returncode == 0


def legacy_references(run: dict, repository: str, release_version: str, main: str) -> dict[str, dict]:
    match = LEGACY_TITLE.fullmatch(str(run.get("display_title", "")))
    if not match:
        return {}
    pr_number, evaluation_version = match.groups()
    pull = api(f"repos/{repository}/pulls/{pr_number}")
    source = (pull.get("head") or {}).get("sha", "")
    if (
        not pull.get("merged_at")
        or (pull.get("base") or {}).get("ref") != "dev"
        or ((pull.get("head") or {}).get("repo") or {}).get("full_name") != repository
        or not exact_legacy_product(source, main)
    ):
        return {}
    payload = api(f"repos/{repository}/actions/runs/{run['id']}/jobs?per_page=100")
    jobs = payload.get("jobs", [])
    if payload.get("total_count") != len(jobs):
        return {}
    conclusions = {job.get("name"): job.get("conclusion") for job in jobs}
    result = {}
    for target, name in TARGET_JOBS.items():
        if conclusions.get(name) == "success":
            result[target] = {
                "candidate_id": candidate_id(release_version, run["run_number"]),
                "evaluation_version": evaluation_version,
                "source_commit": source,
                "source_mode": "pr-evaluation",
                "evaluation_pr": int(pr_number),
                "workflow_run_id": run["id"],
                "workflow_run_number": run["run_number"],
                "target": target,
                "reference_only": True,
                "provenance": "legacy-successful-workflow20-reference",
                "url": run.get("html_url"),
            }
    return result


def resolve_references(root: Path, env: dict[str, str], release_version: str, main: str, targets: set[str]) -> dict:
    repository = env["GITHUB_REPOSITORY"]
    digest = release_product_digest(root)
    runs = api(
        f"repos/{repository}/actions/workflows/{WF20.rsplit('/', 1)[-1]}/runs"
        "?event=workflow_dispatch&status=success&per_page=100"
    ).get("workflow_runs", [])
    resolved: dict[str, dict | None] = {target: None for target in ("windows", "webgui")}
    for run in runs:
        if not eligible_run(run, repository):
            continue
        candidate = None
        try:
            candidate = download_report(run, repository, release_version, digest)
        except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError, subprocess.SubprocessError):
            candidate = None
        if candidate is not None:
            for target in targets:
                if resolved[target] is None:
                    resolved[target] = reference(candidate, target, run)
        else:
            try:
                legacy = legacy_references(run, repository, release_version, main)
            except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
                legacy = {}
            for target in targets:
                if resolved[target] is None and target in legacy:
                    resolved[target] = legacy[target]
        if all(resolved[target] is not None for target in targets):
            break
    return {"reference_only": True, "windows": resolved["windows"], "webgui": resolved["webgui"]}


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    write = subparsers.add_parser("write")
    write.add_argument("--root", type=Path, default=Path.cwd())
    resolve = subparsers.add_parser("resolve")
    resolve.add_argument("--root", type=Path, default=Path.cwd())
    resolve.add_argument("--release-version", required=True)
    resolve.add_argument("--main-sha", required=True)
    resolve.add_argument("--target", choices=("windows", "webgui"), action="append", required=True)
    resolve.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "write":
        create_report(args.root, dict(os.environ))
    else:
        try:
            result = resolve_references(args.root, dict(os.environ), args.release_version, args.main_sha, set(args.target))
        except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError, subprocess.SubprocessError) as exc:
            print(f"::warning::Workflow 20 candidate reference unavailable ({type(exc).__name__}).")
            result = {"reference_only": True, "windows": None, "webgui": None}
        args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
