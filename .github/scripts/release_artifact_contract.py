"""Create and verify immutable cross-workflow release artifact manifests."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from datetime import datetime, timezone
from pathlib import Path


SHA_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
VERSION_RE = re.compile(r"^\d+\.\d+\.\d+$")


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _load(path: Path) -> dict[str, object]:
    value = json.loads(path.read_text(encoding="utf-8-sig"))
    if not isinstance(value, dict):
        raise ValueError(f"Manifest must be an object: {path}")
    return value


def _require_text(manifest: dict[str, object], key: str, pattern: re.Pattern[str]) -> str:
    value = manifest.get(key)
    if not isinstance(value, str) or not pattern.fullmatch(value):
        raise ValueError(f"Invalid {key}: {value!r}")
    return value


def verify_candidate(root: Path, expected_run_id: int | None = None) -> dict[str, object]:
    root = root.resolve()
    manifest_path = root / "release-candidate.json"
    bundle_path = root / "release-source.bundle"
    validation_path = root / "source-validation.json"
    checksum_path = root / "SHA256SUMS-CANDIDATE.txt"
    manifest = _load(manifest_path)
    if manifest.get("schema") != "splined-release-candidate-v1":
        raise ValueError("Unsupported release candidate manifest schema")
    _require_text(manifest, "main_commit", COMMIT_RE)
    _require_text(manifest, "source_commit", COMMIT_RE)
    _require_text(manifest, "candidate_commit", COMMIT_RE)
    _require_text(manifest, "source_version", VERSION_RE)
    _require_text(manifest, "release_version", VERSION_RE)
    _require_text(manifest, "validation_commit", COMMIT_RE)
    bundle_sha = _require_text(manifest, "bundle_sha256", SHA_RE)
    run_id = manifest.get("workflow_run_id")
    if not isinstance(run_id, int) or run_id <= 0:
        raise ValueError(f"Invalid workflow_run_id: {run_id!r}")
    validation_run_id = manifest.get("validation_run_id")
    if not isinstance(validation_run_id, int) or validation_run_id <= 0:
        raise ValueError(f"Invalid validation_run_id: {validation_run_id!r}")
    if expected_run_id is not None and run_id != expected_run_id:
        raise ValueError(
            f"Candidate run mismatch: manifest={run_id}, requested={expected_run_id}"
        )
    if not bundle_path.is_file() or _sha256(bundle_path) != bundle_sha:
        raise ValueError("Release source bundle checksum mismatch")
    validation = _load(validation_path)
    if not isinstance(validation.get("jobs"), list) or not validation["jobs"]:
        raise ValueError("Required validation provenance has no jobs")
    for job in validation["jobs"]:
        if not isinstance(job, dict):
            raise ValueError("Required validation provenance contains an invalid job")
        if job.get("run_id") != validation_run_id:
            raise ValueError("Required validation job does not belong to the recorded run")
        if job.get("head_sha") != manifest["validation_commit"]:
            raise ValueError("Required validation job does not match the recorded commit")
        if job.get("status") != "completed" or job.get("conclusion") not in {
            "success",
            "skipped",
        }:
            raise ValueError(f"Required validation job was not successful: {job.get('name')!r}")
    expected_files = (
        "release-source.bundle",
        "release-candidate.json",
        "source-validation.json",
    )
    expected_ledger = "".join(
        f"{_sha256(root / name)}  {name}\n" for name in expected_files
    )
    if checksum_path.read_text(encoding="ascii") != expected_ledger:
        raise ValueError("Candidate checksum ledger mismatch")
    return manifest


def _parse_pairs(values: list[str]) -> dict[str, str]:
    result: dict[str, str] = {}
    for value in values:
        key, separator, item = value.partition("=")
        if not separator or not key or key in result:
            raise ValueError(f"Expected one unique KEY=VALUE pair: {value!r}")
        result[key] = item
    return result


def create_build(args: argparse.Namespace) -> None:
    root = args.root.resolve()
    candidate = verify_candidate(args.candidate_root.resolve(), args.candidate_run_id)
    manifest_path = root / f"{args.kind}-build-manifest.json"
    checksum_path = root / f"SHA256SUMS-{args.kind.upper()}-BUILD.txt"
    excluded = {manifest_path.resolve(), checksum_path.resolve()}
    files: dict[str, str] = {}
    for path in sorted(item for item in root.rglob("*") if item.is_file()):
        if path.resolve() in excluded:
            continue
        files[path.relative_to(root).as_posix()] = _sha256(path)
    if not files:
        raise ValueError("A build manifest requires at least one artifact file")
    manifest = {
        "schema": "splined-release-build-v1",
        "kind": args.kind,
        "repository": os.environ.get("GITHUB_REPOSITORY", "local"),
        "workflow_run_id": args.run_id,
        "workflow_run_attempt": int(os.environ.get("GITHUB_RUN_ATTEMPT", "1")),
        "candidate_run_id": args.candidate_run_id,
        "candidate_commit": candidate["candidate_commit"],
        "source_commit": candidate["source_commit"],
        "release_version": candidate["release_version"],
        "compile_mode": args.compile_mode,
        "validations": args.validation,
        "metrics": _parse_pairs(args.metric),
        "files": files,
        "created_at": datetime.now(timezone.utc).isoformat(),
    }
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    checksum_path.write_text(
        "".join(
            f"{digest}  {relative}\n"
            for relative, digest in sorted(files.items())
        ),
        encoding="ascii",
    )


def verify_build(root: Path, kind: str, expected_run_id: int | None = None) -> dict[str, object]:
    root = root.resolve()
    manifest_path = root / f"{kind}-build-manifest.json"
    checksum_path = root / f"SHA256SUMS-{kind.upper()}-BUILD.txt"
    manifest = _load(manifest_path)
    if manifest.get("schema") != "splined-release-build-v1" or manifest.get("kind") != kind:
        raise ValueError(f"Unsupported {kind} build manifest")
    _require_text(manifest, "candidate_commit", COMMIT_RE)
    _require_text(manifest, "source_commit", COMMIT_RE)
    _require_text(manifest, "release_version", VERSION_RE)
    run_id = manifest.get("workflow_run_id")
    if not isinstance(run_id, int) or run_id <= 0:
        raise ValueError(f"Invalid workflow_run_id: {run_id!r}")
    if expected_run_id is not None and run_id != expected_run_id:
        raise ValueError(f"Build run mismatch: manifest={run_id}, requested={expected_run_id}")
    candidate_run_id = manifest.get("candidate_run_id")
    if not isinstance(candidate_run_id, int) or candidate_run_id <= 0:
        raise ValueError(f"Invalid candidate_run_id: {candidate_run_id!r}")
    candidate = verify_candidate(root / "candidate", candidate_run_id)
    for key in ("candidate_commit", "source_commit", "release_version"):
        if manifest.get(key) != candidate.get(key):
            raise ValueError(f"Build and embedded candidate disagree on {key}")
    files = manifest.get("files")
    if not isinstance(files, dict) or not files:
        raise ValueError("Build manifest has no files")
    validations = manifest.get("validations")
    required_validations = {
        "windows": {
            "portable-identity-x64-native-smoke",
            "unsigned-msix-package-contract",
            "isolated-development-package",
            "offline-local-store-review",
        },
        "webgui": {
            "python-postgresql-node",
            "container-health-database-smoke",
            "exact-smoke-tested-image-retained",
            "python-source-archive",
        },
    }[kind]
    if not isinstance(validations, list) or not required_validations.issubset(validations):
        raise ValueError(f"Missing required {kind} validation provenance")
    expected_lines = []
    for relative, digest in sorted(files.items()):
        if not isinstance(relative, str) or not isinstance(digest, str) or not SHA_RE.fullmatch(digest):
            raise ValueError(f"Invalid artifact file entry: {relative!r}={digest!r}")
        relative_path = Path(relative)
        if relative_path.is_absolute() or ".." in relative_path.parts:
            raise ValueError(f"Unsafe artifact path: {relative!r}")
        path = root / relative_path
        try:
            path.resolve().relative_to(root)
        except ValueError as exc:
            raise ValueError(f"Unsafe artifact path: {relative!r}") from exc
        if not path.is_file() or _sha256(path) != digest:
            raise ValueError(f"Artifact checksum mismatch: {relative}")
        expected_lines.append(f"{digest}  {relative}\n")
    if checksum_path.read_text(encoding="ascii") != "".join(expected_lines):
        raise ValueError(f"Checksum ledger does not match {kind} build manifest")
    return manifest


def _write_outputs(manifest: dict[str, object], output_path: Path | None) -> None:
    if output_path is None:
        print(json.dumps(manifest, sort_keys=True))
        return
    keys = (
        "source_commit",
        "main_commit",
        "candidate_commit",
        "source_version",
        "release_version",
        "recommended_compile_mode",
        "target",
        "workflow_run_id",
        "validation_commit",
        "validation_run_id",
        "candidate_run_id",
        "compile_mode",
    )
    with output_path.open("a", encoding="utf-8") as handle:
        for key in keys:
            if key in manifest:
                value = manifest[key]
                if isinstance(value, bool):
                    value = str(value).lower()
                handle.write(f"{key}={value}\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    candidate = subparsers.add_parser("verify-candidate")
    candidate.add_argument("root", type=Path)
    candidate.add_argument("--expected-run-id", type=int)
    candidate.add_argument("--github-output", type=Path)

    create = subparsers.add_parser("create-build")
    create.add_argument("--kind", choices=("windows", "webgui"), required=True)
    create.add_argument("--root", type=Path, required=True)
    create.add_argument("--candidate-root", type=Path, required=True)
    create.add_argument("--candidate-run-id", type=int, required=True)
    create.add_argument("--run-id", type=int, required=True)
    create.add_argument("--compile-mode", choices=("PARTIAL UPDATE", "FULL SOURCE"), required=True)
    create.add_argument("--validation", action="append", default=[])
    create.add_argument("--metric", action="append", default=[])

    build = subparsers.add_parser("verify-build")
    build.add_argument("--kind", choices=("windows", "webgui"), required=True)
    build.add_argument("root", type=Path)
    build.add_argument("--expected-run-id", type=int)
    build.add_argument("--github-output", type=Path)

    args = parser.parse_args()
    if args.command == "verify-candidate":
        manifest = verify_candidate(args.root, args.expected_run_id)
        _write_outputs(manifest, args.github_output)
    elif args.command == "create-build":
        create_build(args)
    else:
        manifest = verify_build(args.root, args.kind, args.expected_run_id)
        _write_outputs(manifest, args.github_output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
