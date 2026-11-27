"""Create a local-deployment-compatible, non-publishable WebGUI evaluation manifest.

Keep the existing ZIP member paths and build-manifest schema for development
deployment consumers. Unlike a release build, there is deliberately no
approved official-release candidate report; publication validation cannot accept
this artifact. candidate_run_id identifies the actual evaluation source
selection run (Workflow 20), not an official release preparation or approval.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re


SOURCE = "release-assets/splined-webgui-python.tar.gz"
IMAGE = "release-assets/splined-webgui-container-image.tar.gz"
MANIFEST = "webgui-build-manifest.json"
CHECKSUMS = "SHA256SUMS-WEBGUI-BUILD.txt"
VALIDATIONS = [
    "python-postgresql-node",
    "container-health-database-smoke",
    "exact-smoke-tested-image-retained",
    "python-source-archive",
]


def digest(path: Path) -> str:
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def create_manifest(root: Path, env: dict[str, str]) -> dict:
    run = int(env["GITHUB_RUN_ID"])
    attempt = int(env["GITHUB_RUN_ATTEMPT"])
    source = env["SOURCE_COMMIT"]
    candidate = env["CANDIDATE_COMMIT"]
    version = env["SOURCE_VERSION"]
    mode = env["SOURCE_MODE"]
    pr = env.get("EVALUATION_PR", "")
    definition = env["GITHUB_SHA"]
    if run <= 0 or attempt <= 0:
        raise ValueError("Invalid evaluation run identity")
    if not all(re.fullmatch(r"[0-9a-f]{40}", s) for s in (source, candidate, definition)):
        raise ValueError("Invalid source or workflow commit")
    if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version):
        raise ValueError("Invalid evaluation version")
    if mode == "pr-evaluation":
        if not re.fullmatch(r"[1-9][0-9]*", pr) or candidate == source:
            raise ValueError("Expected stamped evaluation commit and PR identity")
    elif mode != "dev" or pr or candidate != source:
        raise ValueError("Invalid dev evaluation source identity")
    if (root / "candidate").exists():
        raise ValueError("Evaluation artifact must not contain a release candidate bundle")
    files = {}
    for name in (SOURCE, IMAGE):
        path = root / name
        if not path.is_file() or path.stat().st_size <= 0:
            raise ValueError(f"Missing or empty evaluation asset: {name}")
        files[name] = digest(path)
    manifest = {
        "schema": "splined-release-build-v1",
        "kind": "webgui",
        "repository": env["GITHUB_REPOSITORY"],
        "workflow_run_id": run,
        "workflow_run_attempt": attempt,
        "workflow_definition_commit": definition,
        "candidate_run_id": run,
        "candidate_commit": candidate,
        "source_commit": source,
        "release_version": version,
        "source_mode": mode,
        "evaluation_pr": int(pr) if pr else None,
        "compile_mode": "PARTIAL UPDATE",
        "validations": VALIDATIONS,
        "files": files,
        "created_at": datetime.now(timezone.utc).isoformat(),
        "release_eligible": False,
        "evaluation_only": True,
        "candidate_kind": "workflow20-evaluation",
    }
    if env.get("SOURCE_VALIDATION_RUN_ID"):
        manifest["source_validation"] = {
            "kind": "reused-committed-source-pr-validation",
            "run_id": int(env["SOURCE_VALIDATION_RUN_ID"]),
            "run_attempt": int(env["SOURCE_VALIDATION_RUN_ATTEMPT"]),
            "source_commit": source,
            "base_commit": env["SOURCE_VALIDATION_BASE_SHA"],
            "runner_stamp_verified": True,
        }
    else:
        manifest["source_validation"] = {"kind": "executed-in-evaluation", "run_id": run}
    (root / MANIFEST).write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (root / CHECKSUMS).write_text(
        "".join(f"{value}  {name}\n" for name, value in sorted(files.items())),
        encoding="ascii",
    )
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    create_manifest(args.root, dict(os.environ))


if __name__ == "__main__":
    main()
